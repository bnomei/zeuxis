use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    PermissionDenied,
    CaptureUnsupportedOnPlatform,
    WindowNotFound,
    MonitorNotFound,
    InvalidRegion,
    CursorUnavailable,
    EncodeFailed,
    StorageFailed,
    InvalidParams,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::CaptureUnsupportedOnPlatform => "capture_unsupported_on_platform",
            Self::WindowNotFound => "window_not_found",
            Self::MonitorNotFound => "monitor_not_found",
            Self::InvalidRegion => "invalid_region",
            Self::CursorUnavailable => "cursor_unavailable",
            Self::EncodeFailed => "encode_failed",
            Self::StorageFailed => "storage_failed",
            Self::InvalidParams => "invalid_params",
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ServerError {
    code: ErrorCode,
    message: String,
    retryable: bool,
}

impl ServerError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PermissionDenied, message, true)
    }

    pub fn capture_unsupported_on_platform(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::CaptureUnsupportedOnPlatform, message, false)
    }

    pub fn window_not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::WindowNotFound, message, false)
    }

    pub fn monitor_not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::MonitorNotFound, message, false)
    }

    pub fn invalid_region(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRegion, message, false)
    }

    pub fn cursor_unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::CursorUnavailable, message, true)
    }

    pub fn encode_failed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::EncodeFailed, message, false)
    }

    pub fn storage_failed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::StorageFailed, message, true)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, message, false)
    }

    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    pub const fn error_code(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn retryable(&self) -> bool {
        self.retryable
    }

    pub fn structured_content(&self) -> serde_json::Value {
        json!({
            "error_code": self.error_code(),
            "message": self.message(),
            "retryable": self.retryable(),
        })
    }
}
