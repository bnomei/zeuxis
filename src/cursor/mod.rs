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
        let state = DeviceState::checked_new()
            .ok_or_else(|| ServerError::cursor_unavailable(cursor_unavailable_message()))?;

        let mouse = state.get_mouse();
        Ok(Point::new(mouse.coords.0, mouse.coords.1))
    }
}

fn cursor_unavailable_message() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "cursor access is unavailable on this Linux session (common on Wayland without portal support); use capture_screen or capture_rect, or run diagnose_runtime for details"
    }

    #[cfg(not(target_os = "linux"))]
    {
        "cursor access is unavailable (accessibility permission may be missing)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_provider_new_constructs_provider() {
        let _provider = DeviceQueryCursorProvider::new();
    }

    #[test]
    fn cursor_provider_cursor_position_returns_point_or_cursor_unavailable() {
        let provider = DeviceQueryCursorProvider::new();
        let _ = provider.cursor_position();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cursor_message_linux_includes_fallback_guidance() {
        let message = cursor_unavailable_message();
        assert!(message.contains("capture_screen"));
        assert!(message.contains("diagnose_runtime"));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn cursor_message_non_linux_mentions_accessibility() {
        let message = cursor_unavailable_message();
        assert!(message.contains("accessibility"));
    }
}
