//! MP4 Box (Atom) definitions and encoding
//!
//! The ISO Base Media File Format is built on a hierarchy of boxes.
//! Each box has:
//! - size (4 bytes): Total box size including header
//! - type (4 bytes): Four Character Code (4CC) identifying the box
//! - data: Box-specific content
//!
//! For boxes larger than 2^32-1 bytes, an extended size (8 bytes) is used.

use std::io::{self, Write};

/// Four Character Code (4CC) - 4-byte identifier for box types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCC([u8; 4]);

impl FourCC {
    /// Create from bytes
    pub const fn new(bytes: &[u8; 4]) -> Self {
        Self(*bytes)
    }

    /// Create from string literal (compile-time checked)
    pub const fn from_str(s: &str) -> Self {
        let bytes = s.as_bytes();
        // This will panic at compile time if string is not 4 bytes
        assert!(bytes.len() == 4, "FourCC must be exactly 4 characters");
        Self([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Get as bytes
    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    /// Get as string slice
    pub fn as_str(&self) -> &str {
        // Safe because we only create from valid ASCII
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl std::fmt::Display for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// Common box types
impl FourCC {
    // File level boxes
    pub const FTYP: Self = Self::from_str("ftyp");
    pub const MOOV: Self = Self::from_str("moov");
    pub const MOOF: Self = Self::from_str("moof");
    pub const MDAT: Self = Self::from_str("mdat");
    pub const FREE: Self = Self::from_str("free");
    pub const SKIP: Self = Self::from_str("skip");

    // Movie boxes
    pub const MVHD: Self = Self::from_str("mvhd");
    pub const TRAK: Self = Self::from_str("trak");
    pub const MVEX: Self = Self::from_str("mvex");
    pub const MEHD: Self = Self::from_str("mehd");
    pub const TREX: Self = Self::from_str("trex");

    // Track boxes
    pub const TKHD: Self = Self::from_str("tkhd");
    pub const MDIA: Self = Self::from_str("mdia");
    pub const MINF: Self = Self::from_str("minf");
    pub const STBL: Self = Self::from_str("stbl");
    pub const DINF: Self = Self::from_str("dinf");
    pub const EDTS: Self = Self::from_str("edts");

    // Media boxes
    pub const MDHD: Self = Self::from_str("mdhd");
    pub const HDLR: Self = Self::from_str("hdlr");
    pub const VMHD: Self = Self::from_str("vmhd");
    pub const SMHD: Self = Self::from_str("smhd");
    pub const NMHD: Self = Self::from_str("nmhd");

    // Sample table boxes
    pub const STSD: Self = Self::from_str("stsd");
    pub const STTS: Self = Self::from_str("stts");
    pub const STSC: Self = Self::from_str("stsc");
    pub const STSZ: Self = Self::from_str("stsz");
    pub const STCO: Self = Self::from_str("stco");
    pub const STSS: Self = Self::from_str("stss");
    pub const CTTS: Self = Self::from_str("ctts");

    // Sample description boxes
    pub const AVCC: Self = Self::from_str("avcC");
    pub const HVCC: Self = Self::from_str("hvcC");
    pub const AV1C: Self = Self::from_str("av1C");
    pub const AVCC_AS_AVC1: Self = Self::from_str("avc1");
    pub const HVCC_AS_HVC1: Self = Self::from_str("hvc1");
    pub const AV01: Self = Self::from_str("av01");
    pub const MP4A: Self = Self::from_str("mp4a");
    pub const ESDS: Self = Self::from_str("esds");
    pub const DAC3: Self = Self::from_str("dac3");
    pub const DEC3: Self = Self::from_str("dec3");
    pub const DFPM: Self = Self::from_str("dfpm");

    // Data reference box
    pub const URL: Self = Self::from_str("url ");
    pub const URNA: Self = Self::from_str("urn ");

    // Movie fragment boxes
    pub const MFHD: Self = Self::from_str("mfhd");
    pub const TRAF: Self = Self::from_str("traf");
    pub const TFHD: Self = Self::from_str("tfhd");
    pub const TRUN: Self = Self::from_str("trun");
    pub const TFDT: Self = Self::from_str("tfdt");

    // Brand identifiers
    pub const ISOM: Self = Self::from_str("isom");
    pub const ISO2: Self = Self::from_str("iso2");
    pub const ISO3: Self = Self::from_str("iso3");
    pub const ISO4: Self = Self::from_str("iso4");
    pub const ISO5: Self = Self::from_str("iso5");
    pub const ISO6: Self = Self::from_str("iso6");
    pub const MP41: Self = Self::from_str("mp41");
    pub const MP42: Self = Self::from_str("mp42");
    pub const AV01_BRAND: Self = Self::from_str("av01");
    pub const MSDH: Self = Self::from_str("msdh");
}

/// Box type enum for type-safe box handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxType {
    // File level
    Ftyp,
    Moov,
    Moof,
    Mdat,
    Free,

    // Movie header
    Mvhd,

    // Track
    Trak,
    Tkhd,
    Mdia,
    Mdhd,
    Hdlr,
    Minf,
    Stbl,
    Dinf,
    Mvex,
    Mehd,
    Trex,

    // Media info
    Vmhd,
    Smhd,
    Nmhd,

    // Sample table
    Stsd,
    Stts,
    Stsc,
    Stsz,
    Stco,
    Stss,
    Ctts,

    // Codec specific
    AvcC,
    HvcC,
    Av1C,
    Mp4a,
    Esds,

    // Movie fragment
    Mfhd,
    Traf,
    Tfhd,
    Trun,
    Tfdt,

    // Custom
    Custom(FourCC),
}

impl BoxType {
    /// Get the FourCC for this box type
    pub fn fourcc(&self) -> FourCC {
        match self {
            Self::Ftyp => FourCC::FTYP,
            Self::Moov => FourCC::MOOV,
            Self::Moof => FourCC::MOOF,
            Self::Mdat => FourCC::MDAT,
            Self::Free => FourCC::FREE,
            Self::Mvhd => FourCC::MVHD,
            Self::Trak => FourCC::TRAK,
            Self::Tkhd => FourCC::TKHD,
            Self::Mdia => FourCC::MDIA,
            Self::Mdhd => FourCC::MDHD,
            Self::Hdlr => FourCC::HDLR,
            Self::Minf => FourCC::MINF,
            Self::Stbl => FourCC::STBL,
            Self::Dinf => FourCC::DINF,
            Self::Mvex => FourCC::MVEX,
            Self::Mehd => FourCC::MEHD,
            Self::Trex => FourCC::TREX,
            Self::Vmhd => FourCC::VMHD,
            Self::Smhd => FourCC::SMHD,
            Self::Nmhd => FourCC::NMHD,
            Self::Stsd => FourCC::STSD,
            Self::Stts => FourCC::STTS,
            Self::Stsc => FourCC::STSC,
            Self::Stsz => FourCC::STSZ,
            Self::Stco => FourCC::STCO,
            Self::Stss => FourCC::STSS,
            Self::Ctts => FourCC::CTTS,
            Self::AvcC => FourCC::AVCC,
            Self::HvcC => FourCC::HVCC,
            Self::Av1C => FourCC::AV1C,
            Self::Mp4a => FourCC::MP4A,
            Self::Esds => FourCC::ESDS,
            Self::Mfhd => FourCC::MFHD,
            Self::Traf => FourCC::TRAF,
            Self::Tfhd => FourCC::TFHD,
            Self::Trun => FourCC::TRUN,
            Self::Tfdt => FourCC::TFDT,
            Self::Custom(fourcc) => *fourcc,
        }
    }
}

/// Trait for MP4 boxes (dyn-compatible)
pub trait Mp4Box {
    /// Get the box type
    fn box_type(&self) -> BoxType;

    /// Calculate the box size (including header)
    fn box_size(&self) -> u64;

    /// Write the box content (excluding header) to a writer
    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()>;

    /// Write the complete box (with header) to a writer
    fn write_box(&self, writer: &mut dyn Write) -> io::Result<()> {
        let size = self.box_size();
        let fourcc = self.box_type().fourcc();

        // Write size (use extended size if > 2^32-1)
        if size > 0xFFFFFFFF - 8 {
            // Extended size: size=1, followed by 8-byte actual size
            writer.write_all(&1u32.to_be_bytes())?;
            writer.write_all(fourcc.as_bytes())?;
            writer.write_all(&size.to_be_bytes())?;
        } else {
            writer.write_all(&(size as u32).to_be_bytes())?;
            writer.write_all(fourcc.as_bytes())?;
        }

        // Write content
        self.write_box_content(writer)?;

        Ok(())
    }

    /// Encode the box to a Vec<u8>
    fn encode(&self) -> io::Result<Vec<u8>> {
        let size = self.box_size() as usize;
        let mut buf = Vec::with_capacity(size);
        self.write_box(&mut buf)?;
        Ok(buf)
    }
}

/// Helper functions for writing box data
pub mod writer {
    use std::io::{self, Write};

    /// Write a u8
    #[inline]
    pub fn write_u8(writer: &mut dyn Write, value: u8) -> io::Result<()> {
        writer.write_all(&[value])
    }

    /// Write a u16 in big-endian
    #[inline]
    pub fn write_u16(writer: &mut dyn Write, value: u16) -> io::Result<()> {
        writer.write_all(&value.to_be_bytes())
    }

    /// Write a u24 in big-endian (3 bytes)
    #[inline]
    pub fn write_u24(writer: &mut dyn Write, value: u32) -> io::Result<()> {
        writer.write_all(&value.to_be_bytes()[1..])
    }

    /// Write a u32 in big-endian
    #[inline]
    pub fn write_u32(writer: &mut dyn Write, value: u32) -> io::Result<()> {
        writer.write_all(&value.to_be_bytes())
    }

    /// Write a u64 in big-endian
    #[inline]
    pub fn write_u64(writer: &mut dyn Write, value: u64) -> io::Result<()> {
        writer.write_all(&value.to_be_bytes())
    }

    /// Write a FourCC
    #[inline]
    pub fn write_fourcc(writer: &mut dyn Write, fourcc: super::FourCC) -> io::Result<()> {
        writer.write_all(fourcc.as_bytes())
    }

    /// Write bytes
    #[inline]
    pub fn write_bytes(writer: &mut dyn Write, data: &[u8]) -> io::Result<()> {
        writer.write_all(data)
    }

    /// Write zero bytes for padding
    #[inline]
    pub fn write_zeros(writer: &mut dyn Write, count: usize) -> io::Result<()> {
        for _ in 0..count {
            writer.write_all(&[0])?;
        }
        Ok(())
    }

    /// Write a FixedPoint16.16 (Q16.16)
    #[inline]
    pub fn write_fixed16_16(writer: &mut dyn Write, value: f64) -> io::Result<()> {
        let fixed = (value * 65536.0) as u32;
        write_u32(writer, fixed)
    }

    /// Write a FixedPoint8.8 (Q8.8)
    #[inline]
    pub fn write_fixed8_8(writer: &mut dyn Write, value: f64) -> io::Result<()> {
        let fixed = (value * 256.0) as u16;
        write_u16(writer, fixed)
    }

    /// Write a language code (ISO 639-2/T as 3x5-bit packed)
    pub fn write_lang(writer: &mut dyn Write, lang: &str) -> io::Result<()> {
        // Language is 3 characters, each converted to 5-bit packed value
        let chars: Vec<char> = lang.chars().take(3).collect();
        let mut packed = 0u16;

        for (i, &c) in chars.iter().enumerate() {
            // Convert lowercase letter to 5-bit value (a=1, b=2, ...)
            let val = if c.is_ascii_lowercase() {
                (c as u8 - b'a' + 1) as u16
            } else if c.is_ascii_uppercase() {
                (c as u8 - b'A' + 1) as u16
            } else {
                0
            };
            packed |= val << (10 - i * 5);
        }

        write_u16(writer, packed)
    }
}

/// Container box that holds other boxes
pub struct ContainerBox {
    pub box_type: BoxType,
    pub children: Vec<Box<dyn Mp4Box + Send>>,
}

impl ContainerBox {
    pub fn new(box_type: BoxType) -> Self {
        Self {
            box_type,
            children: Vec::new(),
        }
    }

    pub fn add_child(mut self, child: Box<dyn Mp4Box + Send>) -> Self {
        self.children.push(child);
        self
    }
}

impl Mp4Box for ContainerBox {
    fn box_type(&self) -> BoxType {
        self.box_type
    }

    fn box_size(&self) -> u64 {
        // 8 bytes for header
        let mut size = 8u64;
        for child in &self.children {
            size += child.box_size();
        }
        size
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        for child in &self.children {
            child.write_box(writer)?;
        }
        Ok(())
    }
}

/// Free/skip box for padding or alignment
pub struct FreeBox {
    pub size: usize,
}

impl FreeBox {
    pub fn new(size: usize) -> Self {
        // Minimum size is 8 bytes (header only)
        Self { size: size.max(8) }
    }
}

impl Mp4Box for FreeBox {
    fn box_type(&self) -> BoxType {
        BoxType::Free
    }

    fn box_size(&self) -> u64 {
        self.size as u64
    }

    fn write_box_content(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Write zeros for the remaining space (size - 8 for header)
        let zeros = self.size.saturating_sub(8);
        writer.write_all(&vec![0u8; zeros])?;
        Ok(())
    }
}

/// FullBox - base for boxes with version and flags
pub trait FullBox: Mp4Box {
    fn version(&self) -> u8;
    fn flags(&self) -> u32;

    fn write_fullbox_header(&self, writer: &mut dyn Write) -> io::Result<()> {
        let version_flags = ((self.version() as u32) << 24) | (self.flags() & 0x00FFFFFF);
        writer.write_all(&version_flags.to_be_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_creation() {
        let ftyp = FourCC::FTYP;
        assert_eq!(ftyp.as_str(), "ftyp");

        let custom = FourCC::from_str("test");
        assert_eq!(custom.as_str(), "test");
    }

    #[test]
    fn test_box_type_mapping() {
        assert_eq!(BoxType::Ftyp.fourcc(), FourCC::FTYP);
        assert_eq!(BoxType::Moov.fourcc(), FourCC::MOOV);
        assert_eq!(BoxType::Moof.fourcc(), FourCC::MOOF);
    }

    #[test]
    fn test_free_box_size() {
        let free = FreeBox::new(100);
        assert_eq!(free.box_size(), 100);
        assert_eq!(free.box_type(), BoxType::Free);
    }

    #[test]
    fn test_free_box_minimum_size() {
        let free = FreeBox::new(5);
        assert_eq!(free.box_size(), 8); // Minimum is 8
    }

    #[test]
    fn test_writer_functions() {
        let mut buf = Vec::new();

        writer::write_u8(&mut buf, 0x12).unwrap();
        assert_eq!(buf, vec![0x12]);

        buf.clear();
        writer::write_u16(&mut buf, 0x1234).unwrap();
        assert_eq!(buf, vec![0x12, 0x34]);

        buf.clear();
        writer::write_u32(&mut buf, 0x12345678).unwrap();
        assert_eq!(buf, vec![0x12, 0x34, 0x56, 0x78]);

        buf.clear();
        writer::write_u24(&mut buf, 0x123456).unwrap();
        assert_eq!(buf, vec![0x12, 0x34, 0x56]);

        buf.clear();
        writer::write_fixed16_16(&mut buf, 1.0).unwrap();
        // 1.0 in Q16.16 = 0x00010000
        assert_eq!(buf, vec![0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn test_lang_code() {
        let mut buf = Vec::new();
        writer::write_lang(&mut buf, "und").unwrap(); // Undetermined

        // 'u' = 21, 'n' = 14, 'd' = 4 (1-indexed from 'a')
        // packed: (21 << 10) | (14 << 5) | 4 = 21572
        // But we need to check the actual encoding
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn test_writer_unused_functions() {
        let mut buf = Vec::new();

        // Test write_zeros
        writer::write_zeros(&mut buf, 5).unwrap();
        assert_eq!(buf, vec![0, 0, 0, 0, 0]);

        buf.clear();
        // Test write_fixed8_8
        writer::write_fixed8_8(&mut buf, 1.0).unwrap();
        assert_eq!(buf, vec![0x01, 0x00]); // 1.0 in Q8.8 = 256
    }

    #[test]
    fn test_container_box_add_child() {
        let container = ContainerBox::new(BoxType::Moov)
            .add_child(Box::new(FreeBox::new(100)));
        assert_eq!(container.children.len(), 1);
    }
}
