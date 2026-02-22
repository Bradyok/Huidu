# HDSet V4.0.16.0 — Reverse Engineering Reference

## 1. Overview

| Field | Value |
|-------|-------|
| Binary | `HDSet V4.0.16.0.exe` — NSIS self-extracting installer |
| Size | 165 MB compressed, ~300 MB installed |
| Architecture | Qt5 C++ x86 GUI application |
| Purpose | LED display **hardware configuration** (distinct from HDPlayer) |
| Version | 4.0.16.0 |
| Build | MSVC, Qt 5.x |

### HDSet vs HDPlayer: Key Distinction

| Dimension | HDPlayer | HDSet |
|-----------|----------|-------|
| Purpose | Content management — upload programs, media, playlists | Hardware configuration — LED scan mode, gamma, chip params, firmware |
| Protocol | TCP SDK XML (program commands) | TCP SDK XML (FPGA commands) + serial/USB to send cards |
| Target | BoxPlayer embedded Linux | BoxPlayer OR direct-connected send cards |
| Audience | Display operators | LED system integrators / technicians |

---

## 2. Installer Structure

| Field | Value |
|-------|-------|
| Installer | NSIS v3.08 |
| UAC | requireAdministrator |
| Install path | `C:\Program Files\HDSet` |
| Languages | 20+ (AR, CN, CT, DE, EL, EN, ES, FA, FR, HU, ID, IT, JP, KR, MS, PL, PT, RU, SR, TL, TR, VI) |

### Registry keys written
```
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\HDSet
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\HDSet.exe
HKCU\SOFTWARE\HDSet
```

### Driver installation
1. CH341/CH343 USB-serial driver (`SETUP.exe /S`)
2. Silicon Labs CP210x VCP driver
3. `DRVSETUP64\DRVSETUP64.exe`
4. `PnPutil.exe /add-driver HDVP-Config.inf` (VP-series video processor)

---

## 3. Application Architecture

### Main executables
| File | Size | Purpose |
|------|------|---------|
| `HDSet.exe` | ~2 MB | Main Qt5 GUI |
| `HDSetTool.exe` | ~1 MB | Auxiliary configuration tool |
| `HDSettingToolBar.exe` | ~500 KB | Toolbar launcher |
| `HDScreenTestTool.exe` | ~400 KB | Screen test standalone tool |
| `QtWebEngineProcess.exe` | ~400 KB | Chromium renderer process |
| `php.exe` | ~7 MB | PHP local web server |

### Core DLLs
| DLL | Role |
|-----|------|
| `hcommon.dll` (2.1 MB) | Common utilities: HAppConfig, font management, MD5, timezone |
| `MainWindow.dll` (836 KB) | Main UI, hardware config panels |
| `HCatNet.dll` (735 KB) | Network and event core: IEventCore, ICatEventBase |
| `NetIOServices.dll` (1.4 MB) | Network I/O services |
| `hguilib.dll` (4.4 MB) | GUI component library |

### Chip-specific configuration DLLs
Each LED driver chip family has a dedicated plugin DLL implementing chip-specific
gamma curves, current settings, and register maps:

| DLL | Chip family |
|-----|-------------|
| `ICNPlugin.dll` | ICN2038, ICN2045, ICN2053, ICN2163, ICN2165 |
| `SMPlugin.dll` | SM16207, SM16227, SM16259, SM16289, SM16359, SM16389 |
| `MBIPlugin.dll` | MBI5041, MBI5153, MBI5252, MBI5268 |
| `DPPlugin.dll` | DP3264, DP3265, DP3269, DP5125 |
| `FMPlugin.dll` | FM6124, FM6127, FM6153, FM6363, FM6565 |
| `MYPlugin.dll` | MY9862, MY9866, MY9868 |
| `SUMPlugin.dll` | SUM2017, SUM2028, SUM2130 |
| `GammaV2_Variable_BitWidth_Exponent.dll` | Generic gamma V2 |
| `CKSCNSCamma.dll` | CKS/CNS chip gamma |
| `IcnGamma.dll` | ICN chip gamma |
| `MingWeiSM16389SF_Gamma.dll` | SM16389SF gamma |
| `AxsChipConf.dll` | AXS chip configuration |
| `ChipDisplayStyleManage.dll` | Per-chip display style management |
| `ConventionalRzChip.dll` | Conventional RZ chip support |
| `configDLL6619.dll` | Chip 6619 specific config |
| `OTHERPlugin.dll` | Miscellaneous chips |

### USB / serial drivers
| Driver DLL | Interface |
|------------|-----------|
| `CH341PT.DLL` / `CH341SER` | CH341 USB-to-serial (most send cards) |
| `CH343PT.DLL` / `CH343PTA64.DLL` | CH343 USB-to-serial (newer cards) |
| `hidapi.dll` | HID interface |
| `libusb0.dll` / `libusbK.dll` | USB bulk (H-series: H6, H8) |

---

## 4. Communication Architecture

HDSet communicates with LED hardware via two distinct paths:

```
                    ┌─────────────────────────────────────┐
                    │         HDSet.exe (Windows PC)       │
                    └───────────────┬─────────────────────┘
                                    │
               ┌────────────────────┴─────────────────────────┐
               │                                               │
               ▼  Path 1: TCP (port 10001)                    ▼  Path 2: USB/serial
    ┌──────────────────────┐                    ┌─────────────────────────────┐
    │ BoxPlayer (ARM Linux) │                    │ Send Card (VP/T/A/B/C/D/H)  │
    │  PX30 / RK3288 / etc. │                    │  via CH341/CH343/CP210x/USB │
    │                       │                    │  bulk                       │
    │  Same binary protocol │                    │  Lower-level serial FPGA    │
    │  as HDPlayer, but with│                    │  configuration protocol     │
    │  FPGA-specific XML    │                    │  (TBD from wire capture)    │
    │  commands             │                    └─────────────────────────────┘
    └──────────────────────┘
```

### Path 1: TCP to BoxPlayer

Identical binary framing to HDPlayer (same `[u16 len][u16 cmd][payload]` format).
Uses port 10001, same SDK handshake (0x2001/0x2002), same UDP discovery (port 9527).

**HDSet-specific SDK XML methods:**
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetBoxHwConfig` | GET | Read complete FPGA config XML |
| `SetBoxHwConfig` | SET | Write FPGA config (immediate apply) |
| `SaveBoxHwConfig` | SET | Persist FPGA config to flash |
| `ReplaceBoxHwConfig` | SET | Full config replacement |
| `GetSDKFPGAConfig` | GET | Alternative FPGA config read (older SDK) |
| `SetSDKFPGAConfig` | SET | Alternative FPGA config write |
| `SmartSetting` | SET | Auto-detect LED module parameters |
| `SmartDrawLine` | SET | Auto-detect panel dimensions |
| `ScreenTest` | SET | Visual hardware test pattern |
| `GetBootLogo` | GET | Read boot logo filename |
| `SetBootLogoName` | SET | Set boot logo |
| `ClearBootLogo` | SET | Remove boot logo |

**Shared with HDPlayer** (same XML commands):
`GetDeviceInfo`, `GetHardwareInfo`, `GetDeviceName`, `SetDeviceName`,
`GetLuminancePloy`, `SetLuminancePloy`, `OpenScreen`, `CloseScreen`,
`GetTimeInfo`, `SetTimeInfo`, `GetEth0Info`, `SetEth0Info`,
`GetWifiInfo`, `SetWifiInfo`, `FirmwareUpgrade`, `GetUpgradeResult`,
`Reboot`, `GetScreenshot2`

**FPGA binary session commands** (TBD — hex codes pending wire capture):
| Name | Description |
|------|-------------|
| `kFPGASettingInAsk/Answer` | Enter FPGA config session |
| `kFPGASettingOutAsk/Answer` | Exit FPGA config session |
| `kFPGAParamSetAsk/Answer` | Send raw FPGA parameter block |
| `kFPGASetCMDAsk/Answer` | Send raw FPGA command |

### Path 2: Direct serial to send cards

| Connection type | Used by |
|----------------|---------|
| `COM_115200` | VP210, VP410, T-series |
| `COM_256000` | VP820, VP820C, X-series |
| `USB_BULK_V1.0` | H6, H8 |

Serial protocol (partial — needs wire capture):
```
[0xFD]       start-of-frame
[u8 cmd]     command byte
[u16 LE len] payload length
[N bytes]    payload
[u8 chk]     XOR or sum checksum
```

---

## 5. BoxHwConfig XML Format

The central data structure exchanged between HDSet and the device.
Stored on device as `/root/Box/config/hwsetting/send_card_cfg.xml`.

### Complete tag reference

```xml
<BoxHwConfig>
  <CardInfo>
    <Card index="0">

      <!-- LED module identification -->
      <ModuleType value="1"/>           <!-- module model code -->
      <DriveChipType value="0"/>        <!-- driver IC code (see chip table below) -->

      <!-- Physical module geometry -->
      <CellWidth value="32"/>           <!-- module width in pixels -->
      <CellHight value="16"/>           <!-- module height (note: "Hight" typo in original) -->
      <CellScanRow value="4"/>          <!-- scan rows per cycle (denominator) -->
      <ScanMode value="2"/>             <!-- 0=static, 1=1:2, 2=1:4, 3=1:8, 4=1:16, 5=1:32 -->
      <MoreThan16Scan value="0"/>       <!-- flag: >16 scan rows -->

      <!-- Signal topology -->
      <ESignal value="0"/>              <!-- 4th address signal enable -->
      <Chip595 value="0"/>              <!-- 74HC595 variant -->
      <Chip5958 value="0"/>             <!-- 74HC5958 variant -->
      <DecodingMode value="0"/>         <!-- 0=138 decoder, 1=595 decoder -->
      <DataPolarity value="0"/>         <!-- 0=high active, 1=low active -->
      <OEPolarity value="0"/>           <!-- 0=low active, 1=high active -->
      <SignalColor value="0"/>          <!-- 0=RGB, 1=RBG, 2=GRB, 3=GBR, 4=BRG, 5=BGR -->
      <CellNullNum value="0"/>          <!-- null pixels per module -->

      <!-- Timing & refresh -->
      <LookUpTab value="0"/>            <!-- gamma LUT type -->
      <RefreshRate value="1200"/>       <!-- target refresh rate (Hz) -->
      <R_Acc value="0"/>                <!-- R accumulate bits (ICN2053 etc.) -->
      <GrayLevel value="256"/>          <!-- grayscale depth: 16/64/256/1024/4096 -->
      <LuminanceLevel value="64"/>      <!-- luminance levels -->
      <Frequency value="25"/>           <!-- pixel clock (MHz) -->
      <PriorityMode value="2"/>         <!-- 0=gray, 1=refresh, 2=automatic -->

      <!-- Bit depth (exactly one set to "1") -->
      <RGB20 value="0"/>
      <RGB24 value="1"/>                <!-- 8 bits per channel (most common) -->
      <RGB28 value="0"/>
      <RGB32 value="0"/>                <!-- 10-bit (requires chip support) -->

      <!-- Color calibration -->
      <GamaValue value="0"/>            <!-- gamma correction value -->
      <RedCorrection value="1000"/>     <!-- red channel ×1000 (1000=100%) -->
      <GreenCorrection value="1000"/>
      <BlueCorrection value="1000"/>

      <!-- Waveform parameters -->
      <DutyCycle value="500"/>          <!-- OE duty cycle ×1000 (500=50%) -->
      <BV value="6"/>                   <!-- blanking value -->
      <Phase value="0"/>                <!-- clock phase -->
      <Afterglow value="1"/>            <!-- afterglow reduction -->
      <OM value="0"/>                   <!-- oscillation mode -->

      <!-- PWM current (for PWM-capable chips) -->
      <PwmRedCurrent value="0"/>
      <PwmGreenCurrent value="0"/>
      <PwmBlueCurrent value="0"/>

      <!-- Advanced refresh modes -->
      <SPWMMode value="0"/>             <!-- S-PWM mode -->
      <FMPWMMultiplier value="0"/>      <!-- FM-PWM multiplier -->
      <DoubleRefreshRate value="0"/>    <!-- double refresh enable -->
      <GCLKMultiplier value="4"/>       <!-- GCLK frequency multiplier -->
      <PwmFrequency value="0"/>
      <F_Frame value="0"/>              <!-- frame frequency -->

      <!-- Brightness -->
      <Brightness value="100"/>         <!-- 0–100% -->

      <!-- Optional gamma table (hex bytes, space-separated) -->
      <!-- <GamaTab value="00 01 02 03 ... FF"/> -->

      <!-- Output definition (for chips with configurable outputs) -->
      <EnOutputDefinition value="0"/>
      <OutputDefinition value="0"/>

      <!-- PWM IC register values -->
      <PwmChipType value="0"/>
      <PWMICRedReg1 value="0"/>
      <PWMICRedReg2 value="0"/>
      <PWMICRedReg3 value="0"/>
      <PWMICGreenReg1 value="0"/>
      <PWMICGreenReg2 value="0"/>
      <PWMICGreenReg3 value="0"/>
      <PWMICBlueReg1 value="0"/>
      <PWMICBlueReg2 value="0"/>
      <PWMICBlueReg3 value="0"/>

    </Card>
    <!-- Additional <Card> elements for multi-channel configurations -->
  </CardInfo>

  <!-- Global screen geometry -->
  <NetcardCtrlRect x="0" y="0" width="128" height="64"/>
  <ModeSwitchPlan value="0"/>
  <Rotation value="0"/>                 <!-- 0=0°, 1=90°, 2=180°, 3=270° -->

  <!-- Send card modes -->
  <SendCardMode value="0"/>
  <NetCardCtrlMode value="0"/>
  <AsyncPriorityMode value="0"/>
  <EnSendcardOnly value="0"/>

  <!-- Receive card selection -->
  <RecvCardChoose value="0"/>

  <!-- Output dimensions -->
  <RgbCtrlWidth value="128"/>
  <RgbCtrlHeight value="64"/>
  <BrightnessSetMode value="0"/>        <!-- 0=software, 1=hardware PWM -->

  <!-- Optional user-defined gamma list -->
  <!-- <UDefGamaList>...</UDefGamaList> -->

</BoxHwConfig>
```

### Driver chip type codes (DriveChipType)

| Code | Family | Chips |
|------|--------|-------|
| 0 | ICN | ICN2037, ICN2038 |
| 1–5 | ICN | ICN2045, ICN2053, ICN2163, ICN2165 |
| 10–16 | SM | SM16207, SM16227, SM16259, SM16289, SM16359, SM16389, SM16395 |
| 20–23 | MBI | MBI5041, MBI5153, MBI5252, MBI5268 |
| 30–34 | FM | FM6124, FM6127, FM6153, FM6363, FM6565 |
| 40–42 | DP | DP3264, DP3265, DP5125 |
| 50–54 | MY | MY9862, MY9866, MY9868 |
| 60–62 | SUM | SUM2017, SUM2028, SUM2130 |
| 70–74 | LS | LS9919, LS9929, LS9935, LS9956 |

---

## 6. Sender Card Models

From `device.xml` bundled in the installer:

| Series | Models | Architecture | Max canvas |
|--------|--------|--------------|------------|
| VP210/410 | VP210, VP210C, VP410, VP410C | Linux (SoC) | 2048×1024 |
| VP820/1220/1620 | Multiple variants | Linux | 4096×2048 |
| VP2000+ | VP2060, VP2460, VP3060, VP4060, VP8000M | Android | 16384×4096 |
| T-series | T901, T902, T902M, T16, T08, T08F, FT08 | Linux | 2048×1024 |
| A-series | A3, A4, A5, A6, A3L–A6L | Android | 16384×4096 |
| B-series | B6, B6L, B8L | Android | 16384×4096 |
| C-series | C3i, C15, C16, C16L, C16H, C36 | mixed | 8192×2048 |
| D-series | D05, D06, D15, D16, D18, D35, D36, D68 | mixed | 8192×4096 |
| H-series | H6, H8 | Linux (USB bulk) | 4096×2048 |
| X-series | X20, X40 | — | — |

### Connection types per series
- `COM_115200` — CH341 serial at 115200 baud (VP210, VP410, T-series)
- `COM_256000` — CH341 serial at 256000 baud (VP820, X-series)
- `USB_BULK_V1.0` — libusbK/libusb USB bulk (H6, H8)

---

## 7. Receiver Card Models

From `recvcardsupportinfo.xml`:

### FPGA firmware families
| Model family | Notes |
|---|---|
| K08, K12 | Standard receivers |
| R3210 | Combined FPGA+MCU |
| R5S, R500S, R507T | R5xx family |
| R508, R512, R512S, R512T | R5xx extended |
| R516, R516T, R612 | High-resolution |
| R708, R712, R716, R732 | R7xx family (high scan) |
| RB6 | Budget series |

### Firmware naming convention
`{Model}_FPGA_V{fpga_ver}[_MCU_V{mcu_ver}].bin`

---

## 8. ARM Shared Libraries (HDSetSo)

These `.so` files are deployed to the embedded Linux device and loaded by
`HTcpHDSetSo.cpp` via dlopen. They allow HDSet-style hardware configuration
requests to be serviced from the device side without full BoxPlayer involvement.

| File | Target SoC |
|------|------------|
| `libHDSet_PX30.so` | Rockchip PX30 / RK3326 |
| `libHDSet_PX30_RC.so` | PX30 RC variant |
| `libHDSet_rk3188.so` | Rockchip RK3188 |
| `libHDSet_rk3288.so` | Rockchip RK3288 |
| `HDSetApps-rk3326.so` | RK3326 (new SDK) |
| `HDSetApps-rk356.so` | RK3566/RK3568 |
| `HDSetApps-t507.so` | Allwinner T507 |

Deploy path on device: `/root/Box/BoxPlayer/HDSetSo/`

---

## 9. Embedded PHP Web Server

HDSet bundles a local PHP web server for:
- Serving a local web UI (accessed via `Qt5WebEngineWidgets`)
- Providing REST endpoints for the ARM `.so` modules
- Image conversion (`img_conv_core.php`)

| Component | Path |
|-----------|------|
| PHP binary | `\AppData\Local\localserver\HDset\public\exDllModule\php.exe` |
| PHP runtime | `php7.dll`, `php_gd2.dll`, `php_fileinfo.dll` |
| Conversion script | `img_conv_core.php` |

---

## 10. Configuration Files

| File | Purpose |
|------|---------|
| `HDSetConfig.ini` | Main application config |
| `HwConf.ini` | Hardware configuration state |
| `SendRecvParamList.ini` | Send/receive card parameter presets |
| `TestScreenPos.ini` | Screen test position data |
| `preset.ini` | Hardware presets |
| `device.xml` | Send card model database |
| `deviceinfo.xml` | Device info templates |
| `recvcardsupportinfo.xml` | Receive card capability database |
| `recvmntrspt.xml` | Receive card monitor support |
| `Language.xml` | UI localization strings |
| `ColorStyle.xml` | UI color themes |
| `ScreenParamLocal.json` | Local screen parameter cache |
| `classifyConfigLocal.json` | Device classification data |
| `ediddmttimings.json` | EDID DMT timing tables |
| `edidvictimings.json` | EDID VIC timing tables |
| `fontManage.xml` | Font mapping (same format as HDPlayer) |
| `ConventionalRzChip_*.xml` | RZ chip configuration (CN/CT/EN locales) |

---

## 11. Visual Effects (HLSL)

`Effects/default.fx` implements the "dazzle" (炫彩) color animation effect:

```hlsl
// Uniforms
float NowTime;   // animated clock (seconds)
float Speed;     // animation speed multiplier
float Dazzle;    // effect intensity

// Gradient modes (0–8):
// 0 = horizontal gradient
// 1 = vertical gradient
// 2 = tiled pattern
// 3 = diagonal
// 4–8 = rotational/sweep variants

// Color space: HSV→RGB with hue driven by NowTime*Speed
```

---

## 12. Rust Reproduction Status

See `hdset-client/` crate.

| Feature | Status | Notes |
|---------|--------|-------|
| TCP connection to BoxPlayer | ✅ | Same protocol as HDPlayer |
| UDP device discovery | ✅ | Port 9527 |
| File transfer | ✅ | Boot logo, firmware |
| `GetBoxHwConfig` / `SetBoxHwConfig` | ✅ | Full XML parse/serialize |
| `SaveBoxHwConfig` / `ReplaceBoxHwConfig` | ✅ | |
| `SmartSetting` / `SmartDrawLine` | ✅ | XML command |
| `ScreenTest` | ✅ | Duration + solid color |
| Boot logo management | ✅ | Get/set/clear/upload |
| Brightness control | ✅ | |
| Screen on/off | ✅ | |
| Firmware upgrade | ✅ | .zbin upload |
| Network config | ✅ | Ethernet read/write |
| FPGA session binary protocol | ⚠️ | Stub — command codes TBD from wire capture |
| Direct serial to send cards | ⚠️ | Serial framing stub — needs wire capture |
| Chip-specific gamma plugins | ❌ | ICN/SM/MBI DLL logic not reproduced |
| USB bulk (H-series) | ❌ | Needs libusb integration |
| Receive card configuration | ❌ | Needs FPGA session protocol |
| FPGA firmware flashing | ❌ | Needs send card serial protocol |
| Local PHP web server | ❌ | Not needed for Rust implementation |
| GUI | ❌ | egui GUI stub (hdset-gui binary placeholder) |

### Next steps for serial protocol reverse engineering

1. Install USBPcap or Wireshark with USB capture support
2. Connect a Huidu VP210/VP410 (or T-series) send card via USB
3. Launch original HDSet.exe and perform `SmartSetting` + save config
4. Capture the COM port traffic and analyze the `[0xFD]` framing
5. Document command bytes and payload structure in `connection/serial.rs`
