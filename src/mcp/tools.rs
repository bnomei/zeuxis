use std::{sync::Arc, time::Duration};

use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{
    capture::{backend::CaptureBackend, region::GlobalRect},
    cursor::CursorProvider,
    mcp::{errors::ServerError, result},
};

use super::server::ZeuxisScreenshotServer;

const MAX_DELAY_SECONDS: f64 = 30.0;
const MAX_CAPTURE_DIMENSION: u32 = 16_384;
const MAX_CAPTURE_PIXELS: u64 = 40_000_000;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct CommonCaptureParams {
    pub delay_seconds: Option<f64>,
    pub play_sound: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureCursorRegionParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    pub size: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CaptureRectParams {
    #[serde(flatten)]
    pub common: CommonCaptureParams,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[tool_router(router = tool_router)]
impl ZeuxisScreenshotServer {
    #[tool(
        name = "capture_screen",
        description = "Capture the primary monitor into a local PNG artifact.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn capture_screen(
        &self,
        params: Parameters<CommonCaptureParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(self
            .execute_capture("capture_screen", params.0, |backend, _cursor| {
                backend.capture_screen()
            })
            .await)
    }

    #[tool(
        name = "capture_active_window",
        description = "Capture the currently focused, non-minimized window into a local PNG artifact.",
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
        description = "Capture a best-effort non-minimized window that contains the cursor point.",
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
        description = "Capture a square region centered on the current cursor position.",
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
        description = "Capture an explicit global rectangle into a local PNG artifact.",
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
        info!(
            tool = capture_mode,
            phase = "start",
            "tool invocation started"
        );

        let delay = match parse_common_params(&common) {
            Ok(delay) => delay,
            Err(error) => {
                error!(
                    tool = capture_mode,
                    phase = "validation_error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "input validation failed"
                );
                return result::error_result(&error);
            }
        };

        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }

        if let Err(error) = self.permission_gate.ensure_capture_allowed() {
            error!(
                tool = capture_mode,
                phase = "permission_error",
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

        let artifact = match tokio::task::spawn_blocking(move || {
            let image = capture_fn(&backend, &cursor_provider)?;
            storage.write_png(&image, capture_mode)
        })
        .await
        {
            Ok(Ok(artifact)) => artifact,
            Ok(Err(error)) => {
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "capture failed"
                );
                return result::error_result(&error);
            }
            Err(join_error) => {
                let error = ServerError::storage_failed(format!(
                    "capture worker task failed: {join_error}"
                ));
                error!(
                    tool = capture_mode,
                    phase = "capture_error",
                    error_code = error.error_code(),
                    message = error.message(),
                    "capture failed"
                );
                return result::error_result(&error);
            }
        };

        if common.play_sound.unwrap_or(false) {
            emit_capture_feedback();
        }

        info!(
            tool = capture_mode,
            phase = "complete",
            path = %artifact.path.display(),
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

fn parse_common_params(common: &CommonCaptureParams) -> Result<Option<Duration>, ServerError> {
    common.delay_seconds.map(parse_delay_seconds).transpose()
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

    Duration::try_from_secs_f64(delay_seconds).map_err(|_| {
        ServerError::invalid_params("delay_seconds is outside the supported range for duration")
    })
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

fn emit_capture_feedback() {
    eprint!("\x07");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tools_validation_common_delay_rejects_nan() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_seconds: Some(f64::NAN),
            play_sound: None,
        })
        .expect_err("nan should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_validation_common_delay_rejects_excessive_value() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_seconds: Some(1e30),
            play_sound: None,
        })
        .expect_err("overflowing delay should fail");
        assert_eq!(error.error_code(), "invalid_params");
    }

    #[test]
    fn mcp_tools_validation_common_delay_rejects_values_above_policy_limit() {
        let error = parse_common_params(&CommonCaptureParams {
            delay_seconds: Some(MAX_DELAY_SECONDS + 0.5),
            play_sound: None,
        })
        .expect_err("delay above policy should fail");
        assert_eq!(error.error_code(), "invalid_params");
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
}
