################################################################################
#
# huidu-sender
#
# Our LED sender-card control daemon (see huidu-sender crate). Built from the
# in-repo source with Buildroot's cargo infrastructure. The crate lives beside
# this external tree in the same git checkout:
#
#     <repo>/huidu-sender          <- crate
#     <repo>/br2-huidu             <- BR2_EXTERNAL (this tree)
#
# so the source is the sibling directory of BR2_EXTERNAL_HUIDU_PATH.
#
################################################################################

HUIDU_SENDER_VERSION = 0.1.0
HUIDU_SENDER_SITE = $(BR2_EXTERNAL_HUIDU_PATH)/../huidu-sender
HUIDU_SENDER_SITE_METHOD = local
HUIDU_SENDER_LICENSE = Proprietary
# The workspace holds the resolved Cargo.lock; the crate only depends on `libc`.
# When built standalone the download step regenerates a lockfile for it.

# The crate is a workspace member; built out-of-workspace it uses cargo's
# default release profile, which is fine for the daemon.
define HUIDU_SENDER_INSTALL_INIT_SYSV
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_HUIDU_PATH)/package/huidu-sender/S95huidu-sender \
		$(TARGET_DIR)/etc/init.d/S95huidu-sender
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_HUIDU_PATH)/package/huidu-sender/huidu-sender.default \
		$(TARGET_DIR)/etc/default/huidu-sender
endef

$(eval $(cargo-package))
