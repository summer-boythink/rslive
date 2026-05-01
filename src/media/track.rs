//! Media track management for multi-track streams

use super::{CodecType, MediaError, MediaResult, Timestamp};
use bytes::Bytes;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Type of media track
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackType {
    Video,
    Audio,
    Data,
    Subtitle,
}

impl TrackType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrackType::Video => "video",
            TrackType::Audio => "audio",
            TrackType::Data => "data",
            TrackType::Subtitle => "subtitle",
        }
    }
}

/// Track identification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackId(pub u32);

impl TrackId {
    pub const VIDEO: Self = Self(1);
    pub const AUDIO: Self = Self(2);

    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl From<u32> for TrackId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

/// Information about a media track
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub id: TrackId,
    pub track_type: TrackType,
    pub codec: CodecType,
    pub language: Option<String>,
    pub name: Option<String>,

    // Video-specific
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub pixel_aspect_ratio: Option<(u32, u32)>,

    // Audio-specific
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub sample_size: Option<u16>,

    // Codec extra data (e.g., AVCDecoderConfigurationRecord for H.264)
    pub extra_data: Option<Bytes>,
}

impl TrackInfo {
    pub fn new_video(id: TrackId, codec: CodecType) -> Self {
        Self {
            id,
            track_type: TrackType::Video,
            codec,
            language: None,
            name: None,
            width: None,
            height: None,
            frame_rate: None,
            pixel_aspect_ratio: None,
            sample_rate: None,
            channels: None,
            sample_size: None,
            extra_data: None,
        }
    }

    pub fn new_audio(id: TrackId, codec: CodecType) -> Self {
        Self {
            id,
            track_type: TrackType::Audio,
            codec,
            language: Some("und".to_string()),
            name: None,
            width: None,
            height: None,
            frame_rate: None,
            pixel_aspect_ratio: None,
            sample_rate: None,
            channels: None,
            sample_size: None,
            extra_data: None,
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

    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = Some(rate);
        self
    }

    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = Some(channels);
        self
    }

    pub fn with_extra_data(mut self, data: Bytes) -> Self {
        self.extra_data = Some(data);
        self
    }

    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    /// Calculate bitrate from frame size and frame rate
    pub fn estimated_bitrate(&self) -> Option<u32> {
        match self.track_type {
            TrackType::Video => {
                let (w, h) = (self.width?, self.height?);
                let fps = self.frame_rate?;
                // Rough estimation: 0.1 bits per pixel per frame for H.264
                let bpp = match self.codec {
                    CodecType::H264 | CodecType::H265 => 0.1,
                    CodecType::AV1 => 0.08,
                    CodecType::VP8 | CodecType::VP9 => 0.09,
                    _ => 0.1,
                };
                Some(((w * h) as f64 * fps * bpp * 8.0 / 1000.0) as u32)
            }
            TrackType::Audio => {
                let rate = self.sample_rate?;
                let ch = self.channels?;
                // Rough estimation: 128 kbps per channel for AAC
                Some(rate * ch as u32 * 128 / 1000)
            }
            _ => None,
        }
    }

    /// Check if this is a video track
    pub fn is_video(&self) -> bool {
        self.track_type == TrackType::Video
    }

    /// Check if this is an audio track
    pub fn is_audio(&self) -> bool {
        self.track_type == TrackType::Audio
    }
}

/// A media track with state management
pub struct MediaTrack {
    info: TrackInfo,
    sequence_number: AtomicU64,
    last_timestamp: AtomicU64,
    total_bytes: AtomicU64,
    frame_count: AtomicU64,
    keyframe_count: AtomicU64,
}

impl MediaTrack {
    pub fn new(info: TrackInfo) -> Self {
        Self {
            info,
            sequence_number: AtomicU64::new(0),
            last_timestamp: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            frame_count: AtomicU64::new(0),
            keyframe_count: AtomicU64::new(0),
        }
    }

    pub fn info(&self) -> &TrackInfo {
        &self.info
    }

    pub fn next_sequence_number(&self) -> u64 {
        self.sequence_number.fetch_add(1, Ordering::Relaxed)
    }

    pub fn record_frame(&self, timestamp: Timestamp, size: usize, is_keyframe: bool) {
        self.last_timestamp.store(timestamp.as_nanos(), Ordering::Relaxed);
        self.total_bytes.fetch_add(size as u64, Ordering::Relaxed);
        self.frame_count.fetch_add(1, Ordering::Relaxed);
        if is_keyframe {
            self.keyframe_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn last_timestamp(&self) -> Timestamp {
        Timestamp::from_nanos(self.last_timestamp.load(Ordering::Relaxed))
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count.load(Ordering::Relaxed)
    }

    pub fn keyframe_count(&self) -> u64 {
        self.keyframe_count.load(Ordering::Relaxed)
    }

    pub fn keyframe_ratio(&self) -> f64 {
        let frames = self.frame_count.load(Ordering::Relaxed);
        let keyframes = self.keyframe_count.load(Ordering::Relaxed);
        if frames > 0 {
            keyframes as f64 / frames as f64
        } else {
            0.0
        }
    }
}

/// Track manager for multi-track streams
pub struct TrackManager {
    tracks: DashMap<TrackId, Arc<MediaTrack>>,
    next_id: AtomicUsize,
}

impl TrackManager {
    pub fn new() -> Self {
        Self {
            tracks: DashMap::new(),
            next_id: AtomicUsize::new(1),
        }
    }

    /// Add a new track
    pub fn add_track(&self, info: TrackInfo) -> Arc<MediaTrack> {
        let track = Arc::new(MediaTrack::new(info));
        self.tracks.insert(track.info().id, Arc::clone(&track));
        track
    }

    /// Create a video track with auto-assigned ID
    pub fn add_video_track(&self, codec: CodecType) -> (TrackId, Arc<MediaTrack>) {
        let id = TrackId::new(self.next_id.fetch_add(1, Ordering::Relaxed) as u32);
        let info = TrackInfo::new_video(id, codec);
        let track = self.add_track(info);
        (id, track)
    }

    /// Create an audio track with auto-assigned ID
    pub fn add_audio_track(&self, codec: CodecType) -> (TrackId, Arc<MediaTrack>) {
        let id = TrackId::new(self.next_id.fetch_add(1, Ordering::Relaxed) as u32);
        let info = TrackInfo::new_audio(id, codec);
        let track = self.add_track(info);
        (id, track)
    }

    /// Get a track by ID
    pub fn get(&self, id: TrackId) -> Option<Arc<MediaTrack>> {
        self.tracks.get(&id).map(|t| Arc::clone(t.value()))
    }

    /// Remove a track
    pub fn remove(&self, id: TrackId) -> Option<Arc<MediaTrack>> {
        self.tracks.remove(&id).map(|(_, t)| t)
    }

    /// Get all track IDs
    pub fn track_ids(&self) -> Vec<TrackId> {
        self.tracks.iter().map(|e| e.key().clone()).collect()
    }

    /// Get all video tracks
    pub fn video_tracks(&self) -> Vec<Arc<MediaTrack>> {
        self.tracks
            .iter()
            .filter(|e| e.value().info().is_video())
            .map(|e| Arc::clone(e.value()))
            .collect()
    }

    /// Get all audio tracks
    pub fn audio_tracks(&self) -> Vec<Arc<MediaTrack>> {
        self.tracks
            .iter()
            .filter(|e| e.value().info().is_audio())
            .map(|e| Arc::clone(e.value()))
            .collect()
    }

    /// Get primary video track (first video track)
    pub fn primary_video(&self) -> Option<Arc<MediaTrack>> {
        self.tracks
            .iter()
            .find(|e| e.value().info().is_video())
            .map(|e| Arc::clone(e.value()))
    }

    /// Get primary audio track (first audio track)
    pub fn primary_audio(&self) -> Option<Arc<MediaTrack>> {
        self.tracks
            .iter()
            .find(|e| e.value().info().is_audio())
            .map(|e| Arc::clone(e.value()))
    }

    /// Get total bitrate across all tracks
    pub fn total_bitrate(&self) -> u32 {
        self.tracks
            .iter()
            .filter_map(|e| e.value().info().estimated_bitrate())
            .sum()
    }

    /// Clear all tracks
    pub fn clear(&self) {
        self.tracks.clear();
    }

    /// Get track count
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}

impl Default for TrackManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_info() {
        let info = TrackInfo::new_video(TrackId::new(1), CodecType::H264)
            .with_dimensions(1920, 1080)
            .with_frame_rate(30.0);

        assert_eq!(info.id.as_u32(), 1);
        assert_eq!(info.width, Some(1920));
        assert_eq!(info.height, Some(1080));
        assert!(info.is_video());
        assert!(!info.is_audio());
    }

    #[test]
    fn test_track_manager() {
        let manager = TrackManager::new();

        let (video_id, video_track) = manager.add_video_track(CodecType::H264);
        let (audio_id, audio_track) = manager.add_audio_track(CodecType::AAC);

        assert_eq!(manager.len(), 2);

        let video = manager.get(video_id).unwrap();
        assert!(video.info().is_video());

        let audio = manager.get(audio_id).unwrap();
        assert!(audio.info().is_audio());

        assert_eq!(manager.video_tracks().len(), 1);
        assert_eq!(manager.audio_tracks().len(), 1);
    }

    #[test]
    fn test_track_stats() {
        let info = TrackInfo::new_video(TrackId::VIDEO, CodecType::H264);
        let track = MediaTrack::new(info);

        track.record_frame(Timestamp::from_millis(1000), 1000, true);
        track.record_frame(Timestamp::from_millis(1033), 500, false);
        track.record_frame(Timestamp::from_millis(1066), 500, false);

        assert_eq!(track.frame_count(), 3);
        assert_eq!(track.keyframe_count(), 1);
        assert_eq!(track.total_bytes(), 2000);
        assert!((track.keyframe_ratio() - 0.333).abs() < 0.01);
    }
}
