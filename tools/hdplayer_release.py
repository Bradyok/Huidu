#!/usr/bin/env python3
"""Ingest, diff, decompile, test and promote HDPlayer releases.

Same contract as the Daktronics repo's scripts/promote_release.py (versioned
archive under products/<P>/v<ver>/, content-hash VERSION_DIFF.md, VERSION.json
names the current release, promotion gated on evidence, nothing deleted), but
for a native Qt/C++ app: Ghidra instead of ILSpy, and a device test instead of
a dotnet build as the gate.

An HDPlayer install is ~1.1 GB / ~4000 files, almost all of it Qt, ffmpeg,
VLC, USB drivers, sample images and HDSet gamma/scan tables. Only Huidu's own
code and config is kept; everything is still hashed into the manifest.

Layout (products/HDPlayer/):
  VERSION.json               current release + per-version status and tests
  v<ver>/MANIFEST.json       every file: path, size, sha256, category, PE info  [committed]
  v<ver>/VERSION_DIFF.md     first-party changes vs. the previous release       [committed]
  v<ver>/exports/            export list per first-party PE                     [committed]
  v<ver>/full/               first-party files only                             [local]
  v<ver>/strings/            ASCII + UTF-16 strings per PE                      [local]
  v<ver>/decompiled/         Ghidra C output per PE                             [local]

Lifecycle:  ingested -> tested (pass) -> promoted   (or failed)

Usage:
  python tools/hdplayer_release.py ingest "C:/Program Files/HDPlayer_7.11.18.0"
  python tools/hdplayer_release.py decompile 7.11.18.0 [--changed | --all | NAME ...]
  python tools/hdplayer_release.py test 7.11.18.0 --pass --notes "upgrade + program send OK"
  python tools/hdplayer_release.py promote 7.11.18.0
  python tools/hdplayer_release.py status
"""

import argparse
import datetime as dt
import fnmatch
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pefile

REPO = Path(__file__).resolve().parent.parent
PRODUCT = REPO / "products" / "HDPlayer"
REGISTRY = PRODUCT / "VERSION.json"
GHIDRA_SCRIPTS = REPO / "tools" / "ghidra"

# Decompiled by default: the binaries that carry the device protocol, program
# format and upgrade path.
PRIORITY_BINARIES = [
    "HDPlayer.exe", "MainWindow.dll", "NetIOServices.dll", "HCatNet.dll",
    "HDownloadManger.dll", "hcommon.dll", "upgrade.exe",
]

# --- Classification ---------------------------------------------------------
# Paths are matched lowercase with forward slashes, relative to the install dir.

# Whole directories that are never first-party code.
ASSET_DIRS = [
    "images/", "neon_gif_background/", "neonpic/",
    "hdset/gamatable/", "hdset/scantable/", "hdset/normalmodule/",
    "hdset/lstemp/", "hdset/language/",
]
# Sending/receiving-card firmware blobs: hashed so the diff shows new firmware
# versions, but not copied (~115 MB).
FIRMWARE_DIRS = ["hdset/local/"]
FIRMWARE_NAMES = ["*.rbf"]
THIRD_PARTY_DIRS = [
    "plugins/vlc/", "plugins/imageformats/", "plugins/sqldrivers/",
    "platforms/", "styles/", "bearer/", "iconengines/", "printsupport/",
    "translations/", "resources/", "hdset/translations/", "hdset/resources/",
    "hdset/plugins/", "hdset/platforms/", "hdset/styles/",
    "hdset/screentesttool/translations/", "hffplay/translations/",
    "cp210x_vcp_windows xp/", "cp210x_windows_drivers/", "nginx/",
    "hdset/driver/", "hdset/cp210x_vcp_windows xp/", "hdset/cp210x_windows_drivers/",
    "hdset/ch341ser/", "hdset/ch343ser_drivers/", "hdset/hdvp-config/",
    "hdset/lvgl_image_convert_tool-master/", "hdset/screentesttool/iconengines/",
    "hdset/screentesttool/imageformats/", "hdset/screentesttool/platforms/",
]
# Config inside otherwise-skipped dirs that is still worth keeping.
KEEP_ANYWAY = ["nginx/conf/*.conf", "lang/lang_en.qm", "lang/lang_zh.qm"]

THIRD_PARTY_NAMES = [
    "qt5*.dll", "qtwebengineprocess.exe", "av*-[0-9]*.dll", "sw*-[0-9]*.dll",
    "postproc-*.dll", "api-ms-win-*.dll", "ucrtbase.dll", "msvc*.dll",
    "vcruntime*.dll", "concrt*.dll", "vccorlib*.dll", "d3dcompiler_*.dll",
    "d3dx*.dll", "sdl2.dll", "libcrypto*.dll", "libssl*.dll", "libeay32.dll",
    "ssleay32.dll", "libcurl*.dll", "freetype*.dll", "mediainfo*.dll",
    "putty.exe", "ffmpeg.exe", "libvlc*.dll", "libgcc*.dll", "libstdc*.dll",
    "libwinpthread*.dll", "zlib*.dll", "opengl32sw.dll", "libegl*.dll",
    "libglesv2*.dll", "vc_redist*.exe", "uninstall*.exe", "icudt*.dat",
    "*.manifest", "delegated-apnic-latest.txt", "hdmcoder.exe", "hdffmpeg.exe",
    "pdftopng.exe", "bz2.dll", "zip.dll", "zstd.dll", "lzma.dll", "libfaac.dll",
    "crash_report.dll", "crash_sender.exe", "crashrpt_lang.ini", "quazip.dll",
    "hidapi.dll",
]
THIRD_PARTY_COMPANIES = [
    "microsoft", "the qt company", "ffmpeg", "videolan", "silicon lab",
    "mediaarea", "simon tatham", "free software foundation", "curl",
    "openssl", "glyph & cog", "mplayer", "php group", "tukaani",
    "yann collet", "wch.cn", "libusb", "chipone", "axs-technology",
]
ASSET_EXTS = {
    ".bmp", ".png", ".gif", ".jpg", ".jpeg", ".ico", ".svg", ".mp4", ".avi",
    ".mp3", ".wav", ".ttf", ".otf", ".ttc", ".pak", ".qm", ".ssr", ".ssx",
    ".csv", ".bak", ".log", ".zip",
}
PE_EXTS = {".exe", ".dll"}


def _match(rel, patterns):
    name = rel.rsplit("/", 1)[-1]
    return any(fnmatch.fnmatch(name, p) or fnmatch.fnmatch(rel, p) for p in patterns)


def classify(rel, pe_info):
    """Return 'first_party', 'firmware', 'third_party' or 'asset' for a relative path."""
    if _match(rel, KEEP_ANYWAY):
        return "first_party"
    if any(rel.startswith(d) for d in ASSET_DIRS):
        return "asset"
    if any(rel.startswith(d) for d in FIRMWARE_DIRS) or _match(rel, FIRMWARE_NAMES):
        return "firmware"
    if any(rel.startswith(d) for d in THIRD_PARTY_DIRS) or _match(rel, THIRD_PARTY_NAMES):
        return "third_party"
    company = (pe_info or {}).get("version", {}).get("CompanyName", "").lower()
    if any(c in company for c in THIRD_PARTY_COMPANIES):
        return "third_party"
    if Path(rel).suffix in ASSET_EXTS:
        return "asset"
    return "first_party"


# --- PE / file inspection ---------------------------------------------------

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def pe_details(path):
    """(info, exports) for a PE file, or None if it is not one."""
    try:
        pe = pefile.PE(str(path), fast_load=True)
    except pefile.PEFormatError:
        return None
    pe.parse_data_directories(directories=[
        pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"],
        pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"],
        pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"],
    ])
    version = {}
    for fi in getattr(pe, "FileInfo", None) or []:
        for entry in fi:
            for st in getattr(entry, "StringTable", []):
                for k, v in st.entries.items():
                    version[k.decode(errors="replace")] = v.decode(errors="replace").strip()
    imports = sorted({i.dll.decode(errors="replace") for i in getattr(pe, "DIRECTORY_ENTRY_IMPORT", [])})
    exports = []
    if hasattr(pe, "DIRECTORY_ENTRY_EXPORT"):
        exports = [s.name.decode(errors="replace") if s.name else f"#ord{s.ordinal}"
                   for s in pe.DIRECTORY_ENTRY_EXPORT.symbols]
    ts = pe.FILE_HEADER.TimeDateStamp
    info = {
        "machine": pefile.MACHINE_TYPE.get(pe.FILE_HEADER.Machine, hex(pe.FILE_HEADER.Machine)),
        "timestamp": dt.datetime.fromtimestamp(ts, dt.timezone.utc).isoformat() if ts else None,
        "version": version,
        "imports": imports,
        "export_count": len(exports),
    }
    pe.close()
    return info, exports


ASCII_RE = re.compile(rb"[\x20-\x7e]{6,}")
UTF16_RE = re.compile(rb"(?:[\x20-\x7e]\x00){6,}")


def dump_strings(src, dst):
    data = src.read_bytes()
    out = [m.group().decode("ascii") for m in ASCII_RE.finditer(data)]
    out += ["[u16] " + m.group().decode("utf-16le") for m in UTF16_RE.finditer(data)]
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text("\n".join(out), encoding="utf-8")


# --- Registry ---------------------------------------------------------------

def vkey(v):
    return [int(x) for x in v.split(".")]


def load_registry():
    if REGISTRY.exists():
        return json.loads(REGISTRY.read_text(encoding="utf-8"))
    return {"current": None, "versions": {}}


def save_registry(reg):
    REGISTRY.parent.mkdir(parents=True, exist_ok=True)
    REGISTRY.write_text(json.dumps(reg, indent=2) + "\n", encoding="utf-8")


def now():
    return dt.datetime.now().astimezone().isoformat(timespec="seconds")


def detect_version(src):
    m = re.search(r"(\d+\.\d+\.\d+\.\d+)", src.name)
    if m:
        return m.group(1)
    info = pe_details(src / "HDPlayer.exe")
    if info and info[0]["version"].get("FileVersion"):
        return info[0]["version"]["FileVersion"].replace(", ", ".")
    sys.exit("Cannot detect version; pass --version")


def vdir(ver):
    return PRODUCT / f"v{ver}"


def load_manifest(ver):
    path = vdir(ver) / "MANIFEST.json"
    if not path.exists():
        sys.exit(f"No manifest for {ver}; ingest it first")
    return json.loads(path.read_text(encoding="utf-8"))


def read_exports(ver, rel):
    p = vdir(ver) / "exports" / (rel + ".txt")
    return set(p.read_text(encoding="utf-8").split()) if p.exists() else set()


def diff_base(new):
    """The current release, else the newest older ingested one, else None."""
    reg = load_registry()
    cur = reg.get("current")
    if cur and cur != new:
        return cur
    older = [v for v in reg["versions"] if vkey(v) < vkey(new)]
    return max(older, key=vkey) if older else None


def changed_first_party(new, old):
    """(old_files, new_files, added, removed, changed) keyed by lowercase path."""
    a = {f["path"].lower(): f for f in load_manifest(old)["files"] if f["category"] == "first_party"}
    b = {f["path"].lower(): f for f in load_manifest(new)["files"] if f["category"] == "first_party"}
    changed = sorted(k for k in a.keys() & b.keys() if a[k]["sha256"] != b[k]["sha256"])
    return a, b, sorted(b.keys() - a.keys()), sorted(a.keys() - b.keys()), changed


# --- Commands ---------------------------------------------------------------

def cmd_ingest(args):
    src = Path(args.source)
    if not (src / "HDPlayer.exe").exists():
        sys.exit(f"{src} does not look like an HDPlayer install (no HDPlayer.exe)")
    ver = args.version or detect_version(src)
    out = vdir(ver)
    if out.exists() and not args.force:
        sys.exit(f"{ver} already ingested; use --force to redo")
    shutil.rmtree(out, ignore_errors=True)

    files, totals = [], {}
    all_paths = sorted(p for p in src.rglob("*") if p.is_file())
    for i, p in enumerate(all_paths, 1):
        rel = p.relative_to(src).as_posix()
        pe = pe_details(p) if p.suffix.lower() in PE_EXTS else None
        category = classify(rel.lower(), pe[0] if pe else None)
        entry = {"path": rel, "size": p.stat().st_size, "sha256": sha256(p), "category": category}
        if pe:
            entry["pe"] = pe[0]
        files.append(entry)
        t = totals.setdefault(category, [0, 0])
        t[0] += 1
        t[1] += entry["size"]

        if category == "first_party":
            dst = out / "full" / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(p, dst)
            if pe:
                dump_strings(p, out / "strings" / (rel + ".txt"))
                exp = out / "exports" / (rel + ".txt")
                exp.parent.mkdir(parents=True, exist_ok=True)
                exp.write_text("".join(e + "\n" for e in pe[1]), encoding="utf-8")
        if i % 500 == 0:
            print(f"  {i}/{len(all_paths)} files scanned", flush=True)

    manifest = {
        "product": "HDPlayer", "version": ver, "source": str(src),
        "ingested_at": now(),
        "totals": {k: {"files": v[0], "bytes": v[1]} for k, v in totals.items()},
        "files": files,
    }
    out.mkdir(parents=True, exist_ok=True)
    (out / "MANIFEST.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")

    reg = load_registry()
    reg["versions"][ver] = {"status": "ingested", "ingested_at": manifest["ingested_at"],
                            "source": str(src), "tests": []}
    save_registry(reg)

    print(f"\nIngested HDPlayer {ver} -> {out}")
    for k, v in sorted(totals.items()):
        print(f"  {k:12} {v[0]:5} files  {v[1] / 1e6:8.1f} MB")
    write_diff(ver, None)


def write_diff(new, old):
    old = old or diff_base(new)
    if not old:
        print("diff: no earlier version ingested - skipping VERSION_DIFF.md")
        return
    a, b, added, removed, changed = changed_first_party(new, old)

    lines = [f"# HDPlayer {new} vs {old} — first-party changes", "",
             f"Added: {len(added)} · Removed: {len(removed)} · Changed: {len(changed)} · "
             f"Unchanged: {len(a.keys() & b.keys()) - len(changed)}", ""]
    if added:
        lines += ["## Added", ""] + [f"- `{b[k]['path']}`" for k in added] + [""]
    if removed:
        lines += ["## Removed", ""] + [f"- `{a[k]['path']}`" for k in removed] + [""]
    if changed:
        lines += ["## Changed", "", "| File | Size Δ | Version | Exports +/- |", "|---|---|---|---|"]
        for k in changed:
            fa, fb = a[k], b[k]
            va = fa.get("pe", {}).get("version", {}).get("FileVersion", "")
            vb = fb.get("pe", {}).get("version", {}).get("FileVersion", "")
            ea, eb = read_exports(old, fa["path"]), read_exports(new, fb["path"])
            exp = f"+{len(eb - ea)} / -{len(ea - eb)}" if (ea or eb) else ""
            ver_s = f"{va} → {vb}" if va != vb else vb
            lines.append(f"| `{fb['path']}` | {fb['size'] - fa['size']:+,} | {ver_s} | {exp} |")
        lines.append("")
        for k in changed:
            ea, eb = read_exports(old, a[k]["path"]), read_exports(new, b[k]["path"])
            if ea != eb:
                lines += [f"### Export changes: `{b[k]['path']}`", "", "```diff"]
                lines += [f"+ {e}" for e in sorted(eb - ea)] + [f"- {e}" for e in sorted(ea - eb)]
                lines += ["```", ""]

    fa = {f["path"] for f in load_manifest(old)["files"] if f["category"] == "firmware"}
    fb = {f["path"] for f in load_manifest(new)["files"] if f["category"] == "firmware"}
    if fa != fb:
        lines += ["## Firmware blobs (not archived, hashed only)", ""]
        lines += [f"- added `{x}`" for x in sorted(fb - fa)] + [f"- removed `{x}`" for x in sorted(fa - fb)]
        lines.append("")

    out = vdir(new) / "VERSION_DIFF.md"
    out.write_text("\n".join(lines), encoding="utf-8")
    print("\n".join(lines[:3]))
    print(f"diff: {out}")


def cmd_diff(args):
    write_diff(args.version, args.against)


def ghidra_home():
    for c in (os.environ.get("GHIDRA_DIR"), os.environ.get("GHIDRA_INSTALL_DIR"),
              r"C:\ghidra", r"C:\Tools\Ghidra", r"C:\Control4\tools\ghidra"):
        if not c or not Path(c).is_dir():
            continue
        for root in [Path(c), *sorted(Path(c).glob("ghidra_*_PUBLIC"), reverse=True)]:
            if (root / "support" / "analyzeHeadless.bat").exists():
                return root
    sys.exit("Ghidra not found; set GHIDRA_DIR")


def cmd_decompile(args):
    ver = args.version
    pes = [f["path"] for f in load_manifest(ver)["files"]
           if f["category"] == "first_party" and "pe" in f]
    if args.all:
        targets = pes
    elif args.names:
        want = {n.lower() for n in args.names}
        targets = [p for p in pes if p.lower() in want or p.rsplit("/", 1)[-1].lower() in want]
    elif args.changed:
        old = diff_base(ver) or sys.exit("--changed needs an earlier ingested version")
        _, b, added, _, changed = changed_first_party(ver, old)
        targets = [b[k]["path"] for k in added + changed if b[k]["path"] in pes]
    else:
        prio = {n.lower() for n in PRIORITY_BINARIES}
        targets = [p for p in pes if p.lower() in prio]
    if not targets:
        sys.exit("No matching first-party PE binaries")

    headless = ghidra_home() / "support" / "analyzeHeadless.bat"
    failed = []
    with tempfile.TemporaryDirectory(prefix="hd_ghidra_") as proj:
        for i, rel in enumerate(targets, 1):
            out = vdir(ver) / "decompiled" / (rel + ".c")
            if out.exists() and not args.force:
                print(f"[{i}/{len(targets)}] {rel}: already decompiled")
                continue
            out.parent.mkdir(parents=True, exist_ok=True)
            print(f"[{i}/{len(targets)}] {rel} ...", flush=True)
            log = out.with_name(out.name + ".log")
            with open(log, "w", encoding="utf-8") as lf:
                r = subprocess.run([str(headless), proj, f"p{i}",
                                    "-import", str(vdir(ver) / "full" / rel),
                                    "-scriptPath", str(GHIDRA_SCRIPTS),
                                    "-postScript", "DecompileExport.java", str(out),
                                    "-deleteProject"], stdout=lf, stderr=subprocess.STDOUT)
            ok = r.returncode == 0 and out.exists()
            print(f"    {'ok' if ok else 'FAILED, see ' + str(log)}", flush=True)
            if not ok:
                failed.append(rel)
    if failed:
        sys.exit(f"{len(failed)} failed: {', '.join(failed)}")


def cmd_test(args):
    reg = load_registry()
    v = reg["versions"].get(args.version) or sys.exit(f"{args.version} not ingested")
    result = "pass" if args.passed else "fail"
    v["tests"].append({"at": now(), "result": result, "notes": args.notes})
    if v["status"] != "promoted":
        v["status"] = "tested" if result == "pass" else "failed"
    save_registry(reg)
    print(f"{args.version}: recorded {result}")


def cmd_promote(args):
    reg = load_registry()
    v = reg["versions"].get(args.version) or sys.exit(f"{args.version} not ingested")
    last = v["tests"][-1] if v["tests"] else None
    if (not last or last["result"] != "pass") and not args.force:
        sys.exit(f"{args.version}: latest test is not a pass; run `test --pass` first or use --force")
    prev = reg.get("current")
    if prev and prev in reg["versions"] and prev != args.version:
        reg["versions"][prev]["status"] = "superseded"
    v["status"], v["promoted_at"] = "promoted", now()
    reg["current"] = args.version
    save_registry(reg)
    print(f"Promoted {args.version} (was {prev})")


def cmd_status(_args):
    reg = load_registry()
    print(f"current: {reg.get('current')}")
    for ver, v in sorted(reg["versions"].items(), key=lambda kv: vkey(kv[0])):
        last = v["tests"][-1] if v["tests"] else None
        t = f"  last test: {last['result']} {last['at']} {last['notes']}" if last else ""
        dec = vdir(ver) / "decompiled"
        n = len(list(dec.rglob("*.c"))) if dec.exists() else 0
        print(f"  {ver:12} {v['status']:10} decompiled: {n}{t}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("ingest", help="snapshot an install and diff it")
    p.add_argument("source"); p.add_argument("--version"); p.add_argument("--force", action="store_true")
    p = sub.add_parser("diff", help="rewrite VERSION_DIFF.md")
    p.add_argument("version"); p.add_argument("--against")
    p = sub.add_parser("decompile", help="Ghidra-decompile first-party binaries")
    p.add_argument("version"); p.add_argument("names", nargs="*")
    p.add_argument("--changed", action="store_true"); p.add_argument("--all", action="store_true")
    p.add_argument("--force", action="store_true")
    p = sub.add_parser("test", help="record a test result")
    p.add_argument("version")
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--pass", dest="passed", action="store_true")
    g.add_argument("--fail", dest="passed", action="store_false")
    p.add_argument("--notes", default="")
    p = sub.add_parser("promote", help="make a tested version current")
    p.add_argument("version"); p.add_argument("--force", action="store_true")
    sub.add_parser("status")
    args = ap.parse_args()
    {"ingest": cmd_ingest, "diff": cmd_diff, "decompile": cmd_decompile, "test": cmd_test,
     "promote": cmd_promote, "status": cmd_status}[args.cmd](args)


if __name__ == "__main__":
    main()
