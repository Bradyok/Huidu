# Driver Enablement — "will it really work?"

The honest answer for the PX30 C-series (C15/C35/C36) on our mainline-6.18
Buildroot image: **the standard SoC drivers are all in mainline and already
enabled; the LED-specific pieces are custom and are the real work.** None of the
custom pieces need Rockchip's closed 4.4 BSP — they are either (a) covered by a
mainline driver, (b) small userspace we write, or (c) a factory blob we keep.

Legend: ✅ in mainline + enabled · 🔨 we build/port it · 📦 keep factory blob ·
❓ must dump from a live C-unit first.

## 1. Base platform — ✅ all mainline, enabled in `linux.fragment`

| Function | Stock (4.4 BSP) | Our mainline driver | Status |
|---|---|---|---|
| CPU / SMP / PSCI | px30 | `ARCH_ROCKCHIP` | ✅ |
| eMMC | dw_mmc-rockchip | `MMC_DW_ROCKCHIP` | ✅ |
| UART (console ttyS2, FPGA ctrl ttyS1) | 8250_dw | `SERIAL_8250_DW` | ✅ |
| I²C | i2c-rk3x | `I2C_RK3X` | ✅ |
| PMIC (RK809) | rk8xx | `MFD_RK8XX_I2C` + `REGULATOR_RK808` | ✅ |
| Ethernet MAC | rk_gmac / stmmac | `DWMAC_ROCKCHIP` | ✅ |
| USB host/OTG | dwc2 + inno-usb2 phy | `USB_DWC2` + `PHY_ROCKCHIP_INNO_USB2` | ✅ |
| RTC (PCF8563) | rtc-pcf8563 | `RTC_DRV_PCF8563` | ✅ |
| Watchdog | dw_wdt | `DW_WATCHDOG` (+ `HANDLE_BOOT_ENABLED`) | ✅ |
| ADC (board-rev strap) | rockchip_saradc | `ROCKCHIP_SARADC` | ✅ |
| Thermal | rockchip_thermal | `ROCKCHIP_THERMAL` | ✅ |
| cpufreq | — | `CPUFREQ_DT` | ✅ |
| Display controller (VOP) | rockchip drm/vop | `DRM_ROCKCHIP` + `ROCKCHIP_VOP` | ✅ |
| Parallel RGB output | rockchip,px30-rgb | `ROCKCHIP_RGB` | ✅ |
| GPU (Mali-G31) | libmali (closed) | `DRM_PANFROST` (open) | ✅ *(not required by the player — see §5)* |
| Video decode | Hantro (rk vpu) | `VIDEO_HANTRO_ROCKCHIP` | ✅ |

Base bring-up (boot to shell, network, storage, a KMS device) needs nothing we
don't already have.

## 2. LED datapath — the part that actually matters

The image reaches the panel as VOP **parallel RGB888**, latched by the LED FPGA.
Config/status ride `/dev/ttyS1`. Three custom stock pieces sit here:

| Stock piece | What it is | Our approach | Status |
|---|---|---|---|
| `drivers/char/cyclone4.c` → `/dev/cyclone4` | out-of-tree FPGA passive-serial (PS) loader over SPI0 | **reuse mainline `altera-ps-spi`** (`altr,fpga-passive-serial` DT node, already wired) — same PS protocol, does the bit-reversal itself | ✅ driver / ❓ confirm it accepts this bitstream |
| `/root/Box/System/write_fpga` | closed userspace that clocks `/boot/fpga.img` into `/dev/cyclone4` | **`huidu-sender strip-fpga` + `load-fpga`** (new): parse the 8-byte Huidu wrapper, load via the kernel driver's firmware path, or via spidev+gpio as a fallback | 🔨 built (offline core tested; wire load ❓) |
| `/boot/fpga.img` (6.6.0.0) | proprietary Cyclone-IV bitstream (`6C031646` hdr + PS stream) | **keep the factory blob** — never regenerated; we only strip the wrapper | 📦 |
| ttyS1 control (scan/gamma/brightness) | `libFPGADriver.so` | **`huidu-sender`** (new crate) | 🔨 built + tested |

**FPGA load bit-order (❓):** Altera PS is LSB-first; `altera-ps-spi` and our
`load-fpga` bit-reverse each byte by default. If a live unit configures only
with `--msb-first`, flip it. This is the single most likely thing to need a
tweak on first hardware.

## 3. Possibly-custom nodes — ❓ dump from a C-unit before trusting

Seen in `libFPGADriver` but **absent from the D15 kernel** (so likely other
models / C-series-specific, or vestigial):

| Node | ioctls | Purpose (from RE) | Plan |
|---|---|---|---|
| `/dev/mdio_gpio` | `0xc0045702/04` | `ConfigPhyTimer` bit-bangs a PHY on some HW configs | mainline `MDIO_GPIO` exists but presents a bus, not this char dev — **check if any C-unit opens it**; if so, small shim or route through phylib |
| `/dev/audio_switch` | `0xc0046a02/03` | `OpenFPGALedLamp` on/off (LED lamp power enable) | looks like one GPIO — **model as a `gpio` / regulator in DT** once the pin is known |

Neither is on the pixel path; missing them ≠ dark panel. Resolve after first light.

## 4. Networking radios — ❓ confirm the exact part

| Function | Stock | Our config | Risk |
|---|---|---|---|
| Wi-Fi | `rtl8188eu` (built-in, per kernel strings) | `RTL8XXXU=m` (covers RTL8188EU on 6.x) | ⚠️ The real chip is read from `/etc/hardware.conf` `wifi=`. If a C-unit ships a different module (8723/8821/8189), `rtl8xxxu` may not cover it → **grab the matching driver** (possibly out-of-tree) |
| 4G modem | Quectel EC200T (USB) | `USB_SERIAL_OPTION` + `QMI_WWAN` + `CDCETHER` | ✅ standard USB; confirm VID/PID |

**Action:** read `/etc/hardware.conf` and `lsusb`/`dmesg` from each C-unit; that
tells us the exact Wi-Fi part and whether we need an extra driver.

## 5. GPU is optional (de-risks the whole thing)

The player (`boxplayer`) renders with **tiny-skia (CPU)** and scans out via
**DRM/KMS**. It does **not** need GLES/EGL/libmali. So Panfrost is a bonus, not a
dependency — even if Panfrost had issues on PX30, the wall still lights. This is
why we don't need Rockchip's closed `libmali` at all.

## 6. So: do we have all the drivers?

- **To boot, network, and get a KMS framebuffer:** yes, 100% mainline, already
  enabled.
- **To light the LED wall:** we have the *driver* (`altera-ps-spi`) and now the
  *loader* (`huidu-sender strip-fpga`/`load-fpga`) and the *control daemon*
  (`huidu-sender`). What we still need is **hardware to confirm** three things:
  FPGA-load bit-order, the C-series **panel timing** (placeholder is D15's
  1024×128@12 MHz), and the 512-byte scan/gamma **blob** (capture from a stock
  unit — the daemon frames it but can't synthesize it).
- **To match every peripheral:** confirm the **Wi-Fi chip** (may need an extra
  driver) and check whether any C-unit uses `/dev/mdio_gpio` / `/dev/audio_switch`.

Nothing here requires the closed Rockchip BSP or closed Huidu drivers. The
remaining items are **captures**, not ports.

## 7. Hardware-dump checklist (run on the first C-unit)

```sh
# identity / model / hw config
cat /root/Box/data/id; cat /etc/hardware.conf; cat /proc/device-tree/model
# the C-series DTB (panel timing, GPIOs, PHY, extra nodes)
cat /proc/device-tree/... # or pull the RSCE resource from the boot partition
# which custom nodes actually exist here
ls -l /dev/cyclone4 /dev/mdio_gpio /dev/audio_switch /dev/spidev* /dev/ttyS* /dev/dri
# radios
lsusb; dmesg | grep -iE 'rtl|8188|8723|8821|wifi|mmc|quectel'
# the FPGA blob + loaders + factory scan config
cp /boot/fpga.img /root/Box/System/write_fpga /root/Box/System/clear_fpga /tmp/
# capture two ttyS1 param frames at different brightnesses (find the offset)
# capture the send-card/recv-card param frames (the 512-byte scan/gamma blobs)
```

See also `PX30_C_SERIES_HARDWARE.md` (evidence) and `../../huidu-sender/README.md`
(the daemon + loader).
