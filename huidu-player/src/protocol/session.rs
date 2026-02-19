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
