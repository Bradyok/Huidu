# Rust BoxPlayer Reproduction Plan

## Goal
Replace the proprietary Huidu BoxPlayer C++/Qt application with a Rust implementation that:
1. Parses the same XML program format from HDPlayer
2. Renders content (images, video, text, clocks, etc.) to a framebuffer
3. Outputs pixel data to Huidu FPGA hardware via serial (`/dev/cyclone4-x`)
4. Accepts network commands via the same TCP/XML protocol
5. Runs on the same ARM Linux hardware (PX30 aarch64, RK3288 armv7)

---

## Phase 1: Core Framework

### 1.1 Project Setup
```
huidu-player/
├── Cargo.toml
├── src/
│   ├── main.rs                  # Entry point, CLI args
│   ├── lib.rs                   # Library root
│   ├── config/                  # Configuration
│   │   ├── mod.rs
│   │   ├── params.rs            # HConfigParam equivalent
│   │   ├── device_type.rs       # Device type IDs (A3, C15, D15, etc.)
│   │   └── system_env.rs        # HSystemEnv - paths, hardware detection
│   ├── core/                    # Core framework (replaces libCore.so)
│   │   ├── mod.rs
│   │   ├── event_loop.rs        # HEventManager - async event loop (tokio)
│   │   ├── ipc.rs               # HIPCServer/Client - Unix domain sockets
│   │   ├── tcp.rs               # HTcpSocket wrapper
│   │   ├── udp.rs               # HUdpSocket wrapper
│   │   ├── error.rs             # HErrorCode
│   │   ├── logging.rs           # HLog (use tracing crate)
│   │   └── shell.rs             # HShell - command execution
│   ├── protocol/                # Network protocol (replaces libXmlSDK.so + libBoxIOServices.so)
│   │   ├── mod.rs
│   │   ├── tcp_server.rs        # HTcpServer - accept HDPlayer connections
│   │   ├── xml_session.rs       # HXmlSession - parse XML commands
│   │   ├── command.rs           # Command types and dispatch
│   │   ├── method.rs            # IMethod interface trait
│   │   ├── file_transfer.rs     # HFileManager - receive program files
│   │   └── modules/             # Command handler modules
│   │       ├── mod.rs
│   │       ├── program.rs       # HMProgram - program management
│   │       ├── playlist.rs      # HMPlayList
│   │       ├── hw_set.rs        # HMHwSet - FPGA hardware settings
│   │       ├── light.rs         # HMLight - brightness
│   │       ├── screen_onoff.rs  # HMScreenOnoff - scheduling
│   │       ├── time.rs          # HMTime - NTP sync
│   │       ├── general.rs       # HMGeneral - device info
│   │       ├── ethernet.rs      # HMEthernet - network config
│   │       ├── license.rs       # HMLicense
│   │       ├── upgrade.rs       # HMUpgrade - OTA
│   │       └── screen_test.rs   # HMScreenTest
│   ├── program/                 # Program file parsing (replaces part of libXmlSDK.so)
│   │   ├── mod.rs
│   │   ├── parser.rs            # XML program file parser
│   │   ├── scene.rs             # Scene tree structure
│   │   ├── area.rs              # Area/zone definition
│   │   ├── playlist.rs          # Play schedule/mode
│   │   └── content.rs           # Content item types
│   ├── render/                  # Rendering engine (replaces libMainWindow + libMainWindowRender + libhcommon)
│   │   ├── mod.rs
│   │   ├── engine.rs            # Main render loop
│   │   ├── scene_node.rs        # HSceneNode - scene tree
│   │   ├── area_session.rs      # HAreaSession - area renderer
│   │   ├── program_session.rs   # HProgramSession - program lifecycle
│   │   ├── surface.rs           # HImageSurface - render target
│   │   ├── double_buffer.rs     # HDoubleImage - double buffering
│   │   ├── compositor.rs        # Layer compositing
│   │   ├── effects.rs           # Effect rendering (transitions, borders)
│   │   └── plugins/             # Content type renderers
│   │       ├── mod.rs
│   │       ├── plugin_trait.rs  # Plugin interface trait
│   │       ├── image.rs         # Photo/image display
│   │       ├── video.rs         # Video playback (gstreamer-rs)
│   │       ├── text.rs          # Static text rendering
│   │       ├── scrolling_text.rs # Single-line scrolling text
│   │       ├── animated_text.rs # Animated text effects
│   │       ├── clock.rs         # Analog/digital clock
│   │       ├── calendar.rs      # Calendar widget
│   │       ├── time_display.rs  # Time display
│   │       ├── weather.rs       # Weather widget
│   │       ├── temperature.rs   # Temperature sensor display
│   │       ├── humidity.rs      # Humidity sensor display
│   │       ├── neon.rs          # Neon/border effects
│   │       ├── frame.rs         # Frame effects
│   │       ├── gif.rs           # GIF animation
│   │       ├── table.rs         # Table/grid
│   │       ├── dynamic_data.rs  # Dynamic data binding
│   │       ├── network_data.rs  # Network data source
│   │       ├── modbus.rs        # Modbus data display
│   │       └── document.rs      # Document rendering
│   ├── fpga/                    # FPGA driver (replaces libFPGADriver.so)
│   │   ├── mod.rs
│   │   ├── core.rs              # HFPGACore - main FPGA comm
│   │   ├── serial.rs            # HFPGASerial - serial transport
│   │   ├── send_card.rs         # HSendCard
│   │   ├── recv_card.rs         # HRecvCard
│   │   ├── params.rs            # HFPGAParam - scan tables, gamma, etc.
│   │   ├── monitor.rs           # HFPGAMonitor - health check
│   │   ├── task.rs              # HTask - FPGA task queue
│   │   └── upgrade.rs           # HUpgrade - FPGA firmware flash
│   ├── services/                # SDK services (replaces libSDKServices.so)
│   │   ├── mod.rs
│   │   ├── manager.rs           # HServicesManager
│   │   ├── brightness.rs        # HSLight
│   │   ├── screen_schedule.rs   # HSScreenOnoff
│   │   ├── time_sync.rs         # HSTime + HNtpdate
│   │   ├── device_locker.rs     # HSDeviceLocker
│   │   ├── status.rs            # HStatusManager
│   │   ├── usb_disk.rs          # HUDiskEvent
│   │   ├── http_client.rs       # HHttpDownload/Upload (reqwest)
│   │   └── cloud_api.rs         # Cloud reporting endpoints
│   └── daemon/                  # Process management
│       ├── mod.rs
│       ├── watchdog.rs          # BoxDaemon equivalent
│       ├── boot_logo.rs         # BootLogo
│       └── upgrade.rs           # BoxUpgrade
```

### 1.2 Key Dependencies (Cargo.toml)
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }          # Async runtime
serde = { version = "1", features = ["derive"] }         # Serialization
quick-xml = "0.36"                                       # XML parsing (program files + protocol)
image = "0.25"                                           # Image loading/manipulation
tiny-skia = "0.11"                                       # 2D rendering (QPainter replacement)
fontdue = "0.9"                                          # Font rasterization
gstreamer = "0.23"                                       # Video playback
gstreamer-video = "0.23"                                 # Video frames
serialport = "4"                                         # Serial port (FPGA comm)
tracing = "0.1"                                          # Structured logging
tracing-subscriber = "0.3"                               # Log output
reqwest = { version = "0.12", features = ["json"] }      # HTTP client
nix = { version = "0.29", features = ["net", "fs"] }     # Unix APIs
clap = { version = "4", features = ["derive"] }          # CLI args
chrono = "0.4"                                           # Date/time
gif = "0.13"                                             # GIF decoding
notify = "7"                                             # File watching (USB disk)
thiserror = "2"                                          # Error types
anyhow = "1"                                             # Error handling
toml = "0.8"                                             # Config files
drm = "0.12"                                             # DRM/KMS framebuffer
gbm = "0.16"                                             # GBM buffer management
# glow = "0.14"                                          # OpenGL ES 2.0 (if GPU rendering)
mlua = { version = "0.10", features = ["lua51"] }        # Lua 5.1 (hwSetting.lua compat)
ffmpeg-next = "7"                                        # FFmpeg video decoding
```

---

## Phase 2: Implementation Priority

### Milestone 1 - Minimum Viable Player
**Goal: Display a static image on LED panel**

1. **FPGA Serial Driver** (`fpga/serial.rs`, `fpga/core.rs`)
   - Open `/dev/ttyS1` (PX30) or `/dev/cyclone4-0` (RK3288)
   - Implement serial protocol (reverse-engineer from binary analysis)
   - Send raw pixel data to FPGA
   - Read FPGA status/version

2. **DRM/KMS Framebuffer Renderer** (`render/engine.rs`, `render/surface.rs`)
   - Open `/dev/dri/card0` via DRM/KMS (use `drm-rs` crate)
   - Create GBM surface with EGL context
   - OpenGL ES 2.0 rendering via `glow` or `wgpu`
   - Custom shaders for texture mapping, rotation, alpha blending
   - Page flip to DRM framebuffer
   - Alternatively: `tiny-skia` for CPU software rendering (simpler, no GPU dependency)

3. **Image Plugin** (`render/plugins/image.rs`)
   - Load PNG/JPG/BMP
   - Scale to display resolution
   - Output to framebuffer

### Milestone 2 - Program Playback
**Goal: Parse and play HDPlayer programs**

4. **Program Parser** (`program/parser.rs`)
   - Parse XML program files from HDPlayer
   - Build scene tree (programs → scenes → areas)
   - Handle area positioning, sizing, z-order

5. **Scene Compositor** (`render/compositor.rs`)
   - Composite multiple areas onto framebuffer
   - Handle z-ordering and clipping
   - Double-buffered output

6. **Text Plugins** (`render/plugins/text.rs`, `scrolling_text.rs`)
   - Font loading and text rasterization
   - Static text rendering
   - Scrolling/marquee text

7. **Clock Plugin** (`render/plugins/clock.rs`)
   - Analog clock with hand images
   - Digital clock with configurable format

### Milestone 3 - Network Protocol
**Goal: Accept connections from HDPlayer PC software**

8. **TCP Server** (`protocol/tcp_server.rs`)
   - Listen for HDPlayer connections
   - Accept multiple clients

9. **XML Protocol** (`protocol/xml_session.rs`, `protocol/command.rs`)
   - Parse incoming XML commands
   - Route to appropriate handler module
   - Send XML responses

10. **File Transfer** (`protocol/file_transfer.rs`)
    - Receive program files from HDPlayer
    - Store to program directory
    - Trigger program reload

### Milestone 4 - Full Feature Parity
11. Video playback (gstreamer integration)
12. GIF animation
13. Weather/temperature/humidity widgets
14. Neon/frame border effects
15. Screen on/off scheduling
16. Brightness control
17. NTP time sync
18. USB disk program loading
19. Cloud API reporting
20. OTA firmware upgrade

---

## Phase 3: FPGA Protocol Reverse Engineering

This is the most critical unknown. We need to capture the serial protocol between BoxPlayer and the FPGA.

### Approach 1: Wire Sniffing
- Connect logic analyzer / USB-serial adapter between SoC and FPGA
- Capture startup sequence and pixel data transmission
- Analyze framing, addressing, and pixel format

### Approach 2: Binary Analysis of libFPGADriver.so
From decompilation we know:
- Device: Altera/Intel **Cyclone IV** FPGA
- Devices: `/dev/cyclone4-0`, `/dev/cyclone4-1`
- Key structures: `HSendCard`, `HRecvCard`, `HFPGAParam`
- Parameters: scan tables, gray priority, light priority, refresh priority
- Data flow: SoC → Send Card (FPGA) → Receiving Cards → LED modules

### Known FPGA Data Structures
```rust
// From binary analysis
struct SendCardParam {
    // Gamma/brightness tables
    // Color correction (R/G/B channels)
    // Scan mode configuration
    // Pixel clock settings
}

struct RecvCardParam {
    // LED module type
    // Scan type (static, 1/2, 1/4, 1/8, 1/16, 1/32)
    // Row/column mapping
    // Color depth
    // Blanking time
}

struct FPGAParam {
    scan_param: ScanParam,
    dual_scan_tab: Vec<u8>,      // s_dualScanTab
    gray_priority: Vec<u8>,       // s_grayPriority
    light_priority: Vec<u8>,      // s_lightPriority
    refresh_priority: Vec<u8>,    // s_refreshPriority
}
```

### Approach 3: Use Existing SDK
- The `cn.huidu.device.api` (Go binary) and `HSDKProxys` may provide HTTP/gRPC APIs
- The Huidu SDK (SDK.zip in original directory) may have documentation
- Could potentially use these as intermediaries initially

---

## Rust Architecture Advantages

| Original (C++/Qt) | Rust Replacement | Benefit |
|---|---|---|
| Qt offscreen rendering | `tiny-skia` / `wgpu` | No Qt dependency, smaller binary |
| `QPluginLoader` (.so plugins) | Trait-based plugins, compiled in | No runtime linking, safer |
| `QDomDocument` XML parsing | `quick-xml` with serde | Faster, zero-copy parsing |
| Manual memory management | Ownership system | No memory leaks/crashes |
| `QThread` / pthreads | `tokio` async runtime | Better concurrency |
| `QTcpSocket` / `select()` | `tokio::net` | Scalable async I/O |
| log4c logging | `tracing` crate | Structured, filterable logging |
| ~15 shared libraries | Single static binary | Simpler deployment |
| ARM32 + ARM64 builds | Cross-compile with `cross` | Easy multi-target |

---

## Build & Deploy

```bash
# Cross-compile for PX30 (aarch64)
cross build --release --target aarch64-unknown-linux-gnu

# Cross-compile for RK3288 (armv7)
cross build --release --target armv7-unknown-linux-gnueabihf

# Deploy to device
scp target/aarch64-unknown-linux-gnu/release/huidu-player root@device:/root/Box/BoxPlayer/BoxPlayer
```

### Integration with Existing System
The Rust binary can be a drop-in replacement:
1. Replace `/root/Box/BoxPlayer/BoxPlayer` with Rust binary
2. Keep the same `runBoxPlayer.sh` startup script
3. Existing BoxDaemon watchdog works unchanged
4. Existing BoxSDK can run alongside (or be replaced later)
5. FPGA images, kernel, and system scripts remain unchanged
