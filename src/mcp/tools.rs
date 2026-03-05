use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{error, info};

use crate::{
    capture::{
        backend::{CaptureBackend, MonitorInfo, WindowInfo},
        region::{GlobalRect, center_square_on_cursor, rect_contains_point},
    },
    cursor::CursorProvider,
    mcp::{errors::ServerError, result},
    storage::{CaptureOutputFormat, CaptureOutputOptions},
    worker::{
        contract::{
            CaptureOperation, WORKER_CONTRACT_VERSION, WorkerOutputFormat, WorkerOutputOptions,
            WorkerRequest,
        },
        parent::run_worker_capture,
    },
};

use super::server::{CaptureExecutionMode, ZeuxisScreenshotServer};

const MAX_DELAY_SECONDS: f64 = 30.0;
const MAX_DELAY_MILLISECONDS: i64 = 30_000;
const MAX_CAPTURE_DIMENSION: u32 = 16_384;
const MAX_CAPTURE_PIXELS: u64 = 40_000_000;
const DEFAULT_ANALYSIS_MAX_DIMENSION: u32 = 2_560;
const DEFAULT_COMPACT_MAX_DIMENSION: u32 = 1_600;
const DEFAULT_JPEG_QUALITY: u8 = 82;
const DEFAULT_COMPACT_JPEG_QUALITY: u8 = 85;
const MIN_JPEG_QUALITY: i64 = 40;
const MAX_JPEG_QUALITY: i64 = 95;
const MIN_OUTPUT_MAX_DIMENSION: i64 = 256;
const MAX_OUTPUT_MAX_DIMENSION: i64 = 8_192;
const INPUT_UNITS_POINTS: &str = "points";
const INPUT_UNITS_NONE: &str = "none";
const SOURCE_UNITS_PIXELS: &str = "pixels";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputPreset {
    /// Balanced default for LLM analysis: PNG with moderate downscaling.
    #[default]
    Analysis,
    /// Keep original size and PNG fidelity.
    Exact,
    /// Smaller artifacts for faster transfer: JPEG with stronger downscaling.
    Compact,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Png,
    Jpeg,
    Webp,
}

impl OutputFormat {
    const fn to_storage(self) -> CaptureOutputFormat {
        match self {
            Self::Png => CaptureOutputFormat::Png,
            Self::Jpeg => CaptureOutputFormat::Jpeg,
            Self::Webp => CaptureOutputFormat::Webp,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    Preset,
    Custom,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct OutputParams {
    pub mode: OutputMode,
    /// Used only when mode is preset.
    pub preset: Option<OutputPreset>,
    /// Used only when mode is custom.
    pub format: Option<OutputFormat>,
    /// Used only when mode is custom and format is jpeg.
    #[schemars(range(min = MIN_JPEG_QUALITY, max = MAX_JPEG_QUALITY))]
    pub jpeg_quality: Option<i64>,
    /// Used only when mode is custom.
    #[schemars(range(min = MIN_OUTPUT_MAX_DIMENSION, max = MAX_OUTPUT_MAX_DIMENSION))]
    pub max_dimension: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum OutputInput {
    /// Shorthand preset alias, e.g. "analysis", "exact", or "compact".
    Preset(OutputPreset),
    /// Full output object form for preset/custom modes.
    Detailed(OutputParams),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedOutputSettings {
    mode: OutputMode,
    preset: Option<OutputPreset>,
    format: CaptureOutputFormat,
    jpeg_quality: u8,
    max_dimension: Option<u32>,
}

impl ResolvedOutputSettings {
    fn applied_settings(self, delay: Option<Duration>) -> result::AppliedSettingsPayload {
        result::AppliedSettingsPayload {
            output_mode: output_mode_as_str(self.mode).to_owned(),
            output_preset: self.preset.map(output_preset_as_str).map(ToOwned::to_owned),
            jpeg_quality: (self.format == CaptureOutputFormat::Jpeg).then_some(self.jpeg_quality),
            max_dimension: self.max_dimension,
            delay_seconds_applied: delay.map(|value| value.as_secs_f64()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ParsedCommonParams {
    delay: Option<Duration>,
    output: ResolvedOutputSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct CommonCaptureParams {
    /// Optional delay before capture in milliseconds (0..=30000). Prefer this for deterministic clients.
    #[schemars(range(min = 0, max = MAX_DELAY_MILLISECONDS))]
    pub delay_ms: Option<i64>,
    /// Optional delay before capture in seconds (0..=30).
    #[schemars(range(min = 0.0, max = MAX_DELAY_SECONDS))]
    pub delay_seconds: Option<f64>,
    /// Play a shutter sound after a successful capture.
    pub play_sound: Option<bool>,
    /// Output options. Accepts shorthand preset string or detailed object.
    /// If omitted, the analysis preset is used.
    pub output: Option<OutputInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct ListMonitorsParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct ListWindowsParams {
    /// Return only focused windows.
    pub focused_only: Option<bool>,
    /// Include windows that look like system/UI chrome surfaces.
    pub include_system_windows: Option<bool>,
    /// Case-insensitive substring filter on app name.
    pub app_contains: Option<String>,
    /// Case-insensitive substring filter on window title.
    pub title_contains: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct GetRuntimeDiagnosticsParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct GetLatestCaptureParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct ClearSessionArtifactsParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct ListSessionArtifactsParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct CaptureScreenParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Monitor id from list_monitors. Omit to use the primary monitor.
    pub monitor_id: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureWindowParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Snapshot id from list_windows.
    pub snapshot_id: String,
    /// Window id from list_windows.
    pub window_id: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct CaptureCursorWindowParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Include windows that look like system/UI chrome surfaces when resolving cursor target.
    pub include_system_windows: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureCursorRegionParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Square region size in logical desktop points.
    #[schemars(range(min = 1, max = MAX_CAPTURE_DIMENSION))]
    pub size: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureRectParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Global X coordinate in logical desktop points (top-left).
    pub x: i64,
    /// Global Y coordinate in logical desktop points (top-left).
    pub y: i64,
    /// Rectangle width in logical desktop points.
    #[schemars(range(min = 1, max = MAX_CAPTURE_DIMENSION))]
    pub width: i64,
    /// Rectangle height in logical desktop points.
    #[schemars(range(min = 1, max = MAX_CAPTURE_DIMENSION))]
    pub height: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureMonitorRegionParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Monitor id from list_monitors.
    pub monitor_id: u32,
    /// Monitor-local X coordinate in logical desktop points (top-left).
    #[schemars(range(min = 0))]
    pub x: i64,
    /// Monitor-local Y coordinate in logical desktop points (top-left).
    #[schemars(range(min = 0))]
    pub y: i64,
    /// Rectangle width in logical desktop points.
    #[schemars(range(min = 1, max = MAX_CAPTURE_DIMENSION))]
    pub width: i64,
    /// Rectangle height in logical desktop points.
    #[schemars(range(min = 1, max = MAX_CAPTURE_DIMENSION))]
    pub height: i64,
}

struct CaptureWorkOutput {
    image: image::RgbaImage,
    target: result::CaptureTargetPayload,
    input_units: String,
    input_width: Option<u32>,
    input_height: Option<u32>,
}

#[tool_router(router = tool_router)]
impl ZeuxisScreenshotServer {
    #[tool(
        name = "list_monitors",
        description = "Discover available monitors. Use ids with capture_screen.monitor_id and capture_monitor_region.monitor_id.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn list_monitors(
        &self,
        _params: Parameters<ListMonitorsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        info!(
            tool = "list_monitors",
            phase = "start",
            "tool invocation started"
        );

        if let Err(error) = self.permission_gate.ensure_capture_allowed() {
            error!(
                tool = "list_monitors",
                phase = "permission_error",
                error_code = error.error_code(),
                message = error.message(),
                "tool invocation failed"
            );
            return Ok(result::error_result(&error));
        }

        let backend = Arc::clone(&self.backend);
        let timeout = self.blocking_task_timeout;
        let timeout_error_message =
            format!("monitor listing timed out after {}ms", timeout.as_millis());
        let monitors = match run_blocking_with_timeout(
            timeout,
            timeout_error_message,
            "monitor listing worker task failed",
            move || backend.list_monitors(),
        )
        .await
        {
            Ok(monitors) => monitors,
            Err(error) => {
                error!(
                    tool = "list_monitors",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        info!(
            tool = "list_monitors",
            phase = "complete",
            monitor_count = monitors.len(),
            "tool invocation completed"
        );
        Ok(result::monitors_result(monitors))
    }

    #[tool(
        name = "list_windows",
        description = "Discover available windows with deterministic ids for capture_window.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn list_windows(
        &self,
        params: Parameters<ListWindowsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        info!(
            tool = "list_windows",
            phase = "start",
            "tool invocation started"
        );

        if let Err(error) = self.permission_gate.ensure_capture_allowed() {
            error!(
                tool = "list_windows",
                phase = "permission_error",
                error_code = error.error_code(),
                message = error.message(),
                "tool invocation failed"
            );
            return Ok(result::error_result(&error));
        }

        let backend = Arc::clone(&self.backend);
        let timeout = self.blocking_task_timeout;
        let timeout_error_message =
            format!("window listing timed out after {}ms", timeout.as_millis());
        let windows = match run_blocking_with_timeout(
            timeout,
            timeout_error_message,
            "window listing worker task failed",
            move || backend.list_windows(),
        )
        .await
        {
            Ok(windows) => windows,
            Err(error) => {
                error!(
                    tool = "list_windows",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        let filtered = filter_windows(windows, &params.0);
        let listed_at_utc = now_rfc3339_utc();
        let snapshot_id = next_windows_snapshot_id();
        let id_scope = "snapshot".to_owned();

        if let Ok(mut state) = self.last_window_snapshot.lock() {
            *state = Some(super::server::WindowSnapshotState {
                snapshot_id: snapshot_id.clone(),
                id_scope: id_scope.clone(),
                listed_at_utc: listed_at_utc.clone(),
                windows: filtered.clone(),
            });
        }

        info!(
            tool = "list_windows",
            phase = "complete",
            window_count = filtered.len(),
            snapshot_id = %snapshot_id,
            "tool invocation completed"
        );
        Ok(result::windows_result(
            filtered,
            snapshot_id,
            id_scope,
            listed_at_utc,
        ))
    }

    #[tool(
        name = "get_runtime_diagnostics",
        description = "Report capture readiness and session context. Use first when Linux capture is unreliable.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn get_runtime_diagnostics(
        &self,
        _params: Parameters<GetRuntimeDiagnosticsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        info!(
            tool = "get_runtime_diagnostics",
            phase = "start",
            "tool invocation started"
        );

        let permission_result = self.permission_gate.ensure_capture_allowed();

        let backend = Arc::clone(&self.backend);
        let timeout = self.blocking_task_timeout;
        let monitor_timeout_error_message =
            format!("monitor listing timed out after {}ms", timeout.as_millis());
        let monitor_result = run_blocking_with_timeout(
            timeout,
            monitor_timeout_error_message,
            "monitor listing worker task failed",
            move || backend.list_monitors(),
        )
        .await;

        let cursor_result = self.cursor_provider.cursor_position();

        let (permission_ok, permission_error_code, permission_message) =
            status_from_result(permission_result);
        let (permission_checked, permission_check_mode) = permission_check_metadata();
        let (monitors_ok, monitors_error_code, monitors_message, monitor_count) =
            match monitor_result {
                Ok(monitors) => (true, None, None, Some(monitors.len())),
                Err(error) => (
                    false,
                    Some(error.error_code().to_owned()),
                    Some(error.message().to_owned()),
                    None,
                ),
            };
        let (cursor_ok, cursor_error_code, cursor_message, cursor_position) = match cursor_result {
            Ok(cursor) => (
                true,
                None,
                None,
                Some(result::CursorPositionPayload {
                    x: cursor.x,
                    y: cursor.y,
                }),
            ),
            Err(error) => (
                false,
                Some(error.error_code().to_owned()),
                Some(error.message().to_owned()),
                None,
            ),
        };

        let payload = result::RuntimeDiagnosticsPayload {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            xdg_session_type: env_non_empty("XDG_SESSION_TYPE"),
            display: env_non_empty("DISPLAY"),
            wayland_display: env_non_empty("WAYLAND_DISPLAY"),
            permission_checked,
            permission_check_mode: permission_check_mode.to_owned(),
            permission_ok,
            permission_error_code,
            permission_message,
            monitors_ok,
            monitor_count,
            monitors_error_code,
            monitors_message,
            cursor_ok,
            cursor_position,
            cursor_error_code,
            cursor_message,
            diagnosed_at_utc: now_rfc3339_utc(),
        };

        info!(
            tool = "get_runtime_diagnostics",
            phase = "complete",
            permission_checked = payload.permission_checked,
            permission_check_mode = %payload.permission_check_mode,
            permission_ok = payload.permission_ok,
            monitors_ok = payload.monitors_ok,
            monitor_count = ?payload.monitor_count,
            cursor_ok = payload.cursor_ok,
            os = %payload.os,
            session = ?payload.xdg_session_type,
            "tool invocation completed"
        );

        Ok(result::diagnostics_result(payload))
    }

    #[tool(
        name = "get_latest_capture",
        description = "Return the latest screenshot artifact captured during this server session without taking a new capture.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn get_latest_capture(
        &self,
        _params: Parameters<GetLatestCaptureParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let started_at = Instant::now();
        info!(
            tool = "get_latest_capture",
            phase = "start",
            "tool invocation started"
        );

        let storage = Arc::clone(&self.storage);
        let timeout = self.blocking_task_timeout;
        let timeout_error_message = format!(
            "latest capture lookup timed out after {}ms",
            timeout.as_millis()
        );
        let artifact = match run_blocking_with_timeout(
            timeout,
            timeout_error_message,
            "latest capture worker task failed",
            move || storage.latest_artifact(),
        )
        .await
        {
            Ok(artifact) => artifact,
            Err(error) => {
                error!(
                    tool = "get_latest_capture",
                    phase = "error",
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        let context = self
            .last_capture_context
            .lock()
            .ok()
            .and_then(|state| state.clone())
            .unwrap_or_else(|| result::CaptureContextPayload {
                applied_settings: result::AppliedSettingsPayload {
                    output_mode: "unknown".to_owned(),
                    output_preset: None,
                    jpeg_quality: (artifact.output_format == "jpeg")
                        .then_some(DEFAULT_JPEG_QUALITY),
                    max_dimension: None,
                    delay_seconds_applied: None,
                },
                input_units: INPUT_UNITS_NONE.to_owned(),
                input_width: None,
                input_height: None,
                source_units: SOURCE_UNITS_PIXELS.to_owned(),
                source_width: artifact.width,
                source_height: artifact.height,
                target: result::CaptureTargetPayload::default(),
            });

        info!(
            tool = "get_latest_capture",
            phase = "complete",
            elapsed_ms = started_at.elapsed().as_millis(),
            path = %artifact.path.display(),
            mime_type = %artifact.mime_type,
            width = artifact.width,
            height = artifact.height,
            "tool invocation completed"
        );

        Ok(result::success_result(
            "get_latest_capture",
            &artifact,
            &context,
        ))
    }

    #[tool(
        name = "clear_session_artifacts",
        description = "Delete screenshot artifacts created during this Zeuxis server session.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false
        )
    )]
    pub async fn clear_session_artifacts(
        &self,
        _params: Parameters<ClearSessionArtifactsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        info!(
            tool = "clear_session_artifacts",
            phase = "start",
            "tool invocation started"
        );

        let storage = Arc::clone(&self.storage);
        let timeout = self.blocking_task_timeout;
        let timeout_error_message = format!(
            "clear session artifacts timed out after {}ms",
            timeout.as_millis()
        );
        let deleted = match run_blocking_with_timeout(
            timeout,
            timeout_error_message,
            "clear session artifacts worker task failed",
            move || storage.clear_session_artifacts(),
        )
        .await
        {
            Ok(deleted) => deleted,
            Err(error) => {
                error!(
                    tool = "clear_session_artifacts",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        if let Ok(mut state) = self.last_capture_context.lock() {
            *state = None;
        }

        info!(
            tool = "clear_session_artifacts",
            phase = "complete",
            deleted_artifact_count = deleted,
            "tool invocation completed"
        );

        Ok(result::clear_session_artifacts_result(deleted))
    }

    #[tool(
        name = "list_session_artifacts",
        description = "List screenshot artifacts created during this Zeuxis server session.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn list_session_artifacts(
        &self,
        _params: Parameters<ListSessionArtifactsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        info!(
            tool = "list_session_artifacts",
            phase = "start",
            "tool invocation started"
        );

        let storage = Arc::clone(&self.storage);
        let timeout = self.blocking_task_timeout;
        let timeout_error_message = format!(
            "session artifact listing timed out after {}ms",
            timeout.as_millis()
        );
        let artifacts = match run_blocking_with_timeout(
            timeout,
            timeout_error_message,
            "session artifact listing worker task failed",
            move || storage.list_session_artifacts(),
        )
        .await
        {
            Ok(artifacts) => artifacts,
            Err(error) => {
                error!(
                    tool = "list_session_artifacts",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        let latest_storage = Arc::clone(&self.storage);
        let latest_timeout_error_message = format!(
            "latest artifact lookup timed out after {}ms",
            timeout.as_millis()
        );
        let latest_artifact_id = match run_blocking_with_timeout(
            timeout,
            latest_timeout_error_message,
            "latest artifact worker task failed",
            move || latest_storage.latest_artifact(),
        )
        .await
        {
            Ok(latest) => Some(latest.artifact_id),
            Err(error) if error.error_code() == "no_capture_yet" => None,
            Err(error) => {
                error!(
                    tool = "list_session_artifacts",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        info!(
            tool = "list_session_artifacts",
            phase = "complete",
            artifact_count = artifacts.len(),
            "tool invocation completed"
        );
        Ok(result::list_session_artifacts_result(
            artifacts,
            latest_artifact_id,
        ))
    }

    #[tool(
        name = "capture_screen",
        description = "Capture a full monitor. Best default when user asks to see their screen.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_screen(
        &self,
        params: Parameters<CaptureScreenParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = params.0;
        Ok(self
            .execute_capture(
                "capture_screen",
                input.common,
                CaptureOperation::CaptureScreen {
                    monitor_id: input.monitor_id,
                },
            )
            .await)
    }

    #[tool(
        name = "capture_active_window",
        description = "Capture the focused (non-minimized) window only.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_active_window(
        &self,
        params: Parameters<CommonCaptureParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(self
            .execute_capture(
                "capture_active_window",
                params.0,
                CaptureOperation::CaptureActiveWindow,
            )
            .await)
    }

    #[tool(
        name = "capture_cursor_window",
        description = "Capture the non-system window under the cursor. Set include_system_windows=true to allow system/UI chrome surfaces.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_cursor_window(
        &self,
        params: Parameters<CaptureCursorWindowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = params.0;
        Ok(self
            .execute_capture(
                "capture_cursor_window",
                input.common,
                CaptureOperation::CaptureCursorWindow {
                    include_system_windows: input.include_system_windows.unwrap_or(false),
                },
            )
            .await)
    }

    #[tool(
        name = "capture_window",
        description = "Capture a specific window by window_id from list_windows.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_window(
        &self,
        params: Parameters<CaptureWindowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = params.0;
        if let Err(error) =
            self.validate_window_capture_request(&input.snapshot_id, input.window_id)
        {
            return Ok(result::error_result(&error));
        }
        let window_id = input.window_id;
        Ok(self
            .execute_capture(
                "capture_window",
                input.common,
                CaptureOperation::CaptureWindow { window_id },
            )
            .await)
    }

    #[tool(
        name = "capture_cursor_region",
        description = "Capture a square region centered on the cursor.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_cursor_region(
        &self,
        params: Parameters<CaptureCursorRegionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = params.0;
        let size = match to_u32_positive(input.size, "size") {
            Ok(value) => value,
            Err(error) => return Ok(result::error_result(&error)),
        };
        if let Err(error) = validate_capture_dimensions(size, size) {
            return Ok(result::error_result(&error));
        }

        Ok(self
            .execute_capture(
                "capture_cursor_region",
                input.common,
                CaptureOperation::CaptureCursorRegion { size },
            )
            .await)
    }

    #[tool(
        name = "capture_rect",
        description = "Capture an exact global rectangle (x, y, width, height).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_rect(
        &self,
        params: Parameters<CaptureRectParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = params.0;
        let rect = match parse_rect_input(&input) {
            Ok(rect) => rect,
            Err(error) => return Ok(result::error_result(&error)),
        };

        Ok(self
            .execute_capture(
                "capture_rect",
                input.common,
                CaptureOperation::CaptureRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                },
            )
            .await)
    }

    #[tool(
        name = "capture_monitor_region",
        description = "Capture a monitor-local rectangle with monitor_id + x/y/width/height.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_monitor_region(
        &self,
        params: Parameters<CaptureMonitorRegionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = params.0;
        let (x, y, width, height) = match parse_monitor_region_input(&input) {
            Ok(values) => values,
            Err(error) => return Ok(result::error_result(&error)),
        };
        if let Err(error) = validate_capture_dimensions(width, height) {
            return Ok(result::error_result(&error));
        }

        let monitor_id = input.monitor_id;
        Ok(self
            .execute_capture(
                "capture_monitor_region",
                input.common,
                CaptureOperation::CaptureMonitorRegion {
                    monitor_id,
                    x,
                    y,
                    width,
                    height,
                },
            )
            .await)
    }

    async fn execute_capture(
        &self,
        capture_mode: &'static str,
        common: CommonCaptureParams,
        operation: CaptureOperation,
    ) -> CallToolResult {
        let started_at = Instant::now();
        let requested_delay_ms = common.delay_ms;
        let requested_delay_seconds = common.delay_seconds;
        let play_sound = common.play_sound.unwrap_or(false);

        info!(
            tool = capture_mode,
            phase = "start",
            delay_ms = ?requested_delay_ms,
            delay_seconds = ?requested_delay_seconds,
            play_sound,
            "tool invocation started"
        );

        let parsed_common = match parse_common_params(&common) {
            Ok(parsed_common) => parsed_common,
            Err(error) => {
                error!(
                    tool = capture_mode,
                    phase = "validation_error",
                    delay_ms = ?requested_delay_ms,
                    delay_seconds = ?requested_delay_seconds,
                    play_sound,
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "input validation failed"
                );
                return result::error_result(&error);
            }
        };
        let applied_delay_ms = parsed_common.delay.as_ref().map(Duration::as_millis);

        if let Some(delay) = parsed_common.delay {
            tokio::time::sleep(delay).await;
        }

        if let Err(error) = self.permission_gate.ensure_capture_allowed() {
            error!(
                tool = capture_mode,
                phase = "permission_error",
                delay_ms = ?requested_delay_ms,
                delay_seconds = ?requested_delay_seconds,
                play_sound,
                elapsed_ms = started_at.elapsed().as_millis(),
                error_code = error.error_code(),
                message = error.message(),
                "permission gate failed"
            );
            return result::error_result(&error);
        }

        let backend = Arc::clone(&self.backend);
        let cursor_provider = Arc::clone(&self.cursor_provider);
        let storage = Arc::clone(&self.storage);
        let output = parsed_common.output;
        let timeout = self.blocking_task_timeout;
        let timeout_message = format!("capture timed out after {}ms", timeout.as_millis());
        let blocking_phase_started = Instant::now();

        let permit =
            match tokio::time::timeout(timeout, self.capture_slots.clone().acquire_owned()).await {
                Ok(Ok(permit)) => permit,
                Ok(Err(acquire_error)) => {
                    let error = ServerError::storage_failed(format!(
                        "capture slot coordination failed: {acquire_error}"
                    ));
                    error!(
                        tool = capture_mode,
                        phase = "capture_error",
                        delay_ms = ?requested_delay_ms,
                        delay_seconds = ?requested_delay_seconds,
                        play_sound,
                        elapsed_ms = started_at.elapsed().as_millis(),
                        error_code = error.error_code(),
                        message = error.message(),
                        "capture failed"
                    );
                    return result::error_result(&error);
                }
                Err(_) => {
                    let error = ServerError::storage_failed(format!(
                        "capture slot acquisition timed out after {}ms",
                        timeout.as_millis()
                    ));
                    error!(
                        tool = capture_mode,
                        phase = "capture_error",
                        delay_ms = ?requested_delay_ms,
                        delay_seconds = ?requested_delay_seconds,
                        play_sound,
                        elapsed_ms = started_at.elapsed().as_millis(),
                        error_code = error.error_code(),
                        message = error.message(),
                        "capture failed"
                    );
                    return result::error_result(&error);
                }
            };

        let elapsed_before_worker = blocking_phase_started.elapsed();
        let worker_timeout = timeout.saturating_sub(elapsed_before_worker);
        if worker_timeout.is_zero() {
            let error = ServerError::storage_failed(timeout_message.clone());
            error!(
                tool = capture_mode,
                phase = "capture_error",
                delay_ms = ?requested_delay_ms,
                delay_seconds = ?requested_delay_seconds,
                play_sound,
                elapsed_ms = started_at.elapsed().as_millis(),
                error_code = error.error_code(),
                message = error.message(),
                "capture failed"
            );
            return result::error_result(&error);
        }

        let deadline = std::time::Instant::now() + worker_timeout;
        let timeout_message_for_worker = timeout_message.clone();
        let capture_result = match self.capture_execution_mode {
            CaptureExecutionMode::Inline => {
                let operation = operation.clone();
                run_blocking_with_timeout(
                    worker_timeout,
                    timeout_message,
                    "capture worker task failed",
                    move || {
                        let _permit = permit;
                        if std::time::Instant::now() > deadline {
                            return Err(ServerError::storage_failed(
                                timeout_message_for_worker.clone(),
                            ));
                        }
                        let work_output =
                            run_capture_operation_inline(&*backend, &*cursor_provider, &operation)?;
                        let source_width = work_output.image.width();
                        let source_height = work_output.image.height();
                        let image = match output.max_dimension {
                            Some(max_dimension) => {
                                downscale_if_needed(work_output.image, max_dimension)
                            }
                            None => work_output.image,
                        };
                        if std::time::Instant::now() > deadline {
                            return Err(ServerError::storage_failed(
                                timeout_message_for_worker.clone(),
                            ));
                        }
                        let artifact = storage.write_image(
                            image,
                            capture_mode,
                            CaptureOutputOptions {
                                format: output.format,
                                jpeg_quality: output.jpeg_quality,
                            },
                        )?;
                        Ok((
                            artifact,
                            source_width,
                            source_height,
                            work_output.target,
                            work_output.input_units,
                            work_output.input_width,
                            work_output.input_height,
                        ))
                    },
                )
                .await
            }
            CaptureExecutionMode::SubprocessWorker => {
                let worker_executable = match self.worker_executable.clone() {
                    Some(path) => path,
                    None => {
                        return result::error_result(&ServerError::storage_failed(
                            "capture worker executable path is unavailable",
                        ));
                    }
                };
                let output_for_request = WorkerOutputOptions {
                    format: WorkerOutputFormat::from(output.format),
                    jpeg_quality: output.jpeg_quality,
                    max_dimension: output.max_dimension,
                };
                let artifact_path = self.create_worker_artifact_path(
                    capture_mode,
                    output_for_request.format.file_suffix(),
                );
                let request_id = next_worker_request_id();
                let request = WorkerRequest {
                    v: WORKER_CONTRACT_VERSION,
                    request_id: request_id.clone(),
                    operation: operation.clone(),
                    output: output_for_request.clone(),
                    artifact_path: artifact_path.display().to_string(),
                };

                let worker_result = match run_worker_capture(
                    &worker_executable,
                    &request,
                    worker_timeout,
                    self.worker_kill_grace,
                    self.max_worker_stdout_bytes,
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = cleanup_worker_artifact_path(&artifact_path);
                        return result::error_result(&error);
                    }
                };

                let now = std::time::Instant::now();
                if now > deadline {
                    let _ = cleanup_worker_artifact_path(Path::new(&worker_result.artifact_path));
                    Err(ServerError::storage_failed(
                        timeout_message_for_worker.clone(),
                    ))
                } else {
                    let remaining = deadline.saturating_duration_since(now);
                    let storage = Arc::clone(&storage);
                    let artifact_path = PathBuf::from(worker_result.artifact_path.clone());
                    let output_options = CaptureOutputOptions {
                        format: output_for_request.format.to_storage(),
                        jpeg_quality: output_for_request.jpeg_quality,
                    };
                    let adopted = run_blocking_with_timeout(
                        remaining,
                        timeout_message_for_worker.clone(),
                        "capture adopt worker task failed",
                        move || storage.adopt_artifact(artifact_path, capture_mode, output_options),
                    )
                    .await;

                    match adopted {
                        Ok(artifact) => Ok((
                            artifact,
                            worker_result.source_width,
                            worker_result.source_height,
                            worker_result.target,
                            worker_result.input_units,
                            worker_result.input_width,
                            worker_result.input_height,
                        )),
                        Err(error) => {
                            let _ = cleanup_worker_artifact_path(Path::new(
                                &worker_result.artifact_path,
                            ));
                            Err(error)
                        }
                    }
                }
            }
        };

        let capture_result = match capture_result {
            Ok(value) => value,
            Err(error) => {
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
                    delay_ms = ?requested_delay_ms,
                    delay_seconds = ?requested_delay_seconds,
                    play_sound,
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "capture failed"
                );
                return result::error_result(&error);
            }
        };

        if play_sound {
            self.feedback_emitter.emit_capture();
        }

        let (artifact, source_width, source_height, target, input_units, input_width, input_height) =
            capture_result;
        let context = result::CaptureContextPayload {
            applied_settings: output.applied_settings(parsed_common.delay),
            input_units,
            input_width,
            input_height,
            source_units: SOURCE_UNITS_PIXELS.to_owned(),
            source_width,
            source_height,
            target,
        };
        if let Ok(mut state) = self.last_capture_context.lock() {
            *state = Some(context.clone());
        }

        info!(
            tool = capture_mode,
            phase = "complete",
            delay_ms = ?requested_delay_ms,
            delay_seconds = ?requested_delay_seconds,
            applied_delay_ms = ?applied_delay_ms,
            play_sound,
            output_mode = ?parsed_common.output.mode,
            output_preset = ?parsed_common.output.preset,
            output_format = parsed_common.output.format.as_str(),
            jpeg_quality = parsed_common.output.jpeg_quality,
            max_dimension = ?parsed_common.output.max_dimension,
            elapsed_ms = started_at.elapsed().as_millis(),
            path = %artifact.path.display(),
            mime_type = %artifact.mime_type,
            width = artifact.width,
            height = artifact.height,
            "tool invocation completed"
        );

        result::success_result(capture_mode, &artifact, &context)
    }
}

impl ZeuxisScreenshotServer {
    pub(crate) fn build_tool_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::tool_router()
    }

    fn validate_window_capture_request(
        &self,
        snapshot_id: &str,
        window_id: u32,
    ) -> Result<(), ServerError> {
        let state = self
            .last_window_snapshot
            .lock()
            .map_err(|_| ServerError::storage_failed("window snapshot lock poisoned"))?;
        let Some(snapshot) = state.as_ref() else {
            return Err(ServerError::invalid_params(
                "capture_window requires a list_windows snapshot; call list_windows first",
            ));
        };

        if snapshot.snapshot_id != snapshot_id {
            return Err(ServerError::invalid_params(format!(
                "snapshot_id {snapshot_id} is stale; latest snapshot_id is {} (id_scope={}, listed_at_utc={})",
                snapshot.snapshot_id, snapshot.id_scope, snapshot.listed_at_utc
            )));
        }

        if !snapshot.windows.iter().any(|window| window.id == window_id) {
            return Err(ServerError::window_not_found(format!(
                "window_id {window_id} is not present in snapshot {snapshot_id}; call list_windows again"
            )));
        }

        Ok(())
    }

    fn create_worker_artifact_path(&self, capture_mode: &str, suffix: &str) -> PathBuf {
        let base = std::env::temp_dir().join("zeuxis-worker-artifacts");
        let _ = std::fs::create_dir_all(&base);
        let artifact_id = next_worker_artifact_id();
        base.join(format!("zeuxis-{capture_mode}-{artifact_id}{suffix}"))
    }
}

fn parse_common_params(common: &CommonCaptureParams) -> Result<ParsedCommonParams, ServerError> {
    let delay = match (common.delay_ms, common.delay_seconds) {
        (Some(_), Some(_)) => {
            return Err(ServerError::invalid_params(
                "provide either delay_ms or delay_seconds, not both",
            ));
        }
        (Some(delay_ms), None) => Some(parse_delay_milliseconds(delay_ms)?),
        (None, Some(delay_seconds)) => Some(parse_delay_seconds(delay_seconds)?),
        (None, None) => None,
    };
    let output = resolve_output_settings(common.output.as_ref())?;
    Ok(ParsedCommonParams { delay, output })
}

fn output_mode_as_str(mode: OutputMode) -> &'static str {
    match mode {
        OutputMode::Preset => "preset",
        OutputMode::Custom => "custom",
    }
}

fn output_preset_as_str(preset: OutputPreset) -> &'static str {
    match preset {
        OutputPreset::Analysis => "analysis",
        OutputPreset::Exact => "exact",
        OutputPreset::Compact => "compact",
    }
}

fn next_windows_snapshot_id() -> String {
    static SNAPSHOT_COUNTER: AtomicU64 = AtomicU64::new(1);
    let value = SNAPSHOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("windows-{value:016x}")
}

fn next_worker_request_id() -> String {
    static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);
    let value = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("worker-{value:016x}")
}

fn next_worker_artifact_id() -> String {
    static ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(1);
    let value = ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{value:016x}-{}", std::process::id())
}

fn cleanup_worker_artifact_path(path: &Path) -> Result<(), std::io::Error> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn run_capture_operation_inline(
    backend: &dyn CaptureBackend,
    cursor_provider: &dyn CursorProvider,
    operation: &CaptureOperation,
) -> Result<CaptureWorkOutput, ServerError> {
    match operation {
        CaptureOperation::CaptureScreen { monitor_id } => {
            let image = backend.capture_screen(*monitor_id)?;
            let resolved_monitor = resolve_monitor_at_capture(backend, *monitor_id).ok();
            let resolved_monitor_id = resolved_monitor
                .as_ref()
                .map(|monitor| monitor.id)
                .or(*monitor_id);
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    monitor_id: resolved_monitor_id,
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: resolved_monitor.as_ref().map(|monitor| monitor.width),
                input_height: resolved_monitor.as_ref().map(|monitor| monitor.height),
            })
        }
        CaptureOperation::CaptureActiveWindow => {
            let image = backend.capture_active_window()?;
            let resolved_window = resolve_active_window(backend).ok();
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    window_id: resolved_window.as_ref().map(|window| window.id),
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: resolved_window.as_ref().map(|window| window.width),
                input_height: resolved_window.as_ref().map(|window| window.height),
            })
        }
        CaptureOperation::CaptureCursorWindow {
            include_system_windows,
        } => {
            let cursor = cursor_provider.cursor_position()?;
            let resolved_window =
                resolve_window_at_cursor_with_filter(backend, cursor, *include_system_windows)?;
            let image = backend
                .capture_window(resolved_window.id)
                .or_else(|_| backend.capture_window_at_cursor(cursor))?;
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    window_id: Some(resolved_window.id),
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: Some(resolved_window.width),
                input_height: Some(resolved_window.height),
            })
        }
        CaptureOperation::CaptureWindow { window_id } => {
            let image = backend.capture_window(*window_id)?;
            let resolved_window = resolve_window_by_id(backend, *window_id).ok();
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    window_id: Some(*window_id),
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: resolved_window.as_ref().map(|window| window.width),
                input_height: resolved_window.as_ref().map(|window| window.height),
            })
        }
        CaptureOperation::CaptureCursorRegion { size } => {
            let cursor = cursor_provider.cursor_position()?;
            let rect = center_square_on_cursor(cursor, *size)?;
            let image = backend.capture_cursor_region(cursor, *size)?;
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    rect: Some(result::CaptureRectPayload {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                        coordinate_space: "global".to_owned(),
                    }),
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: Some(*size),
                input_height: Some(*size),
            })
        }
        CaptureOperation::CaptureRect {
            x,
            y,
            width,
            height,
        } => {
            let rect = GlobalRect {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            };
            let image = backend.capture_rect(rect)?;
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    rect: Some(result::CaptureRectPayload {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                        coordinate_space: "global".to_owned(),
                    }),
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: Some(rect.width),
                input_height: Some(rect.height),
            })
        }
        CaptureOperation::CaptureMonitorRegion {
            monitor_id,
            x,
            y,
            width,
            height,
        } => {
            let image = backend.capture_monitor_region(*monitor_id, *x, *y, *width, *height)?;
            Ok(CaptureWorkOutput {
                image,
                target: result::CaptureTargetPayload {
                    monitor_id: Some(*monitor_id),
                    rect: Some(result::CaptureRectPayload {
                        x: i32::try_from(*x).unwrap_or(i32::MAX),
                        y: i32::try_from(*y).unwrap_or(i32::MAX),
                        width: *width,
                        height: *height,
                        coordinate_space: "monitor_local".to_owned(),
                    }),
                    ..result::CaptureTargetPayload::default()
                },
                input_units: INPUT_UNITS_POINTS.to_owned(),
                input_width: Some(*width),
                input_height: Some(*height),
            })
        }
    }
}

fn filter_windows(mut windows: Vec<WindowInfo>, params: &ListWindowsParams) -> Vec<WindowInfo> {
    if params.focused_only.unwrap_or(false) {
        windows.retain(|window| window.is_focused && !window.is_minimized);
    }

    if !params.include_system_windows.unwrap_or(false) {
        windows.retain(|window| !is_system_window(window));
    }

    if let Some(app_filter) = params.app_contains.as_deref() {
        let app_filter = app_filter.trim();
        if !app_filter.is_empty() {
            windows.retain(|window| contains_case_insensitive(&window.app, app_filter));
        }
    }

    if let Some(title_filter) = params.title_contains.as_deref() {
        let title_filter = title_filter.trim();
        if !title_filter.is_empty() {
            windows.retain(|window| contains_case_insensitive(&window.title, title_filter));
        }
    }

    windows
}

fn is_system_window(window: &WindowInfo) -> bool {
    let app = window.app.trim();
    let title = window.title.trim();
    if app.is_empty() {
        return true;
    }

    let app_norm = normalize_window_label(app);
    let title_norm = normalize_window_label(title);
    let system_app_keywords = [
        "window server",
        "windowserver",
        "control center",
        "control centre",
        "controlcenter",
        "controlcentre",
        "notification center",
        "notification centre",
        "notificationcenter",
        "systemuiserver",
        "system ui server",
        "dock",
        "spotlight",
        "menu bar",
        "menubar",
    ];
    let system_title_keywords = [
        "menubar",
        "menu bar",
        "menu extra",
        "status item",
        "status menu",
        "notification",
        "desktop",
        "control center",
        "control centre",
    ];

    if system_app_keywords
        .iter()
        .any(|keyword| app_norm.contains(keyword))
    {
        return true;
    }

    if title.is_empty() && (app_norm.contains("menu") || app_norm.contains("status")) {
        return true;
    }

    system_title_keywords
        .iter()
        .any(|keyword| title_norm.contains(keyword))
}

fn normalize_window_label(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

fn contains_case_insensitive(value: &str, needle: &str) -> bool {
    value
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn resolve_monitor_at_capture(
    backend: &dyn CaptureBackend,
    requested_monitor_id: Option<u32>,
) -> Result<MonitorInfo, ServerError> {
    let monitors = backend.list_monitors()?;
    select_monitor(&monitors, requested_monitor_id).cloned()
}

fn select_monitor(
    monitors: &[MonitorInfo],
    requested_monitor_id: Option<u32>,
) -> Result<&MonitorInfo, ServerError> {
    if monitors.is_empty() {
        return Err(ServerError::monitor_not_found(
            "no monitor is available for capture",
        ));
    }

    if let Some(requested_id) = requested_monitor_id {
        if let Some(monitor) = monitors.iter().find(|monitor| monitor.id == requested_id) {
            return Ok(monitor);
        }
        return Err(ServerError::monitor_not_found(format!(
            "monitor with id {requested_id} was not found"
        )));
    }

    monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .or_else(|| monitors.first())
        .ok_or_else(|| ServerError::monitor_not_found("no monitor is available for capture"))
}

fn resolve_active_window(backend: &dyn CaptureBackend) -> Result<WindowInfo, ServerError> {
    let windows = backend.list_windows()?;
    windows
        .into_iter()
        .find(|window| window.is_focused && !window.is_minimized)
        .ok_or_else(|| {
            ServerError::window_not_found("focused non-minimized window could not be found")
        })
}

fn resolve_window_by_id(
    backend: &dyn CaptureBackend,
    window_id: u32,
) -> Result<WindowInfo, ServerError> {
    let windows = backend.list_windows()?;
    windows
        .into_iter()
        .find(|window| window.id == window_id)
        .ok_or_else(|| {
            ServerError::window_not_found(format!("window with id {window_id} was not found"))
        })
}

fn resolve_window_at_cursor_with_filter(
    backend: &dyn CaptureBackend,
    cursor: crate::capture::region::Point,
    include_system_windows: bool,
) -> Result<WindowInfo, ServerError> {
    let windows = backend.list_windows()?;
    let filter = ListWindowsParams {
        focused_only: None,
        include_system_windows: Some(include_system_windows),
        app_contains: None,
        title_contains: None,
    };
    let windows = filter_windows(windows, &filter);
    windows
        .into_iter()
        .find(|window| {
            !window.is_minimized
                && rect_contains_point(window.x, window.y, window.width, window.height, cursor)
        })
        .ok_or_else(|| {
            if include_system_windows {
                ServerError::window_not_found("no non-minimized window contains the cursor point")
            } else {
                ServerError::window_not_found(
                    "no non-system, non-minimized window contains the cursor point; set include_system_windows=true to allow system surfaces"
                )
            }
        })
}

fn parse_delay_milliseconds(delay_ms: i64) -> Result<Duration, ServerError> {
    if delay_ms < 0 {
        return Err(ServerError::invalid_params(
            "delay_ms must be greater than or equal to 0",
        ));
    }
    if delay_ms > MAX_DELAY_MILLISECONDS {
        return Err(ServerError::invalid_params(format!(
            "delay_ms must be less than or equal to {MAX_DELAY_MILLISECONDS}"
        )));
    }

    Ok(Duration::from_millis(delay_ms as u64))
}

fn parse_delay_seconds(delay_seconds: f64) -> Result<Duration, ServerError> {
    if !delay_seconds.is_finite() {
        return Err(ServerError::invalid_params(
            "delay_seconds must be a finite number",
        ));
    }
    if delay_seconds < 0.0 {
        return Err(ServerError::invalid_params(
            "delay_seconds must be greater than or equal to 0",
        ));
    }
    if delay_seconds > MAX_DELAY_SECONDS {
        return Err(ServerError::invalid_params(format!(
            "delay_seconds must be less than or equal to {MAX_DELAY_SECONDS}"
        )));
    }

    Ok(Duration::from_secs_f64(delay_seconds))
}

fn resolve_output_settings(
    output: Option<&OutputInput>,
) -> Result<ResolvedOutputSettings, ServerError> {
    let Some(output) = output else {
        return Ok(default_output_settings(OutputPreset::Analysis));
    };

    let output = match output {
        OutputInput::Preset(preset) => return Ok(default_output_settings(*preset)),
        OutputInput::Detailed(output) => output,
    };

    match output.mode {
        OutputMode::Preset => {
            if output.format.is_some()
                || output.jpeg_quality.is_some()
                || output.max_dimension.is_some()
            {
                return Err(ServerError::invalid_params(
                    "output preset mode only accepts output.preset",
                ));
            }
            Ok(default_output_settings(
                output.preset.unwrap_or(OutputPreset::Analysis),
            ))
        }
        OutputMode::Custom => {
            if output.preset.is_some() {
                return Err(ServerError::invalid_params(
                    "output custom mode does not accept output.preset",
                ));
            }
            let format = output
                .format
                .ok_or_else(|| {
                    ServerError::invalid_params("output.format is required in custom mode")
                })?
                .to_storage();

            let max_dimension = output
                .max_dimension
                .map(parse_output_max_dimension)
                .transpose()?;

            let jpeg_quality = match (format, output.jpeg_quality) {
                (CaptureOutputFormat::Jpeg, Some(value)) => parse_jpeg_quality(value)?,
                (CaptureOutputFormat::Jpeg, None) => DEFAULT_JPEG_QUALITY,
                (_, Some(_)) => {
                    return Err(ServerError::invalid_params(
                        "output.jpeg_quality is only supported when output.format is jpeg",
                    ));
                }
                (_, None) => DEFAULT_JPEG_QUALITY,
            };

            Ok(ResolvedOutputSettings {
                mode: OutputMode::Custom,
                preset: None,
                format,
                jpeg_quality,
                max_dimension,
            })
        }
    }
}

const fn default_output_settings(preset: OutputPreset) -> ResolvedOutputSettings {
    match preset {
        OutputPreset::Analysis => ResolvedOutputSettings {
            mode: OutputMode::Preset,
            preset: Some(preset),
            format: CaptureOutputFormat::Png,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            max_dimension: Some(DEFAULT_ANALYSIS_MAX_DIMENSION),
        },
        OutputPreset::Exact => ResolvedOutputSettings {
            mode: OutputMode::Preset,
            preset: Some(preset),
            format: CaptureOutputFormat::Png,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            max_dimension: None,
        },
        OutputPreset::Compact => ResolvedOutputSettings {
            mode: OutputMode::Preset,
            preset: Some(preset),
            format: CaptureOutputFormat::Jpeg,
            jpeg_quality: DEFAULT_COMPACT_JPEG_QUALITY,
            max_dimension: Some(DEFAULT_COMPACT_MAX_DIMENSION),
        },
    }
}

fn parse_jpeg_quality(jpeg_quality: i64) -> Result<u8, ServerError> {
    if !(MIN_JPEG_QUALITY..=MAX_JPEG_QUALITY).contains(&jpeg_quality) {
        return Err(ServerError::invalid_params(format!(
            "output.jpeg_quality must be in range {MIN_JPEG_QUALITY}..={MAX_JPEG_QUALITY}"
        )));
    }

    Ok(jpeg_quality as u8)
}

fn parse_output_max_dimension(max_dimension: i64) -> Result<u32, ServerError> {
    if !(MIN_OUTPUT_MAX_DIMENSION..=MAX_OUTPUT_MAX_DIMENSION).contains(&max_dimension) {
        return Err(ServerError::invalid_params(format!(
            "output.max_dimension must be in range {MIN_OUTPUT_MAX_DIMENSION}..={MAX_OUTPUT_MAX_DIMENSION}"
        )));
    }

    Ok(max_dimension as u32)
}

fn downscale_if_needed(image: image::RgbaImage, max_dimension: u32) -> image::RgbaImage {
    let current_max = image.width().max(image.height());
    if current_max <= max_dimension {
        return image;
    }

    image::DynamicImage::ImageRgba8(image)
        .resize(
            max_dimension,
            max_dimension,
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8()
}

fn status_from_result(result: Result<(), ServerError>) -> (bool, Option<String>, Option<String>) {
    match result {
        Ok(()) => (true, None, None),
        Err(error) => (
            false,
            Some(error.error_code().to_owned()),
            Some(error.message().to_owned()),
        ),
    }
}

async fn run_blocking_with_timeout<T, F>(
    timeout: Duration,
    timeout_error_message: String,
    join_error_prefix: &str,
    job: F,
) -> Result<T, ServerError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ServerError> + Send + 'static,
{
    match tokio::time::timeout(timeout, tokio::task::spawn_blocking(job)).await {
        Ok(Ok(result)) => result,
        Ok(Err(join_error)) => Err(ServerError::storage_failed(format!(
            "{join_error_prefix}: {join_error}"
        ))),
        Err(_) => Err(ServerError::storage_failed(timeout_error_message)),
    }
}

#[cfg(target_os = "linux")]
fn permission_check_metadata() -> (bool, &'static str) {
    (false, "best_effort_unchecked")
}

#[cfg(target_os = "macos")]
fn permission_check_metadata() -> (bool, &'static str) {
    (true, "macos_preflight")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn permission_check_metadata() -> (bool, &'static str) {
    (true, "unsupported_platform")
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn now_rfc3339_utc() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn parse_rect_input(input: &CaptureRectParams) -> Result<GlobalRect, ServerError> {
    let x = to_i32(input.x, "x")?;
    let y = to_i32(input.y, "y")?;
    let width = to_u32_positive(input.width, "width")?;
    let height = to_u32_positive(input.height, "height")?;
    validate_capture_dimensions(width, height)?;

    Ok(GlobalRect {
        x,
        y,
        width,
        height,
    })
}

fn parse_monitor_region_input(
    input: &CaptureMonitorRegionParams,
) -> Result<(u32, u32, u32, u32), ServerError> {
    let x = to_u32_non_negative(input.x, "x")?;
    let y = to_u32_non_negative(input.y, "y")?;
    let width = to_u32_positive(input.width, "width")?;
    let height = to_u32_positive(input.height, "height")?;
    Ok((x, y, width, height))
}

fn validate_capture_dimensions(width: u32, height: u32) -> Result<(), ServerError> {
    if width > MAX_CAPTURE_DIMENSION || height > MAX_CAPTURE_DIMENSION {
        return Err(ServerError::invalid_region(format!(
            "requested dimensions exceed the v1 limit of {MAX_CAPTURE_DIMENSION} pixels per side"
        )));
    }

    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_CAPTURE_PIXELS {
        return Err(ServerError::invalid_region(format!(
            "requested area exceeds the v1 limit of {MAX_CAPTURE_PIXELS} pixels"
        )));
    }

    Ok(())
}

fn to_i32(value: i64, field: &str) -> Result<i32, ServerError> {
    i32::try_from(value).map_err(|_| {
        ServerError::invalid_region(format!("{field} is outside the supported coordinate range"))
    })
}

fn to_u32_non_negative(value: i64, field: &str) -> Result<u32, ServerError> {
    if value < 0 {
        return Err(ServerError::invalid_region(format!(
            "{field} must be greater than or equal to 0"
        )));
    }

    u32::try_from(value).map_err(|_| {
        ServerError::invalid_region(format!("{field} is outside the supported coordinate range"))
    })
}

fn to_u32_positive(value: i64, field: &str) -> Result<u32, ServerError> {
    if value <= 0 {
        return Err(ServerError::invalid_region(format!(
            "{field} must be greater than 0"
        )));
    }

    u32::try_from(value).map_err(|_| {
        ServerError::invalid_region(format!("{field} is outside the supported coordinate range"))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn mcp_tools_validation_common_delay_rejects_nan() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_ms: None,
            delay_seconds: Some(f64::NAN),
            play_sound: None,
            output: None,
        })
        .expect_err("nan should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_validation_common_delay_rejects_values_above_policy_limit() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_ms: None,
            delay_seconds: Some(MAX_DELAY_SECONDS + 0.5),
            play_sound: None,
            output: None,
        })
        .expect_err("delay above policy should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_output_defaults_use_analysis_png_profile() {
        let parsed = parse_common_params(&CommonCaptureParams::default())
            .expect("default params should parse");
        assert_eq!(parsed.output.mode, OutputMode::Preset);
        assert_eq!(parsed.output.preset, Some(OutputPreset::Analysis));
        assert_eq!(parsed.output.format, CaptureOutputFormat::Png);
        assert_eq!(
            parsed.output.max_dimension,
            Some(DEFAULT_ANALYSIS_MAX_DIMENSION)
        );
    }

    #[test]
    fn mcp_tools_output_compact_preset_maps_to_jpeg_defaults() {
        let parsed = parse_common_params(&CommonCaptureParams {
            output: Some(OutputInput::Detailed(OutputParams {
                mode: OutputMode::Preset,
                preset: Some(OutputPreset::Compact),
                format: None,
                jpeg_quality: None,
                max_dimension: None,
            })),
            ..CommonCaptureParams::default()
        })
        .expect("compact profile should parse");

        assert_eq!(parsed.output.format, CaptureOutputFormat::Jpeg);
        assert_eq!(parsed.output.jpeg_quality, DEFAULT_COMPACT_JPEG_QUALITY);
        assert_eq!(
            parsed.output.max_dimension,
            Some(DEFAULT_COMPACT_MAX_DIMENSION)
        );
    }

    #[test]
    fn mcp_tools_output_compact_string_shorthand_maps_to_jpeg_defaults() {
        let parsed = parse_common_params(&CommonCaptureParams {
            output: Some(OutputInput::Preset(OutputPreset::Compact)),
            ..CommonCaptureParams::default()
        })
        .expect("compact shorthand should parse");

        assert_eq!(parsed.output.mode, OutputMode::Preset);
        assert_eq!(parsed.output.preset, Some(OutputPreset::Compact));
        assert_eq!(parsed.output.format, CaptureOutputFormat::Jpeg);
        assert_eq!(parsed.output.jpeg_quality, DEFAULT_COMPACT_JPEG_QUALITY);
        assert_eq!(
            parsed.output.max_dimension,
            Some(DEFAULT_COMPACT_MAX_DIMENSION)
        );
    }

    #[test]
    fn mcp_tools_output_string_shorthand_deserializes_from_json() {
        let common: CommonCaptureParams = serde_json::from_value(serde_json::json!({
            "output": "compact"
        }))
        .expect("string shorthand should deserialize");

        let parsed = parse_common_params(&common).expect("compact shorthand should parse");
        assert_eq!(parsed.output.preset, Some(OutputPreset::Compact));
        assert_eq!(parsed.output.format, CaptureOutputFormat::Jpeg);
        assert_eq!(parsed.output.jpeg_quality, DEFAULT_COMPACT_JPEG_QUALITY);
    }

    #[test]
    fn mcp_tools_output_preset_mode_rejects_custom_fields() {
        let error = parse_common_params(&CommonCaptureParams {
            output: Some(OutputInput::Detailed(OutputParams {
                mode: OutputMode::Preset,
                preset: Some(OutputPreset::Analysis),
                format: Some(OutputFormat::Webp),
                jpeg_quality: None,
                max_dimension: None,
            })),
            ..CommonCaptureParams::default()
        })
        .expect_err("preset mode should reject format field");

        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_output_custom_mode_requires_format() {
        let error = parse_common_params(&CommonCaptureParams {
            output: Some(OutputInput::Detailed(OutputParams {
                mode: OutputMode::Custom,
                preset: None,
                format: None,
                jpeg_quality: None,
                max_dimension: None,
            })),
            ..CommonCaptureParams::default()
        })
        .expect_err("custom mode without format should fail");

        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_output_custom_mode_rejects_jpeg_quality_for_non_jpeg() {
        let error = parse_common_params(&CommonCaptureParams {
            output: Some(OutputInput::Detailed(OutputParams {
                mode: OutputMode::Custom,
                preset: None,
                format: Some(OutputFormat::Png),
                jpeg_quality: Some(80),
                max_dimension: None,
            })),
            ..CommonCaptureParams::default()
        })
        .expect_err("png custom output should reject jpeg_quality");

        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_output_custom_mode_accepts_webp_with_max_dimension() {
        let parsed = parse_common_params(&CommonCaptureParams {
            output: Some(OutputInput::Detailed(OutputParams {
                mode: OutputMode::Custom,
                preset: None,
                format: Some(OutputFormat::Webp),
                jpeg_quality: None,
                max_dimension: Some(1400),
            })),
            ..CommonCaptureParams::default()
        })
        .expect("custom mode should parse");

        assert_eq!(parsed.output.mode, OutputMode::Custom);
        assert_eq!(parsed.output.preset, None);
        assert_eq!(parsed.output.format, CaptureOutputFormat::Webp);
        assert_eq!(parsed.output.max_dimension, Some(1400));
    }

    #[test]
    fn mcp_tools_validation_rect_parser_rejects_non_positive_width() {
        let error = parse_rect_input(&CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        })
        .expect_err("width 0 should fail");
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn mcp_tools_monitor_region_parser_rejects_negative_coordinates() {
        let error = parse_monitor_region_input(&CaptureMonitorRegionParams {
            common: CommonCaptureParams::default(),
            monitor_id: 1,
            x: -1,
            y: 0,
            width: 10,
            height: 10,
        })
        .expect_err("negative x should fail");

        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn mcp_tools_validation_rect_parser_rejects_oversized_area() {
        let error = parse_rect_input(&CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 0,
            y: 0,
            width: 10_000,
            height: 10_000,
        })
        .expect_err("area over limit should fail");
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn mcp_tools_delay_parser_accepts_zero_and_rejects_negative() {
        assert_eq!(parse_delay_seconds(0.0).expect("zero delay").as_millis(), 0);
        let error = parse_delay_seconds(-0.1).expect_err("negative delay should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_delay_ms_parser_accepts_zero_and_rejects_out_of_range() {
        assert_eq!(
            parse_delay_milliseconds(0).expect("zero delay").as_millis(),
            0
        );
        assert_eq!(
            parse_delay_milliseconds(MAX_DELAY_MILLISECONDS)
                .expect("max delay")
                .as_millis(),
            MAX_DELAY_MILLISECONDS as u128
        );
        assert!(parse_delay_milliseconds(-1).is_err());
        assert!(parse_delay_milliseconds(MAX_DELAY_MILLISECONDS + 1).is_err());
    }

    #[test]
    fn mcp_tools_common_delay_rejects_both_delay_aliases() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_ms: Some(250),
            delay_seconds: Some(0.25),
            play_sound: None,
            output: None,
        })
        .expect_err("supplying both delay aliases should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_jpeg_quality_parser_enforces_bounds() {
        assert_eq!(
            parse_jpeg_quality(MIN_JPEG_QUALITY).expect("min quality"),
            MIN_JPEG_QUALITY as u8
        );
        assert_eq!(
            parse_jpeg_quality(MAX_JPEG_QUALITY).expect("max quality"),
            MAX_JPEG_QUALITY as u8
        );
        assert!(parse_jpeg_quality(MIN_JPEG_QUALITY - 1).is_err());
        assert!(parse_jpeg_quality(MAX_JPEG_QUALITY + 1).is_err());
    }

    #[test]
    fn mcp_tools_max_dimension_parser_enforces_bounds() {
        assert_eq!(
            parse_output_max_dimension(MIN_OUTPUT_MAX_DIMENSION).expect("min max_dimension"),
            MIN_OUTPUT_MAX_DIMENSION as u32
        );
        assert_eq!(
            parse_output_max_dimension(MAX_OUTPUT_MAX_DIMENSION).expect("max max_dimension"),
            MAX_OUTPUT_MAX_DIMENSION as u32
        );
        assert!(parse_output_max_dimension(MIN_OUTPUT_MAX_DIMENSION - 1).is_err());
        assert!(parse_output_max_dimension(MAX_OUTPUT_MAX_DIMENSION + 1).is_err());
    }

    #[test]
    fn mcp_tools_rect_parser_accepts_valid_bounds_and_coordinate_limits() {
        let rect = parse_rect_input(&CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: i64::from(i32::MIN),
            y: i64::from(i32::MAX),
            width: 1,
            height: 1,
        })
        .expect("rect should parse");
        assert_eq!(rect.x, i32::MIN);
        assert_eq!(rect.y, i32::MAX);

        assert!(to_i32(i64::from(i32::MAX) + 1, "x").is_err());
        assert!(to_u32_positive(i64::from(u32::MAX) + 1, "width").is_err());
    }

    #[test]
    fn mcp_tools_validate_capture_dimensions_accepts_limit() {
        assert!(validate_capture_dimensions(MAX_CAPTURE_DIMENSION, 1).is_ok());
    }

    #[test]
    fn mcp_tools_downscale_if_needed_scales_only_when_required() {
        let small = image::RgbaImage::from_pixel(100, 50, image::Rgba([1, 2, 3, 255]));
        let unchanged = downscale_if_needed(small.clone(), 120);
        assert_eq!(unchanged.width(), 100);
        assert_eq!(unchanged.height(), 50);

        let large = image::RgbaImage::from_pixel(4000, 2000, image::Rgba([1, 2, 3, 255]));
        let scaled = downscale_if_needed(large, 1000);
        assert!(scaled.width() <= 1000);
        assert!(scaled.height() <= 1000);
    }

    #[test]
    fn mcp_tools_status_from_result_maps_ok_and_err() {
        let ok = status_from_result(Ok(()));
        assert_eq!(ok, (true, None, None));

        let err = status_from_result(Err(ServerError::storage_failed("broken")));
        assert!(!err.0);
        assert_eq!(err.1.as_deref(), Some("storage_failed"));
        assert_eq!(err.2.as_deref(), Some("broken"));
    }

    #[test]
    fn mcp_tools_env_non_empty_trims_and_ignores_blank_values() {
        let _guard = env_lock().lock().expect("lock env");
        unsafe {
            std::env::set_var("ZEUXIS_TEST_ENV_NON_EMPTY", "  value  ");
        }
        assert_eq!(
            env_non_empty("ZEUXIS_TEST_ENV_NON_EMPTY").as_deref(),
            Some("value")
        );

        unsafe {
            std::env::set_var("ZEUXIS_TEST_ENV_NON_EMPTY", "   ");
        }
        assert_eq!(env_non_empty("ZEUXIS_TEST_ENV_NON_EMPTY"), None);

        unsafe {
            std::env::remove_var("ZEUXIS_TEST_ENV_NON_EMPTY");
        }
        assert_eq!(env_non_empty("ZEUXIS_TEST_ENV_NON_EMPTY"), None);
    }

    #[test]
    fn mcp_tools_now_rfc3339_utc_returns_non_empty_timestamp() {
        let value = now_rfc3339_utc();
        assert!(!value.is_empty());
        assert!(value.contains('T'));
        assert!(value.ends_with('Z'));
    }
}
