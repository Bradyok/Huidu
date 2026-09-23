#!/bin/sh
# dd_readback.sh - read partitions off a LIVE unit over the network (ssh), when
# shell access exists (stock runs sshd with PermitRootLogin yes). This is the
# network alternative to rk_readback.sh (USB/maskrom).
#
#   dd_readback.sh <ssh-target> <outdir> [--parts "boot uboot ..."] [--full] [--max-mib N]
#
#   <ssh-target>  ssh:[user@]host  or  [user@]host
#   --parts "..." only read these by-name partitions (default: all of them)
#   --full        also stream the whole mmcblk device -> golden.img.gz
#   --max-mib N   skip by-name partitions larger than N MiB unless named in
#                 --parts (default 256; keeps userdata/rootfs from being pulled
#                 by accident). Set 0 to disable the guard.
#
# NON-destructive. Each partition is streamed gzip'd:  <outdir>/<name>.img.gz
# Also writes partition-map.json from /dev/block/by-name + /sys sizes (offsets
# and sizes are read from the device; nothing is guessed), and a raw
# partition-table.txt (ls -l by-name, /proc/partitions).
#
# NOTE telnet: binary streaming over telnet is unreliable, so this script
# requires ssh. For an on-device, USB-drive-based dump without ssh, use the
# access-survey package (packages/access-survey) which dds by-name to a stick.
set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"

PARTS=""; FULL=0; MAXMIB=256; POS=""
while [ $# -gt 0 ]; do
	case "$1" in
		--parts)   PARTS=$2; shift 2 ;;
		--full)    FULL=1; shift ;;
		--max-mib) MAXMIB=$2; shift 2 ;;
		-h|--help) sed -n '2,26p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  POS="$POS $1"; shift ;;
	esac
done
# shellcheck disable=SC2086
set -- $POS
[ $# -eq 2 ] || die "usage: dd_readback.sh <ssh-target> <outdir> [--parts \"...\"] [--full] [--max-mib N]"
TARGET=$1; OUT=$2

rt_init "$TARGET"
[ "$RT_KIND" = ssh ] || die "dd_readback needs ssh (binary streaming). Got: $RT_KIND"
require_cmd gzip
mkdir -p "$OUT"

# --- capture the raw layout ---------------------------------------------------
rt_run 'ls -l /dev/block/by-name/ 2>/dev/null; echo "== partitions =="; cat /proc/partitions; echo "== mounts =="; cat /proc/mounts' >"$OUT/partition-table.txt" || true
info "layout saved to partition-table.txt"

# --- enumerate by-name -> name/dev/size(sectors) ------------------------------
# Emits lines: <name> <realdev-basename> <size-sectors>
LAYOUT=$(rt_run '
	for p in /dev/block/by-name/*; do
		[ -e "$p" ] || continue
		n=$(basename "$p"); d=$(readlink -f "$p"); b=$(basename "$d")
		s=$(cat /sys/class/block/$b/size 2>/dev/null)
		echo "$n $b ${s:-0}"
	done')
[ -n "$LAYOUT" ] || die "no /dev/block/by-name/* entries found on target"

# --- partition-map.json (sizes from the device) -------------------------------
JSON="$OUT/partition-map.json"
{
	printf '{\n  "target": "%s",\n  "sector_size": 512,\n  "source": "sysfs /dev/block/by-name",\n  "partitions": [\n' "$TARGET"
	first=1
	printf '%s\n' "$LAYOUT" | while read -r name dev sectors; do
		[ -n "$name" ] || continue
		[ $first = 1 ] || printf ',\n'
		first=0
		printf '    {"name": "%s", "dev": "/dev/%s", "size_sectors": %s, "size_bytes": %s}' \
			"$name" "$dev" "$sectors" "$((sectors * 512))"
	done
	printf '\n  ]\n}\n'
} >"$JSON"
info "partition-map.json written"

# --- read partitions ----------------------------------------------------------
want() {  # name -> 0 if it should be read
	[ -z "$PARTS" ] && return 0
	for w in $PARTS; do [ "$w" = "$1" ] && return 0; done
	return 1
}
printf '%s\n' "$LAYOUT" | while read -r name dev sectors; do
	[ -n "$name" ] || continue
	want "$name" || continue
	mib=$((sectors / 2048))
	if [ -z "$PARTS" ] && [ "$MAXMIB" -gt 0 ] && [ "$mib" -gt "$MAXMIB" ]; then
		warn "skipping large partition '$name' (${mib} MiB > --max-mib $MAXMIB); name it in --parts to force"
		continue
	fi
	info "reading $name (${mib} MiB) via /dev/block/by-name/$name"
	rt_ddget "/dev/block/by-name/$name" "$OUT/$name.img.gz"
	info "  -> $OUT/$name.img.gz"
done

# --- optional whole-device golden read ----------------------------------------
if [ "$FULL" = 1 ]; then
	MMC=$(rt_run 'ls /dev/mmcblk? 2>/dev/null | head -1' | tr -d '\r')
	[ -n "$MMC" ] || die "--full: could not find /dev/mmcblkN on target"
	info "reading whole device $MMC -> golden.img.gz (long)"
	rt_ddget "$MMC" "$OUT/golden.img.gz"
fi

info "done. Decompress with: gunzip -k $OUT/*.img.gz"
info "Note: gzip'd partition images are NOT directly consumable by build_update_img.sh;"
info "gunzip them first (or prefer rk_readback.sh, which yields raw <name>.img)."
