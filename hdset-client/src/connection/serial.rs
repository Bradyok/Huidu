//! Direct USB/serial connection to Huidu send cards.
//!
//! HDSet can configure send cards directly via USB without a BoxPlayer intermediary.
//! The PC communicates with the send card's internal MCU over a USB-to-serial bridge.
//!
//! ## Supported hardware interfaces
//!
//! | Adapter      | Send card series | Notes                          |
//! |--------------|------------------|--------------------------------|
//! | CH341 serial | VP210, VP410, T-series | COM_115200 or COM_256000  |
//! | CH343 serial | VP820, X-series  | Higher speed                   |
//! | CP210x VCP   | Various          | Silicon Labs USB-serial        |
//! | USB bulk     | H6, H8           | libusbK/libusb, USB_BULK_V1.0  |
//!
//! ## Protocol (partial — TBD from send card firmware analysis)
//!
//! The send card serial protocol is a binary framing similar to the FPGA serial
//! protocol used inside the BoxPlayer:
//!
//! ```text
//! [0xFD]        — start-of-frame marker
//! [u8 cmd]      — command byte
//! [u16 LE len]  — payload length
//! [N bytes]     — payload
//! [u8 checksum] — XOR or sum of payload bytes
//! ```
//!
//! Key commands (TBD — need wire capture of CH341 traffic during HDSet session):
//! - Read FPGA version / status
//! - Set scan parameters (scan mode, pixel clock, scan rows)
//! - Set gamma / color correction tables
//! - Flash FPGA firmware (erase, write, verify)
//! - Flash MCU firmware
//!
//! This module is a stub.  Implement after capturing serial traffic with a
//! USB logic analyzer or Wireshark with USBPcap.

use crate::error::{Error, Result};

/// Baud rate for CH341-based send cards (COM_256000).
pub const BAUD_256000: u32 = 256_000;
/// Baud rate for older send cards (COM_115200).
pub const BAUD_115200: u32 = 115_200;

/// Start-of-frame byte for the send card serial protocol.
pub const SOF: u8 = 0xFD;

/// A direct serial connection to a Huidu send card.
pub struct SendCardSerial {
    port: Box<dyn serialport::SerialPort>,
    read_buf: Vec<u8>,
}

impl SendCardSerial {
    /// Open a serial connection to a send card at the given COM port.
    pub fn open(port_name: &str, baud_rate: u32) -> Result<Self> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(std::time::Duration::from_secs(5))
            .open()
            .map_err(|e| Error::Connection(format!("serial open {port_name}: {e}")))?;
        Ok(Self { port, read_buf: Vec::with_capacity(4096) })
    }

    /// List available serial ports.
    pub fn list_ports() -> Result<Vec<String>> {
        serialport::available_ports()
            .map_err(|e| Error::Connection(format!("list ports: {e}")))?
            .into_iter()
            .map(|p| Ok(p.port_name))
            .collect()
    }

    /// Send a raw command frame to the send card.
    ///
    /// Frame format: `[SOF][cmd][len_lo][len_hi][payload][checksum]`
    pub fn send_cmd(&mut self, cmd: u8, payload: &[u8]) -> Result<()> {
        let len = payload.len() as u16;
        let checksum = payload.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(SOF);
        frame.push(cmd);
        frame.push((len & 0xFF) as u8);
        frame.push((len >> 8) as u8);
        frame.extend_from_slice(payload);
        frame.push(checksum);
        self.port.write_all(&frame)
            .map_err(|e| Error::Connection(format!("serial write: {e}")))?;
        Ok(())
    }

    /// Read a response frame from the send card.
    /// Returns `(cmd, payload)`.
    pub fn recv_response(&mut self) -> Result<(u8, Vec<u8>)> {
        use std::io::Read;
        // Read until we have SOF + cmd + len bytes
        let mut hdr = [0u8; 4];
        self.port.read_exact(&mut hdr)
            .map_err(|e| Error::Connection(format!("serial read header: {e}")))?;
        if hdr[0] != SOF {
            return Err(Error::Protocol(format!("serial: expected SOF 0xFD, got 0x{:02X}", hdr[0])));
        }
        let cmd = hdr[1];
        let len = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
        let mut payload = vec![0u8; len + 1]; // +1 for checksum
        self.port.read_exact(&mut payload)
            .map_err(|e| Error::Connection(format!("serial read payload: {e}")))?;
        let checksum = payload.pop().unwrap();
        let expected = payload.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        if checksum != expected {
            return Err(Error::Protocol(format!(
                "serial checksum mismatch: got 0x{:02X}, expected 0x{:02X}", checksum, expected
            )));
        }
        Ok((cmd, payload))
    }

    // ── Known commands (TBD — fill in after wire capture) ──────────────────────

    /// Query the send card firmware version.
    /// Command byte TBD from wire capture.
    pub fn get_version(&mut self) -> Result<String> {
        self.send_cmd(0x01, &[])?;
        let (_cmd, data) = self.recv_response()?;
        Ok(String::from_utf8_lossy(&data).into_owned())
    }
}
