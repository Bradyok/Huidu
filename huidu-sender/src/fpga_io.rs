//! Unix spidev + sysfs-GPIO backend for the userspace FPGA loader
//! ([`crate::fpga_load`]). This is the fallback path; the preferred path is the
//! in-kernel `altera-ps-spi` driver, which needs none of this.
//!
//! ⚠️ Confirm-on-hardware: the three GPIO *global* numbers depend on the
//! running kernel's gpiochip base for GPIO0 (sysfs uses base+offset, not the
//! RK_PA0 offset). Read them from the live unit and pass on the CLI. Requires
//! `CONFIG_GPIO_SYSFS` (added to the kernel fragment) and `CONFIG_SPI_SPIDEV`.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::io::RawFd;
use std::path::PathBuf;

use crate::fpga_load::PsIo;

// spidev ioctls (linux/spi/spidev.h), 'k' = 0x6b.
const SPI_IOC_WR_MODE: libc::c_ulong = 0x4001_6b01; // _IOW('k',1,u8)
const SPI_IOC_WR_BITS_PER_WORD: libc::c_ulong = 0x4001_6b03; // _IOW('k',3,u8)
const SPI_IOC_WR_MAX_SPEED_HZ: libc::c_ulong = 0x4004_6b04; // _IOW('k',4,u32)

/// An open spidev, TX-only (passive serial is write-only: DATA0/DCLK).
pub struct SpiDev {
    fd: RawFd,
}

impl SpiDev {
    /// Open `/dev/spidevB.C`, set mode 0, 8 bits, `speed_hz`.
    pub fn open(path: &str, speed_hz: u32) -> std::io::Result<Self> {
        let cpath = std::ffi::CString::new(path)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_WRONLY) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let dev = SpiDev { fd };
        let mode: u8 = 0;
        let bits: u8 = 8;
        unsafe {
            if libc::ioctl(fd, SPI_IOC_WR_MODE, &mode) < 0
                || libc::ioctl(fd, SPI_IOC_WR_BITS_PER_WORD, &bits) < 0
                || libc::ioctl(fd, SPI_IOC_WR_MAX_SPEED_HZ, &speed_hz) < 0
            {
                let e = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(e);
            }
        }
        Ok(dev)
    }

    /// Write all `buf`, chunked (spidev caps a single transfer, default 4096).
    pub fn write_all(&self, buf: &[u8]) -> std::io::Result<()> {
        for chunk in buf.chunks(4096) {
            let mut off = 0;
            while off < chunk.len() {
                let n = unsafe {
                    libc::write(self.fd, chunk[off..].as_ptr() as *const _, chunk.len() - off)
                };
                if n < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                off += n as usize;
            }
        }
        Ok(())
    }
}

impl Drop for SpiDev {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

/// A single sysfs-exported GPIO line (`/sys/class/gpio/gpioN`).
pub struct SysfsGpio {
    dir: PathBuf,
    number: u32,
}

impl SysfsGpio {
    /// Export `number` and set its direction (`"in"` or `"out"`).
    pub fn export(number: u32, direction: &str) -> std::io::Result<Self> {
        let dir = PathBuf::from(format!("/sys/class/gpio/gpio{number}"));
        if !dir.exists() {
            // Best-effort export; ignore EBUSY (already exported).
            let _ = fs::write("/sys/class/gpio/export", number.to_string());
        }
        // The direction file may appear a beat after export.
        let dpath = dir.join("direction");
        let mut ok = false;
        for _ in 0..50 {
            if fs::write(&dpath, direction).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !ok {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("gpio{number}: cannot set direction {direction}"),
            ));
        }
        Ok(SysfsGpio { dir, number })
    }

    /// Write a raw level (`true` = high).
    pub fn set(&self, high: bool) -> std::io::Result<()> {
        fs::write(self.dir.join("value"), if high { "1" } else { "0" })
    }

    /// Read the raw level (`true` = high).
    pub fn get(&self) -> std::io::Result<bool> {
        let mut f = fs::File::open(self.dir.join("value"))?;
        let mut buf = [0u8; 2];
        let n = f.read(&mut buf)?;
        Ok(n > 0 && buf[0] == b'1')
    }
}

impl Drop for SysfsGpio {
    fn drop(&mut self) {
        let _ = fs::write("/sys/class/gpio/unexport", self.number.to_string());
    }
}

/// A [`PsIo`] over spidev + three sysfs GPIOs.
///
/// Active-low handling: nCONFIG and nSTATUS are active-low (per the Altera PS
/// protocol and the stock DT), CONF_DONE is active-high.
pub struct SpidevPsIo {
    spi: SpiDev,
    nconfig: SysfsGpio, // output, active-low
    nstatus: SysfsGpio, // input,  active-low
    confdone: SysfsGpio, // input, active-high
}

impl SpidevPsIo {
    pub fn new(spi: SpiDev, nconfig: SysfsGpio, nstatus: SysfsGpio, confdone: SysfsGpio) -> Self {
        SpidevPsIo { spi, nconfig, nstatus, confdone }
    }
}

impl PsIo for SpidevPsIo {
    fn reset_assert(&mut self) -> std::io::Result<()> {
        self.nconfig.set(false) // active-low asserted = drive LOW
    }
    fn reset_release(&mut self) -> std::io::Result<()> {
        self.nconfig.set(true) // deasserted = drive HIGH
    }
    fn status_asserted(&mut self) -> std::io::Result<bool> {
        Ok(!self.nstatus.get()?) // active-low: asserted when the pin reads LOW
    }
    fn conf_done(&mut self) -> std::io::Result<bool> {
        self.confdone.get() // active-high
    }
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.spi.write_all(bytes)
    }
    fn delay_us(&mut self, us: u64) {
        std::thread::sleep(std::time::Duration::from_micros(us));
    }
}
