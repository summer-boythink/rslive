//! rslive-server - High-performance streaming server
//!
//! This server integrates multiple streaming protocols:
//! - RTMP (Real-Time Messaging Protocol) for ingest
//! - HLS (HTTP Live Streaming) for delivery
//! - HTTP-FLV for low-latency delivery
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     rslive-server                               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                  │
//! │  ┌─────────────┐    ┌─────────────────────────────────────┐   │
//! │  │ RTMP Server │    │         Tokio Runtime                │   │
//! │  │ :1935       │    │  ┌─────────────┐  ┌─────────────┐   │   │
//! │  │ (thread)    │    │  │ HLS Server  │  │HTTP-FLV Srv │   │   │
//! │  └──────┬──────┘    │  │ :8080       │  │ :8081       │   │   │
//! │         │           │  └──────┬──────┘  └──────┬──────┘   │   │
//! │         │           │         │                │          │   │
//! │         └───────────┼─────────┼────────────────┘          │   │
//! │                     │         │                            │   │
//! │            ┌────────▼─────────▼────────┐                   │   │
//! │            │      StreamRouter         │                   │   │
//! │            │   (Central Coordinator)    │                   │   │
//! │            └─────────────┬─────────────┘                   │   │
//! │                          │                                    │
//! │            ┌─────────────▼─────────────┐                   │   │
//! │            │  HlsPackagerManager       │                   │   │
//! │            │  (HLS Segment Generation) │                   │   │
//! │            └───────────────────────────┘                   │   │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```bash
//! # Start with default settings
//! rslive-server
//!
//! # Custom ports
//! rslive-server --rtmp-port 1935 --hls-port 8080 --flv-port 8081
//!
//! # Enable low-latency HLS
//! rslive-server --low-latency
//! ```

use anyhow::{Context, Result};
use rslive::hls::{HlsPackagerManager, HlsServer, PackagerConfig, ServerConfig as HlsServerConfig};
use rslive::media::{RouterConfig, StreamRouter};
use rslive::protocol::hls::segment::MemorySegmentStorage;
use rslive::rtmp::RtmpServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::oneshot;
use tracing::{error, info, warn};

/// Server configuration
#[derive(Debug, Clone)]
struct ServerConfig {
    /// RTMP server bind address
    rtmp_bind: SocketAddr,
    /// HLS server bind address
    hls_bind: SocketAddr,
    /// HTTP-FLV server bind address
    flv_bind: SocketAddr,
    /// Enable low-latency HLS mode
    low_latency: bool,
    /// Maximum number of streams
    max_streams: usize,
    /// Maximum segments per stream
    max_segments: usize,
    /// Log level
    log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            rtmp_bind: "0.0.0.0:1935".parse().unwrap(),
            hls_bind: "0.0.0.0:8080".parse().unwrap(),
            flv_bind: "0.0.0.0:8081".parse().unwrap(),
            low_latency: false,
            max_streams: 1000,
            max_segments: 100,
            log_level: "info".to_string(),
        }
    }
}

/// Server runtime state
struct ServerRuntime {
    config: ServerConfig,
    router: Arc<StreamRouter>,
    packager_manager: Arc<HlsPackagerManager>,
    rtmp_shutdown: Option<oneshot::Sender<()>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let config = parse_args()?;

    // Initialize logging
    init_logging(&config.log_level)?;

    // Print banner
    print_banner(&config);

    // Create server runtime
    let runtime = ServerRuntime::new(config).await?;

    // Start all services
    runtime.run().await?;

    Ok(())
}

impl ServerRuntime {
    /// Create a new server runtime
    async fn new(config: ServerConfig) -> Result<Self> {
        // Create central stream router
        let router_config = RouterConfig::default();
        let router = Arc::new(StreamRouter::new(router_config));

        // Create HLS packager manager
        let packager_config = if config.low_latency {
            PackagerConfig::for_low_latency()
        } else {
            PackagerConfig::default()
        };
        let segment_storage = Arc::new(MemorySegmentStorage::new(config.max_segments));
        let packager_manager = Arc::new(HlsPackagerManager::new(packager_config, segment_storage));

        Ok(Self {
            config,
            router,
            packager_manager,
            rtmp_shutdown: None,
        })
    }

    /// Run all server components
    async fn run(mut self) -> Result<()> {
        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        self.rtmp_shutdown = Some(shutdown_tx);

        // Start HLS auto-subscription to StreamRouter
        // This must be done before RTMP server starts to catch all new streams
        self.packager_manager.start_auto_subscribe(Arc::clone(&self.router));
        info!("HLS packager auto-subscription started");

        // Start RTMP server in a separate thread (blocking)
        self.start_rtmp_server();

        // Start HLS server (async)
        let hls_handle = self.start_hls_server().await?;

        // Wait for shutdown signal
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received shutdown signal (Ctrl+C)");
            }
            _ = &mut shutdown_rx => {
                info!("Received internal shutdown signal");
            }
        }

        // Graceful shutdown
        self.shutdown(hls_handle).await?;

        Ok(())
    }

    /// Start RTMP server in a blocking thread
    fn start_rtmp_server(&self) {
        let bind_addr = self.config.rtmp_bind.to_string();
        let router = Arc::clone(&self.router);

        std::thread::spawn(move || {
            info!(addr = %bind_addr, "Starting RTMP server");

            let mut server = RtmpServer::with_defaults()
                .on_publish(|conn_id, stream_key| {
                    info!(conn_id, stream_key, "New publisher connected");
                    true // Allow publish
                })
                .on_play(|conn_id, stream_key| {
                    info!(conn_id, stream_key, "New player connected");
                    true // Allow play
                })
                .on_disconnect(|conn_id| {
                    info!(conn_id, "Client disconnected");
                });

            // Integrate with StreamRouter to forward media frames to HLS packagers
            server.set_router(router);
            info!("RTMP server integrated with StreamRouter");

            if let Err(e) = server.listen(&bind_addr) {
                error!(error = %e, "RTMP server error");
            }
        });
    }

    /// Start HLS server
    async fn start_hls_server(&self) -> Result<tokio::task::JoinHandle<()>> {
        let bind_addr = self.config.hls_bind;
        let router = Arc::clone(&self.router);
        let packager_manager = Arc::clone(&self.packager_manager);

        let hls_config = HlsServerConfig {
            bind_addr: bind_addr.to_string(),
            ..Default::default()
        };

        let hls_server = HlsServer::new(
            router,
            packager_manager,
            hls_config,
            rslive::hls::HlsConfig::default(),
        );

        let handle = tokio::spawn(async move {
            info!(addr = %bind_addr, "Starting HLS server");
            if let Err(e) = hls_server.run().await {
                error!(error = %e, "HLS server error");
            }
        });

        // Wait a moment for server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(handle)
    }

    /// Graceful shutdown
    async fn shutdown(self, hls_handle: tokio::task::JoinHandle<()>) -> Result<()> {
        info!("Starting graceful shutdown...");

        // Signal RTMP server to stop (if shutdown channel is available)
        if let Some(tx) = self.rtmp_shutdown {
            let _ = tx.send(());
        }

        // Abort HLS server
        hls_handle.abort();

        info!("Server stopped gracefully");
        Ok(())
    }
}

/// Parse command line arguments
fn parse_args() -> Result<ServerConfig> {
    let mut config = ServerConfig::default();

    // Simple argument parsing
    let args: Vec<String> = std::env::args().collect();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--rtmp-port" | "-r" => {
                i += 1;
                if i < args.len() {
                    let port: u16 = args[i].parse().context("Invalid RTMP port")?;
                    config.rtmp_bind.set_port(port);
                }
            }
            "--hls-port" | "-h" => {
                i += 1;
                if i < args.len() {
                    let port: u16 = args[i].parse().context("Invalid HLS port")?;
                    config.hls_bind.set_port(port);
                }
            }
            "--flv-port" | "-f" => {
                i += 1;
                if i < args.len() {
                    let port: u16 = args[i].parse().context("Invalid FLV port")?;
                    config.flv_bind.set_port(port);
                }
            }
            "--low-latency" | "-l" => {
                config.low_latency = true;
            }
            "--max-streams" => {
                i += 1;
                if i < args.len() {
                    config.max_streams = args[i].parse().context("Invalid max streams")?;
                }
            }
            "--log-level" => {
                i += 1;
                if i < args.len() {
                    config.log_level = args[i].clone();
                }
            }
            "--help" | "-?" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                warn!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }

    Ok(config)
}

/// Initialize logging
fn init_logging(log_level: &str) -> Result<()> {
    let _subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact()
        .init();

    Ok(())
}

/// Print usage information
fn print_usage() {
    println!(r#"rslive-server - High-performance streaming server

USAGE:
    rslive-server [OPTIONS]

OPTIONS:
    -r, --rtmp-port <PORT>     RTMP server port (default: 1935)
    -h, --hls-port <PORT>      HLS server port (default: 8080)
    -f, --flv-port <PORT>      HTTP-FLV server port (default: 8081)
    -l, --low-latency          Enable low-latency HLS mode
        --max-streams <N>      Maximum concurrent streams (default: 1000)
        --log-level <LEVEL>    Log level: trace, debug, info, warn, error (default: info)
        --help                 Print this help message

EXAMPLES:
    # Start with default settings
    rslive-server

    # Custom ports
    rslive-server --rtmp-port 1935 --hls-port 8080

    # Enable low-latency HLS
    rslive-server --low-latency
"#);
}

/// Print startup banner
fn print_banner(config: &ServerConfig) {
    println!(r#"
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│   🚀 rslive-server v{}                                      │
│                                                             │
│   High-performance streaming server                         │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  Protocol    │  Bind Address                                │
├──────────────┼──────────────────────────────────────────────┤
│  RTMP        │  rtmp://{}                             │
│  HLS         │  http://{}                              │
│  HTTP-FLV    │  http://{}                              │
├──────────────┴──────────────────────────────────────────────┤
│  Mode: {}                                                  │
└─────────────────────────────────────────────────────────────┘
"#,
        env!("CARGO_PKG_VERSION"),
        config.rtmp_bind,
        config.hls_bind,
        config.flv_bind,
        if config.low_latency { "Low-Latency" } else { "Standard" }
    );
}
