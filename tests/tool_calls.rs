mod support;

use std::{sync::atomic::Ordering, time::Duration};

use rmcp::handler::server::wrapper::Parameters;

use support::{
    create_test_harness, create_test_harness_with_parallelism,
    create_test_harness_with_parallelism_and_timeout, extract_capture_mode, extract_error_code,
    extract_monitor_count,
};
use zeuxis::mcp::tools::{
    CaptureCursorRegionParams, CaptureCursorWindowParams, CaptureMonitorRegionParams,
    CaptureRectParams, CaptureScreenParams, CaptureWindowParams, ClearSessionArtifactsParams,
    CommonCaptureParams, GetLatestCaptureParams, GetRuntimeDiagnosticsParams, ListMonitorsParams,
    ListSessionArtifactsParams, ListWindowsParams, OutputFormat, OutputInput, OutputMode,
    OutputParams, OutputPreset,
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
    let structured = result.structured_content.expect("structured");
    assert_eq!(structured["target"]["monitor_id"], 100);
    assert!(structured["source_scale_factor"].is_object());
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
    let structured = result.structured_content.expect("structured");
    assert_eq!(structured["target"]["monitor_id"], 200);
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
async fn tool_calls_list_monitors_returns_permission_error_when_denied() {
    let harness = create_test_harness();
    *harness.permission.result.lock().expect("lock") = Err(
        zeuxis::mcp::errors::ServerError::permission_denied("screen permission denied"),
    );

    let result = harness
        .server
        .list_monitors(Parameters(ListMonitorsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "permission_denied");
}

#[tokio::test]
async fn tool_calls_list_windows_returns_structured_window_list() {
    let harness = create_test_harness();
    let result = harness
        .server
        .list_windows(Parameters(ListWindowsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert_eq!(structured["window_count"], 2);
    assert_eq!(structured["id_scope"], "snapshot");
    assert!(structured["snapshot_id"].is_string());
    assert!(structured["listed_at_utc"].is_string());
    let windows = structured["windows"].as_array().expect("windows array");
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0]["id"], 300);
}

#[tokio::test]
async fn tool_calls_list_windows_returns_permission_error_when_denied() {
    let harness = create_test_harness();
    *harness.permission.result.lock().expect("lock") = Err(
        zeuxis::mcp::errors::ServerError::permission_denied("screen permission denied"),
    );

    let result = harness
        .server
        .list_windows(Parameters(ListWindowsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "permission_denied");
}

#[tokio::test]
async fn tool_calls_list_windows_filters_out_system_windows_and_supports_focused_only() {
    let harness = create_test_harness();
    harness
        .backend
        .windows
        .lock()
        .expect("lock")
        .push(zeuxis::capture::backend::WindowInfo {
            id: 500,
            title: "Notification".to_owned(),
            app: "Notification Center".to_owned(),
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            is_focused: false,
            is_minimized: false,
        });

    let focused = harness
        .server
        .list_windows(Parameters(ListWindowsParams {
            focused_only: Some(true),
            include_system_windows: Some(false),
            app_contains: None,
            title_contains: None,
        }))
        .await
        .expect("tool call");

    let focused_structured = focused.structured_content.expect("structured");
    assert_eq!(focused_structured["window_count"], 1);
    assert_eq!(focused_structured["windows"][0]["id"], 300);
}

#[tokio::test]
async fn tool_calls_list_windows_filters_control_centre_and_menu_bar_variants() {
    let harness = create_test_harness();
    harness.backend.windows.lock().expect("lock").extend([
        zeuxis::capture::backend::WindowInfo {
            id: 501,
            title: "Tiny Surface".to_owned(),
            app: "Control Centre".to_owned(),
            x: 0,
            y: 0,
            width: 56,
            height: 80,
            is_focused: false,
            is_minimized: false,
        },
        zeuxis::capture::backend::WindowInfo {
            id: 502,
            title: "menu-bar item".to_owned(),
            app: "ControlCentre".to_owned(),
            x: 0,
            y: 0,
            width: 64,
            height: 36,
            is_focused: false,
            is_minimized: false,
        },
        zeuxis::capture::backend::WindowInfo {
            id: 503,
            title: "menu-bar item".to_owned(),
            app: "ControlCenter".to_owned(),
            x: 0,
            y: 0,
            width: 64,
            height: 36,
            is_focused: false,
            is_minimized: false,
        },
    ]);

    let filtered = harness
        .server
        .list_windows(Parameters(ListWindowsParams {
            focused_only: None,
            include_system_windows: Some(false),
            app_contains: None,
            title_contains: None,
        }))
        .await
        .expect("tool call");

    let structured = filtered.structured_content.expect("structured");
    assert_eq!(structured["window_count"], 2);
    let windows = structured["windows"].as_array().expect("windows array");
    assert!(
        windows.iter().all(|window| {
            let id = window["id"].as_u64().unwrap_or_default();
            id == 300 || id == 400
        }),
        "expected only baseline non-system windows: {windows:?}"
    );
}

#[tokio::test]
async fn tool_calls_list_windows_filters_by_app_and_title_substrings() {
    let harness = create_test_harness();

    let by_app = harness
        .server
        .list_windows(Parameters(ListWindowsParams {
            focused_only: None,
            include_system_windows: Some(true),
            app_contains: Some("saf".to_owned()),
            title_contains: None,
        }))
        .await
        .expect("tool call");
    let by_app_structured = by_app.structured_content.expect("structured");
    assert_eq!(by_app_structured["window_count"], 1);
    assert_eq!(by_app_structured["windows"][0]["id"], 400);

    let by_title = harness
        .server
        .list_windows(Parameters(ListWindowsParams {
            focused_only: None,
            include_system_windows: Some(true),
            app_contains: None,
            title_contains: Some("dit".to_owned()),
        }))
        .await
        .expect("tool call");
    let by_title_structured = by_title.structured_content.expect("structured");
    assert_eq!(by_title_structured["window_count"], 1);
    assert_eq!(by_title_structured["windows"][0]["id"], 300);
}

#[tokio::test]
async fn tool_calls_get_runtime_diagnostics_returns_structured_diagnostics() {
    let harness = create_test_harness();
    let result = harness
        .server
        .get_runtime_diagnostics(Parameters(GetRuntimeDiagnosticsParams::default()))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.expect("structured content");
    assert!(structured.get("os").is_some());
    assert!(structured.get("permission_checked").is_some());
    assert!(structured.get("permission_check_mode").is_some());
    assert!(structured.get("permission_ok").is_some());
    assert!(structured.get("monitors_ok").is_some());
    assert!(structured.get("cursor_ok").is_some());
}

#[tokio::test]
async fn tool_calls_get_latest_capture_returns_last_artifact_without_new_capture() {
    let harness = create_test_harness();
    let first = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("initial capture");

    let latest = harness
        .server
        .get_latest_capture(Parameters(GetLatestCaptureParams::default()))
        .await
        .expect("tool call");

    assert_eq!(latest.is_error, Some(false));
    assert_eq!(extract_capture_mode(&latest), "get_latest_capture");
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

    let artifact_capture_mode = latest
        .structured_content
        .as_ref()
        .and_then(|value| value.get("artifact_capture_mode"))
        .and_then(|value| value.as_str())
        .expect("artifact capture mode");
    assert_eq!(artifact_capture_mode, "capture_screen");
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
    let structured = result.structured_content.expect("structured");
    assert!(structured["source_scale_factor"].is_object());
}

#[tokio::test]
async fn tool_calls_capture_cursor_window_uses_cursor_provider() {
    let harness = create_test_harness();
    let result = harness
        .server
        .capture_cursor_window(Parameters(CaptureCursorWindowParams {
            common: CommonCaptureParams::default(),
            include_system_windows: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_cursor_window");
    assert_eq!(
        *harness.backend.last_window_id.lock().expect("lock"),
        Some(300)
    );
    let structured = result.structured_content.expect("structured");
    assert!(structured["source_scale_factor"].is_object());
}

#[tokio::test]
async fn tool_calls_capture_cursor_window_excludes_system_windows_by_default() {
    let harness = create_test_harness();
    harness.backend.windows.lock().expect("lock").insert(
        0,
        zeuxis::capture::backend::WindowInfo {
            id: 999,
            title: "menu-bar cursor".to_owned(),
            app: "Control Centre".to_owned(),
            x: 40,
            y: 50,
            width: 56,
            height: 80,
            is_focused: false,
            is_minimized: false,
        },
    );

    let result = harness
        .server
        .capture_cursor_window(Parameters(CaptureCursorWindowParams {
            common: CommonCaptureParams::default(),
            include_system_windows: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        *harness.backend.last_window_id.lock().expect("lock"),
        Some(300)
    );
}

#[tokio::test]
async fn tool_calls_capture_cursor_window_can_include_system_windows() {
    let harness = create_test_harness();
    harness.backend.windows.lock().expect("lock").insert(
        0,
        zeuxis::capture::backend::WindowInfo {
            id: 999,
            title: "menu-bar cursor".to_owned(),
            app: "Control Centre".to_owned(),
            x: 40,
            y: 50,
            width: 56,
            height: 80,
            is_focused: false,
            is_minimized: false,
        },
    );

    let result = harness
        .server
        .capture_cursor_window(Parameters(CaptureCursorWindowParams {
            common: CommonCaptureParams::default(),
            include_system_windows: Some(true),
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        *harness.backend.last_window_id.lock().expect("lock"),
        Some(999)
    );
}

#[tokio::test]
async fn tool_calls_capture_window_uses_explicit_window_id() {
    let harness = create_test_harness();
    let listed = harness
        .server
        .list_windows(Parameters(ListWindowsParams::default()))
        .await
        .expect("list windows");
    let snapshot_id = listed
        .structured_content
        .as_ref()
        .and_then(|value| value.get("snapshot_id"))
        .and_then(|value| value.as_str())
        .expect("snapshot id")
        .to_owned();
    let result = harness
        .server
        .capture_window(Parameters(CaptureWindowParams {
            common: CommonCaptureParams::default(),
            snapshot_id,
            window_id: 300,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_window");
    assert_eq!(
        *harness.backend.last_window_id.lock().expect("lock"),
        Some(300)
    );
    let structured = result.structured_content.expect("structured");
    assert_eq!(structured["target"]["window_id"], 300);
    assert!(structured["source_scale_factor"].is_object());
}

#[tokio::test]
async fn tool_calls_capture_window_rejects_stale_snapshot_id() {
    let harness = create_test_harness();
    let _ = harness
        .server
        .list_windows(Parameters(ListWindowsParams::default()))
        .await
        .expect("list windows");

    let result = harness
        .server
        .capture_window(Parameters(CaptureWindowParams {
            common: CommonCaptureParams::default(),
            snapshot_id: "stale-snapshot".to_owned(),
            window_id: 300,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(true));
    assert_eq!(extract_error_code(&result), "invalid_params");
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
async fn tool_calls_capture_monitor_region_passes_monitor_local_rect() {
    let harness = create_test_harness();
    let params = CaptureMonitorRegionParams {
        common: CommonCaptureParams::default(),
        monitor_id: 200,
        x: 8,
        y: 9,
        width: 300,
        height: 200,
    };

    let result = harness
        .server
        .capture_monitor_region(Parameters(params))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_capture_mode(&result), "capture_monitor_region");
    assert_eq!(
        *harness.backend.last_monitor_region.lock().expect("lock"),
        Some((200, 8, 9, 300, 200))
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
    assert_eq!(harness.feedback.capture_calls.load(Ordering::SeqCst), 1);
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
    assert_eq!(harness.feedback.capture_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tool_calls_capture_with_delay_and_play_sound_emits_single_capture_feedback() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                delay_ms: Some(1_100),
                play_sound: Some(true),
                ..CommonCaptureParams::default()
            },
            monitor_id: None,
        }))
        .await
        .expect("tool call");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(harness.feedback.capture_calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.feedback.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tool_calls_capture_screen_applies_compact_output_preset_to_storage_options() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                output: Some(OutputInput::Detailed(OutputParams {
                    mode: OutputMode::Preset,
                    preset: Some(OutputPreset::Compact),
                    format: None,
                    jpeg_quality: None,
                    max_dimension: None,
                })),
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
            jpeg_quality: 85
        })
    );
}

#[tokio::test]
async fn tool_calls_capture_screen_applies_custom_output_to_storage_options() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                output: Some(OutputInput::Detailed(OutputParams {
                    mode: OutputMode::Custom,
                    preset: None,
                    format: Some(OutputFormat::Webp),
                    jpeg_quality: None,
                    max_dimension: Some(1200),
                })),
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

    let structured = result.structured_content.expect("structured");
    assert_eq!(structured["output_format"], "webp");
    assert_eq!(structured["applied_settings"]["output_mode"], "custom");
    assert_eq!(
        structured["applied_settings"]["output_preset"],
        serde_json::Value::Null
    );
    assert_eq!(structured["applied_settings"]["max_dimension"], 1200);
}

#[tokio::test]
async fn tool_calls_list_session_artifacts_returns_latest_marker() {
    let harness = create_test_harness();
    let first = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("first capture");
    let second = harness
        .server
        .capture_rect(Parameters(CaptureRectParams {
            common: CommonCaptureParams::default(),
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        }))
        .await
        .expect("second capture");
    assert_eq!(first.is_error, Some(false));
    assert_eq!(second.is_error, Some(false));

    let listed = harness
        .server
        .list_session_artifacts(Parameters(ListSessionArtifactsParams::default()))
        .await
        .expect("list artifacts");

    assert_eq!(listed.is_error, Some(false));
    let structured = listed.structured_content.expect("structured");
    assert_eq!(structured["artifact_count"], 2);
    let artifacts = structured["artifacts"].as_array().expect("artifact array");
    assert_eq!(artifacts.len(), 2);
    assert!(
        artifacts
            .iter()
            .any(|artifact| artifact["is_latest"] == true)
    );
}

#[tokio::test]
async fn tool_calls_clear_session_artifacts_clears_latest_capture() {
    let harness = create_test_harness();

    let captured = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("capture");
    assert_eq!(captured.is_error, Some(false));

    let cleared = harness
        .server
        .clear_session_artifacts(Parameters(ClearSessionArtifactsParams::default()))
        .await
        .expect("clear");
    assert_eq!(cleared.is_error, Some(false));
    assert_eq!(
        cleared.structured_content.expect("structured")["deleted_artifact_count"],
        1
    );

    let latest = harness
        .server
        .get_latest_capture(Parameters(GetLatestCaptureParams::default()))
        .await
        .expect("latest");
    assert_eq!(latest.is_error, Some(true));
    assert_eq!(extract_error_code(&latest), "no_capture_yet");
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
async fn tool_calls_capture_screen_supports_optional_delay_ms() {
    let harness = create_test_harness();

    let result = harness
        .server
        .capture_screen(Parameters(CaptureScreenParams {
            common: CommonCaptureParams {
                delay_ms: Some(10),
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
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(harness.storage.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tool_calls_capture_screen_times_out_while_waiting_for_capture_slot() {
    let harness = create_test_harness_with_parallelism_and_timeout(1, Duration::from_millis(80));
    *harness.backend.screen_capture_delay.lock().expect("lock") = Some(Duration::from_millis(250));

    let server_a = harness.server.clone();
    let server_b = harness.server.clone();

    let task_a = tokio::spawn(async move {
        server_a
            .capture_screen(Parameters(CaptureScreenParams::default()))
            .await
    });

    for _ in 0..20 {
        if harness
            .backend
            .active_screen_captures
            .load(Ordering::SeqCst)
            > 0
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        harness
            .backend
            .active_screen_captures
            .load(Ordering::SeqCst)
            > 0,
        "first capture should acquire the only slot"
    );

    let result_b = server_b
        .capture_screen(Parameters(CaptureScreenParams::default()))
        .await
        .expect("tool call b");

    assert_eq!(result_b.is_error, Some(true));
    assert_eq!(extract_error_code(&result_b), "storage_failed");
    let error_message = result_b
        .structured_content
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert!(
        error_message.contains("capture slot acquisition timed out"),
        "unexpected error message: {error_message}"
    );

    let _ = task_a.await.expect("join task a").expect("tool call a");
}
