use thiserror::Error;

pub use huidu_protocol::error::DeviceError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("XML parse error: {0}")]
    Xml(String),

    #[error("Device error (code {code}): {message}")]
    Device { code: i32, message: String },

    #[error("File transfer error: {0}")]
    Transfer(String),

    #[error("Timeout")]
    Timeout,

    #[error("Not connected")]
    NotConnected,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

impl From<huidu_protocol::Error> for Error {
    fn from(e: huidu_protocol::Error) -> Self {
        match e {
            huidu_protocol::Error::Io(e)              => Self::Io(e),
            huidu_protocol::Error::Protocol(s)        => Self::Protocol(s),
            huidu_protocol::Error::Connection(s)      => Self::Connection(s),
            huidu_protocol::Error::Device { code, message } => Self::Device { code, message },
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
