//! Stable structured error taxonomy for MCP tool responses.
//!
//! Error code strings and retryability are part of the client contract: tools
//! return them in `structured_content` so agents can decide whether to retry,
//! ask for permissions, or change capture parameters.

use serde_json::json;

/// Machine-readable error codes returned by Zeuxis tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    PermissionDenied,
    CaptureUnsupportedOnPlatform,
    WindowNotFound,
    MonitorNotFound,
    NoCaptureYet,
    InvalidRegion,
    CursorUnavailable,
    EncodeFailed,
    StorageFailed,
    InvalidParams,
}

impl ErrorCode {
    /// Stable snake_case string returned in MCP `structured_content.error_code`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::CaptureUnsupportedOnPlatform => "capture_unsupported_on_platform",
            Self::WindowNotFound => "window_not_found",
            Self::MonitorNotFound => "monitor_not_found",
            Self::NoCaptureYet => "no_capture_yet",
            Self::InvalidRegion => "invalid_region",
            Self::CursorUnavailable => "cursor_unavailable",
            Self::EncodeFailed => "encode_failed",
            Self::StorageFailed => "storage_failed",
            Self::InvalidParams => "invalid_params",
        }
    }
}

/// Error value carried through capture, storage, and MCP result boundaries.
///
/// `retryable` describes whether the same call might succeed after an external
/// state change, such as permission approval or filesystem recovery.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ServerError {
    code: ErrorCode,
    message: String,
    retryable: bool,
}

impl ServerError {
    /// Constructs an error with explicit code, message, and retryability.
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

    pub fn no_capture_yet(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NoCaptureYet, message, true)
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

    /// Typed error code for branching inside the server.
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Stable snake_case code for MCP clients and worker JSON.
    pub const fn error_code(&self) -> &'static str {
        self.code.as_str()
    }

    /// Human-readable detail safe to surface in tool text content.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the same call might succeed after an external state change.
    pub const fn retryable(&self) -> bool {
        self.retryable
    }

    /// JSON object with `error_code`, `message`, and `retryable` fields.
    pub fn structured_content(&self) -> serde_json::Value {
        json!({
            "error_code": self.error_code(),
            "message": self.message(),
            "retryable": self.retryable(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_errors_all_error_codes_have_expected_strings() {
        assert_eq!(ErrorCode::PermissionDenied.as_str(), "permission_denied");
        assert_eq!(
            ErrorCode::CaptureUnsupportedOnPlatform.as_str(),
            "capture_unsupported_on_platform"
        );
        assert_eq!(ErrorCode::WindowNotFound.as_str(), "window_not_found");
        assert_eq!(ErrorCode::MonitorNotFound.as_str(), "monitor_not_found");
        assert_eq!(ErrorCode::NoCaptureYet.as_str(), "no_capture_yet");
        assert_eq!(ErrorCode::InvalidRegion.as_str(), "invalid_region");
        assert_eq!(ErrorCode::CursorUnavailable.as_str(), "cursor_unavailable");
        assert_eq!(ErrorCode::EncodeFailed.as_str(), "encode_failed");
        assert_eq!(ErrorCode::StorageFailed.as_str(), "storage_failed");
        assert_eq!(ErrorCode::InvalidParams.as_str(), "invalid_params");
    }

    #[test]
    fn mcp_errors_constructor_helpers_set_retryability() {
        assert!(ServerError::permission_denied("x").retryable());
        assert!(!ServerError::capture_unsupported_on_platform("x").retryable());
        assert!(!ServerError::window_not_found("x").retryable());
        assert!(!ServerError::monitor_not_found("x").retryable());
        assert!(ServerError::no_capture_yet("x").retryable());
        assert!(!ServerError::invalid_region("x").retryable());
        assert!(ServerError::cursor_unavailable("x").retryable());
        assert!(!ServerError::encode_failed("x").retryable());
        assert!(ServerError::storage_failed("x").retryable());
        assert!(!ServerError::invalid_params("x").retryable());
    }

    #[test]
    fn mcp_errors_structured_content_matches_fields() {
        let error = ServerError::new(ErrorCode::EncodeFailed, "encode fail", false);
        let json = error.structured_content();
        assert_eq!(error.code(), ErrorCode::EncodeFailed);
        assert_eq!(json["error_code"], "encode_failed");
        assert_eq!(json["message"], "encode fail");
        assert_eq!(json["retryable"], false);
    }
}
