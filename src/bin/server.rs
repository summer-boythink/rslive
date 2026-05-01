//! rslive-server - High-performance streaming server
//!
//! Usage:
//!   rslive-server [--rtmp-port 1935] [--http-port 8080]

use tracing::info;

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("🚀 rslive-server starting...");
    info!("RTMP server would listen on rtmp://0.0.0.0:1935");
    info!("HTTP-FLV server would listen on http://0.0.0.0:8080");
    info!("HLS server would listen on http://0.0.0.0:8081");

    // Note: The full async server implementation requires additional work
    // to integrate with the existing blocking RTMP server code.
    // This is a placeholder for the binary entry point.

    info!("Server initialization complete. Full implementation requires async refactoring.");

    Ok(())
}
