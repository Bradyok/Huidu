# Our own OS on Huidu C15 / C35 / C36 (Rockchip PX30)

Goal: replace Huidu's stock Linux + BoxPlayer on C15/C35/C36 controllers with our own OS and software,
while keeping the LED output working.

Inputs: `BOXPLAYER_DECOMPILATION.md` (Operating System section),
`products/BoxPlayer/v7.11.18.0/hardware/PX30_C_SERIES_HARDWARE.md` (board, FPGA, partitions, risks),
`products/BoxPlayer/v7.11.18.0/decompiled/` (libFPGADriver, BootLogo, BoxDaemon logic).
Research date: 2026-09-23.

## What we are replacing

| Layer | Stock (7.11.18.0) |
|---|---|
| Boot | Rockchip U-Boot (version unknown), Android boot image v? on `boot` + DTB in an RSCE resource in the second stage |
| Kernel | Rockchip BSP **4.4.159** (Linaro GCC 6.3, 2020), built-in `cyclone4` FPGA driver, fiq-debugger console |
| Rootfs | Rockchip-SDK **Buildroot 2018.02**, BusyBox init, glibc, root on `userdata` (ext4); `oem` holds the MAC |
| App | `/root/Box`: BoxDaemon (watchdog + supervisor), BootLogo, BoxPlayer (Qt5, GLES2 on DRM/KMS), BoxSDK (HDPlayer protocol) |

### Stock graphics / media stack (all Rockchip vendor pieces)

| Function | Stock driver (kernel 4.4.159) | Stock user space | Mainline replacement |
|---|---|---|---|
| GPU (Mali-G31 Bifrost) | Arm **mali_kbase r8p0-01rel0** (closed-model vendor driver; DT node still says `arm,malit602…midgard`) | Rockchip **libmali** blob (GBM flavor) providing `libEGL`/`libGLESv2`/`libgbm` — on the rootfs, not in the upgrade package | **Panfrost** + Mesa (GLES 3.1) |
| Display | Rockchip vendor DRM (`px30-vop-big`/`-lit`, RGB; LVDS/DSI off) | `libdrm`, BoxPlayer does modeset + page-flip itself | Mainline `rockchipdrm` VOP + `panel-dpi` |
| 2D blit/scale | **RGA2** (`rockchip,rga2`) | `librga` (`RgaBlit`, used across Huidu libs) | Not upstream for PX30 — do it in GLES |
| Video decode | `vpu_service`/`mpp_service` (+`rkvenc`) | Present, but BoxPlayer decodes with **FFmpeg 57 in software** (`avcodec_find_decoder` + `sws_scale`); GStreamer is linked but no calls found | Hantro V4L2 stateless (H.264/VP8/MPEG-2) via GStreamer/FFmpeg v4l2-request, or keep software decode |
| UI toolkit | — | Qt 5 (`-platform offscreen`), GLES2 | anything on KMS/GLES |

Every Huidu library links the same full set (Qt5, GLES2, drm, gbm, rga, GStreamer, FFmpeg), so linkage says little
about use; the table reflects the decompiled calls. The C-series panels are small (HDSet `device.xml`: C15 max
384×320 @60 Hz / 1024×512 @30 Hz, C35 1024×512, C36 1024×1024), so GPU and decode demands are light.

## Why this is feasible

The LED path is ordinary hardware we can drive from any Linux:

1. **Pixels** = parallel RGB888 from the PX30 display controller (VOP) into the FPGA. Any DRM/KMS player works —
   NovaOS's `nova-display-player` already renders GLES onto KMS. We only need the panel timing in the DTS.
2. **Control** = UART1 `/dev/ttyS1` 115200 8N1, CRC-framed. The protocol is in `libFPGADriver` (decompiled, with
   symbol names: `fpga::HSendCard`, `HRecvCard`, scan/gamma/brightness params).
3. **Bitstream** = SPI0 + three GPIOs (nCONFIG/nSTATUS/CONF_DONE) — the classic Altera passive-serial setup.
   Mainline Linux already has an FPGA-manager driver for exactly this (`drivers/fpga/altera-ps-spi.c`), or we can
   do it from user space with spidev + GPIO. We reuse Huidu's `/boot/fpga.img` as a blob; its container format
   still needs decoding (it is not a plain `.rbf`).

Upstream PX30 support is good: mainline Linux has VOP/RGB/LVDS/DSI, GMAC, eMMC, USB, I2S, thermal, cpufreq,
Panfrost (Mali-G31, GLES 3.1) and Hantro decode (H.264/VP8/MPEG-2). Missing upstream: RGA and DDR devfreq.
Mainline U-Boot + TF-A support PX30 with an open DDR init (DDR3 proven; other DRAM types pending).

## Recommendation: modern Buildroot + mainline kernel + Panfrost

Use **Buildroot 2026.02 LTS** (or 2026.08 latest) as an external tree (`BR2_EXTERNAL`), starting from the
upstream `engicam_px30_core_defconfig` (already mainline Linux + TF-A `PLAT=px30` + U-Boot + genimage, no
Rockchip blobs). Why Buildroot fits this family better than a NovaOS/Yocto lane:

- The job is small and fixed: one SoC, three sibling boards, a KMS player, a serial daemon, an FPGA loader.
  Buildroot builds that as one reproducible image in well under an hour on any Linux box or WSL.
- Same kind of system Huidu ships (Buildroot), so their init order and scripts map 1:1 — just modern versions.
- Everything we need is upstream: Linux (VOP/RGB, GMAC, eMMC, USB, Panfrost, Hantro), Mesa Panfrost, FFmpeg,
  GStreamer, `rauc`/`swupdate` for A/B updates, dropbear/openssh, ModemManager for the EC200T.
- NovaOS daemons are static musl Rust binaries, so the ones that are generic (`nova-display-player`, telemetry,
  mgmt gateway) can still be dropped into the Buildroot image as prebuilt packages. We lose NovaOS's
  systemd unit set and bundle tooling, which is the part that costs the ~12-file per-model plumbing.

Pick NovaOS instead only if these units must be managed by the same fleet tooling/update pipeline as the
NovaStar units; the **Colorlight A200** lane (stock U-Boot, Android bootimg, KMS player + UART control daemon)
is the template for that.

Kernel: **mainline (7.2.x / 6.18 LTS) with Panfrost.** The vendor stack (4.4 + mali_kbase r8p0 + libmali) cannot
be carried forward; Rockchip's newer BSPs still list `px30.dtsi` but their libmali pin for NovaOS is a G52 blob.
Bootloader: **keep stock U-Boot first** (Android bootimg v1 + RSCE DTB); switch to mainline U-Boot + TF-A once
the DRAM type is known and maskrom recovery is proven on the bench.

## Phases

### 0. Bench unit and a full backup (blocking)
- One C15 (then C35, C36). Serial console: UART2 (GPIO1_D2/D3, 115200, `ttyFIQ0`) — find the header/pads.
- Root access: SSH is on with `PermitRootLogin yes`, **password unknown**. Alternatives: serial console, or a
  crafted Huidu upgrade package (`upgrade.sh` runs as root — our own `.bin` via HDPlayer is the easiest way in).
- Dump every eMMC partition (`dd` over SSH, or `rkdeveloptool rl` in rockusb/maskrom). `oem` = MAC; keep per unit.
- Find the maskrom entry (pad / eMMC CLK short) and confirm `rkdeveloptool` sees `2207:330d` over USB OTG.

### 1. Capture each C model (the package only carries D15's kernel)
Run the dump list in `PX30_C_SERIES_HARDWARE.md` §8 on each of C15/C35/C36: `/proc/device-tree` (→ DTS),
cmdline, `/proc/partitions` + by-name map, `/boot/fpga.img`, `write_fpga`, `clear_fpga`, `/etc/hardware.conf`,
`modetest` output (RGB timing), `dmesg`, watchdog state, `/dev/mdio_gpio` and `/dev/audio_switch` users.
Also get `PX30_BoxPlayerD15_RC.tar.gz` (C36 package) from the full `BoxPlayer_7_11_18_0.bin`.
Store under `models/huidu-c15/` etc. in NovaOS (`reference_stock/`, `captured-partition-map.json`, `*-stock.dts`).

### 2. Kernel + DTS
- Board DTS on mainline `px30.dtsi`, starting from `px30-evb.dts` / Theobroma ringneck, pinned to the stock DTB:
  RK809 PMIC, PCF8563 RTC, RMII GMAC, eMMC, USB (rtl8188eu wifi, Quectel EC200T 4G), UART1, UART2 console,
  SPI0, watchdog, VOP → RGB `panel-dpi` with the captured timing.
- FPGA load: `altr,fpga-passive-serial` node on SPI0 (verify pin roles and bit order), or a small user-space
  loader. Decode the `fpga.img` container first (compare against what `write_fpga` sends — strace/logic analyzer).
- Boot through stock U-Boot: Android header v1 + gzip kernel + RSCE DTB (`make-boot-img.sh` A200 case),
  try-boot from `recovery` before touching `boot`.

### 3. Buildroot external tree
`br2-huidu/` (`BR2_EXTERNAL`): `configs/huidu_c15_defconfig` (+ c35, c36) derived from
`engicam_px30_core_defconfig`; `board/huidu/px30/` with the DTS files, kernel config fragment (Panfrost,
rockchipdrm RGB, `altera-ps-spi`/spidev, dw watchdog, rtl8xxxu or rtl8188eu, qmi_wwan/option for EC200T),
`post-image.sh` producing an Android v1 boot image with RSCE DTB for stock U-Boot, genimage/rauc config;
`package/` for our daemons (and prebuilt NovaOS static binaries if reused). Userland: BusyBox init, Mesa
Panfrost, libdrm, FFmpeg, openssh/dropbear, chrony, ModemManager. C35/C36 differ only by DTS, timing, FPGA image.
(NovaOS alternative: a `huidu-c15` lane per the A200 checklist — `models.toml`, `nova-px30.inc`, packaging,
services, and the per-model allow-lists.)

### 4. Our software on it
- `huidu-fpga-load` (boot unit): load `fpga.img`, check CONF_DONE.
- `huidu-sender` (daemon): the ttyS1 protocol ported from libFPGADriver — init, scan/gamma/brightness, status —
  and feeding `/dev/watchdog`. Replaces BootLogo + BoxDaemon's hardware duties.
- `nova-display-player` to KMS for content; our management instead of BoxSDK/HDPlayer (optionally a BoxSDK-
  compatible listener later so HDPlayer still works — the protocol is already in `huidu-protocol/`).

### 5. Updates and field install
- First install on fielded units: our own Huidu-format upgrade package (HDPLAYER header + tar.gz, see
  `ZBIN_Firmware_Analysis.md`) whose `upgrade.sh` writes our boot image and rootfs — no case opening.
  Only after the bench flow is proven and restorable.
- After that: NovaOS signed bundles (`nova-fwinstall`); A/B needs a partition plan (A200 has none today).

## Open questions / risks

| Item | Why it matters | How to close |
|---|---|---|
| Root password / shell | No bench access without it | Serial console, or our own upgrade package |
| C-series DTB and RGB timing | D15's DTB is not C-series | Phase 1 dump |
| `fpga.img` format, pin roles, bit order | LED stays dark without a loaded FPGA | Trace `write_fpga`; logic analyzer on SPI0 |
| ttyS1 protocol coverage | Brightness/scan/gamma | libFPGADriver + BootLogo decompile, serial capture of stock unit |
| Watchdog armed at boot? | Reboot loop on a new OS | Check on bench; feed it in `huidu-sender` |
| U-Boot version, secure boot | Boot image format constraints | Serial log of stock boot |
| DRAM type | Only for mainline U-Boot TPL | Chip marking / stock loader |
| Maskrom access | Unbrick path | Find pad, test before first flash |
