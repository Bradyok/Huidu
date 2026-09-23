# Huidu PX30 — Session Handoff

_Last updated: 2026-09-23. Written for a fresh session to pick up cleanly._

This is the working state, the **one hard blocker**, everything ported so far, and
the concrete next steps. Read the "BLOCKER" section first — it gates hardware work.

---

## 0. TL;DR

- **Goal:** run our own OS + player on Huidu PX30 C-series LED controllers
  (C15/C35/C36), gain root on stock units, dump a golden image, and **recover a
  unit we likely bricked** with earlier upgrade experiments.
- **Live hardware right now:** two healthy **C15** units, firmware **7.4.61.0**:
  - `192.168.1.153` — ID `C15-C21-BE371`, screen **128×128**
  - `192.168.1.244` — ID `C15-C21-BF096`
  - Both: telnet(23)+9527+9528 **open**, SSH(22) **closed**, **stock root password
    unknown**. DHCP — IPs change across reboots; rediscover with the tool.
  - A third (older) unit we'd worked on is **offline / probably bricked**. Very
    likely recoverable via **Rockchip maskrom USB** (BootROM in silicon).
- **The blocker:** our upgrade client reaches "status 0 (complete)" but the
  **upgrade script never executes on the device** → we can't set our password →
  no shell. This is a real protocol bug (file not flushed before `tar`). Details
  below. **This is the #1 thing to solve.**
- **Software side is in great shape:** the player (`boxplayer`), the ttyS1 sender
  daemon + FPGA loader (`huidu-sender`), the Buildroot image (`br2-huidu`), the
  clone/dump tooling, and a big batch of SDK method ports are all done + tested.

---

## 1. THE BLOCKER — upgrade script doesn't execute (SOLVE THIS FIRST)

### What we're trying to do
Get root on a stock unit by pushing a small "firmware upgrade" package over the
upgrade channel. A firmware upgrade runs its embedded `upgrade.sh` **as root**, so
our package just sets `root:olympian2026` (+ optionally surveys hardware) and
reboots. **We never need Huidu's factory password** — the upgrade channel *is* the
root access. This is exactly how we got into the earlier unit.

- Package built + verified: `packages/access-survey/access-survey_7.4.61.99_C15.bin`
  (and `.zbin`). Version `7.4.61.99` (chosen to clear the version-limit gate — see
  below), DeviceType `C15,C35,C36`, sets password `olympian2026`, then does a
  read-only survey into `/root/survey.tar.gz`, then reboots. Verified the embedded
  script has the real password (no `@ROOTPW@` placeholder) and the standard
  `killall -1 BoxDaemon; tar zxvf %s -C %s` decompress line.

### What actually happens (the bug)
`hdplayer upgrade-native <bin>` (port 9528 native protocol) runs the full flow and
the device **reports success** (`UpgradeStatus 3 → 2 → 0`), **but the script never
runs**: `.153` stays on firmware `7.4.59.0`, old password, all ports open,
`olympian2026` rejected. Evidence it didn't really execute:

1. **Decompress finished in ~70 ms.** A real decompress of a real package takes
   ~32 s (documented in the PCAP notes in `hdplayer-client/src/upgrade.rs`). 70 ms
   means `tar` had **nothing to extract**.
2. **Log line: `no explicit CloseFile answer — continuing`.** The device never
   sent the `CloseFileAnswer` (0x001c), so `/tmp/Box.tar.gz` was likely **never
   flushed/committed**. `tar zxvf` then extracted an empty/absent file, `upgrade.sh`
   never ran — yet the device still reported status 0.

This is the **known failure mode already documented in
`hdplayer-client/src/upgrade.rs` (phase 4b, ~line 557)**: "we sent CloseFile before
the device acked the final data chunk, so the close was processed out of order and
the file was never flushed. tar extracted an empty archive and upgrade.sh never ran
(even though the device still reported status 0)." A prior fix ("wait for final
FileDataAck before CloseFile", commit `241df31`) **is not sufficient on 7.4.61.0** —
the `CloseFile` ack still times out.

### Why it matters / why it's subtle
- The wire format is otherwise accepted (device happily transitions states).
- Our package is **tiny** (~1 KB inner tar.gz), a single data chunk — the flush /
  CloseFile timing likely behaves differently than for the 330 MB real firmware the
  PCAP was captured from. Suspect an **off-by-one / ordering issue in the
  FileData→CloseFile handshake for small, single-chunk files**.

### The version-limit gate (secondary, already handled but know it)
`BoxUpgrade` parses the `.bin` `<Version>` with `inet_addr()` and **silently skips
the script if `version < /root/Box/version/version.limit`**. The limit is always
≤ the installed version, so stamping `7.4.61.99` clears it on a 7.4.61.0 unit. This
is likely why some *earlier* canaries "didn't run" — but it is **not** the current
blocker (this run passed the gate; the CloseFile/flush issue is what's biting).

### Concrete ways to solve it (in priority order)
1. **Decompile the device's upgrade receiver and fix our client's file handshake.**
   Read the on-device 9528 upgrade service (`BoxUpgrade` / `libSDKServices` /
   whatever binds 9528): handlers `RecvOpenFileAsk` (0x17), `RecvFileContentAsk`
   (0x19), `RecvCloseFileAsk` (0x1b). Find exactly what it requires to **flush the
   QFile** and emit `CloseFileAnswer` (0x1c), then match it in
   `hdplayer-client/src/upgrade.rs` phase 4b. The goal: get a real `0x001c` ack (or
   whatever guarantees flush) **before** decompress, so `tar` sees a complete file.
   Verify by seeing decompress take ~seconds, not ~70 ms.
   **Both automated paths are currently broken on 7.4.61.0 (native + SDK), so
   this decompile is the critical path.**
2. **Try the SDK path** (`hdplayer firmware-upgrade <zbin>`, port 9527 BoxStream).
   Different mechanism — the device's full SDK `BoxUpgrade` handler + our
   MD5-verified file transfer. **TRIED — also fails:** the device **closes the
   connection during the upload** (`Error: Connection failed: connection closed by
   device`) right after `Uploading 'access-survey_...zbin' (2776 bytes)`. So the
   SDK file-transfer/AddFirmware framing is also wrong for 7.4.61.0 (the device
   rejects our upload mid-stream). This needs the same decompile: how does the SDK
   `BoxUpgrade` expect the firmware bytes (AddFirmware/FileStart opcodes, chunking,
   any header/version field it validates before accepting)?
3. **Known-good fallback: push via the real `HDPlayer.exe` GUI.** The user has it
   (`C:\Program Files\HDPlayer_7.11.18.0`) and has successfully upgraded a unit with
   it before — real HDPlayer executes packages correctly. Load
   `access-survey_7.4.61.99_C15.zbin`, select the device, click update. This
   sidesteps our client bug entirely and is the reliable way to get first access if
   the automated paths keep failing.

### Safety note
Every attempt so far left the units **provably unchanged** (still 7.4.59/61.0, same
password, all ports up). The survey script is read-only except the one `chpasswd`
line, and the tiny tar.gz contains a **single file**, so `tar` can't clobber
anything. Pushing is low-risk. The user is (rightly) protective of these two units
after likely bricking the third — **do not run heavier full-firmware upgrades on
them; only the access/survey package.**

---

## 2. Device inventory + how to talk to them

| IP | ID | Model | FW | telnet | 9527 | 9528 | SSH | root pw |
|---|---|---|---|---|---|---|---|---|
| 192.168.1.153 | C15-C21-BE371 | C15 (128×128) | 7.4.61.0 | ✅ | ✅ | ✅ | ✗ | **unknown** |
| 192.168.1.244 | C15-C21-BF096 | C15 | 7.4.61.0 | ✅ | ✅ | ✅ | ✗ | **unknown** |
| (old unit) | — | C15 | 7.11.x | — | — | — | — | was `olympian2026` |

- **Discover (IPs change on reboot):** `target/release/hdplayer discover --timeout 8`
  (binary is package `hdplayer`, crate dir `hdplayer-client/`).
- **SDK info:** `hdplayer --host <ip> info` / `hw-info` / `get-fpga-config`
  (works, but slow: ~12 s UDP-registration timeout then falls back to TCP).
- **Telnet:** prompts `localhost login:` (busybox). No `expect` on host/WSL. There's
  a raw-socket telnet helper at `/tmp/tn.py` (exec a command) and `/tmp/tntry.py`
  (try a cred list) written this session — re-create them if the temp dir is gone
  (they refuse telnet IAC negotiation and split sentinels so the command echo isn't
  mistaken for output).
- Stock telnet creds tried and **rejected**: empty, `root`, `888888`, `huidu`,
  `rockchip`, `admin`, `olympian2026`. Factory password still unknown.

---

## 3. What we're recovering / the golden-image plan

- The "broken" unit needs a golden image (or official firmware) flashed. Rockchip
  PX30 has a **mask-ROM in silicon** → almost always recoverable over **USB maskrom
  mode** even if eMMC is trashed. So it's very likely **not** permanently dead.
- To make a golden image **non-destructively** from a healthy unit, once we have
  shell (see blocker), run **`tools/clone/cunit_dump.sh ssh:root@<ip>`** (or
  `telnet:` once expect is available) — it captures live DTB, `/boot/fpga.img`,
  small partitions (loader/boot/recovery/oem/uboot/trust), `hardware.conf`,
  identity, DRM modes, GPIO bases, Wi-Fi chip → tarball + `FINDINGS.md`.
- The **access-survey package already includes this survey** — so the moment the
  blocker is fixed, one push yields root **and** `/root/survey.tar.gz`.
- Safest of all: **maskrom USB readback** with `tools/clone/rk_readback.sh`
  (rkdeveloptool) — reads eMMC raw without booting/altering the OS. Needs physical
  USB + maskrom mode. Best for both making the golden image and reflashing the
  broken unit.
- Identity that must NOT be overwritten when cloning: `/root/Box/data/id` (master),
  MAC (`GenMACWithID` → `/boot/dev_mac`), license, `oem` partition. See
  `tools/clone/identity_files.list` and `CLONE_AND_RECOVERY.md`.

---

## 4. What was ported / built this session (all committed + pushed to `main`)

Recent commits (newest first):
- `9dffb10` huidu-player: **fix file-transfer integrity** — both FileEnd paths
  computed `md5_ok` then ignored it; a corrupt upload was written + reported success.
  Now `FileTransfer::verify()` checks size+MD5 and rejects (never writes corrupt).
- `1c720fb` huidu-player: **network/storage/sensor ports** — Get/SetEthernetInfo
  (+SetNetworkInfo), Get/SetPPPoEInfo, GetStorageInfo, GetSensorType,
  GetCurrentModbusValue, password aliases; `apply_ethernet_config` helper.
- `b945304` huidu-player: **GetCurrent\* real-time telemetry** — 12 readout methods
  (DateTime/Time/Volume/Luminance/Temperature/Humity/GPSInfo/Protocol+Version/
  PlayProgramIndex/Program/Image).
- `a826601` tools/clone: **`cunit_dump.sh`** — one-command hardware/identity dump.
- `b3ba6d9` huidu-sender: **FPGA loader** (`write_fpga` replacement: parse
  `/boot/fpga.img`, passive-serial load via spidev+gpio, `strip-fpga`/`load-fpga`)
  + **`hardware/DRIVER_ENABLEMENT.md`** driver audit.
- `56f99e8` br2-huidu: drop eudev (serialport w/o libudev).
- `4043dfe` br2-huidu: wire `huidu-player` (boxplayer) into the image.
- `81d6a8b` huidu-sender: **ttyS1 FPGA control daemon** + Buildroot wiring.

Test status: `boxplayer` 163 tests pass; `huidu-sender` 20 tests pass. Both build
clean. Buildroot external tree validated (`make huidu_px30_defconfig` + `show-info`
exit 0, both packages recognized).

### Key components
- **`huidu-player/`** (bin `boxplayer`): the display player. ~150 SDK methods, DRM
  KMS output (`--output drm` = correct PX30 path), file transfer, upgrade, sensors,
  relays, modbus, GPS, cloud. **This is the replacement for stock BoxPlayer.**
- **`huidu-sender/`**: our ttyS1 FPGA control daemon (scan/gamma/brightness/status
  over the 8-byte-preamble + payload + CRC-32 `0xEDB88320` frame) **+** the FPGA
  bitstream loader (replaces closed `write_fpga`). CLI: `run|emit|oneshot|
  strip-fpga|load-fpga`. See its `README.md`.
- **`br2-huidu/`**: Buildroot external tree, mainline 6.18 + Panfrost, DTS for
  C15/D15, both crates packaged. **Driver story:** base platform 100% mainline;
  LED path uses mainline `altera-ps-spi` (not the closed `cyclone4.c`); GPU optional
  (player is tiny-skia + KMS, no libmali). See `hardware/DRIVER_ENABLEMENT.md`.
- **`hdplayer-client/`** (bin `hdplayer`): our HDPlayer.exe reproduction — discover,
  SDK control, and the **upgrade client that has the bug** (§1).
- **`tools/`**: `mkhdplayerbin.py` (build/inspect .bin/.zbin), `clone/` (readback,
  identity backup/restore, `cunit_dump.sh`).
- **`packages/access-survey/`, `packages/keepaccess-7.11/`**: the access packages.

---

## 5. Confirm-on-hardware items still open (need shell first)

Derived from static RE; validate on a live unit once we have root:
1. **C15 panel timing** — real value (screen is **128×128**, not the D15 placeholder
   1024×128 in `br2-huidu/.../px30-huidu-c15.dts`). Pull from the live DTB.
2. **FPGA-load bit order** — default LSB-first; flip `--msb-first` if it won't
   configure (`huidu-sender/src/fpga_load.rs`).
3. **FPGA control GPIO numbers** — sysfs globals for nCONFIG/nSTATUS/CONF_DONE.
4. **512-byte scan/gamma blob** + **brightness offset** — capture two ttyS1
   send-card frames at two brightnesses and diff (`huidu-sender/src/blob.rs`).
5. **Exact Wi-Fi chip** — from `/etc/hardware.conf` (`wifi=`) + `lsusb`; may need a
   driver `rtl8xxxu` doesn't cover.
6. Whether `/dev/mdio_gpio` / `/dev/audio_switch` exist on C-series.

---

## 6. Suggested plan for the next session

1. **Fix the upgrade (blocker).** Decompile the on-device 9528 upgrade receiver
   (`BoxUpgrade`/`libSDKServices`) file handlers (0x17/0x19/0x1b) → find the flush
   requirement → fix `hdplayer-client/src/upgrade.rs` phase 4b so `CloseFile` is
   acked (0x1c) and the file is committed before decompress. Success = decompress
   takes seconds and the script actually runs. (Both native and SDK automated paths
   are already confirmed broken on 7.4.61.0 — this decompile is the critical path.)
2. **Fallback if needed:** push `access-survey_7.4.61.99_C15.zbin` via the real
   `HDPlayer.exe` GUI to get first root, then iterate the automated path with a
   live shell to compare captures.
3. **Once root:** run `cunit_dump.sh` (or just pull `/root/survey.tar.gz`), fold the
   real C15 panel timing / GPIO / Wi-Fi into the DTS + kernel fragment, resolve the
   FPGA bit-order and scan/gamma blob.
4. **Recover the broken unit** via maskrom USB (`rk_readback.sh` to image a healthy
   unit, then flash). Preserve identity (`identity_files.list`).
5. **Keep porting** the remaining SDK surface (niche `GetCurrent*`, and the
   security-gated `RunCommand`/`FormatStorage`/`Restart` — get user intent first).

---

## 7. Gotchas learned this session

- **hdplayer commands are slow** (~12 s UDP-registration timeout → TCP fallback).
  Not a hang; wait it out or fix the discovery timeout.
- **Don't pipe long-running client output through `grep|tail`** — buffering hides
  progress and a kill loses it all. Redirect to a file and read the file.
- **Version stamp for a 7.4.61.0 unit:** use `7.4.61.99` (same train, clears the
  limit gate). Don't stamp 7.11.x onto 7.4 units unnecessarily.
- **CRC-32 poly for ttyS1 is `0xEDB88320`** (the RE report's `0xEDB88820` was a typo;
  verified vs zlib).
- Git attribution: commits end `Co-Authored-By: Claude Opus 4.8
  <noreply@anthropic.com>`; PRs end the Claude Code line. Secrets (device password
  files) are committed to this private repo by explicit user consent.
