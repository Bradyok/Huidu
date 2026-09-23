#!/bin/sh
# Access + hardware survey for Huidu PX30 units (C15/C35/C36), sent from HDPlayer as a firmware upgrade.
# Runs as root. Changes exactly one thing on the unit: the root password (for SSH, which stock already
# runs with PermitRootLogin yes). Everything else is read-only. Stock BoxPlayer is untouched; reboots at the end.
# Result: /root/survey.tar.gz (and a copy on any mounted USB stick). Placeholder @ROOTPW@ is filled at build time.
set -x
cd "$(dirname "$0")"
HERE=$(pwd)
echo "0" > /root/upgrade.status

finish() { echo "1" > /root/upgrade.status; sync; cd /; rm -rf "$HERE"; sync; reboot; exit 0; }

grep -q "PX30" /proc/cpuinfo || { echo "not a PX30 unit, nothing done"; finish; }

O=/root/survey
rm -rf $O; mkdir -p $O/parts $O/files
run() { f=$1; shift; { echo "\$ $*"; sh -c "$*"; } > "$O/$f.txt" 2>&1; }

# --- identity and kernel
run id           'cat /root/Box/data/id; echo; cat /root/Box/version/*; echo; cat /root/Box/SystemConfig/dev_type 2>/dev/null | head -3'
run uname        'uname -a; cat /proc/version'
run cmdline      'cat /proc/cmdline'
run cpuinfo      'cat /proc/cpuinfo'
run meminfo      'cat /proc/meminfo; free'
run iomem        'cat /proc/iomem'
run interrupts   'cat /proc/interrupts'
run modules      'cat /proc/modules; ls -R /lib/modules 2>/dev/null | head -200'
run dmesg        'dmesg'
run mounts       'cat /proc/mounts; df -h'
[ -f /proc/config.gz ] && cp /proc/config.gz $O/files/
[ -f /sys/firmware/fdt ] && cp /sys/firmware/fdt $O/files/live.dtb
tar czf $O/files/proc-device-tree.tar.gz -C /proc device-tree 2>/dev/null

# --- storage layout and small partitions (loader area, boot, recovery, resource, misc, oem, uboot, trust ...)
run partitions   'cat /proc/partitions; ls -l /dev/block/by-name/ 2>/dev/null; ls -l /dev/disk/by-partlabel 2>/dev/null; blkid 2>/dev/null'
MMC=$(ls /dev/mmcblk? 2>/dev/null | head -1)
[ -n "$MMC" ] && dd if=$MMC bs=1M count=8 2>/dev/null | gzip > $O/parts/_first8M.img.gz
for p in /dev/block/by-name/*; do
	[ -e "$p" ] || continue
	n=$(basename $p); dev=$(readlink -f $p); sz=$(cat /sys/class/block/$(basename $dev)/size 2>/dev/null)
	echo "$n $dev $sz" >> $O/parts/sizes.txt
	# 262144 sectors = 128 MiB; userdata/rootfs stay on the unit
	[ -n "$sz" ] && [ "$sz" -le 262144 ] && dd if=$p bs=1M 2>/dev/null | gzip > $O/parts/$n.img.gz
done

# --- display, FPGA and board glue
run drm          'ls -l /dev/dri; for c in /sys/class/drm/card*-*; do echo $c; cat $c/status $c/modes; done; cat /sys/kernel/debug/dri/0/summary 2>/dev/null'
run devices      'ls -l /dev; ls -l /sys/class/gpio /sys/class/spidev /sys/class/tty 2>/dev/null'
mount -t debugfs none /sys/kernel/debug 2>/dev/null
run gpio         'cat /sys/kernel/debug/gpio; cat /sys/kernel/debug/pinctrl/*/pinmux-pins 2>/dev/null | grep -v UNCLAIMED'
run clocks       'cat /sys/kernel/debug/clk/clk_summary 2>/dev/null | head -300'
run regulators   'cat /sys/kernel/debug/regulator/regulator_summary 2>/dev/null'
run watchdog     'ls -l /dev/watchdog*; cat /sys/class/watchdog/*/state /sys/class/watchdog/*/timeout 2>/dev/null'
run usb          'lsusb 2>/dev/null; cat /sys/kernel/debug/usb/devices 2>/dev/null'
run net          'ip addr; ip route; cat /etc/resolv.conf'
run adc          'for f in /sys/bus/iio/devices/*/in_voltage*_raw; do echo "$f $(cat $f)"; done'
run processes    'ps; ls -l /proc/*/exe 2>/dev/null | grep -v " -> $"'
for f in /boot/fpga.img /etc/hardware.conf /etc/inittab /etc/fstab /etc/passwd /etc/shadow /etc/wifi.sh \
         /usr/bin/runBoxUpgrade.sh /etc/ssh/sshd_config; do
	[ -f "$f" ] && cp "$f" "$O/files/$(echo $f | tr / _)"
done
ls -la /boot /etc/init.d /root/Box /root/Box/System /root/Box/bin > $O/listing.txt 2>&1
tar czf $O/files/box-system-bin.tar.gz -C /root/Box System bin SystemConfig 2>/dev/null
tar czf $O/files/etc.tar.gz -C / etc 2>/dev/null
for b in write_fpga clear_fpga; do p=$(command -v $b || ls /root/Box/System/$b 2>/dev/null); [ -n "$p" ] && cp "$p" $O/files/; done

# --- access: set the root password (old shadow saved above as files/_etc_shadow)
if command -v chpasswd >/dev/null; then
	echo 'root:@ROOTPW@' | chpasswd
else
	printf '%s\n%s\n' '@ROOTPW@' '@ROOTPW@' | passwd root
fi
echo "root password set: rc=$?" > $O/access.txt

# --- package the survey, copy to any USB stick
cd /root && tar czf survey.tar.gz survey && ls -l survey.tar.gz >> $O/access.txt
for m in $(awk '$1 ~ /^\/dev\/sd/ {print $2}' /proc/mounts); do cp /root/survey.tar.gz "$m/survey-$(cat /root/Box/data/id 2>/dev/null | tr -c 'A-Za-z0-9-' _).tar.gz"; done

finish
