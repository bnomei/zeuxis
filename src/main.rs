use std::error::Error;

use tracing_subscriber::{EnvFilter, fmt};
use zeuxis::mcp::ZeuxisScreenshotServer;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let server = ZeuxisScreenshotServer::new();
    server.serve_stdio().await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
