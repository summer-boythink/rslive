//! PAT (Program Association Table) Generator
//!
//! PAT is the first table decoded by a receiver. It maps program numbers
//! to the PID of their corresponding PMT.
//!
//! Structure:
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ PAT Section                                                   │
//! ├──────────────────────────────────────────────────────────────┤
//! │ table_id (1B)                 = 0x00                         │
//! │ section_syntax_indicator (1b) = 1                            │
//! │ '0' (1b)                      = 0                            │
//! │ reserved (2b)                 = 11                           │
//! │ section_length (12b)          = N                            │
//! │ transport_stream_id (2B)                                     │
//! │ reserved (2b)                 = 11                           │
//! │ version_number (5b)                                          │
//! │ current_next_indicator (1b)   = 1                            │
//! │ section_number (1B)           = 0                            │
//! │ last_section_number (1B)      = 0                            │
//! │ ┌──────────────────────────────────────────────────────────┐ │
//! │ │ Program Info (4B each)                                   │ │
//! │ │   program_number (2B)                                    │ │
//! │ │   reserved (3b) | program_map_PID (13b)                  │ │
//! │ └──────────────────────────────────────────────────────────┘ │
//! │ CRC32 (4B)                                                   │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use super::{
    calculate_crc32, TsPacket, TsPacketHeader, ContinuityCounter,
    PAT_PID, TS_PACKET_SIZE,
};

/// Program information entry in PAT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramInfo {
    /// Program number (0 = network information table, 1+ = programs)
    pub program_number: u16,

    /// PID of the PMT for this program
    pub pmt_pid: u16,
}

impl ProgramInfo {
    /// Create a new program info entry
    pub fn new(program_number: u16, pmt_pid: u16) -> Self {
        Self {
            program_number,
            pmt_pid: pmt_pid & 0x1FFF, // 13 bits
        }
    }
}

/// PAT Generator
#[derive(Debug, Clone)]
pub struct PatGenerator {
    /// Transport stream ID (arbitrary, usually 0x0001)
    transport_stream_id: u16,

    /// Version number (incremented when PAT changes)
    version_number: u8,

    /// Programs in this transport stream
    programs: Vec<ProgramInfo>,
}

impl Default for PatGenerator {
    fn default() -> Self {
        Self {
            transport_stream_id: 0x0001,
            version_number: 0,
            programs: Vec::new(),
        }
    }
}

impl PatGenerator {
    /// Create a new PAT generator
    pub fn new() -> Self {
        Self::default()
    }

    /// Set transport stream ID
    pub fn with_transport_stream_id(mut self, id: u16) -> Self {
        self.transport_stream_id = id;
        self
    }

    /// Set version number
    pub fn with_version(mut self, version: u8) -> Self {
        self.version_number = version & 0x1F;
        self
    }

    /// Add a program
    pub fn add_program(&mut self, program_number: u16, pmt_pid: u16) {
        let info = ProgramInfo::new(program_number, pmt_pid);
        // Check if program already exists
        if let Some(existing) = self.programs.iter_mut().find(|p| p.program_number == program_number) {
            *existing = info;
        } else {
            self.programs.push(info);
        }
    }

    /// Remove a program
    pub fn remove_program(&mut self, program_number: u16) {
        self.programs.retain(|p| p.program_number != program_number);
    }

    /// Clear all programs
    pub fn clear(&mut self) {
        self.programs.clear();
    }

    /// Increment version number (called when PAT changes)
    pub fn increment_version(&mut self) {
        self.version_number = (self.version_number + 1) & 0x1F;
    }

    /// Calculate the section length (excluding CRC)
    fn section_length(&self) -> usize {
        // header: 5 bytes (table_id through last_section_number)
        // programs: 4 bytes each
        // crc: 4 bytes
        5 + (self.programs.len() * 4) + 4
    }

    /// Generate PAT section bytes (without TS packet wrapper)
    pub fn generate_section(&self) -> Vec<u8> {
        let section_length = self.section_length();

        // Total section size is section_length + 3 (table_id through section_length)
        let mut section = Vec::with_capacity(3 + section_length);

        // table_id
        section.push(0x00);

        // section_syntax_indicator (1) + '0' (1) + reserved (2) + section_length high 4 bits
        let section_len_high = ((section_length >> 8) & 0x0F) as u8;
        section.push(0xB0 | section_len_high); // 1011 0000 | section_length[11:8]

        // section_length low 8 bits
        section.push((section_length & 0xFF) as u8);

        // transport_stream_id (2 bytes)
        section.push((self.transport_stream_id >> 8) as u8);
        section.push((self.transport_stream_id & 0xFF) as u8);

        // reserved (2) + version_number (5) + current_next_indicator (1)
        // current_next_indicator = 1 (this PAT is current)
        let version_byte = 0xC0 | ((self.version_number & 0x1F) << 1) | 0x01;
        section.push(version_byte);

        // section_number
        section.push(0x00);

        // last_section_number
        section.push(0x00);

        // Program info loop
        for program in &self.programs {
            // program_number (2 bytes)
            section.push((program.program_number >> 8) as u8);
            section.push((program.program_number & 0xFF) as u8);

            // reserved (3) + program_map_PID (13)
            let pid_high = ((program.pmt_pid >> 8) & 0x1F) as u8;
            section.push(0xE0 | pid_high); // 1110 0000 | PID[12:8]
            section.push((program.pmt_pid & 0xFF) as u8);
        }

        // Calculate and append CRC32
        let crc = calculate_crc32(&section);
        section.push(((crc >> 24) & 0xFF) as u8);
        section.push(((crc >> 16) & 0xFF) as u8);
        section.push(((crc >> 8) & 0xFF) as u8);
        section.push((crc & 0xFF) as u8);

        section
    }

    /// Generate PAT as TS packets
    pub fn generate_ts_packets(&self, cc: &mut ContinuityCounter) -> Vec<TsPacket> {
        let section = self.generate_section();
        let mut packets = Vec::new();

        // How many bytes can fit in each packet's payload
        // First packet: 1 byte pointer_field + section data
        // Subsequent packets: just section data

        let mut remaining = section.as_slice();
        let mut first_packet = true;

        while !remaining.is_empty() {
            // Calculate available payload space
            let payload_capacity = TS_PACKET_SIZE - 4; // 184 bytes after header

            let payload = if first_packet {
                // First packet needs pointer_field
                let mut payload = Vec::with_capacity(payload_capacity);
                payload.push(0x00); // pointer_field = 0 (section starts immediately)

                let available = payload_capacity - 1;
                let take = remaining.len().min(available);
                payload.extend_from_slice(&remaining[..take]);
                remaining = &remaining[take..];

                first_packet = false;
                payload
            } else {
                // Subsequent packets
                let take = remaining.len().min(payload_capacity);
                let payload = remaining[..take].to_vec();
                remaining = &remaining[take..];
                payload
            };

            // Create packet
            let header = TsPacketHeader::new(PAT_PID)
                .with_pusi(first_packet || packets.is_empty()) // PUSI on first packet
                .with_cc(cc.next(PAT_PID));

            let mut packet = TsPacket::new(PAT_PID);
            packet.header = header;
            packet.payload = payload;

            // Pad to 188 bytes
            let padded_packet = self.create_padded_packet(packet);
            packets.push(padded_packet);
        }

        packets
    }

    /// Create a padded packet
    fn create_padded_packet(&self, mut packet: TsPacket) -> TsPacket {
        let used = 4 + packet.payload.len();
        let remaining = TS_PACKET_SIZE - used;

        if remaining > 0 {
            // Add stuffing bytes to payload
            packet.payload.extend(std::iter::repeat(0xFF).take(remaining));
        }

        packet
    }

    /// Generate PAT as raw bytes (one or more 188-byte packets)
    pub fn generate(&self, cc: &mut ContinuityCounter) -> Vec<u8> {
        let packets = self.generate_ts_packets(cc);
        let mut output = Vec::with_capacity(packets.len() * TS_PACKET_SIZE);

        for packet in packets {
            if let Ok(encoded) = packet.encode() {
                output.extend_from_slice(&encoded);
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pat_section_basic() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);

        let section = pat.generate_section();

        // table_id should be 0x00
        assert_eq!(section[0], 0x00);

        // section_syntax_indicator should be set (0xB0)
        assert_eq!(section[1] & 0xF0, 0xB0);

        // transport_stream_id = 0x0001 (at bytes 3-4)
        assert_eq!(section[3], 0x00);
        assert_eq!(section[4], 0x01);

        // section_number and last_section_number should be 0 (at bytes 6-7)
        assert_eq!(section[6], 0x00);
        assert_eq!(section[7], 0x00);

        // CRC should be at the end (4 bytes)
        assert!(section.len() >= 4);
        let crc_start = section.len() - 4;
        // CRC should not be all zeros (real CRC)
        let crc = &section[crc_start..];
        assert_ne!(crc, &[0, 0, 0, 0]);
    }

    #[test]
    fn test_pat_single_program() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);

        let section = pat.generate_section();

        // Section should contain program info
        // After header (8 bytes), program info starts
        // program_number (2B) + reserved/PID (2B) = 4 bytes
        // Then CRC (4 bytes)

        // Check program number (at bytes 8-9)
        assert_eq!(section[8], 0x00);
        assert_eq!(section[9], 0x01);

        // Check PMT PID (0x1000) (at bytes 10-11)
        // PID 0x1000: high byte = 0xE0 | (0x1000 >> 8 & 0x1F) = 0xE0 | 0x10 = 0xF0
        assert_eq!(section[10], 0xF0);
        assert_eq!(section[11], 0x00);
    }

    #[test]
    fn test_pat_multiple_programs() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);
        pat.add_program(2, 0x1001);

        let section = pat.generate_section();

        // Should have 2 programs (8 bytes) + header (8 bytes) + CRC (4 bytes)

        // First program (program 1, PMT PID 0x1000)
        assert_eq!(section[8], 0x00);
        assert_eq!(section[9], 0x01);
        assert_eq!(section[10], 0xF0); // 0xE0 | 0x10
        assert_eq!(section[11], 0x00);

        // Second program (program 2, PMT PID 0x1001)
        assert_eq!(section[12], 0x00);
        assert_eq!(section[13], 0x02);
        assert_eq!(section[14], 0xF0); // 0xE0 | 0x10
        assert_eq!(section[15], 0x01);
    }

    #[test]
    fn test_pat_version() {
        let mut pat = PatGenerator::new()
            .with_version(3);

        pat.add_program(1, 0x1000);

        let section = pat.generate_section();

        // version_number should be in byte 5, bits 1-5
        // 0xC0 | (version << 1) | 0x01
        // version 3: 0xC0 | 0x06 | 0x01 = 0xC7
        assert_eq!(section[5], 0xC7);
    }

    #[test]
    fn test_pat_ts_packets() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);

        let mut cc = ContinuityCounter::new();
        let packets = pat.generate_ts_packets(&mut cc);

        // Should produce at least one packet
        assert!(!packets.is_empty());

        // Each packet should be encodable
        for packet in &packets {
            let encoded = packet.encode().unwrap();
            assert_eq!(encoded.len(), TS_PACKET_SIZE);
            assert_eq!(encoded[0], 0x47); // TS_SYNC_BYTE

            // PID should be 0x0000 (PAT PID)
            let pid = ((encoded[1] as u16 & 0x1F) << 8) | (encoded[2] as u16);
            assert_eq!(pid, PAT_PID);
        }
    }

    #[test]
    fn test_pat_continuity_counter() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);

        let mut cc = ContinuityCounter::new();

        // Generate multiple times
        let packets1 = pat.generate_ts_packets(&mut cc);
        let packets2 = pat.generate_ts_packets(&mut cc);

        // Continuity counter should increment
        let cc1 = packets1[0].header.continuity_counter;
        let cc2 = packets2[0].header.continuity_counter;

        assert_eq!(cc2, (cc1 + 1) & 0x0F);
    }

    #[test]
    fn test_crc32_correctness() {
        // Test CRC32 with known values
        let mut pat = PatGenerator::new()
            .with_transport_stream_id(0x0001);

        pat.add_program(1, 0x1000);

        let section = pat.generate_section();

        // Verify CRC by recalculating
        let data_for_crc = &section[0..section.len() - 4];
        let stored_crc = u32::from_be_bytes([
            section[section.len() - 4],
            section[section.len() - 3],
            section[section.len() - 2],
            section[section.len() - 1],
        ]);

        let calculated_crc = calculate_crc32(data_for_crc);

        assert_eq!(calculated_crc, stored_crc);
    }

    #[test]
    fn test_remove_program() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);
        pat.add_program(2, 0x1001);
        pat.add_program(3, 0x1002);

        assert_eq!(pat.programs.len(), 3);

        pat.remove_program(2);
        assert_eq!(pat.programs.len(), 2);

        // Remaining programs should be 1 and 3
        assert!(pat.programs.iter().any(|p| p.program_number == 1));
        assert!(pat.programs.iter().any(|p| p.program_number == 3));
        assert!(!pat.programs.iter().any(|p| p.program_number == 2));
    }

    #[test]
    fn test_update_program() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);

        // Update same program with different PID (valid 13-bit PID)
        pat.add_program(1, 0x1001);

        assert_eq!(pat.programs.len(), 1);
        assert_eq!(pat.programs[0].pmt_pid, 0x1001);
    }

    #[test]
    fn test_generate_output_size() {
        let mut pat = PatGenerator::new();
        pat.add_program(1, 0x1000);

        let mut cc = ContinuityCounter::new();
        let output = pat.generate(&mut cc);

        // Output should be multiple of 188
        assert_eq!(output.len() % TS_PACKET_SIZE, 0);
        assert!(output.len() >= TS_PACKET_SIZE);
    }
}
