//! `/dev/ttyS1` raw serial port (115200 8N1), matching `HFPGASerial::Init`.
//!
//! The stock driver opens `O_RDWR|O_NOCTTY` and configures a fully raw line at 115200
//! (`libFPGADriver.so.c:16182-16190`). We reproduce that with libc termios so a frame
//! is written/read verbatim (no canonical processing, no echo, no CR/LF translation).

use std::io;
use std::os::unix::io::RawFd;

/// An open, raw-configured serial port to the FPGA sender link.
pub struct Serial {
    fd: RawFd,
}

impl Serial {
    /// Open and raw-configure the port (default `/dev/ttyS1`).
    pub fn open(path: &str) -> io::Result<Self> {
        let cpath = std::ffi::CString::new(path).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // O_RDWR | O_NOCTTY | O_NONBLOCK during open; clear NONBLOCK after so reads block.
        let fd = unsafe {
            libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK)
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let s = Serial { fd };
        s.configure()?;
        // clear O_NONBLOCK
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
        Ok(s)
    }

    fn configure(&self) -> io::Result<()> {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(self.fd, &mut t) != 0 {
                return Err(io::Error::last_os_error());
            }
            libc::cfmakeraw(&mut t);
            // 115200 8N1
            libc::cfsetispeed(&mut t, libc::B115200);
            libc::cfsetospeed(&mut t, libc::B115200);
            t.c_cflag &= !(libc::PARENB | libc::CSTOPB | libc::CSIZE | libc::CRTSCTS);
            t.c_cflag |= libc::CS8 | libc::CREAD | libc::CLOCAL;
            // Blocking read of at least 1 byte, 0.5s inter-byte timeout.
            t.c_cc[libc::VMIN] = 0;
            t.c_cc[libc::VTIME] = 5;
            if libc::tcsetattr(self.fd, libc::TCSANOW, &t) != 0 {
                return Err(io::Error::last_os_error());
            }
            libc::tcflush(self.fd, libc::TCIOFLUSH);
        }
        Ok(())
    }

    /// Write a whole frame.
    pub fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut off = 0;
        while off < buf.len() {
            let n = unsafe { libc::write(self.fd, buf[off..].as_ptr() as *const _, buf.len() - off) };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            off += n as usize;
        }
        Ok(())
    }

    /// Read available bytes (up to `buf.len()`); returns count (0 on timeout).
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            return Err(e);
        }
        Ok(n as usize)
    }
}

impl Drop for Serial {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}
