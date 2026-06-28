#[cfg(target_os = "linux")]
mod tests {
    use zeuxis::{
        capture::{backend::CaptureBackend, xcap_backend::XcapBackend},
        mcp::errors::ServerError,
    };

    #[test]
    fn linux_smoke_list_monitors_and_capture_screen() {
        if std::env::var_os("ZEUXIS_LINUX_SMOKE").is_none() {
            eprintln!("skipping linux smoke test; set ZEUXIS_LINUX_SMOKE=1 to run");
            return;
        }

        assert!(
            std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
            "linux smoke test requires DISPLAY or WAYLAND_DISPLAY"
        );

        let backend = XcapBackend::new();
        let monitors = match backend.list_monitors() {
            Ok(monitors) => monitors,
            Err(error) if is_xvfb_edid_unsupported(&error) => {
                eprintln!(
                    "skipping linux smoke test; Xvfb display does not expose EDID metadata: {}",
                    error.message()
                );
                return;
            }
            Err(error) => {
                panic!("list_monitors should succeed in linux smoke environment: {error:?}")
            }
        };
        assert!(
            !monitors.is_empty(),
            "expected at least one monitor in linux smoke environment"
        );

        let first_monitor_id = monitors[0].id;
        let image = backend
            .capture_screen(Some(first_monitor_id))
            .expect("capture_screen should succeed in linux smoke environment");
        assert!(image.width() > 0);
        assert!(image.height() > 0);
    }

    fn is_xvfb_edid_unsupported(error: &ServerError) -> bool {
        error.error_code() == "monitor_not_found"
            && error.message() == "capture backend failed: EDID not supported"
    }
}
