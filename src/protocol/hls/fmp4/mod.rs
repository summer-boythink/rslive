//! fMP4 (Fragmented MP4 / CMAF) Muxer
//!
//! This module implements a complete fMP4 muxer for LL-HLS (Low-Latency HLS).
//! fMP4 is based on the ISO Base Media File Format (ISO/IEC 14496-12).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       Fmp4Muxer                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │   MediaFrame ──→ InitSegmentBuilder ──→ Init Segment        │
//! │                  (ftyp + moov)           (一次)              │
//! │                                                              │
//! │   MediaFrame ──→ MediaSegmentBuilder ──→ Media Segment      │
//! │                  (moof + mdat)           (每个段)            │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Concepts
//!
//! - **Box (Atom)**: Basic unit of storage, with type (4CC) + size + data
//! - **Init Segment**: ftyp + moov, contains codec configuration
//! - **Media Segment**: moof + mdat, contains actual media data
//! - **Track**: Audio or video stream within the file
//! - **Sample**: Single frame of audio or video
//!
//! # Comparison with MPEG-TS
//!
//! | Feature        | MPEG-TS       | fMP4          |
//! |---------------|---------------|---------------|
//! | Overhead      | ~14%          | ~3%           |
//! | Latency       | 10-30s        | 2-5s (LL-HLS) |
//! | Complexity    | High          | Medium        |
//! | Codec Config  | In-band       | Init segment  |
//! | Random Access | Any packet    | moof boundary |

mod boxes;
mod init_segment;
mod media_segment;
mod muxer;

pub use boxes::writer;
pub use boxes::{BoxType, ContainerBox, FourCC, FreeBox, Mp4Box};
pub use init_segment::{InitSegmentBuilder, TrackConfig};
pub use media_segment::{
    MdatBox, MediaSegmentBuilder, MfhdBox, MoofBox, Sample, Sample as MediaSample, TfdtBox,
    TfhdBox, TrafBox, TrunBox, TrunSample,
};
pub use muxer::{Fmp4Muxer, Fmp4MuxerBuilder, Fmp4MuxerConfig, Fmp4MuxerError};

/// Default timescale (1000 = milliseconds)
pub const DEFAULT_TIMESCALE: u32 = 1000;

/// Video track ID
pub const VIDEO_TRACK_ID: u32 = 1;

/// Audio track ID
pub const AUDIO_TRACK_ID: u32 = 2;

/// Convert nanoseconds to timescale units
#[inline]
pub fn nanos_to_timescale(nanos: u64, timescale: u32) -> u64 {
    if timescale == 0 {
        return 0;
    }
    // Use 128-bit arithmetic to avoid overflow
    ((nanos as u128) * (timescale as u128) / 1_000_000_000) as u64
}

/// Convert timescale units to nanoseconds
#[inline]
pub fn timescale_to_nanos(units: u64, timescale: u32) -> u64 {
    if timescale == 0 {
        return 0;
    }
    ((units as u128) * 1_000_000_000 / (timescale as u128)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timescale_conversion() {
        // 1 second = 1000 units at 1000 timescale
        assert_eq!(nanos_to_timescale(1_000_000_000, 1000), 1000);

        // Round trip
        let ns = 5_500_000_000u64; // 5.5 seconds
        let units = nanos_to_timescale(ns, 1000);
        let back = timescale_to_nanos(units, 1000);
        assert_eq!(back, ns);
    }

    #[test]
    fn test_timescale_zero_safety() {
        assert_eq!(nanos_to_timescale(1_000_000_000, 0), 0);
        assert_eq!(timescale_to_nanos(1000, 0), 0);
    }
}
