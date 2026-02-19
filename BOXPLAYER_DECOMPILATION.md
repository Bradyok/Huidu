# Huidu BoxPlayer Full Decompilation Analysis

## Executive Summary

The Huidu BoxPlayer is **NOT an Android APK** - it is a **native C++ Linux application** built on Qt 5 + OpenGL ES 2.0 that renders content via DRM/KMS direct framebuffer rendering and outputs pixel data to LED panels via FPGA hardware over serial (`/dev/ttyS1` on PX30, `/dev/cyclone4-0`/`/dev/cyclone4-1` on RK3288).

### Build Environment (from compiler strings)
- **Compiler:** GCC 6.4.0 (Buildroot 2018.02-rc3)
- **Qt:** 5.x (Qt5Core, Qt5Gui, Qt5Widgets, Qt5Xml, Qt5Network)
- **Graphics:** OpenGL ES 2.0 (libGLESv2), EGL, DRM/KMS (libdrm, libgbm)
- **Video:** FFmpeg (libavcodec 57, libavformat 57) + GStreamer 1.0 + Rockchip MPP
- **Scripting:** Lua 5.1 embedded (for hwSetting.lua hardware config)
- **HTTP:** libcurl
- **Audio:** libspeex

The APKs (`cn.huidu.BoxPlayerLoader.apk`) are thin Android JNI wrappers that simply bootstrap the native shared libraries on Android-based hardware variants. On pure Linux variants (PX30), the player runs as a standalone ELF binary.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    HDPlayer PC Software                     │
│              (sends programs via TCP/USB)                    │
└────────────────────────┬────────────────────────────────────┘
                         │ TCP (XML protocol)
                         ▼
┌─────────────────────────────────────────────────────────────┐
│  BoxSDK (Network Server)                                     │
│  ├── HTcpServer (accepts connections from HDPlayer)          │
│  ├── HBoxIOServices (dispatches commands)                    │
│  ├── HXmlSession (parses XML protocol)                       │
│  ├── HFileManager (receives program files)                   │
│  └── HServicesManager (orchestrates services)                │
│       ├── HSHwSet (hardware settings)                        │
│       ├── HSLight (brightness control)                       │
│       ├── HSScreenOnoff (screen on/off scheduling)           │
│       ├── HSTime (time sync/NTP)                             │
│       ├── HSUpgrade (firmware OTA)                            │
│       ├── HSModbus (Modbus RTU/TCP)                          │
│       ├── HSSerialSDK (serial port protocol)                 │
│       └── HTcpModbusClient (Modbus TCP client)               │
├─────────────────────────────────────────────────────────────┤
│  IPC (Unix domain sockets / pipes)                           │
├─────────────────────────────────────────────────────────────┤
│  BoxPlayer (Rendering Engine)                                │
│  ├── libMainWindow.so (scene/area management)                │
│  │   ├── HProgramSession (program lifecycle)                 │
│  │   ├── HAreaSession (area/zone rendering)                  │
│  │   └── HScreenTestPlayer (test patterns)                   │
│  ├── libMainWindowRender.so (pixel composition)              │
│  │   ├── HProgramSession (program rendering)                 │
│  │   ├── HPictureSession (image compositing)                 │
│  │   ├── HImageSurface (surface management)                  │
│  │   ├── HDoubleImage (double-buffered rendering)            │
│  │   └── HFrame / HNodeAttr (frame effects)                 │
│  ├── libhcommon.so (shared components)                       │
│  │   ├── HSceneNode (scene tree)                             │
│  │   ├── PlayerFactory (plugin loading)                      │
│  │   ├── HGifPlay / HGifReader (GIF animation)              │
│  │   ├── ImageConverter (format conversion)                  │
│  │   ├── HScreenTestPlayer (test patterns)                   │
│  │   └── effect_node / effecttext_node (effects)             │
│  ├── libCore.so (core framework)                             │
│  │   ├── HIPCServer / HIPCClient (IPC)                       │
│  │   ├── HTcpSocket / HUdpSocket (networking)               │
│  │   ├── HEventManager (event loop)                          │
│  │   ├── HBoxPlayerConfig (configuration)                    │
│  │   └── HSystemEnv (system environment)                     │
│  ├── libXmlSDK.so (program file parser)                      │
│  │   ├── HSDKCmdManager (command dispatch)                   │
│  │   ├── HFileServices (file I/O)                            │
│  │   ├── HModuleManager (module registry)                    │
│  │   └── HMProgram / HMPlayList (program/playlist XML)       │
│  └── plugins/ (content type renderers)                       │
│       ├── libvideo_plugin.so (video playback)                │
│       ├── libphoto_plugin.so (image display)                 │
│       ├── libtext_plugin.so (static text)                    │
│       ├── libsinglelinetext_plugin.so (scrolling text)       │
│       ├── libanimationText_plugin.so (animated text)         │
│       ├── libclock_plugin.so (analog/digital clock)          │
│       ├── libCalendar_plugin.so (calendar widget)            │
│       ├── libtime_plugin.so (time display)                   │
│       ├── libweather_plugin.so (weather widget)              │
│       ├── libtemperatures_plugin.so (temperature sensor)     │
│       ├── libhumidity_plugin.so (humidity sensor)            │
│       ├── libneon_plugin.so (neon/border effects)            │
│       ├── libframe_plugin.so (frame/border rendering)        │
│       ├── libscreen_plugin.so (screen management)            │
│       ├── libtable_plugin.so (table/grid data)               │
│       ├── libDynamicData_plugin.so (dynamic data binding)    │
│       ├── libNetworkData_plugin.so (network data sources)    │
│       ├── libmodbus_plugin.so (Modbus data display)          │
│       ├── libsensor_plugin.so (sensor readouts)              │
│       ├── libdocument_plugin.so (document rendering)         │
│       ├── libwps_plugin.so (WPS document support)            │
│       ├── libweb_plugin.so (web content)                     │
│       ├── libhdmiin_plugin.so (HDMI input capture)           │
│       ├── librdm_plugin.so (RDM protocol)                    │
│       ├── libewatch_plugin.so (e-watch)                      │
│       ├── libtest3d_plugin.so (3D test)                      │
│       └── libordinary_scene_plugin.so (scene playback)       │
├─────────────────────────────────────────────────────────────┤
│  libFPGADriver.so (FPGA Hardware Interface)                  │
│  ├── fpga::HFPGACore (core FPGA communication)              │
│  ├── fpga::HFPGASerial (serial transport to FPGA)           │
│  ├── fpga::HSendCard (send card configuration)               │
│  ├── fpga::HRecvCard (receiving card configuration)          │
│  ├── fpga::HTask (FPGA task scheduler)                       │
│  └── fpga::HUpgrade (FPGA firmware update)                   │
│       Devices: /dev/cyclone4-0, /dev/cyclone4-1              │
├─────────────────────────────────────────────────────────────┤
│  System Services                                             │
│  ├── BoxDaemon (watchdog/process supervisor)                 │
│  ├── BoxUpgrade (OTA firmware upgrade)                       │
│  ├── BootLogo (boot splash display)                          │
│  ├── cn.huidu.device.api (Go REST API daemon)               │
│  └── cn.huidu.device.service (device service)               │
└─────────────────────────────────────────────────────────────┘
```

---

## Binary Analysis

### Platform: PX30 (Linux / aarch64)

| Binary | Size | Type | Description |
|--------|------|------|-------------|
| `BoxPlayer/BoxPlayer` | 14 KB | ELF 64-bit aarch64 | Main entry - loads libMainWindow.so, runs Qt offscreen |
| `BoxPlayer/BoxSDK` | 106 KB | ELF 64-bit aarch64 | Network SDK server - handles HDPlayer protocol |
| `System/BoxDaemon` | 31 KB | ELF 64-bit aarch64 | Process watchdog/supervisor |
| `System/BoxUpgrade` | 258 KB | ELF 64-bit aarch64 | OTA upgrade handler |
| `System/BootLogo` | 35 KB | ELF 64-bit aarch64 | Boot splash renderer |

### Platform: RK3288 (Android / ARM32) - **NOT STRIPPED (symbols available)**

| Library | Size | Description |
|---------|------|-------------|
| `libSDKServices.so` | 2.1 MB | **Largest** - all SDK services, networking, protocol handling |
| `libCore.so` | 1.3 MB | Core framework: IPC, sockets, events, config, threads |
| `libhcommon.so` | 1.1 MB | Shared rendering: scene nodes, effects, GIF, image conversion |
| `libBoxIOServices.so` | 992 KB | I/O services: TCP server, command dispatch, platform services |
| `libFPGADriver.so` | 823 KB | FPGA hardware driver: serial comm, send/recv card management |
| `libXmlSDK.so` | 545 KB | XML protocol parser: commands, programs, playlists |
| `libMainWindow.so` | 345 KB | Scene/area management, program sessions |
| `libMainWindowRender.so` | 298 KB | Pixel composition, double-buffering, frame effects |
| `libBasicCore.so` | 13 KB | Basic utilities (thin wrapper) |

### Key Insight: Rendering Pipeline

```
BoxPlayer runs with: -platform offscreen  (Qt offscreen rendering)

1. Program XML loaded by libXmlSDK.so
2. Scene tree built by HSceneNode (libhcommon.so)
3. Areas created as HAreaSession objects
4. Each area loads content plugins (video, text, clock, etc.)
5. Plugins render to QImage surfaces
6. libMainWindowRender.so composites all areas via QPainter
7. HDoubleImage provides double-buffered output
8. Rendered framebuffer sent to FPGA via serial (/dev/cyclone4-x)
9. FPGA distributes pixel data to send cards → receiving cards → LED panels
```

---

## Reconstructed Source Tree (284 files)

```
sdk/
├── BoxIOServices/                    # Network I/O services (libBoxIOServices.so)
│   ├── HBoxIOServices.cpp           # Main I/O service manager
│   ├── HPlatformService.cpp         # Platform-specific services
│   ├── HRDMInfoLog.cpp              # RDM protocol logging
│   ├── HSearchService.cpp           # Device discovery
│   ├── HTcpServer.cpp               # TCP server for HDPlayer connections
│   ├── WriteFiles.cpp               # File write operations
│   ├── OldSDK/                      # Legacy SDK compatibility layer
│   │   ├── HFileManager.cpp         # File transfer handling
│   │   ├── HInterfaceManager.cpp    # Method dispatch interface
│   │   ├── IMethod.h                # Method interface (abstract)
│   │   ├── MBootLogo.cpp            # Boot logo method
│   │   ├── MDataSource.cpp          # Data source method
│   │   ├── MDevelopment.cpp         # Development/debug methods
│   │   ├── MEthernet.cpp            # Ethernet config method
│   │   ├── MGeneral.cpp             # General settings method
│   │   ├── MLicense.cpp             # License management method
│   │   ├── MLightPloy.cpp           # Light/brightness policy method
│   │   ├── MModbusInfo.cpp          # Modbus config method
│   │   ├── MPppoe.cpp               # PPPoE method
│   │   ├── MReadBackLog.cpp         # FPGA readback log method
│   │   ├── MReboot.cpp              # Reboot method
│   │   ├── MScreen.cpp              # Screen config method
│   │   ├── MScreenOnOff.cpp         # Screen on/off schedule method
│   │   ├── MSensor.cpp              # Sensor method
│   │   ├── MSensorUpgrade.cpp       # Sensor firmware upgrade method
│   │   ├── MSystemEnv.cpp           # System environment method
│   │   ├── MTimeAdjust.cpp          # Time adjustment method
│   │   ├── MUsbDevice.cpp           # USB device method
│   │   ├── MWifi.cpp                # WiFi config method
│   │   ├── ModuleManager.cpp        # Module registry
│   │   ├── hmrelay.cpp              # Relay control method
│   │   └── mshowiconinfo.cpp        # Icon display method
│   └── Servcies/                    # Network service handlers
│       ├── HNetAppExtern.cpp        # External app integration
│       ├── HNetBootLogo.cpp         # Boot logo network service
│       ├── HNetHwSet.cpp            # Hardware settings network service
│       ├── HNetKeyDefine.cpp        # Key definition service
│       ├── HNetLight.cpp            # Light/brightness service
│       ├── HNetLocalUpgrade.cpp     # Local upgrade service
│       ├── HNetOldSDK.cpp           # Legacy SDK compatibility service
│       ├── HNetPpppoe.cpp           # PPPoE service
│       ├── HNetReboot.cpp           # Reboot service
│       ├── HNetScreenTest.cpp       # Screen test service
│       ├── HNetSwitchScreen.cpp     # Screen switching service
│       ├── HNetTime.cpp             # Time sync service
│       ├── HNetUpgrade.cpp          # Firmware upgrade service
│       ├── HNetWifi.cpp             # WiFi service
│       ├── HUdpTran.cpp             # UDP transport
│       ├── HUpdateProject.cpp       # Project/program update service
│       └── HWebUpgrade.cpp          # Web-based upgrade service
│
├── Core/                            # Core framework (libCore.so)
│   ├── Data/                        # Data model classes
│   │   ├── Data.cpp / Data.h        # Base data types
│   │   ├── DataTools.cpp            # Data utilities
│   │   ├── HAdminModeInfo.cpp       # Admin mode settings
│   │   ├── HBootLogoInfo.cpp        # Boot logo config
│   │   ├── HBoxPlayerConfig.cpp     # Player configuration
│   │   ├── HCustomResolutionInfo.cpp # Custom resolution
│   │   ├── HEthernetInfo.cpp        # Ethernet settings
│   │   ├── HFontInfo.cpp            # Font configuration
│   │   ├── HFuncEnableInfo.cpp      # Feature flags
│   │   ├── HGeneralInfo.cpp         # General device info
│   │   ├── HHDMISwitchInfo.cpp      # HDMI switch settings
│   │   ├── HHardwareInfo.cpp        # Hardware info
│   │   ├── HLightInfo.cpp           # Brightness settings
│   │   ├── HMediaFileInfo.cpp       # Media file metadata
│   │   ├── HModbus.cpp              # Modbus data structures
│   │   ├── HOnboardRelayInfo.cpp    # Relay settings
│   │   ├── HScreenOnoffInfo.cpp     # Screen schedule
│   │   ├── HScreenSyncAdjustTimeInfo.cpp # Screen sync timing
│   │   ├── HScreenTestInfo.cpp      # Screen test config
│   │   ├── HSensorInfo.cpp          # Sensor data
│   │   ├── HSerialSDKInfo.cpp       # Serial SDK config
│   │   ├── HServerAddrInfo.cpp      # Server addresses
│   │   ├── HTTSInfo.cpp             # Text-to-speech settings
│   │   ├── HTimeInfo.cpp            # Time settings
│   │   ├── HUsbDeviceInfo.cpp       # USB device info
│   │   ├── HlicenseInfo.cpp         # License data
│   │   └── hvolumeployitem.cpp      # Volume policy
│   ├── GetGateway.cpp               # Network gateway detection
│   ├── HAnalysisVideo.cpp/.h        # Video file analysis
│   ├── HAsyncExec.cpp               # Async execution
│   ├── HCallbackManage.cpp          # Callback management
│   ├── HConfigParam.cpp             # Global config parameters
│   ├── HConnectTest.cpp             # Connection testing
│   ├── HDNSParse.cpp                # DNS resolution
│   ├── HDnsMonitor.cpp              # DNS monitoring
│   ├── HErrorCode.cpp               # Error codes
│   ├── HEvent.cpp                   # Event system
│   ├── HEventManager.cpp            # Event loop
│   ├── HIPCClient.cpp               # IPC client (Unix sockets)
│   ├── HIPCServer.cpp               # IPC server
│   ├── HLog.cpp                     # Logging
│   ├── HMainEvent.cpp               # Main event handler
│   ├── HNetTools.cpp                # Network utilities
│   ├── HObjectManage.cpp            # Object lifecycle
│   ├── HOptLog.cpp                  # Operation logging
│   ├── HPipeSocket.cpp              # Named pipe IPC
│   ├── HReadWriter.cpp              # Buffered R/W
│   ├── HSelector.cpp                # Select/poll wrapper
│   ├── HShell.cpp                   # Shell command execution
│   ├── HStateList.cpp               # State machine
│   ├── HSystemEnv.cpp               # System environment
│   ├── HTcpHDSetSo.cpp              # HDSet TCP handler
│   ├── HTcpSocket.cpp               # TCP socket wrapper
│   ├── HThread.cpp                  # Thread abstraction
│   ├── HTicker.cpp                  # Timer/ticker
│   ├── HUdpSocket.cpp               # UDP socket wrapper
│   ├── HUnixSocket.cpp              # Unix domain socket
│   ├── HUserCtlManage.cpp           # User control
│   ├── IBoxPlayer.cpp               # BoxPlayer interface (abstract)
│   ├── IFileManager.cpp/.h          # File manager interface
│   ├── ISDeviceLocker.cpp           # Device lock interface
│   └── Tools.cpp                    # Misc utilities
│
├── FPGADriver/                      # FPGA driver (libFPGADriver.so)
│   ├── Config/
│   │   └── ConfigParam.cpp          # FPGA configuration parameters
│   ├── Core/
│   │   ├── Card.cpp                 # Base card class
│   │   ├── ErrorCode.cpp            # FPGA error codes
│   │   ├── FPGAParam.cpp            # FPGA parameters
│   │   ├── HFPGACore.cpp            # Core FPGA communication
│   │   ├── HFPGACoreHelper.cpp      # FPGA helper utilities
│   │   ├── HFPGAMsgList.cpp         # FPGA message queue
│   │   ├── ICard.cpp                # Card interface
│   │   ├── IObject.h                # Object interface
│   │   ├── RecvCard.cpp             # Receiving card driver
│   │   ├── SendCard.cpp             # Send card driver
│   │   ├── Status.cpp               # FPGA status monitoring
│   │   ├── SystemEnv.cpp            # FPGA system environment
│   │   ├── Task.cpp                 # FPGA task scheduler
│   │   └── Tool.cpp                 # FPGA tools
│   ├── Data/
│   │   ├── HFPGAData.cpp            # FPGA data structures
│   │   ├── HHwSetInfo.cpp           # Hardware settings
│   │   └── HParamConvert.cpp        # Parameter conversion
│   ├── HFPGAMonitor.cpp             # FPGA health monitoring
│   ├── Interface/
│   │   └── InterfcFPGA.cpp          # FPGA interface layer
│   ├── Trans/
│   │   ├── FPGASerial.cpp           # FPGA serial transport
│   │   └── Serial.cpp               # Raw serial I/O
│   └── Upgrade/
│       ├── EraseRecvCard.cpp         # Erase receiving card
│       ├── EraseSendCard.cpp         # Erase send card
│       ├── ReadBackRecvCard.cpp      # Readback from recv card
│       ├── ReadBackSendCard.cpp      # Readback from send card
│       ├── SendRecvCardFireware.cpp  # Flash recv card firmware
│       ├── SendSendCardFireware.cpp  # Flash send card firmware
│       └── Upgrade.cpp              # Upgrade orchestration
│
├── Program/                          # Process management
│   ├── HDaemonClient.cpp            # Daemon communication
│   ├── HLocalTcpServer.cpp          # Local TCP server
│   ├── HLocalUdpServer.cpp          # Local UDP server
│   ├── HMainLoop.cpp                # Main event loop
│   └── HSignal.cpp                  # Unix signal handling
│
├── SDKServices/                     # SDK services (libSDKServices.so)
│   ├── Convert/
│   │   ├── HAllParamConvert.cpp     # Parameter format conversion
│   │   ├── HCntLight.cpp            # Light control conversion
│   │   ├── HCntSwitchScreen.cpp     # Screen switch conversion
│   │   └── HCntTime.cpp             # Time format conversion
│   ├── File/
│   │   ├── HFileManager.cpp         # File transfer management
│   │   ├── HHttpDownload.cpp        # HTTP file download
│   │   ├── HHttpUpload.cpp          # HTTP file upload
│   │   └── HWriteFile.cpp           # Safe file writing
│   ├── Peripherals/
│   │   ├── DataSources/
│   │   │   ├── HBaseDataSource.cpp      # Base data source
│   │   │   ├── HDataSourceConfig.cpp    # Data source config
│   │   │   ├── HLoaclDataSource.cpp     # Local data source
│   │   │   ├── HModbusDataSource.cpp    # Modbus data source
│   │   │   ├── HNetWorkDataSource.cpp   # Network data source
│   │   │   └── HSerialPortManager.cpp   # Serial port manager
│   │   ├── GPS/
│   │   │   └── IGPS.cpp                 # GPS interface
│   │   ├── NetworkManager/
│   │   │   ├── Ethernet.cpp             # Ethernet control
│   │   │   ├── HNeowayN720.cpp          # 4G modem driver
│   │   │   ├── HResetWifi.cpp           # WiFi reset
│   │   │   ├── INetwork.cpp             # Network interface
│   │   │   ├── LedSignal.cpp            # LED signal indicator
│   │   │   ├── ModuleAT.cpp             # AT command handler
│   │   │   ├── ModuleDynamicInfo.cpp    # Module info
│   │   │   ├── NetWorkManager.cpp       # Network manager
│   │   │   ├── PPPD.cpp/.h              # PPP daemon
│   │   │   ├── Pppoe.cpp               # PPPoE
│   │   │   ├── PppoeManager.cpp         # PPPoE manager
│   │   │   ├── Quectel_EC20.cpp         # Quectel EC20 modem
│   │   │   ├── Quectel_EC200A.cpp       # Quectel EC200A modem
│   │   │   ├── Quectel_EC200S.cpp       # Quectel EC200S modem
│   │   │   ├── Quectel_EC200T.cpp       # Quectel EC200T modem
│   │   │   ├── Quectel_EC20F.cpp        # Quectel EC20F modem
│   │   │   ├── Quectel_UC20.cpp         # Quectel UC20 modem
│   │   │   ├── ResetPppoeModule.cpp     # PPPoE module reset
│   │   │   ├── Simcom_7600.cpp          # SIMCom 7600 modem
│   │   │   ├── Wifi.cpp                 # WiFi management
│   │   │   ├── Wireless.cpp             # Wireless utilities
│   │   │   ├── ZTE_MC2716.cpp           # ZTE MC2716 modem
│   │   │   ├── ZTE_MF226.cpp            # ZTE MF226 modem
│   │   │   └── ZTE_ZM8620.cpp           # ZTE ZM8620 modem
│   │   ├── HAllocEth0IP.cpp         # IP allocation
│   │   ├── HButton.cpp              # Button input
│   │   ├── HButtonEvent.cpp         # Button events
│   │   ├── HDeviceManager.cpp       # Device manager
│   │   ├── HEth0Event.cpp           # Ethernet events
│   │   ├── HHDSensor.cpp            # HD sensor interface
│   │   ├── HMasterRF.cpp            # RF master
│   │   ├── HNtpdate.cpp             # NTP time sync
│   │   ├── HOnboardRelay.cpp        # Onboard relay control
│   │   ├── HRFAdjust.cpp            # RF adjustment
│   │   ├── HSSensorSerialAnalyze.cpp # Sensor serial protocol
│   │   ├── HSensorUpgrade.cpp       # Sensor firmware upgrade
│   │   ├── HSerial.cpp/.h           # Serial port abstraction
│   │   ├── HSerialServices.cpp      # Serial service layer
│   │   ├── HSlaveRF.cpp             # RF slave
│   │   ├── HTeleController.cpp      # Remote control
│   │   ├── HUDiskEvent.cpp          # USB disk detection
│   │   ├── HUSBIDEvent.cpp          # USB ID events
│   │   ├── HUSBTools.cpp/.h         # USB utilities
│   │   └── ISensor.cpp              # Sensor interface
│   ├── HAsyncDelayManager.cpp       # Async delay execution
│   ├── HConfigSetting.cpp           # Config persistence
│   ├── HCurlServer.cpp              # libcurl HTTP server
│   ├── HDSetAppService.cpp          # HDSet app service
│   ├── HGPSHelper.cpp               # GPS helper
│   ├── HHDSetHot.cpp                # HDSet hotplug
│   ├── HPermanentConfig.cpp         # Persistent config
│   ├── HProgramHelper.cpp           # Program helper utilities
│   ├── HSAdminMode.cpp              # Admin mode
│   ├── HSBootLogo.cpp               # Boot logo management
│   ├── HSBusStationProgramList.cpp  # Bus station program lists
│   ├── HSCustomResoulution.cpp      # Custom resolution
│   ├── HSDataSource.cpp             # Data source management
│   ├── HSDeviceLocker.cpp           # Device locking
│   ├── HSFont.cpp                   # Font management
│   ├── HSFuncEnable.cpp             # Feature toggles
│   ├── HSHDMIPlan.cpp               # HDMI input plan
│   ├── HSHwSet.cpp                  # Hardware settings (FPGA config)
│   ├── HSIcon.cpp                   # Status icons
│   ├── HSLicense.cpp                # License management
│   ├── HSLight.cpp                  # Brightness control
│   ├── HSModbus.cpp                 # Modbus management
│   ├── HSNetWorkData.cpp            # Network data fetching
│   ├── HSOther.cpp                  # Miscellaneous
│   ├── HSProgramCounts.cpp          # Program play count tracking
│   ├── HSReboot.cpp                 # Reboot management
│   ├── HSScreenOnoff.cpp            # Screen on/off scheduling
│   ├── HSSensorsExtend.cpp          # Extended sensor support
│   ├── HSSerialSDK.cpp              # Serial SDK protocol
│   ├── HSTTSConv.cpp                # Text-to-speech
│   ├── HSTime.cpp                   # Time management
│   ├── HSUpgrade.cpp                # Upgrade management
│   ├── HSUsbDevice.cpp              # USB device management
│   ├── HSWebExtraSupportFuncManage.cpp # Web extra functions
│   ├── HServicesManager.cpp         # Service orchestration
│   ├── HStatusManager.cpp           # Status reporting
│   ├── HTcpModbusClient.cpp         # Modbus TCP client
│   ├── HTcpSDKClient.cpp            # SDK TCP client
│   ├── hsrelay.cpp                  # Relay management
│   └── hsvolume.cpp                 # Volume control
│
├── XmlSDK/                          # XML protocol layer (libXmlSDK.so)
│   ├── Core/
│   │   ├── HModuleManager.cpp       # Module registry
│   │   └── IMethod.cpp              # Method dispatch interface
│   ├── HFileServices.cpp            # File services
│   ├── HSDKCmdManager.cpp           # SDK command manager
│   ├── HSDKCmdServices.cpp          # SDK command services
│   ├── HSerialSDKSession.cpp        # Serial SDK session
│   ├── HTcpSession.cpp              # TCP session handling
│   ├── HXmlSession.cpp              # XML session parser
│   ├── HnBoxIOServices.cpp          # BoxIO service binding
│   ├── HnBoxPlayer.cpp              # BoxPlayer binding
│   ├── HpSDKBasic.cpp               # Basic SDK protocol
│   ├── HpSDKIOServices.cpp          # I/O SDK protocol
│   ├── HpSDKPlayer.cpp              # Player SDK protocol
│   └── Module/                      # SDK command modules
│       ├── HMBootLogo.cpp           # Boot logo commands
│       ├── HMEthernet.cpp           # Ethernet commands
│       ├── HMFont.cpp               # Font commands
│       ├── HMFuncEnable.cpp         # Feature flag commands
│       ├── HMGeneral.cpp            # General commands
│       ├── HMHwSet.cpp              # Hardware settings commands
│       ├── HMLicense.cpp            # License commands
│       ├── HMLight.cpp              # Brightness commands
│       ├── HMMediaFile.cpp          # Media file commands
│       ├── HMOFFOrONSpeed.cpp       # Screen transition speed
│       ├── HMOther.cpp              # Other commands
│       ├── HMPlayList.cpp           # Playlist commands
│       ├── HMProgram.cpp            # Program commands
│       ├── HMScreenOnoff.cpp        # Screen schedule commands
│       ├── HMScreenTest.cpp         # Screen test commands
│       ├── HMSensor.cpp             # Sensor commands
│       ├── HMTime.cpp               # Time commands
│       ├── HMUpgrade.cpp            # Upgrade commands
│       └── HMUsbDevice.cpp          # USB device commands
│
├── hcommon/                          # Shared rendering (libhcommon.so)
│   ├── HGifReader.cpp               # GIF file parser
│   ├── HHDMIInPlayer.cpp            # HDMI input player
│   ├── HPlayListHelper.cpp          # Playlist management
│   ├── HScreenTestPlayer.cpp        # Screen test patterns
│   ├── PatchProject.cpp             # Project file patching
│   ├── SDKEncodeDecode.cpp          # SDK encoding/decoding
│   ├── androidplayer.cpp            # Android player wrapper
│   ├── effect_node.cpp              # Effect rendering
│   ├── effecttext_node.cpp          # Text effects
│   ├── hcommonfuc.cpp               # Common functions
│   ├── hgifplay.cpp                 # GIF playback
│   ├── l_screentesting.cpp          # Screen testing
│   ├── modifyprogram.cpp            # Program modification
│   ├── playerfactory.cpp            # Plugin factory
│   ├── scene_node.cpp               # Scene tree node
│   └── tick.cpp                     # Animation timer
│
└── MainWindow/                       # Rendering (libMainWindow.so + libMainWindowRender.so)
    ├── HProgramSession               # Program lifecycle management
    ├── HAreaSession                  # Area/zone rendering
    ├── HPictureSession               # Image compositing
    ├── HImageSurface                 # Render surface
    ├── HDoubleImage                  # Double-buffered rendering
    ├── HFrame / HNodeAttr            # Frame effects
    └── HVertexs / HRectangle        # Geometry primitives
```

---

## Key Technical Details

### Startup Sequence (PX30 Linux)
```
1. BoxPlayerInit.sh
   → Sets GPU to 480MHz, CPU to 1.2GHz
   → Clears FPGA, drops caches
   → Starts BoxDaemon (watchdog)
   → Shows BootLogo
   → Runs BoxUpgrade check
   → Starts run.sh → runBoxPlayer.sh + runBoxSDK.sh
   → Starts cn.huidu.device.api (Go REST API)

2. runBoxPlayer.sh
   → export LD_LIBRARY_PATH=/root/Box/BoxPlayer
   → BoxPlayer -platform offscreen
   (Qt offscreen rendering - no display server needed)

3. runBoxSDK.sh
   → BoxSDK kDebug -platform offscreen
   (Network protocol server)
```

### FPGA Communication
- Device files: `/dev/cyclone4-0`, `/dev/cyclone4-1` (Altera/Intel Cyclone IV FPGA)
- Protocol: Custom serial protocol over SPI/UART to FPGA
- Data structures: `fpga::HSendCard`, `fpga::HRecvCard`, scan tables, gamma, color correction
- Key parameters: `s_dualScanTab`, `s_grayPriority`, `s_lightPriority`, `s_refreshPriority`, `s_scanParam`

### Network Protocol (XML-based)
- HDPlayer PC software connects via TCP
- Commands use XML format parsed by `HXmlSession`
- Method dispatch via `IMethod` / `HInterfaceManager`
- Commands include: `MethodCall`, `GetSetting`, `BoxHwConfig`, etc.
- Program files transferred via `HFileManager`
- Cloud API endpoints: `/api/DeviceApi/Register`, `/api/DeviceApi/Heartbeat`, `/api/DeviceApi/ReportProgram`

### Program File Format
- Programs are XML-based, stored in `/cache/hdTempProgram/`
- Config files use `.boo` extension (`config.boo`, `program.boo`)
- Scene tree: Program → Scenes → Areas → Content Items
- Each area has: position, size, rotation, z-order
- Content types map to plugins (video, photo, text, clock, etc.)
- Media files stored alongside program XML

### Device Type IDs
```
A601=0x02  A602=0x04  A603=0x06  D3=0x08   C1=0x0a
C3=0x0c    D1=0x0e    A10=0x10   A30=0x12  A10+=0x14
A30+=0x16  C10=0x18   C30=0x1a   D10=0x1c  D20=0x20
C30+=0x22  C10+=0x24  A3=0x26    C15=0x28  C35=0x2a
A6=0x2c    V10=0x30   V02=0x32   D15=0x34  D35=0x36
B6=0x38    V15=0x3e   D68=0x40   C16=0x42  C36=0x44
QF4=0x46   D16=0x48   D36=0x4a   D18=0x4c  V16=0x4e
A4=0x50    A5=0x52    C16L=0x54  C08L=0x56 C08L_WIFI=0x58
```

---

## Plugin System

Plugins are loaded dynamically via `QPluginLoader` in `PlayerFactory`. Each plugin is a `.so` that implements a rendering interface:

| Plugin | Identified String | Purpose |
|--------|------------------|---------|
| `libordinary_scene_plugin.so` | `HD_OrdinaryScene_Plugin` | Scene playback |
| `libframe_plugin.so` | `HD_Frame_Plugin` | Frame/border effects |
| `libphoto_plugin.so` | `pictureplayer` | Image display |
| `libvideo_plugin.so` | - | Video playback (uses gstreamer on PX30) |
| `libtext_plugin.so` | - | Static text rendering |
| `libsinglelinetext_plugin.so` | - | Single-line scrolling text |
| `libanimationText_plugin.so` | - | Animated text effects |
| `libclock_plugin.so` | - | Analog/digital clock |
| `libCalendar_plugin.so` | - | Calendar widget |
| `libtime_plugin.so` | - | Time display |
| `libweather_plugin.so` | - | Weather data display |
| `libtemperatures_plugin.so` | - | Temperature sensor |
| `libhumidity_plugin.so` | - | Humidity sensor |
| `libneon_plugin.so` | - | Neon/LED border effects |
| `libDynamicData_plugin.so` | - | Dynamic data binding |
| `libNetworkData_plugin.so` | - | Network data fetching |
| `libmodbus_plugin.so` | - | Modbus data display |
| `libsensor_plugin.so` | - | Generic sensor display |
| `libdocument_plugin.so` | - | Document rendering |
| `libwps_plugin.so` | - | WPS document support |
| `libweb_plugin.so` | - | Web content (Chromium) |
| `libhdmiin_plugin.so` | - | HDMI input capture |
| `libscreen_plugin.so` | - | Screen management |
| `libtable_plugin.so` | - | Table/grid display |
| `librdm_plugin.so` | - | RDM protocol data |
| `libewatch_plugin.so` | - | E-watch widget |
| `libtest3d_plugin.so` | - | 3D test rendering |

---

## Key Classes & Their Roles

### Rendering Pipeline
| Class | Library | Role |
|-------|---------|------|
| `Hd::HSceneNode` | libhcommon | Scene tree node - manages child areas |
| `Hd::HVirtualSceneNode` | libhcommon | Virtual scene node for compositing |
| `Hd::PlayerFactory` | libhcommon | Creates content player instances by type |
| `Hd::IPlayer` | libhcommon | Abstract content player interface |
| `Hd::AndroidPlayer` | libhcommon | Android-specific player wrapper |
| `Hd::HFrame` / `Hd::HFrameNode` | libhcommon | Frame/border effect rendering |
| `Hd::DrawNeonBg` | libhcommon | Neon background effect renderer |
| `HAreaSession` | libMainWindow | Individual area/zone renderer |
| `HProgramSession` | libMainWindow | Program lifecycle manager |
| `HPictureSession` | libMainWindowRender | Image compositing |
| `HDoubleImage` | libMainWindowRender | Double-buffered pixel output |
| `HImageSurface` | libMainWindowRender | Render surface abstraction |
| `ImageConverter` | libhcommon | Image format conversion |
| `HGifPlay` / `HGifReader` | libhcommon | GIF animation player |

### Network/Protocol
| Class | Library | Role |
|-------|---------|------|
| `old::HTcpServer` | libBoxIOServices | TCP server accepting HDPlayer connections |
| `sdk::HIPCServer` | libCore | IPC server (Unix sockets between BoxSDK↔BoxPlayer) |
| `sdk::HIPCClient` | libCore | IPC client |
| `sdk::HTcpSocket` | libCore | TCP socket wrapper |
| `sdk::HUdpSocket` | libCore | UDP socket wrapper |
| `sdk::HEventManager` | libCore | Event loop / select() |
| `sdk::HTcpSDKClient` | libSDKServices | TCP SDK client |
| `sdk::HTcpModbusClient` | libSDKServices | Modbus TCP client |
| `sdk::HServicesManager` | libSDKServices | Service orchestrator |
| `sdk::HXmlSession` | libXmlSDK | XML protocol parser |
| `old::HInterfaceManager` | libBoxIOServices | Method dispatch/routing |

### FPGA/Hardware
| Class | Library | Role |
|-------|---------|------|
| `fpga::HFPGACore` | libFPGADriver | Core FPGA communication |
| `fpga::HFPGASerial` | libFPGADriver | Serial transport to FPGA chip |
| `fpga::HSendCard` | libFPGADriver | Send card parameter management |
| `fpga::HRecvCard` | libFPGADriver | Receiving card parameter management |
| `fpga::HTask` | libFPGADriver | FPGA task scheduling |
| `fpga::HUpgrade` | libFPGADriver | FPGA firmware upgrade |
| `fpga::HStatus` | libFPGADriver | FPGA health/status monitoring (states: kIdleStatus, kEnterWizard, kUpgradeFPGA, kEnterRC) |
| `sdk::HFPGAMonitor` | libSDKServices | FPGA monitoring service |

---

## GPU Rendering Pipeline (libMainWindowRender.so)

The PX30 version uses **DRM/KMS direct rendering** with OpenGL ES 2.0 - NOT software Qt rendering:

```
1. Opens /dev/dri/card0 (DRM device)
2. Creates GBM (Generic Buffer Manager) surface
3. EGL context bound to GBM surface
4. Custom GLSL vertex/fragment shaders:
   - Texture mapping (s_texture1, s_texture2, s_textureIndex)
   - UV coordinate transforms for area positioning
   - Dazzle effects (transition animations)
   - Screen rotation (0/90/180/270)
   - HSV-to-RGB color conversion in shader
   - Alpha blending for layer compositing
5. glReadPixels for screenshots
6. Page flip to DRM framebuffer
7. FPGA reads pixel data from framebuffer
```

**Key Rendering Classes:**
- `HRenderEngine` - Main render loop with async message queue
- `HRenderTools` - EGL context management, shader compilation, texture management
- `HRenderData` - Program/scene state for GPU
- `HMessageQueue` - Async render command queue
- `HPictureSession` - Image layer rendering
- `HAreaSession` - Area region compositing
- Effects: `CenterEffect`, `DoorEffect` transitions

**Render thread runs at max scheduler priority.**

---

## SDK XML Protocol (Complete Method Reference)

### Protocol Format
```xml
<!-- Request from HDPlayer -->
<?xml version="1.0" encoding="utf-8"?>
<sdk guid="##GUID">
    <in method="MethodName">
        <param attr="value"/>
    </in>
</sdk>

<!-- Response from BoxSDK -->
<?xml version="1.0" encoding="utf-8"?>
<sdk guid="##GUID">
    <out method="MethodName" result="kSuccess">
        <data/>
    </out>
</sdk>
```

### Complete SDK Method List

#### HMGeneral (General Device)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetDeviceInfo` | GET | Device model, version, screen size |
| `GetDeviceName` | GET | User-assigned device name |
| `SetDeviceName` | SET | Change device name |
| `GetHardwareInfo` | GET | CPU, RAM, storage info |
| `GetSDKTcpServer` | GET | SDK server port/config |
| `SetSDKTcpServer` | SET | Change SDK server config |
| `GetAdminModeInfo` | GET | Admin mode status |
| `SetAdminModeInfo` | SET | Enable/disable admin mode |
| `UnlockAdminModePassword` | SET | Unlock with password |
| `GetScreenshot2` | GET | Capture current display |
| `GetSystemVolume` | GET | Audio volume |
| `SetSystemVolume` | SET | Change volume |
| `GetDataSourceInfo` | GET | External data source config |
| `SetDataSourceInfo` | SET | Configure data sources |
| `ReloadDeviceID` | SET | Reload device ID from storage |

#### HMProgram (Program Management)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetAllProgram` | GET | List all programs |
| `GetProgram` | GET | Get specific program |
| `AddProgram` | SET | Upload new program |
| `UpdateProgram` | SET | Update existing program |
| `DeleteProgram` | SET | Delete program |
| `SwitchProgram` | SET | Switch active program |
| `RealTimeUpdate` | SET | Live update content |
| `InsertPlayProgram` | SET | Insert priority program |
| `ScreenRotation` | SET | Rotate screen (0/90/180/270) |
| `GetCurrentPlayProgramGUID` | GET | Currently playing program |
| `ModifyProgram` | SET | Modify program in-place |
| `DeleteNotCiteFile` | SET | Clean unreferenced files |

#### HMHwSet (FPGA Hardware Settings)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetSDKFPGAConfig` | GET | Read FPGA configuration |
| `SetSDKFPGAConfig` | SET | Write FPGA configuration |
| `GetBoxHwConfig` | GET | Read hardware config XML |
| `SetBoxHwConfig` | SET | Write hardware config |
| `SaveBoxHwConfig` | SET | Persist to flash |
| `ReplaceBoxHwConfig` | SET | Full replacement |
| `SmartSetting` | SET | Auto-configure for module type |
| `SmartDrawLine` | SET | Smart line drawing test |

#### HMLight (Brightness)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetLuminancePloy` | GET | Brightness schedule/policy |
| `SetLuminancePloy` | SET | Set brightness schedule |

#### HMScreenOnoff (Screen Schedule)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetSwitchTime` | GET | On/off schedule |
| `SetSwitchTime` | SET | Set on/off schedule |
| `OpenScreen` | SET | Turn on immediately |
| `CloseScreen` | SET | Turn off immediately |

#### HMTime (Time Sync)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetTimeInfo` | GET | Current time, timezone, NTP |
| `SetTimeInfo` | SET | Set time, timezone, NTP server |

#### HMEthernet (Network)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetEth0Info` | GET | Ethernet IP/mask/gateway |
| `SetEth0Info` | SET | Configure ethernet |
| `GetPppoeInfo` | GET | PPPoE status |
| `GetWifiInfo` | GET | WiFi configuration |
| `SetWifiInfo` | SET | Configure WiFi |
| `GetNetworkInfo` | GET | Full network status |

#### HMSensor (Sensors/Modbus)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetSensorInfo` | GET | Sensor configuration |
| `GetCurrentSensorValue` | GET | Live sensor readings |
| `GetGPSInfo` | GET | GPS coordinates |
| `GetRelayInfo` | GET | Relay states |
| `SetRelayInfo` | SET | Configure relays |
| `SetRelayStatusInfo` | SET | Toggle relay |
| `GetSerialSDK` | GET | Serial SDK config |
| `SetSerialSDK` | SET | Configure serial SDK |

#### HMLicense (Licensing)
| Method | Direction | Description |
|--------|-----------|-------------|
| `GetLicense` | GET | Current license |
| `SetLicense` | SET | Apply license |
| `ClearLicense` | SET | Remove license |
| `CheckSuperCode` | SET | Validate super code |

#### HMUpgrade (Firmware)
| Method | Direction | Description |
|--------|-----------|-------------|
| `FirmwareUpgrade` | SET | Start OTA upgrade |
| `ExcuteUpgradeShell` | SET | Run upgrade script |
| `GetUpgradeResult` | GET | Upgrade status |

---

## BoxHwConfig XML Tags (Complete FPGA Configuration)

These XML tags define the complete LED panel hardware configuration:

```
BoxHwConfig, CardInfo, Card, ModuleType, DriveChipType,
CellWidth, CellHight, CellScanRow, ScanMode, MoreThan16Scan,
ESignal, Chip595, Chip5958, DecodingMode, DataPolarity, OEPolarity,
SignalColor, CellNullNum, LookUpTab, RefreshRate, R_Acc,
GrayLevel, LuminanceLevel, Frequency, PriorityMode,
RGB20, DSignalExtended, RGB24, RGB28, RGB32,
GamaValue, RedCorrection, GreenCorrection, BlueCorrection,
DutyCycle, BV, Phase, Afterglow, OM,
PwmRedCurrent, PwmGreenCurrent, PwmBlueCurrent,
SPWMMode, FMPWMMultiplier, DoubleRefreshRate, GCLKMultiplier,
PwmFrequency, F_Frame, Brightness,
NetcardCtrlRect, ModeSwitchPlan, Rotation,
SendCardMode, NetCardCtrlMode, AsyncPriorityMode, EnSendcardOnly,
RecvCardChoose, RgbCtrlWidth, RgbCtrlHeight, BrightnessSetMode,
UDefGamaList, GamaTab, EnOutputDefinition, OutputDefinition,
PwmChipType, PWMICRedReg1-3, PWMICGreenReg1-3, PWMICBlueReg1-3
```

---

## Cloud Platform API Endpoints

| Endpoint | Purpose |
|----------|---------|
| `/api/DeviceApi/Register` | Device registration |
| `/api/DeviceApi/Heartbeat` | JSON heartbeat with status |
| `/api/DeviceApi/ReportProgram` | Report playing program |
| `/api/DeviceApi/ReportAllInfo` | Full device info report |
| `/api/DeviceApi/ReportTaskResult` | Task execution result |
| `/api/DeviceApi/ReportGpsInfo` | GPS location data |
| `/api/DeviceApi/HistorySensor` | Historical sensor data |
| `/api/DeviceApi/upload` | File upload |
| `/api/led/hdSet/change/report` | HDSet config change notification |

**Remote Access:** ngrok tunnel to `ngrok1.huidu.cn:4443` (port 29001 → local telnet)

---

## Key File Paths on Device

| Path | Purpose |
|------|---------|
| `/root/Box/` | Main installation directory |
| `/root/Box/BoxPlayer/` | Player binaries and libraries |
| `/root/Box/BoxPlayer/plugins/` | Content rendering plugins |
| `/root/Box/System/` | System binaries |
| `/root/Box/SystemConfig/` | System config (process_info.xml, dev_type) |
| `/root/Box/config/` | Device config (dev_info.xml, hwsetting/, send_card_cfg.xml) |
| `/root/Box/data/` | Runtime data (id, permanentConfig.xml, license.ini) |
| `/root/Box/data/id` | Device ID (format: `D15-XXXXXX`) |
| `/root/Box/project/` | Program files, logs, API |
| `/root/Box/version/` | Version tracking files |
| `/root/Box/image/` | Status icons and clock faces |
| `/root/Box/lib/` | Qt libraries, fonts |
| `/boot/fpga.img` | FPGA firmware image |
| `/boot/httpApi` | HTTP API enable flag |
| `/boot/omsEnable` | OMS cloud service enable flag |
| `/boot/rotation/` | Screen rotation config (0-3) |
| `/dev/ttyS1` | FPGA serial communication port |
| `/dev/watchdog` | Hardware watchdog |
| `/dev/dri/card0` | DRM graphics device |
| `/mnt/usb_storage` | USB disk mount point |
| `/root/upgrade.status` | Upgrade status (0=upgrading, 1=complete) |

---

## Supported Media Types
- **Video:** mp4, mkv, avi, flv, 3gp, dat, ts, mpg, f4v
- **Image:** jpg, jpeg, png, bmp, tiff, gif
- **Playback modes:** Normal, Sync, 12V (vehicle power), Immediate (priority), Bus station, GPS-triggered
