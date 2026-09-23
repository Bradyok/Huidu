#!/bin/sh
# rk_readback.sh - read a full "golden" eMMC image + per-partition images from a
# Huidu PX30 (Rockchip PX30 / RK3326) controller over USB, using rkdeveloptool.
#
#   rk_readback.sh <outdir> [--loader MiniLoaderAll.bin] [--sectors N] [--force]
#
# NON-destructive: it only READS the eMMC (and downloads the loader into RAM).
# It never writes flash. Still: this is your master backup - keep it safe.
#
# Flow:
#   1. Detect mode with `rkdeveloptool ld` (Maskrom vs Loader).
#      - Maskrom  -> requires --loader (PX30/RK3326 MiniLoaderAll.bin from rkbin)
#                    to init DRAM before the flash can be read; it is downloaded
#                    with `db` into RAM only.
#      - Loader   -> read directly.
#   2. Determine total sector count (rkdeveloptool rfi/rci, or --sectors).
#   3. Read the WHOLE eMMC -> <outdir>/golden.img  (full backup FIRST).
#   4. Parse the GPT *out of golden.img itself* (offsets come from the device,
#      never guessed) -> partition-map.json + partition-table.txt.
#   5. Slice each partition out of golden.img by name -> <outdir>/<name>.img.
#
# ---------------------------------------------------------------------------
# MASKROM ENTRY REMINDER (PX30 C-series):
#   Power off. Briefly SHORT the eMMC clock (CLK) line to ground while applying
#   power over the USB-OTG port so the BootROM cannot load the on-flash loader
#   and drops into MaskROM. The device then enumerates on USB as 2207:330d.
#   Confirm with `rkdeveloptool ld` showing "Maskrom".  (`reboot loader` from a
#   shell, magic 0x5242C301, reaches Loader mode instead and does not need the
#   short - use that when you still have shell/serial access.)
# BACK UP FIRST: always capture golden.img (and per-unit identity via
#   identity_backup.sh) BEFORE writing anything to any unit.
# ---------------------------------------------------------------------------
set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"

LOADER=""; SECTORS=""; FORCE=0; OUT=""
while [ $# -gt 0 ]; do
	case "$1" in
		--loader)  LOADER=$2; shift 2 ;;
		--sectors) SECTORS=$2; shift 2 ;;
		--force)   FORCE=1; shift ;;
		-h|--help) sed -n '2,40p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  OUT=$1; shift ;;
	esac
done
[ -n "$OUT" ] || die "usage: rk_readback.sh <outdir> [--loader FILE] [--sectors N]"
require_cmd rkdeveloptool "Build it from https://github.com/rockchip-linux/rkdeveloptool"
require_cmd python3 "needed to parse the GPT"

if [ -e "$OUT" ] && [ "$FORCE" != 1 ]; then
	[ -z "$(ls -A "$OUT" 2>/dev/null)" ] || die "outdir '$OUT' is not empty (use --force to overwrite)"
fi
mkdir -p "$OUT"

# --- 1. detect mode -----------------------------------------------------------
LD=$(rkdeveloptool ld 2>/dev/null || true)
printf '%s\n' "$LD" >"$OUT/rkdeveloptool-ld.txt"
[ -n "$LD" ] || die "no device from 'rkdeveloptool ld'. Enter Maskrom/Loader and check the USB cable (2207:330d)."
info "device: $LD"
case "$LD" in
	*Maskrom*|*MaskRom*|*maskrom*)
		[ -n "$LOADER" ] || die "device is in Maskrom: pass --loader <PX30 MiniLoaderAll.bin> so DRAM can init before reading"
		[ -f "$LOADER" ] || die "loader '$LOADER' not found"
		info "downloading loader into RAM: $LOADER"
		rkdeveloptool db "$LOADER" || die "rkdeveloptool db failed"
		sleep 2 ;;
	*Loader*|*loader*)
		info "device already in Loader mode" ;;
	*)
		warn "could not classify mode from 'ld' output; continuing (read is non-destructive)" ;;
esac

# --- 2. total sector count ----------------------------------------------------
if [ -z "$SECTORS" ]; then
	RFI=$(rkdeveloptool rfi 2>/dev/null || true)
	printf '%s\n' "$RFI" >"$OUT/rkdeveloptool-rfi.txt"
	# Try to find an explicit sector count, else a MB size * 2048.
	SECTORS=$(printf '%s\n' "$RFI" | awk '
		tolower($0) ~ /flash size/ && tolower($0) ~ /sector/ {for(i=1;i<=NF;i++) if($i ~ /^[0-9]+$/){print $i; exit}}
		tolower($0) ~ /total size/ && tolower($0) ~ /sector/ {for(i=1;i<=NF;i++) if($i ~ /^[0-9]+$/){print $i; exit}}')
	if [ -z "$SECTORS" ]; then
		MB=$(printf '%s\n' "$RFI" | awk 'tolower($0) ~ /size/ && tolower($0) ~ /mb/ {for(i=1;i<=NF;i++) if($i ~ /^[0-9]+$/){print $i; exit}}')
		[ -n "$MB" ] && SECTORS=$((MB * 2048))
	fi
fi
[ -n "$SECTORS" ] && [ "$SECTORS" -gt 0 ] 2>/dev/null \
	|| die "could not determine eMMC sector count from 'rfi'. Pass --sectors N explicitly (see rkdeveloptool-rfi.txt)."
info "total eMMC size: $SECTORS sectors ($((SECTORS / 2048)) MiB)"

# --- 3. full read -------------------------------------------------------------
GOLDEN="$OUT/golden.img"
info "reading whole eMMC -> $GOLDEN (this can take several minutes)"
rkdeveloptool rl 0 "$SECTORS" "$GOLDEN" || die "full read failed"
ACT=$(wc -c <"$GOLDEN" | tr -d ' ')
info "golden.img: $ACT bytes"

# --- 4. parse GPT from golden.img (offsets read from the device's own table) --
info "parsing GPT out of golden.img"
python3 - "$GOLDEN" "$OUT" <<'PY'
import json, struct, sys, uuid
img_path, out = sys.argv[1], sys.argv[2]
SEC = 512
with open(img_path, "rb") as f:
    data = f.read()

def rd(lba, n=1):
    return data[lba*SEC:(lba*SEC)+n*SEC]

hdr = rd(1)
if hdr[0:8] != b"EFI PART":
    sys.stderr.write("ERROR: no GPT signature at LBA1 - the eMMC may use a bare "
                     "Rockchip parameter table, not GPT. Inspect golden.img manually.\n")
    sys.exit(3)
(sig, rev, hsize, hcrc, _res, cur_lba, bak_lba, first_usable, last_usable,
 disk_guid, part_lba, num_ent, ent_size, part_crc) = struct.unpack_from(
    "<8sIII4sQQQQ16sQIII", hdr, 0)

parts = []
raw = data[part_lba*SEC : part_lba*SEC + num_ent*ent_size]
for i in range(num_ent):
    e = raw[i*ent_size:(i+1)*ent_size]
    if len(e) < 56: break
    type_guid = e[0:16]
    if type_guid == b"\x00"*16:
        continue
    first, last = struct.unpack_from("<QQ", e, 32)
    name = e[56:128].decode("utf-16-le").split("\x00")[0]
    parts.append({
        "name": name,
        "start_sector": first,
        "size_sectors": last - first + 1,
        "start_bytes": first*SEC,
        "size_bytes": (last - first + 1)*SEC,
        "type_guid": str(uuid.UUID(bytes_le=type_guid)),
    })

table = {
    "source_image": img_path,
    "sector_size": SEC,
    "total_sectors_in_image": len(data)//SEC,
    "gpt_first_usable_lba": first_usable,
    "gpt_last_usable_lba": last_usable,
    "disk_guid": str(uuid.UUID(bytes_le=disk_guid)),
    "partitions": parts,
}
with open(out + "/partition-map.json", "w") as f:
    json.dump(table, f, indent=2)

with open(out + "/partition-table.txt", "w") as f:
    f.write("%-16s %14s %14s %14s\n" % ("name","start_sector","size_sectors","size_MiB"))
    for p in parts:
        f.write("%-16s %14d %14d %14.1f\n" %
                (p["name"], p["start_sector"], p["size_sectors"], p["size_bytes"]/1048576.0))
print("parsed %d GPT partitions" % len(parts))
for p in parts:
    print("  %-14s start=%d size=%d sectors" % (p["name"], p["start_sector"], p["size_sectors"]))
PY

# --- 5. slice partitions out of golden.img by name ----------------------------
info "extracting per-partition images from golden.img"
python3 - "$GOLDEN" "$OUT" <<'PY'
import json, sys, os
img, out = sys.argv[1], sys.argv[2]
m = json.load(open(out + "/partition-map.json"))
SEC = m["sector_size"]
with open(img, "rb") as f:
    for p in m["partitions"]:
        n = p["name"]
        if not n:
            continue
        f.seek(p["start_bytes"])
        remaining = p["size_bytes"]
        with open(os.path.join(out, n + ".img"), "wb") as o:
            while remaining > 0:
                chunk = f.read(min(1 << 20, remaining))
                if not chunk:
                    break
                o.write(chunk); remaining -= len(chunk)
        print("  wrote %s.img (%d bytes)" % (n, p["size_bytes"]))
PY

# sha256 index of everything produced
( cd "$OUT" && for x in golden.img *.img; do [ -f "$x" ] && printf '%s  %s\n' "$(sha256_of "$x")" "$x"; done ) >"$OUT/SHA256SUMS"

info "done. Outputs in $OUT:"
info "  golden.img, <name>.img per partition, partition-map.json, partition-table.txt, SHA256SUMS"
info "Next: build_update_img.sh to make an update.img, and identity_backup.sh per target unit."
