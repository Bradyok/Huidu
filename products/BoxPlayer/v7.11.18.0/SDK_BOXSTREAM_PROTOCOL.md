# SDK control protocol (port 9527, BoxStream) — captured from real HDPlayer.exe

Ground truth from a packet capture of stock **HDPlayer.exe 7.11.18.0** controlling a **C15 on
7.4.61.0** (`scratchpad/hdplayer_real.pcapng`). This is why our `hdplayer` client's control and
upgrade commands were accepted but silently ignored.

Framing: `[u16 LE len][u16 LE cmd][payload]`, len = cmd+payload bytes.

## Connect + authorize (all on ONE 9527 connection — no port 9528, no UDP token)

| # | dir | cmd | payload | notes |
|---|---|---|---|---|
| 1 | PC→ | `0x000b` ConnReq | `09 00 00 01` = **0x01000009** | our client sent 0x01000007 — **wrong** |
| 2 | →PC | `0x000c` ConnAck | `07 00 00 01` | device's own version |
| 3 | PC→ | `0x0410` CliInfoReq | 543-byte CSV (below) | full client + NIC list |
| 4 | →PC | `0x0411` CliInfoAck | `00 00` | |
| 5 | PC→ | `0x0300` | (empty) | |
| 6 | →PC | `0x0301` | `00 00 00 00` | |
| 7 | PC→ | `0x0200` BoxStreamInit | **`00 00 00 00`** | **zeros, NOT the UDP token** — our client sent the token and got closed |
| 8 | →PC | `0x0201` BoxStreamInitAck | `00 00` | device accepts |

CliInfoReq CSV (null-terminated):
`Windows,HDPlayer,<user>,<hostname>,,,_,<YYYY-MM-DD_HH:MM:SS>,<iface>,<iface>,…`
where each `<iface>` = `<type>_<ifindex>-<ip>-<MAC>` for every local adapter.

## Per-message request/response (a full sub-handshake each, with a sequence number)

Every command AND every reply is framed like this (seq is a u32 that increments 0,1,2,…):

```
sender → 0x0200 BoxStreamInit [u32 seq]      # "message seq incoming"
recv   → 0x0201 BoxStreamInitAck 0000
sender → 0x0202 StreamData [u16 seq_lo][XML] # the payload
recv   → 0x0203 StreamAck 0000
sender → 0x0204 [u16 seq]
recv   → 0x0205 0000
```

The device answers with the same six-step pattern (its own seq). So the client must implement this
sequenced exchange, not a single BoxStreamInit.

## Command payload = SDK XML

```xml
<?xml version="1.0" encoding="utf-8"?>
<sdk guid="##GUID">
  <in method="GetIFVersion"><version value="##value"/></in>
</sdk>
```
Reply: `<out method="GetIFVersion" result="kSuccess"><version value="1000000"/></out>`.
`guid="##GUID"` is a literal placeholder.

## Method sequence HDPlayer uses after connect

`GetIFVersion` → **`TryLock`** → then the queries:
`GetDeviceName, GetFirewareVersion, GetKeyDefine, GetPlayStatus, GetSystemVolume, GetBootLogo,
GetSensorInfo, GetGPSInfo, GetCurrentLuminance/Temperature/Humity, GetSensorType, GetSwitchTime,
GetTimeInfo, GetLuminancePloy, GetScreenInfo, GetLicense, GetEth0Info, GetWifiInfo, GetPppoeInfo,
GetDeviceInfo, GetDataSourceInfo, GetRelay`.

**`TryLock` is the authorization gate.** HDPlayer acquires the lock immediately after GetIFVersion;
only then does the device act on commands. Our client never sends TryLock, which is why every
control command (and, by the same account, the upgrade execute) was accepted but not acted on.

## Fixes required in `hdplayer-client` (BoxStream path)

1. ConnReq version → `0x01000009`.
2. BoxStreamInit payload → `0x00000000` (drop the UDP-token logic for this firmware).
3. Do the whole handshake on 9527 (ConnReq → CliInfoReq → 0x0300 → BoxStreamInit); don't rely on the
   port-9528 login for authorization.
4. Implement the per-message sequenced sub-handshake (0x0200/0x0201/0x0202/0x0203/0x0204/0x0205).
5. Send **`TryLock`** right after GetIFVersion, before any control or upgrade action.

Open: confirm whether the firmware **upgrade** also requires holding the 9527 `TryLock` (very likely —
same "accepted but ignored" signature on 9528). Capture an HDPlayer upgrade to verify.
