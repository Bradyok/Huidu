#!/usr/bin/env python3
"""Build the access + survey package: fills in the root password and wraps it as an HDPLAYER .bin.

  python packages/access-survey/build.py [--password PW] [-o out.bin]

The password is printed once and written next to the output (<out>.password.txt); keep both local.
"""
import argparse
import os
import secrets
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "..", "..", "tools", "mkhdplayerbin.py")

ap = argparse.ArgumentParser()
ap.add_argument("--password", help="root password to set (default: random)")
ap.add_argument("--version", default="7.11.18.99")
ap.add_argument("-o", "--output", default="access-survey_7.11.18.99.bin")
a = ap.parse_args()

pw = a.password or secrets.token_urlsafe(9)
if any(c in pw for c in "'\\\n:"):
    sys.exit("password must not contain ' \\ : or newline")

with tempfile.TemporaryDirectory() as tmp:
    script = open(os.path.join(HERE, "upgrade.sh"), encoding="utf-8").read().replace("@ROOTPW@", pw)
    with open(os.path.join(tmp, "upgrade.sh"), "w", newline="\n") as f:
        f.write(script)
    subprocess.run([sys.executable, TOOL, "build", tmp, "-o", a.output, "--version", a.version,
                    "--devices", "C15,C35,C36"], check=True)

with open(a.output + ".password.txt", "w") as f:
    f.write(pw + "\n")
print(f"root password: {pw}   (saved to {a.output}.password.txt)")
