//! Builders and payload schemas for MCP `CallToolResult` values.
//!
//! Each tool returns a short text summary plus structured content. Capture
//! results also include a `file://` resource link so MCP clients can inspect the
//! local artifact without any remote upload step.

use std::path::Path;

use rmcp::model::{CallToolResult, Content, RawResource};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    capture::backend::{MonitorInfo, WindowInfo},
    mcp::errors::ServerError,
    storage::StoredArtifact,
};

/// Output settings actually applied to a capture request.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppliedSettingsPayload {
    pub output_mode: String,
    pub output_preset: Option<String>,
    pub jpeg_quality: Option<u8>,
    pub max_dimension: Option<u32>,
    pub delay_seconds_applied: Option<f64>,
}

/// Ratio between requested logical input dimensions and captured source pixels.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourceScaleFactorPayload {
    pub x: f64,
    pub y: f64,
}

/// Rectangle target metadata returned in capture result payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureRectPayload {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub coordinate_space: String,
}

/// Optional monitor, window, or rectangle identity for the captured target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CaptureTargetPayload {
    pub monitor_id: Option<u32>,
    pub window_id: Option<u32>,
    pub rect: Option<CaptureRectPayload>,
}

/// Context needed to interpret a capture artifact after it has been stored.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureContextPayload {
    pub applied_settings: AppliedSettingsPayload,
    pub input_units: String,
    pub input_width: Option<u32>,
    pub input_height: Option<u32>,
    pub source_units: String,
    pub source_width: u32,
    pub source_height: u32,
    pub target: CaptureTargetPayload,
}

/// Structured content for successful capture-like tool responses.
///
/// `capture_mode` names the tool that returned the response; `artifact_capture_mode`
/// names the capture that produced the underlying artifact, which differs for
/// `get_latest_capture`.
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
    pub artifact_capture_mode: String,
    pub captured_at_utc: String,
    pub applied_settings: AppliedSettingsPayload,
    pub input_units: String,
    pub input_width: Option<u32>,
    pub input_height: Option<u32>,
    pub source_units: String,
    pub source_width: u32,
    pub source_height: u32,
    pub source_scale_factor: Option<SourceScaleFactorPayload>,
    pub target: CaptureTargetPayload,
}

/// Structured content for monitor discovery.
#[derive(Debug, Serialize)]
pub struct MonitorListPayload {
    pub monitor_count: usize,
    pub monitors: Vec<MonitorInfo>,
    pub listed_at_utc: String,
}

/// Structured content for window discovery and snapshot-scoped capture IDs.
#[derive(Debug, Serialize)]
pub struct WindowListPayload {
    pub id_scope: String,
    pub snapshot_id: String,
    pub listed_at_utc: String,
    pub window_count: usize,
    pub windows: Vec<WindowInfo>,
}

/// Structured content for session artifact listing.
#[derive(Debug, Serialize)]
pub struct SessionArtifactsPayload {
    pub artifact_count: usize,
    pub listed_at_utc: String,
    pub artifacts: Vec<SessionArtifactPayload>,
}

/// Stored artifact metadata shown by `list_session_artifacts`.
#[derive(Debug, Serialize)]
pub struct SessionArtifactPayload {
    pub artifact_id: String,
    pub capture_mode: String,
    pub path: String,
    pub uri: String,
    pub output_format: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub captured_at_utc: String,
    pub is_latest: bool,
}

/// Current cursor position reported by runtime diagnostics.
#[derive(Debug, Serialize)]
pub struct CursorPositionPayload {
    pub x: i32,
    pub y: i32,
}

/// Readiness report for permissions, monitor discovery, and cursor access.
#[derive(Debug, Serialize)]
pub struct RuntimeDiagnosticsPayload {
    pub os: String,
    pub arch: String,
    pub xdg_session_type: Option<String>,
    pub display: Option<String>,
    pub wayland_display: Option<String>,
    pub permission_checked: bool,
    pub permission_check_mode: String,
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

/// Result payload for deleting artifacts created in the current server session.
#[derive(Debug, Serialize)]
pub struct ClearSessionArtifactsPayload {
    pub deleted_artifact_count: usize,
    pub cleared_at_utc: String,
}

/// Builds the structured MCP result and resource link for a stored capture artifact.
pub fn success_result(
    capture_mode: &str,
    artifact: &StoredArtifact,
    context: &CaptureContextPayload,
) -> CallToolResult {
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
        artifact_capture_mode: artifact.capture_mode.clone(),
        captured_at_utc: artifact.captured_at_utc.clone(),
        applied_settings: context.applied_settings.clone(),
        input_units: context.input_units.clone(),
        input_width: context.input_width,
        input_height: context.input_height,
        source_units: context.source_units.clone(),
        source_width: context.source_width,
        source_height: context.source_height,
        source_scale_factor: source_scale_factor(context),
        target: context.target.clone(),
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

/// Builds the structured MCP result for monitor discovery.
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

/// Builds the structured MCP result for a window snapshot.
pub fn windows_result(
    windows: Vec<WindowInfo>,
    snapshot_id: String,
    id_scope: String,
    listed_at_utc: String,
) -> CallToolResult {
    let payload = WindowListPayload {
        id_scope,
        snapshot_id,
        listed_at_utc,
        window_count: windows.len(),
        windows,
    };

    let mut tool_result = CallToolResult::success(vec![Content::text(format!(
        "Detected {} window(s)",
        payload.window_count
    ))]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

/// Builds the structured MCP result for session artifact listing.
pub fn list_session_artifacts_result(
    artifacts: Vec<StoredArtifact>,
    latest_artifact_id: Option<String>,
) -> CallToolResult {
    let latest = latest_artifact_id.as_deref();
    let items = artifacts
        .iter()
        .map(|artifact| SessionArtifactPayload {
            artifact_id: artifact.artifact_id.clone(),
            capture_mode: artifact.capture_mode.clone(),
            path: artifact.path.display().to_string(),
            uri: artifact.uri.clone(),
            output_format: artifact.output_format.clone(),
            mime_type: artifact.mime_type.clone(),
            width: artifact.width,
            height: artifact.height,
            captured_at_utc: artifact.captured_at_utc.clone(),
            is_latest: latest == Some(artifact.artifact_id.as_str()),
        })
        .collect::<Vec<_>>();

    let payload = SessionArtifactsPayload {
        artifact_count: items.len(),
        listed_at_utc: now_rfc3339_utc(),
        artifacts: items,
    };

    let mut tool_result = CallToolResult::success(vec![Content::text(format!(
        "Found {} session artifact(s)",
        payload.artifact_count
    ))]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

/// Builds the structured MCP result for runtime diagnostics.
pub fn diagnostics_result(payload: RuntimeDiagnosticsPayload) -> CallToolResult {
    let mut tool_result = CallToolResult::success(vec![Content::text(format!(
        "Runtime diagnostics: permission_ok={} monitors_ok={} cursor_ok={}",
        payload.permission_ok, payload.monitors_ok, payload.cursor_ok
    ))]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

/// Builds the structured MCP result for clearing session artifacts.
pub fn clear_session_artifacts_result(deleted_artifact_count: usize) -> CallToolResult {
    let payload = ClearSessionArtifactsPayload {
        deleted_artifact_count,
        cleared_at_utc: now_rfc3339_utc(),
    };

    let mut tool_result = CallToolResult::success(vec![Content::text(format!(
        "Cleared {deleted_artifact_count} session artifact(s)"
    ))]);
    tool_result.structured_content =
        Some(serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({})));
    tool_result
}

/// Builds an MCP tool error while preserving Zeuxis error code and retryability.
pub fn error_result(error: &ServerError) -> CallToolResult {
    let mut tool_result = CallToolResult::error(vec![Content::text(format!(
        "{}: {}",
        error.error_code(),
        error.message()
    ))]);
    tool_result.structured_content = Some(error.structured_content());
    tool_result
}

fn source_scale_factor(context: &CaptureContextPayload) -> Option<SourceScaleFactorPayload> {
    if context.source_width == 0 || context.source_height == 0 {
        return None;
    }

    let input_width = context.input_width.unwrap_or(context.source_width);
    let input_height = context.input_height.unwrap_or(context.source_height);
    if input_width == 0 || input_height == 0 {
        return None;
    }

    Some(SourceScaleFactorPayload {
        x: context.source_width as f64 / input_width as f64,
        y: context.source_height as f64 / input_height as f64,
    })
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

    use crate::{
        capture::backend::{MonitorInfo, WindowInfo},
        mcp::errors::ServerError,
        storage::StoredArtifact,
    };

    use super::*;

    fn sample_artifact(path: &str) -> StoredArtifact {
        StoredArtifact {
            artifact_id: "a1".to_owned(),
            capture_mode: "capture_screen".to_owned(),
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

    fn sample_context() -> CaptureContextPayload {
        CaptureContextPayload {
            applied_settings: AppliedSettingsPayload {
                output_mode: "preset".to_owned(),
                output_preset: Some("analysis".to_owned()),
                jpeg_quality: None,
                max_dimension: Some(2560),
                delay_seconds_applied: Some(0.5),
            },
            input_units: "points".to_owned(),
            input_width: Some(12),
            input_height: Some(8),
            source_units: "pixels".to_owned(),
            source_width: 24,
            source_height: 16,
            target: CaptureTargetPayload {
                monitor_id: Some(1),
                window_id: None,
                rect: None,
            },
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
    fn mcp_result_windows_result_includes_snapshot_metadata() {
        let result = windows_result(
            vec![WindowInfo {
                id: 3,
                title: "Editor".to_owned(),
                app: "Code".to_owned(),
                x: 10,
                y: 20,
                width: 800,
                height: 600,
                is_focused: true,
                is_minimized: false,
            }],
            "snap-1".to_owned(),
            "snapshot".to_owned(),
            "2026-01-01T00:00:00Z".to_owned(),
        );
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["window_count"], 1);
        assert_eq!(structured["snapshot_id"], "snap-1");
        assert_eq!(structured["id_scope"], "snapshot");
    }

    #[test]
    fn mcp_result_diagnostics_result_preserves_payload() {
        let payload = RuntimeDiagnosticsPayload {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            xdg_session_type: Some("wayland".to_owned()),
            display: None,
            wayland_display: Some("wayland-0".to_owned()),
            permission_checked: false,
            permission_check_mode: "best_effort_unchecked".to_owned(),
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
    fn mcp_result_success_result_includes_units_and_scale_factor() {
        let artifact = sample_artifact("/");
        let result = success_result("capture_screen", &artifact, &sample_context());
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["capture_mode"], "capture_screen");
        assert_eq!(structured["artifact_capture_mode"], "capture_screen");
        assert_eq!(structured["input_units"], "points");
        assert_eq!(structured["source_units"], "pixels");
        assert_eq!(structured["source_scale_factor"]["x"], 2.0);
        assert_eq!(structured["source_scale_factor"]["y"], 2.0);
    }

    #[test]
    fn mcp_result_success_result_falls_back_to_unity_scale_without_input_dimensions() {
        let artifact = sample_artifact("/");
        let mut context = sample_context();
        context.input_width = None;
        context.input_height = None;

        let result = success_result("capture_screen", &artifact, &context);
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["source_scale_factor"]["x"], 1.0);
        assert_eq!(structured["source_scale_factor"]["y"], 1.0);
    }

    #[test]
    fn mcp_result_list_session_artifacts_marks_latest_entry() {
        let mut first = sample_artifact("/tmp/a.png");
        first.artifact_id = "first".to_owned();
        let mut latest = sample_artifact("/tmp/b.png");
        latest.artifact_id = "latest".to_owned();

        let result = list_session_artifacts_result(vec![latest, first], Some("latest".to_owned()));
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["artifact_count"], 2);
        assert_eq!(structured["artifacts"][0]["is_latest"], true);
    }

    #[test]
    fn mcp_result_clear_session_artifacts_includes_deleted_count() {
        let result = clear_session_artifacts_result(3);
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.expect("structured");
        assert_eq!(structured["deleted_artifact_count"], 3);
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
