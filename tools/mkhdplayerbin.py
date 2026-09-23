#!/usr/bin/env python3
"""Build or inspect a Huidu BoxPlayer firmware package (.bin) that HDPlayer can send to a controller.

Format (verified: `rebuild` reproduces BoxPlayer_7_4_0_0.bin byte-identically):
  0x00  "HDPLAYER"
  0x08  MD5 of every byte from 0x18 to EOF (integrity only; no signature)
  0x18  u32 LE  length of the XML
  0x1C  XML <FirmwareInfo> (Version, Decompress, Script, Type*, DeviceType, Info)
  ....  tar.gz payload; the unit runs <Decompress> ("tar zxvf %s -C %s") then <Script> as root.

  mkhdplayerbin.py info    PKG.bin
  mkhdplayerbin.py rebuild PKG.bin -o OUT.bin            # re-emit from parsed parts (self-test)
  mkhdplayerbin.py build   DIR -o OUT.bin --version 7.99.0.1 [--devices C15,C35,C36] [--type BoxPlayer]
      DIR must contain the script (default upgrade.sh); it is packed with every file under DIR.
"""

import argparse
import hashlib
import io
import os
import struct
import sys
import tarfile
from xml.sax.saxutils import escape

MAGIC = b"HDPLAYER"
DECOMPRESS = "killall -1 BoxDaemon; tar zxvf %s -C %s "


def parse(data):
    if data[:8] != MAGIC:
        sys.exit("not an HDPLAYER package")
    md5, xml_len = data[8:24], struct.unpack_from("<I", data, 24)[0]
    if hashlib.md5(data[24:]).digest() != md5:
        sys.exit("MD5 mismatch")
    return data[28:28 + xml_len], data[28 + xml_len:]


def assemble(xml, payload):
    body = struct.pack("<I", len(xml)) + xml + payload
    return MAGIC + hashlib.md5(body).digest() + body


def firmware_xml(version, script, types, devices):
    parts = [f"<Version>{escape(version)}</Version>", f"<Decompress>{escape(DECOMPRESS)}</Decompress>",
             f"<Script>{escape(script)}</Script>"]
    parts += [f"<Type>{escape(t)}</Type>" for t in types]
    parts += [f"<DeviceType>{escape(','.join(devices))}</DeviceType>", "<Info></Info>"]
    return ('<?xml version="1.0" encoding="UTF-8"?><FirmwareInfo>' + "".join(parts) + "</FirmwareInfo>").encode()


def tar_dir(src):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.GNU_FORMAT) as tar:
        for root, dirs, files in os.walk(src):
            dirs.sort()
            for name in sorted(files):
                path = os.path.join(root, name)
                info = tar.gettarinfo(path, os.path.relpath(path, src).replace(os.sep, "/"))
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                info.mode = 0o755 if name.endswith(".sh") or os.access(path, os.X_OK) else 0o644
                with open(path, "rb") as f:
                    data = f.read()
                if name.endswith(".sh"):
                    data = data.replace(b"\r\n", b"\n")  # the unit runs these with /bin/sh
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("info").add_argument("pkg")
    r = sub.add_parser("rebuild")
    r.add_argument("pkg")
    r.add_argument("-o", "--output", required=True)
    b = sub.add_parser("build")
    b.add_argument("dir")
    b.add_argument("-o", "--output", required=True)
    b.add_argument("--version", required=True)
    b.add_argument("--script", default="upgrade.sh")
    b.add_argument("--type", action="append", dest="types")
    b.add_argument("--devices", default="C15,C35,C36")
    a = ap.parse_args()

    if a.cmd in ("info", "rebuild"):
        xml, payload = parse(open(a.pkg, "rb").read())
        if a.cmd == "info":
            print(xml.decode(errors="replace"))
            print(f"payload: {len(payload)} bytes, gzip={payload[:2] == b'\x1f\x8b'}")
            with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as t:
                for m in t.getmembers()[:40]:
                    print(f"  {m.mode:o} {m.size:>10} {m.name}")
        else:
            open(a.output, "wb").write(assemble(xml, payload))
        return

    if not os.path.isfile(os.path.join(a.dir, a.script)):
        sys.exit(f"{a.dir} has no {a.script}")
    xml = firmware_xml(a.version, a.script, a.types or ["BoxPlayer"], a.devices.split(","))
    out = assemble(xml, tar_dir(a.dir))
    open(a.output, "wb").write(out)
    print(f"{a.output}: {len(out)} bytes, version {a.version}, devices {a.devices}")


if __name__ == "__main__":
    main()
