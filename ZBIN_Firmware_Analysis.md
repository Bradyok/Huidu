# Huidu ZBIN Firmware Analysis

## File Analyzed
`BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin` (346,792,414 bytes / ~347 MB)

---

## ZBIN Format Structure

A `.zbin` file is a **standard ZIP archive** (PK magic header `50 4B 03 04`) containing firmware `.bin` files and an XML manifest.

### Top-Level Contents

| File | Size | Description |
|------|------|-------------|
| `BoxPlayer_7_11_18_0.bin` | 330,110,795 bytes | BoxPlayer firmware (Linux-based controllers) |
| `MagicPlayer_V2.12.8.0.bin` | 16,605,669 bytes | MagicPlayer firmware (Android-based controllers) |
| `fileInfo.xml` | 163 bytes | Firmware manifest |

### fileInfo.xml
```xml
<firmwareInfo>
    <file name="BoxPlayer_7_11_18_0.bin" size="330110795"></file>
    <file name="MagicPlayer_V2.12.8.0.bin" size="16605669"></file>
</firmwareInfo>
```

---

## BIN File Format

Each `.bin` file has a custom Huidu header followed by an embedded archive payload.

### Header Structure

```
[MAGIC_STRING]     - ASCII identifier (e.g. "HDPLAYER" or "MAGICPLAYER")
[8-16 bytes]       - Binary metadata (checksum/flags)
[XML metadata]     - Firmware info in XML format
[Archive payload]  - The actual firmware archive (tar.gz or zip)
```

### BoxPlayer_7_11_18_0.bin

- **Magic:** `HDPLAYER` (8 bytes at offset 0x00)
- **XML metadata starts at:** offset 0x12
- **Archive payload starts at:** offset 0x2A6 (678 decimal)
- **Payload format:** tar.gz
- **Version:** 7.11.18.0
- **Firmware types:** FPGA, BoxPlayer
- **Target devices:** A3, C15, C35, A4, A5, A6, D15, D35, B6, C16, C36, D16, D36, C16L, C08L

**XML Header (decoded):**
```xml
<?xml version="1.0" encoding="UTF-8"?>
<FirmwareInfo>
  <Version>7.11.18.0</Version>
  <Decompress>killall -1 BoxDaemon; tar zxvf %s -C %s</Decompress>
  <Script>upgrade.sh</Script>
  <Type>FPGA</Type>
  <Type>BoxPlayer</Type>
  <DeviceType>A3,C15,C35,A4,A5,A6,D15,D35,B6,C16,C36,D16,D36,C16L,C08L</DeviceType>
  <Info>...</Info>
</FirmwareInfo>
```

### MagicPlayer_V2.12.8.0.bin

- **Magic:** `MAGICPLAYER` (11 bytes at offset 0x00)
- **XML metadata starts at:** offset 0x1C
- **Archive payload starts at:** offset 0x142 (322 decimal)
- **Payload format:** ZIP (PK header at offset 0x142)
- **Version:** 2.12.8.0
- **Decompress method:** `unzip`
- **Target devices:** A7, A8, A3L, A4L, A5L, A6L, B6L, H4K, H6, H8, B8L, C16H

**XML Header (decoded):**
```xml
<?xml version="1.0" encoding="utf-8"?>
<FirmwareInfo>
  <Type>MagicPlayer</Type>
  <Script>upgrade.sh</Script>
  <Decompress>unzip</Decompress>
  <DeviceType>A7,A8,A3L,A4L,A5L,A6L,B6L,H4K,H6,H8,B8L,C16H</DeviceType>
  <Version>2.12.8.0</Version>
  <Description></Description>
</FirmwareInfo>
```

---

## BoxPlayer Payload (tar.gz) - Linux-Based Controllers

The BoxPlayer tar.gz payload contains **nested per-platform archives** and a master upgrade script.

### Platform Archives

| Archive | Size | Target Hardware |
|---------|------|-----------------|
| `Android_rk3288_BoxPlayer.tar.gz` | 138 MB | Rockchip RK3288 (Android 5.x) |
| `Android_rk3188_BoxPlayer.tar.gz` | 54 MB | Rockchip RK3188 |
| `Android_rk3288_9_BoxPlayer.tar.gz` | 24 MB | Rockchip RK3288 (Android 9.0) |
| `PX30_BoxPlayerD15.tar.gz` | 24 MB | Rockchip PX30 (D15 series) |
| `PX30_BoxPlayerD15_RC.tar.gz` | 87 MB | Rockchip PX30 (D15 w/ receiving card: C16/C36/D16/D36/C16L/C08L) |
| `PX30_BoxPlayerD18.tar.gz` | 5 MB | Rockchip PX30 (D18 series) |

### CPU Auto-Detection (upgrade.sh)

The master `upgrade.sh` reads `/proc/cpuinfo` to detect the SoC and selects the correct archive:

| CPU Hardware String | Platform Selected |
|---------------------|-------------------|
| `ZTE ZX296702` | ZX296702_BoxPlayerC10,C30,D10,D20,D30 |
| `Freescale i.MX 6DualLite HD Board` | iMax6_BoxPlayerA30,A30+,A601,A602,A603 |
| `RK30board` | Android_rk3188_BoxPlayer |
| `Rockchip RK3288 (Flattened Device Tree)` | Android_rk3288_BoxPlayer |
| `Rockchip RK3288 (Android 9.0)` | Android_rk3288_9_BoxPlayer |
| `PX30-EVB` | PX30 variant (further selects by device type ID) |

### BoxPlayer RK3288 Archive Contents

The richest archive (RK3288) contains:

#### APKs
| APK | Description |
|-----|-------------|
| `cn.huidu.BoxPlayerLoader.apk` (7.6 MB) | Main BoxPlayer loader app - Qt-based Android wrapper |
| `cn.huidu.BoxSDKLoader.apk` (7.6 MB) | BoxSDK loader - companion SDK app |
| `com.google.android.webview.apk` (88 MB) | Bundled Chromium WebView |
| `framework-res.apk` (26 MB) | Custom Android framework resources |

#### Native Libraries (BoxPlayer/ directory)
Core shared libraries that do the actual LED rendering:
- `libBoxSDK.so` - Main BoxPlayer SDK
- `libCore.so` - Core rendering engine
- `libMainWindow.so` / `libMainWindowRender.so` - Display rendering
- `libBoxIOServices.so` - Hardware I/O services
- `libSDKServices.so` - SDK service layer
- `libFPGADriver.so` - FPGA hardware driver
- `libHDAndroid.so` - Android platform layer
- `libXmlSDK.so` - XML configuration parser
- `libBasicCore.so` - Basic core libraries
- `libhcommon.so` - Common utilities

#### Plugins (BoxPlayer/plugins/)
Content rendering plugins (.so files):
- `libvideo_plugin.so` - Video playback
- `libphoto_plugin.so` - Image display
- `libtext_plugin.so` / `libsinglelinetext_plugin.so` / `libanimationText_plugin.so` - Text rendering
- `libclock_plugin.so` / `libCalendar_plugin.so` / `libtime_plugin.so` - Time/date display
- `libweather_plugin.so` / `libtemperatures_plugin.so` / `libhumidity_plugin.so` - Environment data
- `libweb_plugin.so` - Web content
- `libhdmiin_plugin.so` - HDMI input capture
- `libneon_plugin.so` - Neon/border effects
- `libtable_plugin.so` - Table rendering
- `libframe_plugin.so` - Frame rendering
- `libscreen_plugin.so` - Screen management
- `libDynamicData_plugin.so` / `libNetworkData_plugin.so` - Dynamic/network data sources
- `libmodbus_plugin.so` - Modbus protocol support
- `libewatch_plugin.so` - E-watch plugin
- `libwps_plugin.so` / `libdocument_plugin.so` - Document rendering
- `librdm_plugin.so` - RDM protocol
- `libsensor_plugin.so` - Sensor data
- `libtest3d_plugin.so` - 3D test
- `libordinary_scene_plugin.so` - Scene management

#### System Components
- `System/libBoxDaemon.so` - Background daemon
- `System/libBoxUpgrade.so` - OTA upgrade handler
- `System/libBootLogo.so` - Boot logo display
- `System/ProgramLoader` - Program loader binary
- `System/Init.sh` - System initialization
- `System/Process.json` - Process configuration
- FPGA images: `fpga.img`, `fpgaB6.img`, `fpgaA4.img`
- Kernel images: `kernel.img`, `kernelB6.img`, `kernelA4.img`
- SSH server and ngrok tunneling support

---

## MagicPlayer Payload (ZIP) - Android-Based Controllers

### Contents

| File | Size | Description |
|------|------|-------------|
| `MagicPlayer.apk` | 14.5 MB | Main MagicPlayer Android app |
| `cn.huidu.device.api` | 8 MB | Device API daemon (Go binary, ARM 32-bit, statically linked) |
| `cn.huidu.device.api.sh` | 368 bytes | Shell script to launch device API in background |
| `HSDKProxys` | 1.2 MB | HSDK proxy binary (ARM 32-bit ELF, dynamically linked) |
| `upgrade.sh` | 1,496 bytes | Installation/upgrade script |

### MagicPlayer.apk Details

- **Package:** `cn.huidu.lcd.player`
- **Version:** 2.12.8.0
- **Min SDK:** 19 (Android 4.4)
- **Target SDK:** 30 (Android 11)
- **Shared User ID:** `android.uid.system` (runs as system app)
- **Architecture:** ARM (armeabi-v7a + arm64-v8a)
- **Target Devices:** A8, A7, A6L, A5L, A4L, A3L, B6L, H4K, H8, H6, B8L, C16H

#### Key Activities
- `cn.huidu.lcd.player.BootActivity` - Boot/startup activity
- `cn.huidu.lcd.player.PlayerActivity` - Main content player
- `cn.huidu.lcd.player.RgbDataActivity` - RGB data display
- `cn.huidu.lcd.player.ScreenTestActivity` - Screen testing
- `cn.huidu.lcd.player.ScreencastGuideActivity` - Screencast guide
- `cn.huidu.lcd.player.ShutdownActivity` - Shutdown handler
- `cn.huidu.lcd.player.NotSupportActivity` - Unsupported device handler
- `cn.huidu.lcd.player.SignalSourceSettingActivity` - Signal source settings

#### Key Services
- `cn.huidu.lcd.player.PlayerService` - Background player service
- `cn.huidu.lcd.player.SettingService` - Settings/configuration service

#### Key Receivers
- `cn.huidu.lcd.player.receiver.BootReceiver` - Handles BOOT_COMPLETED
- `cn.huidu.lcd.player.receiver.UsbReceiver` - USB device attach/detach
- `cn.huidu.lcd.player.receiver.SdcardReceiver` - SD card mount/unmount
- `cn.huidu.lcd.player.receiver.DateTimeReceiver` - Time/timezone changes

#### Native Libraries
- `libhuidu_compat.so` - Huidu compatibility layer
- `libfpga_mac_jni.so` - FPGA MAC address JNI bridge
- `libmmkv.so` - Tencent MMKV key-value storage
- `libpl_droidsonroids_gif.so` - GIF rendering (android-gif-drawable)

#### Permissions (Notable)
- System-level: `INSTALL_PACKAGES`, `DELETE_PACKAGES`, `REBOOT`, `RECOVERY`, `SET_TIME`, `DEVICE_POWER`
- Hardware: `BLUETOOTH`, `RECORD_AUDIO`, `WAKE_LOCK`
- Storage: `MANAGE_EXTERNAL_STORAGE`, `READ/WRITE_EXTERNAL_STORAGE`
- Network: `INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_STATE`

### Upgrade Process (MagicPlayer)

The `upgrade.sh` script:
1. Copies FPGA image if present
2. Installs binary daemons (`HSDKProxys`, `cn.huidu.device.api`) to `/data/app-bin/`
3. Installs shell scripts to `/data/app-bin/`
4. Installs all `.apk` files using `pm install -r -d`

---

## How to Extract

### Step 1: Unzip the ZBIN
```bash
unzip BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin -d output/
```

### Step 2: Extract MagicPlayer payload (ZIP inside BIN)
```python
# Python - skip the custom header (322 bytes)
with open('MagicPlayer_V2.12.8.0.bin', 'rb') as f:
    f.seek(322)  # Skip MAGICPLAYER header + XML
    data = f.read()
with open('MagicPlayer_payload.zip', 'wb') as f:
    f.write(data)
```
```bash
unzip MagicPlayer_payload.zip -d MagicPlayer/
```

### Step 3: Extract BoxPlayer payload (tar.gz inside BIN)
```python
# Python - skip the custom header (678 bytes)
with open('BoxPlayer_7_11_18_0.bin', 'rb') as f:
    f.seek(678)  # Skip HDPLAYER header + XML
    data = f.read()
with open('BoxPlayer_payload.tar.gz', 'wb') as f:
    f.write(data)
```
```bash
tar xzf BoxPlayer_payload.tar.gz -C BoxPlayer/
# Then extract platform-specific archives:
tar xzf BoxPlayer/Android_rk3288_BoxPlayer.tar.gz -C BoxPlayer_rk3288/
```

---

## Extracted APKs

Saved to `extracted_APKs/` directory:

| APK File | Package Name | Version | Type |
|----------|-------------|---------|------|
| `MagicPlayer.apk` | `cn.huidu.lcd.player` | 2.12.8.0 | Native Android player (Java/Kotlin) |
| `cn.huidu.BoxPlayerLoader.apk` | `cn.huidu.BoxPlayerLoader` | 1.0 | Qt5-based Android wrapper for native BoxPlayer |
| `cn.huidu.BoxSDKLoader.apk` | `cn.huidu.BoxSDKLoader` | 1.0 | Qt5-based Android wrapper for BoxSDK |

### Architecture Differences

**BoxPlayer** (older controllers: A3-A6, C/D series):
- Native C++ rendering engine using Qt5
- APK is just a thin Android loader that bootstraps native `.so` libraries
- Real logic lives in `BoxPlayer/*.so` native libraries
- Supports RK3188, RK3288, PX30, iMX6, ZTE ZX296702 SoCs

**MagicPlayer** (newer controllers: A7/A8, L-series, H-series):
- Full Android Java/Kotlin application
- Uses `cn.huidu.device.api` (Go daemon) for hardware communication
- Uses `HSDKProxys` for SDK proxy services
- Runs as system app with `android.uid.system`
- Supports arm64-v8a (64-bit) + armeabi-v7a (32-bit)
