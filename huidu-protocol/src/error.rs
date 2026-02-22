//! Error types shared across all Huidu crates.

use thiserror::Error;

/// Protocol-level errors produced by shared packet / XML / transfer / discovery code.
///
/// Each higher-level crate (`hdplayer`, `hdset`, `huidu-player`) defines its own
/// richer `Error` enum and implements `From<huidu_protocol::Error>` to map these.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Device error (code {code}): {message}")]
    Device { code: i32, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

// ── Device-side error codes ───────────────────────────────────────────────────

/// All device-side error codes returned in SDK XML `<result value="N"/>` tags.
///
/// Confirmed from string analysis of HCatNet.dll and the BoxPlayer ARM binary.
/// The numeric assignments are fixed by the Huidu firmware — do not renumber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DeviceError {
    Success = 0,
    WriteFinish = 1,
    ProcessError = 2,
    VersionTooLow = 3,
    DeviceOccupied = 4,
    FileOccupied = 5,
    ReadFileExcessive = 6,
    InvalidPacketLen = 7,
    InvalidParam = 8,
    NotSpaceToSave = 9,
    CreateFileFailed = 10,
    WriteFileFailed = 11,
    ReadFileFailed = 12,
    InvalidFileData = 13,
    FileContentError = 14,
    OpenFileFailed = 15,
    SeekFileFailed = 16,
    RenameFailed = 17,
    FileNotFound = 18,
    FileNotFinish = 19,
    XmlCmdTooLong = 20,
    InvalidXmlIndex = 21,
    ParseXmlFailed = 22,
    InvalidMethod = 23,
    MemoryFailed = 24,
    SystemError = 25,
    UnsupportVideo = 26,
    NotMediaFile = 27,
    ParseVideoFailed = 28,
    UnsupportFrameRate = 29,
    UnsupportResolution = 30,
    UnsupportFormat = 31,
    UnsupportDuration = 32,
    DownloadFileFailed = 33,
    DownloadingFile = 34,
    Processing = 35,
    ScreenNodeIsNull = 36,
    NodeExist = 37,
    NodeNotExist = 38,
    PluginNotExist = 39,
    CheckLicenseFailed = 40,
    NotFoundWifiModule = 41,
    TestWifiUnsuccessful = 42,
    RunningError = 43,
    UnsupportMethod = 44,
    InvalidGuid = 45,
    FirmwareFormatError = 46,
    TagNotFound = 47,
    AttrNotFound = 48,
    CreateTagFailed = 49,
    UnsupportDeviceType = 50,
    PermissionDenied = 51,
    PasswdTooSimple = 52,
    UsbNotInsert = 53,
    DelayRespond = 54,
    ShortlyReturn = 55,
    NotSupportSdk = 56,
}

impl DeviceError {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Success,
            1 => Self::WriteFinish,
            2 => Self::ProcessError,
            3 => Self::VersionTooLow,
            4 => Self::DeviceOccupied,
            5 => Self::FileOccupied,
            6 => Self::ReadFileExcessive,
            7 => Self::InvalidPacketLen,
            8 => Self::InvalidParam,
            9 => Self::NotSpaceToSave,
            10 => Self::CreateFileFailed,
            11 => Self::WriteFileFailed,
            12 => Self::ReadFileFailed,
            13 => Self::InvalidFileData,
            14 => Self::FileContentError,
            15 => Self::OpenFileFailed,
            16 => Self::SeekFileFailed,
            17 => Self::RenameFailed,
            18 => Self::FileNotFound,
            19 => Self::FileNotFinish,
            20 => Self::XmlCmdTooLong,
            21 => Self::InvalidXmlIndex,
            22 => Self::ParseXmlFailed,
            23 => Self::InvalidMethod,
            24 => Self::MemoryFailed,
            25 => Self::SystemError,
            26 => Self::UnsupportVideo,
            27 => Self::NotMediaFile,
            28 => Self::ParseVideoFailed,
            29 => Self::UnsupportFrameRate,
            30 => Self::UnsupportResolution,
            31 => Self::UnsupportFormat,
            32 => Self::UnsupportDuration,
            33 => Self::DownloadFileFailed,
            34 => Self::DownloadingFile,
            35 => Self::Processing,
            36 => Self::ScreenNodeIsNull,
            37 => Self::NodeExist,
            38 => Self::NodeNotExist,
            39 => Self::PluginNotExist,
            40 => Self::CheckLicenseFailed,
            41 => Self::NotFoundWifiModule,
            42 => Self::TestWifiUnsuccessful,
            43 => Self::RunningError,
            44 => Self::UnsupportMethod,
            45 => Self::InvalidGuid,
            46 => Self::FirmwareFormatError,
            47 => Self::TagNotFound,
            48 => Self::AttrNotFound,
            49 => Self::CreateTagFailed,
            50 => Self::UnsupportDeviceType,
            51 => Self::PermissionDenied,
            52 => Self::PasswdTooSimple,
            53 => Self::UsbNotInsert,
            54 => Self::DelayRespond,
            55 => Self::ShortlyReturn,
            56 => Self::NotSupportSdk,
            _ => Self::ProcessError,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::WriteFinish => "Write finished",
            Self::ProcessError => "Process error",
            Self::VersionTooLow => "Client version too old",
            Self::DeviceOccupied => "Device occupied",
            Self::FileOccupied => "File occupied",
            Self::ReadFileExcessive => "Read size exceeded",
            Self::InvalidPacketLen => "Invalid packet length",
            Self::InvalidParam => "Invalid parameter",
            Self::NotSpaceToSave => "Insufficient storage",
            Self::CreateFileFailed => "File creation failed",
            Self::WriteFileFailed => "File write failed",
            Self::ReadFileFailed => "File read failed",
            Self::InvalidFileData => "Invalid file data",
            Self::FileContentError => "File content error",
            Self::OpenFileFailed => "File open failed",
            Self::SeekFileFailed => "File seek failed",
            Self::RenameFailed => "Rename failed",
            Self::FileNotFound => "File not found",
            Self::FileNotFinish => "File incomplete",
            Self::XmlCmdTooLong => "XML command too long",
            Self::InvalidXmlIndex => "Invalid XML index",
            Self::ParseXmlFailed => "XML parse failed",
            Self::InvalidMethod => "Unknown method",
            Self::MemoryFailed => "Memory allocation failed",
            Self::SystemError => "System error",
            Self::UnsupportVideo => "Video format unsupported",
            Self::NotMediaFile => "Not a media file",
            Self::ParseVideoFailed => "Video parse failed",
            Self::UnsupportFrameRate => "Frame rate unsupported",
            Self::UnsupportResolution => "Resolution unsupported",
            Self::UnsupportFormat => "Format unsupported",
            Self::UnsupportDuration => "Duration unsupported",
            Self::DownloadFileFailed => "Download failed",
            Self::DownloadingFile => "Download in progress",
            Self::Processing => "Operation in progress",
            Self::ScreenNodeIsNull => "No active screen",
            Self::NodeExist => "Node already exists",
            Self::NodeNotExist => "Node not found",
            Self::PluginNotExist => "Plugin not installed",
            Self::CheckLicenseFailed => "License check failed",
            Self::NotFoundWifiModule => "No WiFi module",
            Self::TestWifiUnsuccessful => "WiFi test failed",
            Self::RunningError => "Runtime error",
            Self::UnsupportMethod => "Method not supported",
            Self::InvalidGuid => "Invalid GUID",
            Self::FirmwareFormatError => "Invalid firmware format",
            Self::TagNotFound => "XML tag not found",
            Self::AttrNotFound => "XML attribute not found",
            Self::CreateTagFailed => "Tag creation failed",
            Self::UnsupportDeviceType => "Device type unsupported",
            Self::PermissionDenied => "Permission denied",
            Self::PasswdTooSimple => "Password too simple",
            Self::UsbNotInsert => "USB not inserted",
            Self::DelayRespond => "Delayed response",
            Self::ShortlyReturn => "Short return",
            Self::NotSupportSdk => "SDK not supported",
        }
    }
}
