/// GPS service — reads NMEA sentences from a serial device and parses GPS data.
/// Only functional on Unix (Linux).

#[derive(Debug, Clone, Default)]
pub struct GpsReading {
    pub lat: f64,
    pub lon: f64,
    pub altitude: f32,
    pub speed: f32,
    pub heading: f32,
    pub valid: bool,
}

impl GpsReading {
    pub fn to_xml(&self) -> String {
        format!(
            "<gps valid=\"{}\" lat=\"{:.6}\" lon=\"{:.6}\" altitude=\"{:.1}\" speed=\"{:.1}\" heading=\"{:.1}\"/>",
            self.valid, self.lat, self.lon, self.altitude, self.speed, self.heading
        )
    }
}

/// Parse a $GPRMC sentence and update the reading.
/// Format: $GPRMC,HHMMSS.ss,A,LLLL.LL,a,YYYYY.YY,a,x.x,x.x,DDMMYY,...
pub fn parse_gprmc(sentence: &str, reading: &mut GpsReading) -> bool {
    let fields: Vec<&str> = sentence.split(',').collect();
    if fields.len() < 7 { return false; }
    if !fields[0].ends_with("RMC") { return false; }

    reading.valid = fields.get(2) == Some(&"A");
    if !reading.valid { return true; }

    if let Some(lat_str) = fields.get(3) {
        if let Some(lat_dir) = fields.get(4) {
            reading.lat = parse_nmea_coord(lat_str, lat_dir);
        }
    }
    if let Some(lon_str) = fields.get(5) {
        if let Some(lon_dir) = fields.get(6) {
            reading.lon = parse_nmea_coord(lon_str, lon_dir);
        }
    }
    if let Some(speed_str) = fields.get(7) {
        reading.speed = speed_str.parse().unwrap_or(0.0);
    }
    if let Some(heading_str) = fields.get(8) {
        reading.heading = heading_str.parse().unwrap_or(0.0);
    }
    true
}

/// Parse a $GPGGA sentence for altitude.
pub fn parse_gpgga(sentence: &str, reading: &mut GpsReading) {
    let fields: Vec<&str> = sentence.split(',').collect();
    if fields.len() < 10 { return; }
    if !fields[0].ends_with("GGA") { return; }
    if let Some(alt_str) = fields.get(9) {
        reading.altitude = alt_str.parse().unwrap_or(0.0);
    }
}

fn parse_nmea_coord(coord: &str, dir: &str) -> f64 {
    if coord.len() < 4 { return 0.0; }
    // DDDMM.MMMM format
    let dot_pos = coord.find('.').unwrap_or(coord.len());
    let deg_digits = if dot_pos > 2 { dot_pos - 2 } else { return 0.0; };
    let degrees: f64 = coord[..deg_digits].parse().unwrap_or(0.0);
    let minutes: f64 = coord[deg_digits..].parse().unwrap_or(0.0);
    let decimal = degrees + minutes / 60.0;
    if dir == "S" || dir == "W" { -decimal } else { decimal }
}

/// Run GPS reading loop on the given device path.
/// Reads lines from the serial device, parses NMEA, updates `state`.
/// Exits when cancelled.
#[cfg(unix)]
pub async fn run(
    device_path: String,
    state: std::sync::Arc<tokio::sync::RwLock<super::manager::ServicesState>>,
    cancel: tokio_util::sync::CancellationToken,
) {
    if device_path.is_empty() { return; }
    tracing::info!("GPS service starting on {}", device_path);

    loop {
        // Spawn a blocking thread to read the serial port
        let (tx, mut rx) = tokio::sync::mpsc::channel::<GpsReading>(8);
        let path = device_path.clone();
        let cancel2 = cancel.clone();

        tokio::task::spawn_blocking(move || {
            use std::io::BufRead;
            let port = serialport::new(&path, 9600)
                .timeout(std::time::Duration::from_millis(500))
                .open();
            let Ok(port) = port else { return; };
            let mut reader = std::io::BufReader::new(port);
            let mut reading = GpsReading::default();
            loop {
                if cancel2.is_cancelled() { break; }
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim().to_string();
                        parse_gprmc(&trimmed, &mut reading);
                        parse_gpgga(&trimmed, &mut reading);
                        let _ = tx.blocking_send(reading.clone());
                    }
                }
            }
        });

        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                msg = rx.recv() => {
                    match msg {
                        Some(reading) => {
                            if let Ok(mut s) = state.try_write() {
                                s.gps_reading = reading;
                            }
                        }
                        None => break, // thread ended
                    }
                }
            }
        }

        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
        }
    }
}

#[cfg(not(unix))]
pub async fn run(
    _device_path: String,
    _state: std::sync::Arc<tokio::sync::RwLock<super::manager::ServicesState>>,
    _cancel: tokio_util::sync::CancellationToken,
) {}
