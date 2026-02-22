# HDPlayer 7.11.8.0 — Complete Reverse Engineering Reference

## 1. Overview

| Field | Value |
|-------|-------|
| Installer | HDPlayer.7.11.8.0.exe — NSIS installer, LZMA compressed, 358 MB compressed, ~1.08 GB installed, 3704 files |
| Launcher | HDPlayer.exe — 140,800-byte x86 PE32 GUI thin launcher |
| Build path | E:\hdplayer\Branches\PX30_A8 |
| Build date | November 4, 2022 |
| Version | 7.11.8.0 |
| Copyright | 2009–2025 Huidu |
| Architecture | Qt5 C++ native Windows application |

HDPlayer.exe is a minimal thin launcher that dynamically loads all application logic from DLLs at runtime. The actual application weight is distributed across a set of specialized DLLs described in Section 3.

---

## 2. Installer Structure

### 2.1 NSIS Installer Sections

| Section | Virtual Size | Notes |
|---------|-------------|-------|
| .text | 0x6DAE | Executable code |
| .rdata | 0x2A62 | Read-only data, string tables |
| .data | 0x67EBC | Initialized data |
| .ndata | (virtual) | NSIS decompression buffer |
| .rsrc | 0x15418 | Resources (icons, version info) |
| .reloc | 0x0F32 | Base relocation table |

The payload is LZMA-compressed and extracted at install time.

### 2.2 Install Targets

Primary: `C:\Program Files\HDPlayer_7.11.8.0\`

Alternate drive variants: `D:\HDPlayer_7.11.8.0\`, `E:\HDPlayer_7.11.8.0\`, `F:\HDPlayer_7.11.8.0\`, `G:\HDPlayer_7.11.8.0\`

### 2.3 Registry Keys Written

```
HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\HDPlayer
HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\HDPlayer.exe
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{9A25302D-30C0-39D9-BD6F-21E6EC160475}
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{86CE1746-9EFF-3C9C-8755-81EA8903AC34}
HKCR\HdPlayer.boo\Shell\open\command
```

The `.boo` file association registers HDPlayer.exe as the handler for `.boo` program files (see Section 5).

---

## 3. Application Architecture

### 3.1 DLL Architecture

HDPlayer.exe is a minimal 140 KB launcher that dynamically loads three core DLLs at startup:

| DLL | Size | Role | Key Exports |
|-----|------|------|-------------|
| hcommon.dll | 2.1 MB | Common utilities | HAppConfig, font management, MD5, timezone table |
| MainWindow.dll | 836 KB | Main UI and logic | HMainWindow, program editing |
| HCatNet.dll | 735 KB | Network and event core | IEventCore, ICatEventBase |

**Note:** HCatNet.dll has a build date of August 21, 2025 — this is the most recently compiled component and indicates active ongoing development of the network protocol layer.

#### Additional DLLs

| DLL | Size | Function |
|-----|------|----------|
| NetIOServices.dll | 1.4 MB | Network I/O services |
| hguilib.dll | 4.4 MB | GUI component library |
| hctrlsplugin.dll | — | Controls plugin host |
| hctrlsexplugin.dll | — | Extended controls plugin |
| MainWindowRender.dll | — | Rendering utilities |
| HDownloadManger.dll | — | Download manager |
| HFFPlayLib.dll | — | FFmpeg wrapper library |
| EditDLL.dll | — | Content editing |
| EditDLLToImage.dll | — | Content-to-image export |
| HDART3DExport.dll | — | 3D content export |

### 3.2 Media Engines

#### HFFPlay.exe (Primary Video Engine)

- FFmpeg 4.x: `avcodec-58`, `avformat-58`, `avutil-56`
- SDL2 for audio output
- Direct3D for video rendering
- PDB path: `D:\WorkSpace\C++\HD Show\Trunk\release\HFFPlay.pdb`

Hardware acceleration backends (in priority order):

| Backend | API |
|---------|-----|
| DXVA2 | DirectX Video Acceleration 2 |
| D3D11VA | Direct3D 11 Video Acceleration |
| NVDEC/CUVID | NVIDIA CUDA video decode |
| Intel QuickSync | Intel Media SDK (MFX) |
| CUDA | NVIDIA CUDA general compute |
| AMF | AMD Advanced Media Framework |

CLI flags used by parent process:
```
-autoAccele     — enable automatic hardware acceleration selection
-showUI         — show player UI overlay
-video_size_auto — automatic video size detection
```

#### VLC Plugin Suite

Full VLC plugin tree including:
- Access: RTSP, RTMP, SRT, HLS, HTTP, file, UDP
- Codecs: H.264, H.265/HEVC, VP8, VP9, AAC, MP3
- Demux: MP4, MKV, TS, FLV, MPEG
- Output: DirectX, OpenGL, audio output

### 3.3 IPC / Single-Instance Enforcement

Uses Qt's `QLocalServer` / `QLocalSocket` (named pipe on Windows) for single-instance enforcement. When a second instance is launched, it sends a message to the existing instance via `sendMessage()`, which is received via the `messageReceived` signal and causes the existing window to raise/focus.

---

## 4. Network Protocol (from HCatNet.dll)

### 4.1 Transport Layer

Connection types enumerated in binary:

| Type | Description |
|------|-------------|
| Serial | COM port (RS-232/RS-485) |
| TCP/TLS | Encrypted TCP |
| TCP Service | Plain TCP server mode |
| Raw Socket | Raw socket connection |
| SDK1.0 | XML command protocol version 1 |
| SDK2.0 | XML command protocol version 2 |
| Serial SDK | SDK over serial port |
| Hex Raw | Raw hex data |
| Raw String | Raw string data |
| LCD SDK | LCD-specific SDK protocol |
| Udp Find Device | UDP broadcast device discovery |

### 4.2 Packet Format

All TCP protocol messages share a common frame structure:

```
┌─────────────────────────────────────────────────┐
│  u16  length    — total byte count incl. command │
│  u16  command   — command code (see table below) │
│  [N]  payload   — command-specific data          │
└─────────────────────────────────────────────────┘
```

- `length` field value = N + 2 (includes the 2-byte command field)
- Server responses use the same framing with the corresponding answer command code
- Multi-byte integers are little-endian

### 4.3 Complete Command Code Table

Known numeric codes are listed explicitly; codes marked `?` are present as string symbols in HCatNet.dll but have not yet been assigned numeric values through static analysis.

| Name | Code | Direction | Description |
|------|------|-----------|-------------|
| kTcpHeartbeatAsk | 0x005F | C→S | TCP keep-alive ping |
| kTcpHeartbeatAnswer | 0x0060 | S→C | TCP keep-alive pong |
| kSearchDeviceAsk | ? | C→? | UDP device search broadcast |
| kSearchDeviceAnswer | ? | S→C | UDP device search response |
| kErrorAnswer | ? | S→C | Generic error response |
| kLCDServiceAsk | ? | C→S | LCD service command |
| kLCDServiceAnswer | ? | S→C | LCD service response |
| kLCDMsgAsk | ? | C→S | LCD message send |
| kLCDMsgAnswer | ? | S→C | LCD message response |
| kSDKServiceAsk | 0x2001 | C→S | SDK service negotiation |
| kSDKServiceAnswer | 0x2002 | S→C | SDK service response |
| kSDKCmdAsk | 0x2003 | C→S | SDK XML command |
| kSDKCmdAnswer | 0x2004 | S→C | SDK XML response |
| kGPSInfoAnswer | ? | S→C | GPS data push |
| kFileStartAsk | 0x8001 | C→S | Begin file transfer |
| kFileStartAnswer | 0x8002 | S→C | File transfer start ack |
| kFileContentAsk | 0x8003 | C→S | File data chunk |
| kFileContentAnswer | 0x8004 | S→C | Chunk ack |
| kFileEndAsk | 0x8005 | C→S | End file transfer |
| kFileEndAnswer | 0x8006 | S→C | End ack |
| kReadFileAsk | ? | C→S | Request file from device |
| kReadFileAnswer | ? | S→C | File data from device |
| kFileListStartAsk | ? | C→S | Begin file list query |
| kFileListStartAnswer | ? | S→C | File list start ack |
| kFileListEndAsk | ? | C→S | End file list query |
| kFileListEndAnswer | ? | S→C | File list end + data |
| kSearchAsk | ? | C→S | Device search |
| kSearchAnswer | ? | S→C | Device search response |
| kDeviceInfoAsk | ? | C→S | Request device info |
| kDeviceInfoAnswer | ? | S→C | Device info response |
| kUpdateDeviceInfoAsk | ? | C→S | Update device info |
| kUpdateDeviceInfoAnswer | ? | S→C | Update device ack |
| kSetNetAddrAsk | ? | C→S | Set network address |
| kSetNetAddrAnswer | ? | S→C | Net addr ack |
| kGetNetAddrAsk | ? | C→S | Get network address |
| kGetNetAddrAnswer | ? | S→C | Net addr data |
| kVersionAsk | ? | C→S | Request version info |
| kVersionAnswer | ? | S→C | Version data |
| kUpdateProjectAsk | ? | C→S | Update project/program |
| kUpdateProjectAnswer | ? | S→C | Update ack |
| kFreeSpaceSizeAsk | ? | C→S | Query free storage |
| kFreeSpaceSizeAnswer | ? | S→C | Free space data |
| kFileListAsk | ? | C→S | List files on device |
| kFileListAnswer | ? | S→C | File list data |
| kImcompleteFileAsk | ? | C→S | Query incomplete transfers |
| kImcompleteFileAnswer | ? | S→C | Incomplete file list |
| kRemoveFileListAsk | ? | C→S | Delete file list |
| kRemoveFileListAnswer | ? | S→C | Delete ack |
| kOpenFileAsk | ? | C→S | Open file on device |
| kOpenFileAnswer | ? | S→C | Open file ack |
| kCloseFileAsk | ? | C→S | Close file on device |
| kCloseFileAnswer | ? | S→C | Close file ack |
| kTransEndAsk | ? | C→S | Transfer complete |
| kRecvEndAnswer | ? | S→C | Receive complete ack |
| kUpdateProjectQuit | ? | C→S | Cancel project update |
| kProjectQuitAnswer | ? | S→C | Quit ack |
| kFPGASettingInAsk | ? | C→S | Begin FPGA config session |
| kFPGASettingInAnswer | ? | S→C | FPGA config session ack |
| kFPGASettingOutAsk | ? | C→S | End FPGA config session |
| kFPGASettingOutAnswer | ? | S→C | End session ack |
| kFPGAParamSetAsk | ? | C→S | Set FPGA parameters |
| kFPGAParamSetAnswer | ? | S→C | Param set ack |
| kFPGASetCMDAsk | ? | C→S | Raw FPGA command |
| kFPGASetCMDAnswer | ? | S→C | Command ack |
| kBootScreenInAsk | ? | C→S | Begin boot screen config |
| kBootScreenOutAsk | ? | C→S | End boot screen config |
| kRemoveBootScreenAsk | ? | C→S | Delete boot screen |
| kLightSetInAsk | ? | C→S | Begin brightness config |
| kLightSetInAnswer | ? | S→C | Brightness session ack |
| kLightSetOutAsk | ? | C→S | End brightness config |
| kLightSetOutAnswer | ? | S→C | End brightness ack |
| kLightFileAsk | ? | C→S | Send brightness schedule file |
| kLightFileAnswer | ? | S→C | Brightness file ack |
| kTimeSetInAsk | ? | C→S | Begin time sync |
| kTimeSetInAnswer | ? | S→C | Time sync ack |
| kTimeSetOutAsk | ? | C→S | End time sync |
| kTimeSetOutAnswer | ? | S→C | End time sync ack |
| kSetTimeAsk | ? | C→S | Set device time |
| kSetTimeAnswer | ? | S→C | Set time ack |
| kGetTimeAsk | ? | C→S | Query device time |
| kGetTimeAnswer | ? | S→C | Device time data |
| kScreenTestInAsk | ? | C→S | Begin screen test |
| kScreenTestCMDAsk | ? | C→S | Screen test command |
| kBoxPlayerInAsk | ? | C→S | Connect to BoxPlayer |
| kBoxPlayerInAnswer | ? | S→C | BoxPlayer connect ack |
| kBoxPlayerPlayAsk | ? | C→S | Play command |
| kBoxPlayerPlayAnswer | ? | S→C | Play ack |
| kBoxPlayerStopAsk | ? | C→S | Stop playback |
| kBoxPlayerPlayImageAsk | ? | C→S | Play single image |
| kBoxPlayerStopImageAsk | ? | C→S | Stop image display |
| kBoxScreenTestDataAsk | ? | C→S | Screen test data |
| kBoxScreenTestDataAnswer | ? | S→C | Screen test ack |
| kBoxNetworkErrorAsk | ? | S→C | Network error notification |
| kUpgradeInAsk | ? | C→S | Begin firmware upgrade |
| kUpgradeCMDAsk | ? | C→S | Firmware upgrade command |
| kUpgradeOutAsk | ? | C→S | End firmware upgrade |
| kScreenWidthHeightAsk | ? | C→S | Query screen dimensions |
| kScreenWidthHeightAnswer | ? | S→C | Screen dimensions |
| kUpdateIDAsk | ? | C→S | Update device ID |
| kUpdateMACAAsk | ? | C→S | Update MAC address |
| kTcpHeartbeatPacketAsk | ? | C→S | Extended heartbeat |
| kTcpHeartbeatPacketAnswer | ? | S→C | Extended heartbeat ack |
| kUpdateDevNameAsk | ? | C→S | Rename device |
| kUpdateDevNameAnswer | ? | S→C | Rename ack |
| kSensorCMD | ? | C→S | Sensor command |
| kSetServerAddrAsk | ? | C→S | Set cloud server address |
| kGetServerAddrAsk | ? | C→S | Get cloud server address |
| kDeviceRebootInAsk | ? | C→S | Initiate reboot |
| kRebootNowAsk | ? | C→S | Reboot immediately |
| kKeyDefinitionsInAsk | ? | C→S | Begin key definitions |
| kKeyDefinitionsSetInfoAsk | ? | C→S | Set key definition |
| kKeyDefinitionsGetInfoAsk | ? | C→S | Get key definitions |
| kReloadKeyDefinitionsAsk | ? | C→S | Reload key config |
| kBoxPlayerSwitchingProgram | ? | S→C | Program switch notification |
| kSwitchProgramIndexAsk | ? | C→S | Switch to program by index |
| kSwitchProgramIndexAnswer | ? | S→C | Switch ack |
| kSwitchScreenInAsk | ? | C→S | Switch screen session start |
| kSwitchScreenFileAsk | ? | C→S | Switch screen with file |
| kBoxIOClientInAsk | ? | C→S | IO client connect |
| kBoxIOClientOutAsk | ? | C→S | IO client disconnect |
| kBoxPlayerConnectChangeAsk | ? | S→C | Connection state changed |
| kLogInAsk | ? | C→S | Login (admin auth) |
| kLogOutAsk | ? | C→S | Logout |
| kUpdateTypeAsk | ? | C→S | Update type info |
| kProjectCompleteAsk | ? | C→S | Project complete notification |
| kRemoveItemListAsk | ? | C→S | Remove items from list |
| kRemoveAllAsk | ? | C→S | Remove all programs |
| kItemStatusInAsk | ? | C→S | Begin item status session |
| kItemStatusOutAsk | ? | C→S | End item status session |
| kUploadDeviceInfoAsk | ? | C→S | Upload device info to cloud |
| kItemNoListAsk | ? | C→S | Query item numbers |
| kFileMD5ListAsk | ? | C→S | Query file MD5 hashes |
| kGetNoSendMD5ListAsk | ? | C→S | Get unsent MD5 list |
| kFunNodePositionAsk | ? | C→S | Function node position |
| kFunNodeUpdateAsk | ? | C→S | Update function node |
| kMemoryDataAsk | ? | C→S | Query memory data |
| kItemStatusSelectFileAsk | ? | C→S | Select file for status |
| kItemStatusResultFileAsk | ? | C→S | Get status result file |
| kSetItemToNullAsk | ? | C→S | Clear item |
| kGetItemStatusAsk | ? | C→S | Get item status |
| kGetParseResultAsk | ? | C→S | Get parse result |
| kDynamicEditInAsk | ? | C→S | Begin dynamic edit |
| kParseCmdFileAsk | ? | C→S | Parse command file |
| kDynamicEditOutAsk | ? | C→S | End dynamic edit |
| kProgramFileReadyAsk | ? | C→S | Program file ready |
| kDownloadProgramFileAnswer | ? | S→C | Program file download |
| kPppoeSetInAsk | ? | C→S | PPPoE config start |
| kPppoeInfoAsk | ? | C→S | PPPoE info query |
| kPppoeOverAsk | ? | C→S | PPPoE config end |
| kWirelessSetInAsk | ? | C→S | WiFi config start |
| kWirelessInfoAsk | ? | C→S | WiFi info query |
| kSetWirelessAsk | ? | C→S | Set WiFi config |
| kWirelessOverAsk | ? | C→S | WiFi config end |
| kNetworkStatusAnswer | ? | S→C | Network status notification |
| kNetworkConnected | ? | S→C | Network connected event |
| kNetworkUnconnected | ? | S→C | Network disconnected event |
| kContentStartAsk | ? | C→S | Content session start |
| kContentDataAsk | ? | C→S | Content data send |
| kContentEndAsk | ? | C→S | Content session end |
| kAppExternInAsk | ? | C→S | External app session start |
| kAppExternOutAsk | ? | C→S | External app session end |
| kTcpTranInAsk | ? | C→S | TCP transparent tunnel start |
| kTcpTranOutAsk | ? | C→S | TCP transparent tunnel end |
| kUdpTranAsk | ? | C→S | UDP transparent tunnel |
| kUdpTranAnswer | ? | S→C | UDP tunnel ack |
| kUpdateDeviceInfoExt1Ask | ? | C→S | Extended device info update |
| kProgramIndexChangedAsk | ? | S→C | Program index changed event |
| kBoxPlayerTimeZoneAsk | ? | C→S | Set player time zone |
| kReloadAllFontsAsk | ? | C→S | Reload font cache |
| kGUIDSwitchProgramAsk | ? | C→S | Switch program by GUID |
| kChangeProgramAsk | ? | C→S | Change active program |
| kTestDeviceLockerAsk | ? | C→S | Test device lock |
| kCheckDeviceLockerAsk | ? | C→S | Check device lock status |
| kHDMIInAsk | ? | C→S | HDMI input command |
| kClientInfoAsk | ? | C→S | Send client info |
| kReloadFPGAParamAsk | ? | C→S | Reload FPGA parameters |
| kUpgradeFinishAsk | ? | C→S | Firmware upgrade complete |
| kConvertDataToOldAsk | ? | C→S | Convert to legacy data format |

### 4.4 Error / Status Codes

From HCatNet.dll string analysis:

```
kSuccess              = 0    OK
kWriteFinish          = 1    Write completed
kProcessError         = 2    Processing error
kVersionTooLow        = 3    Client version too old
kDeviceOccupa         = 4    Device occupied
kFileOccupa           = 5    File occupied
kReadFileExcessive    = ?    Read size exceeded
kInvalidPacketLen     = ?    Bad packet length
kInvalidParam         = ?    Invalid parameter
kNotSpaceToSave       = ?    Insufficient storage
kCreateFileFailed     = ?    File creation failed
kWriteFileFailed      = ?    File write failed
kReadFileFailed       = ?    File read failed
kInvalidFileData      = ?    Bad file data
kFileContentError     = ?    File content error
kOpenFileFailed       = ?    File open failed
kSeekFileFailed       = ?    Seek failed
kRenameFailed         = ?    Rename failed
kFileNotFound         = ?    File not found
kFileNotFinish        = ?    File incomplete
kXmlCmdTooLong        = ?    XML command too long
kInvalidXmlIndex      = ?    Invalid XML index
kParseXmlFailed       = ?    XML parse failure
kInvalidMethod        = ?    Unknown XML method
kMemoryFailed         = ?    Memory allocation failure
kSystemError          = ?    System-level error
kUnsupportVideo       = ?    Video format not supported
kNotMediaFile         = ?    Not a media file
kParseVideoFailed     = ?    Video parse error
kUnsupportFrameRate   = ?    FPS not supported
kUnsupportResolution  = ?    Resolution not supported
kUnsupportFormat      = ?    Format not supported
kUnsupportDuration    = ?    Duration not supported
kDownloadFileFailed   = ?    Download failure
kDownloadingFile      = ?    Download in progress
kProcessing           = ?    Operation in progress
kScreenNodeIsNull     = ?    No active screen
kNodeExist            = ?    Node already exists
kNodeNotExist         = ?    Node not found
kPluginNotExist       = ?    Plugin not installed
kCheckLicenseFailed   = ?    License check failed
kNotFoundWifiModule   = ?    No WiFi module
kTestWifiUnsuccessful = ?    WiFi test failed
kRunningError         = ?    Runtime error
kUnsupportMethod      = ?    Method not supported
kInvalidGUID          = ?    Invalid GUID
kFirmwareFormatError  = ?    Bad firmware file
kTagNotFound          = ?    XML tag not found
kAttrNotFound         = ?    XML attribute not found
kCreateTagFailed      = ?    Tag creation failed
kUnsupportDeviceType  = ?    Device type not supported
kPermissionDenied     = ?    Permission denied
kPasswdTooSimple      = ?    Password too simple
kUsbNotInsert         = ?    USB not inserted
kDelayRespond         = ?    Delayed response (in progress)
kShortlyReturn        = ?    Short-term return
kNotSupportSDK        = ?    SDK not supported
```

### 4.5 File Transfer Log Format

The file transfer protocol uses a semicolon-separated log format for tracking transfer state:

```
{name};;{size};;{md5}        — file entry (queued)
{name};;{size};;{md5};;0     — send in progress
{name};;{size};;{md5};;1     — send complete
{name};;{size};;{md5};;2     — read
{name};;{size};;{md5};;3     — delay
```

Progress logging format:

```
[{session_id}] sending size[{sent}]/[{total}]
```

### 4.6 SDK XML Protocol (SDK1.0 / SDK2.0)

The SDK command protocol wraps operations in XML envelopes transported over the binary framing layer using command codes `0x2003` (request) and `0x2004` (response).

#### Request Format (kSDKCmdAsk, 0x2003)

```xml
<?xml version='1.0' encoding='utf-8'?>
<sdk guid="{CLIENT_GUID}">
  <in method="{MethodName}">
    <!-- method-specific parameters as child elements or attributes -->
  </in>
</sdk>
```

#### Response Format (kSDKCmdAnswer, 0x2004)

```xml
<?xml version='1.0' encoding='utf-8'?>
<sdk guid="{CLIENT_GUID}">
  <out method="{MethodName}">
    <result value="0"/>  <!-- 0 = success, non-zero = error code from Section 4.4 -->
    <!-- response data as child elements -->
  </out>
</sdk>
```

Version check: The `GetIFVersion` method returns a version integer >= 1000000. Used by clients to negotiate protocol capabilities before issuing other commands.

### 4.7 UDP Discovery Protocol (Port 9527)

Devices listen on UDP port 9527 and respond to broadcast discovery packets.

Discovery response packet layout:

```
┌──────────────────────────────────────────────────────────────┐
│  15 bytes   device_id        null-padded ASCII device ID     │
│   4 bytes   ipv4_address     IPv4 address (network order)    │
│   N bytes   player_name      null-terminated ASCII string    │
│   M bytes   DeviceInfo XML   XML payload with device details │
└──────────────────────────────────────────────────────────────┘
```

---

## 5. XML Program Format (.boo Files)

### 5.1 File Location on Device

| Path | Contents |
|------|----------|
| `/tmp/hdTempProgram/program.boo` | Currently active program |
| `/root/Box/project/` | Persistent project storage |
| `/root/Box/project/sdk/image/` | SDK-generated images |
| `/root/Box/project/temp/project/` | Temporary project staging |

### 5.2 .boo File Structure

The `.boo` file is a standard UTF-8 XML file wrapped in the SDK envelope format. It uses the `AddProgram` method to deliver a complete program definition to the device.

```xml
<?xml version='1.0' encoding='utf-8'?>
<sdk guid="{GUID}">
  <in method="AddProgram">
    <screen timeStamps="{UNIX_TIMESTAMP}">

      <program guid="{GUID}" type="normal|global">

        <!-- Border/frame decoration -->
        <border index="{1-13}" effect="{solid|rainbow|chase|...}" speed="{1-10}"/>

        <!-- Associated media file -->
        <file name="{filename}"/>

        <!-- Play mode: LoopTime — play N times -->
        <playControl count="{N}" disabled="{true|false}"/>

        <!-- Play mode: FixedTime — play for N seconds with optional schedule -->
        <playControl duration="{seconds}" disabled="{true|false}">
          <time start="HH:MM" end="HH:MM"/>
          <date start="YYYY-MM-DD" end="YYYY-MM-DD"/>
          <week enable="{7-char binary, e.g. 1111111}"/>
        </playControl>

        <!-- Play modes also include: TimeLimit, SpecifiedDate, DateLimit -->

        <!-- Content area -->
        <area guid="{GUID}" alpha="{0-255}">
          <rectangle x="{x}" y="{y}" width="{w}" height="{h}"/>
          <resources>
            <!-- content items go here (see Section 5.3) -->
          </resources>
        </area>

      </program>
    </screen>
  </in>
</sdk>
```

### 5.3 Content Item Formats

#### Image

```xml
<image guid="{GUID}" fit="{stretch|fill|center|fit}"/>
```

#### Video

```xml
<video guid="{GUID}" aspectRatio="{0=stretch|1=keep}"/>
```

#### Text (Multi-Line)

```xml
<text guid="{GUID}" singleLine="false" background="#{RRGGBB}">
  <string>{text content}</string>
  <font name="{fontname}" italic="{true|false}" bold="{true|false}"
        underline="{true|false}" size="{pt}" color="#{RRGGBB}"/>
  <style align="{left|center|right}" valign="{top|middle|bottom}"/>
  <effect in="{0-29}" inSpeed="{0-8}" out="{0-29}" outSpeed="{0-8}"
          duration="{tenths_of_seconds}"/>
</text>
```

#### Single-Line Text

```xml
<text guid="{GUID}" singleLine="true" background="#{RRGGBB}">
  <!-- same child elements as multi-line -->
</text>
```

Additional text attributes: `DispEffect`, `ClearEffect`, `DispTime`, `ClearTime`, `HoldTime`, `ContentAlign`, `ContentHAlign`

#### Clock (Digital)

```xml
<clock guid="{GUID}" type="{0=digital|1=analog}" timezone="{UTC offset}"
       adjust="{seconds_offset}">
  <font size="{pt}" name="{fontname}"/>
  <title value="{display_text}" color="#{RRGGBB}" display="{true|false}"/>
  <date format="{1-6}" color="#{RRGGBB}" display="{true|false}"/>
  <week format="{1-3}" color="#{RRGGBB}" display="{true|false}"/>
  <lunarCalendar color="#{RRGGBB}" display="{true|false}"/>
  <time format="{1-4}" color="#{RRGGBB}" display="{true|false}"/>
</clock>
```

Digital clock additional attributes: `AdjustType`, `TimeAdjust`, `ClockType`, `TimeZone`, `LcdTimeZone`, `UseDaylightSavingTime`, `DaylightSavingTimeStart`, `DaylightSavingTimeEnd`, `DaylightSavingTimeIndex`, `TimeZoneIndex`

Analog clock additional attributes: `PictrueClockType`, `CustomDialPath`, `PictrueClockHourPointType`, `PictrueClockMinPointType`, `PictrueClockSecPointType`, `CustomDateFormat`, `CustomTimeFormat`, `HourHandColor`, `MinuteHandColor`, `SecondHandColor`, `HourScaleColor`, `MinuteScaleColor`, `HourScaleType`, `MinuteScaleType`, `SecondRectType`, `MinuteRectType`, `HourRectType`, `HourScaleFName`, `HourScaleFSize`

#### Photo / Image (Plugin Format)

Plugin-managed image with effect control:

Attributes: `KeepConvert`, `SendRawImage`, `HeadCloseToTail`, `PreloadFilePath`, `keepAspectRatio`

Effect attributes: `effectType`, `speedType`, `endToEnd`, `speed`, `effectTime`, `displayTime`

Fill types: `center`, `stretch`, `effect`

#### Weather

```xml
<weather guid="{GUID}" DisplayStyle="{0-5}" DateFontName="{font}"
         newExquisiteDisplayType="{type}"/>
```

### 5.4 Border / Frame Types

Border decoration attributes on `<area>` or `<program>` elements:

| Attribute | Values | Description |
|-----------|--------|-------------|
| `FrameEnable` | `true` / `false` | Enable/disable frame border |
| `FrameType` / `border index` | 0–13 | Frame style (0=none, 1=solid white, 2=solid red, ...) |
| `FrameSpeed` / `border speed` | 1–10 | Animation speed for animated border types |

---

## 6. Plugin System

### 6.1 Plugin Registry (PluginConfig.xml)

Plugins are loaded and prioritized as follows:

| Priority | Plugin IDs |
|----------|-----------|
| 1 — Scene/Frame | `HD_OrdinaryScene_Plugin`, `GlobalScene`, `HD_Frame_Plugin` |
| 2 — Video | `HD_Video_Plugin` |
| 3 — Text/Photo | `HD_Photo_Plugin`, `HD_Text_Plugin`, `HD_Gif_Plugin`, `HD_SingleLineText_Plugin`, `HD_animationText_Plugin`, `HD_Text3D_Plugin` |
| 4 — Data/Clock | `HD_Clock_Plugin`, `HD_CALENDAR_Plugin`, `HD_Time_Plugin`, `HD_Weather_Plugin`, `HD_Sensor_Plugin`, `HD_LIVESTREAM_Plugin`, `HD_Web_Plugin`, `HD_HDMI_IN_Plugin`, `HD_Neon_Plugin`, `HD_WPS_Plugin`, `HD_TABLE_Plugin`, `HD_Document_Plugin`, `HD_QRCODE_Plugin`, `HD_EWATCH_Plugin`, `HD_Temperature_Plugin`, `HD_Humidity_Plugin`, `HD_Controller_Plugin`, `HD_DynamicData_Plugin` |

### 6.2 Plugin DLL Details

| DLL | Size | Plugin ID | Key Attributes / Notes |
|-----|------|-----------|------------------------|
| text_plugin.dll | 175 KB | HD_Text_Plugin | singleLine, DispEffect, ClearEffect, DispTime, ClearTime, HoldTime, ContentAlign, ContentHAlign |
| video_plugin.dll | 643 KB | HD_Video_Plugin | aspectRatio, DXVA2/D3D11 YUV shaders |
| clock_plugin.dll | 3.4 MB | HD_Clock_Plugin | ClockType, TimeZone, DaylightSaving, analog hand images |
| weather_plugin.dll | 1.1 MB | HD_Weather_Plugin | DisplayStyle, newExquisiteDisplayType, AirQuality |
| photo_plugin.dll | 183 KB | HD_Photo_Plugin | fit/effect/fill, keepAspectRatio |
| animationText_plugin.dll | 6.8 MB | HD_animationText_Plugin | 3D text animations |
| neon_plugin.dll | 627 KB | HD_Neon_Plugin | neon border effects |
| table_plugin.dll | 749 KB | HD_TABLE_Plugin | grid data display |
| DynamicData_plugin.dll | 91 KB | HD_DynamicData_Plugin | external data binding |
| qrcode_plugin.dll | 84 KB | HD_QRCODE_Plugin | QR code generation |
| sensor_plugin.dll | 347 KB | HD_Sensor_Plugin | Modbus sensor integration |
| temperatures_plugin.dll | 139 KB | HD_Temperature_Plugin | temperature sensor display |
| humidity_plugin.dll | 131 KB | HD_Humidity_Plugin | humidity sensor display |
| Calendar_plugin.dll | 380 KB | HD_CALENDAR_Plugin | calendar display |
| time_plugin.dll | 229 KB | HD_Time_Plugin | time display |
| singlelinetext_plugin.dll | 218 KB | HD_SingleLineText_Plugin | single-line scrolling text |
| frame_plugin.dll | 119 KB | HD_Frame_Plugin | area frame/border rendering |
| screen_plugin.dll | 132 KB | screen_plugin | screen capture |
| ewatch_plugin.dll | 147 KB | HD_EWATCH_Plugin | e-ink watch display |
| liveStram_plugin.dll | 98 KB | HD_LIVESTREAM_Plugin | RTSP/RTMP live stream |
| web_plugin.dll | 144 KB | HD_Web_Plugin | embedded web page |
| wps_plugin.dll | 339 KB | HD_WPS_Plugin | WPS document display |
| hdmiin_plugin.dll | 104 KB | HD_HDMI_IN_Plugin | HDMI input capture |
| ordinary_scene_plugin.dll | 155 KB | HD_OrdinaryScene_Plugin | scene manager |
| test3d_plugin.dll | 71 KB | test3d_plugin | 3D test card |

---

## 7. Hardware Support

### 7.1 Sender Cards

Devices defined in `HDSet/device.xml`:

| Series | Models | AsyncArchitect | Max Canvas |
|--------|--------|----------------|------------|
| VP210/410 | VP210, VP210C, VP410, VP410C | 0 (Linux) | 2048×1024 |
| VP820/1220/1620 | Multiple variants | 0 (Linux) | 4096×2048 |
| VP2000+ | VP2060, VP2460, VP3060, VP4060, VP8000M | 1 (Android) | 16384×4096 |
| T-series | T901, T902, T902M, T16, T08, T08F, FT08 | 0 (Linux) | 2048×1024 |
| A-series | A3, A4, A5, A6, A3L–A6L | 1 (Android async) | 16384×4096 |
| B-series | B6, B6L, B8L | 1 (Android async) | 16384×4096 |
| C-series | C3i, C15, C16, C16L, C16H, C36 | mixed | 8192×2048 |
| D-series | D05, D06, D15, D16, D18, D35, D36, D68 | mixed | 8192×4096 |
| H-series | H6, H8 | 0 (Linux, USB_BULK) | 4096×2048 |

Connection types: `COM_115200`, `COM_256000`, `USB_BULK_V1.0`

### 7.2 Receiver Cards (FPGA)

Supported driver chips organized by manufacturer from `recvcardsupportinfo.xml`:

| Manufacturer | Supported Chips |
|---|---|
| ICN | ICN2038, ICN2045, ICN2053, ICN2163, ICN2165 series |
| SM | SM16207, SM16227, SM16259, SM16289, SM16359, SM16389, SM16395 |
| LS | LS9919, LS9929, LS9935, LS9956 |
| MBI | MBI5041, MBI5153, MBI5252, MBI5268 |
| SUM | SUM2017, SUM2028, SUM2130 |
| FM | FM6124, FM6127, FM6153, FM6363, FM6565 |
| DP | DP3264, DP3265, DP3269, DP5125 |
| MY | MY9862, MY9866, MY9868 |
| CFD | CFD135A, CFD455A, CFD555A, CFD555B |
| CS | CS2033, CS2017 |
| GS | GS6238S, GS6263 |
| HX | HX8863, HX8864, HX8866 |
| RT | RT5965, RT5967 |
| CNS | CNS7153, CNS7253, CNS7263 |
| Others | UCS5603, HBS1910, HBS1923, HBS2910, HBS2920, HG2248 |

Line decode chips: ICN2012, ICN2013, ICN2018, SM5166, SM5266, SM5366, SM5388, DP32019, DP32030, DP32129, TC7258, TC7558, TC7559B, 138 (SN74138 3-to-8 decoder), 595 (74HC595 shift register), RT5958, HX6158H, HX6258, HX6157, VB5658

### 7.3 SoC Targets (Linux ARM)

Shared libraries in `HDSetSo/`:

| File | Target SoC |
|------|------------|
| HDSetApps-rk3326.so | Rockchip RK3326 (PX30-based) |
| HDSetApps-rk356.so | Rockchip RK3566 / RK3568 |
| HDSetApps-t507.so | Allwinner T507 |
| libHDSet_PX30.so | Rockchip PX30 |
| libHDSet_PX30_RC.so | Rockchip PX30 (RC variant) |
| libHDSet_rk3188.so | Rockchip RK3188 |
| libHDSet_rk3288.so | Rockchip RK3288 |

### 7.4 USB / Serial Drivers

| Driver | Purpose |
|--------|---------|
| CH341SER | CH341 USB-to-serial adapter (COM-based sender cards) |
| CH343Ser | CH343 USB-to-serial adapter |
| CP210x VCP | Silicon Labs CP210x (x86 and x64 variants) |
| libusbK / libusb0 | USB bulk transfer for H-series `USB_BULK_V1.0` connection |

---

## 8. Cloud Integration

### 8.1 OMS Cloud Server

| Setting | Value |
|---------|-------|
| Default host | `clouds.huidu.cn` |
| Default port | 80 |
| HTTP User-Agent | `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36` |

### 8.2 Configuration File (config.ini)

Located in the application data directory:

```ini
[General]
DeletePrompt=true
SavePrompt=false
UsePasswd=true
ServerIP=clouds.huidu.cn
ServerPort=80
lang=简体中文
TreeViewWidth=200
```

---

## 9. Visual Effects

### 9.1 HLSL Effects (Effects/default.fx)

Implements `renderMode=3` (dazzle / 炫彩) with the following shader uniforms and behavior:

```hlsl
// Uniforms
float NowTime;   // current time in seconds (animated)
float Speed;     // animation speed multiplier
float Dazzle;    // dazzle intensity

// 9 gradient pattern modes:
// 0 — horizontal gradient
// 1 — vertical gradient
// 2 — tiled pattern
// 3 — diagonal gradient
// 4 — rotational sweep
// 5–8 — additional rotational/diagonal variants

// Color space: HSV → RGB conversion for animated color cycling
// Hue is driven by NowTime * Speed to produce continuous color animation
```

### 9.2 NeonPic Assets

```
NeonPic/MuliColor/    — Multi-color neon effect template images
NeonPic/SingleColor/  — Single-color neon effect template images
```

### 9.3 Web Player Zoom

JavaScript injected into the embedded web view for content scaling:

```javascript
document.body.style.transform = 'scale(%1)';
document.body.style.transformOrigin = '0 0';
document.body.style.width = (100 / %1) + '%';
window.scrollTo(%1, %2);
```

`%1` and `%2` are substituted at runtime with the computed scale factor and scroll offsets.

---

## 10. Font Management (fontManage.xml)

Font mapping configuration file structure:

```xml
<fontManage>
  <fontMap name="{display_name}" path="{font_file_path}">
    <map name="{alias_name}"/>
  </fontMap>
</fontManage>
```

Platform behavior:

- **Windows:** Font face names are enumerated from `HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\Fonts` and matched against `fontManage.xml` mappings.
- **Linux:** Uses FreeType (`FT_New_Face`) with direct path-based font loading from the paths specified in `fontManage.xml`.

---

## 11. Key String References (hcommon.dll)

### 11.1 Version Check Constraints by SDK TCP Version

| SDK Version | Constraint |
|-------------|-----------|
| v1.8 | Screen maximum 640×64 pixels |
| v1.x (earlier) | Screen area maximum 640×128 pixels |
| v1.x (earlier) | Screen height maximum 128 pixels |
| v1.x (earliest) | Screen maximum 384×320 pixels |

### 11.2 Output Image Paths

| Path | Contents |
|------|----------|
| `/outputpath/IconvImage/` | Converted images (format-converted for display) |
| `/outputpath/CropPhoto/` | Cropped photo outputs |
| `/outputpath/Text/` | Pre-rendered text images |

---

## 12. GUID Format

All GUIDs in the `.boo` XML format and SDK protocol use standard UUID v4 format (RFC 4122), generated randomly per session:

```
%08X-%04X-%04x-%02X%02X-%02X%02X%02X%02X%02X%02X
```

Example: `9A25302D-30C0-39D9-BD6F-21E6EC160475`

---

## 13. Build Environment

| Item | Value |
|------|-------|
| Source tree root | `E:\hdplayer\Branches\PX30_A8\` |
| HDPlayer.exe PDB | `E:\hdplayer\Branches\PX30_A8\release\map\Program.pdb` |
| MainWindow.dll PDB | `E:\hdplayer\Branches\PX30_A8\release\map\MainWindow.pdb` |
| HFFPlay.exe PDB | `D:\WorkSpace\C++\HD Show\Trunk\release\HFFPlay.pdb` |
| C++ Runtime | MSVC — VCRUNTIME140.dll, MSVCP140.dll |
| Qt version | 5.x (Qt5Core, Qt5Gui, Qt5Network, Qt5Widgets, Qt5WebEngine) |
| HDPlayer.exe build date | November 4, 2022 |
| HCatNet.dll build date | August 21, 2025 (actively maintained) |

The gap between the HDPlayer.exe build date (2022) and HCatNet.dll (2025) indicates that the network protocol layer is under active development while the launcher binary has remained stable.
