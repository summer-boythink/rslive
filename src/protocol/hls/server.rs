//! HLS HTTP server for serving playlists and segments

use super::{
    m3u8::{MasterPlaylist, Variant},
    packager::HlsPackagerManager,
    HlsConfig, HlsError, HlsResult,
};
use crate::media::{StreamId, StreamRouter};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// HLS server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Server bind address
    pub bind_addr: String,
    /// Base URL for segments
    pub base_url: Option<String>,
    /// CORS allow origin
    pub cors_origin: Option<String>,
    /// Playlist cache duration
    pub playlist_cache_duration: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".to_string(),
            base_url: None,
            cors_origin: Some("*".to_string()),
            playlist_cache_duration: Duration::from_secs(1),
        }
    }
}

/// HLS HTTP server
pub struct HlsServer {
    router: Arc<StreamRouter>,
    packager_manager: Arc<HlsPackagerManager>,
    config: ServerConfig,
    hls_config: HlsConfig,
}

impl HlsServer {
    pub fn new(
        router: Arc<StreamRouter>,
        packager_manager: Arc<HlsPackagerManager>,
        config: ServerConfig,
        hls_config: HlsConfig,
    ) -> Self {
        Self {
            router,
            packager_manager,
            config,
            hls_config,
        }
    }

    /// Start the HTTP server
    pub async fn run(&self) -> HlsResult<()> {
        let app = self.create_router();

        let listener = tokio::net::TcpListener::bind(&self.config.bind_addr)
            .await
            .map_err(|e| HlsError::Io(e))?;

        info!(addr = %self.config.bind_addr, "HLS server starting");

        axum::serve(listener, app)
            .await
            .map_err(|e| HlsError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    fn create_router(&self) -> axum::Router {
        use axum::routing::get;

        let state = ServerState {
            router: Arc::clone(&self.router),
            packager_manager: Arc::clone(&self.packager_manager),
            config: self.config.clone(),
            hls_config: self.hls_config.clone(),
        };

        axum::Router::new()
            .route("/hls/:stream/master.m3u8", get(handle_master_playlist))
            .route("/hls/:stream/index.m3u8", get(handle_media_playlist))
            .route("/hls/:stream/segment:idx", get(handle_segment))
            .route("/health", get(health_check))
            .with_state(state)
    }
}

/// Server state
#[derive(Clone)]
struct ServerState {
    router: Arc<StreamRouter>,
    packager_manager: Arc<HlsPackagerManager>,
    config: ServerConfig,
    hls_config: HlsConfig,
}

/// Handle master playlist request
async fn handle_master_playlist(
    axum::extract::State(state): axum::extract::State<ServerState>,
    axum::extract::Path(stream_name): axum::extract::Path<String>,
) -> axum::response::Response {
    let stream_id = StreamId::new(stream_name.clone());

    // Check if stream exists
    if !state.router.has_stream(&stream_id) {
        return not_found(format!("Stream '{}' not found", stream_name));
    }

    // Generate master playlist
    let mut master = MasterPlaylist::new();

    // Get stream stats for bandwidth estimation
    let bandwidth = 2_000_000u64; // TODO: Get actual bitrate

    // Add variant
    master.add_variant(
        Variant::new(bandwidth, "index.m3u8")
            .with_codecs("avc1.42e00a,mp4a.40.2"),
    );

    let body = master.to_string();

    axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "application/vnd.apple.mpegurl")
        .header("Cache-Control", "no-cache")
        .body(axum::body::Body::from(body))
        .unwrap()
}

/// Handle media playlist request
async fn handle_media_playlist(
    axum::extract::State(state): axum::extract::State<ServerState>,
    axum::extract::Path(stream_name): axum::extract::Path<String>,
) -> axum::response::Response {
    let stream_id = StreamId::new(stream_name.clone());

    // Get or create packager for stream
    let packager = state.packager_manager.get_packager(&stream_id)
        .unwrap_or_else(|| state.packager_manager.create_packager(stream_id));

    // Get playlist
    let playlist = packager.playlist_string().await;

    axum::response::Response::builder()
        .status(200)
        .header("Content-Type", "application/vnd.apple.mpegurl")
        .header("Cache-Control", "max-age=1")
        .body(axum::body::Body::from(playlist))
        .unwrap()
}

/// Handle segment request
async fn handle_segment(
    axum::extract::State(state): axum::extract::State<ServerState>,
    axum::extract::Path((stream_name, idx)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    let stream_id = StreamId::new(stream_name.clone());

    // Parse segment index
    let segment_idx: u64 = match idx.parse() {
        Ok(n) => n,
        Err(_) => return not_found("Invalid segment index"),
    };

    // Get packager
    let packager = match state.packager_manager.get_packager(&stream_id) {
        Some(p) => p,
        None => return not_found(format!("Stream '{}' not found", stream_name)),
    };

    // Get segment
    match packager.get_segment(segment_idx).await {
        Ok(Some(segment)) => {
            axum::response::Response::builder()
                .status(200)
                .header("Content-Type", segment.info.format.mime_type())
                .header("Cache-Control", "max-age=3600")
                .body(axum::body::Body::from(segment.data))
                .unwrap()
        }
        Ok(None) => not_found(format!("Segment {} not found", segment_idx)),
        Err(e) => {
            error!(error = %e, "Failed to load segment");
            server_error(e.to_string())
        }
    }
}

/// Health check
async fn health_check() -> axum::response::Response {
    axum::response::Response::builder()
        .status(200)
        .body(axum::body::Body::from("OK"))
        .unwrap()
}

/// 404 response
fn not_found(message: impl Into<String>) -> axum::response::Response {
    axum::response::Response::builder()
        .status(404)
        .body(axum::body::Body::from(message.into()))
        .unwrap()
}

/// 500 response
fn server_error(message: impl Into<String>) -> axum::response::Response {
    axum::response::Response::builder()
        .status(500)
        .body(axum::body::Body::from(message.into()))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running server
    async fn test_hls_server() {
        // Test implementation would go here
    }
}
