################################################################################
#
# huidu-player  (the `boxplayer` binary)
#
# Our reproduction of Huidu's on-device display player. Built from the in-repo
# `huidu-player` crate (sibling of BR2_EXTERNAL_HUIDU_PATH), which also pulls in
# the `huidu-protocol` crate at ../huidu-protocol — so the local site is the
# repo root and we build the specific package.
#
# Native (system) library deps, resolved via pkg-config by the -sys crates:
#   openssl   (reqwest / native-tls)
#   alsa-lib  (rodio)
# (serialport uses default-features=false, so no libudev/eudev dep.)
#
# NOTE: this is the heavy package. The cross-build of this dependency tree must
# be validated in the WSL Buildroot run; treat a first `make huidu-player` as a
# build-confirm step (see huidu-sender/README.md "confirm-on-hardware" for the
# analogous discipline).
#
################################################################################

HUIDU_PLAYER_VERSION = 0.1.0
# Build from the workspace root so the huidu-protocol path dep resolves, then
# select just the boxplayer package.
#
# NOTE: SITE_METHOD=local rsyncs the whole workspace root (its only hardcoded
# exclude is .git). Run the Buildroot build from a CLEAN checkout, or `cargo
# clean` first, so the multi-GB `target/` isn't copied on every build. A future
# refinement is a git-archive-based export of just {huidu-player, huidu-protocol,
# Cargo.toml, Cargo.lock}.
HUIDU_PLAYER_SITE = $(BR2_EXTERNAL_HUIDU_PATH)/..
HUIDU_PLAYER_SITE_METHOD = local
HUIDU_PLAYER_LICENSE = Proprietary

HUIDU_PLAYER_DEPENDENCIES = host-rustc openssl alsa-lib

# cargo-package builds the whole workspace by default; restrict to boxplayer.
HUIDU_PLAYER_CARGO_BUILD_OPTS = -p boxplayer
HUIDU_PLAYER_CARGO_INSTALL_OPTS = -p boxplayer

# openssl-sys / alsa-sys / libudev-sys find their libs through Buildroot's
# staging pkg-config (set up by the cargo infra); no extra env needed.

define HUIDU_PLAYER_INSTALL_INIT_SYSV
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_HUIDU_PATH)/package/huidu-player/S96huidu-player \
		$(TARGET_DIR)/etc/init.d/S96huidu-player
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_HUIDU_PATH)/package/huidu-player/huidu-player.default \
		$(TARGET_DIR)/etc/default/huidu-player
endef

$(eval $(cargo-package))
