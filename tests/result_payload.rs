mod support;

use std::sync::atomic::Ordering;

use rmcp::handler::server::wrapper::Parameters;

use support::{create_test_harness, extract_error_code};
use zeuxis::mcp::{
    errors::ServerError,
    tools::{
        CaptureCursorRegionParams, CaptureCursorWindowParams, CaptureRectParams,
        CaptureScreenParams, CommonCaptureParams, GetLatestCaptureParams,
        GetRuntimeDiagnosticsParams, ListMonitorsParams,
    },
};

#[tokio::test]
async fn result_payload_success_includes_structured_fields_and_resource_link() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert!(structured.get("path").is_some());
    assert!(structured.get("uri").is_some());
    assert!(structured.get("output_format").is_some());
    assert!(structured.get("mime_type").is_some());
    assert!(structured.get("artifact_sha256").is_some());
    assert!(structured.get("artifact_hmac_sha256").is_some());
    assert!(structured.get("width").is_some());
    assert!(structured.get("height").is_some());
    assert!(structured.get("capture_mode").is_some());
    assert!(structured.get("artifact_capture_mode").is_some());
    assert!(structured.get("captured_at_utc").is_some());
    assert!(structured.get("applied_settings").is_some());
    assert!(structured.get("source_width").is_some());
    assert!(structured.get("source_height").is_some());
    assert!(structured.get("source_scale_factor").is_some());
    assert!(structured.get("target").is_some());

    assert!(
        result
            .content
            .iter()
            .any(|content| content.as_resource_link().is_some()),
        "expected one resource_link content item"
    );
}

#[tokio::test]
async fn result_payload_storage_failure_returns_structured_error() {
    let harness = create_test_harness();
    *harness.storage.error.lock().expect("lock") =
        Some(ServerError::storage_failed("simulated storage failure"));

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn result_payload_get_latest_without_capture_returns_no_capture_yet() {
    let harness = create_test_harness();

    let result = harness
        .server
        .get_latest_capture(Parameters(GetLatestCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "no_capture_yet");
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
    assert_eq!(harness.storage.latest_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn result_payload_get_latest_storage_failure_returns_structured_error() {
    let harness = create_test_harness();
    *harness.storage.latest_error.lock().expect("lock") = Some(ServerError::storage_failed(
        "simulated latest lookup failure",
    ));

    let result = harness
        .server
        .get_latest_capture(Parameters(GetLatestCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
    assert_eq!(harness.storage.latest_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn result_payload_get_latest_worker_panic_returns_structured_error() {
    let harness = create_test_harness();
    *harness.storage.panic_on_latest.lock().expect("lock") = true;

    let result = harness
        .server
        .get_latest_capture(Parameters(GetLatestCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
    assert_eq!(harness.storage.latest_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn result_payload_backend_failure_returns_structured_error() {
    let harness = create_test_harness();
    *harness.backend.rect_error.lock().expect("lock") =
        Some(ServerError::invalid_region("rectangle out of bounds"));

    let result = harness
        .server
        .capture_rect(Parameters(CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_region");
}

#[tokio::test]
async fn result_payload_permission_denied_returns_structured_error_without_storage_write() {
    let harness = create_test_harness();
    *harness.permission.result.lock().expect("lock") =
        Err(ServerError::permission_denied("screen permission denied"));

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "permission_denied");
    assert_eq!(harness.permission.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn result_payload_platform_unsupported_returns_structured_error_without_storage_write() {
    let harness = create_test_harness();
    *harness.permission.result.lock().expect("lock") = Err(
        ServerError::capture_unsupported_on_platform("capture unsupported in this environment"),
    );

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        extract_error_code(&result),
        "capture_unsupported_on_platform"
    );
    assert_eq!(harness.permission.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn result_payload_list_monitors_failure_returns_structured_error() {
    let harness = create_test_harness();
    *harness.backend.monitors_error.lock().expect("lock") =
        Some(ServerError::capture_unsupported_on_platform(
            "monitor listing unsupported on this platform",
        ));

    let result = harness
        .server
        .list_monitors(Parameters(ListMonitorsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        extract_error_code(&result),
        "capture_unsupported_on_platform"
    );
}

#[tokio::test]
async fn result_payload_capture_cursor_window_propagates_cursor_failure() {
    let harness = create_test_harness();
    *harness.cursor.result.lock().expect("lock") =
        Err(ServerError::cursor_unavailable("cursor query failed"));

    let result = harness
        .server
        .capture_cursor_window(Parameters(CaptureCursorWindowParams {
            common: CommonCaptureParams::default(),
            include_system_windows: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "cursor_unavailable");
    assert_eq!(
        *harness.backend.last_window_cursor.lock().expect("lock"),
        None
    );
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn result_payload_capture_cursor_region_propagates_cursor_failure() {
    let harness = create_test_harness();
    *harness.cursor.result.lock().expect("lock") =
        Err(ServerError::cursor_unavailable("cursor query failed"));

    let result = harness
        .server
        .capture_cursor_region(Parameters(CaptureCursorRegionParams {
            common: CommonCaptureParams::default(),
            size: 42,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "cursor_unavailable");
    assert_eq!(
        *harness.backend.last_cursor_region.lock().expect("lock"),
        None
    );
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn result_payload_runtime_diagnostics_reports_component_failures_without_tool_error() {
    let harness = create_test_harness();
    *harness.permission.result.lock().expect("lock") =
        Err(ServerError::permission_denied("permission denied"));
    *harness.backend.monitors_error.lock().expect("lock") =
        Some(ServerError::monitor_not_found("no monitors"));
    *harness.cursor.result.lock().expect("lock") =
        Err(ServerError::cursor_unavailable("cursor unavailable"));

    let result = harness
        .server
        .get_runtime_diagnostics(Parameters(GetRuntimeDiagnosticsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert!(structured.get("permission_checked").is_some());
    assert!(
        structured
            .get("permission_check_mode")
            .and_then(|v| v.as_str())
            .is_some()
    );
    assert_eq!(
        structured.get("permission_ok").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("permission_error_code")
            .and_then(|v| v.as_str()),
        Some("permission_denied")
    );
    assert_eq!(
        structured.get("monitors_ok").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("monitors_error_code")
            .and_then(|v| v.as_str()),
        Some("monitor_not_found")
    );
    assert_eq!(
        structured.get("cursor_ok").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured.get("cursor_error_code").and_then(|v| v.as_str()),
        Some("cursor_unavailable")
    );
}

#[tokio::test]
async fn result_payload_runtime_diagnostics_reports_monitor_worker_panic() {
    let harness = create_test_harness();
    *harness.backend.monitors_panic.lock().expect("lock") = true;

    let result = harness
        .server
        .get_runtime_diagnostics(Parameters(GetRuntimeDiagnosticsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert_eq!(
        structured.get("monitors_ok").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("monitors_error_code")
            .and_then(|v| v.as_str()),
        Some("storage_failed")
    );
}
