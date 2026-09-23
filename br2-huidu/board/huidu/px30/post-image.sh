#!/bin/sh
# Pack each model's DTB with the kernel into the Android v0 + RSCE boot image the stock U-Boot loads.
#   boot-<model>.img           kernel + DTB, root on eMMC        (huidu_px30_defconfig)
#   boot-<model>-ramdisk.img   kernel + initramfs + DTB           (huidu_px30_bringup_defconfig; no eMMC writes)
set -e
MK="$(dirname "$0")/mkrkbootimg.py"
cd "$BINARIES_DIR"
for dtb in px30-huidu-*.dtb; do
	model="${dtb#px30-huidu-}"; model="${model%.dtb}"
	if [ -f rootfs.cpio.gz ]; then
		python3 "$MK" build --kernel Image --ramdisk rootfs.cpio.gz --dtb "$dtb" -o "boot-$model-ramdisk.img"
	else
		python3 "$MK" build --kernel Image --dtb "$dtb" -o "boot-$model.img"
	fi
done
ls -l boot-*.img
