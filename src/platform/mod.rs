use crate::mcp::errors::ServerError;

pub trait PermissionGate: Send + Sync {
    fn ensure_capture_allowed(&self) -> Result<(), ServerError>;
}

#[derive(Debug, Clone, Default)]
pub struct PlatformPermissionGate;

impl PlatformPermissionGate {
    pub const fn new() -> Self {
        Self
    }
}

impl PermissionGate for PlatformPermissionGate {
    fn ensure_capture_allowed(&self) -> Result<(), ServerError> {
        #[cfg(target_os = "macos")]
        {
            let api = CoreGraphicsScreenCaptureAccess;
            evaluate_macos_permission(&api)
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(ServerError::capture_unsupported_on_platform(format!(
                "capture is unsupported on platform '{}' in v1; macOS is required",
                std::env::consts::OS
            )))
        }
    }
}

trait MacScreenCaptureAccess {
    fn preflight(&self) -> bool;
    fn request(&self) -> bool;
}

fn evaluate_macos_permission(api: &dyn MacScreenCaptureAccess) -> Result<(), ServerError> {
    if api.preflight() {
        return Ok(());
    }

    let _ = api.request();

    Err(ServerError::permission_denied(
        "screen capture permission is denied. Grant Screen Recording permission to your terminal app in System Settings > Privacy & Security > Screen Recording, then retry the tool call",
    ))
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct CoreGraphicsScreenCaptureAccess;

#[cfg(target_os = "macos")]
impl MacScreenCaptureAccess for CoreGraphicsScreenCaptureAccess {
    fn preflight(&self) -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    fn request(&self) -> bool {
        unsafe { CGRequestScreenCaptureAccess() }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct MockMacPermissionApi {
        preflight_result: bool,
        request_result: bool,
        request_calls: AtomicUsize,
    }

    impl MockMacPermissionApi {
        fn new(preflight_result: bool, request_result: bool) -> Self {
            Self {
                preflight_result,
                request_result,
                request_calls: AtomicUsize::new(0),
            }
        }
    }

    impl MacScreenCaptureAccess for MockMacPermissionApi {
        fn preflight(&self) -> bool {
            self.preflight_result
        }

        fn request(&self) -> bool {
            self.request_calls.fetch_add(1, Ordering::SeqCst);
            self.request_result
        }
    }

    #[test]
    fn platform_permissions_allows_when_preflight_is_true() {
        let api = MockMacPermissionApi::new(true, false);
        let result = evaluate_macos_permission(&api);
        assert!(result.is_ok());
        assert_eq!(api.request_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn platform_permissions_requests_once_and_returns_denied_without_retry() {
        let api = MockMacPermissionApi::new(false, true);
        let result = evaluate_macos_permission(&api);

        let error = result.expect_err("permission must still be denied in same invocation");
        assert_eq!(error.error_code(), "permission_denied");
        assert_eq!(api.request_calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn platform_permissions_non_macos_returns_unsupported() {
        let gate = PlatformPermissionGate::new();
        let error = gate
            .ensure_capture_allowed()
            .expect_err("non-macOS should be unsupported in v1");
        assert_eq!(error.error_code(), "capture_unsupported_on_platform");
    }
}
