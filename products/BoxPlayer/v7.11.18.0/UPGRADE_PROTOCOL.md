# BoxUpgrade network protocol (port 9528)

Reverse-engineered from the device's `System/BoxUpgrade` (Ghidra, `HUpgrade::*`) and confirmed
against live packet captures of our client talking to a C15 on firmware 7.4.59.0. This is what the
Rust `hdplayer` client's `upgrade.rs` must match.

Framing: `[u16 LE len][u16 LE cmd][payload]`, where `len` counts `cmd`+`payload` (i.e. payload =
len − 2). The device buffers into a 5-byte-prefixed struct, so in the decompile the cmd is at
`session+7` and len at `session+5`.

## Command dispatch (`HUpgrade::DisposeTcpPacket`)

| cmd | handler | payload | purpose |
|---|---|---|---|
| `0x000b` ConnectReq | — | `[u32 LE 0x01000007]` | protocol-version hello; device replies `0x000c` ConnectAck |
| `0x0410` ClientInfoReq | — | CSV, null-terminated | login; device replies `0x0411` ClientInfoAck `[u16 0]` |
| `0x0053` NullCapQuery | — | empty | device replies `0x0054` `[u32 0]` |
| `0x040a` CapQuery | — | `[u16 0]` | device replies `0x040b` `[u8 0]` |
| `0x0017` OpenFileAsk | `RecvOpenFileAsk` | `[filename\0][u64 LE size]` (len>4) | opens/creates the file (stored at `this+0x38`, size at `+0x40`); replies OpenFileAnswer |
| `0x0019` FileContentAsk | `RecvFileContentAsk` | file bytes (chunks) | appends to the open file; periodic `0x001a` acks |
| `0x001b` CloseFileAsk | `RecvCloseFileAsk` | **EMPTY (frame total_length must be exactly 4)** | **closes + renames the file** (`QIODevice::isOpen`→`QFileDevice::close`→`QFile::rename`→`system(flush)`); replies CloseFileAnswer `08 00 1C 00 00 00 00 00`. **Required** — without it the archive is never committed to `/tmp/Box.tar.gz` and `tar` extracts nothing. ⚠️ "exactly 4" means the **len field**, i.e. a **zero-length payload** — sending a 4-byte payload makes total_length=8 and `RecvCloseFileAsk` (`ccmp w2,#4` / `cmp r2,#4;bne`) rejects it SILENTLY (no reply at all). |
| `0x0055` UpgradeCMDAsk | `RecvUpgradeCMDAsk` | `[u16 LE mode][…]` | see mode table below |
| `0x0057` UpgradeOutAsk | `RecvUpgradeOutAsk` | 4 bytes | replies `0x0058` UpgradeOutAnswer |
| `0x0730` UpgradeExec | (raw, in `ServiceAdaptor`) | len>11 | replies `0x0731` ExecAck (carries the IsLimitUpgrade flag). Does **not** itself run the script |

## UpgradeCMDAsk modes (`RecvUpgradeCMDAsk`, switch on the u16 after cmd)

| mode | handler | meaning |
|---|---|---|
| 0 | `SendGetUpgradeResultAnswer` | poll upgrade status/result |
| 1 | `SendUpgradeLimitVersionAnswer` | **query limit version** (not "enter upgrade mode" as earlier assumed) |
| 2 | `RecvUpgradeShellAsk` → `SendUpgradeShellAnswer` | **run the upgrade** (Shell): builds and runs the command |
| 3 | `RecvUpgradeUnpackageAsk` → `SendUpgradeUnPackageAnswer` | Unpackage (stores the decompress command) |

## What actually runs the script (`RecvUpgradeShellAsk` / `ProcessUpgrade`)

The device builds one shell string (via `__sprintf_chk`) and runs it with `system()`
(asynchronously, off a timer → `DisposeUpgradeShellAsk`, `vfork`+`system`):

```
echo "0" > <status>; cd <base>; rm -rf UpgradeDir; mkdir UpgradeDir;
chmod u+x <base>/UpgradeDir/<script>; dos2unix <base>/UpgradeDir/<script>;
<decompress-cmd> <args>;          # e.g. tar zxvf /tmp/Box.tar.gz -C <base>/UpgradeDir
<base>/UpgradeDir/<script>        # e.g. .../UpgradeDir/upgrade.sh
```

`ProcessUpgrade` branches on the firmware `<Type>`:
- `BoxUpgrade`: `system(setup); execvp("/bin/sh", …)`
- contains `BoxPlayer`: **version-limit gate** — parses the upgrade version with `inet_addr()`
  (dotted-quad!) and compares to `HSystemEnv::getLimitVersion()`; if `version < limit` it logs
  "version less than limit version and not to upgrade!" and skips. Otherwise `system(setup);
  system(script)`.
- other: `system(setup); system(script)`.

So the upgrade version in the `.bin` XML must parse as a dotted quad ≥ the device's limit
(`/root/Box/version/version.limit`, 7.6.31.0 here) or the script is silently skipped.

## Live capture: stock HDPlayer upgrading a C15 (2026-09-23) — ground truth

`hdplayer_firmware_upgrade_20260923_1609.pcapng` (repo root, ~395 MB, Ethernet + loopback, unfiltered;
use `tcp.port == 9528`). HDPlayer 7.11.18.0 pushed `BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin`
to C15-C21-BF096 (192.168.1.244). Result: **7.4.61.0 → 7.11.18.0, FPGA 6.3.70.0 → 6.22.70.0, success.**
Only the inner `BoxPlayer_7_11_18_0.bin` is used; `MagicPlayer_V2.12.8.0.bin` is not sent to a C15.
(The second selected box, BE371, was never contacted — HDPlayer apparently upgrades one at a time.)

| t (s) | conn | direction | frame | notes |
|---|---|---|---|---|
| 19.26 | 1 | → | ConnectReq `07 00 00 01` | ConnectAck echoes `07 00 00 01` |
| 19.26 | 1 | → | ClientInfoReq (543-byte CSV) | → `0x0411 [00 00]` |
| 19.26 | 1 | → | NullCap / CapQuery (both empty) | → `[u32 0]` / `[u8 0]` |
| 19.26 | 1 | → | UpgradeCMD mode=1 | → `0x0056 01 00 07 04 3b 00` = **limit version 7.4.59.0** (one byte per field) |
| 19.78 | 1 | → | OpenFile `/tmp/Box.tar.gz\0` + u64 **330110795** | → `[u32 0]`. Declared = **whole .bin** incl. 678-byte header |
| 19.79–53.86 | 1 | → | 35 835 × FileContent (9212 B) = **330110117** B | only the tar.gz (`1f 8b 08 00…`) — 678 B less than declared |
| | 1 | ← | 35 835 × `0x001a [u32 0]` | one answer **per chunk** |
| | 1 | ← / → | device `0x0060` every ~5.7 s; PC `0x005f` every ~6 s | bare keepalives, never answered by either side |
| 53.86 | 1 | → | CloseFile `04 00 1b 00` (empty) | → `0x001c [u32 0]` after 0.85 s |
| 54.71 | 1 | → | UpgradeCMD mode=3 `killall -1 BoxDaemon; tar zxvf %s -C %s \0` | → `0x0056 03 00 00 00 00 00` |
| 54.71 | 1 | → | UpgradeCMD mode=2 `upgrade.sh\0` | **no reply** |
| 59.7–104.7 | 1 | → | mode=0 every 5 s + heartbeat | **no reply at all**; HDPlayer RSTs conn 1 at 125.2 |
| 139.59 | 2 | → | ConnectReq `07 00 00 01` | ConnectAck `09 00 00 01` at 153.88 (**14 s**, protocol **v9** = the new firmware answering) |
| 153.88 | 2 | → | UpgradeExec `0x0730 [u64 8]` | → `0x0731 [00 00]` (10 s) |
| 164.12 | 2 | → | ClientInfoReq | → Ack (10 s) |
| 174.33 | 2 | → | NullCap | → `[u32 0]` (10 s) |
| 184.6–194.7 | 2 | → | mode=0 every 5 s (+ heartbeat) | → `0x0056 00 00 01 00 00 00` at 194.83 = **result 1 = success**; HDPlayer FINs |

HDPlayer also opened a second socket at 139.59 and never used it.

### UpgradeStatus (0x0056) payload = `[u16 echoed mode][4 bytes]`

The first u16 is the **mode being answered**, not a status. For mode=0 the 4 bytes are an i32 read from
`/root/upgrade.status` by `HNetUpgrade::SendGetUpgradeResultAnswer`: `'0'`→0 running, `'1'`→**1 success**,
`'2'`→2 failed, missing/other→−1. (Earlier notes said "poll until status=0 (done)" — wrong: 0 means
still running, and the old client's check read the echoed mode, so any mode=0 reply looked like "done".)

## Correct client sequence (as HDPlayer does it)

```
conn 1:
ConnectReq → ConnectAck
ClientInfoReq → ClientInfoAck
NullCapQuery → NullCapResp ; CapQuery → CapResp
UpgradeCMD(mode=1) → [1][limit version a.b.c.d]
OpenFileAsk(0x17)  [/tmp/Box.tar.gz\0][u64 whole-.bin size]  → OpenFileAnswer
FileContentAsk(0x19) × N (9212 B)  ← 0x1a per chunk; heartbeat 0x5f every 6 s
CloseFileAsk(0x1b) EMPTY  → CloseFileAnswer 0x1c
UpgradeCMD(mode=3) [decompress cmd]  → [3][0]
UpgradeCMD(mode=2) [script name]     → (nothing)
poll mode=0 + heartbeat — device stays silent; abandon the socket
conn 2 (retry until it accepts; each reply may take 10–15 s):
ConnectReq → ConnectAck (v9 = new firmware)
UpgradeExec(0x730) [u64 8] → ExecAck [u16 0]
ClientInfoReq → Ack ; NullCapQuery → Resp
poll mode=0 every 5 s until result 1 (success) or 2 (failed)
```

## Client bugs found and fixed (hdplayer-client/src/upgrade.rs)

1. **`.bin` payload offset** hardcoded to 678 → now read from the header length field (`28 + xml_len`),
   so packages of any size parse.
2. **Blind 600 s decompress wait** → configurable `--decompress-wait`, with an early exit when the
   device reports extraction done.
3. **Missing CloseFileAsk (0x1b)** → the uploaded archive was never flushed, so extraction produced
   nothing and the script never ran. Now sent after the data chunks.
4. **CloseFileAsk payload length (the real 7.4.61.0 blocker)** → the client sent CloseFile with a
   4-byte payload, so the frame's total_length field was 8. `RecvCloseFileAsk` accepts CloseFile only
   when total_length == 4 (an **empty** payload), so the device silently dropped it, never ran
   `SendCloseFileAnswer`, and the file was never close()d/renamed → `tar` saw nothing (the ~70 ms
   "decompress" + missing 0x1c). Verified in disasm: PX30 `BoxUpgrade` @ 0x420798, RK3288
   `libBoxUpgrade.so` @ 0x138bc. **Fixed:** `upgrade.rs` now sends CloseFile with an empty payload
   (`conn.send(CMD_CLOSE_FILE, &[])` → wire `04 00 1B 00`) and requires the 0x1c answer.
5. **Phase-5 heartbeat hang** → the client sent a heartbeat (0x5f) after the file and blocked on a
   reply, but `HUpgrade::DisposeTcpPacket` has no 0x5f handler (default branch, no reply). Removed;
   the 0x1c CloseFileAnswer is now the file-receipt signal.

Note on the earlier `TryLock` theory (see `SDK_BOXSTREAM_PROTOCOL.md`): the string `TryLock` does
**not** appear in any of the real HDPlayer captures we hold (only `GetDeviceLockerEnable` /
`DeviceLocker enable="0"`), so "accepted but ignored ⇒ missing TryLock" is unverified. The 9528
native-upgrade failure is fully explained by the CloseFile length bug above, independent of any lock.

6. **Post-transfer flow (from the 2026-09-23 live capture)** → the client read the echoed mode as the
   status (so any mode=0 reply looked like "done"), stayed on connection 1 after mode=2 (the device
   never answers there), and never ran the connection-2 `UpgradeExec` step. `upgrade.rs` now mirrors
   HDPlayer: poll conn 1 until 60 s of silence, reconnect, UpgradeExec `[u64 8]`, ClientInfo, NullCap,
   then poll mode=0 for result 1/2. It also declares the whole-.bin size in OpenFile and sends the 6 s
   heartbeat throughout, as HDPlayer does.

Still open: whether UpgradeExec on conn 2 is *required*, or HDPlayer's habit (the device finished
installing on its own; conn 2 only reads the result). Our Rust client's new flow has **not yet been
run against hardware** — only HDPlayer's run above has been captured.
