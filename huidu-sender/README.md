# huidu-sender

Our own LED **sender-card control daemon** for the Huidu PX30 controllers
(C15 / C35 / C36). It drives the FPGA config link on `/dev/ttyS1` — the same
link the stock `BoxPlayer` uses through `libFPGADriver.so` — so we can bring a
LED wall up, configure it, and hold it lit **without running any Huidu binary**.

Reverse-engineered in
[`products/BoxPlayer/v7.11.18.0/PLAYER_PIPELINE.md`](../products/BoxPlayer/v7.11.18.0/PLAYER_PIPELINE.md)
§2. Modeled on NovaOS's `a200-sender` (a UART daemon for a non-NovaStar sender).

---

## The one thing to understand first

**Pixels do not travel over `/dev/ttyS1`.** On the PX30 the image reaches the LED
panel as the SoC VOP's **parallel RGB888** output, latched by the LED FPGA. The
serial link only carries *configuration and status*: scan tables, gamma, colour
correction, brightness, geometry, per-card params, and read-backs.

So a drop-in replacement for BoxPlayer's display stack is just two independent
pieces:

1. **A KMS client that produces the right RGB frame** at the right mode/timing
   (any Wayland/DRM app, `kmscube`, ffmpeg → DRM, or our `huidu-player`).
2. **This daemon**, which programs the FPGA over `ttyS1` so that RGB frame
   actually lights the panel correctly.

`huidu-sender` is piece 2.

---

## Wire protocol (`/dev/ttyS1`, 115200 8N1, raw)

### Frame ([`src/frame.rs`](src/frame.rs))

```
+--------------------------+-------------------+---------------+
| 8-byte PREAMBLE (fixed)  |   PAYLOAD (N)     | CRC32 (4, LE) |
| 55 55 55 55 55 55 55 D5  |                   |  over PAYLOAD |
+--------------------------+-------------------+---------------+
```

- Preamble = 7×`0x55` training + `0xD5` SFD (also the RX frame-sync marker).
- **CRC-32 = standard IEEE-802.3 / zlib**: reflected, poly `0xEDB88320`,
  init/xorout `0xFFFFFFFF`, over the payload only. Verified against the zlib
  check vectors (`crc32("123456789") == 0xCBF43926`) — see the unit tests.
  *(The RE report originally transcribed the poly as `0xEDB88820`; that was a
  typo. `0xEDB88320` is confirmed by round-tripping our emitted frames through
  Python's `zlib.crc32`.)*
- Only two payload sizes are legal (the stock `SendData` rejects the rest):
  **control = 25 bytes** (37 on the wire) and **param = 521 bytes** (533 on the
  wire).

### Payload sub-header ([`src/protocol.rs`](src/protocol.rs))

Every payload is a 9-byte sub-header + data (16 for control, 512 for param):

| off | type    | field                                                        |
|-----|---------|--------------------------------------------------------------|
| 0   | u8      | target: `1`=recv-card, `2`=send-card (resp `1`=ctl, `3`=param)|
| 1   | u8      | card index / address                                         |
| 2   | u8      | phy / sub-index                                              |
| 3   | u16 LE  | function code                                                |
| 5   | u16 LE  | param / offset                                               |
| 7   | u16 LE  | data length: `0x10` (control) or `0x200` (param)             |
| 9.. | data    | 16 or 512 bytes                                              |

### Function codes

| code     | meaning                                                     |
|----------|-------------------------------------------------------------|
| `0x0100` | search recv-cards (ctl) / basic param (param)               |
| `0x0200` | readback (ctl) / high-refresh scan table (param)            |
| `0x0300` | gamma table / save-to-card (param)                          |
| `0x0400` | locus / brightness-index LUT (param); status resp sub-func  |
| `0x0500` | recv-card range/geometry (param) / read temperature (ctl)   |
| `0x0600` | check HDMI signal (ctl)                                      |
| `0x0700` | single-phy param change (param)                             |
| `0x1000` / `0x1100` | lock / unlock "last frame" (ctl)                 |
| `0x0000` | global send-card param **incl. brightness** (param, target 2)|
| `0x00FF` | read send-card status (ctl)                                 |
| `0x23CC` / `0x34D2` | erase / read-back SPI flash (ctl)                |

**Brightness, gamma and scan are not opcodes.** They are *fields inside* the
512-byte param blobs. That is deliberate on Huidu's part and it's the crux of
the "secret sauce" problem below.

---

## The 512-byte param blobs — the hard part ([`src/blob.rs`](src/blob.rs))

The param frames carry 512-byte blobs built by big per-driver-IC const tables
baked into `libFPGADriver.so` (`s_dualScanTab`, `s_lightPriority` /
`s_refreshPriority` / `s_grayPriority`, gamma, colour-correction, locus). Those
tables are what actually make a specific panel scan correctly. Re-deriving them
is a large job, so **this daemon does not synthesize them.** It ships them one of
two ways:

1. **Captured templates (recommended for first light).** Dump the exact param
   frames the stock BoxPlayer sends on a correctly-configured C15/C35/C36 (logic
   analyzer, or an on-device serial tap on `ttyS1`), strip preamble+CRC, and
   drop the 512-byte payloads in as files. Replaying them verbatim reproduces
   the stock configuration bit-for-bit — no table re-derivation needed.
2. **Relinked `libFPGADriver`.** Call the stock `.so`'s param builders
   (`GenSendCardParamAsk`, `UpdateFPGASendCardParam`, `GenRecvCardParamAsk`,
   …) over FFI to produce the blob, then frame it here. This keeps Huidu's
   tables intact while replacing everything above them. It's the "right" long
   term answer; it needs the C++ ABI / Qt deps of the lib resolved on the target
   (the lib is already on the eMMC of every unit, so it can be `dlopen`ed).

### Brightness

Global brightness is a field *inside* the 512-byte send-card blob (func
`0x0000`), so `brightness N` just rewrites that field and re-sends the blob — no
full table build. **The exact offset is not yet pinned** (`BRIGHTNESS_OFFSET` is
a sentinel and `patch_brightness` refuses to touch the blob until it's set, so a
wrong guess can't corrupt a panel). To find it: capture two stock send-card
frames at two brightnesses and diff them; wire the differing offset in.

---

## Daemon ([`src/main.rs`](src/main.rs))

```
huidu-sender run [--tty /dev/ttyS1] [--listen 127.0.0.1:7654] [--status-secs 5]
                 [--sendcard FILE] [--recvcard FILE ...]
huidu-sender emit    <search|status|temp|hdmi|lock N|unlock N>   # print frame hex, no device
huidu-sender oneshot <search|status|temp|hdmi>                   # send one frame, print reply
```

`run` performs the power-on sequence (search cards → push param blobs → save →
periodic status) and serves a line protocol on `--listen` for the player/CMS:

```
brightness <0-255> | status | search | lock <card> | unlock <card> | quit
```

`emit` needs no hardware and is the offline conformance tool — it's how the CRC
and framing were validated against zlib. `oneshot` sends a single frame and
prints the decoded reply, for bench bring-up.

Without a captured blob the daemon still frames every control command and reads
status — enough to prove the link on a bench unit — but it cannot push the
scan/gamma tables that light a real panel. Supply the blobs (see above) for a
lit wall.

---

## Buildroot integration (`br2-huidu`)

Wired into our external tree as a package:

- `br2-huidu/package/huidu-sender/Config.in` — `BR2_PACKAGE_HUIDU_SENDER`
  (selects `host-rustc`, built with Buildroot's `cargo-package` infra from this
  crate, which is the sibling of `BR2_EXTERNAL_HUIDU_PATH`).
- `br2-huidu/package/huidu-sender/huidu-sender.mk` — builds the crate and
  installs the init service + defaults.
- `br2-huidu/package/huidu-sender/S95huidu-sender` — starts the daemon late
  (after tty/watchdog/network), reading `/etc/default/huidu-sender`.
- `br2-huidu/package/huidu-sender/huidu-sender.default` — `TTY`, `LISTEN`,
  `SENDCARD`, `RECVCARDS`.
- Enabled by default in `configs/huidu_px30_defconfig`
  (`BR2_PACKAGE_HUIDU_SENDER=y`).

A standalone `Cargo.lock` (crate + `libc` only) ships beside the crate so the
out-of-workspace Buildroot build vendors reproducibly; inside the workspace the
root lockfile takes precedence and this one is ignored.

---

## Confirm-on-hardware checklist

Everything below is derived from static RE and must be validated on a live unit
before a field image:

- [x] **CRC-32 polynomial** — `0xEDB88320` (resolved; verified vs zlib).
- [ ] **Brightness field offset** inside the 512-byte send-card blob
      (`BRIGHTNESS_OFFSET`, `src/blob.rs`). Capture-and-diff.
- [ ] **512-byte blob field layout** for scan / gamma / geometry — or just
      replay captured blobs and skip the layout.
- [ ] **C-series panel mode/timing** the VOP must output (mode line for the KMS
      client) — read from a stock unit's DRM state.
- [ ] **Response framing** on RX (does the FPGA echo control frames with target
      `1`/param `3` as decoded here?). Validate with `oneshot`.
- [ ] **Save semantics** of func `0x0300` (save-to-card) vs volatile config.

## Tests

```
cargo test -p huidu-sender          # 12 frame/protocol/blob tests, incl. zlib CRC vectors
cargo run  -p huidu-sender -- emit search   # offline frame dump
```
