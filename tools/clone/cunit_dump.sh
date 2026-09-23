#!/bin/sh
# cunit_dump.sh - one-shot hardware/identity dump for a Huidu PX30 C-series unit.
#
#   cunit_dump.sh <target> [outdir] [--yes]
#
#   <target>  a mounted rootfs dir, ssh:[user@]host, telnet:host, or user@host
#             (see clone_common.sh rt_init). ssh: is strongly preferred - the
#             DTB and fpga.img are binary and telnet pulls them base64 (slow).
#   [outdir]  where to write (default: ./cunit-dump-<host>-<UTC date>).
#
# READ-ONLY on the unit. Resolves every open item from hardware/DRIVER_ENABLEMENT.md
# in one pass, then tars the result so the analysis can happen off-device:
#
#   probe/*.txt     command output (model, lsusb, dmesg, gpio bases, drm modes, ...)
#   files/*         binary pulls (live DTB, /boot/fpga.img, write_fpga, hardware.conf,
#                   /root/Box/data, version files, ...)
#   FINDINGS.md     auto-filled answers to the 4 unknowns + the 2 live-capture TODOs
#   manifest.txt    status + size + sha256 for every collected file
#
# What this CANNOT grab as files (they are live serial captures - FINDINGS.md
# prints the exact huidu-sender commands to run for them):
#   - the FPGA-load bit order (proven by actually loading)
#   - the two-brightness ttyS1 diff (the send-card blob brightness offset)
#   - the 512-byte scan/gamma param frames (send-/recv-card blobs)

set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"

POS=""
while [ $# -gt 0 ]; do
	case "$1" in
		--yes) CLONE_YES=1; shift ;;   # read-only; accepted for symmetry
		-h|--help) sed -n '2,26p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  POS="$POS $1"; shift ;;
	esac
done
# shellcheck disable=SC2086
set -- $POS
[ $# -ge 1 ] || die "usage: cunit_dump.sh <target> [outdir] [--yes]"
TARGET=$1
rt_init "$TARGET"

# default outdir keyed to the host and date
host_slug=$(printf '%s' "${RT_HOST:-$TARGET}" | sed 's#[^A-Za-z0-9._-]#_#g')
OUT=${2:-"./cunit-dump-${host_slug}-$(date -u +%Y%m%dT%H%M%SZ)"}
[ -e "$OUT" ] && die "outdir '$OUT' already exists - pick another"
mkdir -p "$OUT/probe" "$OUT/files"
info "dumping $TARGET -> $OUT"

# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
# probe <slug> <shell-command...> : record stdout+stderr of a command run ON the
# unit (empty/absent tolerated). Local (mountdir) targets can't run commands, so
# these are skipped with a note.
probe() {
	slug=$1; shift
	if [ "$RT_KIND" = local ]; then
		printf '(local mountdir: cannot run "%s")\n' "$*" > "$OUT/probe/$slug.txt"
		return 0
	fi
	{ printf '$ %s\n' "$*"; rt_run "$*" 2>&1 || printf '(command failed or absent)\n'; } \
		> "$OUT/probe/$slug.txt"
	info "  probe $slug"
}

# grab <device-abs-path> [destname] : copy a file OFF the unit if it exists.
grab() {
	src=$1; dst=${2:-$(printf '%s' "$src" | sed 's#^/##; s#/#_#g')}
	if rt_exists "$src"; then
		if rt_get "$src" "$OUT/files/$dst" 2>/dev/null; then
			info "  got $src ($(rt_size "$src") B)"
		else
			warn "  FAILED to pull $src"; printf '%s PULL_FAILED\n' "$src" >> "$OUT/files/_missing.txt"
		fi
	else
		printf '%s ABSENT\n' "$src" >> "$OUT/files/_missing.txt"
	fi
}

# grab_tree <device-abs-dir> <destname.tgz> : tar a directory off the unit (ssh).
grab_tree() {
	src=$1; dst=$2
	[ "$RT_KIND" = ssh ] || { info "  (skip tree $src: needs ssh)"; return 0; }
	if rt_exists "$src"; then
		ssh $CLONE_SSH_OPTS "$RT_HOST" "tar -cf - -C / '${src#/}' 2>/dev/null | gzip -1" \
			> "$OUT/files/$dst" 2>/dev/null && info "  got tree $src -> $dst" \
			|| warn "  FAILED tree $src"
	else
		printf '%s ABSENT(dir)\n' "$src" >> "$OUT/files/_missing.txt"
	fi
}

# ---------------------------------------------------------------------------
# 1. identity / model / hw config
# ---------------------------------------------------------------------------
info "identity + model"
grab /root/Box/data/id            id
grab /etc/hardware.conf           hardware.conf
grab /etc/wifi.sh                 wifi.sh
grab /root/Box/version/fpga       version_fpga
grab /root/Box/version/version    version_box
grab_tree /root/Box/data          root_Box_data.tgz
probe model      "cat /proc/device-tree/model; echo; cat /proc/device-tree/compatible | tr '\\0' ' '; echo"
probe uname      "uname -a; echo; cat /proc/cmdline"
probe hwrev      "cat /sys/devices/platform/*saradc*/iio:device0/in_voltage0_raw 2>/dev/null; cat /sys/bus/iio/devices/iio:device0/in_voltage0_raw 2>/dev/null"

# ---------------------------------------------------------------------------
# 2. the C-series device tree (panel timing, GPIOs, PHY, extra nodes)
# ---------------------------------------------------------------------------
info "device tree (panel timing / GPIO map)"
grab /sys/firmware/fdt            fdt.dtb          # live flattened DTB (best single artifact)
grab_tree /proc/device-tree       proc_device-tree.tgz
# decoded on the HOST side afterwards: dtc -I dtb -O dts files/fdt.dtb > cunit.dts

# ---------------------------------------------------------------------------
# 3. which custom / LED nodes actually exist on THIS model
# ---------------------------------------------------------------------------
info "device nodes + GPIO bases"
probe devnodes   "ls -l /dev/cyclone4 /dev/mdio_gpio /dev/audio_switch /dev/spidev* /dev/ttyS* /dev/dri/* /dev/fb* 2>&1"
probe fpga_mgr   "ls -l /sys/class/fpga_manager 2>&1; for m in /sys/class/fpga_manager/*; do echo \$m; cat \$m/name \$m/state 2>/dev/null; done"
# gpiochip bases -> lets us compute the global GPIO numbers for load-fpga
probe gpiochips  "for c in /sys/class/gpio/gpiochip*; do echo \$c; echo -n ' base='; cat \$c/base; echo -n ' ngpio='; cat \$c/ngpio; echo -n ' label='; cat \$c/label; done 2>&1"
probe gpio_debug "cat /sys/kernel/debug/gpio 2>&1"
probe modules    "lsmod 2>&1; echo '--- kconfig ---'; zcat /proc/config.gz 2>/dev/null | grep -iE 'cyclone|fpga|mali|rockchip_vop|rgb' || echo '(no /proc/config.gz)'"

# ---------------------------------------------------------------------------
# 4. display / VOP output timing (what the KMS client must produce)
# ---------------------------------------------------------------------------
info "display timing"
probe drm_modes  "for s in /sys/class/drm/card*-*/; do echo \$s; cat \$s/status \$s/modes 2>/dev/null; echo; done 2>&1"
probe modetest   "modetest -c 2>&1 | head -80; echo '--- planes ---'; modetest -p 2>&1 | head -40"

# ---------------------------------------------------------------------------
# 5. radios (exact Wi-Fi part) + modem
# ---------------------------------------------------------------------------
info "radios"
probe usb        "lsusb 2>&1; echo '--- ids ---'; for d in /sys/bus/usb/devices/*/; do i=\$d/idVendor; [ -f \$i ] && echo \$(cat \$d/idVendor):\$(cat \$d/idProduct) \$(cat \$d/product 2>/dev/null); done 2>&1"
probe wifi       "cat /proc/net/wireless 2>&1; iw dev 2>&1; ip -br link 2>&1; dmesg 2>/dev/null | grep -iE 'rtl|8188|8189|8723|8821|wifi|nl80211|cfg80211'"
probe net        "ifconfig -a 2>/dev/null || ip addr; echo '--- mac cache ---'; cat /boot/dev_mac 2>/dev/null"

# ---------------------------------------------------------------------------
# 6. the FPGA blob + closed loaders (we keep the blob, reimplement the loaders)
# ---------------------------------------------------------------------------
info "FPGA blob + loaders"
grab /boot/fpga.img               boot_fpga.img
grab /root/Box/System/write_fpga  write_fpga
grab /root/Box/System/clear_fpga  clear_fpga
probe fpga_hdr   "od -An -tx1 -N 16 /boot/fpga.img 2>&1; echo -n 'size='; wc -c < /boot/fpga.img 2>&1"

# ---------------------------------------------------------------------------
# 7. partition map (for the recovery image later)
# ---------------------------------------------------------------------------
info "storage / partitions"
probe parts      "cat /proc/partitions 2>&1; echo '--- by-name ---'; ls -l /dev/block/by-name 2>&1; echo '--- mounts ---'; mount 2>&1"

# ---------------------------------------------------------------------------
# manifest + findings + package
# ---------------------------------------------------------------------------
info "writing manifest + FINDINGS.md"
{
	printf '# cunit_dump manifest  (target=%s  utc=%s)\n' "$TARGET" "$(date -u +%FT%TZ)"
	printf '%-12s  %-12s  %s\n' STATUS SIZE PATH
	find "$OUT/probe" "$OUT/files" -type f | sort | while read -r f; do
		rel=${f#"$OUT"/}
		printf '%-12s  %-12s  %s\n' OK "$(wc -c <"$f" | tr -d ' ')" "$rel  $(sha256_of "$f")"
	done
	[ -f "$OUT/files/_missing.txt" ] && { printf '\n# absent on this unit:\n'; cat "$OUT/files/_missing.txt"; }
} > "$OUT/manifest.txt"

cat > "$OUT/FINDINGS.md" <<'MD'
# C-unit dump — findings

Decode + fill this in on the host, then feed the answers back into the DTS,
the kernel fragment, and huidu-sender.

## 1. Model / identity
- `files/id`, `files/hardware.conf`, `probe/model.txt`, `probe/hwrev.txt`

## 2. Panel timing  (fixes the D15 placeholder in px30-huidu-c15.dts)
Decode the live DTB and pull the `panel-timing`:
```
dtc -I dtb -O dts files/fdt.dtb > cunit.dts   # (or use files/proc_device-tree.tgz)
sed -n '/panel-timing/,/};/p' cunit.dts
```
→ copy clock-frequency / hactive / vactive / porches / sync into the C dts.

## 3. FPGA control GPIOs  (for huidu-sender load-fpga, if the kernel path fails)
From `probe/gpiochips.txt`: gpiochip base for GPIO0 + the stock pins
(A0=nCONFIG, A1=nSTATUS, A2=CONF_DONE). global number = base + offset
(A0=0, A1=1, A2=2). Also confirm `/dev/cyclone4` vs the mainline path in
`probe/devnodes.txt` / `probe/fpga_mgr.txt`.

## 4. Wi-Fi chip  (may need a driver we don't have yet)
`probe/usb.txt` + `probe/wifi.txt` + `files/hardware.conf` (`wifi=`). If it is
not an RTL8188EU/rtl8xxxu part, note the exact VID:PID / module name.

## 5. Extra custom nodes present?
`probe/devnodes.txt`: do `/dev/mdio_gpio` or `/dev/audio_switch` exist on THIS
model? If yes, they need modelling in the DTS.

---

## Live captures still needed (NOT in this dump — run on the unit)
These need the serial link, so do them with a bench unit + our tools:

**FPGA bit order** — try the kernel path first (load `files/boot_fpga.img`
stripped via `huidu-sender strip-fpga`), else the userspace loader:
```
huidu-sender strip-fpga boot_fpga.img fpga.rbf      # -> kernel altera-ps-spi firmware
# or, spidev fallback (GPIO numbers from finding #3):
huidu-sender load-fpga boot_fpga.img --spidev /dev/spidev0.0 \
    --nconfig-gpio <base+0> --nstatus-gpio <base+1> --confdone-gpio <base+2>
# if CONF_DONE never asserts, add --msb-first
```

**Brightness offset** — capture two stock send-card param frames at two
brightnesses on ttyS1 and diff the 512-byte payloads to find the field
(wire it into huidu-sender src/blob.rs BRIGHTNESS_OFFSET).

**Scan/gamma blobs** — capture the send-card (func 0x0000) and recv-card
(0x0100/0x0200/0x0300) param frames; strip preamble+CRC; save the 512-byte
payloads as the --sendcard / --recvcard files huidu-sender replays.
MD

# tar it up next to the dir
TARBALL="$OUT.tgz"
( cd "$(dirname "$OUT")" && tar -czf "$(basename "$TARBALL")" "$(basename "$OUT")" ) \
	&& info "packaged $TARBALL ($(wc -c <"$TARBALL" | tr -d ' ') B, sha256 $(sha256_of "$TARBALL"))"

info "done. Read $OUT/FINDINGS.md next."
