use std::path::Path;

use rmcp::model::{CallToolResult, Content, RawResource};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{capture::backend::MonitorInfo, mcp::errors::ServerError, storage::StoredArtifact};

#[derive(Debug, Serialize)]
pub struct CaptureSuccessPayload {
    pub path: String,
    pub uri: String,
    pub output_format: String,
    pub mime_type: String,
    pub artifact_sha256: String,
    pub artifact_hmac_sha256: Option<String>,
    pub width: u32,
    pub height: u32,
    pub capture_mode: String,
    pub captured_at_utc: String,
}

#[derive(Debug, Serialize)]
pub struct MonitorListPayload {
    pub monitor_count: usize,
    pub monitors: Vec<MonitorInfo>,
    pub listed_at_utc: String,
}

#[derive(Debug, Serialize)]
pub struct CursorPositionPayload {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize)]
pub struct RuntimeDiagnosticsPayload {
    pub os: String,
    pub arch: String,
    pub xdg_session_type: Option<String>,
    pub display: Option<String>,
    pub wayland_display: Option<String>,
    pub permission_ok: bool,
    pub permission_error_code: Option<String>,
    pub permission_message: Option<String>,
    pub monitors_ok: bool,
    pub monitor_count: Option<usize>,
    pub monitors_error_code: Option<String>,
    pub monitors_message: Option<String>,
    pub cursor_ok: bool,
    pub cursor_position: Option<CursorPositionPayload>,
    pub cursor_error_code: Option<String>,
    pub cursor_message: Option<String>,
    pub diagnosed_at_utc: String,
}

pub fn success_result(capture_mode: &str, artifact: &StoredArtifact) -> CallToolResult {
    let payload = CaptureSuccessPayload {
        path: artifact.path.display().to_string(),
        uri: artifact.uri.clone(),
        output_format: artifact.output_format.clone(),
        mime_type: artifact.mime_type.clone(),
        artifact_sha256: artifact.artifact_sha256.clone(),
        artifact_hmac_sha256: artifact.artifact_hmac_sha256.clone(),
        width: artifact.width,
        height: artifact.height,
        capture_mode: capture_mode.to_owned(),
        captured_at_utc: artifact.captured_at_utc.clone(),
    };

    let resource_name = file_name_or_default(&artifact.path, "capture.png");
    let resource_link =
        RawResource::new(payload.uri.clone(), resource_name).with_mime_type(&payload.mime_type);

    let mut tool_result = CallToolResult::success(vec![
        Content::text(format!(
            "Captured {} ({}x{}) to {}",
            payload.capture_mode, payload.width, payload.height, payload.path
        )),
        Content::resource_link(resource_link),
    ]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

pub fn monitors_result(monitors: Vec<MonitorInfo>) -> CallToolResult {
    let payload = MonitorListPayload {
        monitor_count: monitors.len(),
        monitors,
        listed_at_utc: now_rfc3339_utc(),
    };

    let mut tool_result = CallToolResult::success(vec![Content::text(format!(
        "Detected {} monitor(s)",
        payload.monitor_count
    ))]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

pub fn diagnostics_result(payload: RuntimeDiagnosticsPayload) -> CallToolResult {
    let mut tool_result = CallToolResult::success(vec![Content::text(format!(
        "Runtime diagnostics: permission_ok={} monitors_ok={} cursor_ok={}",
        payload.permission_ok, payload.monitors_ok, payload.cursor_ok
    ))]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

pub fn error_result(error: &ServerError) -> CallToolResult {
    let mut tool_result = CallToolResult::error(vec![Content::text(format!(
        "{}: {}",
        error.error_code(),
        error.message()
    ))]);
    tool_result.structured_content = Some(error.structured_content());
    tool_result
}

fn now_rfc3339_utc() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn file_name_or_default(path: &Path, default: &str) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default.to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{capture::backend::MonitorInfo, mcp::errors::ServerError, storage::StoredArtifact};

    use super::*;

    fn sample_artifact(path: &str) -> StoredArtifact {
        StoredArtifact {
            path: PathBuf::from(path),
            uri: format!("file://{path}"),
            output_format: "png".to_owned(),
            mime_type: "image/png".to_owned(),
            artifact_sha256: "aa".repeat(32),
            artifact_hmac_sha256: Some("bb".repeat(32)),
            width: 12,
            height: 8,
            captured_at_utc: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn mcp_result_monitors_result_includes_count_and_list() {
        let result = monitors_result(vec![MonitorInfo {
            id: 1,
            name: "Primary".to_owned(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
            is_builtin: true,
        }]);
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["monitor_count"], 1);
        assert!(structured["listed_at_utc"].is_string());
    }

    #[test]
    fn mcp_result_diagnostics_result_preserves_payload() {
        let payload = RuntimeDiagnosticsPayload {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            xdg_session_type: Some("wayland".to_owned()),
            display: None,
            wayland_display: Some("wayland-0".to_owned()),
            permission_ok: false,
            permission_error_code: Some("permission_denied".to_owned()),
            permission_message: Some("denied".to_owned()),
            monitors_ok: true,
            monitor_count: Some(1),
            monitors_error_code: None,
            monitors_message: None,
            cursor_ok: false,
            cursor_position: None,
            cursor_error_code: Some("cursor_unavailable".to_owned()),
            cursor_message: Some("cursor failed".to_owned()),
            diagnosed_at_utc: "2026-01-01T00:00:00Z".to_owned(),
        };
        let result = diagnostics_result(payload);
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["permission_ok"], false);
        assert_eq!(structured["monitor_count"], 1);
        assert_eq!(structured["cursor_error_code"], "cursor_unavailable");
    }

    #[test]
    fn mcp_result_success_result_falls_back_to_default_resource_name_without_filename() {
        let artifact = sample_artifact("/");
        let result = success_result("capture_screen", &artifact);
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["capture_mode"], "capture_screen");
        assert_eq!(structured["width"], 12);
        assert_eq!(structured["height"], 8);
        assert_eq!(structured["captured_at_utc"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn mcp_result_error_result_marks_tool_error_and_structured_payload() {
        let result = error_result(&ServerError::invalid_params("bad"));
        assert_eq!(result.is_error, Some(true));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["error_code"], "invalid_params");
        assert_eq!(structured["retryable"], false);
    }
}
