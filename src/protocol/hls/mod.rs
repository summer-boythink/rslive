//! HLS (HTTP Live Streaming) protocol implementation
//!
//! HLS is Apple's adaptive bitrate streaming protocol, widely used
//! for live and on-demand streaming on iOS and other platforms.
//!
//! Features:
//! - Standard HLS (RFC 8216)
//! - Low-Latency HLS (LL-HLS) support
//! - MPEG-TS and fMP4 (CMAF) segment formats
//! - Automatic playlist generation and management

use crate::media::{CodecType, MediaFrame, Timestamp};
use bytes::Bytes;
use std::time::Duration;

pub mod m3u8;
pub mod packager;
pub mod segment;
pub mod server;

pub use m3u8::{MediaPlaylist, MasterPlaylist, PlaylistType};
pub use packager::{HlsPackager, HlsPackagerManager, PackagerConfig};
pub use segment::{Segment, SegmentFormat, SegmentInfo, MemorySegmentStorage};
pub use server::{HlsServer, ServerConfig};

/// HLS configuration
#[derive(Debug, Clone)]
pub struct HlsConfig {
    /// Target segment duration in seconds
    pub target_duration: Duration,
    /// Playlist type (Live, Event, or VOD)
    pub playlist_type: PlaylistType,
    /// Number of segments to keep in playlist
    pub playlist_size: usize,
    /// Enable Low-Latency HLS
    pub low_latency: bool,
    /// Partial segment duration for LL-HLS
    pub partial_segment_duration: Duration,
    /// Segment format (TS or fMP4)
    pub segment_format: SegmentFormat,
    /// Output directory for segments
    pub output_dir: std::path::PathBuf,
    /// Base URL for segments
    pub base_url: Option<String>,
}

impl Default for HlsConfig {
    fn default() -> Self {
        Self {
            target_duration: Duration::from_secs(6),
            playlist_type: PlaylistType::Live,
            playlist_size: 6,
            low_latency: false,
            partial_segment_duration: Duration::from_millis(200),
            segment_format: SegmentFormat::MpegTs,
            output_dir: std::path::PathBuf::from("./hls"),
            base_url: None,
        }
    }
}

impl HlsConfig {
    pub fn for_low_latency() -> Self {
        Self {
            target_duration: Duration::from_secs(4),
            partial_segment_duration: Duration::from_millis(200),
            low_latency: true,
            playlist_type: PlaylistType::Live,
            playlist_size: 12,
            ..Default::default()
        }
    }

    pub fn with_target_duration(mut self, secs: u64) -> Self {
        self.target_duration = Duration::from_secs(secs);
        self
    }

    pub fn with_playlist_size(mut self, size: usize) -> Self {
        self.playlist_size = size;
        self
    }

    pub fn with_output_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.output_dir = dir.into();
        self
    }
}

/// HLS error types
#[derive(Debug, thiserror::Error)]
pub enum HlsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),

    #[error("Segment not found: {0}")]
    SegmentNotFound(String),

    #[error("Playlist generation failed: {0}")]
    PlaylistGeneration(String),

    #[error("MPEG-TS error: {0}")]
    MpegTs(String),

    #[error("fMP4 error: {0}")]
    Fmp4(String),
}

pub type HlsResult<T> = Result<T, HlsError>;

/// Convert MediaFrame to PES packet data
pub fn frame_to_pes_data(frame: &MediaFrame) -> HlsResult<Bytes> {
    // This is a simplified version
    // Real implementation would handle:
    // - AnnexB to AVCC conversion for H.264
    // - ADTS header for AAC
    // - PES packet headers

    match frame.codec {
        CodecType::H264 | CodecType::H265 => {
            // Video PES
            // TODO: Implement proper PES encapsulation
            Ok(frame.data.as_ref().clone())
        }
        CodecType::AAC => {
            // Audio PES
            // TODO: Implement proper ADTS header + PES encapsulation
            Ok(frame.data.as_ref().clone())
        }
        _ => Err(HlsError::UnsupportedCodec(format!("{:?}", frame.codec))),
    }
}

/// Calculate PTS for MPEG-TS
pub fn calc_ts_timestamp(timestamp: Timestamp, timebase: u32) -> u64 {
    // MPEG-TS uses 90kHz clock
    let pts_90khz = timestamp.as_nanos() * 90_000 / 1_000_000_000;
    pts_90khz * (timebase as u64) / 90_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hls_config() {
        let config = HlsConfig::for_low_latency();
        assert!(config.low_latency);
        assert_eq!(config.target_duration, Duration::from_secs(4));
    }
}
