//! Production `xcap` adapter for monitor, window, and region capture.
//!
//! This module keeps platform-specific `xcap` behavior behind `CaptureBackend`,
//! including monitor/window selection and best-effort normalization of backend
//! errors into stable MCP error codes.

use image::RgbaImage;
use xcap::{Monitor, Window, XCapError};

use crate::{
    capture::{
        backend::{CaptureBackend, MonitorInfo, WindowInfo},
        region::{
            GlobalRect, MonitorBounds, Point, center_square_on_cursor, global_to_local_rect,
            rect_contains_point,
        },
    },
    mcp::errors::ServerError,
};

/// Production xcap source backed by the current graphical session.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemXcapSource;

// Small traits make the xcap adapter fakeable in tests without exposing xcap
// concrete types throughout the rest of the crate.
trait MonitorLike {
    fn id(&self) -> Result<u32, XCapError>;
    fn name(&self) -> Result<String, XCapError>;
    fn x(&self) -> Result<i32, XCapError>;
    fn y(&self) -> Result<i32, XCapError>;
    fn width(&self) -> Result<u32, XCapError>;
    fn height(&self) -> Result<u32, XCapError>;
    fn is_primary(&self) -> Result<bool, XCapError>;
    fn is_builtin(&self) -> Result<bool, XCapError>;
    fn capture_image(&self) -> Result<RgbaImage, XCapError>;
    fn capture_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, XCapError>;
}

trait WindowLike {
    fn id(&self) -> Result<u32, XCapError>;
    fn app_name(&self) -> Result<String, XCapError>;
    fn title(&self) -> Result<String, XCapError>;
    fn x(&self) -> Result<i32, XCapError>;
    fn y(&self) -> Result<i32, XCapError>;
    fn width(&self) -> Result<u32, XCapError>;
    fn height(&self) -> Result<u32, XCapError>;
    fn is_minimized(&self) -> Result<bool, XCapError>;
    fn is_focused(&self) -> Result<bool, XCapError>;
    fn capture_image(&self) -> Result<RgbaImage, XCapError>;
}

trait XcapSource: Clone + Send + Sync + 'static {
    type Monitor: MonitorLike;
    type Window: WindowLike;

    fn all_monitors(&self) -> Result<Vec<Self::Monitor>, XCapError>;
    fn monitor_from_point(&self, x: i32, y: i32) -> Result<Self::Monitor, XCapError>;
    fn all_windows(&self) -> Result<Vec<Self::Window>, XCapError>;
}

impl MonitorLike for Monitor {
    fn id(&self) -> Result<u32, XCapError> {
        Monitor::id(self)
    }

    fn name(&self) -> Result<String, XCapError> {
        Monitor::name(self)
    }

    fn x(&self) -> Result<i32, XCapError> {
        Monitor::x(self)
    }

    fn y(&self) -> Result<i32, XCapError> {
        Monitor::y(self)
    }

    fn width(&self) -> Result<u32, XCapError> {
        Monitor::width(self)
    }

    fn height(&self) -> Result<u32, XCapError> {
        Monitor::height(self)
    }

    fn is_primary(&self) -> Result<bool, XCapError> {
        Monitor::is_primary(self)
    }

    fn is_builtin(&self) -> Result<bool, XCapError> {
        Monitor::is_builtin(self)
    }

    fn capture_image(&self) -> Result<RgbaImage, XCapError> {
        Monitor::capture_image(self)
    }

    fn capture_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, XCapError> {
        Monitor::capture_region(self, x, y, width, height)
    }
}

impl WindowLike for Window {
    fn id(&self) -> Result<u32, XCapError> {
        Window::id(self)
    }

    fn app_name(&self) -> Result<String, XCapError> {
        Window::app_name(self)
    }

    fn title(&self) -> Result<String, XCapError> {
        Window::title(self)
    }

    fn x(&self) -> Result<i32, XCapError> {
        Window::x(self)
    }

    fn y(&self) -> Result<i32, XCapError> {
        Window::y(self)
    }

    fn width(&self) -> Result<u32, XCapError> {
        Window::width(self)
    }

    fn height(&self) -> Result<u32, XCapError> {
        Window::height(self)
    }

    fn is_minimized(&self) -> Result<bool, XCapError> {
        Window::is_minimized(self)
    }

    fn is_focused(&self) -> Result<bool, XCapError> {
        Window::is_focused(self)
    }

    fn capture_image(&self) -> Result<RgbaImage, XCapError> {
        Window::capture_image(self)
    }
}

impl XcapSource for SystemXcapSource {
    type Monitor = Monitor;
    type Window = Window;

    fn all_monitors(&self) -> Result<Vec<Self::Monitor>, XCapError> {
        Monitor::all()
    }

    fn monitor_from_point(&self, x: i32, y: i32) -> Result<Self::Monitor, XCapError> {
        Monitor::from_point(x, y)
    }

    fn all_windows(&self) -> Result<Vec<Self::Window>, XCapError> {
        Window::all()
    }
}

/// `CaptureBackend` implementation backed by the `xcap` crate.
///
/// The default backend selects the primary monitor when no monitor ID is
/// supplied and uses backend window order for cursor-window selection.
#[derive(Debug, Clone)]
pub struct XcapBackend<S = SystemXcapSource> {
    source: S,
}

impl XcapBackend<SystemXcapSource> {
    /// Creates a backend connected to the system xcap source.
    pub const fn new() -> Self {
        Self {
            source: SystemXcapSource,
        }
    }
}

impl Default for XcapBackend<SystemXcapSource> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl<S> XcapBackend<S> {
    fn with_source(source: S) -> Self {
        Self { source }
    }
}

impl<S> CaptureBackend for XcapBackend<S>
where
    S: XcapSource,
{
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ServerError> {
        let monitors = self.source.all_monitors().map_err(map_monitor_error)?;
        monitor_infos_from_monitors(&monitors)
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, ServerError> {
        let windows = self.source.all_windows().map_err(map_window_error)?;
        window_infos_from_windows(&windows)
    }

    fn capture_screen(&self, monitor_id: Option<u32>) -> Result<RgbaImage, ServerError> {
        let monitors = self.source.all_monitors().map_err(map_monitor_error)?;
        let monitor_selectors = monitor_selector_entries(&monitors)?;
        let index = select_monitor_index(&monitor_selectors, monitor_id)?;

        monitors[index].capture_image().map_err(map_monitor_error)
    }

    fn capture_window(&self, window_id: u32) -> Result<RgbaImage, ServerError> {
        let windows = self.source.all_windows().map_err(map_window_error)?;
        let window_selectors = window_selector_entries(&windows)?;
        let index = select_window_index_by_id(&window_selectors, window_id)?;

        windows[index].capture_image().map_err(map_window_error)
    }

    fn capture_monitor_region(
        &self,
        monitor_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, ServerError> {
        let monitors = self.source.all_monitors().map_err(map_monitor_error)?;
        let monitor_selectors = monitor_selector_entries(&monitors)?;
        let index = select_monitor_index_by_id(&monitor_selectors, monitor_id)?;
        let bounds = monitor_bounds(&monitors[index])?;
        validate_monitor_local_region(bounds, x, y, width, height)?;

        monitors[index]
            .capture_region(x, y, width, height)
            .map_err(map_region_error)
    }

    fn capture_active_window(&self) -> Result<RgbaImage, ServerError> {
        let windows = self.source.all_windows().map_err(map_window_error)?;
        let descriptors = descriptors_from_windows(&windows)?;

        let Some(index) = select_focused_window_index(&descriptors) else {
            return Err(ServerError::window_not_found(
                "focused non-minimized window could not be found",
            ));
        };

        windows[index].capture_image().map_err(map_window_error)
    }

    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError> {
        let windows = self.source.all_windows().map_err(map_window_error)?;
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
        // xcap resolves a monitor from one point, so require the whole global
        // rectangle to fit the monitor that contains the rectangle origin.
        let monitor = self
            .source
            .monitor_from_point(rect.x, rect.y)
            .map_err(map_monitor_error)?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MonitorSelectorEntry {
    index: usize,
    id: u32,
    is_primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowSelectorEntry {
    index: usize,
    id: u32,
}

fn monitor_infos_from_monitors<M: MonitorLike>(
    monitors: &[M],
) -> Result<Vec<MonitorInfo>, ServerError> {
    monitors
        .iter()
        .map(|monitor| {
            Ok(MonitorInfo {
                id: monitor.id().map_err(map_monitor_error)?,
                name: monitor.name().map_err(map_monitor_error)?,
                x: monitor.x().map_err(map_monitor_error)?,
                y: monitor.y().map_err(map_monitor_error)?,
                width: monitor.width().map_err(map_monitor_error)?,
                height: monitor.height().map_err(map_monitor_error)?,
                is_primary: monitor.is_primary().map_err(map_monitor_error)?,
                is_builtin: monitor.is_builtin().map_err(map_monitor_error)?,
            })
        })
        .collect()
}

fn monitor_selector_entries<M: MonitorLike>(
    monitors: &[M],
) -> Result<Vec<MonitorSelectorEntry>, ServerError> {
    monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| {
            Ok(MonitorSelectorEntry {
                index,
                id: monitor.id().map_err(map_monitor_error)?,
                is_primary: monitor.is_primary().map_err(map_monitor_error)?,
            })
        })
        .collect()
}

fn window_infos_from_windows<W: WindowLike>(windows: &[W]) -> Result<Vec<WindowInfo>, ServerError> {
    windows
        .iter()
        .map(|window| {
            Ok(WindowInfo {
                id: window.id().map_err(map_window_error)?,
                title: window.title().map_err(map_window_error)?,
                app: window.app_name().map_err(map_window_error)?,
                x: window.x().map_err(map_window_error)?,
                y: window.y().map_err(map_window_error)?,
                width: window.width().map_err(map_window_error)?,
                height: window.height().map_err(map_window_error)?,
                is_focused: window.is_focused().map_err(map_window_error)?,
                is_minimized: window.is_minimized().map_err(map_window_error)?,
            })
        })
        .collect()
}

fn window_selector_entries<W: WindowLike>(
    windows: &[W],
) -> Result<Vec<WindowSelectorEntry>, ServerError> {
    windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            Ok(WindowSelectorEntry {
                index,
                id: window.id().map_err(map_window_error)?,
            })
        })
        .collect()
}

fn descriptors_from_windows<W: WindowLike>(
    windows: &[W],
) -> Result<Vec<WindowDescriptor>, ServerError> {
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

fn select_monitor_index(
    monitors: &[MonitorSelectorEntry],
    requested_id: Option<u32>,
) -> Result<usize, ServerError> {
    if monitors.is_empty() {
        return Err(ServerError::monitor_not_found("no monitors could be found"));
    }

    if let Some(requested_id) = requested_id {
        return select_monitor_index_by_id(monitors, requested_id);
    }

    monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .map(|monitor| monitor.index)
        .ok_or_else(|| ServerError::monitor_not_found("primary monitor could not be found"))
}

fn select_monitor_index_by_id(
    monitors: &[MonitorSelectorEntry],
    requested_id: u32,
) -> Result<usize, ServerError> {
    monitors
        .iter()
        .find(|monitor| monitor.id == requested_id)
        .map(|monitor| monitor.index)
        .ok_or_else(|| {
            ServerError::monitor_not_found(format!(
                "monitor with id {requested_id} could not be found"
            ))
        })
}

fn select_window_index_by_id(
    windows: &[WindowSelectorEntry],
    requested_id: u32,
) -> Result<usize, ServerError> {
    windows
        .iter()
        .find(|window| window.id == requested_id)
        .map(|window| window.index)
        .ok_or_else(|| {
            ServerError::window_not_found(format!(
                "window with id {requested_id} could not be found"
            ))
        })
}

fn select_focused_window_index(descriptors: &[WindowDescriptor]) -> Option<usize> {
    descriptors
        .iter()
        .enumerate()
        .find(|(_, descriptor)| descriptor.is_focused && !descriptor.is_minimized)
        .map(|(index, _)| index)
}

fn select_window_at_cursor_index(descriptors: &[WindowDescriptor], cursor: Point) -> Option<usize> {
    // Preserve backend order, which xcap treats as the topmost-to-bottom window
    // order used by cursor-window capture.
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

fn monitor_bounds<M: MonitorLike>(monitor: &M) -> Result<MonitorBounds, ServerError> {
    Ok(MonitorBounds {
        x: monitor.x().map_err(map_monitor_error)?,
        y: monitor.y().map_err(map_monitor_error)?,
        width: monitor.width().map_err(map_monitor_error)?,
        height: monitor.height().map_err(map_monitor_error)?,
    })
}

fn validate_monitor_local_region(
    bounds: MonitorBounds,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<(), ServerError> {
    if width == 0 || height == 0 {
        return Err(ServerError::invalid_region(
            "width and height must be greater than 0",
        ));
    }

    let right = x
        .checked_add(width)
        .ok_or_else(|| ServerError::invalid_region("rectangle width overflows coordinate range"))?;
    let bottom = y.checked_add(height).ok_or_else(|| {
        ServerError::invalid_region("rectangle height overflows coordinate range")
    })?;

    if right > bounds.width || bottom > bounds.height {
        return Err(ServerError::invalid_region(
            "requested rectangle is outside monitor bounds",
        ));
    }

    Ok(())
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
    // xcap reports many platform failures as strings; route known phrases to
    // stable MCP error codes before falling back to the operation context.
    #[cfg(target_os = "linux")]
    if let Some(mapped) = map_linux_xcap_message(&message, &lowered) {
        return mapped;
    }

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

#[cfg(target_os = "linux")]
fn map_linux_xcap_message(message: &str, lowered: &str) -> Option<ServerError> {
    if lowered.contains("permission denied")
        || lowered.contains("access denied")
        || lowered.contains("not authorized")
    {
        return Some(ServerError::permission_denied(format!(
            "screen capture permission denied on Linux backend: {message}"
        )));
    }

    if lowered.contains("wayland")
        || lowered.contains("xdg-desktop-portal")
        || lowered.contains("portal")
        || lowered.contains("pipewire")
        || lowered.contains("compositor")
    {
        return Some(ServerError::capture_unsupported_on_platform(format!(
            "Linux capture backend is unavailable in this session: {message}. Ensure xdg-desktop-portal/pipewire support or try an X11 session"
        )));
    }

    if lowered.contains("cannot open display")
        || lowered.contains("failed to open display")
        || lowered.contains("no display")
        || (lowered.contains("display") && lowered.contains("not found"))
    {
        return Some(ServerError::capture_unsupported_on_platform(format!(
            "Linux display server is unavailable for capture: {message}. Ensure DISPLAY or WAYLAND_DISPLAY is set and a graphical session is active"
        )));
    }

    None
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

    #[derive(Debug, Clone)]
    struct FakeMonitor {
        id: u32,
        name: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        is_primary: bool,
        is_builtin: bool,
        image_error: Option<String>,
        region_error: Option<String>,
    }

    impl FakeMonitor {
        fn image(&self) -> RgbaImage {
            RgbaImage::from_pixel(8, 6, image::Rgba([self.id as u8, 0, 0, 255]))
        }
    }

    impl MonitorLike for FakeMonitor {
        fn id(&self) -> Result<u32, XCapError> {
            Ok(self.id)
        }

        fn name(&self) -> Result<String, XCapError> {
            Ok(self.name.clone())
        }

        fn x(&self) -> Result<i32, XCapError> {
            Ok(self.x)
        }

        fn y(&self) -> Result<i32, XCapError> {
            Ok(self.y)
        }

        fn width(&self) -> Result<u32, XCapError> {
            Ok(self.width)
        }

        fn height(&self) -> Result<u32, XCapError> {
            Ok(self.height)
        }

        fn is_primary(&self) -> Result<bool, XCapError> {
            Ok(self.is_primary)
        }

        fn is_builtin(&self) -> Result<bool, XCapError> {
            Ok(self.is_builtin)
        }

        fn capture_image(&self) -> Result<RgbaImage, XCapError> {
            if let Some(message) = &self.image_error {
                return Err(XCapError::Error(message.clone()));
            }
            Ok(self.image())
        }

        fn capture_region(
            &self,
            _x: u32,
            _y: u32,
            width: u32,
            height: u32,
        ) -> Result<RgbaImage, XCapError> {
            if let Some(message) = &self.region_error {
                return Err(XCapError::Error(message.clone()));
            }
            Ok(RgbaImage::from_pixel(
                width,
                height,
                image::Rgba([self.id as u8, 9, 9, 255]),
            ))
        }
    }

    #[derive(Debug, Clone)]
    struct FakeWindow {
        id: u32,
        title: String,
        app_name: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        is_minimized: bool,
        is_focused: bool,
        image_error: Option<String>,
    }

    impl WindowLike for FakeWindow {
        fn id(&self) -> Result<u32, XCapError> {
            Ok(self.id)
        }

        fn app_name(&self) -> Result<String, XCapError> {
            Ok(self.app_name.clone())
        }

        fn title(&self) -> Result<String, XCapError> {
            Ok(self.title.clone())
        }

        fn x(&self) -> Result<i32, XCapError> {
            Ok(self.x)
        }

        fn y(&self) -> Result<i32, XCapError> {
            Ok(self.y)
        }

        fn width(&self) -> Result<u32, XCapError> {
            Ok(self.width)
        }

        fn height(&self) -> Result<u32, XCapError> {
            Ok(self.height)
        }

        fn is_minimized(&self) -> Result<bool, XCapError> {
            Ok(self.is_minimized)
        }

        fn is_focused(&self) -> Result<bool, XCapError> {
            Ok(self.is_focused)
        }

        fn capture_image(&self) -> Result<RgbaImage, XCapError> {
            if let Some(message) = &self.image_error {
                return Err(XCapError::Error(message.clone()));
            }
            Ok(RgbaImage::from_pixel(5, 4, image::Rgba([1, 1, 1, 255])))
        }
    }

    #[derive(Debug, Clone, Default)]
    struct FakeSource {
        monitors: Vec<FakeMonitor>,
        windows: Vec<FakeWindow>,
        monitors_error: Option<String>,
        windows_error: Option<String>,
        from_point_error: Option<String>,
    }

    impl XcapSource for FakeSource {
        type Monitor = FakeMonitor;
        type Window = FakeWindow;

        fn all_monitors(&self) -> Result<Vec<Self::Monitor>, XCapError> {
            if let Some(message) = &self.monitors_error {
                return Err(XCapError::Error(message.clone()));
            }
            Ok(self.monitors.clone())
        }

        fn monitor_from_point(&self, x: i32, y: i32) -> Result<Self::Monitor, XCapError> {
            if let Some(message) = &self.from_point_error {
                return Err(XCapError::Error(message.clone()));
            }

            self.monitors
                .iter()
                .find(|monitor| {
                    x >= monitor.x
                        && y >= monitor.y
                        && x < monitor.x + i32::try_from(monitor.width).unwrap_or(i32::MAX)
                        && y < monitor.y + i32::try_from(monitor.height).unwrap_or(i32::MAX)
                })
                .cloned()
                .ok_or_else(|| XCapError::Error("monitor not found for point".to_owned()))
        }

        fn all_windows(&self) -> Result<Vec<Self::Window>, XCapError> {
            if let Some(message) = &self.windows_error {
                return Err(XCapError::Error(message.clone()));
            }
            Ok(self.windows.clone())
        }
    }

    fn fake_source() -> FakeSource {
        FakeSource {
            monitors: vec![
                FakeMonitor {
                    id: 1,
                    name: "Primary".to_owned(),
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 80,
                    is_primary: true,
                    is_builtin: true,
                    image_error: None,
                    region_error: None,
                },
                FakeMonitor {
                    id: 2,
                    name: "Secondary".to_owned(),
                    x: 100,
                    y: 0,
                    width: 100,
                    height: 80,
                    is_primary: false,
                    is_builtin: false,
                    image_error: None,
                    region_error: None,
                },
            ],
            windows: vec![
                FakeWindow {
                    id: 11,
                    title: "Primary Window".to_owned(),
                    app_name: "TestApp".to_owned(),
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 40,
                    is_minimized: false,
                    is_focused: true,
                    image_error: None,
                },
                FakeWindow {
                    id: 22,
                    title: "Secondary Window".to_owned(),
                    app_name: "OtherApp".to_owned(),
                    x: 50,
                    y: 50,
                    width: 20,
                    height: 20,
                    is_minimized: false,
                    is_focused: false,
                    image_error: None,
                },
            ],
            ..FakeSource::default()
        }
    }

    #[test]
    fn capture_list_monitors_maps_source_data() {
        let backend = XcapBackend::with_source(fake_source());
        let monitors = backend.list_monitors().expect("list monitors");
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].name, "Primary");
        assert!(monitors[0].is_primary);
    }

    #[test]
    fn capture_list_windows_maps_source_data() {
        let backend = XcapBackend::with_source(fake_source());
        let windows = backend.list_windows().expect("list windows");
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, 11);
        assert_eq!(windows[0].title, "Primary Window");
        assert_eq!(windows[0].app, "TestApp");
        assert!(windows[0].is_focused);
    }

    #[test]
    fn capture_screen_uses_primary_monitor_when_id_is_omitted() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend.capture_screen(None).expect("capture screen");
        assert_eq!(image.get_pixel(0, 0).0[0], 1);
    }

    #[test]
    fn capture_screen_uses_requested_monitor_id() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend.capture_screen(Some(2)).expect("capture monitor 2");
        assert_eq!(image.get_pixel(0, 0).0[0], 2);
    }

    #[test]
    fn capture_screen_reports_monitor_not_found_for_unknown_id() {
        let backend = XcapBackend::with_source(fake_source());
        let error = backend
            .capture_screen(Some(999))
            .expect_err("unknown monitor should fail");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_screen_maps_monitor_capture_image_error() {
        let mut source = fake_source();
        source.monitors[0].image_error = Some("monitor capture failed".to_owned());
        let backend = XcapBackend::with_source(source);
        let error = backend
            .capture_screen(None)
            .expect_err("monitor capture image error should fail");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_window_uses_requested_window_id() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend.capture_window(22).expect("capture window 22");
        assert_eq!(image.width(), 5);
        assert_eq!(image.height(), 4);
    }

    #[test]
    fn capture_window_reports_window_not_found_for_unknown_id() {
        let backend = XcapBackend::with_source(fake_source());
        let error = backend
            .capture_window(999)
            .expect_err("unknown window id should fail");
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_monitor_region_uses_requested_monitor_and_local_region() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend
            .capture_monitor_region(2, 10, 10, 7, 6)
            .expect("capture monitor region");
        assert_eq!(image.width(), 7);
        assert_eq!(image.height(), 6);
        assert_eq!(image.get_pixel(0, 0).0[0], 2);
    }

    #[test]
    fn capture_monitor_region_reports_monitor_not_found_for_unknown_id() {
        let backend = XcapBackend::with_source(fake_source());
        let error = backend
            .capture_monitor_region(999, 10, 10, 7, 6)
            .expect_err("unknown monitor id should fail");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_monitor_region_rejects_out_of_bounds_local_region() {
        let backend = XcapBackend::with_source(fake_source());
        let error = backend
            .capture_monitor_region(1, 95, 70, 10, 20)
            .expect_err("out-of-bounds local region should fail");
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn capture_active_window_returns_focused_window_image() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend
            .capture_active_window()
            .expect("capture active window");
        assert_eq!(image.width(), 5);
        assert_eq!(image.height(), 4);
    }

    #[test]
    fn capture_active_window_returns_window_not_found_when_none_focused() {
        let mut source = fake_source();
        source
            .windows
            .iter_mut()
            .for_each(|window| window.is_focused = false);

        let backend = XcapBackend::with_source(source);
        let error = backend
            .capture_active_window()
            .expect_err("no focused window should fail");
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_window_at_cursor_selects_window_containing_cursor() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend
            .capture_window_at_cursor(Point::new(55, 55))
            .expect("capture window at cursor");
        assert_eq!(image.width(), 5);
    }

    #[test]
    fn capture_window_at_cursor_returns_window_not_found_when_no_match() {
        let backend = XcapBackend::with_source(fake_source());
        let error = backend
            .capture_window_at_cursor(Point::new(500, 500))
            .expect_err("no matching window should fail");
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_rect_captures_requested_dimensions() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend
            .capture_rect(GlobalRect {
                x: 10,
                y: 10,
                width: 7,
                height: 6,
            })
            .expect("capture rect");
        assert_eq!(image.width(), 7);
        assert_eq!(image.height(), 6);
    }

    #[test]
    fn capture_rect_rejects_out_of_bounds_region() {
        let backend = XcapBackend::with_source(fake_source());
        let error = backend
            .capture_rect(GlobalRect {
                x: 90,
                y: 70,
                width: 20,
                height: 20,
            })
            .expect_err("out of bounds region should fail");
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn capture_cursor_region_delegates_to_rect_capture() {
        let backend = XcapBackend::with_source(fake_source());
        let image = backend
            .capture_cursor_region(Point::new(25, 25), 10)
            .expect("capture cursor region");
        assert_eq!(image.width(), 10);
        assert_eq!(image.height(), 10);
    }

    #[test]
    fn capture_list_monitors_maps_monitor_backend_error() {
        let mut source = fake_source();
        source.monitors_error = Some("monitor service unavailable".to_owned());

        let backend = XcapBackend::with_source(source);
        let error = backend
            .list_monitors()
            .expect_err("monitor error should propagate");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

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
    fn capture_select_window_at_cursor_returns_none_when_no_windows_match() {
        let descriptors = vec![
            WindowDescriptor {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
                is_minimized: true,
                is_focused: false,
            },
            WindowDescriptor {
                x: 100,
                y: 100,
                width: 50,
                height: 50,
                is_minimized: false,
                is_focused: false,
            },
        ];

        let cursor = Point::new(30, 30);
        assert_eq!(select_window_at_cursor_index(&descriptors, cursor), None);
    }

    #[test]
    fn capture_descriptors_from_windows_empty_input_returns_empty() {
        let windows = Vec::<Window>::new();
        let descriptors = descriptors_from_windows(&windows).expect("empty windows should parse");
        assert!(descriptors.is_empty());
    }

    #[test]
    fn capture_select_monitor_index_prefers_primary_when_no_id_requested() {
        let monitors = vec![
            MonitorSelectorEntry {
                index: 0,
                id: 42,
                is_primary: false,
            },
            MonitorSelectorEntry {
                index: 1,
                id: 99,
                is_primary: true,
            },
        ];

        let selected = select_monitor_index(&monitors, None).expect("primary monitor");
        assert_eq!(selected, 1);
    }

    #[test]
    fn capture_select_monitor_index_matches_requested_id() {
        let monitors = vec![
            MonitorSelectorEntry {
                index: 0,
                id: 42,
                is_primary: true,
            },
            MonitorSelectorEntry {
                index: 1,
                id: 99,
                is_primary: false,
            },
        ];

        let selected = select_monitor_index(&monitors, Some(99)).expect("monitor by id");
        assert_eq!(selected, 1);
    }

    #[test]
    fn capture_select_monitor_index_rejects_unknown_requested_id() {
        let monitors = vec![MonitorSelectorEntry {
            index: 0,
            id: 42,
            is_primary: true,
        }];

        let error = select_monitor_index(&monitors, Some(7)).expect_err("unknown id should fail");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_select_monitor_index_rejects_empty_monitors() {
        let error = select_monitor_index(&[], None).expect_err("empty monitors should fail");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_active_window_maps_windows_source_error() {
        let mut source = fake_source();
        source.windows_error = Some("window service unavailable".to_owned());
        let backend = XcapBackend::with_source(source);
        let error = backend
            .capture_active_window()
            .expect_err("window error should propagate");
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_active_window_maps_window_capture_image_error() {
        let mut source = fake_source();
        source.windows[0].image_error = Some("window capture failed".to_owned());
        let backend = XcapBackend::with_source(source);
        let error = backend
            .capture_active_window()
            .expect_err("capture image error should propagate");
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_rect_maps_monitor_from_point_error() {
        let mut source = fake_source();
        source.from_point_error = Some("cannot open monitor".to_owned());
        let backend = XcapBackend::with_source(source);
        let error = backend
            .capture_rect(GlobalRect {
                x: 10,
                y: 10,
                width: 5,
                height: 5,
            })
            .expect_err("monitor lookup error should propagate");
        assert_eq!(error.error_code(), "monitor_not_found");
    }

    #[test]
    fn capture_rect_maps_region_capture_error() {
        let mut source = fake_source();
        source.monitors[0].region_error = Some("region encoder failed".to_owned());
        let backend = XcapBackend::with_source(source);
        let error = backend
            .capture_rect(GlobalRect {
                x: 10,
                y: 10,
                width: 5,
                height: 5,
            })
            .expect_err("region capture error should propagate");
        assert_eq!(error.error_code(), "invalid_region");
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

    #[test]
    fn capture_error_mapping_handles_invalid_capture_region_variant() {
        let error = map_xcap_error(
            XCapError::InvalidCaptureRegion("bad region".to_owned()),
            XcapErrorContext::Region,
        );
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn capture_error_mapping_handles_not_supported_variant() {
        let error = map_xcap_error(XCapError::NotSupported, XcapErrorContext::Monitor);
        assert_eq!(error.error_code(), "capture_unsupported_on_platform");
    }

    #[test]
    fn capture_error_mapping_maps_std_sync_poison_error_message() {
        let error = map_xcap_error(
            XCapError::StdSyncPoisonError("window poisoned".to_owned()),
            XcapErrorContext::Window,
        );
        assert_eq!(error.error_code(), "window_not_found");
    }

    #[test]
    fn capture_error_mapping_message_routes_monitor_window_and_region_keywords() {
        let monitor_error =
            map_xcap_message("monitor unavailable".to_owned(), XcapErrorContext::Window);
        assert_eq!(monitor_error.error_code(), "monitor_not_found");

        let window_error =
            map_xcap_message("window unavailable".to_owned(), XcapErrorContext::Monitor);
        assert_eq!(window_error.error_code(), "window_not_found");

        let region_error = map_xcap_message(
            "region bounds invalid".to_owned(),
            XcapErrorContext::Monitor,
        );
        assert_eq!(region_error.error_code(), "invalid_region");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_error_mapping_linux_wayland_message_maps_to_unsupported() {
        let error = map_xcap_message(
            "wayland compositor denied screencast".to_owned(),
            XcapErrorContext::Monitor,
        );
        assert_eq!(error.error_code(), "capture_unsupported_on_platform");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_error_mapping_linux_permission_message_maps_to_permission_denied() {
        let error = map_xcap_message(
            "permission denied while reading display".to_owned(),
            XcapErrorContext::Window,
        );
        assert_eq!(error.error_code(), "permission_denied");
    }

    #[test]
    fn capture_system_source_best_effort_metadata_calls() {
        let source = SystemXcapSource;
        let _ = source.monitor_from_point(0, 0);

        if let Ok(monitors) = source.all_monitors()
            && let Some(monitor) = monitors.first()
        {
            let _ = MonitorLike::id(monitor);
            let _ = MonitorLike::name(monitor);
            let _ = MonitorLike::x(monitor);
            let _ = MonitorLike::y(monitor);
            let _ = MonitorLike::width(monitor);
            let _ = MonitorLike::height(monitor);
            let _ = MonitorLike::is_primary(monitor);
            let _ = MonitorLike::is_builtin(monitor);
        }

        if let Ok(windows) = source.all_windows()
            && let Some(window) = windows.first()
        {
            let _ = WindowLike::x(window);
            let _ = WindowLike::y(window);
            let _ = WindowLike::width(window);
            let _ = WindowLike::height(window);
            let _ = WindowLike::is_minimized(window);
            let _ = WindowLike::is_focused(window);
        }

        let _backend = XcapBackend::default();
    }
}
