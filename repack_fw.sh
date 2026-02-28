#!/bin/bash
# Repack firmware with patched upgrade.sh that handles unknown CPU types
set -e

cd "$(dirname "$0")"

ZBIN="BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin"
BIN_NAME="BoxPlayer_7_11_18_0.bin"
BIN_PAYLOAD_OFFSET=678
WORKDIR="fw_repack_work"
NEWZBIN="BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0_patched.zbin"

echo "=== Firmware repack: patch upgrade.sh for unknown CPU fallback ==="

# Step 1: Extract the .bin from the .zbin
echo "[1] Extracting $BIN_NAME from $ZBIN..."
rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"
unzip -p "$ZBIN" "$BIN_NAME" > "$WORKDIR/$BIN_NAME"
echo "    .bin size: $(wc -c < "$WORKDIR/$BIN_NAME") bytes"

# Step 2: Extract the tar.gz payload (skip 678-byte HDPLAYER header)
echo "[2] Extracting gzip payload (skip ${BIN_PAYLOAD_OFFSET} bytes)..."
tail -c +$((BIN_PAYLOAD_OFFSET + 1)) "$WORKDIR/$BIN_NAME" > "$WORKDIR/payload.tar.gz"
echo "    payload size: $(wc -c < "$WORKDIR/payload.tar.gz") bytes"
# Verify gzip magic
MAGIC=$(xxd -l 2 "$WORKDIR/payload.tar.gz" | awk '{print $2}')
echo "    gzip magic: $MAGIC (should be 1f8b)"

# Step 3: Extract all files from the payload tar.gz
echo "[3] Extracting payload tar.gz to $WORKDIR/extracted/..."
mkdir -p "$WORKDIR/extracted"
tar xzf "$WORKDIR/payload.tar.gz" -C "$WORKDIR/extracted/"
echo "    Files:"
ls -la "$WORKDIR/extracted/"

# Step 4: Write the patched upgrade.sh
echo "[4] Writing patched upgrade.sh..."
cat > "$WORKDIR/extracted/upgrade.sh" << 'UPGRADE_EOF'
set -x
rm -rf /root/Box/fpga*.img
cd "$(dirname "$0")"
rm /root/Box/project/log/wirelog.*
cp log.config /root/Box/project/log/
rm upgrade.sh
dev=$(cat /proc/cpuinfo | grep Hardware | awk -F":" '{ print $2}')
echo "$dev"
if [ "$dev" = " ZTE ZX296702" ]; then
        echo "is ZTE CPU"
        tar xf ZX296702_BoxPlayerC10,C30,D10,D20,D30.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " Freescale i.MX 6DualLite HD Board" ]; then
        echo "is Freescale iMax6 CPU"
        tar xf iMax6_BoxPlayerA30,A30+,A601,A602,A603.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " RK30board" ]; then
        echo "is  RK30board RK3188 CPU"
        tar xf Android_rk3188_BoxPlayer.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " Rockchip RK3288 (Flattened Device Tree)" ]; then
        echo "is RK3288 CPU"
        tar xf Android_rk3288_BoxPlayer.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " Rockchip RK3288 (Android 9.0)" ]; then
        echo "is RK3288 CPU"
        tar xf Android_rk3288_9_BoxPlayer.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " PX30-EVB" ] || echo "$dev" | grep -qi "px30" || echo "$dev" | grep -qi "rk3326"; then
        echo "is PX30/RK3326 CPU (dev=$dev)"
        devType=""
        if [ -f /root/Box/data/id ]
        then
            devType=`awk -F "-" '{print $1}' /root/Box/data/id`
            echo "devType from id file: $devType"
        fi

        if [ "$devType" = "D15" ] || [ "$devType" = "D35" ]; then
            echo "D15/D35 device — using PX30_BoxPlayerD15"
            tar xf PX30_BoxPlayerD15.tar.gz
        elif [ "$devType" = "D18" ]; then
            echo "D18 device — using PX30_BoxPlayerD18"
            tar xf PX30_BoxPlayerD18.tar.gz
        else
            echo "C-series or other PX30 device (devType=$devType) — using PX30_BoxPlayerD15_RC"
            tar xf PX30_BoxPlayerD15_RC.tar.gz
        fi

        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
else
        echo "Unknown CPU (dev=$dev) — devType-based fallback"
        devType=""
        if [ -f /root/Box/data/id ]
        then
            devType=`awk -F "-" '{print $1}' /root/Box/data/id`
            echo "devType from id file: $devType"
        fi

        if [ "$devType" = "D15" ] || [ "$devType" = "D35" ]; then
            tar xf PX30_BoxPlayerD15.tar.gz
        elif [ "$devType" = "D18" ]; then
            tar xf PX30_BoxPlayerD18.tar.gz
        else
            echo "Using PX30_BoxPlayerD15_RC (has fpga_Cx6.img for C-series)"
            tar xf PX30_BoxPlayerD15_RC.tar.gz
        fi

        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
fi
rm /root/Box/*BoxPlayer*.tar.gz
reboot
UPGRADE_EOF
chmod +x "$WORKDIR/extracted/upgrade.sh"
echo "    upgrade.sh written ($(wc -c < "$WORKDIR/extracted/upgrade.sh") bytes)"

# Step 5: Repack the tar.gz
echo "[5] Repacking tar.gz..."
tar czf "$WORKDIR/payload_new.tar.gz" -C "$WORKDIR/extracted" .
echo "    new payload size: $(wc -c < "$WORKDIR/payload_new.tar.gz") bytes"
# Verify gzip magic of new payload
MAGIC=$(xxd -l 2 "$WORKDIR/payload_new.tar.gz" | awk '{print $2}')
echo "    new payload gzip magic: $MAGIC"

# Step 6: Build new .bin = original 678-byte header + new payload
echo "[6] Building new .bin file..."
head -c $BIN_PAYLOAD_OFFSET "$WORKDIR/$BIN_NAME" > "$WORKDIR/${BIN_NAME}_new"
cat "$WORKDIR/payload_new.tar.gz" >> "$WORKDIR/${BIN_NAME}_new"
echo "    new .bin size: $(wc -c < "$WORKDIR/${BIN_NAME}_new") bytes"
# Verify gzip magic at offset 678
MAGIC=$(xxd -s $BIN_PAYLOAD_OFFSET -l 2 "$WORKDIR/${BIN_NAME}_new" | awk '{print $2}')
echo "    gzip magic at offset $BIN_PAYLOAD_OFFSET: $MAGIC (should be 1f8b)"

# Step 7: Create new .zbin
echo "[7] Creating new .zbin: $NEWZBIN..."
rm -f "$NEWZBIN"
(cd "$WORKDIR" && zip "../$NEWZBIN" "${BIN_NAME}_new")
echo "    $NEWZBIN size: $(wc -c < "$NEWZBIN") bytes"

echo ""
echo "=== Done! New firmware: $NEWZBIN ==="
echo "    Upload with: ./target/release/hdplayer -H 192.168.1.90 upgrade-native $NEWZBIN"
