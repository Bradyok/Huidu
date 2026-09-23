#!/usr/bin/env python3
"""Build (or unpack) the boot image the stock Huidu PX30 U-Boot loads from `boot`/`recovery`.

Format, verified against firmware 7.11.18.0 `kernel_d15_ec200T.img` (rebuilds it byte-identically):
  Android boot image header v0, page 2048, kernel @0x10008000, ramdisk @0x11000000,
  second @0x10f00000, tags @0x10000100, empty name/cmdline, id = SHA1(kernel,len,ramdisk,len,second,len).
  The DTB travels in `second` as a Rockchip RSCE resource container:
    block 0: "RSCE" u16 ver=0, u16 tbl_ver=0, u8 hdr_blks=1, u8 tbl_off=1, u8 entry_blks=1, u8 pad, u32 n
    block 1..n: "ENTR" path[256] u32 content_block u32 size   (512-byte blocks)
    contents: each 512-aligned, in table order.
  Stock entries: rk-kernel.dtb, logo.bmp, logo_kernel.bmp.

  mkrkbootimg.py build --kernel Image [--ramdisk rootfs.cpio.gz] --dtb board.dtb
                       [--resource logo.bmp=path ...] [--cmdline ...] -o boot.img
  mkrkbootimg.py unpack boot.img -d outdir
"""

import argparse
import hashlib
import os
import struct
import sys

PAGE = 2048
BLK = 512
ADDR = dict(kernel=0x10008000, ramdisk=0x11000000, second=0x10F00000, tags=0x10000100)


def pad(data, n):
    return data + b"\0" * (-len(data) % n)


def rsce_pack(entries):
    """entries: list of (name, bytes)."""
    n = len(entries)
    hdr = struct.pack("<4sHHBBBBI", b"RSCE", 0, 0, 1, 1, 1, 0, n)
    table, body = b"", b""
    block = 1 + n
    for name, data in entries:
        nm = name.encode()
        if len(nm) >= 256:
            sys.exit(f"resource name too long: {name}")
        table += pad(struct.pack("<4s256sII", b"ENTR", nm, block, len(data)), BLK)
        body += pad(data, BLK)
        block += len(pad(data, BLK)) // BLK
    return pad(hdr, BLK) + table + body


def rsce_unpack(blob):
    magic, _, _, hdr_blks, tbl_off, ent_blks, _, n = struct.unpack_from("<4sHHBBBBI", blob)
    if magic != b"RSCE":
        sys.exit("second stage is not an RSCE container")
    out = []
    for i in range(n):
        e = (tbl_off + i * ent_blks) * BLK
        tag, name, blk, size = struct.unpack_from("<4s256sII", blob, e)
        if tag != b"ENTR":
            sys.exit(f"bad RSCE entry {i}")
        out.append((name.split(b"\0")[0].decode(), blob[blk * BLK: blk * BLK + size]))
    return out


def bootimg(kernel, ramdisk, second, cmdline=b""):
    if len(cmdline) >= 512:
        sys.exit("cmdline too long for v0 header (512)")
    h = hashlib.sha1()
    for part in (kernel, ramdisk, second):
        h.update(part)
        h.update(struct.pack("<I", len(part)))
    hdr = struct.pack(
        "<8s10I16s512s32s1024s", b"ANDROID!",
        len(kernel), ADDR["kernel"], len(ramdisk), ADDR["ramdisk"],
        len(second), ADDR["second"], ADDR["tags"], PAGE, 0, 0,
        b"", cmdline, h.digest(), b"")
    return pad(hdr, PAGE) + pad(kernel, PAGE) + pad(ramdisk, PAGE) + pad(second, PAGE)


def unbootimg(img):
    magic, ks, _, rs, _, ss, _, _, ps, hv, _ = struct.unpack_from("<8s10I", img)
    if magic != b"ANDROID!" or hv != 0:
        sys.exit(f"not an Android v0 boot image (magic={magic!r} version={hv})")
    cmdline = img[64:576].split(b"\0")[0]
    pg = lambda n: -(-n // ps) * ps
    k0 = ps
    r0 = k0 + pg(ks)
    s0 = r0 + pg(rs)
    return img[k0:k0 + ks], img[r0:r0 + rs], img[s0:s0 + ss], cmdline


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--kernel", required=True)
    b.add_argument("--ramdisk")
    b.add_argument("--dtb", required=True)
    b.add_argument("--resource", action="append", default=[], metavar="NAME=PATH",
                   help="extra RSCE entries after rk-kernel.dtb (e.g. logo.bmp=...)")
    b.add_argument("--cmdline", default="")
    b.add_argument("-o", "--output", required=True)
    u = sub.add_parser("unpack")
    u.add_argument("image")
    u.add_argument("-d", "--dir", required=True)
    a = ap.parse_args()

    if a.cmd == "build":
        read = lambda p: open(p, "rb").read()
        entries = [("rk-kernel.dtb", read(a.dtb))]
        for r in a.resource:
            name, _, path = r.partition("=")
            entries.append((name, read(path)))
        img = bootimg(read(a.kernel), read(a.ramdisk) if a.ramdisk else b"",
                      rsce_pack(entries), a.cmdline.encode())
        with open(a.output, "wb") as f:
            f.write(img)
        print(f"{a.output}: {len(img)} bytes, resources: {', '.join(n for n, _ in entries)}")
    else:
        kernel, ramdisk, second, cmdline = unbootimg(open(a.image, "rb").read())
        os.makedirs(a.dir, exist_ok=True)
        open(os.path.join(a.dir, "kernel"), "wb").write(kernel)
        if ramdisk:
            open(os.path.join(a.dir, "ramdisk"), "wb").write(ramdisk)
        for name, data in rsce_unpack(second):
            open(os.path.join(a.dir, name), "wb").write(data)
            print(f"  {name}: {len(data)} bytes")
        print(f"kernel {len(kernel)}, ramdisk {len(ramdisk)}, cmdline {cmdline!r}")


if __name__ == "__main__":
    main()
