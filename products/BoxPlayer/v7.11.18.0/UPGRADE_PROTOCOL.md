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
| `0x001b` CloseFileAsk | `RecvCloseFileAsk` | **exactly 4 bytes** | **closes/flushes the file** (`QFileDevice::close`); replies CloseFileAnswer. **Required** — without it the archive is left unflushed and `tar` extracts nothing |
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

## Correct client sequence (one connection)

```
ConnectReq → ConnectAck
ClientInfoReq → ClientInfoAck
NullCapQuery → NullCapResp ; CapQuery → CapResp
UpgradeCMD(mode=1)  (limit-version)      [optional]
OpenFileAsk(0x17)  [/tmp/Box.tar.gz\0][size]  → OpenFileAnswer
FileContentAsk(0x19) × N
CloseFileAsk(0x1b) [4 bytes]  → CloseFileAnswer      ← was missing in our client
UpgradeCMD(mode=3) [decompress cmd]  → status=3       (Unpackage)
UpgradeCMD(mode=2) [script name]     → runs the shell (async)
poll UpgradeCMD(mode=0) until status=0 (done)
```

## Client bugs found and fixed (hdplayer-client/src/upgrade.rs)

1. **`.bin` payload offset** hardcoded to 678 → now read from the header length field (`28 + xml_len`),
   so packages of any size parse.
2. **Blind 600 s decompress wait** → configurable `--decompress-wait`, with an early exit when the
   device reports extraction done.
3. **Missing CloseFileAsk (0x1b)** → the uploaded archive was never flushed, so extraction produced
   nothing and the script never ran. Now sent after the data chunks. *(under verification)*

Still open: exact `UpgradeExec` connection/state semantics (device only heartbeat-acks it on the
transfer connection); the async timer cadence between mode=2 and status=0.
