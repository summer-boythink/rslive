//! TsMuxer - Main entry point for MPEG-TS generation
//!
//! This module provides a complete MPEG-TS muxer that can convert
//! MediaFrames into a valid MPEG-TS stream.
//!
//! # Example
//!
//! ```rust,ignore
//! use rslive::hls::mpegts::{TsMuxer, TsMuxerConfig};
//! use rslive::media::{MediaFrame, CodecType};
//!
//! let config = TsMuxerConfig::default();
//! let mut muxer = TsMuxer::new(config);
//!
//! // Add frames
//! let segment = muxer.create_segment(&frames);
//! ```

use super::{
    AdaptationField, ContinuityCounter, DEFAULT_AUDIO_PID, DEFAULT_PCR_INTERVAL_MS,
    DEFAULT_PMT_PID, DEFAULT_PROGRAM_NUMBER, DEFAULT_VIDEO_PID, PatGenerator, PcrValue, PesEncoder,
    PmtGenerator, StreamInfo, StreamType, TS_PACKET_SIZE, TsPacket, TsPacketHeader,
};
use crate::media::{CodecType, MediaFrame};
use bytes::Bytes;
use std::time::Duration;

/// Configuration for TsMuxer
#[derive(Debug, Clone)]
pub struct TsMuxerConfig {
    /// PID for PMT
    pub pmt_pid: u16,

    /// PID for video stream
    pub video_pid: u16,

    /// PID for audio stream
    pub audio_pid: u16,

    /// PID for PCR (usually same as video)
    pub pcr_pid: u16,

    /// Program number
    pub program_number: u16,

    /// PCR interval in milliseconds
    pub pcr_interval: Duration,

    /// Video codec
    pub video_codec: Option<CodecType>,

    /// Audio codec
    pub audio_codec: Option<CodecType>,
}

impl Default for TsMuxerConfig {
    fn default() -> Self {
        Self {
            pmt_pid: DEFAULT_PMT_PID,
            video_pid: DEFAULT_VIDEO_PID,
            audio_pid: DEFAULT_AUDIO_PID,
            pcr_pid: DEFAULT_VIDEO_PID,
            program_number: DEFAULT_PROGRAM_NUMBER,
            pcr_interval: Duration::from_millis(DEFAULT_PCR_INTERVAL_MS),
            video_codec: Some(CodecType::H264),
            audio_codec: Some(CodecType::AAC),
        }
    }
}

impl TsMuxerConfig {
    /// Create config with specific codecs
    pub fn with_codecs(video: CodecType, audio: CodecType) -> Self {
        Self {
            video_codec: Some(video),
            audio_codec: Some(audio),
            ..Default::default()
        }
    }
}

/// MPEG-TS Muxer
pub struct TsMuxer {
    /// Configuration
    config: TsMuxerConfig,

    /// PAT generator
    pat_generator: PatGenerator,

    /// PMT generator
    pmt_generator: PmtGenerator,

    /// PES encoder
    pes_encoder: PesEncoder,

    /// Continuity counter manager
    continuity: ContinuityCounter,

    /// Last PCR time (for interval control)
    last_pcr_time: Option<u64>,

    /// Current PTS for PCR calculation
    current_pcr: u64,

    /// Number of bytes written since last PCR
    bytes_since_pcr: usize,
}

impl TsMuxer {
    /// Create a new TsMuxer
    pub fn new(config: TsMuxerConfig) -> Self {
        // Initialize PAT generator
        let mut pat_generator = PatGenerator::new().with_transport_stream_id(0x0001);
        pat_generator.add_program(config.program_number, config.pmt_pid);

        // Initialize PMT generator
        let mut pmt_generator =
            PmtGenerator::new(config.program_number, config.pmt_pid).with_pcr_pid(config.pcr_pid);

        // Add streams to PMT
        if let Some(video_codec) = config.video_codec {
            if let Some(stream_type) = StreamType::from_codec(video_codec) {
                pmt_generator.add_stream(StreamInfo::new(stream_type, config.video_pid));
            }
        }

        if let Some(audio_codec) = config.audio_codec {
            if let Some(stream_type) = StreamType::from_codec(audio_codec) {
                pmt_generator.add_stream(StreamInfo::new(stream_type, config.audio_pid));
            }
        }

        Self {
            config,
            pat_generator,
            pmt_generator,
            pes_encoder: PesEncoder::new(),
            continuity: ContinuityCounter::new(),
            last_pcr_time: None,
            current_pcr: 0,
            bytes_since_pcr: 0,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(TsMuxerConfig::default())
    }

    /// Detect codecs from frames
    fn detect_codecs(frames: &[MediaFrame]) -> (Option<CodecType>, Option<CodecType>) {
        let mut video_codec = None;
        let mut audio_codec = None;

        for frame in frames {
            if frame.is_video() && video_codec.is_none() {
                video_codec = Some(frame.codec);
            }
            if frame.is_audio() && audio_codec.is_none() {
                audio_codec = Some(frame.codec);
            }

            if video_codec.is_some() && audio_codec.is_some() {
                break;
            }
        }

        (video_codec, audio_codec)
    }

    /// Create a TS segment from frames
    pub fn create_segment(&mut self, frames: &[MediaFrame]) -> Bytes {
        // Update config if codecs changed
        let (video_codec, audio_codec) = Self::detect_codecs(frames);
        if video_codec != self.config.video_codec || audio_codec != self.config.audio_codec {
            self.update_pmt(video_codec, audio_codec);
        }

        let mut output = Vec::new();

        // Write PAT
        let pat_data = self.pat_generator.generate(&mut self.continuity);
        output.extend_from_slice(&pat_data);

        // Write PMT
        let pmt_data = self.pmt_generator.generate(&mut self.continuity);
        output.extend_from_slice(&pmt_data);

        // Reset PCR tracking for new segment
        self.current_pcr = 0;
        self.bytes_since_pcr = 0;

        // Process frames
        let mut first_frame = true;
        for frame in frames {
            let packets = self.frame_to_ts_packets(frame, first_frame);
            for packet in packets {
                if let Ok(encoded) = packet.encode() {
                    output.extend_from_slice(&encoded);
                }
            }
            first_frame = false;
        }

        Bytes::from(output)
    }

    /// Update PMT with new codec info
    fn update_pmt(&mut self, video_codec: Option<CodecType>, audio_codec: Option<CodecType>) {
        self.config.video_codec = video_codec;
        self.config.audio_codec = audio_codec;

        // Rebuild PMT
        self.pmt_generator.clear();

        if let Some(codec) = video_codec {
            if let Some(stream_type) = StreamType::from_codec(codec) {
                self.pmt_generator
                    .add_stream(StreamInfo::new(stream_type, self.config.video_pid));
            }
        }

        if let Some(codec) = audio_codec {
            if let Some(stream_type) = StreamType::from_codec(codec) {
                self.pmt_generator
                    .add_stream(StreamInfo::new(stream_type, self.config.audio_pid));
            }
        }

        self.pmt_generator.increment_version();
    }

    /// Convert a frame to TS packets
    fn frame_to_ts_packets(&mut self, frame: &MediaFrame, is_first: bool) -> Vec<TsPacket> {
        // Encode frame to PES
        let pes_packet = self.pes_encoder.encode(frame);
        let pes_data = pes_packet.encode();

        let pid = if frame.is_video() {
            self.config.video_pid
        } else {
            self.config.audio_pid
        };

        // Update current PCR from frame timestamp
        let frame_pts = frame.pts.as_nanos();
        if is_first || self.should_insert_pcr(frame_pts) {
            self.current_pcr = frame_pts;
            self.bytes_since_pcr = 0;
        }

        // Split PES data into TS packets
        self.pes_to_ts_packets(&pes_data, pid, frame.is_keyframe(), frame_pts)
    }

    /// Check if we should insert PCR
    fn should_insert_pcr(&self, current_time: u64) -> bool {
        match self.last_pcr_time {
            None => true,
            Some(last) => {
                let elapsed_ns = current_time.saturating_sub(last);
                elapsed_ns >= self.config.pcr_interval.as_nanos() as u64
            }
        }
    }

    /// Convert PES data to TS packets
    fn pes_to_ts_packets(
        &mut self,
        pes_data: &[u8],
        pid: u16,
        is_keyframe: bool,
        pts_nanos: u64,
    ) -> Vec<TsPacket> {
        let mut packets = Vec::new();
        let mut remaining = pes_data;
        let mut first_packet = true;

        while !remaining.is_empty() {
            // Determine if this packet should have PCR
            let insert_pcr =
                first_packet && (pid == self.config.pcr_pid) && self.should_insert_pcr(pts_nanos);

            // Calculate payload capacity
            let header_overhead = if insert_pcr {
                // Adaptation field with PCR: ~8 bytes
                4 + 8 // header + adaptation field
            } else {
                4 // just header
            };

            let payload_capacity = TS_PACKET_SIZE - header_overhead;

            // Determine how much data to take
            let take = remaining.len().min(payload_capacity);
            let payload = remaining[..take].to_vec();
            remaining = &remaining[take..];

            // Build adaptation field
            let adaptation_field = if insert_pcr {
                self.last_pcr_time = Some(pts_nanos);
                Some(AdaptationField::with_pcr(PcrValue::from_nanos(pts_nanos)))
            } else if is_keyframe && first_packet {
                Some(AdaptationField::with_random_access())
            } else {
                None
            };

            // Build packet
            let header = TsPacketHeader::new(pid)
                .with_pusi(first_packet)
                .with_cc(self.continuity.next(pid))
                .with_afc(if adaptation_field.is_some() { 3 } else { 1 });

            let mut packet = TsPacket::new(pid);
            packet.header = header;

            if let Some(af) = adaptation_field {
                packet = packet.with_adaptation_field(af);
            }

            packet = packet.with_payload(payload);

            // Pad if needed
            let padded = self.pad_packet(packet);
            packets.push(padded);

            first_packet = false;
        }

        packets
    }

    /// Pad a packet to exactly 188 bytes
    fn pad_packet(&self, mut packet: TsPacket) -> TsPacket {
        let header_len = 4;
        let af_len = packet.adaptation_field.as_ref().map_or(0, |af| af.len());
        let payload_len = packet.payload.len();

        let total = header_len + af_len + payload_len;

        if total < TS_PACKET_SIZE {
            let padding_needed = TS_PACKET_SIZE - total;

            if padding_needed > 0 {
                // Add stuffing to adaptation field
                let mut af = packet
                    .adaptation_field
                    .take()
                    .unwrap_or_else(AdaptationField::new);
                af.stuffing_bytes += padding_needed;
                packet.adaptation_field = Some(af);
                packet.header.adaptation_field_control =
                    if packet.payload.is_empty() { 2 } else { 3 };
            }
        }

        packet
    }

    /// Get the PAT for this muxer
    pub fn pat(&mut self) -> Bytes {
        Bytes::from(self.pat_generator.generate(&mut self.continuity))
    }

    /// Get the PMT for this muxer
    pub fn pmt(&mut self) -> Bytes {
        Bytes::from(self.pmt_generator.generate(&mut self.continuity))
    }

    /// Reset continuity counters (for new segment)
    pub fn reset(&mut self) {
        self.continuity = ContinuityCounter::new();
        self.last_pcr_time = None;
        self.current_pcr = 0;
        self.bytes_since_pcr = 0;
    }
}

/// Segment information for MPEG-TS
#[derive(Debug, Clone)]
pub struct TsSegmentInfo {
    /// Segment duration
    pub duration: Duration,

    /// First PTS in segment
    pub first_pts: u64,

    /// Last PTS in segment
    pub last_pts: u64,

    /// Whether segment starts with keyframe
    pub starts_with_keyframe: bool,

    /// Segment size in bytes
    pub size: usize,

    /// Video codec
    pub video_codec: Option<CodecType>,

    /// Audio codec
    pub audio_codec: Option<CodecType>,
}

/// Create a TS segment with metadata
pub fn create_ts_segment(frames: &[MediaFrame]) -> Result<(Bytes, TsSegmentInfo), TsMuxerError> {
    if frames.is_empty() {
        return Err(TsMuxerError::EmptyFrames);
    }

    // Detect codecs
    let (video_codec, audio_codec) = TsMuxer::detect_codecs(frames);

    // Create muxer with detected codecs
    let mut config = TsMuxerConfig::default();
    config.video_codec = video_codec;
    config.audio_codec = audio_codec;

    let mut muxer = TsMuxer::new(config);

    // Generate segment
    let data = muxer.create_segment(frames);

    // Calculate metadata
    let first_pts = frames[0].pts.as_nanos();
    let last_pts = frames[frames.len() - 1].pts.as_nanos();
    let duration = Duration::from_nanos(last_pts.saturating_sub(first_pts));

    let starts_with_keyframe = frames[0].is_keyframe();

    let info = TsSegmentInfo {
        duration,
        first_pts,
        last_pts,
        starts_with_keyframe,
        size: data.len(),
        video_codec,
        audio_codec,
    };

    Ok((data, info))
}

/// Errors from TsMuxer
#[derive(Debug, Clone, thiserror::Error)]
pub enum TsMuxerError {
    #[error("No frames to create segment")]
    EmptyFrames,

    #[error("Unsupported codec: {0:?}")]
    UnsupportedCodec(CodecType),

    #[error("Failed to encode packet: {0}")]
    EncodingError(String),
}

#[cfg(test)]
mod tests {
    use super::super::{DEFAULT_AUDIO_PID, DEFAULT_VIDEO_PID, TS_PACKET_SIZE};
    use super::*;
    use crate::media::{AudioFrameType, Timestamp, VideoFrameType};

    fn create_video_frame(pts_ms: u64, is_keyframe: bool, size: usize) -> MediaFrame {
        MediaFrame::video(
            1,
            Timestamp::from_millis(pts_ms),
            if is_keyframe {
                VideoFrameType::Keyframe
            } else {
                VideoFrameType::Interframe
            },
            CodecType::H264,
            Bytes::from(vec![0u8; size]),
        )
    }

    fn create_audio_frame(pts_ms: u64, size: usize) -> MediaFrame {
        MediaFrame::audio(
            2,
            Timestamp::from_millis(pts_ms),
            AudioFrameType::Raw,
            CodecType::AAC,
            Bytes::from(vec![0u8; size]),
        )
    }

    #[test]
    fn test_muxer_config_default() {
        let config = TsMuxerConfig::default();

        assert_eq!(config.pmt_pid, DEFAULT_PMT_PID);
        assert_eq!(config.video_pid, DEFAULT_VIDEO_PID);
        assert_eq!(config.audio_pid, DEFAULT_AUDIO_PID);
        assert_eq!(config.pcr_pid, DEFAULT_VIDEO_PID);
    }

    #[test]
    fn test_muxer_creation() {
        let muxer = TsMuxer::with_defaults();

        assert!(muxer.config.video_codec.is_some());
        assert!(muxer.config.audio_codec.is_some());
    }

    #[test]
    fn test_create_segment_basic() {
        let mut muxer = TsMuxer::with_defaults();

        let frames = vec![
            create_video_frame(0, true, 1000),
            create_video_frame(33, false, 500),
            create_video_frame(66, false, 500),
        ];

        let segment = muxer.create_segment(&frames);

        // Segment should not be empty
        assert!(!segment.is_empty());

        // Should be multiple of 188
        assert_eq!(segment.len() % TS_PACKET_SIZE, 0);

        // Should start with sync byte
        assert_eq!(segment[0], 0x47);
    }

    #[test]
    fn test_segment_starts_with_pat() {
        let mut muxer = TsMuxer::with_defaults();

        let frames = vec![create_video_frame(0, true, 1000)];
        let segment = muxer.create_segment(&frames);

        // First packet should be PAT (PID 0x0000)
        let pid = ((segment[1] as u16 & 0x1F) << 8) | (segment[2] as u16);
        assert_eq!(pid, 0x0000); // PAT_PID
    }

    #[test]
    fn test_segment_contains_pmt() {
        let mut muxer = TsMuxer::with_defaults();

        let frames = vec![create_video_frame(0, true, 1000)];
        let segment = muxer.create_segment(&frames);

        // Find PMT packet (should be after PAT)
        // PAT is one packet (188 bytes), PMT should follow
        let pmt_start = TS_PACKET_SIZE; // After first packet

        // PMT PID is 0x1000 by default
        let pid = ((segment[pmt_start + 1] as u16 & 0x1F) << 8) | (segment[pmt_start + 2] as u16);
        assert_eq!(pid, DEFAULT_PMT_PID);
    }

    #[test]
    fn test_segment_with_keyframe() {
        let mut muxer = TsMuxer::with_defaults();

        let frames = vec![create_video_frame(0, true, 5000)];
        let segment = muxer.create_segment(&frames);

        // Should contain video data (PID 0x0100)
        // Look for video packets after PAT and PMT
        let mut found_video = false;
        for chunk in segment.chunks(TS_PACKET_SIZE) {
            let pid = ((chunk[1] as u16 & 0x1F) << 8) | (chunk[2] as u16);
            if pid == DEFAULT_VIDEO_PID {
                found_video = true;
                break;
            }
        }

        assert!(found_video);
    }

    #[test]
    fn test_segment_with_audio() {
        let mut muxer = TsMuxer::with_defaults();

        let frames = vec![
            create_video_frame(0, true, 1000),
            create_audio_frame(0, 200),
        ];
        let segment = muxer.create_segment(&frames);

        // Should contain audio packets (PID 0x0101)
        let mut found_audio = false;
        for chunk in segment.chunks(TS_PACKET_SIZE) {
            let pid = ((chunk[1] as u16 & 0x1F) << 8) | (chunk[2] as u16);
            if pid == DEFAULT_AUDIO_PID {
                found_audio = true;
                break;
            }
        }

        assert!(found_audio);
    }

    #[test]
    fn test_continuity_counter_increment() {
        let mut muxer = TsMuxer::with_defaults();

        // Use larger frames to ensure multiple video packets
        let frames = vec![
            create_video_frame(0, true, 5000),
            create_video_frame(33, false, 5000),
        ];

        let segment = muxer.create_segment(&frames);

        // Find video packets and check CC increments
        let video_packets: Vec<_> = segment
            .chunks(TS_PACKET_SIZE)
            .filter(|chunk| {
                let pid = ((chunk[1] as u16 & 0x1F) << 8) | (chunk[2] as u16);
                pid == DEFAULT_VIDEO_PID
            })
            .collect();

        // Should have multiple video packets with 5000 byte frames
        assert!(
            video_packets.len() >= 2,
            "Expected at least 2 video packets, got {}",
            video_packets.len()
        );

        // CC should increment (wrapping at 16)
        let cc1 = video_packets[0][3] & 0x0F;
        let cc2 = video_packets[1][3] & 0x0F;

        assert_eq!((cc1 + 1) & 0x0F, cc2);
    }

    #[test]
    fn test_pcr_insertion() {
        let mut muxer = TsMuxer::with_defaults();

        // Create a large frame to span multiple packets
        let frames = vec![create_video_frame(0, true, 10000)];
        let segment = muxer.create_segment(&frames);

        // First video packet should have PCR
        let mut found_pcr = false;
        for chunk in segment.chunks(TS_PACKET_SIZE) {
            let pid = ((chunk[1] as u16 & 0x1F) << 8) | (chunk[2] as u16);
            if pid == DEFAULT_VIDEO_PID {
                // Check adaptation field control
                let afc = (chunk[3] >> 4) & 0x03;
                if afc == 2 || afc == 3 {
                    // Has adaptation field
                    // Check PCR flag (bit 4 of flags byte)
                    let af_len = chunk[4] as usize;
                    if af_len > 0 {
                        let flags = chunk[5];
                        if flags & 0x10 != 0 {
                            found_pcr = true;
                            break;
                        }
                    }
                }
            }
        }

        assert!(found_pcr);
    }

    #[test]
    fn test_create_ts_segment_function() {
        let frames = vec![
            create_video_frame(0, true, 1000),
            create_video_frame(33, false, 500),
        ];

        let result = create_ts_segment(&frames);
        assert!(result.is_ok());

        let (data, info) = result.unwrap();

        assert!(!data.is_empty());
        assert!(info.starts_with_keyframe);
        assert_eq!(info.video_codec, Some(CodecType::H264));
    }

    #[test]
    fn test_empty_frames_error() {
        let frames: Vec<MediaFrame> = vec![];
        let result = create_ts_segment(&frames);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TsMuxerError::EmptyFrames));
    }

    #[test]
    fn test_muxer_reset() {
        let mut muxer = TsMuxer::with_defaults();

        // Generate a segment
        let frames = vec![create_video_frame(0, true, 1000)];
        muxer.create_segment(&frames);

        // Reset
        muxer.reset();

        // Continuity counters should be reset
        assert_eq!(muxer.continuity.current(DEFAULT_VIDEO_PID), 0);
    }

    #[test]
    fn test_large_frame() {
        let mut muxer = TsMuxer::with_defaults();

        // Create a frame larger than one TS packet
        let frames = vec![create_video_frame(0, true, 50000)];
        let segment = muxer.create_segment(&frames);

        // Should be valid TS stream
        assert_eq!(segment.len() % TS_PACKET_SIZE, 0);

        // Count video packets
        let video_count = segment
            .chunks(TS_PACKET_SIZE)
            .filter(|chunk| {
                let pid = ((chunk[1] as u16 & 0x1F) << 8) | (chunk[2] as u16);
                pid == DEFAULT_VIDEO_PID
            })
            .count();

        // Should need multiple packets for 50KB
        assert!(video_count >= 10);
    }

    #[test]
    fn test_h265_codec() {
        let mut config = TsMuxerConfig::default();
        config.video_codec = Some(CodecType::H265);

        let mut muxer = TsMuxer::new(config);

        let frames = vec![MediaFrame::video(
            1,
            Timestamp::from_millis(0),
            VideoFrameType::Keyframe,
            CodecType::H265,
            Bytes::from(vec![0; 1000]),
        )];

        let segment = muxer.create_segment(&frames);
        assert!(!segment.is_empty());
    }
}
