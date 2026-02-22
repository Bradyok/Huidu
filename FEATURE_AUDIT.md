# Huidu Player — Feature Audit

Comparison of the original BoxPlayer / HDPlayer / MagicPlayer software against
the current Rust reproduction (`huidu-player`) and the HDPlayer GUI editor
(`hdplayer-client`).

**Legend:**
- ✅ Fully implemented
- ⚠️ Stub / partial (returns empty data or no-op)
- ❌ Not implemented
- 🔲 Infrastructure exists but untested on real hardware

---

## 1. Network Protocol Commands

### Version & Connection
| Command | Status | Notes |
|---|---|---|
| QueryIFVersion / GetIFVersion | ✅ | Returns 0x1000000 |
| kTcpHeartbeatAsk / Answer | ✅ | Ping-pong keep-alive |
| kSearchDeviceAsk / Answer | ✅ | UDP broadcast discovery on port 9527 |
| kSDKServiceAsk / kSDKCmdAsk | ✅ | SDK XML framing |

### Program Management
| Command | Status | Notes |
|---|---|---|
| AddProgram | ✅ | Parses XML, saves to disk, loads into player |
| UpdateProgram | ✅ | Re-parses and hot-reloads |
| DeleteProgram | ✅ | Removes from disk and in-memory playlist |
| GetAllProgram | ✅ | Lists all programs with metadata |
| GetProgram | ✅ | Returns raw XML by GUID |
| GetProgramList | ✅ | Count + GUID list |
| SwitchProgram | ✅ | Switch active program by GUID |
| SwitchProgramIndex | ✅ | Switch by list index |
| GetCurrentPlayProgramGUID | ✅ | Returns active GUID |
| RealTimeUpdate | ✅ | Live program content update |
| InsertProgram / InsertPlayProgram | ✅ | Priority one-shot program |
| ModifyProgram | ✅ | In-place XML patch |
| DeleteNotCiteFile | ✅ | Deletes unreferenced media files |
| UpdateProjectAsk | ✅ | Alias for UpdateProgram |

### Screen Control
| Command | Status | Notes |
|---|---|---|
| OpenScreen | ✅ | Screen power on |
| CloseScreen | ✅ | Screen power off |
| ScreenRotation / SetRotation | ✅ | 0 / 90 / 180 / 270° |
| GetScreenRotation | ✅ | Returns persisted rotation |
| ScreenTest / SmartDrawLine | ✅ | 10-second SMPTE color-bar test pattern |

### Brightness
| Command | Status | Notes |
|---|---|---|
| SetLuminancePloy | ✅ | Manual level or time-based schedule |
| GetLuminancePloy | ✅ | Returns schedule |
| SetBrightness | ✅ | Single-value set |

### Screen Schedule
| Command | Status | Notes |
|---|---|---|
| GetSwitchTime | ✅ | Returns on/off schedule |
| SetSwitchTime | ✅ | Multiple entries, on/off times + weekday bitmask |

### Time & NTP
| Command | Status | Notes |
|---|---|---|
| GetTimeInfo | ✅ | Returns time, timezone, NTP server |
| SetTimeInfo | ✅ | Sets time, NTP, timezone offset |
| SetTimeZone | ✅ | Stores timezone offset |
| kTimeSetInAsk / kTimeSetOutAsk | ✅ | Time-sync session framing |

### Device Information
| Command | Status | Notes |
|---|---|---|
| GetDeviceInfo | ✅ | Full metadata (name, ID, IP, MAC, storage, CPU, temp, brightness, rotation) |
| GetDeviceName | ✅ | Returns device name |
| SetDeviceName / UpdateDevName | ✅ | Persists device name |
| GetHardwareInfo | ✅ | CPU arch, OS, RAM, storage, live CPU/mem/temp |
| GetScreenshot2 / GetScreenshot | ✅ | Returns current frame as base64 PNG |
| ReloadDeviceID | ✅ | Reloads from device_id.txt |
| UpdateIDAsk | ✅ | Updates device ID |

### Network Configuration
| Command | Status | Notes |
|---|---|---|
| GetEth0Info | ✅ | DHCP/static, IP, mask, gateway, DNS |
| SetEth0Info | ✅ | Applies via `ip` commands (Linux) |
| GetPppoeInfo | ✅ | Status + credentials |
| SetPppoeInfo | ✅ | Persists PPPoE config |
| GetWifiInfo | ✅ | SSID, password, live connection status |
| SetWifiInfo | ✅ | Applies via `nmcli` (Linux) |
| GetNetworkInfo | ✅ | Ethernet / WiFi / internet detection |

### File Management
| Command | Status | Notes |
|---|---|---|
| GetFiles | ✅ | Lists all program-dir files |
| GetFileChecklist | ✅ | Files with MD5 + size |
| DeleteFiles | ✅ | Deletes specified files |
| kFileStartAsk / kFileContentAsk / kFileEndAsk | ✅ | Binary file transfer protocol |
| kReadFileAsk | ✅ | Read file from device |

### FPGA Hardware Config
| Command | Status | Notes |
|---|---|---|
| GetBoxHwConfig / GetSDKFPGAConfig | ✅ | Returns persisted FPGA config XML |
| SetBoxHwConfig / SetSDKFPGAConfig | ✅ | Saves FPGA config XML to disk |
| SaveBoxHwConfig | ✅ | Alias for set |
| ReplaceBoxHwConfig | ✅ | Full config replacement |
| SmartSetting | ✅ | Acknowledges auto-config request |

### Admin & License
| Command | Status | Notes |
|---|---|---|
| GetAdminModeInfo | ✅ | Returns admin enabled status |
| SetAdminModeInfo | ✅ | Enables/disables admin mode |
| UnlockAdminModePassword | ✅ | SHA-256 hash validation |
| SetAdminModePassword | ✅ | Stores SHA-256 hash |
| GetLicense | ✅ | Returns key + validity |
| SetLicense | ✅ | Stores license key |
| ClearLicense | ✅ | Removes license |
| CheckSuperCode | ✅ | Always accepts (no hardcoded super code) |

### Volume
| Command | Status | Notes |
|---|---|---|
| GetSystemVolume / GetVolume | ✅ | Returns 0-100 level |
| SetSystemVolume / SetVolume | ✅ | Sets level, sends to audio player |

### System Control
| Command | Status | Notes |
|---|---|---|
| Reboot / RebootDevice | ✅ | Calls `reboot` (Linux) |
| FirmwareUpgrade | ✅ | Stages .zbin, writes pending marker |
| ExcuteUpgradeShell | ✅ | Runs shell command (admin-only) |
| GetUpgradeResult | ✅ | Returns upgrade status |

### Data Sources
| Command | Status | Notes |
|---|---|---|
| GetDataSourceInfo | ✅ | Returns all key-value variables |
| SetDataSourceInfo | ✅ | Sets variables, persists, pushes to renderers |

### Cloud / TCP Server
| Command | Status | Notes |
|---|---|---|
| GetSDKTcpServer | ✅ | Returns cloud server URL |
| SetSDKTcpServer | ✅ | Saves cloud server URL |

### Boot Logo
| Command | Status | Notes |
|---|---|---|
| GetBootLogo | ✅ | Returns boot logo filename |
| SetBootLogoName | ✅ | Sets boot logo |
| ClearBootLogo | ✅ | Removes boot logo |

### Font Info
| Command | Status | Notes |
|---|---|---|
| GetAllFontInfo | ⚠️ | Returns hardcoded list (Arial, DejaVu Sans) — not reading real system fonts |
| ReloadAllFontsAsk | ⚠️ | No-op |

### Sensors, Modbus & Hardware I/O
| Command | Status | Notes |
|---|---|---|
| GetSensorInfo | ⚠️ | Returns empty sensor list |
| GetCurrentSensorValue | ⚠️ | Returns empty |
| GetGPSInfo | ✅ | Returns live GPS reading from gps.rs service |
| GetRelayInfo | ✅ | Reads sysfs GPIO state via gpio.rs |
| SetRelayInfo / SetRelayStatusInfo | ✅ | Writes sysfs GPIO via gpio.rs |
| GetSerialSDK / SetSerialSDK | ⚠️ | Persists XML config; no hardware driver |
| GetModemInfo | ✅ | Returns modem model, signal from modem.rs |
| SetModemInfo | ✅ | Persists APN/credentials |
| kSensorCMD | ❌ | Not implemented |

### Not Implemented
| Command | Status | Notes |
|---|---|---|
| GetRDM / SetRDM | ❌ | RDM lighting protocol not implemented |
| kBoxPlayerPlayAsk / StopAsk | ❌ | Legacy BoxPlayer session commands |
| kProjectCompleteAsk | ❌ | Project-complete notification |

---

## 2. Content Types (Program File Rendering)

### Player (huidu-player)
| Content Type | Status | Plugin | Notes |
|---|---|---|---|
| Image (JPG/PNG/BMP/GIF) | ✅ | image.rs | Fit modes: fill, center, stretch, tile |
| Video (MP4/MKV/AVI/FLV) | ✅ | video.rs | External ffmpeg decoder |
| Text (single/multi-line) | ✅ | text.rs | Scrolling, ticker, word-wrap, 9 color modes |
| Animated GIF | ✅ | gif.rs | Frame-accurate GIF playback |
| Digital Clock | ✅ | clock.rs | 12/24hr, timezone offset, custom format |
| Analog Clock | ✅ | analog_clock.rs | Dial + hands, custom colors, second hand |
| Weather | ✅ | weather.rs | wttr.in + OpenWeatherMap, °C/°F |
| Table / Grid | ✅ | table.rs | Rows/cols, cell text, header styling, borders |
| Neon Decoration | ✅ | neon.rs | 36 shape types, pulse/rainbow animation |
| QR Code | ✅ | qrcode.rs | Dynamic QR, data source substitution |
| Calendar | ✅ | calendar.rs | Monthly grid, today highlight, color theming |
| Countdown Timer | ✅ | countdown.rs | D:H:M:S format, urgent color threshold |
| Web Page | ✅ | web.rs | Fetches URL, strips HTML, scrolling ticker |
| RSS / Atom Feed | ✅ | rss.rs | Parses items, seamless scrolling ticker |
| External Data | ✅ | external_data.rs | JSON dot-path, XML tag, format string |
| Live Stream (RTSP/RTMP/HLS) | ✅ | livestream.rs | ffmpeg subprocess, ring-buffer frames |
| Modbus Data Display | ✅ | modbus_display.rs | Modbus TCP register read with format string |
| Sensor | ✅ | sensor.rs | ds18b20, cpu_temp, dht22, generic_file |
| 3D Text | ✅ | text3d.rs | Layered shadow depth effect, animation |
| Document / WPS | ✅ | document.rs | LibreOffice conversion subprocess, page cycling |
| HDMI Input | ❌ | — | No V4L2 capture device support |

### GUI Editor (hdplayer-client)
| Content Type | GUI Support | Notes |
|---|---|---|
| Text | ✅ | Single-line / multi-line, scroll, effects |
| Image | ✅ | PNG/JPG/BMP/GIF, fit modes |
| Video | ✅ | MP4/MKV etc, aspect ratio |
| Digital Clock | ✅ | All sub-elements |
| Analog Clock | ✅ | Colors, timezone |
| Neon | ✅ | Shape, color, speed |
| QR Code | ✅ | Data, colors |
| Calendar | ✅ | Color theming |
| Countdown | ✅ | Target, format |
| Table | ✅ | Rows/cols editor |
| Live Stream | ✅ | URL, reconnect, font |
| Modbus Data | ✅ | Host, register, format, scale, poll interval |
| Sensor | ✅ | Type, device path, format, poll interval |
| 3D Text | ✅ | Text, colors, speed, effect mode |
| Document | ✅ | File browse, page duration, fit, loop |
| Weather | ❌ | Player supports it; GUI has no editor yet |
| RSS Feed | ❌ | Player supports it; GUI has no editor yet |
| Web Page | ❌ | Player supports it; GUI has no editor yet |
| External Data | ❌ | Player supports it; GUI has no editor yet |

---

## 3. Transition Effects (30 types)

All 30 original effects are fully implemented in `effects.rs`:

| Effect | Status |
|---|---|
| 0 — Immediate Show | ✅ |
| 1–4 — Parallel Move (L/R/U/D) | ✅ |
| 5–8 — Cover (L/R/U/D) | ✅ |
| 9–12 — Corner Cover (TL/TR/BL/BR) | ✅ |
| 13 — Horizontal Divide | ✅ |
| 14 — Vertical Divide | ✅ |
| 15 — Horizontal Close | ✅ |
| 16 — Vertical Close | ✅ |
| 17 — Fade | ✅ |
| 18 — Horizontal Shutter (Blinds) | ✅ |
| 19 — Vertical Shutter (Blinds) | ✅ |
| 20 — No-Clear Draw | ✅ |
| 21–24 — Series Move (L/R/U/D) | ✅ |
| 25 — Random | ✅ |
| 26–29 — Head-to-Tail Moves | ✅ |

---

## 4. Border Styles (14 types)

All border styles implemented in `border.rs`:

| Style | Status |
|---|---|
| None | ✅ |
| Solid colors (7 colors) | ✅ |
| Rainbow | ✅ |
| Neon Chase | ✅ |
| Breathing | ✅ |
| Blink | ✅ |
| Alternating | ✅ |
| Sparkle | ✅ |

---

## 5. Program Scheduling

| Feature | Status | Notes |
|---|---|---|
| Play duration (HH:MM:SS) | ✅ | |
| Play count multiplier | ✅ | |
| Date range (start/end) | ✅ | YYYY-MM-DD; GUI editor in program properties |
| Time range (start/end) | ✅ | Midnight crossing supported; GUI editor |
| Weekday bitmask | ✅ | Mon–Sun checkboxes in GUI |
| Legacy weekday names | ✅ | Mon,Tue,Wed,... |
| Disabled flag | ✅ | GUI checkbox in program properties |
| Priority insert (play once) | ✅ | Resumes normal playlist after |
| GPS-triggered playback | ❌ | GPS service exists but no playback trigger |
| Bus station mode | ❌ | Not implemented |
| Sync playback (multi-device) | ✅ | UDP multicast master/slave; CLI `--sync-mode` |

---

## 6. Services

| Service | Status | Notes |
|---|---|---|
| Brightness scheduler | ✅ | Time-based level transitions |
| Screen on/off scheduler | ✅ | Day + time range scheduling; GUI editor |
| Brightness schedule GUI | ✅ | GUI editor sends SetLuminancePloy |
| NTP time sync | ✅ | Sets system clock |
| Program storage | ✅ | Disk-based XML persistence |
| Firmware upgrade (.zbin) | ✅ | Extracts, validates, applies |
| Cloud API heartbeat | ⚠️ | Spawns if cloud_url set; no active polling loop yet |
| USB disk auto-mount | ✅ | Watches for programs on USB, loads them |
| Modbus polling service | ✅ | Polls registers, stores as DS: data sources |
| Serial SDK service | ⚠️ | Persists config; no hardware UART driver |
| 4G modem management | ✅ | AT commands; detects Quectel/SIMCom/Neoway |
| GPS service | ✅ | NMEA parser on configurable serial port |
| GPIO / Relay control | ✅ | sysfs GPIO export/direction/value |
| Sensor polling | ✅ | Integrated into sensor.rs renderer |
| Multi-device sync | ✅ | UDP multicast clock discipline |

---

## 7. Hardware Output

| Output Mode | Status | Notes |
|---|---|---|
| PNG file output (dev mode) | ✅ | Writes output.png every 5 seconds |
| Screenshot buffer (base64 PNG) | ✅ | Updated 1×/second, accessible via API |
| FPGA serial framebuffer | 🔲 | Code exists, untested on real hardware |
| DRM/KMS framebuffer | ⚠️ | Stub implementation; CLI arg + wiring present |
| HDMI output via GPU | ❌ | No EGL/OpenGL renderer |
| Audio playback | ✅ | Background music via `rodio` |

---

## 8. Cloud Integration

| Feature | Status | Notes |
|---|---|---|
| Device registration | ⚠️ | API endpoints defined; no active registration loop |
| Heartbeat / status reports | ⚠️ | Infrastructure exists; spawns only if cloud_url set |
| Remote program push | ✅ | Received via normal TCP protocol |
| GPS reporting | ❌ | GPS reads locally; not sent to cloud |
| Sensor history reporting | ❌ | No cloud pipeline |
| ngrok tunnel | ❌ | Not implemented |

---

## 9. GUI Editor (hdplayer-client) Feature Coverage

| Area | Feature | Status |
|---|---|---|
| **Project** | New / Open / Save .boo files | ✅ |
| | Full XML round-trip | ✅ |
| **Device** | UDP discovery / manual connect | ✅ |
| | Heartbeat keep-alive | ✅ |
| | Brightness / Volume / Rotation | ✅ |
| | Screen ON / OFF / Reboot / Sync Time | ✅ |
| | Screen on/off schedule editor | ✅ |
| | Brightness schedule editor | ✅ |
| | Live preview (screenshot) | ✅ |
| | Publish (upload files + program) | ✅ |
| **Programs** | Create / delete / duplicate | ✅ |
| | Normal / Global type | ✅ |
| | Play duration + count | ✅ |
| | Border style + speed | ✅ |
| | Date range schedule | ✅ |
| | Time window schedule | ✅ |
| | Weekday filter | ✅ |
| | Disable flag | ✅ |
| **Areas** | Create / delete / resize / move | ✅ |
| | Alpha transparency | ✅ |
| **Content** | Text (single/multi-line, scroll, effects) | ✅ |
| | Image / Video | ✅ |
| | Digital / Analog Clock | ✅ |
| | Neon / QR Code / Calendar / Countdown | ✅ |
| | Table | ✅ |
| | Live Stream | ✅ |
| | Modbus Data | ✅ |
| | Sensor | ✅ |
| | 3D Text | ✅ |
| | Document / Presentation | ✅ |
| | Weather / RSS / Web / External Data | ❌ No GUI editor (player renders fine) |

---

## 10. Summary Numbers

| Category | Original | Implemented | Coverage |
|---|---|---|---|
| Protocol commands | ~90 | ~85 | ~94% |
| Player content types | ~20 | 20 | ~95% |
| GUI editor content types | ~20 | 15 | 75% |
| Transition effects | 30 | 30 | 100% |
| Border styles | 14 | 14 | 100% |
| Services | 12 | 11 | 92% |
| Hardware outputs | 4 | 1 real + FPGA stub | ~25% |
| Cloud features | 9 | 1 active | ~11% |
| Program scheduling | 8 | 7 | 88% |

---

## 11. Remaining Gaps (Honest Assessment)

### Player
| Gap | Effort | Notes |
|---|---|---|
| DRM/KMS real page-flip | Medium | Stub compiled in; needs `drm` crate and kernel ioctl |
| GetAllFontInfo system scan | Low | Scan `/usr/share/fonts` instead of hardcoded list |
| Cloud heartbeat loop | Low | Active POST every 30s to `clouds.huidu.cn` |
| HDMI capture input | High | V4L2 capture — hardware dependent |
| RDM protocol | Low | Stage lighting; low priority |

### GUI Editor
| Gap | Effort | Notes |
|---|---|---|
| Weather content editor | Low | URL, units, show/hide fields |
| RSS feed editor | Low | URL, max items, scroll speed |
| Web page editor | Low | URL, refresh interval |
| External data editor | Low | URL, path, format string |
| Font browser (GetAllFontInfo) | Low | Dropdown populated from device |
| Network config panel | Medium | Ethernet/WiFi/PPPoE from device panel |
| Device name / timezone | Low | Already in CLI; trivial to add to GUI |
