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

pub trait CaptureBackend: Send + Sync {
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ServerError>;
    fn capture_screen(&self, monitor_id: Option<u32>) -> Result<RgbaImage, ServerError>;
    fn capture_active_window(&self) -> Result<RgbaImage, ServerError>;
    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError>;
    fn capture_cursor_region(&self, cursor: Point, size: u32) -> Result<RgbaImage, ServerError>;
    fn capture_rect(&self, rect: GlobalRect) -> Result<RgbaImage, ServerError>;
}
