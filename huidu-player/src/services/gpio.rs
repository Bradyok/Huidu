/// GPIO/Relay control via Linux sysfs.
/// Only functional on Linux; returns empty/no-op on other platforms.

#[derive(Debug, Clone)]
pub struct RelayState {
    pub pin: u32,
    pub state: bool,
}

/// Export a GPIO pin via sysfs.
#[cfg(unix)]
pub fn export_pin(pin: u32) -> std::io::Result<()> {
    let export_path = "/sys/class/gpio/export";
    let gpio_path = format!("/sys/class/gpio/gpio{}", pin);
    if !std::path::Path::new(&gpio_path).exists() {
        std::fs::write(export_path, pin.to_string())?;
        // Give kernel time to create the sysfs entry
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // Set direction to output
    let dir_path = format!("{}/direction", gpio_path);
    std::fs::write(dir_path, "out")?;
    Ok(())
}

#[cfg(not(unix))]
pub fn export_pin(_pin: u32) -> std::io::Result<()> { Ok(()) }

/// Set a GPIO pin high (true) or low (false).
#[cfg(unix)]
pub fn set_pin(pin: u32, value: bool) -> std::io::Result<()> {
    let _ = export_pin(pin);
    let val_path = format!("/sys/class/gpio/gpio{}/value", pin);
    std::fs::write(val_path, if value { "1" } else { "0" })?;
    Ok(())
}

#[cfg(not(unix))]
pub fn set_pin(_pin: u32, _value: bool) -> std::io::Result<()> { Ok(()) }

/// Read the current state of a GPIO pin.
#[cfg(unix)]
pub fn get_pin(pin: u32) -> bool {
    let val_path = format!("/sys/class/gpio/gpio{}/value", pin);
    std::fs::read_to_string(val_path)
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn get_pin(_pin: u32) -> bool { false }

/// Get state of all configured relay pins.
pub fn get_relay_states(pins: &[u32]) -> Vec<RelayState> {
    pins.iter().map(|&pin| RelayState { pin, state: get_pin(pin) }).collect()
}

/// Set a relay pin by index.
pub fn set_relay(pins: &[u32], index: usize, value: bool) -> bool {
    if let Some(&pin) = pins.get(index) {
        set_pin(pin, value).is_ok()
    } else {
        false
    }
}
