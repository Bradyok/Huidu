#!/bin/sh
# identity_backup.sh - save the per-unit identity of one Huidu PX30 C-series unit.
#
#   identity_backup.sh <target> <outdir> [--oem-dev DEV | --oem-img FILE] [--yes]
#
#   <target>  a mounted rootfs dir, ssh:[user@]host, telnet:host, or user@host
#             (see clone_common.sh rt_init for the exact forms).
#   <outdir>  where to write the backup (created; must be empty or new).
#
# NON-destructive (read-only on the unit). It copies exactly the files listed in
# identity_files.list, optionally the `oem` partition, and writes:
#   manifest.txt   human-readable list: status, size, sha256, path
#   manifest.json  machine-readable manifest (consumed by identity_restore.sh)
#   files/<path>   the backed-up files, under their device-absolute paths
#   oem.img        the oem partition image, if --oem-dev/--oem-img was given
#
# The device ID (/root/Box/data/id) is REQUIRED: if it is missing the backup
# aborts, because it is the master identity everything else keys off.
set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"
LIST="$HERE/identity_files.list"

OEM_DEV=""; OEM_IMG=""; POS=""
while [ $# -gt 0 ]; do
	case "$1" in
		--oem-dev) OEM_DEV=$2; shift 2 ;;
		--oem-img) OEM_IMG=$2; shift 2 ;;
		--yes)     CLONE_YES=1; shift ;;      # accepted for symmetry; backup is read-only
		-h|--help) sed -n '2,20p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  POS="$POS $1"; shift ;;
	esac
done
# shellcheck disable=SC2086
set -- $POS
[ $# -eq 2 ] || die "usage: identity_backup.sh <target> <outdir> [--oem-dev DEV|--oem-img FILE]"
TARGET=$1; OUT=$2
[ -f "$LIST" ] || die "identity_files.list not found next to this script"

rt_init "$TARGET"
mkdir -p "$OUT/files"
MANIFEST="$OUT/manifest.txt"
JSON="$OUT/manifest.json"
: >"$MANIFEST"
printf '# identity backup  target=%s  date=%s\n' "$TARGET" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$MANIFEST"

# --- capture the device ID string first (for labelling), read-only ------------
DEVID_FILE="$OUT/files/root/Box/data/id"
DEVID="unknown"
if rt_exists /root/Box/data/id; then
	mkdir -p "$(dirname "$DEVID_FILE")"
	rt_get /root/Box/data/id "$DEVID_FILE"
	DEVID=$(tr -c 'A-Za-z0-9._-' ' ' <"$DEVID_FILE" | awk '{print $1}')
	[ -n "$DEVID" ] || DEVID="unknown"
fi
info "device ID: $DEVID"
printf '# device-id: %s\n' "$DEVID" >>"$MANIFEST"

# --- json manifest header -----------------------------------------------------
{
	printf '{\n'
	printf '  "target": "%s",\n' "$TARGET"
	printf '  "device_id": "%s",\n' "$DEVID"
	printf '  "date": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
	printf '  "files": [\n'
} >"$JSON"

first=1
emit_json() {  # path status size sha256
	[ $first = 1 ] || printf ',\n' >>"$JSON"
	first=0
	printf '    {"path": "%s", "status": "%s", "size": %s, "sha256": "%s"}' \
		"$1" "$2" "${3:-0}" "${4:-}" >>"$JSON"
}

# --- walk the identity list ---------------------------------------------------
MISSING_REQUIRED=0
# shellcheck disable=SC2162
while read req path rest; do
	case "$req" in ''|'#'*) continue ;; esac
	dest="$OUT/files$path"
	if rt_exists "$path"; then
		mkdir -p "$(dirname "$dest")"
		# id may already be fetched above; re-fetching is harmless
		rt_get "$path" "$dest"
		sz=$(wc -c <"$dest" | tr -d ' ')
		sha=$(sha256_of "$dest")
		printf 'OK    %10s  %s  %s\n' "$sz" "$sha" "$path" >>"$MANIFEST"
		emit_json "$path" present "$sz" "$sha"
		info "backed up $path ($sz bytes)"
	else
		if [ "$req" = required ]; then
			printf 'MISSING(REQUIRED)          %s\n' "$path" >>"$MANIFEST"
			emit_json "$path" missing_required 0 ""
			warn "REQUIRED file missing on target: $path"
			MISSING_REQUIRED=1
		else
			printf 'absent                     %s\n' "$path" >>"$MANIFEST"
			emit_json "$path" absent 0 ""
			info "absent (optional): $path"
		fi
	fi
done <"$LIST"

# --- oem partition (optional) -------------------------------------------------
if [ -n "$OEM_DEV" ] || [ -n "$OEM_IMG" ]; then
	oem_out="$OUT/oem.img"
	if [ -n "$OEM_IMG" ]; then
		[ -f "$OEM_IMG" ] || die "--oem-img '$OEM_IMG' not found"
		cp "$OEM_IMG" "$oem_out"
	else
		[ "$RT_KIND" = ssh ] && rt_ddget "$OEM_DEV" "$oem_out.gz" && gunzip -f "$oem_out.gz" \
			|| { info "reading oem via cat (non-ssh)"; rt_get "$OEM_DEV" "$oem_out"; }
	fi
	sz=$(wc -c <"$oem_out" | tr -d ' ')
	sha=$(sha256_of "$oem_out")
	printf 'OK    %10s  %s  %s\n' "$sz" "$sha" "oem.img (${OEM_DEV:-$OEM_IMG})" >>"$MANIFEST"
	emit_json "oem.img" present "$sz" "$sha"
	info "backed up oem partition ($sz bytes)"
else
	warn "oem partition NOT backed up (pass --oem-dev /dev/block/by-name/oem or --oem-img)"
fi

printf '\n  ]\n}\n' >>"$JSON"

info "manifest: $MANIFEST"
[ "$MISSING_REQUIRED" = 0 ] || die "backup INCOMPLETE: required identity file(s) missing - do NOT clone this unit yet"
info "identity backup complete for '$DEVID' -> $OUT"
