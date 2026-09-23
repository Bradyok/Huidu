# br2-huidu — our own OS for Huidu PX30 controllers (C15 / C35 / C36)

A Buildroot external tree (`BR2_EXTERNAL`) building a modern Linux for the Rockchip PX30 Huidu
controllers, replacing the stock Buildroot-2018 + kernel-4.4 + BoxPlayer image. Rationale, phases
and risks: `../PX30_CUSTOM_OS_PLAN.md`.

Stack: Buildroot 2026.02 LTS · mainline Linux 6.18.53 · Panfrost (Mali-G31) on DRM/KMS · booted by
the **stock** Rockchip U-Boot (Android v0 boot image + RSCE DTB, see `board/huidu/px30/mkrkbootimg.py`,
which rebuilds the stock image byte-identically).

```
configs/huidu_px30_defconfig          # installed system: Mesa/Panfrost, FFmpeg, ext4 root on eMMC
configs/huidu_px30_bringup_defconfig  # bring-up: kernel + initramfs in the boot image, no eMMC writes
board/huidu/px30/
  linux.fragment                      # kernel config on top of arch defconfig (Rockchip-only, Panfrost, FPGA-PS)
  mkrkbootimg.py                      # pack/unpack the stock Android+RSCE boot image
  post-image.sh                       # produce boot-<model>[-ramdisk].img per DTB
  dts/rockchip/                       # px30-huidu.dtsi + per-model dts (see its README)
  rootfs-overlay/                     # watchdog init + huidu-hwinfo survey tool
```

Build (WSL/Linux; PATH must be space-free — see `~/huidu-br/env.sh`):
```
make BR2_EXTERNAL=$PWD/br2-huidu O=<out> huidu_px30_bringup_defconfig && make -C <out>
# -> <out>/images/boot-c15-ramdisk.img   (send via a keepaccess-style package, or flash to boot/recovery)
```

Status: builds configured; userspace building; DTS compiles clean against 6.18.53. **Nothing booted
on hardware yet** — every board-specific value (panel timing, FPGA pin roles, PHY) still needs a dump
from a real C15/C35/C36. TODOs are marked in the DTS and `dts/rockchip/README.md`.
