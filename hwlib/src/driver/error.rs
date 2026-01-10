use thiserror::Error;

/// Driver operation errors
#[derive(Error, Debug)]
pub enum DriverError {
    #[error("Driver not initialized: {0}")]
    NotInitialized(String),

    #[error("Driver initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Service operation failed: {0}")]
    ServiceError(String),

    #[error("IOCTL operation failed: {0}")]
    IoctlError(String),

    #[error("File operation failed: {0}")]
    FileError(String),

    #[error("Hash verification failed: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Windows API error: {0}")]
    WindowsApiError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Resource extraction failed: {0}")]
    ResourceExtractionFailed(String),

    #[error("Driver communication timeout")]
    Timeout,

    #[error("Invalid response from driver: {0}")]
    InvalidResponse(String),

    #[error("Operation not supported: {0}")]
    NotSupported(String),
}

impl From<std::io::Error> for DriverError {
    fn from(error: std::io::Error) -> Self {
        DriverError::FileError(error.to_string())
    }
}

/// Result type for driver operations
pub type DriverResult<T> = Result<T, DriverError>;
