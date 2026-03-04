use image::RgbaImage;
use xcap::{Monitor, Window, XCapError};

use crate::{
    capture::{
        backend::CaptureBackend,
        region::{
            GlobalRect, MonitorBounds, Point, center_square_on_cursor, global_to_local_rect,
            rect_contains_point,
        },
    },
    mcp::errors::ServerError,
};

#[derive(Debug, Clone, Default)]
pub struct XcapBackend;

impl XcapBackend {
    pub const fn new() -> Self {
        Self
    }
}

impl CaptureBackend for XcapBackend {
    fn capture_screen(&self) -> Result<RgbaImage, ServerError> {
        let monitors = Monitor::all().map_err(map_monitor_error)?;
        let mut primary: Option<Monitor> = None;

        for monitor in monitors {
            if monitor.is_primary().map_err(map_monitor_error)? {
                primary = Some(monitor);
                break;
            }
        }

        let monitor = primary
            .ok_or_else(|| ServerError::monitor_not_found("primary monitor could not be found"))?;

        monitor.capture_image().map_err(map_monitor_error)
    }

    fn capture_active_window(&self) -> Result<RgbaImage, ServerError> {
        let windows = Window::all().map_err(map_window_error)?;
        let descriptors = descriptors_from_windows(&windows)?;

        let Some(index) = select_focused_window_index(&descriptors) else {
            return Err(ServerError::window_not_found(
                "focused non-minimized window could not be found",
            ));
        };

        windows[index].capture_image().map_err(map_window_error)
    }

    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError> {
        let windows = Window::all().map_err(map_window_error)?;
        let descriptors = descriptors_from_windows(&windows)?;

        let Some(index) = select_window_at_cursor_index(&descriptors, cursor) else {
            return Err(ServerError::window_not_found(
                "no non-minimized window contains the cursor point",
            ));
        };

        windows[index].capture_image().map_err(map_window_error)
    }

    fn capture_cursor_region(&self, cursor: Point, size: u32) -> Result<RgbaImage, ServerError> {
        let rect = center_square_on_cursor(cursor, size)?;
        self.capture_rect(rect)
    }

    fn capture_rect(&self, rect: GlobalRect) -> Result<RgbaImage, ServerError> {
        let monitor = Monitor::from_point(rect.x, rect.y).map_err(map_monitor_error)?;
        let bounds = monitor_bounds(&monitor)?;
        let local = global_to_local_rect(rect, bounds)?;

        monitor
            .capture_region(local.x, local.y, local.width, local.height)
            .map_err(map_region_error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowDescriptor {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    is_minimized: bool,
    is_focused: bool,
}

fn descriptors_from_windows(windows: &[Window]) -> Result<Vec<WindowDescriptor>, ServerError> {
    windows
        .iter()
        .map(|window| {
            Ok(WindowDescriptor {
                x: window.x().map_err(map_window_error)?,
                y: window.y().map_err(map_window_error)?,
                width: window.width().map_err(map_window_error)?,
                height: window.height().map_err(map_window_error)?,
                is_minimized: window.is_minimized().map_err(map_window_error)?,
                is_focused: window.is_focused().map_err(map_window_error)?,
            })
        })
        .collect()
}

fn select_focused_window_index(descriptors: &[WindowDescriptor]) -> Option<usize> {
    descriptors
        .iter()
        .enumerate()
        .find(|(_, descriptor)| descriptor.is_focused && !descriptor.is_minimized)
        .map(|(index, _)| index)
}

fn select_window_at_cursor_index(descriptors: &[WindowDescriptor], cursor: Point) -> Option<usize> {
    descriptors
        .iter()
        .enumerate()
        .find(|(_, descriptor)| {
            !descriptor.is_minimized
                && rect_contains_point(
                    descriptor.x,
                    descriptor.y,
                    descriptor.width,
                    descriptor.height,
                    cursor,
                )
        })
        .map(|(index, _)| index)
}

fn monitor_bounds(monitor: &Monitor) -> Result<MonitorBounds, ServerError> {
    Ok(MonitorBounds {
        x: monitor.x().map_err(map_monitor_error)?,
        y: monitor.y().map_err(map_monitor_error)?,
        width: monitor.width().map_err(map_monitor_error)?,
        height: monitor.height().map_err(map_monitor_error)?,
    })
}

#[derive(Debug, Clone, Copy)]
enum XcapErrorContext {
    Monitor,
    Window,
    Region,
}

fn map_monitor_error(error: XCapError) -> ServerError {
    map_xcap_error(error, XcapErrorContext::Monitor)
}

fn map_window_error(error: XCapError) -> ServerError {
    map_xcap_error(error, XcapErrorContext::Window)
}

fn map_region_error(error: XCapError) -> ServerError {
    map_xcap_error(error, XcapErrorContext::Region)
}

fn map_xcap_error(error: XCapError, context: XcapErrorContext) -> ServerError {
    match error {
        XCapError::InvalidCaptureRegion(message) => {
            ServerError::invalid_region(format!("invalid capture region: {message}"))
        }
        XCapError::NotSupported => ServerError::capture_unsupported_on_platform(
            "capture is not supported on this platform backend",
        ),
        XCapError::Error(message) | XCapError::StdSyncPoisonError(message) => {
            map_xcap_message(message, context)
        }
        other => fallback_backend_error(context, format!("capture backend failed: {other}")),
    }
}

fn map_xcap_message(message: String, context: XcapErrorContext) -> ServerError {
    let lowered = message.to_lowercase();
    if lowered.contains("monitor") {
        return ServerError::monitor_not_found(message);
    }
    if lowered.contains("window") {
        return ServerError::window_not_found(message);
    }

    if lowered.contains("region") || lowered.contains("bound") {
        return ServerError::invalid_region(format!("invalid capture region: {message}"));
    }

    fallback_backend_error(context, format!("capture backend failed: {message}"))
}

fn fallback_backend_error(context: XcapErrorContext, message: String) -> ServerError {
    match context {
        XcapErrorContext::Monitor => ServerError::monitor_not_found(message),
        XcapErrorContext::Window => ServerError::window_not_found(message),
        XcapErrorContext::Region => ServerError::invalid_region(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_select_focused_window_picks_non_minimized() {
        let descriptors = vec![
            WindowDescriptor {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                is_minimized: true,
                is_focused: true,
            },
            WindowDescriptor {
                x: 10,
                y: 10,
                width: 20,
                height: 20,
                is_minimized: false,
                is_focused: true,
            },
        ];

        assert_eq!(select_focused_window_index(&descriptors), Some(1));
    }

    #[test]
    fn capture_select_window_at_cursor_uses_backend_order() {
        let descriptors = vec![
            WindowDescriptor {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                is_minimized: false,
                is_focused: false,
            },
            WindowDescriptor {
                x: 10,
                y: 10,
                width: 30,
                height: 30,
                is_minimized: false,
                is_focused: false,
            },
        ];

        let cursor = Point::new(15, 15);
        assert_eq!(select_window_at_cursor_index(&descriptors, cursor), Some(0));
    }

    #[test]
    fn capture_error_mapping_monitor_context_avoids_storage_failed() {
        let error = map_monitor_error(XCapError::Error("boom".to_owned()));
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_error_mapping_window_context_avoids_storage_failed() {
        let error = map_window_error(XCapError::Error("boom".to_owned()));
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_error_mapping_region_context_avoids_storage_failed() {
        let error = map_region_error(XCapError::Error("boom".to_owned()));
        assert_eq!(error.error_code(), "invalid_region");
    }
}
