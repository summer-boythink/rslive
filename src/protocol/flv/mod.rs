//! FLV (Flash Video) format implementation
//!
//! FLV is a container format used for streaming audio and video over the internet.
//! It's commonly used with RTMP and HTTP-FLV streaming.
//!
//! This module provides:
//! - FLV muxer (encoder) for creating FLV streams
//! - FLV demuxer (decoder) for parsing FLV streams
//! - HTTP-FLV server for streaming over HTTP

use crate::media::frame::{AudioFrameType, VideoFrameType as MediaVideoFrameType};
use crate::media::{CodecType, FrameType, MediaFrame};
use crate::protocol::common::{
    AacPacketType, AvcPacketType, SoundFormat, SoundRate, TagType, VideoCodecId, VideoFrameType,
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::Cursor;

pub mod decoder;
pub mod encoder;
pub mod http_server;
pub mod writer;

pub use decoder::FlvDecoder;
pub use encoder::FlvEncoder;
pub use http_server::{HttpFlvConfig, HttpFlvServer};
pub use writer::FlvWriter;

/// FLV header magic bytes
pub const FLV_HEADER_MAGIC: &[u8] = b"FLV";

/// FLV version
pub const FLV_VERSION: u8 = 1;

/// FLV header size (9 bytes)
pub const FLV_HEADER_SIZE: usize = 9;

/// Previous tag size (4 bytes)
pub const PREVIOUS_TAG_SIZE: usize = 4;

/// FLV tag header size (11 bytes for video/audio, variable for script)
pub const FLV_TAG_HEADER_SIZE: usize = 11;

/// FLV header flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlvHeaderFlags {
    pub has_video: bool,
    pub has_audio: bool,
}

impl FlvHeaderFlags {
    pub fn new(has_video: bool, has_audio: bool) -> Self {
        Self {
            has_video,
            has_audio,
        }
    }

    pub fn to_u8(&self) -> u8 {
        let mut flags = 0u8;
        if self.has_audio {
            flags |= 0x04;
        }
        if self.has_video {
            flags |= 0x01;
        }
        flags
    }

    pub fn from_u8(flags: u8) -> Self {
        Self {
            has_audio: (flags & 0x04) != 0,
            has_video: (flags & 0x01) != 0,
        }
    }
}

/// FLV header
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlvHeader {
    pub version: u8,
    pub flags: FlvHeaderFlags,
    pub header_size: u32,
}

impl FlvHeader {
    pub fn new(has_video: bool, has_audio: bool) -> Self {
        Self {
            version: FLV_VERSION,
            flags: FlvHeaderFlags::new(has_video, has_audio),
            header_size: FLV_HEADER_SIZE as u32,
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(FLV_HEADER_SIZE + PREVIOUS_TAG_SIZE);

        // Magic
        buf.extend_from_slice(FLV_HEADER_MAGIC);

        // Version
        buf.put_u8(self.version);

        // Flags
        buf.put_u8(self.flags.to_u8());

        // Header size (big-endian)
        buf.put_u32(self.header_size);

        // Previous tag size (0 for first tag)
        buf.put_u32(0);

        buf.freeze()
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < FLV_HEADER_SIZE {
            return None;
        }

        let mut cursor = Cursor::new(data);

        // Check magic
        let mut magic = [0u8; 3];
        cursor.copy_to_slice(&mut magic);
        if &magic != FLV_HEADER_MAGIC {
            return None;
        }

        let version = cursor.get_u8();
        let flags = FlvHeaderFlags::from_u8(cursor.get_u8());
        let header_size = cursor.get_u32();

        Some(Self {
            version,
            flags,
            header_size,
        })
    }
}

/// FLV tag header
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlvTagHeader {
    pub tag_type: TagType,
    pub data_size: u32,
    pub timestamp: u32,
    pub stream_id: u32,
}

impl FlvTagHeader {
    pub fn new(tag_type: TagType, data_size: u32, timestamp: u32) -> Self {
        Self {
            tag_type,
            data_size,
            timestamp,
            stream_id: 0, // Always 0
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(FLV_TAG_HEADER_SIZE);

        // Tag type (1 byte)
        buf.put_u8(self.tag_type.as_u8());

        // Data size (3 bytes, big-endian)
        buf.put_u8(((self.data_size >> 16) & 0xFF) as u8);
        buf.put_u8(((self.data_size >> 8) & 0xFF) as u8);
        buf.put_u8((self.data_size & 0xFF) as u8);

        // Timestamp (3 bytes + 1 extended byte)
        buf.put_u8(((self.timestamp >> 16) & 0xFF) as u8);
        buf.put_u8(((self.timestamp >> 8) & 0xFF) as u8);
        buf.put_u8((self.timestamp & 0xFF) as u8);
        buf.put_u8(((self.timestamp >> 24) & 0xFF) as u8);

        // Stream ID (3 bytes, always 0)
        buf.put_u8(0);
        buf.put_u8(0);
        buf.put_u8(0);

        buf.freeze()
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < FLV_TAG_HEADER_SIZE {
            return None;
        }

        let mut cursor = Cursor::new(data);

        let tag_type = TagType::from_u8(cursor.get_u8())?;

        let data_size = ((cursor.get_u8() as u32) << 16)
            | ((cursor.get_u8() as u32) << 8)
            | (cursor.get_u8() as u32);

        let timestamp_low = ((cursor.get_u8() as u32) << 16)
            | ((cursor.get_u8() as u32) << 8)
            | (cursor.get_u8() as u32);
        let timestamp_extended = cursor.get_u8() as u32;
        let timestamp = (timestamp_extended << 24) | timestamp_low;

        // Stream ID (always 0)
        let _ = cursor.get_u8();
        let _ = cursor.get_u8();
        let _ = cursor.get_u8();

        Some(Self {
            tag_type,
            data_size,
            timestamp,
            stream_id: 0,
        })
    }
}

/// Audio tag header (first byte of audio data)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTagHeader {
    pub sound_format: SoundFormat,
    pub sound_rate: SoundRate,
    pub sound_size: u8,                         // 0 = 8-bit, 1 = 16-bit
    pub sound_type: u8,                         // 0 = mono, 1 = stereo
    pub aac_packet_type: Option<AacPacketType>, // Only for AAC
}

impl AudioTagHeader {
    pub fn new_aac(packet_type: AacPacketType) -> Self {
        Self {
            sound_format: SoundFormat::Aac,
            sound_rate: SoundRate::KHz44, // AAC always uses 44kHz in FLV
            sound_size: 1,                // AAC always uses 16-bit
            sound_type: 1,                // AAC always uses stereo
            aac_packet_type: Some(packet_type),
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(2);

        let byte1 = ((self.sound_format as u8) << 4)
            | ((self.sound_rate as u8) << 2)
            | ((self.sound_size & 0x01) << 1)
            | (self.sound_type & 0x01);
        buf.put_u8(byte1);

        // AAC packet type (only for AAC)
        if let Some(packet_type) = self.aac_packet_type {
            buf.put_u8(packet_type as u8);
        }

        buf.freeze()
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let byte1 = data[0];
        let sound_format = SoundFormat::from_u8((byte1 >> 4) & 0x0F)?;
        let sound_rate = SoundRate::from_u8((byte1 >> 2) & 0x03)?;
        let sound_size = (byte1 >> 1) & 0x01;
        let sound_type = byte1 & 0x01;

        let aac_packet_type = if sound_format == SoundFormat::Aac && data.len() > 1 {
            AacPacketType::from_u8(data[1])
        } else {
            None
        };

        Some(Self {
            sound_format,
            sound_rate,
            sound_size,
            sound_type,
            aac_packet_type,
        })
    }

    pub fn is_sequence_header(&self) -> bool {
        self.aac_packet_type == Some(AacPacketType::SequenceHeader)
    }
}

/// Video tag header (first bytes of video data)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoTagHeader {
    pub frame_type: VideoFrameType,
    pub codec_id: VideoCodecId,
    pub avc_packet_type: Option<AvcPacketType>, // Only for AVC
    pub composition_time: i32,                  // Only for AVC (CTS)
}

impl VideoTagHeader {
    pub fn new_avc_keyframe() -> Self {
        Self {
            frame_type: VideoFrameType::Keyframe,
            codec_id: VideoCodecId::Avc,
            avc_packet_type: Some(AvcPacketType::Nalu),
            composition_time: 0,
        }
    }

    pub fn new_avc_interframe() -> Self {
        Self {
            frame_type: VideoFrameType::Interframe,
            codec_id: VideoCodecId::Avc,
            avc_packet_type: Some(AvcPacketType::Nalu),
            composition_time: 0,
        }
    }

    pub fn new_avc_sequence_header() -> Self {
        Self {
            frame_type: VideoFrameType::Keyframe,
            codec_id: VideoCodecId::Avc,
            avc_packet_type: Some(AvcPacketType::SequenceHeader),
            composition_time: 0,
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(5);

        let byte1 = ((self.frame_type as u8) << 4) | (self.codec_id as u8);
        buf.put_u8(byte1);

        // AVC packet type and composition time (only for AVC)
        if let Some(packet_type) = self.avc_packet_type {
            buf.put_u8(packet_type as u8);

            // Composition time (SI24, signed)
            let cts = self.composition_time;
            buf.put_u8(((cts >> 16) & 0xFF) as u8);
            buf.put_u8(((cts >> 8) & 0xFF) as u8);
            buf.put_u8((cts & 0xFF) as u8);
        }

        buf.freeze()
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let byte1 = data[0];
        let frame_type = VideoFrameType::from_u8((byte1 >> 4) & 0x0F)?;
        let codec_id = VideoCodecId::from_u8(byte1 & 0x0F)?;

        let (avc_packet_type, composition_time) =
            if codec_id == VideoCodecId::Avc && data.len() >= 5 {
                let packet_type = AvcPacketType::from_u8(data[1])?;
                let cts = ((data[2] as i32) << 16) | ((data[3] as i32) << 8) | (data[4] as i32);
                // Sign extend if negative
                let cts = if cts & 0x800000 != 0 {
                    cts | !0xFFFFFF
                } else {
                    cts
                };
                (Some(packet_type), cts)
            } else {
                (None, 0)
            };

        Some(Self {
            frame_type,
            codec_id,
            avc_packet_type,
            composition_time,
        })
    }

    pub fn is_keyframe(&self) -> bool {
        self.frame_type.is_keyframe()
    }

    pub fn is_sequence_header(&self) -> bool {
        self.avc_packet_type == Some(AvcPacketType::SequenceHeader)
    }
}

/// FLV error types
#[derive(Debug, thiserror::Error)]
pub enum FlvError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid FLV data: {0}")]
    InvalidData(String),

    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),

    #[error("Incomplete data: expected {expected}, got {got}")]
    IncompleteData { expected: usize, got: usize },

    #[error("End of stream")]
    EndOfStream,
}

pub type FlvResult<T> = Result<T, FlvError>;

/// Convert MediaFrame to FLV video tag data
pub fn video_frame_to_flv(frame: &MediaFrame) -> FlvResult<Bytes> {
    if !frame.is_video() {
        return Err(FlvError::InvalidData("Not a video frame".into()));
    }

    let frame_type = match frame.frame_type {
        FrameType::Video(vt) => vt,
        _ => return Err(FlvError::InvalidData("Not a video frame".into())),
    };

    let flv_frame_type = match frame_type {
        MediaVideoFrameType::Keyframe => VideoFrameType::Keyframe,
        MediaVideoFrameType::Interframe => VideoFrameType::Interframe,
        MediaVideoFrameType::DisposableInterframe => VideoFrameType::DisposableInterframe,
        _ => VideoFrameType::Interframe,
    };

    let codec_id = match frame.codec {
        CodecType::H264 => VideoCodecId::Avc,
        CodecType::H265 => VideoCodecId::Hevc,
        _ => {
            return Err(FlvError::UnsupportedCodec(format!("{:?}", frame.codec)));
        }
    };

    // Determine if this is a sequence header
    let is_sequence_header = frame.data.len() > 0 && (frame.data[0] == 0);

    let avc_packet_type = if is_sequence_header {
        AvcPacketType::SequenceHeader
    } else {
        AvcPacketType::Nalu
    };

    let video_header = VideoTagHeader {
        frame_type: flv_frame_type,
        codec_id,
        avc_packet_type: Some(avc_packet_type),
        composition_time: frame.composition_time() as i32,
    };

    let mut data = BytesMut::new();
    data.extend_from_slice(&video_header.encode());
    data.extend_from_slice(&frame.data);

    Ok(data.freeze())
}

/// Convert MediaFrame to FLV audio tag data
pub fn audio_frame_to_flv(frame: &MediaFrame) -> FlvResult<Bytes> {
    if !frame.is_audio() {
        return Err(FlvError::InvalidData("Not an audio frame".into()));
    }

    let frame_type = match frame.frame_type {
        FrameType::Audio(at) => at,
        _ => return Err(FlvError::InvalidData("Not an audio frame".into())),
    };

    let sound_format = match frame.codec {
        CodecType::AAC => SoundFormat::Aac,
        CodecType::Mp3 => SoundFormat::Mp3,
        CodecType::G711A => SoundFormat::G711ALaw,
        CodecType::G711U => SoundFormat::G711MuLaw,
        _ => {
            return Err(FlvError::UnsupportedCodec(format!("{:?}", frame.codec)));
        }
    };

    let aac_packet_type = if frame.codec == CodecType::AAC {
        Some(if frame_type == AudioFrameType::SequenceHeader {
            AacPacketType::SequenceHeader
        } else {
            AacPacketType::Raw
        })
    } else {
        None
    };

    let audio_header = AudioTagHeader {
        sound_format,
        sound_rate: SoundRate::KHz44,
        sound_size: 1,
        sound_type: 1,
        aac_packet_type,
    };

    let mut data = BytesMut::new();
    data.extend_from_slice(&audio_header.encode());
    data.extend_from_slice(&frame.data);

    Ok(data.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flv_header() {
        let header = FlvHeader::new(true, true);
        let encoded = header.encode();
        assert_eq!(encoded.len(), FLV_HEADER_SIZE + PREVIOUS_TAG_SIZE);

        let decoded = FlvHeader::decode(&encoded[..FLV_HEADER_SIZE]).unwrap();
        assert_eq!(decoded.version, 1);
        assert!(decoded.flags.has_video);
        assert!(decoded.flags.has_audio);
    }

    #[test]
    fn test_flv_tag_header() {
        let header = FlvTagHeader::new(TagType::Video, 1024, 1000);
        let encoded = header.encode();
        assert_eq!(encoded.len(), FLV_TAG_HEADER_SIZE);

        let decoded = FlvTagHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.tag_type, TagType::Video);
        assert_eq!(decoded.data_size, 1024);
        assert_eq!(decoded.timestamp, 1000);
    }

    #[test]
    fn test_video_tag_header() {
        let header = VideoTagHeader::new_avc_keyframe();
        let encoded = header.encode();
        assert_eq!(encoded.len(), 5);

        let decoded = VideoTagHeader::decode(&encoded).unwrap();
        assert!(decoded.is_keyframe());
        assert_eq!(decoded.codec_id, VideoCodecId::Avc);
    }

    #[test]
    fn test_audio_tag_header() {
        let header = AudioTagHeader::new_aac(AacPacketType::SequenceHeader);
        let encoded = header.encode();
        assert_eq!(encoded.len(), 2);

        let decoded = AudioTagHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.sound_format, SoundFormat::Aac);
        assert!(decoded.is_sequence_header());
    }
}
