//! Common types and utilities shared across protocols

use crate::media::CodecType;

/// Tag types for FLV and similar formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    Audio = 8,
    Video = 9,
    Script = 18,
}

impl TagType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            8 => Some(Self::Audio),
            9 => Some(Self::Video),
            18 => Some(Self::Script),
            _ => None,
        }
    }

    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
}

/// AVC packet types for H.264 in FLV/RTMP
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvcPacketType {
    SequenceHeader = 0,
    Nalu = 1,
    EndOfSequence = 2,
}

impl AvcPacketType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::SequenceHeader),
            1 => Some(Self::Nalu),
            2 => Some(Self::EndOfSequence),
            _ => None,
        }
    }
}

/// AAC packet types for AAC in FLV/RTMP
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AacPacketType {
    SequenceHeader = 0,
    Raw = 1,
}

impl AacPacketType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::SequenceHeader),
            1 => Some(Self::Raw),
            _ => None,
        }
    }
}

/// Audio codec IDs used in FLV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundFormat {
    LinearPcmPlatformEndian = 0,
    AdPcm = 1,
    Mp3 = 2,
    LinearPcmLittleEndian = 3,
    Nellymoser16kHzMono = 4,
    Nellymoser8kHzMono = 5,
    Nellymoser = 6,
    G711ALaw = 7,
    G711MuLaw = 8,
    Reserved = 9,
    Aac = 10,
    Speex = 11,
    Mp38kHz = 14,
    DeviceSpecific = 15,
}

impl SoundFormat {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::LinearPcmPlatformEndian),
            1 => Some(Self::AdPcm),
            2 => Some(Self::Mp3),
            3 => Some(Self::LinearPcmLittleEndian),
            4 => Some(Self::Nellymoser16kHzMono),
            5 => Some(Self::Nellymoser8kHzMono),
            6 => Some(Self::Nellymoser),
            7 => Some(Self::G711ALaw),
            8 => Some(Self::G711MuLaw),
            9 => Some(Self::Reserved),
            10 => Some(Self::Aac),
            11 => Some(Self::Speex),
            14 => Some(Self::Mp38kHz),
            15 => Some(Self::DeviceSpecific),
            _ => None,
        }
    }

    pub fn to_codec_type(&self) -> Option<CodecType> {
        match self {
            Self::Aac => Some(CodecType::AAC),
            Self::Mp3 | Self::Mp38kHz => Some(CodecType::Mp3),
            Self::G711ALaw => Some(CodecType::G711A),
            Self::G711MuLaw => Some(CodecType::G711U),
            _ => None,
        }
    }
}

/// Sound rate for audio in FLV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundRate {
    KHz5_5 = 0,
    KHz11 = 1,
    KHz22 = 2,
    KHz44 = 3,
}

impl SoundRate {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::KHz5_5),
            1 => Some(Self::KHz11),
            2 => Some(Self::KHz22),
            3 => Some(Self::KHz44),
            _ => None,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            Self::KHz5_5 => 5500,
            Self::KHz11 => 11025,
            Self::KHz22 => 22050,
            Self::KHz44 => 44100,
        }
    }
}

/// Video codec IDs used in FLV
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodecId {
    Jpeg = 1,
    SorensonH263 = 2,
    ScreenVideo = 3,
    On2Vp6 = 4,
    On2Vp6Alpha = 5,
    ScreenVideoV2 = 6,
    Avc = 7,
    Hevc = 12,
}

impl VideoCodecId {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Jpeg),
            2 => Some(Self::SorensonH263),
            3 => Some(Self::ScreenVideo),
            4 => Some(Self::On2Vp6),
            5 => Some(Self::On2Vp6Alpha),
            6 => Some(Self::ScreenVideoV2),
            7 => Some(Self::Avc),
            12 => Some(Self::Hevc),
            _ => None,
        }
    }

    pub fn to_codec_type(&self) -> Option<CodecType> {
        match self {
            Self::Avc => Some(CodecType::H264),
            Self::Hevc => Some(CodecType::H265),
            _ => None,
        }
    }
}

/// Frame type in video tag
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameType {
    Keyframe = 1,
    Interframe = 2,
    DisposableInterframe = 3,
    GeneratedKeyframe = 4,
    VideoInfoFrame = 5,
}

impl VideoFrameType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Keyframe),
            2 => Some(Self::Interframe),
            3 => Some(Self::DisposableInterframe),
            4 => Some(Self::GeneratedKeyframe),
            5 => Some(Self::VideoInfoFrame),
            _ => None,
        }
    }

    pub fn is_keyframe(&self) -> bool {
        matches!(self, Self::Keyframe | Self::GeneratedKeyframe)
    }
}

/// Utility functions for protocol handling
pub mod utils {
    use bytes::{Buf, BufMut};

    /// Read a 24-bit big-endian integer
    pub fn read_u24(buf: &mut impl Buf) -> u32 {
        let b = [buf.get_u8(), buf.get_u8(), buf.get_u8()];
        ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32)
    }

    /// Write a 24-bit big-endian integer
    pub fn write_u24(buf: &mut impl BufMut, value: u32) {
        buf.put_u8(((value >> 16) & 0xFF) as u8);
        buf.put_u8(((value >> 8) & 0xFF) as u8);
        buf.put_u8((value & 0xFF) as u8);
    }

    /// Convert milliseconds to FLV timestamp format (24-bit + extended)
    pub fn write_flv_timestamp(buf: &mut impl BufMut, timestamp: u32) {
        let extended = (timestamp >> 24) as u8;
        write_u24(buf, timestamp & 0xFFFFFF);
        buf.put_u8(extended);
    }

    /// Read FLV timestamp format
    pub fn read_flv_timestamp(buf: &mut impl Buf) -> u32 {
        let low = read_u24(buf);
        let extended = buf.get_u8() as u32;
        (extended << 24) | low
    }
}

#[cfg(test)]
mod tests {
    use super::utils::*;
    use bytes::BytesMut;

    #[test]
    fn test_u24_rw() {
        let mut buf = BytesMut::new();
        write_u24(&mut buf, 0x123456);
        assert_eq!(read_u24(&mut buf), 0x123456);
    }

    #[test]
    fn test_flv_timestamp() {
        let mut buf = BytesMut::new();
        write_flv_timestamp(&mut buf, 0x12345678);
        assert_eq!(read_flv_timestamp(&mut buf), 0x12345678);
    }
}
