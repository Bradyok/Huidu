# PX30 C-series clone & recovery tooling

Scripts to capture one **golden eMMC image** from a Huidu PX30 C-series controller
(C15 / C35 / C36; Rockchip PX30 / RK3326, aarch64) and clone it onto other
same-model units **while preserving each unit's unique identity** (device ID, MAC,
licence, keys). Plan and rationale: [`../../CLONE_AND_RECOVERY.md`](../../CLONE_AND_RECOVERY.md).

> These scripts are written to run **later, against real hardware**. They are
> safe by construction: readbacks are read-only; every destructive step is
> **dry-run by default** and needs `--yes`; **no partition offset is ever
> hard-coded** — offsets are read from the device's own GPT / sysfs at runtime,
> and the scripts fail loudly if they can't get them.

## Files

| Script | What it does | Destructive? |
|---|---|---|
| `cunit_dump.sh` | One-shot hardware/identity dump: resolves the `DRIVER_ENABLEMENT.md` unknowns (DTB/panel timing, GPIO bases, Wi-Fi chip, custom nodes, fpga.img, loaders) into a tarball + `FINDINGS.md` | no (read only) |
| `rk_readback.sh` | USB/maskrom read via `rkdeveloptool`: whole `golden.img` + per-partition `<name>.img` + `partition-map.json` | no (read only) |
| `dd_readback.sh` | Network read over ssh: `dd` each `/dev/block/by-name/*` gzip'd + `partition-map.json` | no (read only) |
| `identity_backup.sh` | Save one unit's per-unit files (+ optional `oem`) with a manifest + sha256 | no (read only) |
| `identity_restore.sh` | Write a saved identity back onto a unit | **yes** (`--yes`) |
| `build_update_img.sh` | Pack a golden readback into a Rockchip RKFW `update.img` (`afptool` + `rkImageMaker`) | no (makes a file) |
| `clone_unit.sh` | Orchestrate: backup identity → flash golden → restore identity → verify checklist | **yes** (`--yes`) |
| `clone_common.sh` | Shared helpers + transport layer (sourced, not run directly) | — |
| `identity_files.list` | **Single source of truth** for the per-unit preserve list | — |

## Per-unit preserve-list (never clone these)

From decompiling `System/BoxUpgrade` / `libCore` / `libSDKServices`. Identity chain
is **`/root/Box/data/id` → MAC (`GenMACWithID`) → `/boot/dev_mac`**. Kept in
`identity_files.list`:

| Item | Path | Required |
|---|---|---|
| Device ID (master) | `/root/Box/data/id` | **yes** |
| MAC cache | `/boot/dev_mac` | optional |
| Licence | `/root/Box/data/license.ini` | optional |
| Device key | `/root/usb_dev/HDPlayerUsbExport/tips/device.key` | optional |
| Device info | `/root/Box/config/dev_info.xml` | optional |
| Management password | `/root/Box/config/device_locker` | optional |
| Server / name | `/root/Box/data/permanentConfig.xml` | optional |
| `oem` partition | raw partition (`/dev/block/by-name/oem`) — holds the MAC per `libSDKServices UpdateMACAddress` | back up via `--oem-dev` |

## Prerequisites

- **`rkdeveloptool`** — build from <https://github.com/rockchip-linux/rkdeveloptool>
  (needs libusb). Used by `rk_readback.sh` and `clone_unit.sh`.
- **PX30 / RK3326 loader** `MiniLoaderAll.bin` — from Rockchip
  [`rkbin`](https://github.com/rockchip-linux/rkbin) (`bin/rk33/…`), or reuse the
  `uboot`/loader you read back from a unit. Needed to init DRAM in maskrom before
  reading/flashing, and as the `bootloader` entry in `update.img`.
- **`afptool` + `rkImageMaker`** — from `rkbin/tools/` or the RKDevTool package.
  Used by `build_update_img.sh`.
- **`python3`** — GPT parsing (`rk_readback.sh`) and `parameter.txt` synthesis.
- **ssh** on the host; stock units run `sshd` with `PermitRootLogin yes`
  (`/root/Box/bin/start-ssh`). `dd_readback.sh` and the network identity paths use it.
- For flashing target units on Windows you can also use **RKDevTool** with the
  produced `update.img`.

### Maskrom entry (PX30 C-series)

Power off. Briefly **short the eMMC CLK line to ground** while applying power over
the **USB-OTG** port so the BootROM can't load the on-flash loader and drops into
MaskROM; it enumerates as USB **`2207:330d`**. Confirm with `rkdeveloptool ld`
(shows *Maskrom*). If you still have shell/serial access, `reboot loader` (reboot
magic `0x5242C301`) reaches **Loader** mode with no shorting needed.

**Always back up first** — capture `golden.img` and each target's identity before
writing anything.

## End-to-end workflow

```sh
# 0. Pick and fully upgrade ONE unit to the latest firmware (via HDPlayer). This
#    is your golden source.

# 1. Read the golden unit over USB (maskrom -> loader). Yields golden.img,
#    <name>.img per partition, partition-map.json (offsets from the device GPT).
./rk_readback.sh out/golden --loader /path/to/MiniLoaderAll.bin
#    (Network alternative when you have ssh instead of USB:)
#    ./dd_readback.sh ssh:root@UNIT out/golden --full

# 2. Pack a flashable update.img from the golden readback.
./build_update_img.sh --images out/golden --loader /path/to/MiniLoaderAll.bin \
                      --from-map out/golden/partition-map.json -o out/update.img
#    (Review the synthesized parameter.txt, or pass a known-good --parameter.)

# 3. For EACH target unit: dry-run first, then --yes.
./clone_unit.sh --target ssh:root@TARGET --workdir out/targets/TARGET \
                --update-img out/update.img --oem-dev /dev/block/by-name/oem
#    inspect the plan, then:
./clone_unit.sh --target ssh:root@TARGET --workdir out/targets/TARGET \
                --update-img out/update.img --oem-dev /dev/block/by-name/oem --yes
```

`clone_unit.sh` does, in order: **back up** the target's identity (aborts if the
master `id` is missing), **flash** the golden image (whole `update.img`, or only
the common partitions via `--images`/`--write-parts`), **restore** the identity,
then print a **verify checklist** (ID, `ifconfig eth0` MAC, licence, discovery).

### Alternative: common-partitions-only clone (keeps identity in place)

Instead of a full `update.img` (which wipes everything, including `userdata`/`oem`),
write only the model-common partitions with `rkdeveloptool wlx <name>`:

```sh
./clone_unit.sh --target ssh:root@TARGET --workdir out/targets/TARGET \
                --images out/golden --write-parts "boot rootfs" --yes
```

The script refuses to write `userdata`/`oem` in this mode, so per-unit data stays.
Identity files are still restored afterward as a safety net.

## Transports (identity scripts)

`identity_backup.sh` / `identity_restore.sh` accept a target as:

- **`ssh:[user@]host`** or **`[user@]host`** — live unit (recommended).
- **a mounted directory** — an offline rootfs mount; `/boot/*` maps to
  `$CLONE_BOOT_DIR` or `<mountdir>/boot`. Useful when the eMMC is mounted on a PC.
- **`telnet:host`** — best-effort, needs `expect`, small files only. Prefer ssh.

Env knobs: `CLONE_SSH_OPTS`, `CLONE_BOOT_DIR`, `CLONE_TELNET_USER/PASS`.

## Assumptions to confirm on real hardware

- The eMMC uses a **GPT** at LBA1 (the RE points to a Rockchip SDK GPT layout).
  `rk_readback.sh` fails loudly if there's no `EFI PART` signature — if that
  happens the unit uses a bare Rockchip parameter table and needs a different parse.
- **`rkImageMaker` chip tag** defaults to `RK330C` for PX30/RK3326 — confirm
  against your `rkbin`/RKDevTool config (`--chip`).
- The synthesized `parameter.txt` (`--from-map`) reproduces the partition
  geometry but **not** the `root=`/`uuid:` CMDLINE specifics — review it, or feed a
  real `parameter.txt` read back from a unit.
- Whether **`oem`** holds an authoritative MAC separate from `/boot/dev_mac`
  (back it up either way with `--oem-dev`).
- `rkdeveloptool rfi` sector-count parsing varies by version — pass `--sectors N`
  if auto-detect fails.
