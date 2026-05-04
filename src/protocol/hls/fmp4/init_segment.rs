//! Init Segment Builder (ftyp + moov)
//!
//! The Init Segment contains all the metadata needed to decode the media:
//! - ftyp: File type and compatibility
//! - moov: Movie metadata (tracks, codecs, etc.)
//!
//! This segment is sent once at the beginning of the stream.

use super::boxes::{BoxType, FourCC, Mp4Box, FullBox, ContainerBox, writer};
use super::DEFAULT_TIMESCALE;
use crate::media::CodecType;
use std::io::{self, Write};
use std::time::Duration;

/// Track configuration for Init Segment
#[derive(Debug, Clone)]
pub struct TrackConfig {
    /// Track ID (must be unique)
    pub track_id: u32,

    /// Codec type
    pub codec: CodecType,

    /// Timescale for this track
    pub timescale: u32,

    /// Duration (0 for live streams)
    pub duration: Duration,

    /// Language code (ISO 639-2/T, e.g., "und" for undetermined)
    pub language: String,

    // Video specific
    pub width: u16,
    pub height: u16,

    // Audio specific
    pub sample_rate: u32,
    pub channels: u8,
}

impl Default for TrackConfig {
    fn default() -> Self {
        Self {
            track_id: 1,
            codec: CodecType::H264,
            timescale: DEFAULT_TIMESCALE,
            duration: Duration::ZERO,
            language: "und".to_string(),
            width: 0,
            height: 0,
            sample_rate: 0,
            channels: 0,
        }
    }
}

impl TrackConfig {
    /// Create a video track configuration
    pub fn video(track_id: u32, codec: CodecType, width: u16, height: u16) -> Self {
        Self {
            track_id,
            codec,
            width,
            height,
            ..Default::default()
        }
    }

    /// Create an audio track configuration
    pub fn audio(track_id: u32, codec: CodecType, sample_rate: u32, channels: u8) -> Self {
        Self {
            track_id,
            codec,
            sample_rate,
            channels,
            ..Default::default()
        }
    }

    /// Set timescale
    pub fn with_timescale(mut self, timescale: u32) -> Self {
        self.timescale = timescale;
        self
    }

    /// Set language
    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = lang.into();
        self
    }

    /// Check if this is a video track
    pub fn is_video(&self) -> bool {
        matches!(self.codec, CodecType::H264 | CodecType::H265 | CodecType::AV1 | CodecType::VP8 | CodecType::VP9)
    }

    /// Check if this is an audio track
    pub fn is_audio(&self) -> bool {
        matches!(self.codec, CodecType::AAC | CodecType::Opus | CodecType::Mp3 | CodecType::G711A | CodecType::G711U)
    }
}

// ============================================================================
// ftyp Box (File Type Box)
// ============================================================================

/// ftyp box - File Type Box
pub struct FtypBox {
    major_brand: FourCC,
    minor_version: u32,
    compatible_brands: Vec<FourCC>,
}

impl FtypBox {
    pub fn new(major_brand: FourCC, minor_version: u32) -> Self {
        Self {
            major_brand,
            minor_version,
            compatible_brands: Vec::new(),
        }
    }

    pub fn add_compatible_brand(mut self, brand: FourCC) -> Self {
        self.compatible_brands.push(brand);
        self
    }

    /// Create a standard ftyp for fMP4
    pub fn for_fmp4() -> Self {
        Self::new(FourCC::ISO5, 512)
            .add_compatible_brand(FourCC::ISO5)
            .add_compatible_brand(FourCC::ISO6)
            .add_compatible_brand(FourCC::MP41)
    }
}

impl Mp4Box for FtypBox {
    fn box_type(&self) -> BoxType {
        BoxType::Ftyp
    }

    fn box_size(&self) -> u64 {
        8 + // header
        4 + // major_brand
        4 + // minor_version
        (self.compatible_brands.len() as u64) * 4
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer::write_fourcc(writer, self.major_brand)?;
        writer::write_u32(writer, self.minor_version)?;
        for brand in &self.compatible_brands {
            writer::write_fourcc(writer, *brand)?;
        }
        Ok(())
    }
}

// ============================================================================
// mvhd Box (Movie Header Box)
// ============================================================================

/// mvhd box - Movie Header Box
pub struct MvhdBox {
    creation_time: u64,
    modification_time: u64,
    timescale: u32,
    duration: u64,
    next_track_id: u32,
}

impl MvhdBox {
    pub fn new(timescale: u32, duration: u64, next_track_id: u32) -> Self {
        Self {
            creation_time: 0, // 0 means unknown
            modification_time: 0,
            timescale,
            duration,
            next_track_id,
        }
    }
}

impl FullBox for MvhdBox {
    fn version(&self) -> u8 {
        1 // Use version 1 for 64-bit times
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for MvhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Mvhd
    }

    fn box_size(&self) -> u64 {
        // Version 1 = 108 bytes total
        4 + // size
        4 + // type
        4 + // version + flags
        8 + // creation_time
        8 + // modification_time
        4 + // timescale
        8 + // duration
        4 + // rate
        2 + // volume
        2 + // reserved
        8 + // reserved
        36 + // matrix
        24 + // pre_defined
        4 // next_track_id
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        // Version 1 fields
        writer::write_u64(writer, self.creation_time)?;
        writer::write_u64(writer, self.modification_time)?;
        writer::write_u32(writer, self.timescale)?;
        writer::write_u64(writer, self.duration)?;

        // Rate (1.0 = 0x00010000)
        writer::write_u32(writer, 0x00010000)?;

        // Volume (1.0 = 0x0100)
        writer::write_u16(writer, 0x0100)?;

        // Reserved
        writer::write_u16(writer, 0)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;

        // Matrix (identity matrix)
        writer::write_u32(writer, 0x00010000)?; // a
        writer::write_u32(writer, 0)?; // b
        writer::write_u32(writer, 0)?; // u
        writer::write_u32(writer, 0)?; // c
        writer::write_u32(writer, 0x00010000)?; // d
        writer::write_u32(writer, 0)?; // v
        writer::write_u32(writer, 0)?; // tx
        writer::write_u32(writer, 0)?; // ty
        writer::write_u32(writer, 0x40000000)?; // w

        // Pre_defined (6 * 4 = 24 bytes)
        for _ in 0..6 {
            writer::write_u32(writer, 0)?;
        }

        // Next track ID
        writer::write_u32(writer, self.next_track_id)?;

        Ok(())
    }
}

// ============================================================================
// tkhd Box (Track Header Box)
// ============================================================================

/// tkhd box - Track Header Box
pub struct TkhdBox {
    track_id: u32,
    duration: u64,
    width: u16,
    height: u16,
    is_audio: bool,
}

impl TkhdBox {
    pub fn new(track_id: u32, duration: u64, width: u16, height: u16, is_audio: bool) -> Self {
        Self {
            track_id,
            duration,
            width,
            height,
            is_audio,
        }
    }
}

impl FullBox for TkhdBox {
    fn version(&self) -> u8 {
        1
    }

    fn flags(&self) -> u32 {
        // Track enabled, in movie, in preview
        0x000003
    }
}

impl Mp4Box for TkhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Tkhd
    }

    fn box_size(&self) -> u64 {
        4 + 4 + // header
        4 + // version + flags
        8 + 8 + // creation/modification time
        4 + 8 + // track_id + reserved + duration
        8 + // reserved
        2 + 2 + 2 + // layer + alternate_group + volume
        2 + // reserved
        36 + // matrix
        4 + 4 // width + height (16.16 fixed)
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        // Creation/modification time (0 = unknown)
        writer::write_u64(writer, 0)?;
        writer::write_u64(writer, 0)?;

        // Track ID and reserved
        writer::write_u32(writer, self.track_id)?;
        writer::write_u32(writer, 0)?;

        // Duration
        writer::write_u64(writer, self.duration)?;

        // Reserved
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;

        // Layer, alternate_group
        writer::write_u16(writer, 0)?;
        writer::write_u16(writer, 0)?;

        // Volume (0x0100 for audio, 0 for video)
        writer::write_u16(writer, if self.is_audio { 0x0100 } else { 0 })?;

        // Reserved
        writer::write_u16(writer, 0)?;

        // Matrix (identity)
        writer::write_u32(writer, 0x00010000)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0x00010000)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0x40000000)?;

        // Width and height (16.16 fixed, 0 for audio)
        let w = (self.width as u32) << 16;
        let h = (self.height as u32) << 16;
        writer::write_u32(writer, w)?;
        writer::write_u32(writer, h)?;

        Ok(())
    }
}

// ============================================================================
// mdhd Box (Media Header Box)
// ============================================================================

/// mdhd box - Media Header Box
pub struct MdhdBox {
    timescale: u32,
    duration: u64,
    language: String,
}

impl MdhdBox {
    pub fn new(timescale: u32, duration: u64, language: String) -> Self {
        Self {
            timescale,
            duration,
            language,
        }
    }
}

impl FullBox for MdhdBox {
    fn version(&self) -> u8 {
        1
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for MdhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Mdhd
    }

    fn box_size(&self) -> u64 {
        4 + 4 + // header
        4 + // version + flags
        8 + 8 + // creation/modification time
        4 + 8 + // timescale + duration
        2 + 2 // language + pre_defined
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        writer::write_u64(writer, 0)?; // creation_time
        writer::write_u64(writer, 0)?; // modification_time
        writer::write_u32(writer, self.timescale)?;
        writer::write_u64(writer, self.duration)?;
        writer::write_lang(writer, &self.language)?;
        writer::write_u16(writer, 0)?; // pre_defined

        Ok(())
    }
}

// ============================================================================
// hdlr Box (Handler Reference Box)
// ============================================================================

/// hdlr box - Handler Reference Box
pub struct HdlrBox {
    handler_type: FourCC,
    name: String,
}

impl HdlrBox {
    pub fn video() -> Self {
        Self {
            handler_type: FourCC::from_str("vide"),
            name: "VideoHandler".to_string(),
        }
    }

    pub fn audio() -> Self {
        Self {
            handler_type: FourCC::from_str("soun"),
            name: "SoundHandler".to_string(),
        }
    }
}

impl FullBox for HdlrBox {
    fn version(&self) -> u8 {
        0
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for HdlrBox {
    fn box_type(&self) -> BoxType {
        BoxType::Hdlr
    }

    fn box_size(&self) -> u64 {
        4 + 4 + // header
        4 + // version + flags
        4 + // pre_defined
        4 + // handler_type
        12 + // reserved
        (self.name.len() + 1) as u64 // name + null terminator
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        writer::write_u32(writer, 0)?; // pre_defined
        writer::write_fourcc(writer, self.handler_type)?;
        writer::write_u32(writer, 0)?; // reserved
        writer::write_u32(writer, 0)?; // reserved
        writer::write_u32(writer, 0)?; // reserved
        writer::write_bytes(writer, self.name.as_bytes())?;
        writer::write_u8(writer, 0)?; // null terminator

        Ok(())
    }
}

// ============================================================================
// vmhd Box (Video Media Header Box)
// ============================================================================

pub struct VmhdBox;

impl FullBox for VmhdBox {
    fn version(&self) -> u8 {
        0
    }

    fn flags(&self) -> u32 {
        1 // flags = 1
    }
}

impl Mp4Box for VmhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Vmhd
    }

    fn box_size(&self) -> u64 {
        4 + 4 + 4 + 2 + 2 + 2 + 2 // header + version/flags + graphicsmode + opcolor
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        writer::write_u16(writer, 0)?; // graphicsmode
        writer::write_u16(writer, 0)?; // opcolor
        writer::write_u16(writer, 0)?;
        writer::write_u16(writer, 0)?;

        Ok(())
    }
}

// ============================================================================
// smhd Box (Sound Media Header Box)
// ============================================================================

pub struct SmhdBox;

impl FullBox for SmhdBox {
    fn version(&self) -> u8 {
        0
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for SmhdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Smhd
    }

    fn box_size(&self) -> u64 {
        4 + 4 + 4 + 2 + 2 // header + version/flags + balance + reserved
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;

        writer::write_u16(writer, 0)?; // balance
        writer::write_u16(writer, 0)?; // reserved

        Ok(())
    }
}

// ============================================================================
// dinf Box (Data Information Box)
// ============================================================================

pub struct DinfBox;

impl Mp4Box for DinfBox {
    fn box_type(&self) -> BoxType {
        BoxType::Dinf
    }

    fn box_size(&self) -> u64 {
        36 // Container with dref
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        // dref box
        writer::write_u32(writer, 28)?; // dref size
        writer::write_fourcc(writer, FourCC::from_str("dref"))?;
        writer::write_u32(writer, 0)?; // version + flags
        writer::write_u32(writer, 1)?; // entry_count

        // url box (self-contained)
        writer::write_u32(writer, 12)?; // url size
        writer::write_fourcc(writer, FourCC::URL)?;
        writer::write_u32(writer, 1)?; // version + flags = 1 (self-contained)

        Ok(())
    }
}

// ============================================================================
// Sample Entry Boxes (avc1, hvc1, mp4a)
// ============================================================================

/// avc1 sample entry (H.264 video)
pub struct Avc1SampleEntry {
    width: u16,
    height: u16,
    avcc_data: Vec<u8>,
}

impl Avc1SampleEntry {
    pub fn new(width: u16, height: u16, avcc_data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            avcc_data,
        }
    }
}

impl Mp4Box for Avc1SampleEntry {
    fn box_type(&self) -> BoxType {
        BoxType::Custom(FourCC::AVCC_AS_AVC1)
    }

    fn box_size(&self) -> u64 {
        8 + // header
        6 + 2 + // reserved + data_reference_index
        16 + // pre_defined + reserved
        2 + 2 + // width + height
        4 + 4 + // horizresolution + vertresolution
        4 + // reserved
        2 + // frame_count
        32 + // compressorname
        2 + 2 + // depth + pre_defined
        8 + self.avcc_data.len() as u64 // avcC
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Reserved
        writer::write_u32(writer, 0)?;
        writer::write_u16(writer, 0)?;
        writer::write_u16(writer, 1)?; // data_reference_index

        // Pre_defined + reserved
        for _ in 0..4 {
            writer::write_u32(writer, 0)?;
        }

        // Width and height
        writer::write_u16(writer, self.width)?;
        writer::write_u16(writer, self.height)?;

        // Resolution (72 dpi = 0x00480000 in 16.16)
        writer::write_u32(writer, 0x00480000)?;
        writer::write_u32(writer, 0x00480000)?;

        // Reserved
        writer::write_u32(writer, 0)?;

        // Frame count
        writer::write_u16(writer, 1)?;

        // Compressor name (32 bytes, first byte is length)
        writer::write_u8(writer, 0)?;
        for _ in 0..31 {
            writer::write_u8(writer, 0)?;
        }

        // Depth
        writer::write_u16(writer, 0x0018)?; // 24-bit color
        writer::write_u16(writer, 0xFFFF)?; // pre_defined = -1

        // avcC box
        let avcc_size = 8 + self.avcc_data.len();
        writer::write_u32(writer, avcc_size as u32)?;
        writer::write_fourcc(writer, FourCC::AVCC)?;
        writer::write_bytes(writer, &self.avcc_data)?;

        Ok(())
    }
}

/// hvc1 sample entry (H.265/HEVC video)
pub struct Hvc1SampleEntry {
    width: u16,
    height: u16,
    hvcc_data: Vec<u8>,
}

impl Hvc1SampleEntry {
    pub fn new(width: u16, height: u16, hvcc_data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            hvcc_data,
        }
    }
}

impl Mp4Box for Hvc1SampleEntry {
    fn box_type(&self) -> BoxType {
        BoxType::Custom(FourCC::HVCC_AS_HVC1)
    }

    fn box_size(&self) -> u64 {
        8 + // header
        6 + 2 + // reserved + data_reference_index
        16 + // pre_defined + reserved
        2 + 2 + // width + height
        4 + 4 + // horizresolution + vertresolution
        4 + // reserved
        2 + // frame_count
        32 + // compressorname
        2 + 2 + // depth + pre_defined
        8 + self.hvcc_data.len() as u64 // hvcC
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Same structure as avc1
        writer::write_u32(writer, 0)?;
        writer::write_u16(writer, 0)?;
        writer::write_u16(writer, 1)?;

        for _ in 0..4 {
            writer::write_u32(writer, 0)?;
        }

        writer::write_u16(writer, self.width)?;
        writer::write_u16(writer, self.height)?;

        writer::write_u32(writer, 0x00480000)?;
        writer::write_u32(writer, 0x00480000)?;

        writer::write_u32(writer, 0)?;
        writer::write_u16(writer, 1)?;

        writer::write_u8(writer, 0)?;
        for _ in 0..31 {
            writer::write_u8(writer, 0)?;
        }

        writer::write_u16(writer, 0x0018)?;
        writer::write_u16(writer, 0xFFFF)?;

        // hvcC box
        let hvcc_size = 8 + self.hvcc_data.len();
        writer::write_u32(writer, hvcc_size as u32)?;
        writer::write_fourcc(writer, FourCC::HVCC)?;
        writer::write_bytes(writer, &self.hvcc_data)?;

        Ok(())
    }
}

/// mp4a sample entry (AAC audio)
pub struct Mp4aSampleEntry {
    sample_rate: u32,
    channels: u16,
    esds_data: Vec<u8>,
}

impl Mp4aSampleEntry {
    /// Create with default AAC-LC config
    pub fn aac_lc(sample_rate: u32, channels: u16) -> Self {
        // Simple ESDS for AAC-LC
        // This is a minimal implementation; real usage may need proper AudioSpecificConfig
        Self {
            sample_rate,
            channels,
            esds_data: vec![
                0x03, 0x19, // ES_Descriptor tag, length
                0x00, 0x01, // ES_ID
                0x00, // flags
                0x04, 0x11, // DecoderConfigDescriptor tag, length
                0x40, // objectTypeIndication = Audio ISO/IEC 14496-3
                0x15, // streamType = audio, upStream = 0, reserved = 1
                0x00, 0x00, 0x00, // bufferSizeDB
                0x00, 0x00, 0x00, 0x00, // maxBitrate
                0x00, 0x00, 0x00, 0x00, // avgBitrate
                0x05, 0x02, // DecoderSpecificInfo tag, length
                0x11, 0x90, // AudioSpecificConfig (AAC-LC, 44100Hz, stereo)
                0x06, 0x01, 0x02, // SLConfigDescriptor
            ],
        }
    }
}

impl Mp4Box for Mp4aSampleEntry {
    fn box_type(&self) -> BoxType {
        BoxType::Mp4a
    }

    fn box_size(&self) -> u64 {
        8 + // header
        6 + 2 + // reserved + data_reference_index
        8 + // reserved
        2 + 2 + 2 + 2 + // channelcount + samplesize + pre_defined + reserved
        4 + // samplerate (16.16)
        8 + self.esds_data.len() as u64 // esds
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer::write_u32(writer, 0)?;
        writer::write_u16(writer, 0)?;
        writer::write_u16(writer, 1)?; // data_reference_index

        // Reserved
        writer::write_u32(writer, 0)?;
        writer::write_u32(writer, 0)?;

        // Channel count and sample size
        writer::write_u16(writer, self.channels)?;
        writer::write_u16(writer, 16)?; // samplesize = 16 bits
        writer::write_u16(writer, 0)?; // pre_defined
        writer::write_u16(writer, 0)?; // reserved

        // Sample rate (16.16 fixed)
        let rate = (self.sample_rate as u32) << 16;
        writer::write_u32(writer, rate)?;

        // esds box
        let esds_size = 8 + self.esds_data.len();
        writer::write_u32(writer, esds_size as u32)?;
        writer::write_fourcc(writer, FourCC::ESDS)?;
        writer::write_bytes(writer, &self.esds_data)?;

        Ok(())
    }
}

// ============================================================================
// stsd Box (Sample Description Box)
// ============================================================================

/// stsd box wrapper
pub struct StsdBox {
    sample_entries: Vec<Box<dyn Mp4Box + Send>>,
}

impl StsdBox {
    pub fn new() -> Self {
        Self {
            sample_entries: Vec::new(),
        }
    }

    pub fn add_entry(mut self, entry: Box<dyn Mp4Box + Send>) -> Self {
        self.sample_entries.push(entry);
        self
    }
}

impl FullBox for StsdBox {
    fn version(&self) -> u8 {
        0
    }

    fn flags(&self) -> u32 {
        0
    }
}

impl Mp4Box for StsdBox {
    fn box_type(&self) -> BoxType {
        BoxType::Stsd
    }

    fn box_size(&self) -> u64 {
        let mut size = 4 + 4 + 4 + 4; // header + version/flags + entry_count
        for entry in &self.sample_entries {
            size += entry.box_size() as usize;
        }
        size as u64
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, self.sample_entries.len() as u32)?;
        for entry in &self.sample_entries {
            entry.write_box(writer)?;
        }
        Ok(())
    }
}

// ============================================================================
// Empty sample table boxes
// ============================================================================

/// stts box (Time-to-Sample) - empty for fMP4
pub struct SttsBox;

impl FullBox for SttsBox {
    fn version(&self) -> u8 { 0 }
    fn flags(&self) -> u32 { 0 }
}

impl Mp4Box for SttsBox {
    fn box_type(&self) -> BoxType { BoxType::Stts }
    fn box_size(&self) -> u64 { 16 }
    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, 0) // entry_count = 0
    }
}

/// stsc box (Sample-to-Chunk) - empty for fMP4
pub struct StscBox;

impl FullBox for StscBox {
    fn version(&self) -> u8 { 0 }
    fn flags(&self) -> u32 { 0 }
}

impl Mp4Box for StscBox {
    fn box_type(&self) -> BoxType { BoxType::Stsc }
    fn box_size(&self) -> u64 { 16 }
    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, 0)
    }
}

/// stsz box (Sample Size) - empty for fMP4
pub struct StszBox;

impl FullBox for StszBox {
    fn version(&self) -> u8 { 0 }
    fn flags(&self) -> u32 { 0 }
}

impl Mp4Box for StszBox {
    fn box_type(&self) -> BoxType { BoxType::Stsz }
    fn box_size(&self) -> u64 { 20 }
    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, 0)?; // sample_size = 0
        writer::write_u32(writer, 0) // sample_count = 0
    }
}

/// stco box (Chunk Offset) - empty for fMP4
pub struct StcoBox;

impl FullBox for StcoBox {
    fn version(&self) -> u8 { 0 }
    fn flags(&self) -> u32 { 0 }
}

impl Mp4Box for StcoBox {
    fn box_type(&self) -> BoxType { BoxType::Stco }
    fn box_size(&self) -> u64 { 16 }
    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, 0)
    }
}

// ============================================================================
// trex Box (Track Extends Box)
// ============================================================================

/// trex box - Track Extends Box
pub struct TrexBox {
    track_id: u32,
    default_sample_description_index: u32,
    default_sample_duration: u32,
    default_sample_size: u32,
    default_sample_flags: u32,
}

impl TrexBox {
    pub fn new(track_id: u32) -> Self {
        Self {
            track_id,
            default_sample_description_index: 1,
            default_sample_duration: 0,
            default_sample_size: 0,
            default_sample_flags: 0,
        }
    }
}

impl FullBox for TrexBox {
    fn version(&self) -> u8 { 0 }
    fn flags(&self) -> u32 { 0 }
}

impl Mp4Box for TrexBox {
    fn box_type(&self) -> BoxType { BoxType::Trex }
    fn box_size(&self) -> u64 { 32 }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.write_fullbox_header(writer)?;
        writer::write_u32(writer, self.track_id)?;
        writer::write_u32(writer, self.default_sample_description_index)?;
        writer::write_u32(writer, self.default_sample_duration)?;
        writer::write_u32(writer, self.default_sample_size)?;
        writer::write_u32(writer, self.default_sample_flags)?;
        Ok(())
    }
}

// ============================================================================
// InitSegmentBuilder
// ============================================================================

/// Builder for creating Init Segment
pub struct InitSegmentBuilder {
    tracks: Vec<TrackConfig>,
    timescale: u32,
    duration: u64,
}

impl Default for InitSegmentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl InitSegmentBuilder {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            timescale: DEFAULT_TIMESCALE,
            duration: 0,
        }
    }

    pub fn with_timescale(mut self, timescale: u32) -> Self {
        self.timescale = timescale;
        self
    }

    pub fn add_video_track(mut self, track_id: u32, codec: CodecType, width: u16, height: u16) -> Self {
        self.tracks.push(TrackConfig::video(track_id, codec, width, height));
        self
    }

    pub fn add_audio_track(mut self, track_id: u32, codec: CodecType, sample_rate: u32, channels: u8) -> Self {
        self.tracks.push(TrackConfig::audio(track_id, codec, sample_rate, channels));
        self
    }

    pub fn add_track(mut self, config: TrackConfig) -> Self {
        self.tracks.push(config);
        self
    }

    /// Build the init segment
    pub fn build(&self) -> io::Result<Vec<u8>> {
        let mut output = Vec::with_capacity(4096);

        // Write ftyp
        let ftyp = FtypBox::for_fmp4();
        ftyp.write_box(&mut output)?;

        // Build moov
        let moov = self.build_moov()?;
        moov.write_box(&mut output)?;

        Ok(output)
    }

    fn build_moov(&self) -> io::Result<ContainerBox> {
        let next_track_id = self.tracks.iter().map(|t| t.track_id + 1).max().unwrap_or(1);

        let mut moov = ContainerBox::new(BoxType::Moov);

        // mvhd
        moov.children.push(Box::new(MvhdBox::new(
            self.timescale,
            self.duration,
            next_track_id,
        )));

        // Tracks
        for track in &self.tracks {
            moov.children.push(Box::new(self.build_trak(track)?));
        }

        // mvex
        let mut mvex = ContainerBox::new(BoxType::Mvex);
        for track in &self.tracks {
            mvex.children.push(Box::new(TrexBox::new(track.track_id)));
        }
        moov.children.push(Box::new(mvex));

        Ok(moov)
    }

    fn build_trak(&self, track: &TrackConfig) -> io::Result<ContainerBox> {
        let duration = track.duration.as_millis() as u64 * self.timescale as u64 / 1000;

        let mut trak = ContainerBox::new(BoxType::Trak);

        // tkhd
        trak.children.push(Box::new(TkhdBox::new(
            track.track_id,
            duration,
            track.width,
            track.height,
            track.is_audio(),
        )));

        // mdia
        let mdia = self.build_mdia(track);
        trak.children.push(Box::new(mdia));

        Ok(trak)
    }

    fn build_mdia(&self, track: &TrackConfig) -> ContainerBox {
        let duration = track.duration.as_millis() as u64 * track.timescale as u64 / 1000;

        let mut mdia = ContainerBox::new(BoxType::Mdia);

        // mdhd
        mdia.children.push(Box::new(MdhdBox::new(
            track.timescale,
            duration,
            track.language.clone(),
        )));

        // hdlr
        mdia.children.push(Box::new(if track.is_video() {
            HdlrBox::video()
        } else {
            HdlrBox::audio()
        }));

        // minf
        let minf = self.build_minf(track);
        mdia.children.push(Box::new(minf));

        mdia
    }

    fn build_minf(&self, track: &TrackConfig) -> ContainerBox {
        let mut minf = ContainerBox::new(BoxType::Minf);

        // Media header
        if track.is_video() {
            minf.children.push(Box::new(VmhdBox));
        } else {
            minf.children.push(Box::new(SmhdBox));
        }

        // dinf
        minf.children.push(Box::new(DinfBox));

        // stbl
        let stbl = self.build_stbl(track);
        minf.children.push(Box::new(stbl));

        minf
    }

    fn build_stbl(&self, track: &TrackConfig) -> ContainerBox {
        let mut stbl = ContainerBox::new(BoxType::Stbl);

        // stsd with sample entry
        let mut stsd = StsdBox::new();

        if track.is_video() {
            match track.codec {
                CodecType::H264 => {
                    // Use default SPS/PPS if no data provided
                    let avcc_data = vec![
                        0x01, // configurationVersion
                        0x64, 0x00, 0x1F, // AVCProfileIndication, profile_compatibility, AVCLevelIndication
                        0xFF, // lengthSizeMinusOne = 3 (4-byte lengths)
                        0xE1, // numOfSequenceParameterSets = 1
                        0x00, 0x08, // SPS length
                        0x67, 0x64, 0x00, 0x1F, 0xAC, 0xD9, 0x40, 0x50, // SPS NAL
                        0x01, // numOfPictureParameterSets
                        0x00, 0x04, // PPS length
                        0x68, 0xEB, 0xE3, 0xCB, // PPS NAL
                    ];
                    stsd = stsd.add_entry(Box::new(Avc1SampleEntry::new(
                        track.width,
                        track.height,
                        avcc_data,
                    )));
                }
                CodecType::H265 => {
                    // Minimal hvcC
                    let hvcc_data = vec![
                        0x01, // configurationVersion
                        0x01, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                        0x00, // general_configuration
                        0x00, 0x00, 0x00, 0x00, 0x00, // min_spatial/timeline
                        0xB0, // parallelType
                        0x00, // chroma
                        0x00, 0x00, // bit_depth
                        0x00, 0x00, // avgFrameRate
                        0x0F, // constantFrameRate, numTemporalLayers, temporalIdNested, lengthSizeMinusOne
                        0x00, // numOfArrays
                    ];
                    stsd = stsd.add_entry(Box::new(Hvc1SampleEntry::new(
                        track.width,
                        track.height,
                        hvcc_data,
                    )));
                }
                _ => {
                    // Default to H.264
                    stsd = stsd.add_entry(Box::new(Avc1SampleEntry::new(
                        track.width,
                        track.height,
                        vec![0x01, 0x64, 0x00, 0x1F, 0xFF, 0xE1, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00],
                    )));
                }
            }
        } else {
            stsd = stsd.add_entry(Box::new(Mp4aSampleEntry::aac_lc(
                track.sample_rate,
                track.channels as u16,
            )));
        }

        stbl.children.push(Box::new(stsd));
        stbl.children.push(Box::new(SttsBox));
        stbl.children.push(Box::new(StscBox));
        stbl.children.push(Box::new(StszBox));
        stbl.children.push(Box::new(StcoBox));

        stbl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ftyp_box() {
        let ftyp = FtypBox::for_fmp4();
        assert_eq!(ftyp.box_type(), BoxType::Ftyp);
        assert!(ftyp.box_size() > 0);

        let encoded = ftyp.encode().unwrap();
        // ftyp box: 8 (header) + 4 (brand) + 4 (version) + 12 (3 brands) = 28 bytes = 0x1C
        assert!(encoded.starts_with(b"\x00\x00\x00\x1Cftyp")); // size + 'ftyp'
    }

    #[test]
    fn test_mvhd_box() {
        let mvhd = MvhdBox::new(1000, 0, 2);
        assert_eq!(mvhd.box_type(), BoxType::Mvhd);

        let encoded = mvhd.encode().unwrap();
        assert!(encoded.len() > 100);
    }

    #[test]
    fn test_tkhd_box() {
        let tkhd = TkhdBox::new(1, 0, 1920, 1080, false);
        let encoded = tkhd.encode().unwrap();

        // Check track_id in encoded data (at offset ~20 for version 1)
        assert!(encoded.len() > 90);
    }

    #[test]
    fn test_init_segment_builder() {
        let builder = InitSegmentBuilder::new()
            .add_video_track(1, CodecType::H264, 1920, 1080)
            .add_audio_track(2, CodecType::AAC, 48000, 2);

        let init = builder.build().unwrap();

        // Should start with ftyp (28 bytes = 0x1C)
        assert!(init.starts_with(b"\x00\x00\x00\x1Cftyp"));

        // Should contain moov
        assert!(init.windows(4).any(|w| w == b"moov"));
    }

    #[test]
    fn test_init_segment_minimal() {
        let builder = InitSegmentBuilder::new()
            .add_video_track(1, CodecType::H264, 1280, 720);

        let init = builder.build().unwrap();

        // Verify structure
        assert!(init.windows(4).any(|w| w == b"ftyp"));
        assert!(init.windows(4).any(|w| w == b"moov"));
        assert!(init.windows(4).any(|w| w == b"trak"));
        assert!(init.windows(4).any(|w| w == b"avc1"));
    }

    #[test]
    fn test_track_config() {
        let video = TrackConfig::video(1, CodecType::H264, 1920, 1080);
        assert!(video.is_video());
        assert!(!video.is_audio());

        let audio = TrackConfig::audio(2, CodecType::AAC, 48000, 2);
        assert!(audio.is_audio());
        assert!(!audio.is_video());
    }

    #[test]
    fn test_mp4a_sample_entry() {
        let mp4a = Mp4aSampleEntry::aac_lc(48000, 2);
        let encoded = mp4a.encode().unwrap();

        // Should contain mp4a type
        assert!(encoded.windows(4).any(|w| w == b"mp4a"));
    }
}
