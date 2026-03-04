use std::path::Path;

use rmcp::model::{CallToolResult, Content, RawResource};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{mcp::errors::ServerError, storage::StoredArtifact};

#[derive(Debug, Serialize)]
pub struct CaptureSuccessPayload {
    pub path: String,
    pub uri: String,
    pub width: u32,
    pub height: u32,
    pub capture_mode: String,
    pub captured_at_utc: String,
}

pub fn success_result(capture_mode: &str, artifact: &StoredArtifact) -> CallToolResult {
    let payload = CaptureSuccessPayload {
        path: artifact.path.display().to_string(),
        uri: artifact.uri.clone(),
        width: artifact.width,
        height: artifact.height,
        capture_mode: capture_mode.to_owned(),
        captured_at_utc: now_rfc3339_utc(),
    };

    let resource_name = file_name_or_default(&artifact.path, "capture.png");
    let resource_link =
        RawResource::new(payload.uri.clone(), resource_name).with_mime_type("image/png");

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
