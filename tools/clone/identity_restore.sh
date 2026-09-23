#!/bin/sh
# identity_restore.sh - write a saved per-unit identity back onto a unit.
#
#   identity_restore.sh <indir> <target> [--oem-dev DEV] [--no-oem] [--yes]
#
#   <indir>   a backup directory produced by identity_backup.sh
#   <target>  a mounted rootfs dir, ssh:[user@]host, telnet:host, or user@host
#
# DESTRUCTIVE: it overwrites identity files (and optionally the oem partition) on
# the target. Dry-run by default: it verifies the backup and PRINTS what it would
# write. Pass --yes to actually write.
#
# Every file is verified against manifest.json (sha256) before it is pushed, so a
# corrupt backup cannot be flashed. The device ID is written first.
set -eu
HERE=$(CDPATH= cd "$(dirname "$0")" && pwd)
. "$HERE/clone_common.sh"

OEM_DEV=""; DO_OEM=1; POS=""
while [ $# -gt 0 ]; do
	case "$1" in
		--oem-dev) OEM_DEV=$2; shift 2 ;;
		--no-oem)  DO_OEM=0; shift ;;
		--yes)     CLONE_YES=1; CLONE_DRYRUN=0; shift ;;
		-h|--help) sed -n '2,18p' "$0"; exit 0 ;;
		-*) die "unknown option: $1" ;;
		*)  POS="$POS $1"; shift ;;
	esac
done
# shellcheck disable=SC2086
set -- $POS
[ $# -eq 2 ] || die "usage: identity_restore.sh <indir> <target> [--oem-dev DEV] [--no-oem]"
IN=$1; TARGET=$2
[ -d "$IN/files" ] || die "'$IN' does not look like an identity backup (no files/)"
[ -f "$IN/manifest.txt" ] || die "no manifest.txt in '$IN'"

rt_init "$TARGET"

DRY=$CLONE_DRYRUN
[ "$DRY" = 1 ] && info "DRY-RUN (no changes will be written; pass --yes to apply)"

# --- verify every present file against its recorded sha256 --------------------
# manifest.txt lines: "OK  <size>  <sha256>  <path>"
verify_one() {  # path sha256
	f="$IN/files$1"
	[ -f "$f" ] || { warn "listed file not in backup: $1"; return 1; }
	got=$(sha256_of "$f")
	[ "$got" = "$2" ] || { warn "sha256 MISMATCH for $1"; return 1; }
	return 0
}

BAD=0
while read -r st sz sha path _; do
	[ "$st" = OK ] || continue
	case "$path" in oem.img) continue ;; esac    # oem handled separately
	verify_one "$path" "$sha" || BAD=1
done <"$IN/manifest.txt"
[ "$BAD" = 0 ] || die "backup failed verification - refusing to restore"
info "backup verified against manifest sha256s"

# --- confirm before writing ---------------------------------------------------
DEVID=$(sed -n 's/^# device-id: //p' "$IN/manifest.txt" | head -1)
info "restoring identity for device-id: ${DEVID:-unknown}  ->  $TARGET"
[ "$DRY" = 1 ] || confirm "About to OVERWRITE identity files on $TARGET"

# --- push files (id first) ----------------------------------------------------
push_one() {  # path
	src="$IN/files$1"
	[ -f "$src" ] || return 0
	if [ "$DRY" = 1 ]; then
		info "WOULD write $(wc -c <"$src" | tr -d ' ') bytes -> $1"
	else
		rt_put "$src" "$1"
		info "wrote $1"
	fi
}

# id first so the identity chain is consistent even if a later step fails
[ -f "$IN/files/root/Box/data/id" ] && push_one /root/Box/data/id

while read -r st sz sha path _; do
	[ "$st" = OK ] || continue
	case "$path" in oem.img|/root/Box/data/id) continue ;; esac
	push_one "$path"
done <"$IN/manifest.txt"

# --- oem partition ------------------------------------------------------------
if [ "$DO_OEM" = 1 ] && [ -f "$IN/oem.img" ]; then
	if [ -z "$OEM_DEV" ]; then
		warn "oem.img present but --oem-dev not given; skipping oem restore"
	elif [ "$DRY" = 1 ]; then
		info "WOULD write oem.img -> $OEM_DEV ($(wc -c <"$IN/oem.img" | tr -d ' ') bytes)"
	else
		confirm "About to OVERWRITE oem partition $OEM_DEV on $TARGET"
		case "$RT_KIND" in
			ssh)   ssh $CLONE_SSH_OPTS "$RT_HOST" "cat > '$OEM_DEV'" <"$IN/oem.img" ;;
			local) rt_put "$IN/oem.img" "$OEM_DEV" ;;
			*)     die "oem restore over telnet is unsupported (binary/size); use ssh" ;;
		esac
		info "wrote oem partition -> $OEM_DEV"
	fi
fi

if [ "$DRY" = 1 ]; then
	info "dry-run complete. Re-run with --yes to apply."
else
	info "identity restore complete. Verify: ID, 'ifconfig eth0' MAC, licence, discovery."
fi
