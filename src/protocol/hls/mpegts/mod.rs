//! MPEG-TS (Transport Stream) Muxer
//!
//! This module implements a complete MPEG-TS muxer for HLS segmentation.
//! It follows ISO/IEC 13818-1 specification.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       TsMuxer                               │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │   MediaFrame ──→ PesEncoder ──→ TsPacketizer ──→ TS Output │
//! │                                                              │
//! │   ┌──────────┐   ┌──────────┐   ┌──────────────┐           │
//! │   │ PAT Gen  │   │ PMT Gen  │   │ PCR Handler  │           │
//! │   └──────────┘   └──────────┘   └──────────────┘           │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Concepts
//!
//! - **TS Packet**: Fixed 188-byte packet with header and payload
//! - **PAT**: Program Association Table (PID 0x0000), maps programs to PMT
//! - **PMT**: Program Map Table, describes streams in a program
//! - **PES**: Packetized Elementary Stream, encapsulates audio/video data
//! - **PCR**: Program Clock Reference, for synchronization

mod ts_packet;
mod pat;
mod pmt;
mod pes;
mod muxer;

pub use ts_packet::{TsPacket, TsPacketHeader, AdaptationField, PcrValue, ContinuityCounter};
pub use pat::PatGenerator;
pub use pmt::{PmtGenerator, StreamInfo, StreamType};
pub use pes::PesEncoder;
pub use muxer::{TsMuxer, TsMuxerConfig, TsMuxerError, TsSegmentInfo, create_ts_segment};

/// TS packet size in bytes (fixed by specification)
pub const TS_PACKET_SIZE: usize = 188;

/// TS sync byte (every packet starts with this)
pub const TS_SYNC_BYTE: u8 = 0x47;

/// PID for Program Association Table
pub const PAT_PID: u16 = 0x0000;

/// Default PID for Program Map Table
pub const DEFAULT_PMT_PID: u16 = 0x1000;

/// Default PID for video stream
pub const DEFAULT_VIDEO_PID: u16 = 0x0100;

/// Default PID for audio stream
pub const DEFAULT_AUDIO_PID: u16 = 0x0101;

/// Default program number
pub const DEFAULT_PROGRAM_NUMBER: u16 = 0x0001;

/// PCR interval in milliseconds (recommended: every 100ms)
pub const DEFAULT_PCR_INTERVAL_MS: u64 = 100;

/// System clock frequency (27 MHz)
pub const SYSTEM_CLOCK_FREQUENCY: u64 = 27_000_000;

/// Program clock frequency (90 kHz)
pub const PROGRAM_CLOCK_FREQUENCY: u64 = 90_000;

/// Convert nanoseconds to 90kHz clock units
#[inline]
pub fn nanos_to_90khz(nanos: u64) -> u64 {
    // Use 128-bit arithmetic to avoid overflow
    ((nanos as u128) * (PROGRAM_CLOCK_FREQUENCY as u128) / 1_000_000_000) as u64
}

/// Convert nanoseconds to 27MHz clock units (for PCR)
#[inline]
pub fn nanos_to_27mhz(nanos: u64) -> u64 {
    ((nanos as u128) * (SYSTEM_CLOCK_FREQUENCY as u128) / 1_000_000_000) as u64
}

/// Calculate CRC32 for PAT/PMT tables (IEEE 802.3 polynomial)
pub fn calculate_crc32(data: &[u8]) -> u32 {
    // CRC-32 polynomial: 0x04C11DB7
    // Standard table-based implementation
    static CRC32_TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = (i as u32) << 24;
            let mut j = 0;
            while j < 8 {
                if crc & 0x80000000 != 0 {
                    crc = (crc << 1) ^ 0x04C11DB7;
                } else {
                    crc <<= 1;
                }
                j += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };

    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        let index = ((crc >> 24) ^ (byte as u32)) as usize;
        crc = (crc << 8) ^ CRC32_TABLE[index];
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nanosecond_conversions() {
        // 1 second = 90,000 units at 90kHz
        assert_eq!(nanos_to_90khz(1_000_000_000), 90_000);

        // 1 second = 27,000,000 units at 27MHz
        assert_eq!(nanos_to_27mhz(1_000_000_000), 27_000_000);

        // 100ms
        assert_eq!(nanos_to_90khz(100_000_000), 9_000);
    }

    #[test]
    fn test_crc32() {
        // Known test vector: empty data
        let crc = calculate_crc32(&[]);
        assert_eq!(crc, 0xFFFFFFFF);

        // Test with some data
        let data = [0x00, 0xB0, 0x0D, 0x00, 0x01, 0xC1, 0x00, 0x00];
        let crc = calculate_crc32(&data);
        // CRC should be non-zero
        assert_ne!(crc, 0);
    }
}
