//! Media abstraction layer for unified frame handling across protocols
//!
//! This module provides common types and traits for media streaming,
//! enabling zero-copy forwarding between different protocols.

use bytes::Bytes;
use std::time::Duration;

pub mod frame;
pub mod router;
pub mod track;

pub use frame::{AudioFrame, AudioFrameType, FrameType, MediaFrame, VideoFrame, VideoFrameType};
pub use router::{
    RouterConfig, StreamId, StreamRouter, StreamSink, StreamSource, StreamSubscriber,
};
pub use track::{MediaTrack, TrackInfo, TrackType};

/// Codec types supported across all protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecType {
    // Video codecs
    H264,
    H265,
    AV1,
    VP8,
    VP9,
    // Audio codecs
    AAC,
    Opus,
    Mp3,
    G711A, // G.711 A-law
    G711U, // G.711 μ-law
}

impl CodecType {
    /// Returns true if this is a video codec
    pub fn is_video(&self) -> bool {
        matches!(
            self,
            CodecType::H264 | CodecType::H265 | CodecType::AV1 | CodecType::VP8 | CodecType::VP9
        )
    }

    /// Returns true if this is an audio codec
    pub fn is_audio(&self) -> bool {
        !self.is_video()
    }

    /// Returns the MIME type for this codec
    pub fn mime_type(&self) -> &'static str {
        match self {
            CodecType::H264 => "video/avc",
            CodecType::H265 => "video/hevc",
            CodecType::AV1 => "video/av1",
            CodecType::VP8 => "video/vp8",
            CodecType::VP9 => "video/vp9",
            CodecType::AAC => "audio/aac",
            CodecType::Opus => "audio/opus",
            CodecType::Mp3 => "audio/mpeg",
            CodecType::G711A => "audio/g711-alaw",
            CodecType::G711U => "audio/g711-mlaw",
        }
    }
}

/// Timestamp with nanosecond precision
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const ZERO: Self = Self(0);

    pub fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub fn from_millis(millis: u64) -> Self {
        Self(millis * 1_000_000)
    }

    pub fn from_seconds(secs: u64) -> Self {
        Self(secs * 1_000_000_000)
    }

    pub fn as_nanos(&self) -> u64 {
        self.0
    }

    pub fn as_millis(&self) -> u64 {
        self.0 / 1_000_000
    }

    pub fn as_seconds(&self) -> u64 {
        self.0 / 1_000_000_000
    }

    pub fn duration_since(&self, other: Timestamp) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(other.0))
    }
}

impl std::ops::Add<Duration> for Timestamp {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.0 + rhs.as_nanos() as u64)
    }
}

impl std::ops::Sub<Duration> for Timestamp {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        Self(self.0.saturating_sub(rhs.as_nanos() as u64))
    }
}

/// Metadata for a media stream
#[derive(Debug, Clone)]
pub struct StreamMetadata {
    pub duration: Option<Duration>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub video_codec: Option<CodecType>,
    pub audio_codec: Option<CodecType>,
    pub video_bitrate: Option<u32>,
    pub audio_bitrate: Option<u32>,
    pub extra_data: Option<Bytes>, // Codec-specific extradata (e.g., AVCDecoderConfigurationRecord)
}

impl StreamMetadata {
    pub fn new() -> Self {
        Self {
            duration: None,
            width: None,
            height: None,
            frame_rate: None,
            video_codec: None,
            audio_codec: None,
            video_bitrate: None,
            audio_bitrate: None,
            extra_data: None,
        }
    }

    pub fn with_video(mut self, codec: CodecType, width: u32, height: u32) -> Self {
        self.video_codec = Some(codec);
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub fn with_audio(mut self, codec: CodecType) -> Self {
        self.audio_codec = Some(codec);
        self
    }
}

/// Error types for media operations
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid codec: {0}")]
    InvalidCodec(String),

    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    #[error("Track not found: {0}")]
    TrackNotFound(String),

    #[error("Invalid frame data: {0}")]
    InvalidFrame(String),

    #[error("Router error: {0}")]
    Router(String),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Buffer full")]
    BufferFull,
}

pub type MediaResult<T> = Result<T, MediaError>;
