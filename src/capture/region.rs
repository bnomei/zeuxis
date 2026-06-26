//! Coordinate types and overflow-safe region math for desktop captures.
//!
//! Zeuxis accepts logical desktop points from clients, translates them to
//! monitor-local rectangles for capture backends, and treats rectangles as
//! half-open bounds: left/top inclusive, right/bottom exclusive.

use crate::mcp::errors::ServerError;

/// Point in global logical desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    /// Constructs a global desktop point from logical x/y coordinates.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Rectangle in global logical desktop coordinates.
///
/// `x` and `y` may be negative on multi-monitor desktops whose origin is not the
/// top-left of every display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Rectangle in monitor-local logical coordinates.
///
/// Local coordinates are unsigned because they have already been validated
/// against a specific monitor's origin and bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Monitor bounds in the global logical desktop coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Builds a square global rectangle centered on the cursor point.
///
/// Returns `invalid_region` for zero size or coordinate overflow.
pub fn center_square_on_cursor(cursor: Point, size: u32) -> Result<GlobalRect, ServerError> {
    if size == 0 {
        return Err(ServerError::invalid_region("size must be greater than 0"));
    }

    let half = i64::from(size / 2);
    let x = i64::from(cursor.x) - half;
    let y = i64::from(cursor.y) - half;

    let x =
        i32::try_from(x).map_err(|_| ServerError::invalid_region("computed x is out of range"))?;
    let y =
        i32::try_from(y).map_err(|_| ServerError::invalid_region("computed y is out of range"))?;

    Ok(GlobalRect {
        x,
        y,
        width: size,
        height: size,
    })
}

/// Converts a global rectangle into monitor-local coordinates.
///
/// The full rectangle must fit inside the supplied monitor bounds; crossing a
/// monitor edge is rejected rather than clipped.
pub fn global_to_local_rect(
    global: GlobalRect,
    monitor: MonitorBounds,
) -> Result<LocalRect, ServerError> {
    if global.width == 0 || global.height == 0 {
        return Err(ServerError::invalid_region(
            "width and height must be greater than 0",
        ));
    }

    let global_left = i64::from(global.x);
    let global_top = i64::from(global.y);
    let global_right = global_left
        .checked_add(i64::from(global.width))
        .ok_or_else(|| ServerError::invalid_region("rectangle width overflows coordinate range"))?;
    let global_bottom = global_top
        .checked_add(i64::from(global.height))
        .ok_or_else(|| {
            ServerError::invalid_region("rectangle height overflows coordinate range")
        })?;

    let monitor_left = i64::from(monitor.x);
    let monitor_top = i64::from(monitor.y);
    let monitor_right = monitor_left
        .checked_add(i64::from(monitor.width))
        .ok_or_else(|| ServerError::invalid_region("monitor width overflows coordinate range"))?;
    let monitor_bottom = monitor_top
        .checked_add(i64::from(monitor.height))
        .ok_or_else(|| ServerError::invalid_region("monitor height overflows coordinate range"))?;

    if global_left < monitor_left
        || global_top < monitor_top
        || global_right > monitor_right
        || global_bottom > monitor_bottom
    {
        return Err(ServerError::invalid_region(
            "requested rectangle is outside monitor bounds",
        ));
    }

    let local_x = u32::try_from(global_left - monitor_left)
        .map_err(|_| ServerError::invalid_region("local x out of bounds"))?;
    let local_y = u32::try_from(global_top - monitor_top)
        .map_err(|_| ServerError::invalid_region("local y out of bounds"))?;

    Ok(LocalRect {
        x: local_x,
        y: local_y,
        width: global.width,
        height: global.height,
    })
}

/// Tests whether a point lies inside a half-open rectangle.
pub fn rect_contains_point(x: i32, y: i32, width: u32, height: u32, point: Point) -> bool {
    if width == 0 || height == 0 {
        return false;
    }

    let left = i64::from(x);
    let top = i64::from(y);
    let right = left + i64::from(width);
    let bottom = top + i64::from(height);

    let px = i64::from(point.x);
    let py = i64::from(point.y);
    px >= left && px < right && py >= top && py < bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_mapping_translates_global_to_local() {
        let global = GlobalRect {
            x: 110,
            y: 210,
            width: 50,
            height: 30,
        };
        let monitor = MonitorBounds {
            x: 100,
            y: 200,
            width: 400,
            height: 300,
        };

        let local = global_to_local_rect(global, monitor).expect("local rect");
        assert_eq!(
            local,
            LocalRect {
                x: 10,
                y: 10,
                width: 50,
                height: 30
            }
        );
    }

    #[test]
    fn coordinate_mapping_rejects_out_of_bounds_regions() {
        let global = GlobalRect {
            x: 490,
            y: 210,
            width: 20,
            height: 20,
        };
        let monitor = MonitorBounds {
            x: 100,
            y: 200,
            width: 400,
            height: 300,
        };

        let err = global_to_local_rect(global, monitor).expect_err("must fail");
        assert_eq!(err.error_code(), "invalid_region");
    }

    #[test]
    fn coordinate_center_square_on_cursor_works() {
        let rect = center_square_on_cursor(Point::new(100, 200), 40).expect("square");
        assert_eq!(rect.x, 80);
        assert_eq!(rect.y, 180);
        assert_eq!(rect.width, 40);
        assert_eq!(rect.height, 40);
    }

    #[test]
    fn coordinate_rect_contains_point_checks_half_open_bounds() {
        assert!(rect_contains_point(10, 10, 20, 20, Point::new(10, 10)));
        assert!(rect_contains_point(10, 10, 20, 20, Point::new(29, 29)));
        assert!(!rect_contains_point(10, 10, 20, 20, Point::new(30, 29)));
        assert!(!rect_contains_point(10, 10, 20, 20, Point::new(29, 30)));
    }

    #[test]
    fn coordinate_center_square_rejects_zero_size() {
        let error = center_square_on_cursor(Point::new(10, 10), 0).expect_err("size 0 fails");
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn coordinate_mapping_rejects_zero_dimensions() {
        let error = global_to_local_rect(
            GlobalRect {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
            MonitorBounds {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
        )
        .expect_err("zero width fails");
        assert_eq!(error.error_code(), "invalid_region");
    }

    #[test]
    fn coordinate_rect_contains_point_rejects_zero_sized_rectangles() {
        assert!(!rect_contains_point(0, 0, 0, 1, Point::new(0, 0)));
        assert!(!rect_contains_point(0, 0, 1, 0, Point::new(0, 0)));
    }
}
