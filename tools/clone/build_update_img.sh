#!/bin/sh
# build_update_img.sh - pack a golden readback into a Rockchip RKFW update.img
# for flashing target units with RKDevTool (Windows) or `upgrade_tool`/
# `rkdeveloptool uf` (Linux).
#
#   build_update_img.sh --images DIR --loader MiniLoaderAll.bin -o update.img \
#                       [--parameter parameter.txt | --from-map partition-map.json] \
#                       [--chip RK330C] [--only "boot uboot trust ..."]
#
#   --images DIR    directory of raw <name>.img files (from rk_readback.sh)
#   --loader FILE   PX30/RK3326 MiniLoaderAll.bin (the DDR init + miniloader)
#   --parameter F   Rockchip parameter.txt (partition layout + CMDLINE). If you
#                   don't have one, use --from-map to synthesize it from the GPT
#                   that rk_readback.sh captured (offsets come from the device).
#   --only "..."    pack only these partitions (default: every one in parameter)
#   --chip TAG      rkImageMaker chip tag (default RK330C; PX30/RK3326 == RK330C -
#                   CONFIRM against your rkbin/RKDevTool config before relying).
#
# TOOLS (not in this repo - get them from Rockchip's rkbin / tools):
#   afptool, rkImageMaker : https://github.com/rockchip-linux/rkbin  (tools/)
#                           or the RKDevTool package. Put them in PATH or pass
#                           --afptool / --rkimagemaker.
#   MiniLoaderAll.bin     : rkbin/bin/rk33/  (PX30_MiniLoaderAll or rk3326_*),
#                           OR the `uboot`+loader you read back from the unit.
#
# NON-destructive (produces a file). This only PACKS; flashing is a separate,
# deliberate step (see clone_unit.sh / README).
set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"

IMAGES=""; LOADER=""; PARAM=""; FROMMAP=""; ONLY=""; CHIP="RK330C"; OUT=""
AFPTOOL="afptool"; RKIMG="rkImageMaker"
while [ $# -gt 0 ]; do
	case "$1" in
		--images)       IMAGES=$2; shift 2 ;;
		--loader)       LOADER=$2; shift 2 ;;
		--parameter)    PARAM=$2; shift 2 ;;
		--from-map)     FROMMAP=$2; shift 2 ;;
		--only)         ONLY=$2; shift 2 ;;
		--chip)         CHIP=$2; shift 2 ;;
		--afptool)      AFPTOOL=$2; shift 2 ;;
		--rkimagemaker) RKIMG=$2; shift 2 ;;
		-o|--output)    OUT=$2; shift 2 ;;
		-h|--help)      sed -n '2,34p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  die "unexpected arg: $1" ;;
	esac
done
[ -n "$IMAGES" ] && [ -d "$IMAGES" ] || die "--images DIR (raw <name>.img files) is required"
[ -n "$LOADER" ] && [ -f "$LOADER" ] || die "--loader MiniLoaderAll.bin is required"
[ -n "$OUT" ] || die "-o update.img is required"
require_cmd "$AFPTOOL" "get it from rockchip-linux/rkbin (tools/) - or pass --afptool PATH"
require_cmd "$RKIMG"   "get it from rockchip-linux/rkbin (tools/) - or pass --rkimagemaker PATH"

STAGE=$(mktemp -d "${TMPDIR:-/tmp}/rkfw.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/Image"

# --- parameter.txt: use given, or synthesize from the GPT map ------------------
if [ -z "$PARAM" ]; then
	[ -n "$FROMMAP" ] && [ -f "$FROMMAP" ] || die "provide --parameter FILE or --from-map partition-map.json"
	require_cmd python3
	info "synthesizing parameter.txt from $FROMMAP (offsets from the device GPT)"
	PARAM="$STAGE/parameter.txt"
	python3 - "$FROMMAP" "$PARAM" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
parts = [p for p in m["partitions"] if p.get("name")]
parts.sort(key=lambda p: p["start_sector"])
# Rockchip CMDLINE mtdparts: name(@0xstart)size(hex sectors)..., last uses '-'
segs = []
for i, p in enumerate(parts):
    start = p["start_sector"]; size = p["size_sectors"]
    # last partition grows to fill the disk -> size '-'
    sz = "-" if i == len(parts) - 1 else "0x%08x" % size
    segs.append("%s@0x%08x(%s)" % (sz, start, p["name"]))
cmd = "mtdparts=rk29xxnand:" + ",".join(segs)
with open(sys.argv[2], "w") as f:
    f.write("FIRMWARE_VER:1.0.0\n")
    f.write("MACHINE_MODEL:PX30\n")
    f.write("MACHINE_ID:007\n")
    f.write("MANUFACTURER:HUIDU\n")
    f.write("MAGIC: 0x5041524B\n")
    f.write("ATAG: 0x00200800\n")
    f.write("MACHINE: 0xffffffff\n")
    f.write("CHECK_MASK: 0x80\n")
    f.write("PWR_HLD: 0,0,A,0,1\n")
    f.write("TYPE: GPT\n")
    f.write("CMDLINE:" + cmd + "\n")
sys.stderr.write("WROTE parameter.txt with %d partitions.\n"
                 "  REVIEW IT: the root= UUID and any uuid: entries are NOT "
                 "reconstructed here and may need to be added by hand.\n" % len(parts))
PY
	warn "synthesized parameter.txt is a best-effort layout - review CMDLINE/root= before flashing"
fi
cp "$PARAM" "$STAGE/parameter.txt"

# --- collect partition names from parameter.txt CMDLINE -----------------------
NAMES=$(sed -n 's/.*CMDLINE:.*mtdparts=[^:]*:\(.*\)/\1/p' "$STAGE/parameter.txt" \
	| tr ',' '\n' | sed -n 's/.*(\([^)]*\)).*/\1/p')
[ -n "$NAMES" ] || die "could not parse partition names from parameter.txt CMDLINE"

# --- build the afptool package-file -------------------------------------------
PKG="$STAGE/package-file"
{
	printf '# NAME            Relative path\n'
	printf 'package-file      package-file\n'
	printf 'bootloader        %s\n' "$(basename "$LOADER")"
	printf 'parameter         parameter.txt\n'
} >"$PKG"
cp "$LOADER" "$STAGE/$(basename "$LOADER")"

included=0
for n in $NAMES; do
	# skip the pseudo entries that have no image of their own
	case "$n" in grow|userdata|user) skip_default=1 ;; *) skip_default=0 ;; esac
	if [ -n "$ONLY" ]; then
		keep=0; for w in $ONLY; do [ "$w" = "$n" ] && keep=1; done
		[ "$keep" = 1 ] || continue
	fi
	img="$IMAGES/$n.img"
	if [ ! -f "$img" ]; then
		[ "$skip_default" = 1 ] && { info "no image for '$n' (ok, typically empty/user data)"; continue; }
		warn "no image '$img' for partition '$n' - it will be OMITTED from update.img"
		continue
	fi
	cp "$img" "$STAGE/Image/$n.img"
	printf '%-17s Image/%s.img\n' "$n" "$n" >>"$PKG"
	included=$((included + 1))
done
printf 'RESERVED\n' >>"$PKG"
[ "$included" -gt 0 ] || die "no partition images were included - check --images DIR names match parameter.txt"
info "package-file lists $included partition image(s)"

# --- pack: afptool -> raw firmware, rkImageMaker -> RKFW update.img ------------
RAW="$STAGE/update-raw.img"
info "afptool pack ..."
( cd "$STAGE" && "$AFPTOOL" -pack ./ "$RAW" ) || die "afptool -pack failed"
info "rkImageMaker (chip $CHIP) ..."
"$RKIMG" -"$CHIP" "$LOADER" "$RAW" "$OUT" -os_type:androidos || die "rkImageMaker failed"

sz=$(wc -c <"$OUT" | tr -d ' ')
info "built $OUT ($sz bytes)"
info "Flash a target with: rkdeveloptool uf $OUT   (or upgrade_tool uf / RKDevTool)."
info "REMEMBER: back up + restore each target's identity around the flash (clone_unit.sh)."
