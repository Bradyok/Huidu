# BoxPlayer release archive

BoxPlayer is device-side firmware (aarch64 Linux, PX30), not part of the
HDPlayer PC install. v7.11.18.0 was taken from `firmware_extract/PX30_D15/BoxPlayer`
(firmware `version/version` = 7.11.18.0, limit 7.6.31.0), matching HDPlayer 7.11.18.0.

Per version (`v<ver>/`), same layout as `products/HDPlayer`:

| Path | Contents | Committed |
|---|---|---|
| `firmware_version/` | component version files from the firmware image | yes |
| `exports/` | demangled dynamic symbols per ELF (`nm -D --defined-only \| c++filt`) | yes |
| `full/` | 36 unique ELFs (deduped by sha256, `.so.N.N.N` → `.so`) | no |
| `strings/` | `strings -n 5` per ELF | no |
| `decompiled/` | Ghidra 12.1.3 headless + `tools/ghidra/DecompileExport.java` | no |

Binaries are stripped, but the dynamic symbol table keeps C++ names, so most
functions in `decompiled/` carry real `Class::method` names. Function counts
in `decompiled/_run.log` include imports, which are not decompiled.
