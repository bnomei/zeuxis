mod support;

use rmcp::handler::server::wrapper::Parameters;

use support::{create_test_harness, extract_error_code};
use zeuxis::mcp::tools::{
    CaptureCursorRegionParams, CaptureRectParams, CaptureScreenParams, CommonCaptureParams,
};

#[tokio::test]
async fn mcp_tools_validation_rejects_negative_delay() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                delay_seconds: Some(-1.0),
                play_sound: None,
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_params");
}

#[tokio::test]
async fn mcp_tools_validation_rejects_delay_above_policy_limit() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                delay_seconds: Some(31.0),
                play_sound: None,
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_params");
}

#[tokio::test]
async fn mcp_tools_validation_rejects_non_positive_cursor_size() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_cursor_region(Parameters(CaptureCursorRegionParams {
            common: CommonCaptureParams::default(),
            size: 0,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_region");
}

#[tokio::test]
async fn mcp_tools_validation_rejects_non_positive_rect_width() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_rect(Parameters(CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 0,
            y: 0,
            width: 0,
            height: 20,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_region");
}

#[tokio::test]
async fn mcp_tools_validation_rejects_rect_above_dimension_limit() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_rect(Parameters(CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 0,
            y: 0,
            width: 16_385,
            height: 20,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_region");
}

#[tokio::test]
async fn mcp_tools_validation_rejects_cursor_region_above_dimension_limit() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_cursor_region(Parameters(CaptureCursorRegionParams {
            common: CommonCaptureParams::default(),
            size: 16_385,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_region");
}
