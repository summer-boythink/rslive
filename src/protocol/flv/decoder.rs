//! FLV decoder (demuxer) for parsing FLV streams into MediaFrames

use super::{
    AudioTagHeader, FlvError, FlvHeader, FlvResult, FlvTagHeader, TagType, VideoTagHeader,
};
use crate::media::{CodecType, FrameType, MediaFrame, Timestamp};
use crate::media::frame::{AudioFrameType, VideoFrameType};
use crate::protocol::common::{AacPacketType, AvcPacketType, SoundFormat, VideoCodecId};
use bytes::{Buf, Bytes, BytesMut};
use std::io::Cursor;

/// FLV decoder for demuxing FLV streams into MediaFrames
pub struct FlvDecoder {
    header_parsed: bool,
    has_video: bool,
    has_audio: bool,
    buffer: BytesMut,
    /// Minimum bytes needed for next parse operation
    needed: usize,
    /// Sequence headers received
    video_sequence_header: Option<Bytes>,
    audio_sequence_header: Option<Bytes>,
}

impl FlvDecoder {
    pub fn new() -> Self {
        Self {
            header_parsed: false,
            has_video: false,
            has_audio: false,
            buffer: BytesMut::new(),
            needed: super::FLV_HEADER_SIZE + super::PREVIOUS_TAG_SIZE,
            video_sequence_header: None,
            audio_sequence_header: None,
        }
    }

    /// Push data into decoder buffer
    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Try to parse next item (header or frame)
    ///
    /// Returns Ok(Some(frame)) if a frame was parsed
    /// Returns Ok(None) if more data is needed
    pub fn parse_next(&mut self) -> FlvResult<Option<MediaFrame>> {
        // Check if we have enough data
        if self.buffer.len() < self.needed {
            return Ok(None);
        }

        // Parse header if not done
        if !self.header_parsed {
            return self.parse_header();
        }

        // Parse tag
        self.parse_tag()
    }

    /// Parse FLV header
    fn parse_header(&mut self) -> FlvResult<Option<MediaFrame>> {
        if self.buffer.len() < super::FLV_HEADER_SIZE {
            self.needed = super::FLV_HEADER_SIZE;
            return Ok(None);
        }

        let header = FlvHeader::decode(&self.buffer).ok_or_else(|| {
            FlvError::InvalidData("Failed to parse FLV header".into())
        })?;

        self.has_video = header.flags.has_video;
        self.has_audio = header.flags.has_audio;
        self.header_parsed = true;

        // Consume header and previous tag size
        let consumed = super::FLV_HEADER_SIZE + super::PREVIOUS_TAG_SIZE;
        self.buffer.advance(consumed);

        // Next we need a tag header
        self.needed = super::FLV_TAG_HEADER_SIZE;

        // Header parsed, no frame returned yet
        Ok(None)
    }

    /// Parse FLV tag
    fn parse_tag(&mut self) -> FlvResult<Option<MediaFrame>> {
        if self.buffer.len() < super::FLV_TAG_HEADER_SIZE {
            self.needed = super::FLV_TAG_HEADER_SIZE;
            return Ok(None);
        }

        // Parse tag header
        let tag_header = FlvTagHeader::decode(&self.buffer).ok_or_else(|| {
            FlvError::InvalidData("Failed to parse tag header".into())
        })?;

        let total_size = super::FLV_TAG_HEADER_SIZE
            + tag_header.data_size as usize
            + super::PREVIOUS_TAG_SIZE;

        if self.buffer.len() < total_size {
            self.needed = total_size;
            return Ok(None);
        }

        // Skip tag header
        self.buffer.advance(super::FLV_TAG_HEADER_SIZE);

        // Extract tag data
        let data = self.buffer.split_to(tag_header.data_size as usize).freeze();

        // Skip previous tag size
        self.buffer.advance(super::PREVIOUS_TAG_SIZE);

        // Reset needed for next tag
        self.needed = super::FLV_TAG_HEADER_SIZE;

        // Parse tag data based on type
        match tag_header.tag_type {
            TagType::Video => self.parse_video_tag(tag_header.timestamp, data),
            TagType::Audio => self.parse_audio_tag(tag_header.timestamp, data),
            TagType::Script => {
                // Script data not yet supported as MediaFrame
                Ok(None)
            }
        }
    }

    /// Parse video tag data
    fn parse_video_tag(
        &mut self,
        timestamp: u32,
        data: Bytes,
    ) -> FlvResult<Option<MediaFrame>> {
        if data.is_empty() {
            return Ok(None);
        }

        let header = VideoTagHeader::decode(&data).ok_or_else(|| {
            FlvError::InvalidData("Failed to parse video tag header".into())
        })?;

        // Skip video header bytes
        let header_len = if header.avc_packet_type.is_some() { 5 } else { 1 };
        let frame_data = data.slice(header_len..);

        // Cache sequence header
        if header.is_sequence_header() {
            self.video_sequence_header = Some(frame_data.clone());
        }

        let codec = match header.codec_id {
            VideoCodecId::Avc => CodecType::H264,
            VideoCodecId::Hevc => CodecType::H265,
            _ => {
                return Err(FlvError::UnsupportedCodec(format!(
                    "Video codec {:?}",
                    header.codec_id
                )));
            }
        };

        let frame_type = match header.frame_type {
            crate::protocol::common::VideoFrameType::Keyframe => VideoFrameType::Keyframe,
            crate::protocol::common::VideoFrameType::Interframe => VideoFrameType::Interframe,
            crate::protocol::common::VideoFrameType::DisposableInterframe => {
                VideoFrameType::DisposableInterframe
            }
            crate::protocol::common::VideoFrameType::GeneratedKeyframe => {
                VideoFrameType::GeneratedKeyframe
            }
            _ => VideoFrameType::Interframe,
        };

        // Calculate composition timestamp
        let pts = Timestamp::from_millis(timestamp as u64);
        let dts = if header.composition_time >= 0 {
            pts - std::time::Duration::from_millis(header.composition_time as u64)
        } else {
            pts + std::time::Duration::from_millis((-header.composition_time) as u64)
        };

        let frame = MediaFrame::with_dts(
            1, // Video track
            pts,
            dts,
            FrameType::Video(frame_type),
            codec,
            frame_data,
        );

        Ok(Some(frame))
    }

    /// Parse audio tag data
    fn parse_audio_tag(
        &mut self,
        timestamp: u32,
        data: Bytes,
    ) -> FlvResult<Option<MediaFrame>> {
        if data.is_empty() {
            return Ok(None);
        }

        let header = AudioTagHeader::decode(&data).ok_or_else(|| {
            FlvError::InvalidData("Failed to parse audio tag header".into())
        })?;

        // Skip audio header bytes
        let header_len = if header.aac_packet_type.is_some() { 2 } else { 1 };
        let frame_data = data.slice(header_len..);

        // Cache sequence header
        if header.is_sequence_header() {
            self.audio_sequence_header = Some(frame_data.clone());
        }

        let codec = match header.sound_format {
            SoundFormat::Aac => CodecType::AAC,
            SoundFormat::Mp3 => CodecType::Mp3,
            SoundFormat::G711ALaw => CodecType::G711A,
            SoundFormat::G711MuLaw => CodecType::G711U,
            _ => {
                return Err(FlvError::UnsupportedCodec(format!(
                    "Audio codec {:?}",
                    header.sound_format
                )));
            }
        };

        let frame_type = if header.is_sequence_header() {
            AudioFrameType::SequenceHeader
        } else {
            AudioFrameType::Raw
        };

        let pts = Timestamp::from_millis(timestamp as u64);

        let frame = MediaFrame::new(
            2, // Audio track
            pts,
            FrameType::Audio(frame_type),
            codec,
            frame_data,
        );

        Ok(Some(frame))
    }

    /// Get available bytes in buffer
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if stream has video
    pub fn has_video(&self) -> bool {
        self.has_video
    }

    /// Check if stream has audio
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }

    /// Get cached video sequence header
    pub fn video_sequence_header(&self) -> Option<&Bytes> {
        self.video_sequence_header.as_ref()
    }

    /// Get cached audio sequence header
    pub fn audio_sequence_header(&self) -> Option<&Bytes> {
        self.audio_sequence_header.as_ref()
    }

    /// Clear decoder state
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.needed = super::FLV_TAG_HEADER_SIZE;
    }
}

impl Default for FlvDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Async FLV decoder that works with streams
pub struct FlvStreamDecoder {
    decoder: FlvDecoder,
}

impl FlvStreamDecoder {
    pub fn new() -> Self {
        Self {
            decoder: FlvDecoder::new(),
        }
    }

    /// Decode from byte stream
    ///
    /// This is a convenience method for parsing complete FLV data.
    pub fn decode(&mut self, data: &[u8]) -> FlvResult<Vec<MediaFrame>> {
        self.decoder.push(data);

        let mut frames = Vec::new();

        loop {
            match self.decoder.parse_next() {
                Ok(Some(frame)) => frames.push(frame),
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }

        Ok(frames)
    }

    /// Get inner decoder reference
    pub fn inner(&self) -> &FlvDecoder {
        &self.decoder
    }

    /// Get inner decoder mutable reference
    pub fn inner_mut(&mut self) -> &mut FlvDecoder {
        &mut self.decoder
    }
}

impl Default for FlvStreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::flv::encoder::FlvEncoder;

    #[test]
    fn test_flv_decode() {
        let mut encoder = FlvEncoder::video_only();

        // Create test frame
        let frame = MediaFrame::video(
            1,
            Timestamp::from_millis(1000),
            VideoFrameType::Keyframe,
            CodecType::H264,
            Bytes::from(vec![0x65, 0x88, 0x80, 0x00]), // IDR slice NAL
        );

        // Encode
        let header = encoder.header().unwrap();
        let tag = encoder.encode_frame(&frame).unwrap().unwrap();

        // Decode
        let mut decoder = FlvDecoder::new();
        decoder.push(&header);
        decoder.push(&tag);

        // Parse header
        let result = decoder.parse_next().unwrap();
        assert!(result.is_none()); // Header returns None

        // Parse frame
        let decoded = decoder.parse_next().unwrap().unwrap();

        assert!(decoded.is_video());
        assert!(decoded.is_keyframe());
        assert_eq!(decoded.pts.as_millis(), 1000);
        assert_eq!(decoded.codec, CodecType::H264);
    }

    #[test]
    fn test_flv_decode_audio() {
        let mut encoder = FlvEncoder::audio_only();

        let frame = MediaFrame::audio(
            2,
            Timestamp::from_millis(1000),
            AudioFrameType::Raw,
            CodecType::AAC,
            Bytes::from(vec![0x00, 0x01, 0x02, 0x03]),
        );

        let header = encoder.header().unwrap();
        let tag = encoder.encode_frame(&frame).unwrap().unwrap();

        let mut decoder = FlvDecoder::new();
        decoder.push(&header);
        decoder.push(&tag);

        decoder.parse_next().unwrap(); // Header
        let decoded = decoder.parse_next().unwrap().unwrap();

        assert!(decoded.is_audio());
        assert_eq!(decoded.pts.as_millis(), 1000);
        assert_eq!(decoded.codec, CodecType::AAC);
    }
}
