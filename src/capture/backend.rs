use image::RgbaImage;

use crate::{
    capture::region::{GlobalRect, Point},
    mcp::errors::ServerError,
};

pub trait CaptureBackend: Send + Sync {
    fn capture_screen(&self) -> Result<RgbaImage, ServerError>;
    fn capture_active_window(&self) -> Result<RgbaImage, ServerError>;
    fn capture_window_at_cursor(&self, cursor: Point) -> Result<RgbaImage, ServerError>;
    fn capture_cursor_region(&self, cursor: Point, size: u32) -> Result<RgbaImage, ServerError>;
    fn capture_rect(&self, rect: GlobalRect) -> Result<RgbaImage, ServerError>;
}
