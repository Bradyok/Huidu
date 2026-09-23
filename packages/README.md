# Device packages

Firmware-format `.bin` packages we build and send to a Huidu PX30 controller through HDPlayer's
upgrade path (`tools/mkhdplayerbin.py` wraps a tar.gz as an `HDPLAYER` package; the unit runs the
`upgrade.sh` inside as root). Built `.bin` files and `*.password.txt` are gitignored — regenerate
with each `build.py`.

| Package | What it does | Changes on device |
|---|---|---|
| `access-survey/` | Read-only hardware + partition survey; sets a known root password; backs up small partitions to `/root/survey.tar.gz` (and any USB stick). | root password only |
| `keepaccess-7.11/` | Repackages the real 7.11.18.0 C-series app **with** telnet kept up and a known root password, so upgrading from 7.4.x doesn't lose access (stock 7.11 runs `S50telnet stop` + `start-ssh` with creds we don't have). | 7.11 app + telnet kept + root password |

Both target C15/C35/C36. The C-series never flashes a kernel or FPGA in the stock `upgrade.sh`
(guarded by `devType==D15/D35`); the D15 kernel image is excluded from `keepaccess-7.11` anyway.

**Safe order on a bench unit:** run `access-survey` first (full backup + hardware dump), keep the
survey tarball, *then* consider `keepaccess-7.11`. See `../PX30_CUSTOM_OS_PLAN.md`.

Build:
```
python packages/access-survey/build.py            # -> access-survey_*.bin (+ .password.txt)
python packages/keepaccess-7.11/build.py          # -> keepaccess-7.11.18.0_C15.bin (+ .password.txt)
```
