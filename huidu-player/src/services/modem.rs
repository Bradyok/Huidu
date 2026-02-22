/// 4G modem control via AT commands on Unix serial devices.
/// Only functional on Unix (Linux).

#[derive(Debug, Clone, Default)]
pub struct ModemInfo {
    pub device: String,
    pub model: String,
    pub imei: String,
    pub signal_rssi: i32,  // dBm
    pub registered: bool,
    pub connected: bool,
    pub apn: String,
}

impl ModemInfo {
    pub fn to_xml(&self) -> String {
        format!(
            "<modem device=\"{}\" model=\"{}\" imei=\"{}\" signal=\"{}\" \
             registered=\"{}\" connected=\"{}\" apn=\"{}\"/>",
            xml_esc(&self.device), xml_esc(&self.model), xml_esc(&self.imei),
            self.signal_rssi,
            if self.registered { "true" } else { "false" },
            if self.connected { "true" } else { "false" },
            xml_esc(&self.apn),
        )
    }
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Send an AT command and read the response (blocking, Unix-only).
#[cfg(unix)]
pub fn send_at_command(device: &str, cmd: &str) -> Option<String> {
    use std::io::{Read, Write};
    let port = serialport::new(device, 115200)
        .timeout(std::time::Duration::from_secs(3))
        .open()
        .ok()?;
    let mut port = port;
    port.write_all(format!("{}\r", cmd).as_bytes()).ok()?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mut buf = vec![0u8; 1024];
    let n = port.read(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf[..n]).to_string())
}

#[cfg(not(unix))]
pub fn send_at_command(_device: &str, _cmd: &str) -> Option<String> { None }

/// Scan for a modem on common serial devices.
#[cfg(unix)]
pub fn detect_modem() -> Option<String> {
    let candidates = vec![
        "/dev/ttyUSB0", "/dev/ttyUSB1", "/dev/ttyUSB2",
        "/dev/ttyACM0", "/dev/ttyACM1",
    ];
    for dev in candidates {
        if let Some(resp) = send_at_command(dev, "ATI") {
            if resp.contains("OK") || resp.contains("Quectel") || resp.contains("SIMCom") {
                return Some(dev.to_string());
            }
        }
    }
    None
}

#[cfg(not(unix))]
pub fn detect_modem() -> Option<String> { None }

/// Parse +CSQ response to dBm.
fn parse_csq(resp: &str) -> i32 {
    // +CSQ: 15,0
    if let Some(pos) = resp.find("+CSQ:") {
        let after = resp[pos + 5..].trim();
        let rssi_str = after.split(',').next().unwrap_or("99").trim();
        if let Ok(rssi) = rssi_str.parse::<i32>() {
            if rssi == 99 { return -113; } // Unknown
            return -113 + rssi * 2;
        }
    }
    -113
}

/// Parse +CREG response for registration status.
fn parse_creg(resp: &str) -> bool {
    if let Some(pos) = resp.find("+CREG:") {
        let after = resp[pos + 6..].trim();
        // Could be ",1" or ",5" (roaming) for registered
        let stat = after.split(',').last().unwrap_or("0").trim().trim_end_matches('\r').trim_end_matches('\n');
        return stat == "1" || stat == "5";
    }
    false
}

/// Get modem info from the given device.
pub fn get_modem_info(device: &str) -> ModemInfo {
    let mut info = ModemInfo {
        device: device.to_string(),
        ..Default::default()
    };

    if let Some(resp) = send_at_command(device, "ATI") {
        // Extract model name from ATI response
        for line in resp.lines() {
            let l = line.trim();
            if !l.is_empty() && l != "OK" && !l.starts_with("AT") {
                info.model = l.to_string();
                break;
            }
        }
    }

    if let Some(resp) = send_at_command(device, "AT+GSN") {
        for line in resp.lines() {
            let l = line.trim();
            if l.len() == 15 && l.chars().all(|c| c.is_ascii_digit()) {
                info.imei = l.to_string();
                break;
            }
        }
    }

    if let Some(resp) = send_at_command(device, "AT+CSQ") {
        info.signal_rssi = parse_csq(&resp);
    }

    if let Some(resp) = send_at_command(device, "AT+CREG?") {
        info.registered = parse_creg(&resp);
    }

    info
}
