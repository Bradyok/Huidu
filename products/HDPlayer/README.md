# HDPlayer release archive

Managed by `tools/hdplayer_release.py`. `VERSION.json` names the current
(promoted) release; the decompilation docs and Rust clients target that one.

## New release

```
python tools/hdplayer_release.py ingest "C:/Program Files/HDPlayer_<ver>"
python tools/hdplayer_release.py decompile <ver> --changed     # or no flag = priority set, --all
# test the Rust clients against a device running / paired with <ver>
python tools/hdplayer_release.py test <ver> --pass --notes "what was tested, on which card"
python tools/hdplayer_release.py promote <ver>
```

`ingest` hashes every file of the install into `v<ver>/MANIFEST.json` and
copies only first-party code/config into `v<ver>/full/`. Categories:

| Category | Kept? | Examples |
|---|---|---|
| `first_party` | copied | HDPlayer.exe, MainWindow.dll, NetIOServices.dll, plugins/*_plugin.dll, HDSet.exe + HDSet.pdb, HDSetSo/*.so, configs |
| `firmware` | hashed only | HDSet/local sending-card `.bin`, `.rbf` FPGA images |
| `third_party` | hashed only | Qt, ffmpeg/MPlayer, VLC, OpenSSL, USB drivers, PHP, nginx |
| `asset` | hashed only | sample images, neon GIFs, gamma/scan tables, translations |

Rules live at the top of the script; if a new release adds a vendor library
that lands in `first_party`, add it to `THIRD_PARTY_NAMES` / `_COMPANIES`.

`full/`, `strings/` and `decompiled/` are gitignored (regenerable from the
installer). `MANIFEST.json`, `VERSION_DIFF.md` and `exports/` are committed, so
future diffs work even after an old install is gone.

Note: 7.11.8.0 (what `HDPLAYER_DECOMPILATION.md` describes) was never ingested,
so 7.11.18.0 has no VERSION_DIFF; the next release will diff against it.
