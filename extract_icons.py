#!/usr/bin/env python3
"""Extract embedded PNG resources from HDPlayer Qt DLLs."""

import os
import struct
import sys
from pathlib import Path

PNG_MAGIC = b'\x89PNG\r\n\x1a\n'
IEND_CHUNK = b'IEND\xaeB`\x82'

def extract_pngs_from_file(src: Path, out_dir: Path):
    data = src.read_bytes()
    count = 0
    pos = 0
    while True:
        idx = data.find(PNG_MAGIC, pos)
        if idx == -1:
            break
        # Find the IEND chunk to know where the PNG ends
        end_idx = data.find(IEND_CHUNK, idx)
        if end_idx == -1:
            pos = idx + 1
            continue
        end_idx += len(IEND_CHUNK)
        png_data = data[idx:end_idx]
        out_path = out_dir / f"{src.stem}_{count:04d}.png"
        out_path.write_bytes(png_data)
        print(f"  [{count:04d}] {out_path.name}  ({len(png_data)} bytes)")
        count += 1
        pos = end_idx
    return count

# Also try to extract Qt RCC resources (qresource format)
def find_qt_resources(src: Path, out_dir: Path):
    data = src.read_bytes()
    # Qt RCC magic: "qres" at offset 0 or embedded
    count = 0
    pos = 0
    rcc_magic = b'qres'
    while True:
        idx = data.find(rcc_magic, pos)
        if idx == -1:
            break
        print(f"  Found Qt RCC marker at offset {idx:#x}")
        pos = idx + 1
        count += 1
    return count

SEARCH_DIR = Path(r"C:\Users\Owner\Documents\GitHub\Huidu\hdplayer_extracted")
OUT_DIR = Path(r"C:\Users\Owner\Documents\GitHub\Huidu\hdplayer-client\assets\extracted")
OUT_DIR.mkdir(parents=True, exist_ok=True)

# Priority DLLs first, then scan all
priority = [
    "MainWindow.dll",
    "hguilib.dll",
    "hcommon.dll",
    "HDPlayer.exe",
]

total = 0
for name in priority:
    p = SEARCH_DIR / name
    if not p.exists():
        # try subdirs
        found = list(SEARCH_DIR.rglob(name))
        if not found:
            print(f"NOT FOUND: {name}")
            continue
        p = found[0]
    sub = OUT_DIR / p.stem
    sub.mkdir(exist_ok=True)
    print(f"\n=== {p.name} ({p.stat().st_size // 1024} KB) ===")
    n = extract_pngs_from_file(p, sub)
    print(f"  -> {n} PNGs extracted")
    total += n

# Also scan all DLLs not yet covered
print("\n=== Scanning remaining DLLs/EXEs ===")
done = {n.lower() for n in priority}
for p in sorted(SEARCH_DIR.glob("*.dll")) + sorted(SEARCH_DIR.glob("*.exe")):
    if p.name.lower() in done:
        continue
    sub = OUT_DIR / p.stem
    sub.mkdir(exist_ok=True)
    n = extract_pngs_from_file(p, sub)
    if n:
        print(f"  {p.name}: {n} PNGs")
        total += n

print(f"\nTotal PNGs extracted: {total}")
print(f"Output directory: {OUT_DIR}")
