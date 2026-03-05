use std::{
    error::Error,
    fs,
    io::{Read, Write},
    path::Path,
};

use crate::{
    capture::{
        backend::{CaptureBackend, MonitorInfo, WindowInfo},
        region::{GlobalRect, Point, center_square_on_cursor, rect_contains_point},
        xcap_backend::XcapBackend,
    },
    cursor::{CursorProvider, DeviceQueryCursorProvider},
    mcp::{errors::ServerError, result},
};

use super::contract::{
    CaptureOperation, WorkerErrorPayload, WorkerRequest, WorkerResponse, WorkerSuccessPayload,
    parse_request_json,
};

const INPUT_UNITS_POINTS: &str = "points";
const MAX_CAPTURE_DIMENSION: u32 = 16_384;
const MAX_CAPTURE_PIXELS: u64 = 40_000_000;

struct CaptureWorkOutput {
    image: image::RgbaImage,
    target: result::CaptureTargetPayload,
    input_units: String,
    input_width: Option<u32>,
    input_height: Option<u32>,
}

pub fn run_stdio_worker() -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let response = match parse_request_json(&input) {
        Ok(request) => handle_request(request),
        Err(error) => WorkerResponse::error("unknown", error),
    };

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &response)?;
    handle.write_all(b"\n")?;
    handle.flush()?;

    Ok(())
}

fn handle_request(request: WorkerRequest) -> WorkerResponse {
    match execute_request(&request) {
        Ok(result) => WorkerResponse::success(request.request_id, result),
        Err(error) => WorkerResponse::error(
            request.request_id,
            WorkerErrorPayload::from_server_error(&error),
        ),
    }
}

fn execute_request(request: &WorkerRequest) -> Result<WorkerSuccessPayload, ServerError> {
    request
        .validate()
        .map_err(|error| error.to_server_error())?;

    let backend = XcapBackend::new();
    let cursor_provider = DeviceQueryCursorProvider::new();
    let work_output = execute_operation(&backend, &cursor_provider, &request.operation)?;
    let source_width = work_output.image.width();
    let source_height = work_output.image.height();

    let image = match request.output.max_dimension {
        Some(max_dimension) => downscale_if_needed(work_output.image, max_dimension),
        None => work_output.image,
    };
    let width = image.width();
    let height = image.height();

    write_image_to_path(
        image,
        Path::new(&request.artifact_path),
        request.output.format,
        request.output.jpeg_quality,
    )?;

    Ok(WorkerSuccessPayload {
        artifact_path: request.artifact_path.clone(),
        output_format: request.output.format.as_str().to_owned(),
        mime_type: request.output.format.mime_type().to_owned(),
        width,
        height,
        source_width,
        source_height,
        input_units: work_output.input_units,
        input_width: work_output.input_width,
        input_height: work_output.input_height,
        target: work_output.target,
    })
}

fn execute_operation(
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
            validate_capture_dimensions(*size, *size)?;
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
            validate_capture_dimensions(*width, *height)?;
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
            validate_capture_dimensions(*width, *height)?;
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

fn validate_capture_dimensions(width: u32, height: u32) -> Result<(), ServerError> {
    if width == 0 || height == 0 {
        return Err(ServerError::invalid_params(
            "capture dimensions must be > 0",
        ));
    }
    if width > MAX_CAPTURE_DIMENSION || height > MAX_CAPTURE_DIMENSION {
        return Err(ServerError::invalid_params(format!(
            "capture dimensions exceed supported max {MAX_CAPTURE_DIMENSION}"
        )));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_CAPTURE_PIXELS {
        return Err(ServerError::invalid_params(format!(
            "capture area exceeds supported max {MAX_CAPTURE_PIXELS} pixels"
        )));
    }
    Ok(())
}

fn downscale_if_needed(image: image::RgbaImage, max_dimension: u32) -> image::RgbaImage {
    if max_dimension == 0 {
        return image;
    }

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

fn write_image_to_path(
    image: image::RgbaImage,
    path: &Path,
    format: super::contract::WorkerOutputFormat,
    jpeg_quality: u8,
) -> Result<(), ServerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ServerError::storage_failed(format!(
                "failed to create worker artifact directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let file = fs::File::create(path).map_err(|err| {
        ServerError::storage_failed(format!(
            "failed to create worker artifact {}: {err}",
            path.display()
        ))
    })?;
    let mut writer = std::io::BufWriter::new(file);
    let dynamic = image::DynamicImage::ImageRgba8(image);
    let encoded = match format {
        super::contract::WorkerOutputFormat::Png => {
            dynamic.write_to(&mut writer, image::ImageFormat::Png)
        }
        super::contract::WorkerOutputFormat::Jpeg => {
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, jpeg_quality);
            encoder.encode_image(&dynamic)
        }
        super::contract::WorkerOutputFormat::Webp => {
            dynamic.write_to(&mut writer, image::ImageFormat::WebP)
        }
    };
    encoded.map_err(|err| {
        ServerError::encode_failed(format!(
            "failed to encode {} in worker: {err}",
            format.as_str()
        ))
    })?;
    writer.flush().map_err(|err| {
        ServerError::storage_failed(format!(
            "failed to flush worker artifact {}: {err}",
            path.display()
        ))
    })?;
    Ok(())
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
    cursor: Point,
    include_system_windows: bool,
) -> Result<WindowInfo, ServerError> {
    let windows = backend.list_windows()?;
    let windows = filter_windows_for_cursor(windows, include_system_windows);
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
                    "no non-system, non-minimized window contains the cursor point; set include_system_windows=true to allow system surfaces",
                )
            }
        })
}

fn filter_windows_for_cursor(
    mut windows: Vec<WindowInfo>,
    include_system_windows: bool,
) -> Vec<WindowInfo> {
    if !include_system_windows {
        windows.retain(|window| !is_system_window(window));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::contract::{
        WORKER_CONTRACT_VERSION, WorkerOutputFormat, WorkerOutputOptions,
    };

    #[test]
    fn worker_child_handle_request_returns_error_for_missing_payload() {
        let response = handle_request(WorkerRequest {
            v: WORKER_CONTRACT_VERSION,
            request_id: "req-1".to_owned(),
            operation: CaptureOperation::CaptureScreen { monitor_id: None },
            output: WorkerOutputOptions {
                format: WorkerOutputFormat::Png,
                jpeg_quality: 82,
                max_dimension: Some(256),
            },
            artifact_path: String::new(),
        });
        assert!(!response.ok);
        let error = response.error.expect("error payload");
        assert_eq!(error.error_code, "invalid_params");
    }
}
