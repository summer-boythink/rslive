//! TS Packet implementation (188-byte fixed format)
//!
//! Structure of a TS packet:
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ Header (4 bytes)                                              │
//! ├──────────────────────────────────────────────────────────────┤
//! │  0  │  1  │  2  │  3  │
//! │ sync│ PID │ PID │ CC  │
//! │  47 │ flags+PID │ counter│
//! ├──────────────────────────────────────────────────────────────┤
//! │ Adaptation Field (optional, 0-183 bytes)                     │
//! ├──────────────────────────────────────────────────────────────┤
//! │ Payload (remaining bytes)                                     │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use super::{TS_PACKET_SIZE, TS_SYNC_BYTE, nanos_to_27mhz};

/// TS Packet Header (4 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsPacketHeader {
    /// Transport error indicator (1 bit)
    /// Set when the packet is erroneous
    pub transport_error_indicator: bool,

    /// Payload unit start indicator (1 bit)
    /// Set for the first packet of each PES unit or section
    pub payload_unit_start_indicator: bool,

    /// Transport priority (1 bit)
    /// Higher priority than other packets with same PID
    pub transport_priority: bool,

    /// Packet identifier (13 bits)
    /// 0x0000 = PAT, 0x0001-0x000F = reserved, others for streams
    pub pid: u16,

    /// Transport scrambling control (2 bits)
    /// 00 = not scrambled
    pub transport_scrambling_control: u8,

    /// Adaptation field control (2 bits)
    /// 00 = reserved, 01 = payload only, 10 = adaptation only, 11 = both
    pub adaptation_field_control: u8,

    /// Continuity counter (4 bits)
    /// Increments for each packet with same PID (wraps 0-15)
    pub continuity_counter: u8,
}

impl Default for TsPacketHeader {
    fn default() -> Self {
        Self {
            transport_error_indicator: false,
            payload_unit_start_indicator: false,
            transport_priority: false,
            pid: 0,
            transport_scrambling_control: 0,
            adaptation_field_control: 1, // Payload only
            continuity_counter: 0,
        }
    }
}

impl TsPacketHeader {
    /// Create a new header for the given PID
    pub fn new(pid: u16) -> Self {
        Self {
            pid,
            ..Default::default()
        }
    }

    /// Set payload unit start indicator
    pub fn with_pusi(mut self, pusi: bool) -> Self {
        self.payload_unit_start_indicator = pusi;
        self
    }

    /// Set continuity counter
    pub fn with_cc(mut self, cc: u8) -> Self {
        self.continuity_counter = cc & 0x0F;
        self
    }

    /// Set adaptation field control
    pub fn with_afc(mut self, afc: u8) -> Self {
        self.adaptation_field_control = afc & 0x03;
        self
    }

    /// Encode header to bytes
    pub fn encode(&self) -> [u8; 4] {
        let mut buf = [0u8; 4];

        // Byte 0: Sync byte
        buf[0] = TS_SYNC_BYTE;

        // Byte 1: TEI + PUSI + TP + PID[12:8]
        buf[1] = (if self.transport_error_indicator {
            0x80
        } else {
            0
        }) | (if self.payload_unit_start_indicator {
            0x40
        } else {
            0
        }) | (if self.transport_priority { 0x20 } else { 0 })
            | ((self.pid >> 8) as u8 & 0x1F);

        // Byte 2: PID[7:0]
        buf[2] = self.pid as u8;

        // Byte 3: TSC + AFC + CC
        buf[3] = ((self.transport_scrambling_control & 0x03) << 6)
            | ((self.adaptation_field_control & 0x03) << 4)
            | (self.continuity_counter & 0x0F);

        buf
    }

    /// Decode header from bytes
    pub fn decode(data: &[u8; 4]) -> Result<Self, TsPacketError> {
        if data[0] != TS_SYNC_BYTE {
            return Err(TsPacketError::InvalidSyncByte(data[0]));
        }

        Ok(Self {
            transport_error_indicator: (data[1] & 0x80) != 0,
            payload_unit_start_indicator: (data[1] & 0x40) != 0,
            transport_priority: (data[1] & 0x20) != 0,
            pid: ((data[1] as u16 & 0x1F) << 8) | (data[2] as u16),
            transport_scrambling_control: (data[3] >> 6) & 0x03,
            adaptation_field_control: (data[3] >> 4) & 0x03,
            continuity_counter: data[3] & 0x0F,
        })
    }
}

/// Adaptation Field for PCR and stuffing
#[derive(Debug, Clone)]
pub struct AdaptationField {
    /// Discontinuity indicator
    pub discontinuity_indicator: bool,

    /// Random access indicator (set for keyframes)
    pub random_access_indicator: bool,

    /// Elementary stream priority indicator
    pub elementary_stream_priority_indicator: bool,

    /// PCR flag
    pub pcr_flag: bool,

    /// OPCR flag
    pub opcr_flag: bool,

    /// Splicing point flag
    pub splicing_point_flag: bool,

    /// Transport private data flag
    pub transport_private_data_flag: bool,

    /// Adaptation field extension flag
    pub adaptation_field_extension_flag: bool,

    /// Program Clock Reference (33 bits base + 9 bits extension)
    pub pcr: Option<PcrValue>,

    /// Original Program Clock Reference
    pub opcr: Option<PcrValue>,

    /// Transport private data
    pub private_data: Vec<u8>,

    /// Number of stuffing bytes
    pub stuffing_bytes: usize,
}

impl Default for AdaptationField {
    fn default() -> Self {
        Self {
            discontinuity_indicator: false,
            random_access_indicator: false,
            elementary_stream_priority_indicator: false,
            pcr_flag: false,
            opcr_flag: false,
            splicing_point_flag: false,
            transport_private_data_flag: false,
            adaptation_field_extension_flag: false,
            pcr: None,
            opcr: None,
            private_data: Vec::new(),
            stuffing_bytes: 0,
        }
    }
}

impl AdaptationField {
    /// Create a new adaptation field
    pub fn new() -> Self {
        Self::default()
    }

    /// Create adaptation field with PCR
    pub fn with_pcr(pcr: PcrValue) -> Self {
        Self {
            pcr_flag: true,
            pcr: Some(pcr),
            ..Default::default()
        }
    }

    /// Create adaptation field for random access (keyframe)
    pub fn with_random_access() -> Self {
        Self {
            random_access_indicator: true,
            ..Default::default()
        }
    }

    /// Create adaptation field with stuffing bytes
    pub fn with_stuffing(stuffing_bytes: usize) -> Self {
        Self {
            stuffing_bytes,
            ..Default::default()
        }
    }

    /// Calculate the length of this adaptation field
    pub fn len(&self) -> usize {
        let mut len = 1; // Length field itself

        // Flags byte
        if self.discontinuity_indicator
            || self.random_access_indicator
            || self.elementary_stream_priority_indicator
            || self.pcr_flag
            || self.opcr_flag
            || self.splicing_point_flag
            || self.transport_private_data_flag
            || self.adaptation_field_extension_flag
        {
            len += 1;
        }

        // PCR (6 bytes)
        if self.pcr_flag {
            len += 6;
        }

        // OPCR (6 bytes)
        if self.opcr_flag {
            len += 6;
        }

        // Private data
        if !self.private_data.is_empty() {
            len += 1 + self.private_data.len();
        }

        // Stuffing
        len += self.stuffing_bytes;

        len
    }

    /// Check if adaptation field is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 1 && self.stuffing_bytes == 0
    }

    /// Encode adaptation field to bytes
    pub fn encode(&self) -> Vec<u8> {
        let length = self.len();
        let mut buf = Vec::with_capacity(length);

        // Length field (1 byte) - length of fields after this byte
        buf.push((length - 1) as u8);

        // Only encode flags and data if there is any
        if length > 1 {
            // Flags byte
            let flags = (if self.discontinuity_indicator {
                0x80
            } else {
                0
            }) | (if self.random_access_indicator {
                0x40
            } else {
                0
            }) | (if self.elementary_stream_priority_indicator {
                0x20
            } else {
                0
            }) | (if self.pcr_flag { 0x10 } else { 0 })
                | (if self.opcr_flag { 0x08 } else { 0 })
                | (if self.splicing_point_flag { 0x04 } else { 0 })
                | (if self.transport_private_data_flag {
                    0x02
                } else {
                    0
                })
                | (if self.adaptation_field_extension_flag {
                    0x01
                } else {
                    0
                });
            buf.push(flags);

            // PCR (6 bytes)
            if self.pcr_flag {
                if let Some(ref pcr) = self.pcr {
                    buf.extend_from_slice(&pcr.encode());
                } else {
                    buf.extend_from_slice(&[0; 6]);
                }
            }

            // OPCR (6 bytes)
            if self.opcr_flag {
                if let Some(ref opcr) = self.opcr {
                    buf.extend_from_slice(&opcr.encode());
                } else {
                    buf.extend_from_slice(&[0; 6]);
                }
            }

            // Private data
            if !self.private_data.is_empty() {
                buf.push(self.private_data.len() as u8);
                buf.extend_from_slice(&self.private_data);
            }

            // Stuffing bytes (0xFF)
            for _ in 0..self.stuffing_bytes {
                buf.push(0xFF);
            }
        }

        buf
    }
}

/// PCR (Program Clock Reference) value
///
/// PCR consists of:
/// - Base: 33 bits, measured in 90kHz clock
/// - Extension: 9 bits, measured in 27MHz clock (0-299)
///
/// PCR = base * 300 + extension
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcrValue {
    /// Base part (33 bits, 90kHz)
    pub base: u64,
    /// Extension part (9 bits, 27MHz/300)
    pub extension: u16,
}

impl PcrValue {
    /// Create PCR from nanoseconds
    pub fn from_nanos(nanos: u64) -> Self {
        let ticks_27mhz = nanos_to_27mhz(nanos);
        Self {
            base: ticks_27mhz / 300,
            extension: (ticks_27mhz % 300) as u16,
        }
    }

    /// Create PCR from 90kHz clock units
    pub fn from_90khz(clock_90khz: u64) -> Self {
        Self {
            base: clock_90khz & 0x1FFFFFFFF, // 33 bits
            extension: 0,
        }
    }

    /// Create PCR from 27MHz clock units
    pub fn from_27mhz(clock_27mhz: u64) -> Self {
        Self {
            base: clock_27mhz / 300,
            extension: (clock_27mhz % 300) as u16,
        }
    }

    /// Convert to nanoseconds
    pub fn to_nanos(&self) -> u64 {
        let ticks_27mhz = self.base * 300 + (self.extension as u64);
        (ticks_27mhz as u128 * 1_000_000_000 / 27_000_000) as u64
    }

    /// Encode PCR to 6 bytes
    ///
    /// Format:
    /// ```text
    /// Byte 0: PCR[32:28] + '1'
    /// Byte 1: PCR[27:20]
    /// Byte 2: PCR[19:12] + '1'
    /// Byte 3: PCR[11:4]
    /// Byte 4: PCR[3:0] + Ext[8:5] + '1'
    /// Byte 5: Ext[4:0] + '11111' (reserved)
    /// ```
    pub fn encode(&self) -> [u8; 6] {
        let base = self.base & 0x1FFFFFFFF; // 33 bits
        let ext = self.extension & 0x1FF; // 9 bits

        let mut buf = [0u8; 6];

        // PCR base (33 bits) with marker bits
        buf[0] = ((base >> 25) as u8) & 0xFF;
        buf[1] = ((base >> 17) as u8) & 0xFF;
        buf[2] = ((base >> 9) as u8) & 0xFF;
        buf[3] = ((base >> 1) as u8) & 0xFF;
        buf[4] = (((base & 0x01) << 7) | 0x7E | (((ext >> 8) & 0x01) as u64)) as u8;
        buf[5] = (ext & 0xFF) as u8;

        buf
    }
}

/// TS Packet (188 bytes)
#[derive(Debug, Clone)]
pub struct TsPacket {
    /// Packet header
    pub header: TsPacketHeader,

    /// Optional adaptation field
    pub adaptation_field: Option<AdaptationField>,

    /// Payload data
    pub payload: Vec<u8>,
}

impl TsPacket {
    /// Create a new TS packet with the given PID
    pub fn new(pid: u16) -> Self {
        Self {
            header: TsPacketHeader::new(pid),
            adaptation_field: None,
            payload: Vec::new(),
        }
    }

    /// Set payload unit start indicator
    pub fn with_pusi(mut self, pusi: bool) -> Self {
        self.header.payload_unit_start_indicator = pusi;
        self
    }

    /// Set continuity counter
    pub fn with_cc(mut self, cc: u8) -> Self {
        self.header.continuity_counter = cc & 0x0F;
        self
    }

    /// Set adaptation field
    pub fn with_adaptation_field(mut self, af: AdaptationField) -> Self {
        self.adaptation_field = Some(af);
        self.header.adaptation_field_control = if self.payload.is_empty() { 2 } else { 3 };
        self
    }

    /// Set payload
    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        if self.adaptation_field.is_some() {
            self.header.adaptation_field_control = 3;
        } else {
            self.header.adaptation_field_control = 1;
        }
        self
    }

    /// Calculate the maximum payload size for this packet
    pub fn max_payload_size(&self) -> usize {
        let mut available = TS_PACKET_SIZE - 4; // Header is 4 bytes

        if let Some(ref af) = self.adaptation_field {
            available -= af.len();
        }

        available
    }

    /// Encode the packet to 188 bytes
    pub fn encode(&self) -> Result<[u8; TS_PACKET_SIZE], TsPacketError> {
        let mut buf = [0u8; TS_PACKET_SIZE];
        let mut pos = 0;

        // Encode header
        let header_bytes = self.header.encode();
        buf[pos..pos + 4].copy_from_slice(&header_bytes);
        pos += 4;

        // Encode adaptation field if present
        if let Some(ref af) = self.adaptation_field {
            let af_bytes = af.encode();
            if pos + af_bytes.len() > TS_PACKET_SIZE {
                return Err(TsPacketError::AdaptationFieldTooLarge(af_bytes.len()));
            }
            buf[pos..pos + af_bytes.len()].copy_from_slice(&af_bytes);
            pos += af_bytes.len();
        }

        // Add payload
        if !self.payload.is_empty() {
            if pos + self.payload.len() > TS_PACKET_SIZE {
                return Err(TsPacketError::PayloadTooLarge(
                    self.payload.len(),
                    TS_PACKET_SIZE - pos,
                ));
            }
            buf[pos..pos + self.payload.len()].copy_from_slice(&self.payload);
        }

        Ok(buf)
    }

    /// Create a packet with padding to fill 188 bytes
    pub fn encode_with_padding(&self) -> [u8; TS_PACKET_SIZE] {
        let mut packet = self.clone();

        // Calculate how much space we have
        let used = 4 + packet.adaptation_field.as_ref().map_or(0, |af| af.len());
        let remaining = TS_PACKET_SIZE - used;

        // If payload is smaller than remaining, we need to add stuffing
        if packet.payload.len() < remaining {
            let stuffing_needed = remaining - packet.payload.len();

            if stuffing_needed > 0 {
                // Add stuffing to adaptation field
                let af = packet
                    .adaptation_field
                    .take()
                    .unwrap_or_else(AdaptationField::new);
                let mut af = af;
                af.stuffing_bytes += stuffing_needed;
                packet.adaptation_field = Some(af);
                packet.header.adaptation_field_control =
                    if packet.payload.is_empty() { 2 } else { 3 };
            }
        }

        // Encode (should not fail with padding)
        packet.encode().unwrap_or_else(|_| {
            // Fallback: create empty packet with just header
            let mut buf = [0u8; TS_PACKET_SIZE];
            let header = packet.header.encode();
            buf[0..4].copy_from_slice(&header);
            // Fill rest with stuffing
            for b in &mut buf[4..] {
                *b = 0xFF;
            }
            buf
        })
    }
}

/// Errors for TS packet operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum TsPacketError {
    #[error("Invalid sync byte: expected 0x47, got 0x{0:02X}")]
    InvalidSyncByte(u8),

    #[error("Adaptation field too large: {0} bytes")]
    AdaptationFieldTooLarge(usize),

    #[error("Payload too large: {0} bytes, maximum is {1}")]
    PayloadTooLarge(usize, usize),

    #[error("Invalid packet size: expected 188, got {0}")]
    InvalidPacketSize(usize),
}

/// Continuity counter manager for multiple PIDs
#[derive(Debug, Default)]
pub struct ContinuityCounter {
    counters: std::collections::HashMap<u16, u8>,
}

impl ContinuityCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the next continuity counter for a PID
    pub fn next(&mut self, pid: u16) -> u8 {
        let counter = self.counters.entry(pid).or_insert(0);
        let current = *counter;
        *counter = (current + 1) & 0x0F;
        current
    }

    /// Get current counter without incrementing
    pub fn current(&self, pid: u16) -> u8 {
        self.counters.get(&pid).copied().unwrap_or(0)
    }

    /// Reset counter for a PID
    pub fn reset(&mut self, pid: u16) {
        self.counters.insert(pid, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode() {
        let header = TsPacketHeader::new(0x1000)
            .with_pusi(true)
            .with_cc(5)
            .with_afc(3);

        let encoded = header.encode();
        let decoded = TsPacketHeader::decode(&encoded).unwrap();

        assert_eq!(decoded.pid, 0x1000);
        assert!(decoded.payload_unit_start_indicator);
        assert_eq!(decoded.continuity_counter, 5);
        assert_eq!(decoded.adaptation_field_control, 3);
    }

    #[test]
    fn test_header_pid_range() {
        // Test minimum PID
        let header = TsPacketHeader::new(0x0000);
        assert_eq!(header.pid, 0x0000);

        // Test maximum PID (13 bits = 0x1FFF)
        let header = TsPacketHeader::new(0x1FFF);
        assert_eq!(header.pid, 0x1FFF);

        // PID is stored as-is, but encoding truncates to 13 bits
        let header = TsPacketHeader::new(0xFFFF);
        assert_eq!(header.pid, 0xFFFF); // Stored value

        // Verify encoding truncates to 13 bits
        let encoded = header.encode();
        let encoded_pid = ((encoded[1] as u16 & 0x1F) << 8) | (encoded[2] as u16);
        assert_eq!(encoded_pid, 0x1FFF); // Truncated in encoding
    }

    #[test]
    fn test_pcr_encoding() {
        // PCR from 90kHz clock (1 second = 90000)
        let pcr = PcrValue::from_90khz(90_000);
        assert_eq!(pcr.base, 90_000);

        let encoded = pcr.encode();
        assert_eq!(encoded.len(), 6);

        // Verify marker bits are set
        assert_eq!(encoded[4] & 0x7E, 0x7E);
    }

    #[test]
    fn test_pcr_from_nanos() {
        // 1 second in nanoseconds
        let pcr = PcrValue::from_nanos(1_000_000_000);

        // Should equal 27,000,000 ticks at 27MHz
        let total_ticks = pcr.base * 300 + pcr.extension as u64;
        assert_eq!(total_ticks, 27_000_000);
    }

    #[test]
    fn test_adaptation_field_pcr() {
        let pcr = PcrValue::from_nanos(1_000_000_000);
        let af = AdaptationField::with_pcr(pcr);

        assert!(af.pcr_flag);
        assert!(af.pcr.is_some());

        // Length should be: 1 (length) + 1 (flags) + 6 (PCR) = 8
        assert_eq!(af.len(), 8);

        let encoded = af.encode();
        assert_eq!(encoded.len(), 8);
    }

    #[test]
    fn test_adaptation_field_stuffing() {
        let af = AdaptationField::with_stuffing(10);
        assert_eq!(af.stuffing_bytes, 10);

        // Length should be: 1 (length) + 10 (stuffing) = 11
        assert_eq!(af.len(), 11);
    }

    #[test]
    fn test_ts_packet_basic() {
        let packet = TsPacket::new(0x100).with_payload(vec![1, 2, 3, 4, 5]);

        let encoded = packet.encode().unwrap();
        assert_eq!(encoded.len(), TS_PACKET_SIZE);

        // First byte should be sync byte
        assert_eq!(encoded[0], TS_SYNC_BYTE);

        // PID should be 0x100
        let pid = ((encoded[1] as u16 & 0x1F) << 8) | (encoded[2] as u16);
        assert_eq!(pid, 0x100);
    }

    #[test]
    fn test_ts_packet_with_pcr() {
        let pcr = PcrValue::from_nanos(500_000_000); // 500ms
        let af = AdaptationField::with_pcr(pcr);

        let packet = TsPacket::new(0x100)
            .with_pusi(true)
            .with_adaptation_field(af)
            .with_payload(vec![1, 2, 3, 4]);

        let encoded = packet.encode().unwrap();
        assert_eq!(encoded.len(), TS_PACKET_SIZE);

        // Check adaptation field control = 3 (both adaptation and payload)
        assert_eq!((encoded[3] >> 4) & 0x03, 3);
    }

    #[test]
    fn test_ts_packet_with_padding() {
        let packet = TsPacket::new(0x100).with_payload(vec![1, 2, 3]); // Small payload

        let encoded = packet.encode_with_padding();
        assert_eq!(encoded.len(), TS_PACKET_SIZE);

        // Should have sync byte
        assert_eq!(encoded[0], TS_SYNC_BYTE);
    }

    #[test]
    fn test_continuity_counter() {
        let mut cc = ContinuityCounter::new();

        // Should start at 0
        assert_eq!(cc.next(0x100), 0);
        assert_eq!(cc.next(0x100), 1);
        assert_eq!(cc.next(0x100), 2);

        // Different PID should have separate counter
        assert_eq!(cc.next(0x101), 0);
        assert_eq!(cc.next(0x101), 1);

        // First PID should continue
        assert_eq!(cc.next(0x100), 3);

        // Should wrap around at 16
        for _ in 0..12 {
            cc.next(0x100);
        }
        assert_eq!(cc.next(0x100), 0); // Wrapped
    }

    #[test]
    fn test_packet_size_exactly_188() {
        // Create a packet that fills exactly 188 bytes
        let packet = TsPacket::new(0x100);
        let encoded = packet.encode_with_padding();

        assert_eq!(encoded.len(), 188);

        // Verify sync byte at start
        assert_eq!(encoded[0], 0x47);
    }

    #[test]
    fn test_random_access_indicator() {
        let af = AdaptationField::with_random_access();
        assert!(af.random_access_indicator);

        let encoded = af.encode();
        // Flags byte should have bit 6 set (0x40)
        assert!(encoded.len() >= 2);
        assert_eq!(encoded[1] & 0x40, 0x40);
    }
}
