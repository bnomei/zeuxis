#[cfg(target_os = "linux")]
mod tests {
    use zeuxis::capture::{backend::CaptureBackend, xcap_backend::XcapBackend};

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
        let monitors = backend
            .list_monitors()
            .expect("list_monitors should succeed in linux smoke environment");
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
}
