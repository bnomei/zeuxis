use std::sync::Arc;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler,
};

use crate::{
    capture::{backend::CaptureBackend, xcap_backend::XcapBackend},
    cursor::{CursorProvider, DeviceQueryCursorProvider},
    platform::{PermissionGate, PlatformPermissionGate},
    storage::{PngStorage, TempPngStorage},
};

#[derive(Clone)]
pub struct ZeuxisScreenshotServer {
    pub(crate) backend: Arc<dyn CaptureBackend>,
    pub(crate) cursor_provider: Arc<dyn CursorProvider>,
    pub(crate) permission_gate: Arc<dyn PermissionGate>,
    pub(crate) storage: Arc<dyn PngStorage>,
    pub(crate) capture_slots: Arc<tokio::sync::Semaphore>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl ZeuxisScreenshotServer {
    pub fn new() -> Self {
        Self::with_components(
            Arc::new(XcapBackend::new()),
            Arc::new(DeviceQueryCursorProvider::new()),
            Arc::new(PlatformPermissionGate::new()),
            Arc::new(TempPngStorage::new()),
        )
    }

    pub fn with_components(
        backend: Arc<dyn CaptureBackend>,
        cursor_provider: Arc<dyn CursorProvider>,
        permission_gate: Arc<dyn PermissionGate>,
        storage: Arc<dyn PngStorage>,
    ) -> Self {
        Self {
            backend,
            cursor_provider,
            permission_gate,
            storage,
            capture_slots: Arc::new(tokio::sync::Semaphore::new(default_capture_parallelism())),
            tool_router: Self::build_tool_router(),
        }
    }

    pub async fn serve_stdio(self) -> Result<(), rmcp::RmcpError> {
        let service = self.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

impl Default for ZeuxisScreenshotServer {
    fn default() -> Self {
        Self::new()
    }
}

const DEFAULT_MAX_CONCURRENT_CAPTURES: usize = 2;
const MAX_CONCURRENT_CAPTURES_LIMIT: usize = 16;

fn default_capture_parallelism() -> usize {
    std::env::var("ZEUXIS_MAX_CONCURRENT_CAPTURES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=MAX_CONCURRENT_CAPTURES_LIMIT).contains(value))
        .unwrap_or(DEFAULT_MAX_CONCURRENT_CAPTURES)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ZeuxisScreenshotServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("zeuxis", env!("CARGO_PKG_VERSION"))
                    .with_title("Zeuxis Screenshot Server")
                    .with_description("Read-only local MCP screenshot server"),
            )
            .with_instructions(
                "Provides local screenshot capture tools only. No remote upload, OCR, or automation.",
            )
    }
}
