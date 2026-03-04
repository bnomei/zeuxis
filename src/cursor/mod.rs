use device_query::{DeviceQuery, DeviceState};

use crate::{capture::region::Point, mcp::errors::ServerError};

pub trait CursorProvider: Send + Sync {
    fn cursor_position(&self) -> Result<Point, ServerError>;
}

#[derive(Debug, Clone, Default)]
pub struct DeviceQueryCursorProvider;

impl DeviceQueryCursorProvider {
    pub const fn new() -> Self {
        Self
    }
}

impl CursorProvider for DeviceQueryCursorProvider {
    fn cursor_position(&self) -> Result<Point, ServerError> {
        let state = DeviceState::checked_new().ok_or_else(|| {
            ServerError::cursor_unavailable(
                "cursor access is unavailable (accessibility permission may be missing)",
            )
        })?;

        let mouse = state.get_mouse();
        Ok(Point::new(mouse.coords.0, mouse.coords.1))
    }
}
