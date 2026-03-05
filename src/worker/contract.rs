use serde::{Deserialize, Serialize};

use crate::{
    mcp::{
        errors::{ErrorCode, ServerError},
        result::CaptureTargetPayload,
    },
    storage::CaptureOutputFormat,
};

pub const WORKER_CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerOutputFormat {
    Png,
    Jpeg,
    Webp,
}

impl WorkerOutputFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::Png => ".png",
            Self::Jpeg => ".jpg",
            Self::Webp => ".webp",
        }
    }

    pub const fn to_storage(self) -> CaptureOutputFormat {
        match self {
            Self::Png => CaptureOutputFormat::Png,
            Self::Jpeg => CaptureOutputFormat::Jpeg,
            Self::Webp => CaptureOutputFormat::Webp,
        }
    }
}

impl From<CaptureOutputFormat> for WorkerOutputFormat {
    fn from(value: CaptureOutputFormat) -> Self {
        match value {
            CaptureOutputFormat::Png => Self::Png,
            CaptureOutputFormat::Jpeg => Self::Jpeg,
            CaptureOutputFormat::Webp => Self::Webp,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureOperation {
    CaptureScreen {
        monitor_id: Option<u32>,
    },
    CaptureActiveWindow,
    CaptureCursorWindow {
        include_system_windows: bool,
    },
    CaptureWindow {
        window_id: u32,
    },
    CaptureCursorRegion {
        size: u32,
    },
    CaptureRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    CaptureMonitorRegion {
        monitor_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

impl CaptureOperation {
    pub const fn capture_mode(&self) -> &'static str {
        match self {
            Self::CaptureScreen { .. } => "capture_screen",
            Self::CaptureActiveWindow => "capture_active_window",
            Self::CaptureCursorWindow { .. } => "capture_cursor_window",
            Self::CaptureWindow { .. } => "capture_window",
            Self::CaptureCursorRegion { .. } => "capture_cursor_region",
            Self::CaptureRect { .. } => "capture_rect",
            Self::CaptureMonitorRegion { .. } => "capture_monitor_region",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerOutputOptions {
    pub format: WorkerOutputFormat,
    pub jpeg_quality: u8,
    pub max_dimension: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerRequest {
    pub v: u32,
    pub request_id: String,
    pub operation: CaptureOperation,
    pub output: WorkerOutputOptions,
    pub artifact_path: String,
}

impl WorkerRequest {
    pub fn validate(&self) -> Result<(), WorkerErrorPayload> {
        if self.v != WORKER_CONTRACT_VERSION {
            return Err(WorkerErrorPayload::invalid_params(format!(
                "unsupported worker contract version {}; expected {}",
                self.v, WORKER_CONTRACT_VERSION
            )));
        }
        if self.request_id.trim().is_empty() {
            return Err(WorkerErrorPayload::invalid_params(
                "request_id must not be empty",
            ));
        }
        if self.artifact_path.trim().is_empty() {
            return Err(WorkerErrorPayload::invalid_params(
                "artifact_path must not be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerSuccessPayload {
    pub artifact_path: String,
    pub output_format: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub input_units: String,
    pub input_width: Option<u32>,
    pub input_height: Option<u32>,
    pub target: CaptureTargetPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerResponse {
    pub v: u32,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<WorkerSuccessPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WorkerErrorPayload>,
}

impl WorkerResponse {
    pub fn success(request_id: impl Into<String>, result: WorkerSuccessPayload) -> Self {
        Self {
            v: WORKER_CONTRACT_VERSION,
            request_id: request_id.into(),
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(request_id: impl Into<String>, error: WorkerErrorPayload) -> Self {
        Self {
            v: WORKER_CONTRACT_VERSION,
            request_id: request_id.into(),
            ok: false,
            result: None,
            error: Some(error),
        }
    }

    pub fn validate(&self) -> Result<(), WorkerErrorPayload> {
        if self.v != WORKER_CONTRACT_VERSION {
            return Err(WorkerErrorPayload::invalid_params(format!(
                "unsupported worker contract version {}; expected {}",
                self.v, WORKER_CONTRACT_VERSION
            )));
        }
        if self.request_id.trim().is_empty() {
            return Err(WorkerErrorPayload::invalid_params(
                "request_id must not be empty",
            ));
        }
        if self.ok {
            if self.error.is_some() {
                return Err(WorkerErrorPayload::invalid_params(
                    "ok response must not include error payload",
                ));
            }
            if self.result.is_none() {
                return Err(WorkerErrorPayload::invalid_params(
                    "ok response must include result payload",
                ));
            }
        } else {
            if self.result.is_some() {
                return Err(WorkerErrorPayload::invalid_params(
                    "error response must not include result payload",
                ));
            }
            if self.error.is_none() {
                return Err(WorkerErrorPayload::invalid_params(
                    "error response must include error payload",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerErrorPayload {
    pub error_code: String,
    pub message: String,
    pub retryable: bool,
}

impl WorkerErrorPayload {
    pub fn storage_failed(message: impl Into<String>) -> Self {
        Self {
            error_code: ErrorCode::StorageFailed.as_str().to_owned(),
            message: message.into(),
            retryable: true,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            error_code: ErrorCode::InvalidParams.as_str().to_owned(),
            message: message.into(),
            retryable: false,
        }
    }

    pub fn from_server_error(error: &ServerError) -> Self {
        Self {
            error_code: error.error_code().to_owned(),
            message: error.message().to_owned(),
            retryable: error.retryable(),
        }
    }

    pub fn to_server_error(self) -> ServerError {
        match self.error_code.as_str() {
            "permission_denied" => ServerError::permission_denied(self.message),
            "capture_unsupported_on_platform" => {
                ServerError::capture_unsupported_on_platform(self.message)
            }
            "window_not_found" => ServerError::window_not_found(self.message),
            "monitor_not_found" => ServerError::monitor_not_found(self.message),
            "no_capture_yet" => ServerError::no_capture_yet(self.message),
            "invalid_region" => ServerError::invalid_region(self.message),
            "cursor_unavailable" => ServerError::cursor_unavailable(self.message),
            "encode_failed" => ServerError::encode_failed(self.message),
            "invalid_params" => ServerError::invalid_params(self.message),
            "storage_failed" => ServerError::storage_failed(self.message),
            _ => ServerError::storage_failed(format!(
                "unknown worker error code {}: {}",
                self.error_code, self.message
            )),
        }
    }
}

pub fn parse_request_json(input: &str) -> Result<WorkerRequest, WorkerErrorPayload> {
    let request: WorkerRequest = serde_json::from_str(input).map_err(|error| {
        WorkerErrorPayload::invalid_params(format!("failed to decode worker request JSON: {error}"))
    })?;
    request.validate()?;
    Ok(request)
}

pub fn parse_response_json(input: &str) -> Result<WorkerResponse, WorkerErrorPayload> {
    let response: WorkerResponse = serde_json::from_str(input).map_err(|error| {
        WorkerErrorPayload::invalid_params(format!(
            "failed to decode worker response JSON: {error}"
        ))
    })?;
    response.validate()?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_contract_request_json_roundtrip() {
        let request = WorkerRequest {
            v: WORKER_CONTRACT_VERSION,
            request_id: "req-1".to_owned(),
            operation: CaptureOperation::CaptureScreen { monitor_id: None },
            output: WorkerOutputOptions {
                format: WorkerOutputFormat::Png,
                jpeg_quality: 82,
                max_dimension: Some(1024),
            },
            artifact_path: "/tmp/zeuxis-test.png".to_owned(),
        };
        let json = serde_json::to_string(&request).expect("serialize request");
        let parsed = parse_request_json(&json).expect("parse request");
        assert_eq!(parsed, request);
    }

    #[test]
    fn worker_contract_response_json_roundtrip() {
        let response = WorkerResponse::error(
            "req-2",
            WorkerErrorPayload::storage_failed("unsupported mode"),
        );
        let json = serde_json::to_string(&response).expect("serialize response");
        let parsed = parse_response_json(&json).expect("parse response");
        assert_eq!(parsed, response);
    }

    #[test]
    fn worker_contract_request_rejects_version_mismatch() {
        let request = WorkerRequest {
            v: WORKER_CONTRACT_VERSION + 1,
            request_id: "req-3".to_owned(),
            operation: CaptureOperation::CaptureScreen { monitor_id: None },
            output: WorkerOutputOptions {
                format: WorkerOutputFormat::Png,
                jpeg_quality: 82,
                max_dimension: None,
            },
            artifact_path: "/tmp/zeuxis-test.png".to_owned(),
        };
        let json = serde_json::to_string(&request).expect("serialize request");
        let error = parse_request_json(&json).expect_err("version mismatch should fail");
        assert_eq!(error.error_code, "invalid_params");
    }

    #[test]
    fn worker_contract_response_rejects_missing_error_when_not_ok() {
        let response = WorkerResponse {
            v: WORKER_CONTRACT_VERSION,
            request_id: "req-4".to_owned(),
            ok: false,
            result: None,
            error: None,
        };
        let json = serde_json::to_string(&response).expect("serialize response");
        let error = parse_response_json(&json).expect_err("error payload should be required");
        assert_eq!(error.error_code, "invalid_params");
    }

    #[test]
    fn worker_contract_unknown_error_maps_to_storage_failed() {
        let error = WorkerErrorPayload {
            error_code: "nonesuch".to_owned(),
            message: "bad".to_owned(),
            retryable: false,
        };
        assert_eq!(error.to_server_error().error_code(), "storage_failed");
    }
}
