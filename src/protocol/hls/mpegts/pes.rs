//! PES (Packetized Elementary Stream) Encoder
//!
//! PES packets carry audio and video data. They are then split into TS packets.
//!
//! Structure:
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ PES Packet                                                    │
//! ├──────────────────────────────────────────────────────────────┤
//! │ packet_start_code_prefix (3B) = 0x000001                     │
//! │ stream_id (1B)                                                │
//! │   0xE0-0xEF: Video                                           │
//! │   0xC0-0xDF: Audio                                           │
//! │ PES_packet_length (2B)   = 0 for video (unbounded)          │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Optional PES Header                                           │
//! │   '10' (2b)                                                  │
//! │   PES_scrambling_control (2b)                                │
//! │   PES_priority (1b)                                          │
//! │   data_alignment_indicator (1b)                              │
//! │   copyright (1b)                                             │
//! │   original_or_copy (1b)                                      │
//! │   PTS_DTS_flags (2b)    = 10 (PTS only) or 11 (PTS+DTS)     │
//! │   ESCR_flag (1b)                                             │
//! │   ES_rate_flag (1b)                                          │
//! │   DSM_trick_mode_flag (1b)                                   │
//! │   additional_copy_info_flag (1b)                             │
//! │   PES_CRC_flag (1b)                                          │
//! │   PES_extension_flag (1b)                                    │
//! │   PES_header_data_length (1B)                                │
//! │   PTS (5B) if PTS_DTS_flags = 10 or 11                      │
//! │   DTS (5B) if PTS_DTS_flags = 11                             │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Payload (elementary stream data)                              │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use super::nanos_to_90khz;
use crate::media::MediaFrame;
use bytes::Bytes;

/// PES stream IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PesStreamId {
    /// Video stream 0-15 (0xE0-0xEF)
    Video(u8),

    /// Audio stream 0-15 (0xC0-0xCF)
    Audio(u8),

    /// Private stream 1 (0xBD) - for AC-3, DTS, etc.
    PrivateStream1,
}

impl PesStreamId {
    /// Get the byte value
    pub fn as_byte(&self) -> u8 {
        match self {
            Self::Video(n) => 0xE0 | (n & 0x0F),
            Self::Audio(n) => 0xC0 | (n & 0x0F),
            Self::PrivateStream1 => 0xBD,
        }
    }

    /// Create video stream ID
    pub fn video(stream_num: u8) -> Self {
        Self::Video(stream_num & 0x0F)
    }

    /// Create audio stream ID
    pub fn audio(stream_num: u8) -> Self {
        Self::Audio(stream_num & 0x0F)
    }
}

/// PTS/DTS flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtsDtsFlags {
    /// No PTS or DTS
    None = 0b00,
    /// PTS only
    PtsOnly = 0b10,
    /// Both PTS and DTS
    Both = 0b11,
}

/// PES Packet
#[derive(Debug, Clone)]
pub struct PesPacket {
    /// Stream ID
    pub stream_id: PesStreamId,

    /// PTS (Presentation Time Stamp) in 90kHz units
    pub pts: Option<u64>,

    /// DTS (Decode Time Stamp) in 90kHz units
    pub dts: Option<u64>,

    /// Payload data
    pub payload: Bytes,

    /// PES priority (higher priority packets may be transmitted first)
    pub priority: bool,

    /// Data alignment indicator
    pub data_alignment: bool,

    /// Whether this is a keyframe (random access point)
    pub random_access: bool,
}

impl PesPacket {
    /// Create a new PES packet
    pub fn new(stream_id: PesStreamId, payload: Bytes) -> Self {
        Self {
            stream_id,
            pts: None,
            dts: None,
            payload,
            priority: false,
            data_alignment: false,
            random_access: false,
        }
    }

    /// Set PTS
    pub fn with_pts(mut self, pts: u64) -> Self {
        self.pts = Some(pts & 0x1FFFFFFFF); // 33 bits
        self
    }

    /// Set DTS
    pub fn with_dts(mut self, dts: u64) -> Self {
        self.dts = Some(dts & 0x1FFFFFFFF); // 33 bits
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: bool) -> Self {
        self.priority = priority;
        self
    }

    /// Set random access (keyframe)
    pub fn with_random_access(mut self, random_access: bool) -> Self {
        self.random_access = random_access;
        self
    }

    /// Get PTS/DTS flags
    fn pts_dts_flags(&self) -> PtsDtsFlags {
        match (self.pts, self.dts) {
            (Some(_), Some(_)) => PtsDtsFlags::Both,
            (Some(_), None) => PtsDtsFlags::PtsOnly,
            _ => PtsDtsFlags::None,
        }
    }

    /// Calculate the PES header data length
    fn header_data_length(&self) -> u8 {
        let flags = self.pts_dts_flags();
        let mut len = 0u8;

        // PTS: 5 bytes
        if flags == PtsDtsFlags::PtsOnly || flags == PtsDtsFlags::Both {
            len += 5;
        }

        // DTS: 5 bytes
        if flags == PtsDtsFlags::Both {
            len += 5;
        }

        len
    }

    /// Encode PTS or DTS (5 bytes)
    ///
    /// Format:
    /// ```text
    /// Byte 0: '0010' or '0011' (marker) + PTS[32:30] + marker
    /// Byte 1: PTS[29:22]
    /// Byte 2: PTS[21:15] + marker
    /// Byte 3: PTS[14:7]
    /// Byte 4: PTS[6:0] + marker
    /// ```
    fn encode_timestamp(ts: u64, prefix: u8) -> [u8; 5] {
        let ts = ts & 0x1FFFFFFFF; // 33 bits

        let mut buf = [0u8; 5];

        // '001x' + PTS[32:30] + '1'
        buf[0] = (prefix << 4) | (((ts >> 30) & 0x07) as u8) << 1 | 0x01;

        // PTS[29:22]
        buf[1] = ((ts >> 22) & 0xFF) as u8;

        // PTS[21:15] + '1'
        buf[2] = (((ts >> 15) & 0x7F) as u8) << 1 | 0x01;

        // PTS[14:7]
        buf[3] = ((ts >> 7) & 0xFF) as u8;

        // PTS[6:0] + '1'
        buf[4] = (((ts << 1) & 0xFE) | 0x01) as u8;

        buf
    }

    /// Encode the PES packet to bytes
    pub fn encode(&self) -> Vec<u8> {
        let header_data_len = self.header_data_length();
        let payload_len = self.payload.len();

        // Calculate total packet length
        // Header: 6 bytes (start code + stream_id + length)
        // Optional header: 3 bytes (flags + header_data_length)
        // + PTS/DTS data
        // + payload

        // PES packet length field is 16 bits
        // For video, we often use 0 (unbounded) if payload is large
        let optional_header_len = 3 + header_data_len as usize;
        let packet_len_field = if payload_len + optional_header_len > 65535 - 6 {
            0 // Unbounded
        } else {
            (optional_header_len + payload_len) as u16
        };

        let mut output = Vec::with_capacity(6 + optional_header_len + payload_len);

        // Packet start code prefix (0x000001)
        output.push(0x00);
        output.push(0x00);
        output.push(0x01);

        // Stream ID
        output.push(self.stream_id.as_byte());

        // PES packet length
        output.push((packet_len_field >> 8) as u8);
        output.push((packet_len_field & 0xFF) as u8);

        // Optional PES header
        // Flags byte 1:
        // '10' (2b) + PES_scrambling_control (2b) + PES_priority (1b) +
        // data_alignment_indicator (1b) + copyright (1b) + original_or_copy (1b)
        // Flags byte 1:
        // '10' (2b) + PES_scrambling_control (2b) + PES_priority (1b) +
        // data_alignment_indicator (1b) + copyright (1b) + original_or_copy (1b)
        let flags1 = 0x80 // '10' marker
            | (0 << 6)    // PES_scrambling_control = 00
            | (if self.priority { 0x08 } else { 0 }) // PES_priority
            | 0x00        // data_alignment_indicator
            | 0x00        // copyright
            | 0x00; // original_or_copy

        // Flags byte 2
        // PTS_DTS_flags (2b) + ESCR_flag (1b) + ES_rate_flag (1b) +
        // DSM_trick_mode_flag (1b) + additional_copy_info_flag (1b) + PES_CRC_flag (1b) + PES_extension_flag (1b)
        let pts_dts = self.pts_dts_flags() as u8;
        let flags2 = (pts_dts << 6) | 0x00; // No other flags

        output.push(flags1);
        output.push(flags2);
        output.push(header_data_len);

        // PTS
        if let Some(pts) = self.pts {
            let prefix = if self.dts.is_some() { 0x03 } else { 0x02 };
            output.extend_from_slice(&Self::encode_timestamp(pts, prefix));
        }

        // DTS
        if let Some(dts) = self.dts {
            output.extend_from_slice(&Self::encode_timestamp(dts, 0x01));
        }

        // Payload
        output.extend_from_slice(&self.payload);

        output
    }

    /// Get the encoded size (approximate, for buffer allocation)
    pub fn encoded_size(&self) -> usize {
        6 + 3 + self.header_data_length() as usize + self.payload.len()
    }
}

/// PES Encoder for converting MediaFrames to PES packets
#[derive(Debug, Clone)]
pub struct PesEncoder {
    /// Stream ID for video
    video_stream_id: PesStreamId,

    /// Stream ID for audio
    audio_stream_id: PesStreamId,
}

impl Default for PesEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl PesEncoder {
    /// Create a new PES encoder
    pub fn new() -> Self {
        Self {
            video_stream_id: PesStreamId::video(0),
            audio_stream_id: PesStreamId::audio(0),
        }
    }

    /// Set video stream number
    pub fn with_video_stream(mut self, num: u8) -> Self {
        self.video_stream_id = PesStreamId::video(num);
        self
    }

    /// Set audio stream number
    pub fn with_audio_stream(mut self, num: u8) -> Self {
        self.audio_stream_id = PesStreamId::audio(num);
        self
    }

    /// Encode a video frame to PES packet
    pub fn encode_video(&self, frame: &MediaFrame) -> PesPacket {
        let pts_90khz = nanos_to_90khz(frame.pts.as_nanos());
        let dts_90khz = nanos_to_90khz(frame.dts.as_nanos());

        let mut packet =
            PesPacket::new(self.video_stream_id, (*frame.data).clone()).with_priority(true);

        // Set PTS (and DTS if different from PTS)
        if frame.pts != frame.dts {
            packet = packet.with_pts(pts_90khz).with_dts(dts_90khz);
        } else {
            packet = packet.with_pts(pts_90khz);
        }

        // Mark keyframes as random access
        if frame.is_keyframe() {
            packet = packet.with_random_access(true);
        }

        packet
    }

    /// Encode an audio frame to PES packet
    pub fn encode_audio(&self, frame: &MediaFrame) -> PesPacket {
        let pts_90khz = nanos_to_90khz(frame.pts.as_nanos());

        // Audio typically only has PTS (no DTS)
        PesPacket::new(self.audio_stream_id, (*frame.data).clone()).with_pts(pts_90khz)
    }

    /// Encode any frame to PES packet
    pub fn encode(&self, frame: &MediaFrame) -> PesPacket {
        if frame.is_video() {
            self.encode_video(frame)
        } else if frame.is_audio() {
            self.encode_audio(frame)
        } else {
            // Default to video stream ID for other data
            PesPacket::new(self.video_stream_id, (*frame.data).clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{CodecType, Timestamp, VideoFrameType};

    fn create_test_frame(is_keyframe: bool, size: usize) -> MediaFrame {
        MediaFrame::video(
            1,
            Timestamp::from_millis(1000),
            if is_keyframe {
                VideoFrameType::Keyframe
            } else {
                VideoFrameType::Interframe
            },
            CodecType::H264,
            Bytes::from(vec![0u8; size]),
        )
    }

    #[test]
    fn test_stream_id_encoding() {
        assert_eq!(PesStreamId::video(0).as_byte(), 0xE0);
        assert_eq!(PesStreamId::video(1).as_byte(), 0xE1);
        assert_eq!(PesStreamId::video(15).as_byte(), 0xEF);

        assert_eq!(PesStreamId::audio(0).as_byte(), 0xC0);
        assert_eq!(PesStreamId::audio(1).as_byte(), 0xC1);
        assert_eq!(PesStreamId::audio(15).as_byte(), 0xCF);

        assert_eq!(PesStreamId::PrivateStream1.as_byte(), 0xBD);
    }

    #[test]
    fn test_timestamp_encoding() {
        // Test with known value
        let ts: u64 = 90_000; // 1 second at 90kHz

        let encoded = PesPacket::encode_timestamp(ts, 0x02);

        // Decode back
        // Byte 0: 0010  + ts[32:30] + 1
        assert_eq!((encoded[0] >> 4) & 0x0F, 0x02);

        // Check marker bits
        assert_eq!(encoded[0] & 0x01, 0x01);
        assert_eq!(encoded[2] & 0x01, 0x01);
        assert_eq!(encoded[4] & 0x01, 0x01);
    }

    #[test]
    fn test_pes_packet_basic() {
        let payload = Bytes::from(vec![1, 2, 3, 4, 5]);
        let packet = PesPacket::new(PesStreamId::video(0), payload.clone());

        assert_eq!(packet.stream_id.as_byte(), 0xE0);
        assert_eq!(packet.payload, payload);
        assert!(packet.pts.is_none());
        assert!(packet.dts.is_none());
    }

    #[test]
    fn test_pes_packet_with_pts() {
        let packet =
            PesPacket::new(PesStreamId::video(0), Bytes::from(vec![0; 100])).with_pts(90_000);

        assert_eq!(packet.pts, Some(90_000));
        assert!(packet.dts.is_none());
        assert_eq!(packet.pts_dts_flags(), PtsDtsFlags::PtsOnly);
    }

    #[test]
    fn test_pes_packet_with_pts_dts() {
        let packet = PesPacket::new(PesStreamId::video(0), Bytes::from(vec![0; 100]))
            .with_pts(90_000)
            .with_dts(89_000);

        assert_eq!(packet.pts, Some(90_000));
        assert_eq!(packet.dts, Some(89_000));
        assert_eq!(packet.pts_dts_flags(), PtsDtsFlags::Both);
    }

    #[test]
    fn test_pes_encode_no_timestamps() {
        let packet = PesPacket::new(PesStreamId::video(0), Bytes::from(vec![1, 2, 3]));

        let encoded = packet.encode();

        // Check start code
        assert_eq!(&encoded[0..3], &[0x00, 0x00, 0x01]);

        // Check stream_id
        assert_eq!(encoded[3], 0xE0);

        // Header data length should be 0
        assert_eq!(encoded[8], 0);
    }

    #[test]
    fn test_pes_encode_with_pts() {
        let packet =
            PesPacket::new(PesStreamId::video(0), Bytes::from(vec![0; 100])).with_pts(90_000);

        let encoded = packet.encode();

        // Check start code and stream_id
        assert_eq!(&encoded[0..4], &[0x00, 0x00, 0x01, 0xE0]);

        // Check PTS_DTS_flags = 10
        assert_eq!((encoded[7] >> 6) & 0x03, 0b10);

        // Header data length should be 5
        assert_eq!(encoded[8], 5);
    }

    #[test]
    fn test_pes_encode_with_pts_dts() {
        let packet = PesPacket::new(PesStreamId::video(0), Bytes::from(vec![0; 100]))
            .with_pts(90_000)
            .with_dts(89_000);

        let encoded = packet.encode();

        // Check PTS_DTS_flags = 11
        assert_eq!((encoded[7] >> 6) & 0x03, 0b11);

        // Header data length should be 10 (5 + 5)
        assert_eq!(encoded[8], 10);
    }

    #[test]
    fn test_pes_encode_keyframe() {
        let packet = PesPacket::new(PesStreamId::video(0), Bytes::from(vec![0; 100]))
            .with_pts(90_000)
            .with_random_access(true);

        let encoded = packet.encode();

        // Check flags byte 1 for random_access indicator
        // Actually, random access is indicated in the adaptation field of TS packets
        // PES header doesn't have a direct random_access field
        // The data_alignment_indicator could be used but it's different

        // Just verify encoding works
        assert!(encoded.len() > 10);
    }

    #[test]
    fn test_pes_encoder_video() {
        let encoder = PesEncoder::new();
        let frame = create_test_frame(true, 1000);

        let packet = encoder.encode_video(&frame);

        assert_eq!(packet.stream_id.as_byte(), 0xE0);
        assert!(packet.pts.is_some());
        assert!(packet.random_access);
    }

    #[test]
    fn test_pes_encoder_audio() {
        let encoder = PesEncoder::new();
        let frame = MediaFrame::audio(
            1,
            Timestamp::from_millis(1000),
            crate::media::AudioFrameType::Raw,
            CodecType::AAC,
            Bytes::from(vec![0; 100]),
        );

        let packet = encoder.encode_audio(&frame);

        assert_eq!(packet.stream_id.as_byte(), 0xC0);
        assert!(packet.pts.is_some());
        assert!(packet.dts.is_none()); // Audio typically has no DTS
    }

    #[test]
    fn test_pes_unbounded_packet() {
        // Large payload should result in unbounded packet (length = 0)
        let large_payload = Bytes::from(vec![0u8; 70000]);
        let packet = PesPacket::new(PesStreamId::video(0), large_payload).with_pts(90_000);

        let encoded = packet.encode();

        // Check packet length field = 0
        assert_eq!(encoded[4], 0x00);
        assert_eq!(encoded[5], 0x00);
    }

    #[test]
    fn test_header_data_length() {
        let mut packet = PesPacket::new(PesStreamId::video(0), Bytes::new());
        assert_eq!(packet.header_data_length(), 0);

        packet = packet.with_pts(90_000);
        assert_eq!(packet.header_data_length(), 5);

        packet = packet.with_dts(89_000);
        assert_eq!(packet.header_data_length(), 10);
    }

    #[test]
    fn test_pts_roundtrip() {
        // Test various PTS values
        let test_values = [0, 90_000, 900_000, 9_000_000, 0x1FFFFFFFF];

        for &pts in &test_values {
            let encoded = PesPacket::encode_timestamp(pts, 0x02);

            // Decode back (simplified)
            // ts[32:30] from byte 0
            // ts[29:22] from byte 1
            // ts[21:15] from byte 2
            // ts[14:7] from byte 3
            // ts[6:0] from byte 4

            let ts_32_30 = ((encoded[0] >> 1) & 0x07) as u64;
            let ts_29_22 = encoded[1] as u64;
            let ts_21_15 = ((encoded[2] >> 1) & 0x7F) as u64;
            let ts_14_7 = encoded[3] as u64;
            let ts_6_0 = (encoded[4] >> 1) as u64;

            let decoded =
                (ts_32_30 << 30) | (ts_29_22 << 22) | (ts_21_15 << 15) | (ts_14_7 << 7) | ts_6_0;

            assert_eq!(decoded, pts & 0x1FFFFFFFF);
        }
    }
}
