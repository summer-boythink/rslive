//! Media Segment Builder (moof + mdat)
//!
//! The Media Segment contains the actual media data:
//! - moof: Movie Fragment (metadata for samples in this segment)
//! - mdat: Media Data (actual audio/video data)
//!
//! Each segment is independently playable (after the Init Segment).

use super::boxes::{BoxType, FullBox, Mp4Box, writer};
use super::{AUDIO_TRACK_ID, VIDEO_TRACK_ID};
use std::io::{self, Write};

/// A single media sample (frame)
#[derive(Debug, Clone)]
pub struct Sample {
    /// Sample data (encoded frame)
    pub data: Vec<u8>,

    /// Duration in timescale units
    pub duration: u32,

    /// Composition time offset (for B-frames, in timescale units)
    pub composition_time_offset: i32,

    /// Is this a sync sample (keyframe)?
    pub is_sync: bool,

    /// Track ID this sample belongs to
    pub track_id: u32,
}

impl Sample {
    /// Create a new sample
    pub fn new(track_id: u32, data: Vec<u8>, duration: u32, is_sync: bool) -> Self {
        Self {
            track_id,
            data,
            duration,
            composition_time_offset: 0,
            is_sync,
        }
    }

    /// Create a video keyframe sample
    pub fn video_keyframe(data: Vec<u8>, duration: u32) -> Self {
        Self::new(VIDEO_TRACK_ID, data, duration, true)
    }

    /// Create a video non-keyframe sample
    pub fn video_frame(data: Vec<u8>, duration: u32) -> Self {
        Self::new(VIDEO_TRACK_ID, data, duration, false)
    }

    /// Create an audio sample (always sync)
    pub fn audio(data: Vec<u8>, duration: u32) -> Self {
        Self::new(AUDIO_TRACK_ID, data, duration, true)
    }

    /// Set composition time offset
    pub fn with_composition_time_offset(mut self, offset: i32) -> Self {
        self.composition_time_offset = offset;
        self
    }

    /// Get sample size in bytes
    pub fn size(&self) -> u32 {
        self.data.len() as u32
    }
}

// ============================================================================
// mfhd Box (Movie Fragment Header Box)
// ============================================================================

/// mfhd box - Movie Fragment Header Box
pub struct MfhdBox {
    /// Sequence number (incremented for each fragment)
    sequence_number: u32,
}

impl MfhdBox {
    pub fn new(sequence_number: u32) -> Self {
        Self { sequence_number }
    }
}

impl FullBox for MfhdBox {
    fn version(&self) -> u8 {
        0
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for MfhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Mfhd
    }

    fn box_size(&self) -> u64 {
        16 // 8 (header) + 4 (version/flags) + 4 (sequence_number)
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, self.sequence_number)?;
        Ok(())
    }
}

// ============================================================================
// tfhd Box (Track Fragment Header Box)
// ============================================================================

/// tfhd box - Track Fragment Header Box
pub struct TfhdBox {
    track_id: u32,
    base_data_offset: Option<u64>,
    sample_description_index: Option<u32>,
    default_sample_duration: Option<u32>,
    default_sample_size: Option<u32>,
    default_sample_flags: Option<u32>,
}

impl TfhdBox {
    pub fn new(track_id: u32) -> Self {
        Self {
            track_id,
            base_data_offset: None,
            sample_description_index: None,
            default_sample_duration: None,
            default_sample_size: None,
            default_sample_flags: None,
        }
    }

    /// Set base data offset
    pub fn with_base_data_offset(mut self, offset: u64) -> Self {
        self.base_data_offset = Some(offset);
        self
    }

    /// Set default sample duration
    pub fn with_default_sample_duration(mut self, duration: u32) -> Self {
        self.default_sample_duration = Some(duration);
        self
    }

    /// Set default sample size
    pub fn with_default_sample_size(mut self, size: u32) -> Self {
        self.default_sample_size = Some(size);
        self
    }

    /// Set default sample flags
    pub fn with_default_sample_flags(mut self, flags: u32) -> Self {
        self.default_sample_flags = Some(flags);
        self
    }

    /// Calculate flags field
    fn calculate_flags(&self) -> u32 {
        let mut flags = 0u32;

        if self.base_data_offset.is_some() {
            flags |= 0x000001; // base-data-offset-present
        }
        if self.sample_description_index.is_some() {
            flags |= 0x000002; // sample-description-index-present
        }
        if self.default_sample_duration.is_some() {
            flags |= 0x000008; // default-sample-duration-present
        }
        if self.default_sample_size.is_some() {
            flags |= 0x000010; // default-sample-size-present
        }
        if self.default_sample_flags.is_some() {
            flags |= 0x000020; // default-sample-flags-present
        }

        // Always set duration-is-empty and default-base-is-moof for fMP4
        flags |= 0x010008; // default-base-is-moof | duration-is-empty

        flags
    }

    /// Calculate box size based on which fields are present
    fn calculate_size(&self) -> u64 {
        let mut size = 8 + 4 + 4; // header + version/flags + track_id

        if self.base_data_offset.is_some() {
            size += 8;
        }
        if self.sample_description_index.is_some() {
            size += 4;
        }
        if self.default_sample_duration.is_some() {
            size += 4;
        }
        if self.default_sample_size.is_some() {
            size += 4;
        }
        if self.default_sample_flags.is_some() {
            size += 4;
        }

        size
    }
}

impl FullBox for TfhdBox {
    fn version(&self) -> u8 {
        0
    }

    fn flags(&self) -> u32 {
        self.calculate_flags()
    }
}

impl Mp4Box for TfhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Tfhd
    }

    fn box_size(&self) -> u64 {
        self.calculate_size()
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, self.track_id)?;

        if let Some(offset) = self.base_data_offset {
            writer::write_u64(writer, offset)?;
        }
        if let Some(index) = self.sample_description_index {
            writer::write_u32(writer, index)?;
        }
        if let Some(duration) = self.default_sample_duration {
            writer::write_u32(writer, duration)?;
        }
        if let Some(size) = self.default_sample_size {
            writer::write_u32(writer, size)?;
        }
        if let Some(flags) = self.default_sample_flags {
            writer::write_u32(writer, flags)?;
        }

        Ok(())
    }
}

// ============================================================================
// tfdt Box (Track Fragment Base Media Decode Time Box)
// ============================================================================

/// tfdt box - Track Fragment Base Media Decode Time Box
pub struct TfdtBox {
    /// Base media decode time in timescale units
    base_media_decode_time: u64,
}

impl TfdtBox {
    pub fn new(base_media_decode_time: u64) -> Self {
        Self {
            base_media_decode_time,
        }
    }
}

impl FullBox for TfdtBox {
    fn version(&self) -> u8 {
        1 // Version 1 for 64-bit time
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for TfdtBox {
    fn box_type(&self) -> BoxType {
        BoxType::Tfdt
    }

    fn box_size(&self) -> u64 {
        20 // 8 (header) + 4 (version/flags) + 8 (base_media_decode_time)
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u64(writer, self.base_media_decode_time)?;
        Ok(())
    }
}

// ============================================================================
// trun Box (Track Run Box)
// ============================================================================

/// Sample entry for trun box
#[derive(Debug, Clone, Default)]
pub struct TrunSample {
    pub duration: Option<u32>,
    pub size: Option<u32>,
    pub flags: Option<u32>,
    pub composition_time_offset: Option<i32>,
}

/// trun box - Track Run Box
pub struct TrunBox {
    data_offset: i32,
    samples: Vec<TrunSample>,
    first_sample_flags: Option<u32>,
}

impl TrunBox {
    pub fn new(data_offset: i32) -> Self {
        Self {
            data_offset,
            samples: Vec::new(),
            first_sample_flags: None,
        }
    }

    /// Add a sample to the run
    pub fn add_sample(&mut self, sample: TrunSample) {
        self.samples.push(sample);
    }

    /// Set first sample flags (used instead of per-sample flags for first sample)
    pub fn with_first_sample_flags(mut self, flags: u32) -> Self {
        self.first_sample_flags = Some(flags);
        self
    }

    /// Calculate flags field
    fn calculate_flags(&self) -> u32 {
        let mut flags = 0u32;

        // data-offset-present (always set for fMP4)
        flags |= 0x000001;

        // Check if all samples have the same values to optimize
        let has_duration = self.samples.iter().any(|s| s.duration.is_some());
        let has_size = self.samples.iter().any(|s| s.size.is_some());
        let has_flags = self.samples.iter().any(|s| s.flags.is_some());
        let has_cto = self
            .samples
            .iter()
            .any(|s| s.composition_time_offset.is_some());

        if has_duration {
            flags |= 0x000100; // sample-duration-present
        }
        if has_size {
            flags |= 0x000200; // sample-size-present
        }
        if has_flags {
            flags |= 0x000400; // sample-flags-present
        }
        if self.first_sample_flags.is_some() {
            flags |= 0x000004; // first-sample-flags-present
        }
        if has_cto {
            // Use version 1 for signed composition time offsets
            flags |= 0x000800; // sample-composition-time-offsets-present
        }

        flags
    }

    /// Calculate box size
    fn calculate_size(&self) -> u64 {
        let _flags = self.calculate_flags();

        let mut size = 8 + 4 + 4; // header + version/flags + sample_count
        size += 4; // data_offset

        if self.first_sample_flags.is_some() {
            size += 4;
        }

        for sample in &self.samples {
            if sample.duration.is_some() {
                size += 4;
            }
            if sample.size.is_some() {
                size += 4;
            }
            if sample.flags.is_some() {
                size += 4;
            }
            if sample.composition_time_offset.is_some() {
                size += 4;
            }
        }

        size
    }
}

impl FullBox for TrunBox {
    fn version(&self) -> u8 {
        // Version 1 if composition time offsets are present (for signed values)
        let flags = self.calculate_flags();
        if (flags & 0x000800) != 0 { 1 } else { 0 }
    }

    fn flags(&self) -> u32 {
        self.calculate_flags()
    }
}

impl Mp4Box for TrunBox {
    fn box_type(&self) -> BoxType {
        BoxType::Trun
    }

    fn box_size(&self) -> u64 {
        self.calculate_size()
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        // Sample count
        writer::write_u32(writer, self.samples.len() as u32)?;

        // Data offset (signed 32-bit)
        writer::write_u32(writer, self.data_offset as u32)?;

        // First sample flags (if present)
        if let Some(flags) = self.first_sample_flags {
            writer::write_u32(writer, flags)?;
        }

        // Write samples
        for sample in &self.samples {
            if let Some(duration) = sample.duration {
                writer::write_u32(writer, duration)?;
            }
            if let Some(size) = sample.size {
                writer::write_u32(writer, size)?;
            }
            if let Some(flags) = sample.flags {
                writer::write_u32(writer, flags)?;
            }
            if let Some(cto) = sample.composition_time_offset {
                writer::write_u32(writer, cto as u32)?;
            }
        }

        Ok(())
    }
}

// ============================================================================
// traf Box (Track Fragment Box)
// ============================================================================

/// traf box - Track Fragment Box (container)
pub struct TrafBox {
    track_id: u32,
    base_decode_time: u64,
    samples: Vec<Sample>,
    moof_offset: u64, // Offset of moof in the file
}

impl TrafBox {
    pub fn new(track_id: u32, base_decode_time: u64) -> Self {
        Self {
            track_id,
            base_decode_time,
            samples: Vec::new(),
            moof_offset: 0,
        }
    }

    /// Add a sample
    pub fn add_sample(&mut self, sample: Sample) {
        self.samples.push(sample);
    }

    /// Build the traf box with all children
    fn build_boxes(&self, data_offset: i32) -> io::Result<Vec<Box<dyn Mp4Box + Send>>> {
        let mut boxes: Vec<Box<dyn Mp4Box + Send>> = Vec::new();

        // tfhd
        let tfhd = TfhdBox::new(self.track_id).with_base_data_offset(self.moof_offset);
        boxes.push(Box::new(tfhd));

        // tfdt
        boxes.push(Box::new(TfdtBox::new(self.base_decode_time)));

        // trun
        let mut trun = TrunBox::new(data_offset);

        for sample in &self.samples {
            let trun_sample = TrunSample {
                duration: Some(sample.duration),
                size: Some(sample.size()),
                flags: Some(sample_flags_to_u32(sample)),
                composition_time_offset: if sample.composition_time_offset != 0 {
                    Some(sample.composition_time_offset)
                } else {
                    None
                },
            };
            trun.add_sample(trun_sample);
        }

        boxes.push(Box::new(trun));

        Ok(boxes)
    }
}

impl Mp4Box for TrafBox {
    fn box_type(&self) -> BoxType {
        BoxType::Traf
    }

    fn box_size(&self) -> u64 {
        // Calculate size of all child boxes
        let data_offset = 0; // Placeholder for size calculation
        let boxes = self.build_boxes(data_offset).unwrap_or_default();
        let mut size = 8u64; // Container header
        for b in &boxes {
            size += b.box_size();
        }
        size
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Calculate mdat header size (8 bytes) and traf size to get data_offset
        // data_offset = moof_size + mdat_header_size
        let moof_size = self.calculate_moof_size();
        let data_offset = (moof_size + 8) as i32; // 8 = mdat header

        let boxes = self.build_boxes(data_offset)?;
        for b in &boxes {
            b.write_box(writer)?;
        }
        Ok(())
    }
}

impl TrafBox {
    fn calculate_moof_size(&self) -> u64 {
        // mfhd + traf size
        let mfhd_size = MfhdBox::new(0).box_size();
        let traf_size = self.box_size();
        8 + mfhd_size + traf_size // moof header + children
    }
}

// ============================================================================
// moof Box (Movie Fragment Box)
// ============================================================================

/// moof box - Movie Fragment Box
pub struct MoofBox {
    sequence_number: u32,
    tracks: Vec<TrafBox>,
}

impl MoofBox {
    pub fn new(sequence_number: u32) -> Self {
        Self {
            sequence_number,
            tracks: Vec::new(),
        }
    }

    /// Add a track fragment
    pub fn add_track(&mut self, track: TrafBox) {
        self.tracks.push(track);
    }
}

impl Mp4Box for MoofBox {
    fn box_type(&self) -> BoxType {
        BoxType::Moof
    }

    fn box_size(&self) -> u64 {
        let mut size = 8u64; // Container header
        size += MfhdBox::new(0).box_size(); // mfhd
        for track in &self.tracks {
            size += track.box_size();
        }
        size
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Write mfhd
        let mfhd = MfhdBox::new(self.sequence_number);
        mfhd.write_box(writer)?;

        // Write traf boxes
        for track in &self.tracks {
            track.write_box(writer)?;
        }

        Ok(())
    }
}

// ============================================================================
// mdat Box (Media Data Box)
// ============================================================================

/// mdat box - Media Data Box
pub struct MdatBox {
    data: Vec<u8>,
}

impl MdatBox {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Create from multiple sample data
    pub fn from_samples(samples: &[Sample]) -> Self {
        let total_size: usize = samples.iter().map(|s| s.data.len()).sum();
        let mut data = Vec::with_capacity(total_size);
        for sample in samples {
            data.extend_from_slice(&sample.data);
        }
        Self { data }
    }
}

impl Mp4Box for MdatBox {
    fn box_type(&self) -> BoxType {
        BoxType::Mdat
    }

    fn box_size(&self) -> u64 {
        8 + self.data.len() as u64 // header + data
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer::write_bytes(writer, &self.data)?;
        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert sample properties to sample flags u32
fn sample_flags_to_u32(sample: &Sample) -> u32 {
    let mut flags = 0u32;

    // is_leading (2 bits) - 0 = not a leading sample
    // sample_depends_on (2 bits)
    if sample.is_sync {
        flags |= 0x02000000; // sample_does_not_depend_on_others (independent)
    } else {
        flags |= 0x01000000; // sample_depends_on_others
    }

    // sample_is_depended_on (2 bits) - 0 = unknown
    // sample_has_redundancy (2 bits) - 0 = unknown
    // sample_padding_value (3 bits) - 0
    // sample_is_non_sync_sample (1 bit)
    if !sample.is_sync {
        flags |= 0x00010000;
    }

    // sample_degradation_priority (16 bits) - 0

    flags
}

// ============================================================================
// MediaSegmentBuilder
// ============================================================================

/// Builder for creating Media Segments
#[derive(Clone)]
pub struct MediaSegmentBuilder {
    sequence_number: u32,
    video_samples: Vec<Sample>,
    audio_samples: Vec<Sample>,
    video_decode_time: u64,
    audio_decode_time: u64,
}

impl Default for MediaSegmentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaSegmentBuilder {
    pub fn new() -> Self {
        Self {
            sequence_number: 1,
            video_samples: Vec::new(),
            audio_samples: Vec::new(),
            video_decode_time: 0,
            audio_decode_time: 0,
        }
    }

    /// Set sequence number
    pub fn with_sequence_number(mut self, seq: u32) -> Self {
        self.sequence_number = seq;
        self
    }

    /// Set video decode time base
    pub fn with_video_decode_time(mut self, time: u64) -> Self {
        self.video_decode_time = time;
        self
    }

    /// Set audio decode time base
    pub fn with_audio_decode_time(mut self, time: u64) -> Self {
        self.audio_decode_time = time;
        self
    }

    /// Add a video sample
    pub fn add_video_sample(&mut self, sample: Sample) {
        self.video_samples.push(sample);
    }

    /// Add an audio sample
    pub fn add_audio_sample(&mut self, sample: Sample) {
        self.audio_samples.push(sample);
    }

    /// Add multiple video samples
    pub fn add_video_samples(&mut self, samples: Vec<Sample>) {
        self.video_samples.extend(samples);
    }

    /// Add multiple audio samples
    pub fn add_audio_samples(&mut self, samples: Vec<Sample>) {
        self.audio_samples.extend(samples);
    }

    /// Build the media segment (moof + mdat)
    pub fn build(&self) -> io::Result<Vec<u8>> {
        let mut output = Vec::with_capacity(4096);

        // Build moof
        let moof = self.build_moof()?;

        // Build mdat
        let mdat = self.build_mdat();

        // Write moof
        moof.write_box(&mut output)?;

        // Write mdat
        mdat.write_box(&mut output)?;

        Ok(output)
    }

    fn build_moof(&self) -> io::Result<MoofBox> {
        let mut moof = MoofBox::new(self.sequence_number);

        // Add video track if we have samples
        if !self.video_samples.is_empty() {
            let mut traf = TrafBox::new(VIDEO_TRACK_ID, self.video_decode_time);
            for sample in &self.video_samples {
                traf.add_sample(sample.clone());
            }
            moof.add_track(traf);
        }

        // Add audio track if we have samples
        if !self.audio_samples.is_empty() {
            let mut traf = TrafBox::new(AUDIO_TRACK_ID, self.audio_decode_time);
            for sample in &self.audio_samples {
                traf.add_sample(sample.clone());
            }
            moof.add_track(traf);
        }

        Ok(moof)
    }

    fn build_mdat(&self) -> MdatBox {
        let mut all_samples = Vec::new();
        all_samples.extend(self.video_samples.clone());
        all_samples.extend(self.audio_samples.clone());
        MdatBox::from_samples(&all_samples)
    }

    /// Get total duration of video samples
    pub fn video_duration(&self) -> u64 {
        self.video_samples.iter().map(|s| s.duration as u64).sum()
    }

    /// Get total duration of audio samples
    pub fn audio_duration(&self) -> u64 {
        self.audio_samples.iter().map(|s| s.duration as u64).sum()
    }

    /// Check if there are any samples
    pub fn is_empty(&self) -> bool {
        self.video_samples.is_empty() && self.audio_samples.is_empty()
    }

    /// Clear all samples (for reusing builder)
    pub fn clear(&mut self) {
        self.video_samples.clear();
        self.audio_samples.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mfhd_box() {
        let mfhd = MfhdBox::new(42);
        assert_eq!(mfhd.box_type(), BoxType::Mfhd);
        assert_eq!(mfhd.box_size(), 16);

        let encoded = mfhd.encode().unwrap();
        assert!(encoded.starts_with(b"\x00\x00\x00\x10mfhd"));

        // Check sequence number at the end
        assert_eq!(&encoded[12..16], &42u32.to_be_bytes());
    }

    #[test]
    fn test_tfhd_box_basic() {
        let tfhd = TfhdBox::new(1);
        assert_eq!(tfhd.box_type(), BoxType::Tfhd);

        let encoded = tfhd.encode().unwrap();
        assert!(encoded.windows(4).any(|w| w == b"tfhd"));
    }

    #[test]
    fn test_tfhd_box_with_fields() {
        let tfhd = TfhdBox::new(1)
            .with_default_sample_duration(1000)
            .with_default_sample_size(500);

        let encoded = tfhd.encode().unwrap();
        assert!(encoded.len() > 16);
    }

    #[test]
    fn test_tfdt_box() {
        let tfdt = TfdtBox::new(12345678);
        assert_eq!(tfdt.box_type(), BoxType::Tfdt);
        assert_eq!(tfdt.box_size(), 20);

        let encoded = tfdt.encode().unwrap();
        assert!(encoded.windows(4).any(|w| w == b"tfdt"));
    }

    #[test]
    fn test_trun_box() {
        let mut trun = TrunBox::new(100);
        trun.add_sample(TrunSample {
            duration: Some(40),
            size: Some(1000),
            flags: Some(0x02000000), // sync sample
            composition_time_offset: None,
        });
        trun.add_sample(TrunSample {
            duration: Some(40),
            size: Some(800),
            flags: Some(0x01010000), // non-sync
            composition_time_offset: Some(20),
        });

        let encoded = trun.encode().unwrap();
        assert!(encoded.windows(4).any(|w| w == b"trun"));
    }

    #[test]
    fn test_mdat_box() {
        let data = vec![0u8; 100];
        let mdat = MdatBox::new(data.clone());

        assert_eq!(mdat.box_type(), BoxType::Mdat);
        assert_eq!(mdat.box_size(), 108); // 8 header + 100 data

        let encoded = mdat.encode().unwrap();
        assert!(encoded.starts_with(b"\x00\x00\x00\x6cmdat"));
    }

    #[test]
    fn test_sample_creation() {
        let video = Sample::video_keyframe(vec![1, 2, 3, 4], 40);
        assert_eq!(video.track_id, VIDEO_TRACK_ID);
        assert!(video.is_sync);
        assert_eq!(video.size(), 4);

        let audio = Sample::audio(vec![5, 6, 7], 20);
        assert_eq!(audio.track_id, AUDIO_TRACK_ID);
        assert!(audio.is_sync);
    }

    #[test]
    fn test_media_segment_builder() {
        let mut builder = MediaSegmentBuilder::new().with_sequence_number(1);

        builder.add_video_sample(Sample::video_keyframe(vec![0; 1000], 40));
        builder.add_video_sample(Sample::video_frame(vec![0; 500], 40));
        builder.add_audio_sample(Sample::audio(vec![0; 200], 20));

        let segment = builder.build().unwrap();

        // Should contain moof and mdat
        assert!(segment.windows(4).any(|w| w == b"moof"));
        assert!(segment.windows(4).any(|w| w == b"mdat"));
        assert!(segment.windows(4).any(|w| w == b"mfhd"));
        assert!(segment.windows(4).any(|w| w == b"traf"));
        assert!(segment.windows(4).any(|w| w == b"trun"));
    }

    #[test]
    fn test_media_segment_durations() {
        let mut builder = MediaSegmentBuilder::new();

        builder.add_video_sample(Sample::video_keyframe(vec![0; 100], 40));
        builder.add_video_sample(Sample::video_frame(vec![0; 100], 40));
        builder.add_audio_sample(Sample::audio(vec![0; 100], 20));
        builder.add_audio_sample(Sample::audio(vec![0; 100], 30));

        assert_eq!(builder.video_duration(), 80);
        assert_eq!(builder.audio_duration(), 50);
    }

    #[test]
    fn test_sample_flags() {
        let sync_sample = Sample::video_keyframe(vec![0], 40);
        let flags = sample_flags_to_u32(&sync_sample);
        assert!(flags & 0x02000000 != 0); // independent
        assert!(flags & 0x00010000 == 0); // not non-sync

        let non_sync = Sample::video_frame(vec![0], 40);
        let flags = sample_flags_to_u32(&non_sync);
        assert!(flags & 0x01000000 != 0); // depends on others
        assert!(flags & 0x00010000 != 0); // non-sync
    }

    #[test]
    fn test_empty_segment() {
        let builder = MediaSegmentBuilder::new();
        assert!(builder.is_empty());

        // Empty segment should still be buildable
        let segment = builder.build().unwrap();
        // Should only have moof header, no mdat data
        assert!(segment.windows(4).any(|w| w == b"moof"));
    }

    #[test]
    fn test_tfhd_all_fields() {
        let tfhd = TfhdBox::new(1)
            .with_default_sample_duration(1000)
            .with_default_sample_size(500)
            .with_default_sample_flags(0x02000000);

        let encoded = tfhd.encode().unwrap();
        assert!(encoded.len() > 20);
    }

    #[test]
    fn test_trun_with_first_sample_flags() {
        let mut trun = TrunBox::new(100).with_first_sample_flags(0x02000000);

        trun.add_sample(TrunSample {
            duration: Some(40),
            size: Some(1000),
            flags: None, // Will use first_sample_flags
            composition_time_offset: None,
        });

        let encoded = trun.encode().unwrap();
        assert!(encoded.windows(4).any(|w| w == b"trun"));
    }
}
