mod support;

use std::{sync::atomic::Ordering, time::Duration};

use rmcp::handler::server::wrapper::Parameters;

use support::{
    create_test_harness, create_test_harness_with_parallelism,
    create_test_harness_with_parallelism_and_timeout, extract_capture_mode, extract_error_code,
    extract_monitor_count,
};
use zeuxis::mcp::tools::{
    CaptureCursorRegionParams, CaptureRectParams, CaptureScreenParams, CommonCaptureParams,
    DiagnoseRuntimeParams, GetLatestScreenshotParams, ListMonitorsParams, OutputFormat,
    OutputPreset,
};
use zeuxis::storage::{CaptureOutputFormat, CaptureOutputOptions};

#[tokio::test]
async fn tool_calls_capture_screen_returns_success_payload() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_screen");
    assert_eq!(
        *harness.backend.last_screen_monitor_id.lock().expect("lock"),
        Some(None)
    );
}

#[tokio::test]
async fn tool_calls_capture_screen_forwards_requested_monitor_id() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams::default(),
            monitor_id: Some(200),
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_screen");
    assert_eq!(
        *harness.backend.last_screen_monitor_id.lock().expect("lock"),
        Some(Some(200))
    );
}

#[tokio::test]
async fn tool_calls_list_monitors_returns_structured_monitor_list() {
    let harness = create_test_harness();
    let result = harness
        .server
        .list_monitors(Parameters(ListMonitorsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_monitor_count(&result), 2);
    let structured = result.structured_content.expect("structured content");
    let monitors = structured
        .get("monitors")
        .and_then(|value| value.as_array())
        .expect("monitors array");
    assert_eq!(monitors.len(), 2);
}

#[tokio::test]
async fn tool_calls_diagnose_runtime_returns_structured_diagnostics() {
    let harness = create_test_harness();
    let result = harness
        .server
        .diagnose_runtime(Parameters(DiagnoseRuntimeParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert!(structured.get("os").is_some());
    assert!(structured.get("permission_ok").is_some());
    assert!(structured.get("monitors_ok").is_some());
    assert!(structured.get("cursor_ok").is_some());
}

#[tokio::test]
async fn tool_calls_get_latest_screenshot_returns_last_artifact_without_new_capture() {
    let harness = create_test_harness();
    let first = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("initial capture");

    let latest = harness
        .server
        .get_latest_screenshot(Parameters(GetLatestScreenshotParams::default()))
        .await
        .expect("tool call");

    assert_eq!(latest.is_error, Some(false));
    assert_eq!(extract_capture_mode(&latest), "get_latest_screenshot");
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.storage.latest_calls.load(Ordering::SeqCst), 1);

    let first_uri = first
        .structured_content
        .as_ref()
        .and_then(|value| value.get("uri"))
        .and_then(|value| value.as_str())
        .expect("first uri");
    let latest_uri = latest
        .structured_content
        .as_ref()
        .and_then(|value| value.get("uri"))
        .and_then(|value| value.as_str())
        .expect("latest uri");
    assert_eq!(latest_uri, first_uri);

    let first_captured_at = first
        .structured_content
        .as_ref()
        .and_then(|value| value.get("captured_at_utc"))
        .and_then(|value| value.as_str())
        .expect("first captured_at_utc");
    let latest_captured_at = latest
        .structured_content
        .as_ref()
        .and_then(|value| value.get("captured_at_utc"))
        .and_then(|value| value.as_str())
        .expect("latest captured_at_utc");
    assert_eq!(latest_captured_at, first_captured_at);
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

#[tokio::test]
async fn tool_calls_capture_with_play_sound_emits_feedback_once_on_success() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                play_sound: Some(true),
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(harness.feedback.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tool_calls_capture_with_play_sound_does_not_emit_feedback_on_failure() {
    let harness = create_test_harness();
    *harness.backend.screen_error.lock().expect("lock") = Some(
        zeuxis::mcp::errors::ServerError::monitor_not_found("simulated failure"),
    );

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                play_sound: Some(true),
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(harness.feedback.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tool_calls_capture_screen_applies_compact_output_preset_to_storage_options() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                output_preset: Some(OutputPreset::Compact),
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        *harness.storage.last_output.lock().expect("lock"),
        Some(CaptureOutputOptions {
            format: CaptureOutputFormat::Jpeg,
            jpeg_quality: 82
        })
    );
}

#[tokio::test]
async fn tool_calls_capture_screen_applies_output_overrides_to_storage_options() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                output_preset: Some(OutputPreset::Exact),
                output_format: Some(OutputFormat::Webp),
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        *harness.storage.last_output.lock().expect("lock"),
        Some(CaptureOutputOptions {
            format: CaptureOutputFormat::Webp,
            jpeg_quality: 82
        })
    );
}

#[tokio::test]
async fn tool_calls_concurrent_capture_screen_respects_parallelism_limit() {
    let harness = create_test_harness_with_parallelism(1);
    *harness.backend.screen_capture_delay.lock().expect("lock") = Some(Duration::from_millis(120));

    let server_a = harness.server.clone();
    let server_b = harness.server.clone();

    let task_a = tokio::spawn(async move {
        server_a
            .capture_screen(Parameters(CaptureScreenParams::default()))
            .await
    });
    let task_b = tokio::spawn(async move {
        server_b
            .capture_screen(Parameters(CaptureScreenParams::default()))
            .await
    });

    let (result_a, result_b) = tokio::join!(task_a, task_b);
    let result_a = result_a.expect("join task a").expect("tool call a");
    let result_b = result_b.expect("join task b").expect("tool call b");

    assert_eq!(result_a.is_error, Some(false));
    assert_eq!(result_b.is_error, Some(false));
    assert_eq!(
        harness
            .backend
            .max_active_screen_captures
            .load(Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn tool_calls_list_monitors_handles_worker_panic_as_storage_failed() {
    let harness = create_test_harness();
    *harness.backend.monitors_panic.lock().expect("lock") = true;

    let result = harness
        .server
        .list_monitors(Parameters(ListMonitorsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
}

#[tokio::test]
async fn tool_calls_capture_screen_handles_worker_panic_as_storage_failed() {
    let harness = create_test_harness();
    *harness.backend.screen_panic.lock().expect("lock") = true;

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
}

#[tokio::test]
async fn tool_calls_capture_screen_supports_optional_delay_seconds() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                delay_seconds: Some(0.01),
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_screen");
}

#[tokio::test]
async fn tool_calls_capture_screen_times_out_backend_worker() {
    let harness = create_test_harness_with_parallelism_and_timeout(1, Duration::from_millis(100));
    *harness.backend.screen_capture_delay.lock().expect("lock") = Some(Duration::from_millis(120));

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
}
