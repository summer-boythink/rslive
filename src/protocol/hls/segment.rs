//! HLS segment management
//!
//! This module handles the creation and storage of HLS segments.
//! It supports both MPEG-TS and fMP4 formats.

use super::{HlsError, HlsResult};
use super::mpegts::{TsMuxer, TsMuxerConfig};
use crate::media::{CodecType, MediaFrame, Timestamp};
use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;

/// Segment format (MPEG-TS or fMP4)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentFormat {
    /// MPEG-TS format
    MpegTs,
    /// Fragmented MP4 (CMAF)
    Fmp4,
}

impl SegmentFormat {
    pub fn file_extension(&self) -> &'static str {
        match self {
            SegmentFormat::MpegTs => ".ts",
            SegmentFormat::Fmp4 => ".m4s",
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            SegmentFormat::MpegTs => "video/mp2t",
            SegmentFormat::Fmp4 => "video/iso.segment",
        }
    }
}

/// Segment information
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    /// Segment index
    pub index: u64,
    /// Segment start timestamp
    pub start_time: Timestamp,
    /// Segment duration
    pub duration: Duration,
    /// Segment format
    pub format: SegmentFormat,
    /// Whether segment starts with a keyframe
    pub starts_with_keyframe: bool,
    /// Segment file size in bytes
    pub size: usize,
}

impl SegmentInfo {
    pub fn new(index: u64, start_time: Timestamp, duration: Duration) -> Self {
        Self {
            index,
            start_time,
            duration,
            format: SegmentFormat::MpegTs,
            starts_with_keyframe: false,
            size: 0,
        }
    }

    pub fn with_format(mut self, format: SegmentFormat) -> Self {
        self.format = format;
        self
    }

    pub fn filename(&self) -> String {
        format!("segment{}{}", self.index, self.format.file_extension())
    }
}

/// HLS segment containing media data
pub struct Segment {
    /// Segment information
    pub info: SegmentInfo,
    /// Segment data
    pub data: Bytes,
    /// Video codec used
    pub video_codec: Option<CodecType>,
    /// Audio codec used
    pub audio_codec: Option<CodecType>,
    /// Whether this segment is complete
    pub complete: bool,
}

impl Segment {
    pub fn new(info: SegmentInfo, data: Bytes) -> Self {
        Self {
            info,
            data,
            video_codec: None,
            audio_codec: None,
            complete: true,
        }
    }

    pub fn from_frames(
        index: u64,
        frames: &[MediaFrame],
        format: SegmentFormat,
    ) -> HlsResult<Self> {
        if frames.is_empty() {
            return Err(HlsError::InvalidData("No frames to create segment".into()));
        }

        let start_time = frames[0].pts;
        let end_time = frames[frames.len() - 1].pts;
        let duration = end_time.duration_since(start_time);

        let starts_with_keyframe = frames[0].is_keyframe();

        let mut info = SegmentInfo::new(index, start_time, duration).with_format(format);
        info.starts_with_keyframe = starts_with_keyframe;

        // Encode frames based on format
        let data = match format {
            SegmentFormat::MpegTs => encode_ts_segment(frames)?,
            SegmentFormat::Fmp4 => encode_fmp4_segment(frames)?,
        };

        info.size = data.len();

        let mut segment = Self::new(info, data);

        // Extract codec info from frames
        for frame in frames {
            if frame.is_video() && segment.video_codec.is_none() {
                segment.video_codec = Some(frame.codec);
            }
            if frame.is_audio() && segment.audio_codec.is_none() {
                segment.audio_codec = Some(frame.codec);
            }
        }

        Ok(segment)
    }

    pub fn data(&self) -> &Bytes {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Encode frames to MPEG-TS segment
fn encode_ts_segment(frames: &[MediaFrame]) -> HlsResult<Bytes> {
    if frames.is_empty() {
        return Err(HlsError::InvalidData("No frames to encode".into()));
    }

    // Detect codecs from frames
    let video_codec = frames.iter()
        .find(|f| f.is_video())
        .map(|f| f.codec);

    let audio_codec = frames.iter()
        .find(|f| f.is_audio())
        .map(|f| f.codec);

    // Create muxer config
    let mut config = TsMuxerConfig::default();
    config.video_codec = video_codec;
    config.audio_codec = audio_codec;

    // Create muxer and generate segment
    let mut muxer = TsMuxer::new(config);
    Ok(muxer.create_segment(frames))
}

/// Encode frames to fMP4 segment
fn encode_fmp4_segment(_frames: &[MediaFrame]) -> HlsResult<Bytes> {
    // TODO: Implement fMP4 muxer
    // This is a placeholder - real implementation would:
    // 1. Create moof (movie fragment) box
    // 2. Create mdat box with sample data
    // 3. Handle sample tables and durations
    Err(HlsError::Fmp4("fMP4 muxer not yet implemented".into()))
}

/// Segment storage interface
pub trait SegmentStorage: Send + Sync {
    fn store(&self, segment: &Segment) -> HlsResult<()>;
    fn load(&self, index: u64) -> HlsResult<Option<Segment>>;
    fn delete(&self, index: u64) -> HlsResult<()>;
    fn list(&self) -> HlsResult<Vec<SegmentInfo>>;
}

/// In-memory segment storage (for testing and low-latency scenarios)
pub struct MemorySegmentStorage {
    segments: Arc<dashmap::DashMap<u64, Segment>>,
    max_segments: usize,
}

impl MemorySegmentStorage {
    pub fn new(max_segments: usize) -> Self {
        Self {
            segments: Arc::new(dashmap::DashMap::new()),
            max_segments,
        }
    }

    fn enforce_limits(&self) {
        while self.segments.len() > self.max_segments {
            // Remove oldest segment
            let oldest = self
                .segments
                .iter()
                .map(|e| (*e.key(), e.value().info.start_time))
                .min_by_key(|(_, ts)| ts.as_nanos());

            if let Some((index, _)) = oldest {
                self.segments.remove(&index);
            }
        }
    }
}

impl SegmentStorage for MemorySegmentStorage {
    fn store(&self, segment: &Segment) -> HlsResult<()> {
        self.segments.insert(
            segment.info.index,
            Segment {
                info: segment.info.clone(),
                data: segment.data.clone(),
                video_codec: segment.video_codec,
                audio_codec: segment.audio_codec,
                complete: segment.complete,
            },
        );

        self.enforce_limits();
        Ok(())
    }

    fn load(&self, index: u64) -> HlsResult<Option<Segment>> {
        Ok(self.segments.get(&index).map(|s| Segment {
            info: s.info.clone(),
            data: s.data.clone(),
            video_codec: s.video_codec,
            audio_codec: s.audio_codec,
            complete: s.complete,
        }))
    }

    fn delete(&self, index: u64) -> HlsResult<()> {
        self.segments.remove(&index);
        Ok(())
    }

    fn list(&self) -> HlsResult<Vec<SegmentInfo>> {
        let mut segments: Vec<_> = self
            .segments
            .iter()
            .map(|e| e.value().info.clone())
            .collect();

        segments.sort_by_key(|s| s.index);
        Ok(segments)
    }
}

/// File-based segment storage
pub struct FileSegmentStorage {
    output_dir: std::path::PathBuf,
}

impl FileSegmentStorage {
    pub fn new(output_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
        }
    }
}

impl SegmentStorage for FileSegmentStorage {
    fn store(&self, segment: &Segment) -> HlsResult<()> {
        let path = self.output_dir.join(segment.info.filename());

        // Ensure directory exists
        std::fs::create_dir_all(&self.output_dir).map_err(|e| HlsError::Io(e))?;

        // Write segment
        std::fs::write(&path, &segment.data).map_err(|e| HlsError::Io(e))?;

        Ok(())
    }

    fn load(&self, index: u64) -> HlsResult<Option<Segment>> {
        // Try to find segment by index
        for entry in std::fs::read_dir(&self.output_dir).map_err(|e| HlsError::Io(e))? {
            let entry = entry.map_err(|e| HlsError::Io(e))?;
            let filename = entry.file_name();
            let name = filename.to_string_lossy();

            if name.starts_with(&format!("segment{}", index)) {
                let data = std::fs::read(entry.path()).map_err(|e| HlsError::Io(e))?;

                // Parse info from file
                let format = if name.ends_with(".ts") {
                    SegmentFormat::MpegTs
                } else {
                    SegmentFormat::Fmp4
                };

                let info =
                    SegmentInfo::new(index, Timestamp::ZERO, Duration::ZERO).with_format(format);

                return Ok(Some(Segment::new(info, Bytes::from(data))));
            }
        }

        Ok(None)
    }

    fn delete(&self, index: u64) -> HlsResult<()> {
        for format in [SegmentFormat::MpegTs, SegmentFormat::Fmp4] {
            let path = self
                .output_dir
                .join(format!("segment{}{}", index, format.file_extension()));
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| HlsError::Io(e))?;
            }
        }
        Ok(())
    }

    fn list(&self) -> HlsResult<Vec<SegmentInfo>> {
        let mut segments = Vec::new();

        for entry in std::fs::read_dir(&self.output_dir).map_err(|e| HlsError::Io(e))? {
            let entry = entry.map_err(|e| HlsError::Io(e))?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Parse segment index from filename
            if name.starts_with("segment") {
                if let Some(index_str) = name.strip_prefix("segment") {
                    if let Some(index) = index_str.split('.').next() {
                        if let Ok(index) = index.parse::<u64>() {
                            let format = if name.ends_with(".ts") {
                                SegmentFormat::MpegTs
                            } else {
                                SegmentFormat::Fmp4
                            };

                            let info = SegmentInfo::new(index, Timestamp::ZERO, Duration::ZERO)
                                .with_format(format);

                            segments.push(info);
                        }
                    }
                }
            }
        }

        segments.sort_by_key(|s| s.index);
        Ok(segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{CodecType, VideoFrameType};

    #[test]
    fn test_segment_info() {
        let info = SegmentInfo::new(0, Timestamp::from_millis(0), Duration::from_secs(6))
            .with_format(SegmentFormat::MpegTs);

        assert_eq!(info.filename(), "segment0.ts");
        assert_eq!(info.format.file_extension(), ".ts");
    }

    #[test]
    fn test_segment_from_frames() {
        let frames = vec![
            MediaFrame::video(
                1,
                Timestamp::from_millis(0),
                VideoFrameType::Keyframe,
                CodecType::H264,
                Bytes::from(vec![0; 100]),
            ),
            MediaFrame::video(
                1,
                Timestamp::from_millis(100),
                VideoFrameType::Interframe,
                CodecType::H264,
                Bytes::from(vec![0; 50]),
            ),
        ];

        let segment = Segment::from_frames(0, &frames, SegmentFormat::MpegTs).unwrap();

        assert_eq!(segment.info.index, 0);
        assert!(segment.info.starts_with_keyframe);
        assert_eq!(segment.video_codec, Some(CodecType::H264));
    }
}
