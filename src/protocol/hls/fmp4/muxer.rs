//! FMP4 Muxer - Main Entry Point
//!
//! The Fmp4Muxer provides a high-level API for creating fMP4 streams.
//! It manages Init Segment generation and Media Segment creation.
//!
//! # Example
//!
//! ```ignore
//! use rslive::protocol::hls::fmp4::{Fmp4Muxer, Fmp4MuxerConfig, TrackConfig};
//!
//! // Create muxer
//! let config = Fmp4MuxerConfig::default();
//! let mut muxer = Fmp4Muxer::new(config);
//!
//! // Add tracks
//! muxer.add_video_track(1, CodecType::H264, 1920, 1080);
//! muxer.add_audio_track(2, CodecType::AAC, 48000, 2);
//!
//! // Get init segment
//! let init_segment = muxer.init_segment()?;
//!
//! // Add samples and get media segment
//! muxer.add_video_sample(sample);
//! let media_segment = muxer.flush_media_segment()?;
//! ```

use super::init_segment::{InitSegmentBuilder, TrackConfig};
use super::media_segment::{MediaSegmentBuilder, Sample};
use crate::media::CodecType;
use std::io;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Default segment duration in milliseconds
pub const DEFAULT_SEGMENT_DURATION_MS: u64 = 2000;

/// Error type for fMP4 muxer operations
#[derive(Debug, thiserror::Error)]
pub enum Fmp4MuxerError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// No tracks configured
    #[error("No tracks configured")]
    NoTracks,

    /// No samples to encode
    #[error("No samples to encode")]
    NoSamples,

    /// Track not found
    #[error("Track {0} not found")]
    TrackNotFound(u32),

    /// Invalid track configuration
    #[error("Invalid track configuration: {0}")]
    InvalidConfig(String),

    /// Segment too large
    #[error("Segment too large: {0} bytes")]
    SegmentTooLarge(usize),

    /// Sequence number overflow
    #[error("Sequence number overflow")]
    SequenceOverflow,
}

/// Configuration for the fMP4 muxer
#[derive(Debug, Clone)]
pub struct Fmp4MuxerConfig {
    /// Timescale for the movie (default: 1000 for milliseconds)
    pub timescale: u32,

    /// Target segment duration in milliseconds
    pub target_segment_duration_ms: u64,

    /// Maximum segment size in bytes (0 = unlimited)
    pub max_segment_size: usize,

    /// Enable low-latency mode (smaller segments)
    pub low_latency_mode: bool,

    /// Use 64-bit sizes for large boxes
    pub use_64bit_sizes: bool,
}

impl Default for Fmp4MuxerConfig {
    fn default() -> Self {
        Self {
            timescale: 1000,
            target_segment_duration_ms: DEFAULT_SEGMENT_DURATION_MS,
            max_segment_size: 0,
            low_latency_mode: false,
            use_64bit_sizes: false,
        }
    }
}

impl Fmp4MuxerConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set timescale
    pub fn with_timescale(mut self, timescale: u32) -> Self {
        self.timescale = timescale;
        self
    }

    /// Set target segment duration
    pub fn with_segment_duration(mut self, duration_ms: u64) -> Self {
        self.target_segment_duration_ms = duration_ms;
        self
    }

    /// Enable low-latency mode
    pub fn with_low_latency(mut self, enabled: bool) -> Self {
        self.low_latency_mode = enabled;
        self
    }

    /// Set maximum segment size
    pub fn with_max_segment_size(mut self, size: usize) -> Self {
        self.max_segment_size = size;
        self
    }
}

/// Track state for the muxer
#[derive(Debug)]
struct TrackState {
    config: TrackConfig,
    decode_time: AtomicU64,
    sample_count: AtomicU32,
}

impl TrackState {
    fn new(config: TrackConfig) -> Self {
        Self {
            config,
            decode_time: AtomicU64::new(0),
            sample_count: AtomicU32::new(0),
        }
    }

    fn advance_time(&self, duration: u64) {
        self.decode_time.fetch_add(duration, Ordering::SeqCst);
    }

    fn get_decode_time(&self) -> u64 {
        self.decode_time.load(Ordering::SeqCst)
    }

    fn increment_sample_count(&self) {
        self.sample_count.fetch_add(1, Ordering::SeqCst);
    }
}

/// Main fMP4 Muxer
pub struct Fmp4Muxer {
    /// Configuration
    pub config: Fmp4MuxerConfig,
    tracks: Vec<TrackState>,
    segment_builder: MediaSegmentBuilder,
    sequence_number: AtomicU32,
    init_segment_cache: Option<Vec<u8>>,
}

impl Default for Fmp4Muxer {
    fn default() -> Self {
        Self::new(Fmp4MuxerConfig::default())
    }
}

impl Fmp4Muxer {
    /// Create a new fMP4 muxer
    pub fn new(config: Fmp4MuxerConfig) -> Self {
        Self {
            config,
            tracks: Vec::new(),
            segment_builder: MediaSegmentBuilder::new(),
            sequence_number: AtomicU32::new(1),
            init_segment_cache: None,
        }
    }

    /// Add a video track
    pub fn add_video_track(
        &mut self,
        track_id: u32,
        codec: CodecType,
        width: u16,
        height: u16,
    ) -> Result<(), Fmp4MuxerError> {
        self.add_track(TrackConfig::video(track_id, codec, width, height))
    }

    /// Add an audio track
    pub fn add_audio_track(
        &mut self,
        track_id: u32,
        codec: CodecType,
        sample_rate: u32,
        channels: u8,
    ) -> Result<(), Fmp4MuxerError> {
        self.add_track(TrackConfig::audio(track_id, codec, sample_rate, channels))
    }

    /// Add a track with custom configuration
    pub fn add_track(&mut self, config: TrackConfig) -> Result<(), Fmp4MuxerError> {
        // Validate track ID is unique
        if self
            .tracks
            .iter()
            .any(|t| t.config.track_id == config.track_id)
        {
            return Err(Fmp4MuxerError::InvalidConfig(format!(
                "Duplicate track ID: {}",
                config.track_id
            )));
        }

        // Invalidate cached init segment
        self.init_segment_cache = None;

        self.tracks.push(TrackState::new(config));
        Ok(())
    }

    /// Generate the Init Segment (ftyp + moov)
    ///
    /// This should be sent once at the beginning of the stream.
    /// The result is cached for efficiency.
    pub fn init_segment(&mut self) -> Result<Vec<u8>, Fmp4MuxerError> {
        if self.tracks.is_empty() {
            return Err(Fmp4MuxerError::NoTracks);
        }

        // Return cached init segment if available
        if let Some(ref cached) = self.init_segment_cache {
            return Ok(cached.clone());
        }

        let mut builder = InitSegmentBuilder::new().with_timescale(self.config.timescale);

        for track in &self.tracks {
            builder = builder.add_track(track.config.clone());
        }

        let init = builder.build()?;
        self.init_segment_cache = Some(init.clone());

        Ok(init)
    }

    /// Add a media sample to the current segment
    pub fn add_sample(&mut self, sample: Sample) -> Result<(), Fmp4MuxerError> {
        let track_id = sample.track_id;

        // Find the track
        let track = self
            .tracks
            .iter()
            .find(|t| t.config.track_id == track_id)
            .ok_or(Fmp4MuxerError::TrackNotFound(track_id))?;

        // Update track state
        track.advance_time(sample.duration as u64);
        track.increment_sample_count();

        // Add to segment builder
        if track.config.is_video() {
            self.segment_builder.add_video_sample(sample);
        } else {
            self.segment_builder.add_audio_sample(sample);
        }

        Ok(())
    }

    /// Add multiple samples at once
    pub fn add_samples(&mut self, samples: Vec<Sample>) -> Result<(), Fmp4MuxerError> {
        for sample in samples {
            self.add_sample(sample)?;
        }
        Ok(())
    }

    /// Check if the current segment is ready to be flushed
    pub fn is_segment_ready(&self) -> bool {
        if self.segment_builder.is_empty() {
            return false;
        }

        // Check duration
        let video_duration = self.segment_builder.video_duration();
        let audio_duration = self.segment_builder.audio_duration();
        let max_duration = video_duration.max(audio_duration);

        let target_duration = self.config.target_segment_duration_ms;

        if max_duration >= target_duration {
            return true;
        }

        // Check size if limit is set
        if self.config.max_segment_size > 0 {
            // Estimate segment size (rough approximation)
            let estimated_size = self.estimate_segment_size();
            if estimated_size >= self.config.max_segment_size {
                return true;
            }
        }

        false
    }

    /// Estimate current segment size
    fn estimate_segment_size(&self) -> usize {
        // Rough estimate: moof ~200 bytes + mdat with sample data
        let sample_size: usize = self
            .tracks
            .iter()
            .map(|t| t.sample_count.load(Ordering::SeqCst) as usize * 1000) // Estimate 1KB per sample
            .sum();
        200 + sample_size
    }

    /// Flush the current media segment (moof + mdat)
    ///
    /// This should be called when:
    /// - Segment duration target is reached
    /// - Explicit segment boundary is needed
    /// - End of stream
    pub fn flush_media_segment(&mut self) -> Result<Vec<u8>, Fmp4MuxerError> {
        if self.segment_builder.is_empty() {
            return Err(Fmp4MuxerError::NoSamples);
        }

        // Set sequence number
        let seq = self.sequence_number.fetch_add(1, Ordering::SeqCst);
        self.segment_builder = self.segment_builder.clone().with_sequence_number(seq);

        // Set decode times from track states
        for track in &self.tracks {
            let decode_time = track.get_decode_time();
            if track.config.is_video() {
                self.segment_builder = self
                    .segment_builder
                    .clone()
                    .with_video_decode_time(decode_time);
            } else {
                self.segment_builder = self
                    .segment_builder
                    .clone()
                    .with_audio_decode_time(decode_time);
            }
        }

        // Build segment
        let segment = self.segment_builder.build()?;

        // Check size limit
        if self.config.max_segment_size > 0 && segment.len() > self.config.max_segment_size {
            return Err(Fmp4MuxerError::SegmentTooLarge(segment.len()));
        }

        // Reset for next segment
        self.segment_builder.clear();

        Ok(segment)
    }

    /// Get current sequence number
    pub fn sequence_number(&self) -> u32 {
        self.sequence_number.load(Ordering::SeqCst)
    }

    /// Get track count
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Check if there are pending samples
    pub fn has_pending_samples(&self) -> bool {
        !self.segment_builder.is_empty()
    }

    /// Get total sample count across all tracks
    pub fn total_sample_count(&self) -> u32 {
        self.tracks
            .iter()
            .map(|t| t.sample_count.load(Ordering::SeqCst))
            .sum()
    }

    /// Reset the muxer state (keeping track configurations)
    pub fn reset(&mut self) {
        for track in &self.tracks {
            track.decode_time.store(0, Ordering::SeqCst);
            track.sample_count.store(0, Ordering::SeqCst);
        }
        self.segment_builder.clear();
        self.sequence_number.store(1, Ordering::SeqCst);
    }

    /// Create a new segment with the given samples
    ///
    /// Convenience method for creating a single segment from samples.
    pub fn create_segment(&mut self, samples: Vec<Sample>) -> Result<Vec<u8>, Fmp4MuxerError> {
        self.add_samples(samples)?;
        self.flush_media_segment()
    }

    /// Get video track decode time
    pub fn video_decode_time(&self) -> u64 {
        self.tracks
            .iter()
            .find(|t| t.config.is_video())
            .map(|t| t.get_decode_time())
            .unwrap_or(0)
    }

    /// Get audio track decode time
    pub fn audio_decode_time(&self) -> u64 {
        self.tracks
            .iter()
            .find(|t| t.config.is_audio())
            .map(|t| t.get_decode_time())
            .unwrap_or(0)
    }
}

/// Builder for creating Fmp4Muxer with fluent API
pub struct Fmp4MuxerBuilder {
    config: Fmp4MuxerConfig,
    tracks: Vec<TrackConfig>,
}

impl Default for Fmp4MuxerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Fmp4MuxerBuilder {
    pub fn new() -> Self {
        Self {
            config: Fmp4MuxerConfig::default(),
            tracks: Vec::new(),
        }
    }

    /// Set configuration
    pub fn with_config(mut self, config: Fmp4MuxerConfig) -> Self {
        self.config = config;
        self
    }

    /// Set timescale
    pub fn with_timescale(mut self, timescale: u32) -> Self {
        self.config.timescale = timescale;
        self
    }

    /// Set target segment duration
    pub fn with_segment_duration(mut self, duration_ms: u64) -> Self {
        self.config.target_segment_duration_ms = duration_ms;
        self
    }

    /// Enable low-latency mode
    pub fn with_low_latency(mut self, enabled: bool) -> Self {
        self.config.low_latency_mode = enabled;
        self
    }

    /// Add a video track
    pub fn video_track(mut self, track_id: u32, codec: CodecType, width: u16, height: u16) -> Self {
        self.tracks
            .push(TrackConfig::video(track_id, codec, width, height));
        self
    }

    /// Add an audio track
    pub fn audio_track(
        mut self,
        track_id: u32,
        codec: CodecType,
        sample_rate: u32,
        channels: u8,
    ) -> Self {
        self.tracks
            .push(TrackConfig::audio(track_id, codec, sample_rate, channels));
        self
    }

    /// Build the muxer
    pub fn build(self) -> Result<Fmp4Muxer, Fmp4MuxerError> {
        let mut muxer = Fmp4Muxer::new(self.config);
        for track in self.tracks {
            muxer.add_track(track)?;
        }
        Ok(muxer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_muxer_config() {
        let config = Fmp4MuxerConfig::new()
            .with_timescale(90000)
            .with_segment_duration(1000)
            .with_low_latency(true);

        assert_eq!(config.timescale, 90000);
        assert_eq!(config.target_segment_duration_ms, 1000);
        assert!(config.low_latency_mode);
    }

    #[test]
    fn test_muxer_builder() {
        let muxer = Fmp4MuxerBuilder::new()
            .with_timescale(1000)
            .video_track(1, CodecType::H264, 1920, 1080)
            .audio_track(2, CodecType::AAC, 48000, 2)
            .build()
            .unwrap();

        assert_eq!(muxer.track_count(), 2);
    }

    #[test]
    fn test_muxer_init_segment() {
        let mut muxer = Fmp4Muxer::new(Fmp4MuxerConfig::default());
        muxer
            .add_video_track(1, CodecType::H264, 1920, 1080)
            .unwrap();

        let init = muxer.init_segment().unwrap();

        // Verify structure
        assert!(init.windows(4).any(|w| w == b"ftyp"));
        assert!(init.windows(4).any(|w| w == b"moov"));
        assert!(init.windows(4).any(|w| w == b"trak"));
    }

    #[test]
    fn test_muxer_init_segment_no_tracks() {
        let mut muxer = Fmp4Muxer::new(Fmp4MuxerConfig::default());

        let result = muxer.init_segment();
        assert!(matches!(result, Err(Fmp4MuxerError::NoTracks)));
    }

    #[test]
    fn test_muxer_add_sample() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        let sample = Sample::video_keyframe(vec![0; 1000], 40);
        muxer.add_sample(sample).unwrap();

        assert!(muxer.has_pending_samples());
    }

    #[test]
    fn test_muxer_add_sample_invalid_track() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        let sample = Sample::new(999, vec![0; 100], 40, true); // Invalid track ID
        let result = muxer.add_sample(sample);

        assert!(matches!(result, Err(Fmp4MuxerError::TrackNotFound(999))));
    }

    #[test]
    fn test_muxer_flush_empty() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        let result = muxer.flush_media_segment();
        assert!(matches!(result, Err(Fmp4MuxerError::NoSamples)));
    }

    #[test]
    fn test_muxer_media_segment() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .audio_track(2, CodecType::AAC, 48000, 2)
            .build()
            .unwrap();

        // Get init segment first
        let _init = muxer.init_segment().unwrap();

        // Add samples
        for _ in 0..10 {
            muxer
                .add_sample(Sample::video_keyframe(vec![0; 5000], 40))
                .unwrap();
            muxer.add_sample(Sample::audio(vec![0; 100], 20)).unwrap();
        }

        // Flush media segment
        let segment = muxer.flush_media_segment().unwrap();

        // Verify structure
        assert!(segment.windows(4).any(|w| w == b"moof"));
        assert!(segment.windows(4).any(|w| w == b"mdat"));
        assert!(segment.windows(4).any(|w| w == b"mfhd"));
        assert!(segment.windows(4).any(|w| w == b"traf"));
    }

    #[test]
    fn test_muxer_segment_ready() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .with_segment_duration(100) // 100ms target
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        // Initially not ready
        assert!(!muxer.is_segment_ready());

        // Add samples to reach target duration (40ms each, need 3 for >100ms)
        muxer
            .add_sample(Sample::video_keyframe(vec![0; 1000], 40))
            .unwrap();
        assert!(!muxer.is_segment_ready());

        muxer
            .add_sample(Sample::video_frame(vec![0; 500], 40))
            .unwrap();
        assert!(!muxer.is_segment_ready());

        muxer
            .add_sample(Sample::video_frame(vec![0; 500], 40))
            .unwrap();
        // Now 120ms > 100ms target
        assert!(muxer.is_segment_ready());
    }

    #[test]
    fn test_muxer_sequence_number() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        assert_eq!(muxer.sequence_number(), 1);

        muxer
            .add_sample(Sample::video_keyframe(vec![0; 100], 40))
            .unwrap();
        muxer.flush_media_segment().unwrap();
        assert_eq!(muxer.sequence_number(), 2);

        muxer
            .add_sample(Sample::video_frame(vec![0; 100], 40))
            .unwrap();
        muxer.flush_media_segment().unwrap();
        assert_eq!(muxer.sequence_number(), 3);
    }

    #[test]
    fn test_muxer_reset() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        muxer
            .add_sample(Sample::video_keyframe(vec![0; 100], 40))
            .unwrap();
        muxer.flush_media_segment().unwrap();

        assert_eq!(muxer.sequence_number(), 2);

        muxer.reset();

        assert_eq!(muxer.sequence_number(), 1);
        assert!(!muxer.has_pending_samples());
    }

    #[test]
    fn test_muxer_duplicate_track_id() {
        let mut muxer = Fmp4Muxer::new(Fmp4MuxerConfig::default());

        muxer
            .add_video_track(1, CodecType::H264, 1920, 1080)
            .unwrap();

        let result = muxer.add_video_track(1, CodecType::H265, 1920, 1080);
        assert!(matches!(result, Err(Fmp4MuxerError::InvalidConfig(_))));
    }

    #[test]
    fn test_muxer_create_segment() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        let samples = vec![
            Sample::video_keyframe(vec![0; 1000], 40),
            Sample::video_frame(vec![0; 500], 40),
            Sample::video_frame(vec![0; 500], 40),
        ];

        let segment = muxer.create_segment(samples).unwrap();

        assert!(segment.windows(4).any(|w| w == b"moof"));
        assert!(segment.windows(4).any(|w| w == b"mdat"));
    }

    #[test]
    fn test_muxer_decode_time_tracking() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .audio_track(2, CodecType::AAC, 48000, 2)
            .build()
            .unwrap();

        // Add video samples (40ms each)
        for _ in 0..5 {
            muxer
                .add_sample(Sample::video_keyframe(vec![0; 100], 40))
                .unwrap();
        }

        // Add audio samples (20ms each)
        for _ in 0..10 {
            muxer.add_sample(Sample::audio(vec![0; 50], 20)).unwrap();
        }

        // Video: 5 * 40 = 200ms
        assert_eq!(muxer.video_decode_time(), 200);
        // Audio: 10 * 20 = 200ms
        assert_eq!(muxer.audio_decode_time(), 200);
    }

    #[test]
    fn test_muxer_init_segment_cache() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        let init1 = muxer.init_segment().unwrap();
        let init2 = muxer.init_segment().unwrap();

        // Should be exactly the same (cached)
        assert_eq!(init1, init2);
    }

    #[test]
    fn test_muxer_total_sample_count() {
        let mut muxer = Fmp4MuxerBuilder::new()
            .video_track(1, CodecType::H264, 1920, 1080)
            .audio_track(2, CodecType::AAC, 48000, 2)
            .build()
            .unwrap();

        assert_eq!(muxer.total_sample_count(), 0);

        muxer
            .add_sample(Sample::video_keyframe(vec![0; 100], 40))
            .unwrap();
        muxer
            .add_sample(Sample::video_frame(vec![0; 100], 40))
            .unwrap();
        muxer.add_sample(Sample::audio(vec![0; 50], 20)).unwrap();

        assert_eq!(muxer.total_sample_count(), 3);
    }

    #[test]
    fn test_muxer_builder_with_config() {
        let config = Fmp4MuxerConfig::new()
            .with_timescale(90000)
            .with_segment_duration(2000);

        let muxer = Fmp4MuxerBuilder::new()
            .with_config(config)
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        assert_eq!(muxer.track_count(), 1);
    }

    #[test]
    fn test_muxer_builder_with_low_latency() {
        let muxer = Fmp4MuxerBuilder::new()
            .with_low_latency(true)
            .with_segment_duration(500)
            .video_track(1, CodecType::H264, 1920, 1080)
            .build()
            .unwrap();

        assert!(muxer.config.low_latency_mode);
        assert_eq!(muxer.config.target_segment_duration_ms, 500);
    }
}
