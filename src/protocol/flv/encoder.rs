//! FLV encoder (muxer) for converting MediaFrames to FLV format

use super::{
    FLV_TAG_HEADER_SIZE, FlvHeader, FlvResult, FlvTagHeader, PREVIOUS_TAG_SIZE, TagType,
    audio_frame_to_flv, video_frame_to_flv,
};
use crate::media::{FrameType, MediaFrame};
use bytes::{BufMut, Bytes, BytesMut};
use std::sync::atomic::{AtomicU32, Ordering};

/// FLV encoder for muxing media frames into FLV format
pub struct FlvEncoder {
    header_sent: bool,
    has_video: bool,
    has_audio: bool,
    sequence_headers_sent: SequenceHeaders,
    last_timestamp: AtomicU32,
}

#[derive(Default)]
struct SequenceHeaders {
    video: Option<Bytes>,
    audio: Option<Bytes>,
}

impl FlvEncoder {
    pub fn new(has_video: bool, has_audio: bool) -> Self {
        Self {
            header_sent: false,
            has_video,
            has_audio,
            sequence_headers_sent: SequenceHeaders::default(),
            last_timestamp: AtomicU32::new(0),
        }
    }

    /// Create encoder for video-only stream
    pub fn video_only() -> Self {
        Self::new(true, false)
    }

    /// Create encoder for audio-only stream
    pub fn audio_only() -> Self {
        Self::new(false, true)
    }

    /// Create encoder for video+audio stream
    pub fn video_audio() -> Self {
        Self::new(true, true)
    }

    /// Get the FLV header (call once at start)
    pub fn header(&mut self) -> Option<Bytes> {
        if self.header_sent {
            return None;
        }
        self.header_sent = true;

        let header = FlvHeader::new(self.has_video, self.has_audio);
        Some(header.encode())
    }

    /// Encode a media frame into FLV tag
    ///
    /// Returns the encoded FLV data ready for streaming.
    /// Sequence headers (AVCDecoderConfigurationRecord, AudioSpecificConfig)
    /// are cached for new subscribers.
    pub fn encode_frame(&mut self, frame: &MediaFrame) -> FlvResult<Option<Bytes>> {
        // Update last timestamp
        let timestamp_ms = frame.pts.as_millis() as u32;
        self.last_timestamp.store(timestamp_ms, Ordering::Relaxed);

        // Encode based on frame type
        let (tag_type, data) = match frame.frame_type {
            FrameType::Video(_) => {
                let data = video_frame_to_flv(frame)?;

                // Cache sequence header ONLY when frame is actual SequenceHeader type
                // This prevents false positives from regular frames that happen to have similar byte patterns
                if frame.is_sequence_header() {
                    self.sequence_headers_sent.video = Some(data.clone());
                }

                (TagType::Video, data)
            }
            FrameType::Audio(_) => {
                let data = audio_frame_to_flv(frame)?;

                // Cache sequence header ONLY when frame is actual SequenceHeader type
                if frame.is_sequence_header() {
                    self.sequence_headers_sent.audio = Some(data.clone());
                }

                (TagType::Audio, data)
            }
            _ => {
                // Script/data tags not yet supported
                return Ok(None);
            }
        };

        // Create tag
        let tag = self.create_tag(tag_type, timestamp_ms, data)?;

        Ok(Some(tag))
    }

    /// Encode with sequence headers prepended (for new subscribers)
    ///
    /// This prepends cached sequence headers before regular keyframes.
    /// SequenceHeader frames are passed through directly without prepending.
    pub fn encode_frame_with_headers(&mut self, frame: &MediaFrame) -> FlvResult<Option<Bytes>> {
        let mut result = BytesMut::new();

        // If frame IS a SequenceHeader, pass it through directly without prepending
        // This prevents "Found another AVCDecoderConfigurationRecord!" errors
        if frame.is_sequence_header() {
            if let Some(tag) = self.encode_frame(frame)? {
                result.extend_from_slice(&tag);
            }
        } else if frame.is_regular_keyframe() {
            // Regular keyframe: prepend cached sequence headers first
            if let Some(ref video_header) = self.sequence_headers_sent.video {
                let tag = self.create_tag(TagType::Video, 0, video_header.clone())?;
                result.extend_from_slice(&tag);
            }
            if let Some(ref audio_header) = self.sequence_headers_sent.audio {
                let tag = self.create_tag(TagType::Audio, 0, audio_header.clone())?;
                result.extend_from_slice(&tag);
            }
            // Then encode the actual frame
            if let Some(tag) = self.encode_frame(frame)? {
                result.extend_from_slice(&tag);
            }
        } else {
            // Non-keyframe (interframe, audio raw): just encode normally
            if let Some(tag) = self.encode_frame(frame)? {
                result.extend_from_slice(&tag);
            }
        }

        if result.is_empty() {
            Ok(None)
        } else {
            Ok(Some(result.freeze()))
        }
    }

    /// Create a FLV tag from components
    fn create_tag(&self, tag_type: TagType, timestamp: u32, data: Bytes) -> FlvResult<Bytes> {
        let data_size = data.len() as u32;

        // Tag header (11 bytes)
        let header = FlvTagHeader::new(tag_type, data_size, timestamp);
        let header_bytes = header.encode();

        // Previous tag size (4 bytes, big-endian)
        let tag_size = FLV_TAG_HEADER_SIZE as u32 + data_size;

        let mut buf = BytesMut::with_capacity(FLV_TAG_HEADER_SIZE + data.len() + PREVIOUS_TAG_SIZE);
        buf.extend_from_slice(&header_bytes);
        buf.extend_from_slice(&data);
        buf.put_u32(tag_size);

        Ok(buf.freeze())
    }

    /// Send metadata tag (onMetaData)
    ///
    /// This should be sent at the beginning of the stream.
    pub fn encode_metadata(&mut self, metadata: &ScriptData) -> FlvResult<Bytes> {
        let data = metadata.encode()?;
        self.create_tag(TagType::Script, 0, data)
    }

    /// Get the last timestamp sent
    pub fn last_timestamp(&self) -> u32 {
        self.last_timestamp.load(Ordering::Relaxed)
    }

    /// Check if header has been sent
    pub fn is_header_sent(&self) -> bool {
        self.header_sent
    }

    /// Check if stream has video
    pub fn has_video(&self) -> bool {
        self.has_video
    }

    /// Check if stream has audio
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }
}

/// Script data object for onMetaData
#[derive(Debug, Clone, Default)]
pub struct ScriptData {
    pub duration: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub video_data_rate: Option<f64>,
    pub audio_data_rate: Option<f64>,
    pub frame_rate: Option<f64>,
    pub video_codec_id: Option<f64>,
    pub audio_codec_id: Option<f64>,
    pub encoder: Option<String>,
    pub file_size: Option<f64>,
}

impl ScriptData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_video(mut self, width: f64, height: f64, fps: f64) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self.frame_rate = Some(fps);
        self
    }

    pub fn with_audio(mut self, _sample_rate: f64, _channels: f64) -> Self {
        self.audio_codec_id = Some(10.0); // AAC
        self
    }

    pub fn with_bitrate(mut self, video_kbps: f64, audio_kbps: f64) -> Self {
        self.video_data_rate = Some(video_kbps);
        self.audio_data_rate = Some(audio_kbps);
        self
    }

    /// Encode to AMF0
    pub fn encode(&self) -> FlvResult<Bytes> {
        use crate::protocol::amf0::{Amf0Encoder, Amf0Value};
        use std::collections::HashMap;

        let mut obj = HashMap::new();

        if let Some(v) = self.duration {
            obj.insert("duration".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.width {
            obj.insert("width".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.height {
            obj.insert("height".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.video_data_rate {
            obj.insert("videodatarate".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.audio_data_rate {
            obj.insert("audiodatarate".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.frame_rate {
            obj.insert("framerate".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.video_codec_id {
            obj.insert("videocodecid".to_string(), Amf0Value::Number(v));
        }
        if let Some(v) = self.audio_codec_id {
            obj.insert("audiocodecid".to_string(), Amf0Value::Number(v));
        }
        if let Some(ref v) = self.encoder {
            obj.insert("encoder".to_string(), Amf0Value::String(v.clone()));
        }
        if let Some(v) = self.file_size {
            obj.insert("filesize".to_string(), Amf0Value::Number(v));
        }

        let mut buf = Vec::new();

        // Encode "onMetaData" string
        Amf0Encoder::encode_value(&mut buf, &Amf0Value::String("onMetaData".to_string()))?;

        // ECMA array (object with length)
        Amf0Encoder::encode_value(&mut buf, &Amf0Value::EcmaArray(obj))?;

        Ok(Bytes::from(buf))
    }
}

/// Batch encoder for efficient bulk encoding
pub struct FlvBatchEncoder {
    encoder: FlvEncoder,
    buffer: BytesMut,
    batch_size: usize,
}

impl FlvBatchEncoder {
    pub fn new(has_video: bool, has_audio: bool, batch_size: usize) -> Self {
        Self {
            encoder: FlvEncoder::new(has_video, has_audio),
            buffer: BytesMut::with_capacity(batch_size * 1024),
            batch_size,
        }
    }

    /// Add frame to batch
    pub fn add_frame(&mut self, frame: &MediaFrame) -> FlvResult<Option<Bytes>> {
        // Add header if not sent
        if !self.encoder.is_header_sent() {
            if let Some(header) = self.encoder.header() {
                self.buffer.extend_from_slice(&header);
            }
        }

        // Encode frame
        if let Some(data) = self.encoder.encode_frame(frame)? {
            self.buffer.extend_from_slice(&data);
        }

        // Flush if buffer is full
        if self.buffer.len() >= self.batch_size {
            Ok(Some(self.flush()))
        } else {
            Ok(None)
        }
    }

    /// Flush buffered data
    pub fn flush(&mut self) -> Bytes {
        let result = self.buffer.split();
        self.buffer.reserve(self.batch_size * 1024);
        result.freeze()
    }

    /// Check if encoder is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{CodecType, Timestamp, VideoFrameType};

    #[test]
    fn test_flv_encoder() {
        let mut encoder = FlvEncoder::video_audio();

        // Get header
        let header = encoder.header().unwrap();
        assert!(!header.is_empty());

        // Encode video keyframe
        let frame = MediaFrame::video(
            1,
            Timestamp::from_millis(1000),
            VideoFrameType::Keyframe,
            CodecType::H264,
            Bytes::from(vec![0x67, 0x42, 0x00, 0x0A]), // Fake H.264 data
        );

        let tag = encoder.encode_frame(&frame).unwrap().unwrap();
        assert!(!tag.is_empty());
    }

    #[test]
    fn test_script_data_encode() {
        let script = ScriptData::new()
            .with_video(1920.0, 1080.0, 30.0)
            .with_bitrate(2000.0, 128.0);

        let data = script.encode().unwrap();
        assert!(!data.is_empty());
    }
}
