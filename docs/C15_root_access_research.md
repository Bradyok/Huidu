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

### The open puzzle (why cmd2 no-ops on running-7.11)

A small script-only image never runs cmd2 on a running-7.11 box (no reboot, root pw
never set) — via **both** our client and genuine HDPlayer. Yet the file **write
succeeds** (`code=0`, committed), proving `GetFirewareDir` returns a valid writable
dir for a small file. The only confirmed success ever was a big image on a **fresh
7.4** box. Root cause still under investigation (see open questions). The device
records the exact `cmd1[%s], cmd2[%s]` and `fireware dir` in
`/root/Box/config/upgradeLog.ini`, which we can't read without a shell.

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
| eth0 dns | `echo "nameserver ##dns" > /etc/resolv.conf` | network apply | appears to only run on change / restart — see open questions |
| eth0 ip/gateway | `ifconfig ##ip …` / `route add default gw ##gateway` | network apply | changing these risks connectivity |
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
| eth0 DNS value → root shell | ⏳ Sink exists but did not fire on a same-IP static set — trigger under investigation |
| 9528 script upgrade | ⏳ Mechanism fully mapped; cmd2 no-op on running-7.11 unexplained despite valid write |

Positive: the SDK config channel is **unblocked** (device now accepts our NTP and
eth0 commands), and the file-write / upgrade mechanism is fully mapped.

---

## 4. Open questions (being investigated)

1. **eth0 apply trigger**: does the resolv.conf/ifconfig/route apply only run on a
   changed value or dhcp↔static toggle? How to force it. Is the dns value
   shell-evaluated in place (`$()`/`;`)?
2. **Best-triggered config sink**: which setter runs a root shell command
   immediately & unconditionally on set.
3. **Upgrade cmd2 non-execution**: why cmd2 no-ops on running-7.11 despite a valid
   write — different `GetFirewareDir` size arg at open vs mode3/mode2? a state gate?
   `killall -1 BoxDaemon` interfering? path mismatch?

## 5. Tooling produced

- `hdplayer-client/src/command.rs` — corrected `set_ntp_server` and `set_eth0_info`
  schemas (+ tests).
- Hand-rolled 9528 file-transfer client (session scratchpad `putfile.py`) — confirms
  the fixed-destination write behaviour.
- SSH default-password sweep script (session scratchpad `ssh_defaults.py`).

> Note: large packet captures (`*.pcapng`, hundreds of MB) and `*.log` files from
> this work are intentionally **not** committed.
