# BoxPlayer Render + FPGA Pipeline — what a replacement player must reproduce

Firmware 7.11.18.0, PX30 C-series (C15/C35/C36). Evidence is `file:line` into
`products/BoxPlayer/v7.11.18.0/decompiled/*.c` and `strings/*.txt`, plus bytes read
from the real binaries in `firmware_extract/RK3288_unstripped/` (ARM32, **not stripped** — this is
what was decompiled) and `products/BoxPlayer/v7.11.18.0/full/` (aarch64, stripped, the PX30 build).

> **Ghidra image base = 0x100000.** Verified: the `.rodata` function-name constants
> `"CRC32"`/`"PacketBuffer"`/`"CheckOneFrame"` sit at aarch64 file offsets 0x93ea0/0x93eb0/0x93ed0,
> i.e. Ghidra 0x1938a0/0x1938b0/0x1938d0 — exactly the `__func__` args those functions pass to
> `HLog::Trace`. So Ghidra `DAT_00193ec0` → aarch64 file offset **0x93ec0**.

---

## 0. TL;DR reimplementation surface

A replacement player on this SoC keeps the **entire pixel path unchanged** — it is all mainline
Rockchip/Mesa/FFmpeg — and must re-implement only the **Huidu-proprietary control + bitstream side**:

| Layer | Standard / Huidu | Replace? |
|---|---|---|
| Compose + scan-out (KMS/GBM/EGL/GLES2, `drmModePageFlip`, XRGB8888) | mainline Rockchip DRM | No — use any KMS client |
| Video decode (FFmpeg software) + YUV→RGB/scale (librga) | mainline | No |
| **`/dev/ttyS1` FPGA control protocol** (§2) | **Huidu** | **Yes — full re-implementation** |
| **`/boot/fpga.img` bitstream + `/dev/cyclone4` SPI load + `write_fpga`** | **Huidu/Altera** | **Yes — reuse the blob as-is** |
| **Scan / gamma / gray / refresh / light-priority tables** (§2.6) | **Huidu, baked into libFPGADriver** | **Yes — must reproduce or link the lib** |
| **Program/playlist XML model + content plugins** (§3) | **Huidu** | Only if you keep HDPlayer/SDK compatibility |

The FPGA never sees your pixels over serial. It scans the SoC's parallel-RGB888 VOP output
(hardware/PX30_C_SERIES_HARDWARE.md §4). ttyS1 only carries *configuration and status*: which scan
table, gamma, brightness, panel geometry, and receiving-card parameters the FPGA/receiver cards use.
**If you produce the right RGB frame on the CRTC and send the right ttyS1 config, the panel lights.**

---

## 1. Render loop (libMainWindowRender.so)

Dedicated render **thread**, not a Qt timer.

- `HRenderEngine::RenderLoop` (`libMainWindowRender.so.c:20521`, thread name "RenderThread" `:20846`)
  raises itself to **`SCHED_RR` at max priority** (`sched_get_priority_max` +
  `pthread_setschedparam(...,2,...)`, `:20523-20530`), then loops unbounded
  `do { …; Render(this); } while` (`:20551-20561`), checking a debug pause hook
  `QFile::exists("/root/pause")` each pass (`:20554`).
- **No inter-frame sleep in the active path.** Cadence is set entirely by the page-flip block (below).
  Idle-only sleep when no program is active: `usleep(RefreshRate()*k)` (`:18381-18382`); startup
  wait `usleep(1000)` on the message queue (`:16621-16623`).
- `RefreshRate()` returns a **fixed `.rodata` double** unconditionally (`:10317-10325`); it is used
  **only for effect frame-count math** (e.g. `maxShowFrames=(RefreshRate+duration-1)/RefreshRate`,
  `:10358,10374`), not to clock frames.
- Measured FPS is a debug print `"%f fps."` at end of every `EndScene` (`PrintFrameRate` `:1420`,
  called `:17858`) — confirming free-running-throttled-by-display, not a 16 ms timer.

**DRM/KMS output — GBM+EGL+DRM, class `NativeStateDRM` (glmark2 pattern; Rockchip DRM).**
Per frame `EndScene` (`:17841`): `glDisable(GL_DITHER)`→`glFlush`→`eglSwapBuffers`
(`:17847-17850`)→`NativeStateDRM::flip()` (`:17857`). `flip()` (`:46441`):
`gbm_surface_lock_front_buffer` (`:46458`) → wrap BO in a DRM FB cached per-BO (`fb_get_from_bo`
`:46397`, `gbm_bo_get/set_user_data` `:46408/46430`) → **first frame** `drmModeSetCrtc`
(`:46463-46465`); **later frames** `drmModePageFlip(..,DRM_MODE_PAGE_FLIP_EVENT,..)` (`:46475`) then
**blocks in `select()` on the DRM fd** (`:46515`) and `drmHandleEvent`→`page_flip_handler`
(`:46350/46507`), then `gbm_surface_release_buffer` (`:46511`).

- **Double-buffered** (two GBM BOs cycled; exactly one prior buffer retained), **vsync/page-flip
  locked** — the render thread stalls until the flip completes, which *is* the frame clock.
- FB format: `drmModeAddFB(..,depth=0x18,bpp=0x20,..)` = **XRGB8888, 24/32** (`:46420`).
- Wide-panel reshape: if `width>1920` it halves width / doubles height,
  `stride=(width/2)<<2` (`:46415-46418`) — folds very wide panels into two stacked halves.

**Composition = GLES2 textured quads, not QPainter.** `Render()` (`:18117`) per frame binds a program
(`glUseProgram` `:18325`), sets uniforms `nowTime`/fb-size/`playStatus` (`:18327-18337`), then
`BeginScene`→`RenderProgram` (per program `:18355/18368`)→`EndScene`. Textures uploaded via
`glTexImage2D` in `UpdateARGBToTextures`/`HLoadResource::LoadImageToTexture` (`:529`, `.txt:373`).
Source images are double-buffered at the compose layer via `HImageSurface`/`HDoubleImage`
(`.txt:68-69,231`). **Areas are stenciled by an index/mask texture (`s_textureIndex`)**, not CPU-clipped.

**Shaders** (full GLSL in `libMainWindowRender.so.txt:1348-1604`): one rotation-capable vertex shader
(`zRotation` mat4 for 90/180/270° panel rotation, `RotationArea()` per-area rotate, `.txt:1493-1555`)
+ ~6 fragment variants: plain blit; **alpha+dazzle**; **index-masked composite**; index+alpha;
alpha-only; and a **`samplerExternalOES`** variant for zero-copy video (`.txt:1488-1490,1556-1604`).
Shared helpers: full 6-sector **`HSV2RGB`** (`.txt:1348`) and **`GetDazzleColor`** — a 9-mode
(`Dazzle==1..9`) animated rainbow-sweep (`.txt:1397-1435`). Transitions layer on top via
`HEffectManager` (§3).

---

## 2. FPGA control protocol over /dev/ttyS1  ← the critical piece

Source: `sdk/FPGADriver/Trans/{FPGASerial,Serial}.cpp`, `Core/{SendCard,RecvCard,SystemEnv,HFPGACore}.cpp`.

### 2.1 Port
`fpga::HSystemEnv::HSerialParam` ctor (`libFPGADriver.so.c:16182-16190`) hardcodes:
`"/dev/ttyS1"`, `+0x100 = 0x1C200` (= **115200** baud), `+0x104 = 8` data bits, `+0x108 = 1`
stop bit, `+0x10c = 0x6E ('n')` **no parity** → **115200 8N1**. `HFPGACore::Init` can override the
name from config `sdk::HConfigParam::px30FPGASerial_` (`:70787`), default still `/dev/ttyS1`
(`.txt:1817`). Opened `open64(name, 0x102)` = **O_RDWR|O_NOCTTY** (`HSerial::OpenSerial` `:40465`),
then raw termios in `HSerial::InitSerial` (`:40525+`, `cfsetispeed` `:5156`).

### 2.2 Frame format (both directions)
`PacketBuffer` (`:39192`) builds every frame as:

```
+--------------------------+-------------------+---------------+
| 8-byte PREAMBLE (fixed)  |   PAYLOAD (N)     | CRC32 (4, LE) |
| 55 55 55 55 55 55 55 D5  |                   | over PAYLOAD  |
+--------------------------+-------------------+---------------+
    DAT_00193ec0                                CRC of the N payload bytes only
```

- `*(u64*)dst = DAT_00193ec0; memcpy(dst+8, payload, N); *(u32*)(dst+8+N) = CRC32(payload,N);`
  returns `N+0xC` (`:39205-39210`).
- **Preamble bytes = `55 55 55 55 55 55 55 D5`** (read at aarch64 file offset 0x93ec0). This is a
  classic Ethernet-style preamble: 7×`0x55` training + `0xD5` start-frame-delimiter. Same constant
  is the RX frame-sync (`memcmp(&DAT_00193ec0,8)` in `Read`, `:40235`).
- **CRC32** (`:39160`): table-driven **reflected** CRC-32, `init=0xFFFFFFFF`, per byte
  `crc = table[(crc^byte)&0xFF] ^ (crc>>8)`, `xorout=0xFFFFFFFF` — the standard IEEE-802.3/zlib
  algorithm (poly 0xEDB88320, table built in `InitCRCTable` `:38155`; **verify the poly against one
  live ttyS1 capture** before trusting it). CRC covers **payload only**, not the preamble.

### 2.3 Only two frame sizes exist
`SendData` rejects any payload length except **0x19 (25)** or **0x209 (521)** (`:40095`). So on the
wire there are exactly:

| Frame | Payload N | Total (8+N+4) | Use |
|---|---|---|---|
| **Control** | 25 = 0x19 | 37 = 0x25 | short commands / status / readback asks |
| **Param** | 521 = 0x209 | 533 = 0x215 | bulk config (scan table, gamma, geometry, card params) |

RX parser `Read` (`:40189`) accumulates into a 0x429-byte buffer, scans for the preamble, then
dispatches on **payload byte [0]** (`__s1[8]`): `1`→control response (37 B)→`ParseControllFrame`;
`3`→param response (533 B)→`ParseRespondParamFrame`; else "invalid data type" (`:40239-40275`).

### 2.4 Payload sub-header (9 bytes) + data
Both frame types share this layout (offsets are **within the payload**, i.e. frame offset − 8;
derived from `SendSearchRecvCardAsk` `:14432-14446` and `GenSendCardParamAsk` `:15862-15875`):

| Off | Size | Field | Values seen |
|---|---|---|---|
| 0 | u8 | **target type** | send: `1`=receiving-card, `2`=send/main-card. resp: `1`=control, `3`=param |
| 1 | u8 | card index / address | `this[0x28]` card id, or recv-card number |
| 2 | u8 | phy / sub-index | phy channel for recv-card ops (0 default) |
| 3 | u16 LE | **function code** | see table §2.5 |
| 5 | u16 LE | param / offset | e.g. flash offset |
| 7 | u16 LE | **data length** | `0x10` for control data, `0x200` (512) for param |
| 9 | N−9 | data | 16 B (control) or 512 B (param) |

So control = 9-B header + 16-B data = 25; param = 9-B header + 512-B data = 521. Exact.

### 2.5 Function codes (payload[3:4], u16 LE)
From the builders (`RecvCard.cpp`/`SendCard.cpp`), each ending in `SendData(...,0x19|0x209)`:

| Code | Frame | Command (fpga::…) | line |
|---|---|---|---|
| `0x0100` | ctl | `HRecvCard::SendSearchRecvCardAsk` | 14442 |
| `0x0100` | param | `HRecvCard::GenRecvCardBasicParamAsk` | 14939 |
| `0x0200` | ctl | `HRecvCard::SendReadBackRecvCardParamAsk` | 14545 |
| `0x0200` | param | `HRecvCard::GenHighRefreshScanTableAsk` | 15000 |
| `0x0300` | param | `HRecvCard::GenGammaTableAsk` | 15060 |
| `0x0300` | param | `HRecvCard::GenSaveParamToRecvCardAsk` | 15375 |
| `0x0400` | param | `HRecvCard::GenLocusIndexTableAsk` (brightness/locus LUT) | 15120 |
| `0x0400` | resp | control-status response (`ParseControllFrame` expects `0x400` at [3]) | 39479 |
| `0x0500` | param | `HRecvCard::GenRecvCardRangeAsk` (geometry) | 15203 |
| `0x0500` | ctl | `HSendCard::SendReadTempAsk` | 15661 |
| `0x0600` | ctl | `HSendCard::SendCheckHDMISignalAsk` | 15702 |
| `0x0700` | param | `HRecvCard::GenSinglePhyParamChangeAsk` | 15315 |
| `0x1000` | ctl | `HRecvCard::SendLockLastFrameAsk` | 14682 |
| `0x1100` | ctl | `HRecvCard::SendUnlockLastFrameAsk` | 14727 |
| `0x0000` | param | `HSendCard::GenSendCardParamAsk` (global send-card param incl. brightness) | 15864 |
| `0x00FF` | ctl | `HSendCard::SendReadBackSendCardStatusAsk` | 15530 |
| `0x23CC` | ctl | `HSendCard::SendEraseSPIFlashAsk` | 15572 |
| `0x34D2` | ctl | `HSendCard::SendReadBackSPIFlashAsk` | 15616 |

`ParseControllFrame` (`:39451`) validates a control **response**: total len 0x25, CRC over the 25-B
payload (`:39472`), sub-function `[3]==0x400` (`:39476`), status byte `[9]==0xAA` (offset 0x11)
= OK; `[10]==0x99` (0x12) means HDMI present (`SetHDMIStatus`, `:39480-39505`).
`ParseRespondParamFrame` (`:39517`): total 0x215, CRC over 0x209 payload.
Respond callback types 1/2 = "just status", 3/4/5 = "status + frame body" (`Respond` `:39377`,
`SetRespondType`/`OneFrameFinish`).

### 2.6 What the 521-byte param frames carry — the Huidu secret sauce
The **512-byte data blob** in a param frame is the receiving-card / send-card configuration. Its
contents come from large **const lookup tables baked into libFPGADriver**, selected per driver-IC and
per user "priority" preference. `HTool::CalcHighRefreshTable` (`:21980-22200`) picks a row from one of:

- `s_dualScanTab` (`:22087`) — scan tables, indexed by IC type + scan mode
- `s_lightPriority` (`:22108`) / `s_refreshPriority` (`:22100`) / `s_grayPriority` (`:22114`) —
  per-IC timing tables chosen by **PriorityMode** `param[0xb]`: **0 = brightness-first, 1 =
  refresh-rate-first, 2 = grayscale-first** (matches `CHPriorityMode<3`).

Selection keys: IC model `param_1[0x31]` (range `0x10..0x80`), scan lines `param_1[0xc]`,
`param_1[0xd]`; rows are 0x200/0x400 bytes and are copied byte-swapped into the param buffer at
offset 0x428+ (`:22135-22200`). Also present: gamma table (`GenGammaTableAsk`), color-correction
(`_HColorCorrection`, `:9406`), locus/brightness index LUT (`GenLocusIndexTableAsk`).

**Config value domains** (validators `HConfigParam::CH*`, `Config/ConfigParam.cpp`):
`LuminanceLevel < 3` (`:10244`), `ClockFrequency < 0x10` (16 steps), `PriorityMode < 3`,
`RGBGroupType ∈ {0, …}`, **`Gamma 1.0..6.0`** float (`CHGamma` `:10300`),
`ColorCorrection 0..100` (`<0x65`, `:10320`).

**Global brightness** originates in program/hardware XML (`HOldSendCardParam::ParseBrightness`
reads tags `"Brightness"`/`"Value"`/`"BrightnessSetMode"`, `:23966,26281`), is packed by
`HFPGAParam::UpdateFPGASendCardParam` (`:11990`) into the 512-B send-card blob returned by
`GetFPGASendCardParam` (`:12199`), and shipped by `GenSendCardParamAsk` (func code `0x0000`,
target `2`, datalen `0x200`). So **brightness/on-off/gamma are not individual opcodes** — they are
fields inside the periodic 512-B send-card / recv-card param frames.

### 2.7 Init sequence
`HFPGACore::Init` (`:70763`): resolve serial name (config `px30FPGASerial`, default `/dev/ttyS1`)
→ `SetSerialName` → create singletons `HFPGAMonitor`, `HFPGACoreHelper`, `HFPGAMsgList`.
`HFPGASerial::Init` opens+configures the port and installs the RX callback; `HFPGACore::InitFPGAVersion`
(`:71022`) queries FPGA version. At boot, **`BootLogo` also drives ttyS1** ("Write to FPGA"/"Read from
FPGA", same CRC framing — BootLogo strings) to show the splash before BoxPlayer starts. Typical
runtime order after power-on: load bitstream (§2.8) → open ttyS1 → search receiving cards
(`0x0100`) → push basic param / scan table / gamma / range / locus (param frames) →
`GenSaveParamToRecvCardAsk` (`0x0300`) → periodic status readback (`0x00FF`, temp `0x0500`, HDMI
`0x0600`) and lock/unlock-frame around updates (`0x1000`/`0x1100`).

### 2.8 Bitstream load (separate from ttyS1) — for completeness
FPGA gateware `/boot/fpga.img` is loaded over **SPI passive-serial** via kernel `drivers/char/cyclone4.c`
→ **`/dev/cyclone4`**, using the external helper `write_fpga <img> /dev/cyclone4`
(`libFPGADriver.so.c:~68547`; ioctls `CYCLONE4_IOC_INIT 0x4302`, `RD_CONF_DONE 0x80044301`,
`:66959-66969`). Details in hardware/PX30_C_SERIES_HARDWARE.md §4. The `.img` is a proprietary
container (8-B header + `0xFF` pad + `CC 55 AA 33` sync), **not** a stock Altera RBF — treat it as an
opaque blob to copy through as-is.

---

## 3. Program / content model (libXmlSDK + plugins)

**Managers** (`libXmlSDK.so`): `sdk::HMProgram` (root `<program>`, `AddProgram`/`SwitchProgram`/
`InsertPlayProgram`, temp `/tmp/hdTempProgram/program.boo`, `.txt:2022-2053`),
`sdk::HMPlayList` (root `<playLists>`→`<playList updateTime version>`, `.txt:2757-2765`),
`sdk::HMFont` (root `<fonts>`, faces in `/usr/lib/fonts/` and `/root/Box/project/sdk/font/`).

**Rendered geometry** — every content node serializes via `Hd::HFrameNode::CoverToSDKFormate`
(`libhcommon.so.c:93892`) to:
```
<program><area guid alpha>
  <rectangle x y width height/>
  <resource …/>              ← plugin-specific payload
</area>…</program>
```
Internal editor attrs: `X Y Width Height Alpha __GUID__ HoldTime playMode` (`libhcommon.so.c`).

**Scheduling** — two systems: (a) **intercut/timed** on the program element
(`HMProgram::InsertPlayProgram` `:73925-74200`): `InsertMode/InsertTimeMode/InsertPlayCount/
InsertDuration/InsertInterval/InsertAllDay/InsertPeriodicRules/InsertDaysOfWeek/InsertDaysOfMonth/
InsertStartTime|EndTime (hh:mm:ss)/InsertStartDate|EndDate (yyyy-MM-dd)`; runtime log
`"play mode[%d] timemode[%d] maxPlayCount[%d] Duration[%d] RepeatPolicyMode[%d]"` `:74194`).
(b) **Screen on/off weekly timer** `sdk::HMScreenOnoff` (tags `week/weekItem/item/openAllDay/start`,
`libCore.so.c:158416`) — display power, not per-program.

**Plugins** (Qt plugins; `INodePlugin` with `type()`/`PluginMatch(tagName)`/`Create`/
`CreateAreaRender`; dispatched by `Hd::PlayerFactory::createPlayer`):

| type() id | tag | content |
|---|---|---|
| HD_OrdinaryScene_Plugin | `program` | scene/area container + audio (`Volume`,`BgImage`) |
| HD_Video_Plugin | `video` | video + `rtsp://`; `PlayTimes,aspectRatio,HDTranscoding,VideoQuality,FileMd5` |
| HD_Photo_Plugin | `image` | image/GIF; `FillType(fill/fit/center/stretch/tile),KeepRatio,pageCount/pagetime` |
| HD_Text/SingleLineText/animationText | `text`/`singleLine`/`animationText` | static / scroll / animated text; `Speed,PlayType,EnableTTS,EffectType` |
| HD_Clock/CALENDAR/Time_Plugin | `clock` … | analog+digital clock, calendar, time/date |
| HD_Weather/Temperature/Humidity/Sensor_Plugin | — | weather + sensor readouts (WenQuanYi CJK font) |
| HD_Neon/Frame_Plugin | `neon` | decorative borders/glow |
| HD_TABLE/MODBUS_Plugin | `Table` | grid/register tables (row/col/merge) |
| HD_DynamicData/NetworkData/RDM_Plugin | `dataSource`/`networkData` | dynamic/HTTP data text |
| HD_Document/WPS_Plugin | `Office`/`Page` | office pages → images |
| HD_EWATCH/Text3D_Plugin, HD_Controller_Plugin | `EWatch` … | countdown, 3-D text, screen descriptor |

**Effects/transitions** — `sdk::HEffectManager` (`libMainWindowRender.so.c:25291-25446`), ids `0..0x23`:
`HImmediate, HParalleMove×4, HRectCovert×4, HSlantCovert×4, HDivide×2, HClose×2, HFade, HShutter×2,
HRandom, HTSeriesMove×4, Flicker, RotationEffect, DoorEffect×2, CenterEffect×2`; step/duration from
`GeteffectStep(effect,speed)`. "Dazzle" is the separate 9-mode GLSL layer (§1).

**Formats** — video via FFmpeg/GStreamer (no hard-coded codec whitelist → H.264/H.265/MPEG-4/etc.);
images via Qt `QImage` (JPG/PNG/BMP/GIF, GIF explicitly, animated by `pageCount/pagetime`); fonts via
Qt `QFont`/TrueType (WenQuanYi, Arial, simsun). Documents rendered to page images.

---

## 4. Decode / GPU load

**Video decode = FFmpeg software** on the 4×Cortex-A35. The `libvideo_plugin` is only a scene-node
wrapper (zero decode calls); the real pipeline is `VideoDecodeThread` in **libhcommon.so**:
`avcodec_find_decoder(codec_id)` (**by integer ID = generic software decoder**, `libhcommon.so.c:208404`),
`avcodec_open2` (`:208413`), `av_read_frame` (`:212331`), `avcodec_send_packet`/`receive_frame`
(`:207467/207490`). **No `avcodec_find_decoder_by_name`, no `h264_rkvdec`/`rkmpp`/`MppCtx`/`hantro`/
`mediacodec`, no `gst_*` call sites** — those library names in `strings` are just `DT_NEEDED`
boilerplate common to every `.so`. The Rockchip VPU is **not** used for decode.

**YUV→RGB + scale = hardware librga (RGA 2D).** `ShowImage` branches on pixel format
(`libhcommon.so.c:~212640`): YUV420P → `RockchipRga::RkRgaBlit` (`:212749`, also
`libMainWindowRender.so.c:47579/47973/50243`, `RkRgaInit` `:48309`); other formats → software
`sws_getContext`/`sws_scale` fallback (`:212757/212782`). GIF/images = Qt software
(`HGifPlay`/`HQGifReader` `:2856/2052`, `QImageReader::read` `:1997`, `ImageConverter::DoConvert`
`:2725`).

**Cost profile (C-series ≤1024×1024):** SD/≈720p H.264 at modest bitrate decodes comfortably in
software because pixel counts are tiny and RGA absorbs the color-convert/scale. **1080p+/high-bitrate
source is the failure mode** — software A35 decode drops frames, and there is *no* VPU fallback.
`HVideoNode::SetHDTranscoding` (`plugins_libvideo_plugin.so.c:540`) shows the toolchain expects
**host-side pre-transcoding** of HD content. Compose/GLES2 and the RGA blits are cheap.

---

## 5. SoC-generic vs Huidu-proprietary (the exact replacement surface)

**Standard, reuse unchanged (mainline Rockchip Linux 4.4 / Mesa / FFmpeg):**
- KMS/GBM/EGL/GLES2 compose + `drmModeSetCrtc`/`drmModePageFlip` double-buffered scan-out at
  XRGB8888 — any KMS client that drives `/dev/dri/card0` with the panel's mode works.
- FFmpeg software decode; librga (`RkRgaBlit`) for YUV→RGB/scale; Qt raster for images.
- The parallel-RGB888 VOP→FPGA electrical path (kernel/DTS `rgb` node, panel timing).

**Huidu-proprietary, must be re-implemented or reused:**
1. **`/dev/ttyS1` control protocol (§2)** — 115200 8N1; preamble `55×7 D5`; payload-only reflected
   CRC-32; two fixed frame sizes (25/521); the 9-byte sub-header (target/index/phy/funccode/param/len)
   and the function-code set (search/basic-param/scan-table/gamma/locus/range/save/lock/status/temp/
   HDMI). This is what makes the panel actually display and hold brightness/gamma.
2. **The per-IC scan / gamma / gray / refresh / light-priority tables (§2.6)** — bulk const data
   inside libFPGADriver, packed into the 512-byte param blobs. Either **link the stock
   `libFPGADriver.so`** (simplest, keeps every IC/panel profile) or dump the tables and reproduce the
   `CalcHighRefreshTable` selection. These cannot be guessed; they are panel-vendor timing.
3. **`/boot/fpga.img` bitstream + `/dev/cyclone4` SPI loader + `write_fpga`** — proprietary container
   and Altera-passive-serial load; copy through as opaque blobs (§2.8).
4. **Program/playlist XML model + content plugins (§3)** — only needed if you want to stay
   HDPlayer/BoxSDK-compatible; a from-scratch player can define its own scene format and just render
   to the CRTC.

**Bottom line for a replacement player:** keep the KMS/GLES/FFmpeg/RGA stack, **relink or wrap the
stock `libFPGADriver.so`** (or re-emit its ttyS1 frames byte-for-byte using §2), keep `fpga.img` +
`write_fpga` + the `cyclone4`/spidev driver, and supply the panel's own KMS mode. Everything else
(scenes, scheduling, plugins) is replaceable application code.

> **Open items to confirm on a live C-unit:** the CRC-32 polynomial (almost certainly 0xEDB88820 —
> capture one ttyS1 frame), the exact 512-byte send-card/recv-card blob field offsets (brightness,
> gamma, RGB order), and the C15/C35/C36 panel KMS timing (the D15 `simple-panel` 1024×128@60 is not
> the C-series mode).
