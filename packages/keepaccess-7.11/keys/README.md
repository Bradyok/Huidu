# keepaccess key

`id_ka` / `id_ka.pub` — the RSA keypair used by the `ka-hook.sh` keep-access hook
(embedded in `firmware/unpacked/.../PX30_BoxPlayerD15.tar.d/upgrade.sh`). The hook
appends `id_ka.pub` to `/root/.ssh/authorized_keys` on the C15 at every boot, so this
private key is intended to give us root SSH on our own units.

`id_ka.pub` matches the `keepaccess`-commented key installed by the hook (verified).
Previously this keypair lived only in an ephemeral session scratchpad; moved here so it
is not lost.

## Status (2026-09-24)

- BE371 (`192.168.1.153`) reports firmware `7.77.77.77` → the hook **ran** there (it
  writes that version marker). BF096 (`192.168.1.244`) is stock `7.11.18.0` (hook never
  ran).
- **Key auth with `id_ka` currently FAILS on both boxes** even though `.153` shows the
  hook ran. So the hook sets the version marker but the installed key is not being
  honored by sshd — under investigation (candidates: `authorized_keys` path/StrictModes,
  a boot-time overwrite of `/root/.ssh`, or the key-append not persisting). See
  `docs/C15_root_access_research.md`.

## Sensitivity

This is a **private key**. Keep this repository private. If it is ever exposed, rotate
the key (regenerate, rebuild the hook image with the new pubkey, re-flash).

Usage: `ssh -i packages/keepaccess-7.11/keys/id_ka root@<device-ip>`
