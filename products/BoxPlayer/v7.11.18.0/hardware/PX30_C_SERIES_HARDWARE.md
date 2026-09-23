# PX30 C-Series (C15 / C35 / C36) Hardware and Boot Reference

Goal: the facts we need to run our own Linux on Huidu PX30 controllers. The target models are **C15, C35 and C36**.
The only boot image we have is the **D15** one (`firmware_extract/PX30_D15/kernel_d15_ec200T.img`, package 7.11.18.0), so this report keeps three kinds of fact apart:

- **[PX30]**: true for the whole PX30 family (SoC, user space, scripts, and libraries shared by every model).
- **[D15]**: read from the D15 kernel/DTB only. It is the closest reference, but it has **not been confirmed on C15/C35/C36**.
- **[UNKNOWN]**: we have to dump it from a live C unit (see section 8).

Files in this folder:

| File | What it is |
|---|---|
| `D15_rk-kernel.dtb` | DTB from the RSCE resource inside the D15 boot image (90,111 B, sha256 prefix `aa8afcceeeaf7176`) |
| `D15_rk-kernel.dts` | The same DTB decompiled with python `fdt` (phandles are left numeric; line numbers below point into this file) |
| (no kernel config) | The kernel has **no IKCONFIG**: no `IKCFG_ST` marker, and no `/proc/config.gz` support was found |

The raw kernel `Image` (14.3 MB) and the logos are kept in scratchpad `.../scratchpad/boot/` and are not committed.

---

## 1. Why the D15 image is not the C-series image

| Evidence | Meaning |
|---|---|
| `upgrade.sh:17-25`: `devType` = first `-`-separated field of `/root/Box/data/id`; `D35` is folded into `D15` | Behaviour depends on the device ID prefix |
| `upgrade.sh:91-94`: `cat kernel_d15_ec200T.img > /dev/block/by-name/boot` **only if devType == D15** | On C15/C35/C36 the boot partition is **never rewritten** by this package, so the kernel/DTB on those units is whatever the factory flashed [UNKNOWN] |
| `upgrade.sh:47-51`: copies `fpga_$devType.img` to `/boot/fpga.img`, then runs `write_fpga`; the package only contains `fpga_D15.img` | C units keep their factory `/boot/fpga.img` |
| `upgrade.sh:41-44`: `wifi_$devType.sh` goes to `/etc/wifi.sh`; only `wifi_D15.sh` exists | C units keep their factory `/etc/wifi.sh` |
| `SystemConfig/dev_type`: `C15=0x28 C35=0x2a C36=0x44 D15=0x34 D35=0x36` | Device-type codes used by the application |
| ZBIN_Firmware_Analysis.md:104 | C36 (and C16/D16/D36/C16L/C08L) take `PX30_BoxPlayerD15_RC.tar.gz`, which we **do not have**. The RC package may carry a C36 kernel and FPGA image |
| `api/cn.huidu.device.api.sh:10-16` | The HTTP API only starts when field 2 of `data/id` starts with `D` (and `/boot/httpApi` exists) |

Per-model code paths in the shared user space [PX30]:

| Where | Model-specific behaviour |
|---|---|
| `libCore` `HSystemEnv::InitHardwareVersion` (decompiled/libCore.so.c:~76060-76200) | For some types it reads the board revision from `/sys/devices/platform/ff288000.saradc/iio:device0/in_voltage0_raw` (SARADC ch0 resistor strap) and formats model strings such as `HD-C36-%1.X` and `HD-C16-%1.X` |
| `libCore` `HSystemEnv::InitHardwareConfig` (libCore.so.c:77390-77760) | Reads **`/etc/hardware.conf`** (libCore.so.c:14959) with keys `cpu`, `wifi`, `wifiReset`, `gPhy`, `version`, then special-cases IDs containing `C16-C` / `C36-C` |
| `libFPGADriver` `HFPGAMonitor::ConfigPhyTimer` (libFPGADriver.so.c:68071-68300) | Depending on the CPU/hardware config, it opens **`/dev/mdio_gpio`** and issues ioctls `0xc0045702` / `0xc0045704` to configure a PHY. **The D15 kernel has no mdio_gpio driver**, so this path is probably for other models (C-series?) [UNKNOWN] |
| `libFPGADriver` `OpenFPGALedLamp` (libFPGADriver.so.c:67790-67900) | Opens **`/dev/audio_switch`** and uses ioctls `0xc0046a02` (lamp on) and `0xc0046a03` (lamp off). **This node is also missing from the D15 kernel** |
| `libscreen_plugin` strings 281-284 | Capability lists include `C35|C15|C30|C10|...` |
| `BoxUpgrade` strings 1011-1031 | Accepts `C15-`, `C35-`, `C36-`, `D15-`, `D35-` ... prefixes |

---

## 2. Boot image (D15) [D15]

`kernel_d15_ec200T.img` is an Android boot image, header v0:

| Field | Value |
|---|---|
| magic @0x0 | `ANDROID!` |
| kernel_size / addr | 14,991,368 B / 0x10008000 |
| ramdisk_size / addr | **0** (no initramfs) / 0x11000000 |
| second_size / addr | 282,112 B / 0x10F00000; data at file offset **0xE4D000** |
| tags_addr / page_size | 0x10000100 / 2048 |
| name, cmdline, extra cmdline | all **empty** |
| kernel | Raw **uncompressed arm64 `Image`**. Header: text_offset 0x80000, image_size 0x1035000, flags 0xA, magic `ARMd` @0x38 |
| `Linux version` @kernel+0x8F00A0 | `Linux version 4.4.159 (wulong@HDServer) (gcc version 6.3.1 20170404 (Linaro GCC 6.3-2017.05) ) #19 SMP Tue Sep 22 18:00:57 CST 2020` |
| built-in initramfs | gzip @kernel+0xCE3988. It is the default empty cpio (`/dev`, `/dev/console`, `/root`) |
| IKCONFIG | **absent** |
| second stage | Rockchip **RSCE** resource: `rk-kernel.dtb` (blk 4, 90,111 B), `logo.bmp` (blk 180, 170,326 B), `logo_kernel.bmp` (blk 513, 19,160 B) |

For comparison, user space was built with `GCC: (Buildroot 2018.02-rc3-gbd351b2dc-dirty) 6.4.0` (BootLogo/BoxDaemon `.comment`).

Features seen in the kernel strings: ext4, squashfs, `option` USB serial with Quectel EC20 port handling (`handle_quectel_ec20`), cdc_ether, rndis_wlan, rtl8188eu (built in), fiq-debugger, `drivers/char/cyclone4.c`, and Huidu `led-control` / `pcie-control`.

### Kernel command line [D15]
- The bootimg cmdline is empty.
- DTB `/chosen/bootargs` (dts:3120): `earlycon=uart8250,mmio32,0xff160000 swiotlb=1 console=ttyFIQ0 root=PARTUUID=614e0000-0000 rootwait`
- `614e0000-0000...` is the rootfs UUID from the stock Rockchip SDK `parameter.txt`. Rockchip U-Boot normally appends or overrides with `storagemedia=`, `androidboot.mode=` and partition info. The final `/proc/cmdline` is [UNKNOWN].

---

## 3. SoC and board (from the D15 DTB) [D15, mostly PX30-generic]

| Item | DTS evidence | Value |
|---|---|---|
| Compatible / model | dts:7,11 | `rockchip,px30-evb-ddr3-v10-linux`, `rockchip,px30`; "Rockchip linux PX30 evb ddr3 board". This is the **Rockchip EVB DTS, lightly modified** |
| CPU | dts:293-330 | 4x Cortex-A35, OPP 408 MHz to 1.512 GHz. BoxPlayerInit.sh pins it to 1.2 GHz and the GPU to 480 MHz |
| RAM | none | **No `/memory` node** (U-Boot fills it in). DDR3 per the compatible and `ddr_timing`. Size [UNKNOWN] |
| Storage | dts:1745 `dwmmc@ff390000` okay, 8-bit, HS200 | **eMMC**. `nandc@ff3b0000` is also "okay" (dts:1766), a generic EVB leftover. **SD card `ff370000` and SDIO `ff380000` are disabled** |
| Ethernet | dts:1671 | GMAC `ff360000`, **RMII**, external ref clock in (`clock_in_out="input"`), reset GPIO2_B5 active-low. Pins GPIO2_A0-A7,B1. The PHY is not described in the DT (generic MDIO probe) |
| USB | dts:1630/1647/1659 | dwc2 OTG `ff300000` (`dr_mode=otg`), EHCI/OHCI host `ff340000`/`ff350000` |
| Wi-Fi | dts:3227 | `wlan-platdata`, `wifi_chip_type="rtl8188eu"` (**USB**). BT disabled. `wifi_D15.sh` uses wlan0 (STA) and wlan1 (AP) |
| 4G modem | kernel strings | EC200T is on USB (`option` driver and `pppd` from the package). `pcie-control` node (dts:3152) drives GPIO0_A5; the driver's GPIO names are `3g_reset` / `pcie_pwren` |
| UART0 `ff030000` | dts:678 | okay, GPIO0_B2/B3 |
| UART1 `ff158000` = **ttyS1** | dts:827 | okay, GPIO1_C0/C1: **FPGA control link** (section 4) |
| UART2 `ff160000` | dts:841, 3122 | node disabled, but used by **fiq-debugger** (`serial-id=2`, 115200) on GPIO1_D2/D3 (uart2m0) → **console `ttyFIQ0`**, earlycon 0xff160000 |
| UART3 `ff168000` | dts:855 | okay, GPIO0_C0/C1 (m0) |
| UART4 `ff170000` | dts:869 | okay, GPIO1_D4/D5 |
| UART5 `ff178000` | dts:883 | disabled |
| I2C0 | dts:897-1157 | **RK809 PMIC** @0x20 (regulators, RTC, codec; codec disabled) |
| I2C1 | dts:1158-1174 | **PCF8563 RTC** @0x51 |
| I2C2 / I2C3 | dts:1175/1208 | disabled (EVB OV5695 camera leftover) |
| SPI0 `ff1d0000` | dts:1221-1243 | okay. Child `spi_cyclone4@0`, compatible `huidu,spi_bus0_cs0`, 12 MHz, `gpios = GPIO0_A0, GPIO0_A1, GPIO0_A2` (FPGA nCONFIG / nSTATUS / CONF_DONE; which pin is which is [UNKNOWN]). Pins GPIO1_B4-B7 |
| SPI1 | dts:1244 | disabled |
| Watchdog | dts:1260 | `snps,dw-wdt` `ff1e0000` okay. **BoxDaemon opens `/dev/watchdog`** (WDIOC_SETPRETIMEOUT, KEEPALIVE; BoxDaemon strings) |
| LEDs | dts:3143, 3156 | `sys_run` heartbeat on GPIO1_C5. `huidu,led-control` GPIO0_B4/B5 (`led_red`/`led_green`) |
| Key | dts:3160 | `test-key` GPIO1_C4 (KEY 0x94) |
| Audio | dts:3183 | rk809-sound card declared, but the RK809 codec and I2S nodes are not enabled. Effectively **no audio** on D15 |
| Reboot modes | dts:659 | PMUGRF+0x200: normal `0x5242C300`, **loader/bootloader `0x5242C301`**, recovery `0x5242C303`, fastboot `…309`, charge `…30B`, ums `…30C` |
| Backlight PWM | dts:3168 | EVB leftover |

---

## 4. How pixels and control reach the FPGA [PX30 user space; D15 kernel]

Two corrections to BOXPLAYER_DECOMPILATION.md: there is a single node **`/dev/cyclone4`**, not `-0`/`-1`, and pixels do **not** travel over serial.

| Path | Mechanism | Evidence |
|---|---|---|
| **Pixel data** | DRM/KMS `/dev/dri/card0` → **VOP → parallel RGB888 (24-bit) LCD interface → FPGA** | DTS `rgb` node okay (dts:779, data pins `lcdc-rgb888-m0` GPIO3_A4-D3 (24 lines) + DCLK/HSYNC/VSYNC/DEN on GPIO3_A0/A1/A2/A3, dts:2942). LVDS (dts:1566) and DSI (dts:1899) are disabled. libMainWindowRender uses glmark2-style `NativeStateDRM` with gbm + `drmModeSetCrtc`/`PageFlip` on `/dev/dri/card0` (strings 806-843, 1095) |
| Panel timing the FPGA expects [D15] | `simple-panel` (dts:3246) | **1024x128**, pclk **12 MHz**, h 75/76/75, v 11/10/11 → htotal 1250, vtotal 160 → **60 Hz**. `bus-format=0x1013` (MEDIA_BUS_FMT_BGR888_1X24 per mainline header; check before relying on it), `rgb-mode="p888"`. **C15/C35/C36 are almost certainly different**, because 1024x128 = 131k px is below C-series capacity [UNKNOWN] |
| Control/status channel | **UART `/dev/ttyS1` 115200 8N1** (UART1) | libFPGADriver `HSerialParam` ctor sets `/dev/ttyS1`, 0x1c200, 8, 1, 'n' (libFPGADriver.so.c:16184-16189). BootLogo also talks to the FPGA over ttyS1 ("Write to FPGA", "Read from FPGA", CRC framing; BootLogo strings ~220-280) |
| Bitstream load | **SPI0 passive-serial via kernel driver `drivers/char/cyclone4.c` → `/dev/cyclone4`** | Kernel strings @0xB14908-0xB14A69: `cyclone4_nconfig`, `cyclone4_conf_done`, `cyclone4_nstatus`, "only support one device". ioctls: `0x80044301` = `CYCLONE4_IOC_RD_CONF_DONE` (`_IOR('C',1,int)`), `0x4302` = `CYCLONE4_IOC_INIT` (libFPGADriver.so.c:66959-66969) |
| `write_fpga` | External binary `/root/Box/System/write_fpga` (**not in this package**; lives on the rootfs). libFPGADriver runs `write_fpga <img> /dev/cyclone4` (libFPGADriver.so.c:68547-68580). `upgrade.sh:50` runs it after copying `/boot/fpga.img`. `clear_fpga` (BoxPlayerInit.sh) is also rootfs-only |
| FPGA image | `fpga_D15.img`, 696,114 B: 8-byte header (`6c031646` + LE length 0x0A9F2A = file-8), then 0xFF padding and sync `CC 55 AA 33`. **Not a standard Altera RBF** (the driver name says Cyclone IV). The real FPGA part/vendor is [UNKNOWN]. Version file `version/fpga` = 6.6.0.0 |
| Extra nodes (not in the D15 kernel) | `/dev/mdio_gpio` (PHY config), `/dev/audio_switch` (LED lamp) | See section 1. They may exist in C kernels [UNKNOWN] |

To keep the LED output working, a replacement OS must supply: the `cyclone4` SPI driver (or a spidev + GPIO equivalent), the RGB panel timing, the ttyS1 protocol, and `/boot/fpga.img`.

---

## 5. Partitions, bootloader, update paths

| Partition (by-name) | Referenced by | Notes |
|---|---|---|
| `boot` | upgrade.sh:93 | Android bootimg: kernel + RSCE(DTB, logos) |
| `userdata` | S21mountall.sh:257 (`fsck.ext4 -fy`) | ext4 |
| `oem` | libSDKServices.so.c:220451 (`UpdateMACAddress` opens it read-write) | **Stores the MAC address. Back it up** |
| rootfs | DTB `root=PARTUUID=614e0000-0000` | Standard Rockchip SDK UUID |
| `/boot` (mount) | fpga.img, logo.bmp, dev_mac, rotation/, setting/, httpApi, omsEnable, bootscreen | Filesystem/partition behind `/boot` is [UNKNOWN] |
| `recovery`, `misc`, `uboot`, `trust` | not referenced by name | `S21mountall.sh:261-265` is the stock SDK `is_recovery()` and the DT has a recovery reboot mode. Whether a recovery partition exists is [UNKNOWN] |
| `hddata`, `fbparam` | strings in the multi-platform Go `cn.huidu.device.service` | Probably other SoCs, not PX30 |

- **Bootloader**: no U-Boot/miniloader binary or version string is in this package. Android-format boot + RSCE + `rockchip,drm-logo` + reboot-mode magic `0x5242C3xx` point to **Rockchip U-Boot (next-dev, 2017.09 era)**, using `boot_android`/`bootrkp`. The version is [UNKNOWN]. There is no `parameter.txt` and no `mtdparts` in the cmdline; the kernel only contains the parser string `mtdparts=` @0xB00FF7.
- **Update flow**: `BoxUpgrade` (TCP / USB `HDPlayerUsbExport/BoxPlayer.bin`) unpacks the `.bin`/`.zbin`, runs the embedded script, checks `version.limit`, and writes `/boot/fpga.img` + `write_fpga`. `upgrade.sh` then overwrites `/root/Box/*`, `/usr/sbin/*`, `/usr/sbin/pppd`, ntfs-3g, `/etc/init.d/S21mountall.sh`, `/usr/bin/BoxUpgrade`, `/etc/ssh/sshd_config`, `/etc/wifi.sh`, and then `reboot`s. It never touches the bootloader or partition table.

---

## 6. Boot and runtime chain [PX30]

BusyBox init → `/etc/init.d/S21mountall.sh` (resize, fsck, mount) → … → `System/BoxPlayerInit.sh`: set CPU/GPU governors, `clear_fpga`, start `BoxDaemon` (watchdog + process supervisor per `SystemConfig/process_info.xml`), `BootLogo` (FPGA init over ttyS1 + logo), `runBoxUpgrade.sh`, `run.sh` (BoxPlayer `-platform offscreen` + BoxSDK), `start-ssh`, **`/etc/init.d/S50telnet stop`**, then the device.api and device.service scripts. The init script that calls BoxPlayerInit.sh is not in the package [UNKNOWN]. Weston is **not** started (`InitWayland.sh start` is commented out, BoxPlayerInit.sh:10).

---

## 7. Access routes and lock-out risks [PX30]

| Route | Details | Evidence |
|---|---|---|
| **Serial console** | `ttyFIQ0` on UART2-m0 (GPIO1_D2/D3), 115200. Most reliable way in | dts:3120-3131 |
| **SSH** | OpenSSH `sshd -D` via `/root/Box/bin/start-ssh`; `PermitRootLogin yes`, keys generated on first run | ssh/start-ssh, ssh/sshd_config, BoxPlayerInit.sh:9 |
| Root password | **Not in this package** (no `/etc/shadow`). `System/userCtl.sh` can create users and change passwords (`passwd`), and is reachable from the app. Default credentials [UNKNOWN] | userCtl.sh |
| Telnet | Explicitly stopped at boot (`S50telnet stop`) | BoxPlayerInit.sh:10 |
| ngrok | `ngrok1.huidu.cn:4443`, tunnel "ssh" → remote 29001 → **local tcp 23 (telnet)**; started by libBoxIOServices only when enabled in config | ngrok/ngrok.cfg, libBoxIOServices strings 3161-3164 |
| cn.huidu.device.service (OMS) | Go agent to `service.huidu.cn` with `ssh.go` / `shell_linux.go` (remote shell); only runs if `/boot/omsEnable` exists | api/cn.huidu.device.service.sh:33-88, strings |
| cn.huidu.device.api | HTTP API; runs only if `/boot/httpApi` exists and the model is D-type; **executes every `huidushell_*.sh`** in its folder with bash | api/cn.huidu.device.api.sh |
| **device_locker** | **App-level password only.** `HSDeviceLocker` guards HDPlayer/SDK management and has a "super" password derived from base64(device ID) (libSDKServices.so.c `CheckPasswd`; exact derivation not fully traced). `device_locker.sh` only **deletes** `/root/Box/config/device_locker` when upgrading from ≤ 6.4.9.15. **It does not touch Linux users, ssh or the bootloader, so it cannot lock us out of the OS** | device_locker.sh, libSDKServices strings 9884-9898 |
| Recovery/flash | `reboot loader` (magic 0x5242C301) → Rockusb over USB OTG; PX30 BootROM MaskROM as a last resort | dts:659, dts:1630 |

---

## 8. What to dump from a live C15 / C35 / C36 unit

```
cat /proc/cpuinfo /proc/cmdline /proc/version /proc/partitions /proc/mtd /proc/mounts
ls -l /dev/block/by-name/ ; cat /etc/fstab /etc/hardware.conf /root/Box/data/id
ls /proc/config.gz ; dmesg > /tmp/dmesg.txt
tar czf /tmp/dt.tgz /proc/device-tree   # or: dd if=/dev/block/by-name/boot
for p in /dev/block/by-name/*; do dd if=$p of=/tmp/$(basename $p).img; done   # at minimum: boot uboot trust misc oem recovery parameter
ls -l /dev/cyclone4 /dev/mdio_gpio /dev/audio_switch /dev/ttyS* /dev/dri
cat /sys/class/drm/*/modes ; cat /sys/kernel/debug/dri/0/summary
cat /sys/devices/platform/ff288000.saradc/iio:device0/in_voltage0_raw
cp /root/Box/System/write_fpga /root/Box/System/clear_fpga /boot/fpga.img /tmp/
head -c 4M /dev/mmcblk0 > /tmp/emmc_head.bin   # GPT + idbloader + parameter
```

Plus: the U-Boot banner from the serial console, and `PX30_BoxPlayerD15_RC.tar.gz` (a C36 kernel/FPGA may be inside).

---

## 9. Risks when replacing the OS

1. **The DTB is D15-only.** Panel timing, mdio_gpio/audio_switch and possibly the Ethernet PHY differ on C units. Always boot the unit's **own** DTB first.
2. **FPGA runtime pieces are closed source**: the `cyclone4` driver, `write_fpga`, the ttyS1 protocol, and the proprietary bitstream format. Without them the LED output stays dark.
3. **`oem` holds the MAC address.** Back up every partition, especially `oem`, `/boot` and `userdata`, before writing anything.
4. **Watchdog**: BoxDaemon feeds `/dev/watchdog`. If U-Boot or the kernel enables it at boot (unverified), a new OS must feed it or the unit reboot-loops.
5. **Bootloader not seen**: U-Boot version, whether secure boot/verified boot is enabled, and how `root=` is chosen are all [UNKNOWN]. Get serial console access and a working Rockusb/MaskROM path before flashing `boot`.
6. Don't use `upgrade.sh` on C units as a model for kernel updates: it deliberately skips them.
