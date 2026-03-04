mod support;

use rmcp::handler::server::wrapper::Parameters;

use support::{create_test_harness, extract_capture_mode};
use zeuxis::mcp::tools::{CaptureCursorRegionParams, CaptureRectParams, CommonCaptureParams};

#[tokio::test]
async fn tool_calls_capture_screen_returns_success_payload() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CommonCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_screen");
}

#[tokio::test]
async fn tool_calls_capture_active_window_returns_success_payload() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_active_window(Parameters(CommonCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_active_window");
}

#[tokio::test]
async fn tool_calls_capture_window_at_cursor_uses_cursor_provider() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_window_at_cursor(Parameters(CommonCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_window_at_cursor");
    assert_eq!(
        *harness.backend.last_window_cursor.lock().expect("lock"),
        Some(zeuxis::capture::region::Point::new(50, 60))
    );
}

#[tokio::test]
async fn tool_calls_capture_cursor_region_uses_cursor_and_size() {
    let harness = create_test_harness();
    let params = CaptureCursorRegionParams {
        common: CommonCaptureParams::default(),
        size: 42,
    };

    let result = harness
        .server
        .capture_cursor_region(Parameters(params))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_cursor_region");
    assert_eq!(
        *harness.backend.last_cursor_region.lock().expect("lock"),
        Some((zeuxis::capture::region::Point::new(50, 60), 42))
    );
}

#[tokio::test]
async fn tool_calls_capture_rect_passes_exact_rect() {
    let harness = create_test_harness();
    let params = CaptureRectParams {
        common: CommonCaptureParams::default(),
        x: 10,
        y: 20,
        width: 300,
        height: 200,
    };

    let result = harness
        .server
        .capture_rect(Parameters(params))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_rect");
    assert_eq!(
        *harness.backend.last_rect.lock().expect("lock"),
        Some(zeuxis::capture::region::GlobalRect {
            x: 10,
            y: 20,
            width: 300,
            height: 200
        })
    );
}
