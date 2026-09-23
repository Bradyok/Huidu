#!/bin/sh
# clone_common.sh - shared helpers for the Huidu PX30 C-series clone/recovery tools.
#
# This file is *sourced* by the identity/orchestration scripts. It provides:
#   - logging + confirmation helpers (info/warn/die/confirm, --yes handling)
#   - a small transport layer so the same identity code works against either a
#     mounted rootfs/boot directory (offline) or a live unit over ssh/telnet:
#         rt_init "<target>"        pick local|ssh|telnet from the target string
#         rt_exists <abs-path>      0 if the file exists on the target
#         rt_size   <abs-path>      byte size of a file, or empty
#         rt_get    <abs> <local>   copy a file OFF the target (binary safe)
#         rt_put    <local> <abs>   copy a file ONTO the target (destructive)
#         rt_run    "<cmd>"         run a shell command on a live target (ssh/telnet)
#         rt_ddget  <dev> <local.gz> stream a block device through gzip (ssh only)
#   - sha256_of / b64d cross-platform wrappers.
#
# NOTHING here is device-specific: no partition offsets, no MACs, no IDs are
# baked in. Offsets are always read from the device at runtime by the callers.
#
# Target string forms understood by rt_init:
#   /path/to/mountdir      -> local  (an *offline* mounted rootfs; /boot maps to
#                                      $CLONE_BOOT_DIR or <mountdir>/boot)
#   ssh:[user@]host        -> ssh    (recommended: stock runs sshd, root login on)
#   [user@]host            -> ssh    (convenience; if it is not an existing dir)
#   telnet:host            -> telnet (best effort, needs `expect`; small files only)

# ---------------------------------------------------------------------------
# logging / safety
# ---------------------------------------------------------------------------
: "${CLONE_YES:=0}"        # set to 1 by callers when --yes was passed
: "${CLONE_DRYRUN:=1}"     # destructive scripts default to dry-run (1)

_prog() { basename "$0"; }
info()  { printf '[%s] %s\n'  "$(_prog)" "$*" >&2; }
warn()  { printf '[%s] WARN: %s\n' "$(_prog)" "$*" >&2; }
die()   { printf '[%s] ERROR: %s\n' "$(_prog)" "$*" >&2; exit 1; }

# require_cmd <name> [hint]
require_cmd() {
	command -v "$1" >/dev/null 2>&1 || die "'$1' not found in PATH.${2:+ $2}"
}

# confirm - block a destructive action unless --yes (CLONE_YES=1) was given.
# In an interactive TTY without --yes it asks; non-interactive without --yes aborts.
confirm() {
	msg=${1:-"Proceed?"}
	[ "$CLONE_YES" = 1 ] && return 0
	if [ -t 0 ]; then
		printf '%s [type YES to proceed] ' "$msg" >&2
		read -r ans
		[ "$ans" = YES ] && return 0
	fi
	die "not confirmed (pass --yes to run destructive steps non-interactively)"
}

# ---------------------------------------------------------------------------
# hashing / base64 (portable across busybox, GNU coreutils, macOS)
# ---------------------------------------------------------------------------
sha256_of() {
	# GNU coreutils prefixes the line with '\' and escapes when the *path*
	# contains a backslash/newline; strip that leading escape so the bare hex
	# hash is returned on every host.
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | sed 's/^\\//' | awk '{print $1}'
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$1" | sed 's/^\\//' | awk '{print $1}'
	else
		die "no sha256sum/shasum available on this host"
	fi
}
b64d() {  # decode base64 from stdin -> stdout
	if base64 -d </dev/null >/dev/null 2>&1; then base64 -d
	else base64 -D; fi
}

# ---------------------------------------------------------------------------
# transport layer
# ---------------------------------------------------------------------------
: "${CLONE_SSH_OPTS:=-o BatchMode=yes -o StrictHostKeyChecking=accept-new}"
: "${CLONE_BOOT_DIR:=}"          # override for /boot when using a local mountdir

RT_KIND=""; RT_HOST=""; RT_ROOT=""; RT_BOOT=""

rt_init() {
	t=$1
	case "$t" in
		ssh:*)    RT_KIND=ssh;    RT_HOST=${t#ssh:} ;;
		telnet:*) RT_KIND=telnet; RT_HOST=${t#telnet:} ;;
		*)
			if [ -d "$t" ]; then
				RT_KIND=local
				RT_ROOT=${t%/}
				RT_BOOT=${CLONE_BOOT_DIR:-$RT_ROOT/boot}
			elif printf '%s' "$t" | grep -q '@'; then
				RT_KIND=ssh; RT_HOST=$t          # user@host convenience
			else
				die "target '$t' is neither an existing directory nor ssh:/telnet:/user@host"
			fi ;;
	esac
	case "$RT_KIND" in
		ssh)    require_cmd ssh; info "transport: ssh -> $RT_HOST" ;;
		telnet) command -v expect >/dev/null 2>&1 || die \
		            "telnet transport needs 'expect'. Stock units run sshd (PermitRootLogin yes) - prefer ssh:."
		        info "transport: telnet -> $RT_HOST (best effort, small files only)" ;;
		local)  info "transport: local mountdir root=$RT_ROOT boot=$RT_BOOT" ;;
	esac
}

# map a device-absolute path to a host path for the local (mountdir) transport
_localpath() {
	case "$1" in
		/boot/*) printf '%s/%s' "$RT_BOOT" "${1#/boot/}" ;;
		/boot)   printf '%s' "$RT_BOOT" ;;
		*)       printf '%s%s' "$RT_ROOT" "$1" ;;
	esac
}

# telnet: run one command via expect, echo output between sentinels.
# User/pass come from CLONE_TELNET_USER / CLONE_TELNET_PASS (may be empty for
# passwordless busybox telnetd). Binary-unsafe: used only via base64 wrappers.
_tn_run() {
	require_cmd expect
	CLONE_TELNET_CMD=$1 expect -c '
		set timeout 30
		set host  [lindex [split $env(RT_HOST_EXP) ":"] 0]
		spawn telnet $host
		expect {
			-re "(login|Username):" { send "$env(CLONE_TELNET_USER)\r"; exp_continue }
			-re "(Password):"       { send "$env(CLONE_TELNET_PASS)\r"; exp_continue }
			-re "\[#$] $"           {}
			timeout { exit 2 }
		}
		send "echo __B__; $env(CLONE_TELNET_CMD); echo __E__\r"
		expect "__E__"
	' 2>/dev/null | sed -n '/__B__/,/__E__/p' | sed '1d;$d'
}
_rt_run_line() {  # run a command capturing text output, ssh or telnet
	case "$RT_KIND" in
		ssh)    ssh $CLONE_SSH_OPTS "$RT_HOST" "$1" ;;
		telnet) RT_HOST_EXP=$RT_HOST CLONE_TELNET_USER=${CLONE_TELNET_USER:-} \
		        CLONE_TELNET_PASS=${CLONE_TELNET_PASS:-} _tn_run "$1" ;;
		*)      die "_rt_run_line: not a live transport" ;;
	esac
}

rt_run() {  # binary-safe stdout passthrough (ssh only; telnet is text via _rt_run_line)
	case "$RT_KIND" in
		ssh)    ssh $CLONE_SSH_OPTS "$RT_HOST" "$1" ;;
		telnet) _rt_run_line "$1" ;;
		*)      die "rt_run needs a live (ssh/telnet) target" ;;
	esac
}

rt_exists() {
	case "$RT_KIND" in
		local) [ -e "$(_localpath "$1")" ] ;;
		*)     [ "$(_rt_run_line "[ -e '$1' ] && echo Y || echo N")" = Y ] ;;
	esac
}

rt_size() {
	case "$RT_KIND" in
		local) f=$(_localpath "$1"); [ -e "$f" ] && wc -c <"$f" | tr -d ' ' ;;
		*)     _rt_run_line "wc -c < '$1' 2>/dev/null" | tr -d ' \r' ;;
	esac
}

rt_get() {  # <device-abs-path> <local-file>
	case "$RT_KIND" in
		local)
			f=$(_localpath "$1"); [ -e "$f" ] || return 1
			cp "$f" "$2" ;;
		ssh)
			ssh $CLONE_SSH_OPTS "$RT_HOST" "cat -- '$1'" >"$2" ;;
		telnet)
			_rt_run_line "base64 < '$1'" | tr -d '\r\n ' | b64d >"$2" ;;
	esac
}

rt_put() {  # <local-file> <device-abs-path>   (DESTRUCTIVE on target)
	case "$RT_KIND" in
		local)
			f=$(_localpath "$2"); mkdir -p "$(dirname "$f")"; cp "$1" "$f" ;;
		ssh)
			ssh $CLONE_SSH_OPTS "$RT_HOST" "mkdir -p '$(dirname "$2")'; cat > '$2'" <"$1" ;;
		telnet)
			b64=$(base64 <"$1" | tr -d '\r\n')
			_rt_run_line "mkdir -p '$(dirname "$2")'; printf '%s' '$b64' | base64 -d > '$2'" >/dev/null ;;
	esac
}

rt_ddget() {  # <block-device> <local.gz>   (ssh only; streams gzip)
	[ "$RT_KIND" = ssh ] || die "rt_ddget requires ssh (binary streaming); got $RT_KIND"
	ssh $CLONE_SSH_OPTS "$RT_HOST" "dd if='$1' bs=1M 2>/dev/null | gzip -1" >"$2"
}
