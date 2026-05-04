//! PMT (Program Map Table) Generator
//!
//! PMT describes the streams (audio, video, etc.) that comprise a program.
//! Each program has its own PMT.
//!
//! Structure:
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ PMT Section                                                   │
//! ├──────────────────────────────────────────────────────────────┤
//! │ table_id (1B)                 = 0x02                         │
//! │ section_syntax_indicator (1b) = 1                            │
//! │ '0' (1b)                      = 0                            │
//! │ reserved (2b)                 = 11                           │
//! │ section_length (12b)          = N                            │
//! │ program_number (2B)                                          │
//! │ reserved (2b)                 = 11                           │
//! │ version_number (5b)                                          │
//! │ current_next_indicator (1b)   = 1                            │
//! │ section_number (1B)           = 0                            │
//! │ last_section_number (1B)      = 0                            │
//! │ reserved (3b)                                                │
//! │ PCR_PID (13b)                                                │
//! │ reserved (4b)                                                │
//! │ program_info_length (12b)                                    │
//! │ ┌──────────────────────────────────────────────────────────┐ │
//! │ │ Program descriptors (optional)                           │ │
//! │ └──────────────────────────────────────────────────────────┘ │
//! │ ┌──────────────────────────────────────────────────────────┐ │
//! │ │ Stream Info Loop (N entries)                             │ │
//! │ │   stream_type (1B)                                       │ │
//! │ │   reserved (3b)                                          │ │
//! │ │   elementary_PID (13b)                                   │ │
//! │ │   reserved (4b)                                          │ │
//! │ │   ES_info_length (12b)                                   │ │
//! │ │   ES descriptors (optional)                              │ │
//! │ └──────────────────────────────────────────────────────────┘ │
//! │ CRC32 (4B)                                                   │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use super::{
    calculate_crc32, TsPacket, TsPacketHeader, ContinuityCounter,
    TS_PACKET_SIZE,
};
use crate::media::CodecType;

/// Stream types (ISO/IEC 13818-1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// MPEG-1 Video (ITU-T Rec. H.262 | ISO/IEC 13818-2)
    Mpeg1Video = 0x01,

    /// MPEG-2 Video
    Mpeg2Video = 0x02,

    /// MPEG-1 Audio (ISO/IEC 11172-3)
    Mpeg1Audio = 0x03,

    /// MPEG-2 Audio (ISO/IEC 13818-3)
    Mpeg2Audio = 0x04,

    /// H.264/AVC Video (ISO/IEC 14496-10)
    H264 = 0x1B,

    /// H.265/HEVC Video (ITU-T Rec. H.265)
    H265 = 0x24,

    /// AAC Audio (ISO/IEC 13818-7)
    Aac = 0x0F,

    /// LATM AAC (ISO/IEC 14496-3)
    AacLatm = 0x11,

    /// AC-3 Audio (ATSC A/52)
    Ac3 = 0x81,

    /// Enhanced AC-3 (E-AC-3)
    Eac3 = 0x87,

    /// Private data (for Opus, etc.)
    PrivateData = 0x06,
}

impl StreamType {
    /// Get stream type from codec
    pub fn from_codec(codec: CodecType) -> Option<Self> {
        match codec {
            CodecType::H264 => Some(Self::H264),
            CodecType::H265 => Some(Self::H265),
            CodecType::AAC => Some(Self::Aac),
            CodecType::Opus => Some(Self::PrivateData),
            CodecType::Mp3 => Some(Self::Mpeg1Audio),
            _ => None,
        }
    }

    /// Get the byte value
    pub fn as_byte(&self) -> u8 {
        *self as u8
    }
}

impl std::fmt::Display for StreamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mpeg1Video => write!(f, "MPEG-1 Video"),
            Self::Mpeg2Video => write!(f, "MPEG-2 Video"),
            Self::Mpeg1Audio => write!(f, "MPEG-1 Audio (MP3)"),
            Self::Mpeg2Audio => write!(f, "MPEG-2 Audio"),
            Self::H264 => write!(f, "H.264/AVC"),
            Self::H265 => write!(f, "H.265/HEVC"),
            Self::Aac => write!(f, "AAC"),
            Self::AacLatm => write!(f, "AAC LATM"),
            Self::Ac3 => write!(f, "AC-3"),
            Self::Eac3 => write!(f, "E-AC-3"),
            Self::PrivateData => write!(f, "Private Data"),
        }
    }
}

/// Stream information entry in PMT
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// Stream type (audio, video, etc.)
    pub stream_type: StreamType,

    /// PID for this elementary stream
    pub elementary_pid: u16,

    /// Optional descriptors (e.g., codec configuration)
    pub descriptors: Vec<u8>,
}

impl StreamInfo {
    /// Create a new stream info
    pub fn new(stream_type: StreamType, elementary_pid: u16) -> Self {
        Self {
            stream_type,
            elementary_pid: elementary_pid & 0x1FFF,
            descriptors: Vec::new(),
        }
    }

    /// Create from codec type
    pub fn from_codec(codec: CodecType, pid: u16) -> Option<Self> {
        StreamType::from_codec(codec).map(|st| Self::new(st, pid))
    }

    /// Add a descriptor
    pub fn with_descriptor(mut self, descriptor: Vec<u8>) -> Self {
        self.descriptors = descriptor;
        self
    }

    /// Calculate the byte length of this stream info entry
    pub fn byte_len(&self) -> usize {
        // stream_type (1) + reserved + elementary_PID (3) + reserved + ES_info_length (2)
        // + descriptors
        5 + self.descriptors.len()
    }
}

/// PMT Generator
#[derive(Debug, Clone)]
pub struct PmtGenerator {
    /// Program number this PMT describes
    program_number: u16,

    /// PID for this PMT
    pmt_pid: u16,

    /// PID of the stream containing PCR
    pcr_pid: u16,

    /// Version number
    version_number: u8,

    /// Streams in this program
    streams: Vec<StreamInfo>,

    /// Program descriptors
    program_descriptors: Vec<u8>,
}

impl PmtGenerator {
    /// Create a new PMT generator
    pub fn new(program_number: u16, pmt_pid: u16) -> Self {
        Self {
            program_number,
            pmt_pid: pmt_pid & 0x1FFF,
            pcr_pid: 0x1FFF, // Default: no PCR
            version_number: 0,
            streams: Vec::new(),
            program_descriptors: Vec::new(),
        }
    }

    /// Set PCR PID
    pub fn with_pcr_pid(mut self, pid: u16) -> Self {
        self.pcr_pid = pid & 0x1FFF;
        self
    }

    /// Set version number
    pub fn with_version(mut self, version: u8) -> Self {
        self.version_number = version & 0x1F;
        self
    }

    /// Add a stream
    pub fn add_stream(&mut self, stream: StreamInfo) {
        // Remove existing stream with same PID
        self.streams.retain(|s| s.elementary_pid != stream.elementary_pid);
        self.streams.push(stream);
    }

    /// Add a video stream
    pub fn add_video_stream(&mut self, codec: CodecType, pid: u16) {
        if let Some(stream) = StreamInfo::from_codec(codec, pid) {
            self.add_stream(stream);
        }
    }

    /// Add an audio stream
    pub fn add_audio_stream(&mut self, codec: CodecType, pid: u16) {
        if let Some(stream) = StreamInfo::from_codec(codec, pid) {
            self.add_stream(stream);
        }
    }

    /// Remove a stream by PID
    pub fn remove_stream(&mut self, pid: u16) {
        self.streams.retain(|s| s.elementary_pid != pid);
    }

    /// Clear all streams
    pub fn clear(&mut self) {
        self.streams.clear();
    }

    /// Set program descriptors
    pub fn set_program_descriptors(&mut self, descriptors: Vec<u8>) {
        self.program_descriptors = descriptors;
    }

    /// Increment version number
    pub fn increment_version(&mut self) {
        self.version_number = (self.version_number + 1) & 0x1F;
    }

    /// Calculate the total section length
    fn section_length(&self) -> usize {
        // Program info: 9 bytes (after section_length)
        // program_info_length: 2 bytes in header, but counted as data

        // Header after section_length: 9 bytes
        // program_number (2) + reserved/version/etc (3) + section_number (1) +
        // last_section_number (1) + reserved/PCR_PID (3) + reserved/program_info_length (2)
        // = 12 bytes actually

        // Let's calculate properly:
        // After section_length field:
        // program_number (2) + reserved/version/etc (3) + section_number (1) +
        // last_section_number (1) + reserved/PCR_PID (2) + reserved/program_info_length (2)
        // = 11 bytes

        // Plus: program_descriptors + streams + CRC

        let streams_len: usize = self.streams.iter().map(|s| s.byte_len()).sum();

        9 + self.program_descriptors.len() + streams_len + 4 // CRC
    }

    /// Generate PMT section bytes
    pub fn generate_section(&self) -> Vec<u8> {
        let section_length = self.section_length();

        // Total size is section_length + 3 (table_id through section_length)
        let mut section = Vec::with_capacity(3 + section_length);

        // table_id
        section.push(0x02);

        // section_syntax_indicator (1) + '0' (1) + reserved (2) + section_length high 4 bits
        let section_len_high = ((section_length >> 8) & 0x0F) as u8;
        section.push(0xB0 | section_len_high);

        // section_length low 8 bits
        section.push((section_length & 0xFF) as u8);

        // program_number (2 bytes)
        section.push((self.program_number >> 8) as u8);
        section.push((self.program_number & 0xFF) as u8);

        // reserved (2) + version_number (5) + current_next_indicator (1)
        let version_byte = 0xC0 | ((self.version_number & 0x1F) << 1) | 0x01;
        section.push(version_byte);

        // section_number
        section.push(0x00);

        // last_section_number
        section.push(0x00);

        // reserved (3) + PCR_PID (13)
        let pcr_high = ((self.pcr_pid >> 8) & 0x1F) as u8;
        section.push(0xE0 | pcr_high);
        section.push((self.pcr_pid & 0xFF) as u8);

        // reserved (4) + program_info_length (12)
        let prog_info_len = self.program_descriptors.len();
        section.push(0xF0 | ((prog_info_len >> 8) & 0x0F) as u8);
        section.push((prog_info_len & 0xFF) as u8);

        // Program descriptors
        if !self.program_descriptors.is_empty() {
            section.extend_from_slice(&self.program_descriptors);
        }

        // Stream info loop
        for stream in &self.streams {
            // stream_type
            section.push(stream.stream_type.as_byte());

            // reserved (3) + elementary_PID (13)
            let pid_high = ((stream.elementary_pid >> 8) & 0x1F) as u8;
            section.push(0xE0 | pid_high);
            section.push((stream.elementary_pid & 0xFF) as u8);

            // reserved (4) + ES_info_length (12)
            let es_info_len = stream.descriptors.len();
            section.push(0xF0 | ((es_info_len >> 8) & 0x0F) as u8);
            section.push((es_info_len & 0xFF) as u8);

            // ES descriptors
            if !stream.descriptors.is_empty() {
                section.extend_from_slice(&stream.descriptors);
            }
        }

        // Calculate and append CRC32
        let crc = calculate_crc32(&section);
        section.push(((crc >> 24) & 0xFF) as u8);
        section.push(((crc >> 16) & 0xFF) as u8);
        section.push(((crc >> 8) & 0xFF) as u8);
        section.push((crc & 0xFF) as u8);

        section
    }

    /// Generate PMT as TS packets
    pub fn generate_ts_packets(&self, cc: &mut ContinuityCounter) -> Vec<TsPacket> {
        let section = self.generate_section();
        let mut packets = Vec::new();

        let mut remaining = section.as_slice();
        let mut first_packet = true;

        while !remaining.is_empty() {
            let payload_capacity = TS_PACKET_SIZE - 4;

            let payload = if first_packet {
                let mut payload = Vec::with_capacity(payload_capacity);
                payload.push(0x00); // pointer_field

                let available = payload_capacity - 1;
                let take = remaining.len().min(available);
                payload.extend_from_slice(&remaining[..take]);
                remaining = &remaining[take..];

                first_packet = false;
                payload
            } else {
                let take = remaining.len().min(payload_capacity);
                let payload = remaining[..take].to_vec();
                remaining = &remaining[take..];
                payload
            };

            let header = TsPacketHeader::new(self.pmt_pid)
                .with_pusi(first_packet || packets.is_empty())
                .with_cc(cc.next(self.pmt_pid));

            let mut packet = TsPacket::new(self.pmt_pid);
            packet.header = header;
            packet.payload = payload;

            // Pad to 188 bytes
            let remaining_space = TS_PACKET_SIZE - 4 - packet.payload.len();
            if remaining_space > 0 {
                packet.payload.extend(std::iter::repeat(0xFF).take(remaining_space));
            }

            packets.push(packet);
        }

        packets
    }

    /// Generate PMT as raw bytes
    pub fn generate(&self, cc: &mut ContinuityCounter) -> Vec<u8> {
        let packets = self.generate_ts_packets(cc);
        let mut output = Vec::with_capacity(packets.len() * TS_PACKET_SIZE);

        for packet in packets {
            if let Ok(encoded) = packet.encode() {
                output.extend_from_slice(&encoded);
            }
        }

        output
    }

    /// Get the PMT PID
    pub fn pid(&self) -> u16 {
        self.pmt_pid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_type_from_codec() {
        assert_eq!(StreamType::from_codec(CodecType::H264), Some(StreamType::H264));
        assert_eq!(StreamType::from_codec(CodecType::H265), Some(StreamType::H265));
        assert_eq!(StreamType::from_codec(CodecType::AAC), Some(StreamType::Aac));
        assert_eq!(StreamType::from_codec(CodecType::Mp3), Some(StreamType::Mpeg1Audio));
    }

    #[test]
    fn test_pmt_section_basic() {
        let pmt = PmtGenerator::new(1, 0x1000)
            .with_pcr_pid(0x100);

        let section = pmt.generate_section();

        // table_id should be 0x02
        assert_eq!(section[0], 0x02);

        // section_syntax_indicator should be set
        assert_eq!(section[1] & 0xF0, 0xB0);

        // program_number at bytes 3-4
        assert_eq!(section[3], 0x00);
        assert_eq!(section[4], 0x01);

        // CRC at end should not be zero
        let crc_start = section.len() - 4;
        let crc = &section[crc_start..];
        assert_ne!(crc, &[0, 0, 0, 0]);
    }

    #[test]
    fn test_pmt_with_streams() {
        let mut pmt = PmtGenerator::new(1, 0x1000)
            .with_pcr_pid(0x100);

        pmt.add_video_stream(CodecType::H264, 0x100);
        pmt.add_audio_stream(CodecType::AAC, 0x101);

        let section = pmt.generate_section();

        // Should have 2 streams
        // Each stream: stream_type (1) + PID (2) + ES_info_length (2) = 5 bytes minimum

        // Check PCR_PID at bytes 8-9
        let pcr_pid = ((section[8] as u16 & 0x1F) << 8) | (section[9] as u16);
        assert_eq!(pcr_pid, 0x100);

        // Verify CRC
        let data_for_crc = &section[0..section.len() - 4];
        let stored_crc = u32::from_be_bytes([
            section[section.len() - 4],
            section[section.len() - 3],
            section[section.len() - 2],
            section[section.len() - 1],
        ]);
        let calculated_crc = calculate_crc32(data_for_crc);
        assert_eq!(calculated_crc, stored_crc);
    }

    #[test]
    fn test_pmt_video_stream() {
        let mut pmt = PmtGenerator::new(1, 0x1000);
        pmt.add_video_stream(CodecType::H264, 0x100);

        let section = pmt.generate_section();

        // After header (12 bytes), streams start
        // program_info_length is at bytes 10-11, should be 0
        assert_eq!(section[10], 0xF0);
        assert_eq!(section[11], 0x00);

        // First stream at byte 12:
        // stream_type = 0x1B (H.264)
        assert_eq!(section[12], 0x1B);

        // elementary_PID at bytes 13-14
        let pid = ((section[13] as u16 & 0x1F) << 8) | (section[14] as u16);
        assert_eq!(pid, 0x100);
    }

    #[test]
    fn test_pmt_audio_stream() {
        let mut pmt = PmtGenerator::new(1, 0x1000);
        pmt.add_audio_stream(CodecType::AAC, 0x101);

        let section = pmt.generate_section();

        // stream_type should be 0x0F (AAC) at byte 12
        assert_eq!(section[12], 0x0F);

        // PID should be 0x101 at bytes 13-14
        let pid = ((section[13] as u16 & 0x1F) << 8) | (section[14] as u16);
        assert_eq!(pid, 0x101);
    }

    #[test]
    fn test_pmt_ts_packets() {
        let mut pmt = PmtGenerator::new(1, 0x1000)
            .with_pcr_pid(0x100);

        pmt.add_video_stream(CodecType::H264, 0x100);
        pmt.add_audio_stream(CodecType::AAC, 0x101);

        let mut cc = ContinuityCounter::new();
        let packets = pmt.generate_ts_packets(&mut cc);

        assert!(!packets.is_empty());

        for packet in &packets {
            let encoded = packet.encode().unwrap();
            assert_eq!(encoded.len(), TS_PACKET_SIZE);
            assert_eq!(encoded[0], 0x47); // TS_SYNC_BYTE

            // PID should be 0x1000 (PMT PID)
            let pid = ((encoded[1] as u16 & 0x1F) << 8) | (encoded[2] as u16);
            assert_eq!(pid, 0x1000);
        }
    }

    #[test]
    fn test_pmt_version() {
        let pmt = PmtGenerator::new(1, 0x1000)
            .with_version(5);

        let section = pmt.generate_section();

        // version is in byte 5, bits 1-5
        // 0xC0 | (version << 1) | 0x01
        // version 5: 0xC0 | 0x0A | 0x01 = 0xCB
        assert_eq!(section[5], 0xCB);
    }

    #[test]
    fn test_pmt_with_descriptors() {
        let stream = StreamInfo::new(StreamType::H264, 0x100)
            .with_descriptor(vec![0x05, 0x04, 0x41, 0x42, 0x43, 0x44]); // Registration descriptor

        let mut pmt = PmtGenerator::new(1, 0x1000);
        pmt.add_stream(stream);

        let section = pmt.generate_section();

        // Stream starts at byte 12
        // stream_type (12) + PID (13-14) + ES_info_length (15-16)
        // ES_info_length should be 6
        assert_eq!(section[15], 0xF0);
        assert_eq!(section[16], 0x06);

        // Descriptor should follow at byte 17
        assert_eq!(section[17], 0x05); // tag
        assert_eq!(section[18], 0x04); // length
    }

    #[test]
    fn test_stream_info_byte_len() {
        let stream = StreamInfo::new(StreamType::H264, 0x100);
        assert_eq!(stream.byte_len(), 5); // No descriptors

        let stream_with_desc = StreamInfo::new(StreamType::H264, 0x100)
            .with_descriptor(vec![1, 2, 3, 4, 5]);
        assert_eq!(stream_with_desc.byte_len(), 10); // 5 + 5 descriptors
    }

    #[test]
    fn test_remove_stream() {
        let mut pmt = PmtGenerator::new(1, 0x1000);
        pmt.add_video_stream(CodecType::H264, 0x100);
        pmt.add_audio_stream(CodecType::AAC, 0x101);

        assert_eq!(pmt.streams.len(), 2);

        pmt.remove_stream(0x101);
        assert_eq!(pmt.streams.len(), 1);
        assert_eq!(pmt.streams[0].elementary_pid, 0x100);
    }
}
