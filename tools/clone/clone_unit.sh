#!/bin/sh
# clone_unit.sh - clone one golden image onto a TARGET unit while preserving that
# unit's identity. Orchestrates: backup identity -> flash golden -> restore
# identity -> print a verify checklist.
#
#   clone_unit.sh --target <ssh-target> --workdir DIR \
#                 (--update-img update.img [--rk-target maskrom]        # whole-image flash
#                  | --images DIR --write-parts "boot uboot trust ...") # per-partition flash
#                 [--oem-dev /dev/block/by-name/oem] [--yes]
#
#   --target        the unit to clone, as ssh:[user@]host / [user@]host / mountdir
#                   (used for identity backup+restore; must have shell/ssh access).
#   --workdir DIR   where the per-unit identity backup is stored.
#   --update-img F  flash this RKFW update.img with rkdeveloptool (unit in
#                   maskrom/loader over USB). Wipes ALL partitions -> identity is
#                   restored afterwards from the backup.
#   --images DIR    per-partition raw images (from rk_readback.sh) to write by
#                   name with `rkdeveloptool wl` (loader mode) instead of a full
#                   update.img. Use with --write-parts to name the COMMON
#                   partitions only (never userdata/oem) so identity survives.
#   --write-parts   space list of partition names to write from --images.
#   --oem-dev       oem block device on the target (for identity backup/restore).
#
# DESTRUCTIVE. Dry-run by default; requires --yes AND a verified identity backup
# (with the master /root/Box/data/id) before it will flash.
#
# This script SHELLS OUT to the siblings in this dir (identity_backup.sh,
# identity_restore.sh) and to rkdeveloptool. It never invents partition offsets.
set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"

TARGET=""; WORKDIR=""; UPDIMG=""; IMAGES=""; WRITE_PARTS=""; OEM_DEV=""
while [ $# -gt 0 ]; do
	case "$1" in
		--target)      TARGET=$2; shift 2 ;;
		--workdir)     WORKDIR=$2; shift 2 ;;
		--update-img)  UPDIMG=$2; shift 2 ;;
		--images)      IMAGES=$2; shift 2 ;;
		--write-parts) WRITE_PARTS=$2; shift 2 ;;
		--oem-dev)     OEM_DEV=$2; shift 2 ;;
		--yes)         CLONE_YES=1; CLONE_DRYRUN=0; shift ;;
		-h|--help)     sed -n '2,34p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  die "unexpected arg: $1" ;;
	esac
done
[ -n "$TARGET" ]  || die "--target is required"
[ -n "$WORKDIR" ] || die "--workdir is required"
if [ -n "$UPDIMG" ]; then
	[ -f "$UPDIMG" ] || die "--update-img '$UPDIMG' not found"
elif [ -n "$IMAGES" ]; then
	[ -d "$IMAGES" ] || die "--images '$IMAGES' not found"
	[ -n "$WRITE_PARTS" ] || die "--images requires --write-parts \"name name ...\""
else
	die "provide either --update-img OR --images + --write-parts"
fi

DRY=$CLONE_DRYRUN
[ "$DRY" = 1 ] && info "DRY-RUN: will back up identity and PRINT the flash plan (pass --yes to flash)."

BKDIR="$WORKDIR/identity"
mkdir -p "$WORKDIR"

# --- STEP 1: back up target identity (read-only, always runs) ------------------
info "STEP 1/4: backing up target identity -> $BKDIR"
oem_arg=""
[ -n "$OEM_DEV" ] && oem_arg="--oem-dev $OEM_DEV"
# shellcheck disable=SC2086
sh "$HERE/identity_backup.sh" "$TARGET" "$BKDIR" $oem_arg \
	|| die "identity backup failed - ABORTING before any flash"

# hard gate: the master ID must be present in the backup
[ -f "$BKDIR/files/root/Box/data/id" ] \
	|| die "backup has no /root/Box/data/id - refusing to flash (identity would be lost)"
DEVID=$(sed -n 's/^# device-id: //p' "$BKDIR/manifest.txt" | head -1)
info "verified backup for device-id: ${DEVID:-unknown}"

# --- STEP 2: flash golden ------------------------------------------------------
info "STEP 2/4: flash golden onto target"
if [ -n "$UPDIMG" ]; then
	info "  plan: rkdeveloptool uf $UPDIMG   (unit in maskrom/loader over USB; wipes all partitions)"
	if [ "$DRY" != 1 ]; then
		require_cmd rkdeveloptool
		confirm "FLASH whole update.img to the target now? Identity backup is at $BKDIR"
		rkdeveloptool uf "$UPDIMG" || die "rkdeveloptool uf failed"
	fi
else
	info "  plan: write only these partitions from $IMAGES (loader mode):"
	for n in $WRITE_PARTS; do
		img="$IMAGES/$n.img"
		[ -f "$img" ] || die "missing image for partition '$n': $img"
		case "$n" in
			userdata|user|oem)
				die "refusing to write '$n' in per-partition mode (it carries per-unit data)" ;;
		esac
		info "    wl by-name $n  <- $img"
	done
	if [ "$DRY" != 1 ]; then
		require_cmd rkdeveloptool
		confirm "WRITE the listed partitions to the target now?"
		for n in $WRITE_PARTS; do
			info "  writing $n ..."
			# write by partition NAME so rkdeveloptool resolves the offset from the
			# device's own GPT (offsets are never passed by hand here).
			rkdeveloptool wlx "$n" "$IMAGES/$n.img" || die "rkdeveloptool wlx $n failed"
		done
	fi
fi

# --- STEP 3: restore identity --------------------------------------------------
info "STEP 3/4: restore identity onto target"
if [ "$DRY" = 1 ]; then
	# shellcheck disable=SC2086
	sh "$HERE/identity_restore.sh" "$BKDIR" "$TARGET" $oem_arg   # dry-run inside too
else
	info "  (the unit must be booted / reachable again on '$TARGET' for restore)"
	confirm "Target is booted and reachable for identity restore?"
	# shellcheck disable=SC2086
	sh "$HERE/identity_restore.sh" "$BKDIR" "$TARGET" $oem_arg --yes \
		|| die "identity restore FAILED - unit is generic! backup preserved at $BKDIR"
fi

# --- STEP 4: verify checklist --------------------------------------------------
info "STEP 4/4: verify checklist (run these on the target after it boots):"
cat >&2 <<EOF
  ------------------------------------------------------------------
  [ ] ID matches backup:   cat /root/Box/data/id     (== $DEVID)
  [ ] MAC applied:         ifconfig eth0 | grep HWaddr   (from ID / /boot/dev_mac)
  [ ] Licence present:     ls -l /root/Box/data/license.ini
  [ ] Device key:          ls -l /root/usb_dev/HDPlayerUsbExport/tips/device.key
  [ ] dev_info/locker:     ls -l /root/Box/config/dev_info.xml /root/Box/config/device_locker
  [ ] Firmware version:    cat /root/Box/version/version
  [ ] Discovery:           unit appears in HDPlayer with its own name/ID
  [ ] oem MAC (if used):   restored to $OEM_DEV
  ------------------------------------------------------------------
  Identity backup kept at: $BKDIR
EOF
[ "$DRY" = 1 ] && info "DRY-RUN complete. Re-run with --yes to perform the clone."
