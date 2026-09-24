# C15 Root-Access & Firmware-Upgrade Research

Investigation notes for regaining root/administrative access to our own Huidu
C15 LED-controller units and understanding their firmware-update mechanism.
All work here is on hardware we own (units BE371 / BF096).

- Units: **BE371** `192.168.1.153` (MAC 28:32:fd:be:37:10), **BF096** `192.168.1.244` (MAC 28:32:fd:bf:09:60)
- Both stock **BoxPlayer 7.11.18.0**, FPGA 6.22.70.0, SoC Rockchip **PX30** (aarch64, Linux)
- Open TCP ports (both): **22** (OpenSSH_7.5), **9527** (BoxPlayer SDK), **9528** (BoxUpgrade). No adb/telnet/http.
- sshd: `PermitRootLogin yes`, `AuthorizedKeysFile .ssh/authorized_keys` (→ `/root/.ssh/authorized_keys`), StrictModes default (on). Root password unknown & **non-default** (60-candidate sweep, all clean rejects).

## Goal

Regain a root shell on both units. Password login is not available (non-default
password). Key-based login needs a key installed under `/root/.ssh`. The two
network surfaces (9528 upgrade, 9527 SDK config) are the only levers.

---

## 1. Firmware-upgrade mechanism (port 9528 / BoxUpgrade)

Reference binary: `.../PX30_BoxPlayerD15.tar.d/System/BoxUpgrade` (aarch64 ELF).
Decompiled sibling: `libBoxUpgrade.so` (`ghidra_work/teamA/unstripped.c`).

Wire framing: `[u16 LE total_len(incl. 4-byte header)][u16 LE cmd][payload]`.

Handshake (9528): ConnectReq `0x000b`(u32 `0x01000007`)→`0x000c`; ClientInfoReq
`0x0410`(csv\0)→`0x0411`; NullCap `0x0053`→`0x0054`; Cap `0x040a`→`0x040b`.

File transfer: OpenFile `0x0017`(`path\0`+`u64 size`)→`0x0018`; FileContent
`0x0019`(≤9212 B chunks, acked `0x001a`)→; CloseFile `0x001b`(**empty payload**,
total_len must == 4)→`0x001c`.

Apply: UpgradeCMD `0x0055` mode=3 stores the decompress command; mode=2 →
`HUpgrade::DisposeUpgradeShellAsk` runs cmd1 then `vfork`+`system(cmd2)`.

### Reconstructed cmd1 / cmd2 (from BoxUpgrade rodata templates)

```
cmd1 = echo "0" > <statusfile>; cd <firewareDir>; rm -rf UpgradeDir; mkdir UpgradeDir;
       killall -1 BoxDaemon; tar zxvf <archive> -C <firewareDir>/UpgradeDir;
       chmod u+x <firewareDir>/UpgradeDir/<script>; dos2unix <firewareDir>/UpgradeDir/<script>;
cmd2 = <firewareDir>/UpgradeDir/<script>        (template "%s/UpgradeDir/%s")
```

- The "status 0" the client observes is cmd1's own `echo "0" > <statusfile>`, **not**
  proof our script ran.
- `DisposeUpgradeShellAsk` runs `system(cmd2)` in the vfork child **unconditionally**;
  the `QByteArray::indexOf` on the script only gates a post-run `ReloadFPGAParam()`.
- The real 7.11 image and our access image both use `<Script>upgrade.sh</Script>`
  with `upgrade.sh` at the tar top level → correct (the untar `-C` target is UpgradeDir).

### Destination path is NOT client-controlled

`RecvOpenFileAsk` stores the requested path, but `SendOpenFileAnswer` **overwrites**
it: dest = `GetFirewareDir()` + `/Box.tar.gz` (fixed name), written to a temp
(`+unfinish` suffix), and `SendCloseFileAnswer` renames temp→dest then runs a fixed
`system()` (≈`sync`). **Uploads always land at `<firewareDir>/Box.tar.gz`** regardless
of the OpenFile path — so there is **no arbitrary-file-write primitive** here.
(Confirmed empirically with a hand-rolled 9528 client: writing to
`/root/.ssh/authorized_keys` returned code 0 / committed, but SSH key auth failed
because the bytes went to `firewareDir/Box.tar.gz`.)

### `GetFirewareDir(size)` (decomp ~line 14369)

Picks DIR_A if `declaredSize ≤ freeSpace(DIR_A)*0x80000`, else DIR_B
(`/mnt/usb_storage`), else (if `(free+folder)*0x80000 ≥ size`) clears DIR_A and uses
it, else `""`. Binary has **no `/tmp` string** — `/tmp/Box.tar.gz` is purely the
client's chosen OpenFile path. Log strings: `fireware dir: %s.`, `not enought space!`.

### Everything runs as root (no privilege separation)

The boot chain (`BoxPlayerInit.sh` → `BoxDaemon`, `run.sh` → `runBoxPlayer.sh`/
`runBoxSDK.sh`, `start-ssh`, `cn.huidu.device.*`) performs root-only ops
(`echo > /proc/sys`, `mount`, `sysctl -w`, `killall`) with **no privilege drop**
anywhere (no `su`/`setuidgid`/`start-stop-daemon --chuid`/`nobody`). App home is
`/root/Box`. So BoxPlayer/BoxDaemon/BoxUpgrade/sshd all run as **root** — replacing
BoxPlayer with our own agent, or getting the upgrade script to run, yields root.
Privilege is not the blocker.

### The open puzzle (why cmd2 no-ops on running-7.11)

A small script-only image never runs cmd2 on a running-7.11 box (no reboot, root pw
never set) — via **both** our client and genuine HDPlayer. Yet the file **write
succeeds** (`code=0`, committed), proving `GetFirewareDir` returns a valid writable
dir for a small file. The only confirmed success ever was a big image on a **fresh
7.4** box.

**Deep trace verdict (all mechanical theories DISPROVEN):**
- `GetFirewareDir` is called **once** (in `SendOpenFileAnswer`), cached to `this+0x2c`
  (dir) and `this+0x1c` (`<fire>/Box.tar.gz`); mode-2/`DisposeUpgradeShellAsk` reuse
  the cache — tar source, `-C` dir, and cmd2 all derive from the **same** dir the file
  was written to. No open-vs-run mismatch; the declared size only steers that one
  selection, which succeeded (code 0).
- **No** version/limit/state gate on the mode-2 path (the USB path *is* version-gated;
  the network path is not). cmd2 (`this+0x44`) is built **unconditionally**.
- The observed "status 0" is `cmd1`'s own `echo "0" > /root/upgrade.status`, which
  **proves `DisposeUpgradeShellAsk` ran and `system(cmd1)` executed**.
- `killall -1 BoxDaemon` is **benign** — SIGHUP to *BoxDaemon* (a separate process that
  handles it; the updater is *BoxUpgrade*); the working 7.4→7.11 flash also carried it.
- Archive layout correct: payload gzip @ offset 287, single top-level `upgrade.sh`;
  reconstructed cmd1/cmd2 paths all agree at `<fire>/UpgradeDir/upgrade.sh`.
- `SendCloseFileAnswer`'s post-rename `system()` is a fixed no-arg (sync-class), not an
  `rm` — archive isn't deleted before untar.

**Residual cause:** a **runtime property of the already-running-7.11 box** (BoxPlayer/
BoxSDK live) that static artifacts can't reveal — something between `cmd1`'s `echo` and
the script taking effect, present on a live box and absent on a fresh idle one. The only
ground truth that discriminates it is `/root/Box/config/upgradeLog.ini`
(`cmd1[%s], cmd2[%s].`, `fireware dir: %s.`), unreadable without a shell.

**Highest-value untried lever:** **reboot then flash within the fresh boot window** —
reproduces the only condition under which cmd2 has ever run (fresh, idle box). A
one-token control test (strip `killall -1 BoxDaemon` from `<Decompress>`) is predicted
NOT to help.

---

## 2. SDK config surface (port 9527) & config→shell sinks

The SDK config commands are handled by `libBoxIOServices.so` (`old::HM*` classes),
network by `HNetWorkManager` in `libSDKServices.so`, with shell/config templates in
`libCore.so`. Three OS sinks: `sdk::SystemCmd`, `sdk::SystemCall`, libc `system()`.
`##token` (QString::replace) and Qt `%1` substitution do **no** shell escaping, and
neither does writing a value verbatim into a `.conf` file. **XML-escaping does not
stop shell injection** (device un-escapes before substitution).

Config values that reach a **root shell** (from static audit):

| Field (SDK) | Shell template | Trigger | Notes |
|---|---|---|---|
| ntp server | `ntpdate <server>` via SystemCmd | **cloud only** (`CheckMulit`); the on-box periodic sync is `sdk::HNtpdate`, a **native NTP client** (no shell) | Dead end on LAN-only units |
| eth0 dns/ip/gateway | `ifconfig ##ip …; route add default gw ##gateway; echo "nameserver ##dns" > /etc/resolv.conf` (one `system()`) | apply is **immediate & unconditional** on `SetEth0Info` (`HEthernet::SetAddrInfo`→`HNetTools::SetNetAddress`) | ❌ **DEAD END — not injectable.** `sdk::HnAddrInfo::Convert` runs `Inet_aton()` on ip/netmask/gateway/dns and stores 4-byte ints; the shell string is rebuilt via `Inet_ntoa`, so every value is a clean dotted-quad. **Confirmed empirically** on BE371: both a `dns=…$()…` and a `gateway=…;cmd;` payload were accepted but did NOT execute (root pw not set). (An earlier audit that called this injectable missed the `Inet_aton` numericisation.) |
| wifi ssid/psk | written unescaped into `wpa_supplicant.conf` | wifi bring-up | config-directive injection, not direct shell; ethernet box |
| set_time_info | `date -s "%s"; hwclock -w` | on set | **SAFE** — value round-trips through QDateTime (good sanitisation model) |

Safe (not shell sinks): device name, box_hw/fpga config, data source / dynamic
data, license, admin password, brightness/rotation/volume.

### Client SDK-schema bugs found & fixed (this repo)

Our `hdplayer` client was sending **wrong XML bodies** for two config setters; the
device rejected both with `kParseXmlFailed`. Correct schemas (from the device's own
`Gen*`/`Parse*` serializers, round-trip-confirmed against `Get*`):

- **NTP** (`SetNtpServerAddr`): `<server><item host="H" port="123"/></server>`
  (was `<ntp server="X"/>`).
- **eth0** (`SetEth0Info`): per-field elements with `addr` attributes —
  `<enable value="true"/><dhcp auto="0|1"/><ip addr="…"/><netmask addr="…"/><gateway addr="…"/><dns addr="…"/>`
  (was `<eth0 dhcp=… ip=… mask=… gateway=… dns=…/>`; note `netmask` not `mask`, and
  `dhcp auto="1|0"` not `dhcp="true|false"`).

Both fixes are in `hdplayer-client/src/command.rs` with unit tests, and all field
values now pass through `xml::xml_escape`. The corrected commands are **accepted**
by the live device.

---

## 3. Status of each access route

| Route | Result |
|---|---|
| SSH default/vendor passwords | ❌ Dead — 60 candidates/box, all clean rejects |
| Arbitrary root file-write via 9528 | ❌ Dead — uploads always land at `firewareDir/Box.tar.gz` |
| Read `upgradeLog.ini` via SDK/USB | ❌ No route (only `*.log` readback exists; wrong file). USB export = config clone (`BoxPlayer.bin`, hwsetting, `device.key`), no logs |
| adb / telnet / other services | ❌ Not present (only 22/9527/9528 open) |
| NTP value → root shell | ❌ Dead on LAN units (native NTP client; shell path is cloud-only) |
| eth0 field → root shell | ❌ Dead — values `Inet_aton`-numericised before the shell command; tested dns + gateway payloads on hardware, neither executed |
| wifi ssid/psk / pppoe apn → conf file | ⚠️ Genuine *verbatim* writes into `wpa_supplicant.conf` / pppd chat-script, but into config FILES, not a shell (launch commands are fixed literals); ethernet unit, so these paths may be inactive. pppoe user/password are never consumed by the firmware. Untested |
| 9528 script upgrade | ⏳ Mechanism fully mapped; cmd2 no-op on running-7.11 unexplained despite valid write (under investigation) |

Positive: the SDK config channel is **unblocked** (device now accepts our NTP and
eth0 commands), and the file-write / upgrade mechanism is fully mapped.

---

## 4. Open questions (being investigated)

1. ✅ *Answered.* eth0 apply is immediate & unconditional, but the values are
   `Inet_aton`-numericised → not injectable (see §2/§3).
2. ✅ *Answered.* The audit's "most reliable sink" (eth0) is numericised; the only
   remaining verbatim writes are into **config files** (wifi/pppoe), not a shell.
3. **Upgrade cmd2 non-execution** (still open): why cmd2 no-ops on running-7.11
   despite a valid write — different `GetFirewareDir` size arg at open vs
   mode3/mode2? a state gate? `killall -1 BoxDaemon` interfering? path mismatch?
4. **wifi/pppoe conf-file injection** (untested): whether the `wpa_supplicant.conf`
   ssid/psk write or the pppd chat-script `$apn` write can be leveraged on an
   ethernet unit, and whether their bring-up ever runs.

## 4a. Remote reboot is NOT available (blocks the fresh-window lever)

We hoped to reboot via SDK to reproduce the fresh-boot window (the only state where
the upgrade script has ever run). Result: **the C15 does not expose reboot over the
local SDK.** On the 9527/9528 path, `Reboot` (the node-huidu-sdk method name, for a
*different* controller model), `DeviceReboot` (this firmware's `OldSDK/MReboot.cpp`
label), and even the discovery method `GetAllMethodNames` all return
**`kUnsupportMethod`**. The reboot code exists in the binary but is wired only to the
cloud/OMS path (`HPlatformService::DecodeReboot`), which is inactive on LAN-only units.

Correction to an earlier note: our SDK `reboot` has **never actually rebooted** the box
— every call errored; a prior "box came back at t+32s" reading was a poll artifact (the
box was up throughout). **Consequence: the reboot-then-flash "fresh window" experiment
cannot be performed remotely** — it needs a physical power-cycle.

Third-party SDK review (alparslanahmed/huidu-led, KryQ/node-huidu-sdk): both are
content-push clients covering a **subset** of the command surface we already mapped from
the firmware; neither offers an auth bypass, password-set, shell, factory-reset, or a
different upgrade/root path. `huidu-led` targets an older HD2020/Gen6 controller, not the
PX30 C15. Net new value: the `<in ... delay="N">` reboot attribute form (which the C15
rejects anyway) and confirmation we're ahead of these implementations.

## 4b. Bottom line

Every **remote software** path to root on an already-running 7.11 C15 is now closed:
SSH defaults, arbitrary-write, NTP/eth0 injection, SDK reboot — all dead; the upgrade
script uploads + runs `cmd1` but its script step no-ops on a live box, and we cannot
remotely reboot to hit the fresh window. Remaining realistic routes require **physical
access**: (a) power-cycle + flash the small access image within the fresh boot window;
(b) serial console (UART) for a root shell / boot interruption; (c) USB/loader recovery
to flash the `px30-custom-os` image. All three need hands on the unit.

## 5. Tooling produced

- `hdplayer-client/src/command.rs` — corrected `set_ntp_server` and `set_eth0_info`
  schemas (+ tests).
- Hand-rolled 9528 file-transfer client (session scratchpad `putfile.py`) — confirms
  the fixed-destination write behaviour.
- SSH default-password sweep script (session scratchpad `ssh_defaults.py`).
- `hdplayer sdk-raw <method> [body]` — sends an arbitrary SDK method and prints the raw
  response; used to probe method support (`GetAllMethodNames` → `kUnsupportMethod`, etc.).
  Plus `xml::sdk_request_attr` for extra `<in>` attributes and `client.reboot_after`.

> Note: large packet captures (`*.pcapng`, hundreds of MB) and `*.log` files from
> this work are intentionally **not** committed.
