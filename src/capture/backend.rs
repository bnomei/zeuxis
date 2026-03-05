use image::RgbaImage;
use serde::Serialize;

use crate::{
    capture::region::{GlobalRect, Point},
    mcp::errors::ServerError,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_focused: bool,
    pub is_minimized: bool,
}

pub trait CaptureBackend: Send + Sync {
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ServerError>;
    fn list_windows(&self) -> Result<Vec<WindowInfo>, ServerError> {
        Err(ServerError::capture_unsupported_on_platform(
            "listing windows is not supported by this capture backend",
        ))
    }
    fn capture_screen(&self, monitor_id: Option<u32>) -> Result<RgbaImage, ServerError>;
    fn capture_window(&self, _window_id: u32) -> Result<RgbaImage, ServerError> {
        Err(ServerError::capture_unsupported_on_platform(
            "capturing a window by id is not supported by this capture backend",
        ))
    }
    fn capture_monitor_region(
        &self,
        _monitor_id: u32,
        _x: u32,
        _y: u32,
        _width: u32,
        _height: u32,
    ) -> Result<RgbaImage, ServerError> {
        Err(ServerError::capture_unsupported_on_platform(
            "capturing a monitor region by monitor id is not supported by this capture backend",
        ))
    }
    fn capture_active_window(&self) -> Result<RgbaImage, ServerError>;
    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError>;
    fn capture_cursor_region(&self, cursor: Point, size: u32) -> Result<RgbaImage, ServerError>;
    fn capture_rect(&self, rect: GlobalRect) -> Result<RgbaImage, ServerError>;
}
