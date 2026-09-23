#!/usr/bin/env python3
"""Build a 7.11.18.0 C-series upgrade package that KEEPS local access.

Stock 7.11.18.0 `System/BoxPlayerInit.sh`, at every boot, runs `/etc/init.d/S50telnet stop`
and `start-ssh` (SSH with a root password we don't have + Huidu's own authorized_keys). So a
stock upgrade from 7.4.x loses the telnet foothold we currently have. This repackages the real
7.11.18.0 C-series payload with two minimal, reversible edits to BoxPlayerInit.sh:

  1. keep telnet up   (the `S50telnet stop` line is neutralised and telnet is (re)started)
  2. set a known root password  (so SSH is usable by us too)

Nothing else in the 7.11 app is changed. The C-series never flashes a kernel or FPGA in the
stock upgrade.sh (guarded by devType==D15/D35), and the D15 kernel image is excluded here anyway.

  python packages/keepaccess-7.11/build.py [--password PW] [--payload DIR] [-o out.bin]

Source payload defaults to the extracted 7.11.18.0 D15/C-series staging tree
(firmware_extract/PX30_D15). The password is saved next to the output; keep it local.
"""
import argparse
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
TOOL = os.path.join(REPO, "tools", "mkhdplayerbin.py")
DEFAULT_PAYLOAD = os.path.join(REPO, "firmware_extract", "PX30_D15")
# D15-only artifacts: never wanted in a C-series package.
EXCLUDE = {"kernel_d15_ec200T.img", "fpga_D15.img", "wifi_D15.sh"}

ap = argparse.ArgumentParser()
ap.add_argument("--password", help="root password to set (default: random)")
ap.add_argument("--payload", default=DEFAULT_PAYLOAD)
ap.add_argument("--devices", default="C15,C35,C36")
ap.add_argument("--version", default="7.11.18.0")
ap.add_argument("--keep-ngrok", action="store_true",
                help="leave Huidu's outbound ngrok tunnel enabled (default: disable the phone-home)")
ap.add_argument("-o", "--output", default=os.path.join(HERE, "keepaccess-7.11.18.0_C15.bin"))
a = ap.parse_args()

pw = a.password or secrets.token_urlsafe(9)
if any(c in pw for c in "'\\\n:"):
    sys.exit("password must not contain ' \\ : or newline")

init_rel = os.path.join("System", "BoxPlayerInit.sh")
if not os.path.isfile(os.path.join(a.payload, init_rel)):
    sys.exit(f"{a.payload} is not a 7.11 payload tree (no {init_rel})")

with tempfile.TemporaryDirectory() as tmp:
    stage = os.path.join(tmp, "stage")
    shutil.copytree(a.payload, stage, ignore=shutil.ignore_patterns(*EXCLUDE))

    # Patch BoxPlayerInit.sh
    p = os.path.join(stage, init_rel)
    lines = open(p, encoding="utf-8", errors="surrogateescape").read().replace("\r\n", "\n").split("\n")
    out, patched, ngrok_off = [], False, False
    block = [
        "# --- access-patch: keep local access across the upgrade (was: S50telnet stop) ---",
        "/etc/init.d/S50telnet start 2>/dev/null || telnetd -l /bin/sh 2>/dev/null &",
        "echo 'root:@ROOTPW@' | chpasswd 2>/dev/null || "
        "(printf '%s\\n%s\\n' '@ROOTPW@' '@ROOTPW@' | passwd root)",
        "# --- end access-patch ---",
    ]
    for ln in lines:
        s = ln.strip()
        if s == "/etc/init.d/S50telnet stop":
            out += block
            patched = True
        elif not a.keep_ngrok and "ngrok" in s and not s.startswith("#"):
            out.append("# access-patch disabled phone-home: " + ln)
            ngrok_off = True
        else:
            out.append(ln)
    if not patched:
        sys.exit("did not find the 'S50telnet stop' line to patch — payload changed, aborting")
    open(p, "w", encoding="utf-8", errors="surrogateescape", newline="\n").write(
        "\n".join(out).replace("@ROOTPW@", pw))

    subprocess.run([sys.executable, TOOL, "build", stage, "-o", a.output,
                    "--version", a.version, "--type", "BoxPlayer", "--devices", a.devices],
                   check=True)

open(a.output + ".password.txt", "w").write(pw + "\n")
print(f"root password: {pw}   (saved to {os.path.basename(a.output)}.password.txt)")
print("access after upgrade: telnet 23 stays up; SSH root login with the password above")
if not a.keep_ngrok:
    print("phone-home: ngrok line in BoxPlayerInit.sh disabled" if ngrok_off
          else "note: no ngrok line in BoxPlayerInit.sh (it starts elsewhere; not touched)")
