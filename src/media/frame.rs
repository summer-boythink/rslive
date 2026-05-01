//! Media frame types with zero-copy semantics

use bytes::Bytes;
use std::sync::Arc;

use super::{CodecType, Timestamp};

/// Type of media frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Video(VideoFrameType),
    Audio(AudioFrameType),
    Data(DataFrameType),
}

impl FrameType {
    pub fn is_video(&self) -> bool {
        matches!(self, FrameType::Video(_))
    }

    pub fn is_audio(&self) -> bool {
        matches!(self, FrameType::Audio(_))
    }

    pub fn is_keyframe(&self) -> bool {
        matches!(self, FrameType::Video(VideoFrameType::Keyframe))
    }
}

/// Video frame types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameType {
    Keyframe,
    Interframe,
    DisposableInterframe, // H.264 only
    GeneratedKeyframe,
    VideoInfoFrame,
}

impl VideoFrameType {
    /// Create from H.264 NAL unit type
    pub fn from_h264_nal(nal_type: u8) -> Option<Self> {
        match nal_type {
            5 => Some(Self::Keyframe),   // IDR slice
            1 => Some(Self::Interframe), // Non-IDR slice
            _ => None,
        }
    }

    /// Create from FLV video frame type
    pub fn from_flv_frame_type(frame_type: u8) -> Option<Self> {
        match frame_type {
            1 => Some(Self::Keyframe),
            2 => Some(Self::Interframe),
            3 => Some(Self::DisposableInterframe),
            4 => Some(Self::GeneratedKeyframe),
            5 => Some(Self::VideoInfoFrame),
            _ => None,
        }
    }

    /// Convert to FLV video frame type
    pub fn to_flv_frame_type(&self) -> u8 {
        match self {
            Self::Keyframe => 1,
            Self::Interframe => 2,
            Self::DisposableInterframe => 3,
            Self::GeneratedKeyframe => 4,
            Self::VideoInfoFrame => 5,
        }
    }
}

/// Audio frame types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFrameType {
    /// AAC sequence header / MP3 header
    SequenceHeader,
    /// Raw audio data
    Raw,
}

impl AudioFrameType {
    pub fn from_flv_frame_type(frame_type: u8) -> Option<Self> {
        match frame_type {
            0 => Some(Self::SequenceHeader),
            1 => Some(Self::Raw),
            _ => None,
        }
    }

    pub fn to_flv_frame_type(&self) -> u8 {
        match self {
            Self::SequenceHeader => 0,
            Self::Raw => 1,
        }
    }
}

/// Data frame types (metadata, subtitles, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFrameType {
    Metadata,
    Subtitle,
    Custom(u8),
}

/// Unified media frame structure
///
/// Uses Arc<Bytes> for zero-copy sharing between protocols.
/// The actual data is reference-counted and only cloned when necessary.
#[derive(Clone)]
pub struct MediaFrame {
    /// Track/stream identifier
    pub track_id: u32,
    /// Presentation timestamp
    pub pts: Timestamp,
    /// Decode timestamp (may differ from pts for B-frames)
    pub dts: Timestamp,
    /// Frame type
    pub frame_type: FrameType,
    /// Codec type
    pub codec: CodecType,
    /// Frame data (reference-counted for zero-copy)
    pub data: Arc<Bytes>,
}

impl MediaFrame {
    /// Create a new media frame
    pub fn new(
        track_id: u32,
        pts: Timestamp,
        frame_type: FrameType,
        codec: CodecType,
        data: Bytes,
    ) -> Self {
        Self {
            track_id,
            pts,
            dts: pts,
            frame_type,
            codec,
            data: Arc::new(data),
        }
    }

    /// Create with explicit decode timestamp
    pub fn with_dts(
        track_id: u32,
        pts: Timestamp,
        dts: Timestamp,
        frame_type: FrameType,
        codec: CodecType,
        data: Bytes,
    ) -> Self {
        Self {
            track_id,
            pts,
            dts,
            frame_type,
            codec,
            data: Arc::new(data),
        }
    }

    /// Create a video frame
    pub fn video(
        track_id: u32,
        pts: Timestamp,
        video_type: VideoFrameType,
        codec: CodecType,
        data: Bytes,
    ) -> Self {
        Self::new(track_id, pts, FrameType::Video(video_type), codec, data)
    }

    /// Create an audio frame
    pub fn audio(
        track_id: u32,
        pts: Timestamp,
        audio_type: AudioFrameType,
        codec: CodecType,
        data: Bytes,
    ) -> Self {
        Self::new(track_id, pts, FrameType::Audio(audio_type), codec, data)
    }

    /// Check if this is a video frame
    pub fn is_video(&self) -> bool {
        self.frame_type.is_video()
    }

    /// Check if this is an audio frame
    pub fn is_audio(&self) -> bool {
        self.frame_type.is_audio()
    }

    /// Check if this is a keyframe
    pub fn is_keyframe(&self) -> bool {
        self.frame_type.is_keyframe()
    }

    /// Get frame size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Get duration between decode and presentation
    pub fn composition_time(&self) -> i64 {
        self.pts.as_nanos() as i64 - self.dts.as_nanos() as i64
    }

    /// Clone with shared data (zero-copy)
    pub fn share(&self) -> Self {
        Self {
            track_id: self.track_id,
            pts: self.pts,
            dts: self.dts,
            frame_type: self.frame_type,
            codec: self.codec,
            data: Arc::clone(&self.data),
        }
    }

    /// Convert to owned bytes (requires copy)
    pub fn to_bytes(&self) -> Bytes {
        (*self.data).clone()
    }
}

impl std::fmt::Debug for MediaFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaFrame")
            .field("track_id", &self.track_id)
            .field("pts", &self.pts)
            .field("dts", &self.dts)
            .field("frame_type", &self.frame_type)
            .field("codec", &self.codec)
            .field("size", &self.size())
            .finish()
    }
}

/// Video-specific frame with extra metadata
#[derive(Clone)]
pub struct VideoFrame {
    pub base: MediaFrame,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
}

impl VideoFrame {
    pub fn new(base: MediaFrame) -> Self {
        Self {
            base,
            width: None,
            height: None,
            frame_rate: None,
        }
    }

    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub fn with_frame_rate(mut self, fps: f64) -> Self {
        self.frame_rate = Some(fps);
        self
    }
}

impl std::ops::Deref for VideoFrame {
    type Target = MediaFrame;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

/// Audio-specific frame with extra metadata
#[derive(Clone)]
pub struct AudioFrame {
    pub base: MediaFrame,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub sample_size: Option<u16>,
}

impl AudioFrame {
    pub fn new(base: MediaFrame) -> Self {
        Self {
            base,
            sample_rate: None,
            channels: None,
            sample_size: None,
        }
    }

    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = Some(rate);
        self
    }

    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = Some(channels);
        self
    }
}

impl std::ops::Deref for AudioFrame {
    type Target = MediaFrame;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

/// Frame statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct FrameStats {
    pub frames_total: u64,
    pub frames_key: u64,
    pub frames_inter: u64,
    pub bytes_total: u64,
    pub bytes_video: u64,
    pub bytes_audio: u64,
    pub last_pts: Option<Timestamp>,
    pub first_pts: Option<Timestamp>,
}

impl FrameStats {
    pub fn record(&mut self, frame: &MediaFrame) {
        self.frames_total += 1;
        self.bytes_total += frame.size() as u64;

        match frame.frame_type {
            FrameType::Video(vt) => {
                self.bytes_video += frame.size() as u64;
                match vt {
                    VideoFrameType::Keyframe => self.frames_key += 1,
                    _ => self.frames_inter += 1,
                }
            }
            FrameType::Audio(_) => {
                self.bytes_audio += frame.size() as u64;
            }
            _ => {}
        }

        self.last_pts = Some(frame.pts);
        if self.first_pts.is_none() {
            self.first_pts = Some(frame.pts);
        }
    }

    pub fn bitrate_bps(&self, duration_secs: f64) -> u64 {
        if duration_secs > 0.0 {
            (self.bytes_total as f64 * 8.0 / duration_secs) as u64
        } else {
            0
        }
    }

    pub fn frame_rate(&self, duration_secs: f64) -> f64 {
        if duration_secs > 0.0 {
            self.frames_total as f64 / duration_secs
        } else {
            0.0
        }
    }

    pub fn keyframe_ratio(&self) -> f64 {
        if self.frames_total > 0 {
            self.frames_key as f64 / self.frames_total as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_frame_creation() {
        let data = Bytes::from(vec![0; 100]);
        let frame = MediaFrame::video(
            1,
            Timestamp::from_millis(1000),
            VideoFrameType::Keyframe,
            CodecType::H264,
            data.clone(),
        );

        assert_eq!(frame.track_id, 1);
        assert_eq!(frame.pts.as_millis(), 1000);
        assert!(frame.is_video());
        assert!(frame.is_keyframe());
        assert_eq!(frame.size(), 100);
    }

    #[test]
    fn test_zero_copy_share() {
        let data = Bytes::from(vec![0; 1000]);
        let frame1 = MediaFrame::audio(
            1,
            Timestamp::ZERO,
            AudioFrameType::Raw,
            CodecType::AAC,
            data,
        );

        let frame2 = frame1.share();

        // Both frames point to same data
        assert_eq!(Arc::strong_count(&frame1.data), 2); // frame1 and frame2 share the same Arc
        assert!(Arc::ptr_eq(&frame1.data, &frame2.data));
    }

    #[test]
    fn test_frame_stats() {
        let mut stats = FrameStats::default();

        for i in 0..10 {
            let frame_type = if i % 5 == 0 {
                VideoFrameType::Keyframe
            } else {
                VideoFrameType::Interframe
            };

            let frame = MediaFrame::video(
                1,
                Timestamp::from_millis(i as u64 * 100),
                frame_type,
                CodecType::H264,
                Bytes::from(vec![0; 1000]),
            );
            stats.record(&frame);
        }

        assert_eq!(stats.frames_total, 10);
        assert_eq!(stats.frames_key, 2); // 0, 5
        assert_eq!(stats.frames_inter, 8);
        assert_eq!(stats.bytes_total, 10000);
    }
}
