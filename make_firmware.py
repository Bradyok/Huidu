#!/usr/bin/env python
"""Create a custom firmware package for diagnosing upgrade failures.

v3 approach: DIAGNOSTIC — flat archive with a minimal upgrade.sh that only
writes a detectable version marker ("7.99.0.1") to /root/Box/version/version
and reboots.

Purpose: determine whether upgrade.sh is ever executed by the device.
- If the device reports version "7.99.0.1" after reboot → script ran, the
  problem is in the upgrade.sh content (device_locker, cp paths, etc.).
- If the device stays at "7.2.32.0" → script never ran, the problem is in the
  Rust client's protocol (upgrade.rs Phase 7 UpgradeExec timing/connection).

Once the execution path is confirmed, restore the full upgrade.sh logic.
"""
import zipfile, io, gzip, tarfile

ZBIN = "C:/Users/Owner/Documents/GitHub/Huidu/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin"
OUT  = "C:/Users/Owner/Documents/GitHub/Huidu/custom_upgrade.tar.gz"
BIN_OFFSET = 678

print("Reading firmware archive...")
with zipfile.ZipFile(ZBIN) as z:
    with z.open("BoxPlayer_7_11_18_0.bin") as f:
        f.read(BIN_OFFSET)
        payload = f.read()
print("  Outer payload: %d bytes" % len(payload))

print("Extracting PX30_BoxPlayerD15.tar.gz from outer archive...")
gz = gzip.GzipFile(fileobj=io.BytesIO(payload))
px30_data = None
with tarfile.open(fileobj=gz, mode="r|") as tar:
    for m in tar:
        if m.name == "PX30_BoxPlayerD15.tar.gz":
            px30_data = tar.extractfile(m).read()
            print("  PX30_BoxPlayerD15.tar.gz: %d bytes" % len(px30_data))
            break
assert px30_data, "PX30_BoxPlayerD15.tar.gz not found in outer archive"

print("Extracting files from PX30_BoxPlayerD15.tar.gz...")
gz2 = gzip.GzipFile(fileobj=io.BytesIO(px30_data))
files = {}   # name -> (data, mode)
with tarfile.open(fileobj=gz2, mode="r|") as tar2:
    for m in tar2:
        if not m.isfile():
            continue
        f2 = tar2.extractfile(m)
        if not f2:
            continue
        files[m.name] = (f2.read(), m.mode)
        print("  %s: %d bytes" % (m.name, len(files[m.name][0])))

# DIAGNOSTIC: Replace upgrade.sh with a minimal script to confirm it executes.
# If the device reports version "7.99.0.1" after reboot, upgrade.sh ran and
# can write to /root/Box/.  Once confirmed, restore the full upgrade logic.
# If the device stays at 7.2.32.0, upgrade.sh is never triggered — fix the
# protocol (upgrade.rs Phase 7 UpgradeExec timing) first.
assert "upgrade.sh" in files, "upgrade.sh not found in PX30_BoxPlayerD15.tar.gz"
original_sh, sh_mode = files["upgrade.sh"]
print("\nReplacing upgrade.sh with diagnostic script (writes 7.99.0.1 + reboots)...")
print("  Original upgrade.sh: %d bytes" % len(original_sh))
diagnostic_sh = (
    "#!/bin/sh\n"
    "echo '7.99.0.1' > /root/Box/version/version\n"
    "sync\n"
    "reboot\n"
).encode("utf-8")
files["upgrade.sh"] = (diagnostic_sh, sh_mode)

print("\nBuilding flat custom_upgrade.tar.gz...")
out_buf = io.BytesIO()
with gzip.GzipFile(fileobj=out_buf, mode="wb", mtime=0) as gz_out:
    with tarfile.open(fileobj=gz_out, mode="w|") as tar_out:
        for name, (data, mode) in files.items():
            ti = tarfile.TarInfo(name=name)
            ti.size = len(data)
            ti.mode = mode
            ti.mtime = 0
            tar_out.addfile(ti, io.BytesIO(data))

result = out_buf.getvalue()
print("Output: %d bytes (%.1f MB)" % (len(result), len(result)/1e6))
with open(OUT, "wb") as f:
    f.write(result)
print("Written to: %s" % OUT)
