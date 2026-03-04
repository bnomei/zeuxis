mod support;

use std::sync::atomic::Ordering;

use rmcp::handler::server::wrapper::Parameters;

use support::{create_test_harness, extract_error_code};
use zeuxis::mcp::{
    errors::ServerError,
    tools::{CaptureRectParams, CommonCaptureParams},
};

#[tokio::test]
async fn result_payload_success_includes_structured_fields_and_resource_link() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_screen(Parameters(CommonCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert!(structured.get("path").is_some());
    assert!(structured.get("uri").is_some());
    assert!(structured.get("width").is_some());
    assert!(structured.get("height").is_some());
    assert!(structured.get("capture_mode").is_some());
    assert!(structured.get("captured_at_utc").is_some());

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
        .capture_screen(Parameters(CommonCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "storage_failed");
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 1);
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
        .capture_screen(Parameters(CommonCaptureParams::default()))
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
        .capture_screen(Parameters(CommonCaptureParams::default()))
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
