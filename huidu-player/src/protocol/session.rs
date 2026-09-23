/// TCP session state for a connected HDPlayer client.
use uuid::Uuid;

pub struct Session {
    /// Unique session GUID
    pub guid: String,
    /// XML accumulation buffer (commands may span multiple packets)
    xml_buffer: Vec<u8>,
    xml_total_len: usize,
    /// Active file transfer state
    file_transfer: Option<FileTransfer>,
}

pub struct FileTransfer {
    pub filename: String,
    pub expected_size: u64,
    pub file_type: u16,
    pub md5: String,
    pub data: Vec<u8>,
}

/// FileEndAnswer result code for a transfer whose received bytes don't match the
/// declared size or MD5 (matches `SdkError::FileContentError` = 14).
pub const FILE_CONTENT_ERROR: u32 = 14;

impl FileTransfer {
    /// Verify the received bytes against the declared size and MD5.
    ///
    /// Returns `Ok(())` when the transfer is intact, or the non-zero
    /// FileEndAnswer result code to send back on a mismatch. An empty `md5` or a
    /// zero `expected_size` means "not declared" and that check is skipped.
    pub fn verify(&self) -> Result<(), u32> {
        if self.expected_size != 0 && self.data.len() as u64 != self.expected_size {
            return Err(FILE_CONTENT_ERROR);
        }
        if !self.md5.is_empty() {
            let computed = format!("{:x}", md5::compute(&self.data));
            if computed != self.md5.to_lowercase() {
                return Err(FILE_CONTENT_ERROR);
            }
        }
        Ok(())
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            guid: Uuid::new_v4().to_string(),
            xml_buffer: Vec::new(),
            xml_total_len: 0,
            file_transfer: None,
        }
    }

    /// Accumulate XML data from an SDK command packet
    pub fn accumulate_xml(&mut self, chunk: &[u8], total_len: usize, index: usize) {
        if index == 0 {
            self.xml_buffer.clear();
            self.xml_total_len = total_len;
        }
        self.xml_buffer.extend_from_slice(chunk);
    }

    /// Check if we've received all XML data
    pub fn xml_complete(&self) -> bool {
        self.xml_buffer.len() >= self.xml_total_len
    }

    /// Take the complete XML data, resetting the buffer
    pub fn take_xml(&mut self) -> Vec<u8> {
        self.xml_total_len = 0;
        std::mem::take(&mut self.xml_buffer)
    }

    /// Start a new file transfer
    pub fn start_file_transfer(
        &mut self,
        filename: String,
        size: u64,
        file_type: u16,
        md5: String,
    ) {
        self.file_transfer = Some(FileTransfer {
            filename,
            expected_size: size,
            file_type,
            md5,
            data: Vec::with_capacity(size as usize),
        });
    }

    /// Append data to the active file transfer
    pub fn append_file_data(&mut self, data: &[u8]) {
        if let Some(ref mut transfer) = self.file_transfer {
            transfer.data.extend_from_slice(data);
        }
    }

    /// Complete the file transfer and return the data
    pub fn complete_file_transfer(&mut self) -> Option<FileTransfer> {
        self.file_transfer.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(data: &[u8], size: u64, md5: &str) -> FileTransfer {
        FileTransfer {
            filename: "prog.bin".into(),
            expected_size: size,
            file_type: 0,
            md5: md5.into(),
            data: data.to_vec(),
        }
    }

    #[test]
    fn verify_accepts_matching_md5_and_size() {
        let data = b"hello world";
        let md5 = format!("{:x}", md5::compute(data));
        assert_eq!(transfer(data, data.len() as u64, &md5).verify(), Ok(()));
    }

    #[test]
    fn verify_rejects_wrong_md5() {
        let data = b"hello world";
        // md5 of different content
        let bad = format!("{:x}", md5::compute(b"corrupted"));
        assert_eq!(transfer(data, data.len() as u64, &bad).verify(), Err(FILE_CONTENT_ERROR));
    }

    #[test]
    fn verify_rejects_size_mismatch() {
        let data = b"hello world";
        // declared size larger than received bytes, no md5 declared
        assert_eq!(transfer(data, 999, "").verify(), Err(FILE_CONTENT_ERROR));
    }

    #[test]
    fn verify_skips_undeclared_checks() {
        // empty md5 + zero size => nothing to check => accepted
        assert_eq!(transfer(b"anything", 0, "").verify(), Ok(()));
    }

    #[test]
    fn verify_is_case_insensitive_on_md5() {
        let data = b"MixedCase";
        let md5_upper = format!("{:x}", md5::compute(data)).to_uppercase();
        assert_eq!(transfer(data, data.len() as u64, &md5_upper).verify(), Ok(()));
    }

    #[test]
    fn transfer_lifecycle_appends_and_completes() {
        let mut s = Session::new();
        s.start_file_transfer("a.bin".into(), 5, 0, String::new());
        s.append_file_data(b"ab");
        s.append_file_data(b"cde");
        let t = s.complete_file_transfer().expect("transfer present");
        assert_eq!(t.data, b"abcde");
        assert!(s.complete_file_transfer().is_none()); // taken
    }
}
