use std::{
    sync::Arc,
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
    capture::{backend::CaptureBackend, region::GlobalRect},
    cursor::CursorProvider,
    mcp::{errors::ServerError, result},
    storage::{CaptureOutputFormat, CaptureOutputOptions},
};

use super::server::ZeuxisScreenshotServer;

const MAX_DELAY_SECONDS: f64 = 30.0;
const MAX_CAPTURE_DIMENSION: u32 = 16_384;
const MAX_CAPTURE_PIXELS: u64 = 40_000_000;
const DEFAULT_ANALYSIS_MAX_DIMENSION: u32 = 2_560;
const DEFAULT_COMPACT_MAX_DIMENSION: u32 = 1_600;
const DEFAULT_JPEG_QUALITY: u8 = 82;
const MIN_JPEG_QUALITY: i64 = 40;
const MAX_JPEG_QUALITY: i64 = 95;
const MIN_OUTPUT_MAX_DIMENSION: i64 = 256;
const MAX_OUTPUT_MAX_DIMENSION: i64 = 8_192;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedOutputSettings {
    preset: OutputPreset,
    format: CaptureOutputFormat,
    jpeg_quality: u8,
    max_dimension: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct ParsedCommonParams {
    delay: Option<Duration>,
    output: ResolvedOutputSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct CommonCaptureParams {
    /// Optional delay before capture in seconds (0..=30).
    pub delay_seconds: Option<f64>,
    /// Play a shutter sound after a successful capture.
    pub play_sound: Option<bool>,
    /// Output profile: analysis (default), exact, or compact.
    pub output_preset: Option<OutputPreset>,
    /// Override image format: png, jpeg, or webp.
    pub output_format: Option<OutputFormat>,
    /// JPEG quality (40..=95). Only valid when output resolves to jpeg.
    pub jpeg_quality: Option<i64>,
    /// Optional long-edge cap in pixels (256..=8192), preserving aspect ratio.
    pub max_dimension: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct ListMonitorsParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct DiagnoseRuntimeParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct GetLatestScreenshotParams {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct CaptureScreenParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Monitor id from list_monitors. Omit to use the primary monitor.
    pub monitor_id: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureCursorRegionParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Square region size in pixels.
    pub size: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureRectParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    /// Global X coordinate (top-left).
    pub x: i64,
    /// Global Y coordinate (top-left).
    pub y: i64,
    /// Rectangle width in pixels.
    pub width: i64,
    /// Rectangle height in pixels.
    pub height: i64,
}

#[tool_router(router = tool_router)]
impl ZeuxisScreenshotServer {
    #[tool(
        name = "list_monitors",
        description = "Discover available monitors. Use ids with capture_screen.monitor_id.",
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

        let backend = Arc::clone(&self.backend);
        let timeout = self.blocking_task_timeout;
        let monitors = match tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || backend.list_monitors()),
        )
        .await
        {
            Ok(Ok(Ok(monitors))) => monitors,
            Ok(Ok(Err(error))) => {
                error!(
                    tool = "list_monitors",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
            Ok(Err(join_error)) => {
                let error = ServerError::storage_failed(format!(
                    "monitor listing worker task failed: {join_error}"
                ));
                error!(
                    tool = "list_monitors",
                    phase = "error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
            Err(_) => {
                let error = ServerError::storage_failed(format!(
                    "monitor listing timed out after {}ms",
                    timeout.as_millis()
                ));
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
        name = "diagnose_runtime",
        description = "Report capture readiness and session context. Use first when Linux capture is unreliable.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn diagnose_runtime(
        &self,
        _params: Parameters<DiagnoseRuntimeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        info!(
            tool = "diagnose_runtime",
            phase = "start",
            "tool invocation started"
        );

        let permission_result = self.permission_gate.ensure_capture_allowed();

        let backend = Arc::clone(&self.backend);
        let timeout = self.blocking_task_timeout;
        let monitor_result = match tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || backend.list_monitors()),
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(join_error)) => Err(ServerError::storage_failed(format!(
                "monitor listing worker task failed: {join_error}"
            ))),
            Err(_) => Err(ServerError::storage_failed(format!(
                "monitor listing timed out after {}ms",
                timeout.as_millis()
            ))),
        };

        let cursor_result = self.cursor_provider.cursor_position();

        let (permission_ok, permission_error_code, permission_message) =
            status_from_result(permission_result);
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
            tool = "diagnose_runtime",
            phase = "complete",
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
        name = "get_latest_screenshot",
        description = "Return the latest screenshot artifact captured during this server session without taking a new capture.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn get_latest_screenshot(
        &self,
        _params: Parameters<GetLatestScreenshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let started_at = Instant::now();
        info!(
            tool = "get_latest_screenshot",
            phase = "start",
            "tool invocation started"
        );

        let storage = Arc::clone(&self.storage);
        let timeout = self.blocking_task_timeout;
        let artifact = match tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || storage.latest_artifact()),
        )
        .await
        {
            Ok(Ok(Ok(artifact))) => artifact,
            Ok(Ok(Err(error))) => {
                error!(
                    tool = "get_latest_screenshot",
                    phase = "error",
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
            Ok(Err(join_error)) => {
                let error = ServerError::storage_failed(format!(
                    "latest screenshot worker task failed: {join_error}"
                ));
                error!(
                    tool = "get_latest_screenshot",
                    phase = "error",
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
            Err(_) => {
                let error = ServerError::storage_failed(format!(
                    "latest screenshot timed out after {}ms",
                    timeout.as_millis()
                ));
                error!(
                    tool = "get_latest_screenshot",
                    phase = "error",
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "tool invocation failed"
                );
                return Ok(result::error_result(&error));
            }
        };

        info!(
            tool = "get_latest_screenshot",
            phase = "complete",
            elapsed_ms = started_at.elapsed().as_millis(),
            path = %artifact.path.display(),
            mime_type = %artifact.mime_type,
            width = artifact.width,
            height = artifact.height,
            "tool invocation completed"
        );

        Ok(result::success_result("get_latest_screenshot", &artifact))
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
        let monitor_id = input.monitor_id;
        Ok(self
            .execute_capture("capture_screen", input.common, move |backend, _cursor| {
                backend.capture_screen(monitor_id)
            })
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
            .execute_capture("capture_active_window", params.0, |backend, _cursor| {
                backend.capture_active_window()
            })
            .await)
    }

    #[tool(
        name = "capture_window_at_cursor",
        description = "Capture the window under the cursor.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_window_at_cursor(
        &self,
        params: Parameters<CommonCaptureParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(self
            .execute_capture(
                "capture_window_at_cursor",
                params.0,
                |backend, cursor_provider| {
                    let cursor = cursor_provider.cursor_position()?;
                    backend.capture_window_at_cursor(cursor)
                },
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
                move |backend, cursor_provider| {
                    let cursor = cursor_provider.cursor_position()?;
                    backend.capture_cursor_region(cursor, size)
                },
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
            .execute_capture("capture_rect", input.common, move |backend, _cursor| {
                backend.capture_rect(rect)
            })
            .await)
    }

    async fn execute_capture<F>(
        &self,
        capture_mode: &'static str,
        common: CommonCaptureParams,
        capture_fn: F,
    ) -> CallToolResult
    where
        F: FnOnce(
                &Arc<dyn CaptureBackend>,
                &Arc<dyn CursorProvider>,
            ) -> Result<image::RgbaImage, ServerError>
            + Send
            + 'static,
    {
        let started_at = Instant::now();
        let requested_delay_seconds = common.delay_seconds;
        let play_sound = common.play_sound.unwrap_or(false);
        let requested_output_preset = common.output_preset;
        let requested_output_format = common.output_format;
        let requested_jpeg_quality = common.jpeg_quality;
        let requested_max_dimension = common.max_dimension;

        info!(
            tool = capture_mode,
            phase = "start",
            delay_seconds = ?requested_delay_seconds,
            play_sound,
            output_preset = ?requested_output_preset,
            output_format = ?requested_output_format,
            jpeg_quality = ?requested_jpeg_quality,
            max_dimension = ?requested_max_dimension,
            "tool invocation started"
        );

        let parsed_common = match parse_common_params(&common) {
            Ok(parsed_common) => parsed_common,
            Err(error) => {
                error!(
                    tool = capture_mode,
                    phase = "validation_error",
                    delay_seconds = ?requested_delay_seconds,
                    play_sound,
                    output_preset = ?requested_output_preset,
                    output_format = ?requested_output_format,
                    jpeg_quality = ?requested_jpeg_quality,
                    max_dimension = ?requested_max_dimension,
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
                delay_seconds = ?requested_delay_seconds,
                play_sound,
                elapsed_ms = started_at.elapsed().as_millis(),
                error_code = error.error_code(),
                message = error.message(),
                "permission gate failed"
            );
            return result::error_result(&error);
        }

        let _permit = match self.capture_slots.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(join_error) => {
                let error = ServerError::storage_failed(format!(
                    "capture slot coordination failed: {join_error}"
                ));
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
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

        let backend = Arc::clone(&self.backend);
        let cursor_provider = Arc::clone(&self.cursor_provider);
        let storage = Arc::clone(&self.storage);
        let output = parsed_common.output;
        let timeout = self.blocking_task_timeout;

        let artifact = match tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                let image = capture_fn(&backend, &cursor_provider)?;
                let image = match output.max_dimension {
                    Some(max_dimension) => downscale_if_needed(image, max_dimension),
                    None => image,
                };
                storage.write_image(
                    image,
                    capture_mode,
                    CaptureOutputOptions {
                        format: output.format,
                        jpeg_quality: output.jpeg_quality,
                    },
                )
            }),
        )
        .await
        {
            Ok(Ok(Ok(artifact))) => artifact,
            Ok(Ok(Err(error))) => {
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
                    delay_seconds = ?requested_delay_seconds,
                    play_sound,
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error_code = error.error_code(),
                    message = error.message(),
                    "capture failed"
                );
                return result::error_result(&error);
            }
            Ok(Err(join_error)) => {
                let error = ServerError::storage_failed(format!(
                    "capture worker task failed: {join_error}"
                ));
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
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
                    "capture timed out after {}ms",
                    timeout.as_millis()
                ));
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
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
            self.feedback_emitter.emit();
        }

        info!(
            tool = capture_mode,
            phase = "complete",
            delay_seconds = ?requested_delay_seconds,
            applied_delay_ms = ?applied_delay_ms,
            play_sound,
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

        result::success_result(capture_mode, &artifact)
    }
}

impl ZeuxisScreenshotServer {
    pub(crate) fn build_tool_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::tool_router()
    }
}

fn parse_common_params(common: &CommonCaptureParams) -> Result<ParsedCommonParams, ServerError> {
    let delay = common.delay_seconds.map(parse_delay_seconds).transpose()?;
    let output = resolve_output_settings(common)?;
    Ok(ParsedCommonParams { delay, output })
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
    common: &CommonCaptureParams,
) -> Result<ResolvedOutputSettings, ServerError> {
    let preset = common.output_preset.unwrap_or_default();
    let mut settings = default_output_settings(preset);

    if let Some(output_format) = common.output_format {
        settings.format = output_format.to_storage();
    }

    if let Some(max_dimension) = common.max_dimension {
        settings.max_dimension = Some(parse_output_max_dimension(max_dimension)?);
    }

    if let Some(jpeg_quality) = common.jpeg_quality {
        if settings.format != CaptureOutputFormat::Jpeg {
            return Err(ServerError::invalid_params(
                "jpeg_quality is only supported when output_format resolves to jpeg",
            ));
        }
        settings.jpeg_quality = parse_jpeg_quality(jpeg_quality)?;
    }

    Ok(settings)
}

const fn default_output_settings(preset: OutputPreset) -> ResolvedOutputSettings {
    match preset {
        OutputPreset::Analysis => ResolvedOutputSettings {
            preset,
            format: CaptureOutputFormat::Png,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            max_dimension: Some(DEFAULT_ANALYSIS_MAX_DIMENSION),
        },
        OutputPreset::Exact => ResolvedOutputSettings {
            preset,
            format: CaptureOutputFormat::Png,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            max_dimension: None,
        },
        OutputPreset::Compact => ResolvedOutputSettings {
            preset,
            format: CaptureOutputFormat::Jpeg,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            max_dimension: Some(DEFAULT_COMPACT_MAX_DIMENSION),
        },
    }
}

fn parse_jpeg_quality(jpeg_quality: i64) -> Result<u8, ServerError> {
    if !(MIN_JPEG_QUALITY..=MAX_JPEG_QUALITY).contains(&jpeg_quality) {
        return Err(ServerError::invalid_params(format!(
            "jpeg_quality must be in range {MIN_JPEG_QUALITY}..={MAX_JPEG_QUALITY}"
        )));
    }

    Ok(jpeg_quality as u8)
}

fn parse_output_max_dimension(max_dimension: i64) -> Result<u32, ServerError> {
    if !(MIN_OUTPUT_MAX_DIMENSION..=MAX_OUTPUT_MAX_DIMENSION).contains(&max_dimension) {
        return Err(ServerError::invalid_params(format!(
            "max_dimension must be in range {MIN_OUTPUT_MAX_DIMENSION}..={MAX_OUTPUT_MAX_DIMENSION}"
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
            delay_seconds: Some(f64::NAN),
            play_sound: None,
            ..CommonCaptureParams::default()
        })
        .expect_err("nan should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_validation_common_delay_rejects_excessive_value() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_seconds: Some(1e30),
            play_sound: None,
            ..CommonCaptureParams::default()
        })
        .expect_err("overflowing delay should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_validation_common_delay_rejects_values_above_policy_limit() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_seconds: Some(MAX_DELAY_SECONDS + 0.5),
            play_sound: None,
            ..CommonCaptureParams::default()
        })
        .expect_err("delay above policy should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_output_defaults_use_analysis_png_profile() {
        let parsed = parse_common_params(&CommonCaptureParams::default())
            .expect("default params should parse");
        assert_eq!(parsed.output.preset, OutputPreset::Analysis);
        assert_eq!(parsed.output.format, CaptureOutputFormat::Png);
        assert_eq!(
            parsed.output.max_dimension,
            Some(DEFAULT_ANALYSIS_MAX_DIMENSION)
        );
    }

    #[test]
    fn mcp_tools_output_compact_maps_to_jpeg_defaults() {
        let parsed = parse_common_params(&CommonCaptureParams {
            output_preset: Some(OutputPreset::Compact),
            ..CommonCaptureParams::default()
        })
        .expect("compact profile should parse");

        assert_eq!(parsed.output.format, CaptureOutputFormat::Jpeg);
        assert_eq!(parsed.output.jpeg_quality, DEFAULT_JPEG_QUALITY);
        assert_eq!(
            parsed.output.max_dimension,
            Some(DEFAULT_COMPACT_MAX_DIMENSION)
        );
    }

    #[test]
    fn mcp_tools_output_rejects_jpeg_quality_when_output_not_jpeg() {
        let error = parse_common_params(&CommonCaptureParams {
            output_format: Some(OutputFormat::Png),
            jpeg_quality: Some(80),
            ..CommonCaptureParams::default()
        })
        .expect_err("jpeg quality on png should fail");

        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_output_accepts_jpeg_override_with_quality_and_dimension() {
        let parsed = parse_common_params(&CommonCaptureParams {
            output_preset: Some(OutputPreset::Exact),
            output_format: Some(OutputFormat::Jpeg),
            jpeg_quality: Some(90),
            max_dimension: Some(1400),
            ..CommonCaptureParams::default()
        })
        .expect("overrides should parse");

        assert_eq!(parsed.output.format, CaptureOutputFormat::Jpeg);
        assert_eq!(parsed.output.jpeg_quality, 90);
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
    fn mcp_tools_validation_rect_parser_rejects_oversized_dimension() {
        let error = parse_rect_input(&CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 0,
            y: 0,
            width: i64::from(MAX_CAPTURE_DIMENSION) + 1,
            height: 10,
        })
        .expect_err("dimension over limit should fail");
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
    fn mcp_tools_output_supports_webp_override() {
        let parsed = parse_common_params(&CommonCaptureParams {
            output_format: Some(OutputFormat::Webp),
            ..CommonCaptureParams::default()
        })
        .expect("webp override should parse");
        assert_eq!(parsed.output.format, CaptureOutputFormat::Webp);
    }

    #[test]
    fn mcp_tools_delay_parser_accepts_zero_and_rejects_negative() {
        assert_eq!(parse_delay_seconds(0.0).expect("zero delay").as_millis(), 0);
        let error = parse_delay_seconds(-0.1).expect_err("negative delay should fail");
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
