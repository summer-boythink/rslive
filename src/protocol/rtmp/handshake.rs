use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    io::{Read, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{RTMP_HANDSHAKE_SIZE, RTMP_VERSION, RtmpError, RtmpResult};

/// RTMP handshake implementation
///
/// The RTMP handshake consists of three packets exchanged between client and server:
/// - C0/S0: 1 byte version
/// - C1/S1: 1536 bytes timestamp + random data
/// - C2/S2: 1536 bytes echo of peer's C1/S1
#[derive(Debug)]
pub struct RtmpHandshake {
    /// Local timestamp
    pub timestamp: u32,
    /// Random bytes for handshake
    pub random_bytes: Vec<u8>,
    /// Peer's handshake data (for echo)
    pub peer_data: Option<Vec<u8>>,
}

impl RtmpHandshake {
    /// Create new handshake instance
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;

        let mut random_bytes = vec![0u8; RTMP_HANDSHAKE_SIZE - 8]; // 1536 - 4 (timestamp) - 4 (zero)
        // Fill with pseudo-random data based on timestamp
        for (i, byte) in random_bytes.iter_mut().enumerate() {
            *byte = ((timestamp.wrapping_add(i as u32)) & 0xFF) as u8;
        }

        Self {
            timestamp,
            random_bytes,
            peer_data: None,
        }
    }

    /// Perform client-side handshake
    pub async fn client_handshake<S>(&mut self, stream: &mut S) -> RtmpResult<()>
    where
        S: Read + Write + Unpin,
    {
        // Send C0 + C1
        self.send_c0_c1(stream)?;

        // Read S0 + S1
        self.read_s0_s1(stream)?;

        // Send C2
        self.send_c2(stream)?;

        // Read S2 (optional verification)
        self.read_s2(stream)?;

        Ok(())
    }

    /// Perform server-side handshake
    pub async fn server_handshake<S>(&mut self, stream: &mut S) -> RtmpResult<()>
    where
        S: Read + Write + Unpin,
    {
        // Read C0 + C1
        self.read_c0_c1(stream)?;

        // Send S0 + S1
        self.send_s0_s1(stream)?;

        // Read C2
        self.read_c2(stream)?;

        // Send S2
        self.send_s2(stream)?;

        Ok(())
    }

    /// Send C0 + C1 (client version + handshake data)
    fn send_c0_c1<W: Write>(&self, writer: &mut W) -> RtmpResult<()> {
        // Send C0 (version)
        writer.write_u8(RTMP_VERSION)?;

        // Send C1 (timestamp + zero + random data)
        writer.write_u32::<BigEndian>(self.timestamp)?;
        writer.write_u32::<BigEndian>(0)?; // Zero field
        writer.write_all(&self.random_bytes)?;

        Ok(())
    }

    /// Read S0 + S1 (server version + handshake data)
    fn read_s0_s1<R: Read>(&mut self, reader: &mut R) -> RtmpResult<()> {
        // Read S0 (version)
        let version = reader.read_u8()?;
        if version != RTMP_VERSION {
            return Err(RtmpError::HandshakeFailed(format!(
                "Unsupported RTMP version: {}",
                version
            )));
        }

        // Read S1 (1536 bytes)
        let mut s1_data = vec![0u8; RTMP_HANDSHAKE_SIZE];
        reader.read_exact(&mut s1_data)?;

        // Store peer data for C2 echo
        self.peer_data = Some(s1_data);

        Ok(())
    }

    /// Send C2 (echo of S1)
    fn send_c2<W: Write>(&self, writer: &mut W) -> RtmpResult<()> {
        if let Some(ref peer_data) = self.peer_data {
            writer.write_all(peer_data)?;
        } else {
            return Err(RtmpError::HandshakeFailed(
                "No peer data available for C2".to_string(),
            ));
        }
        Ok(())
    }

    /// Read S2 (should be echo of C1)
    fn read_s2<R: Read>(&self, reader: &mut R) -> RtmpResult<()> {
        let mut s2_data = vec![0u8; RTMP_HANDSHAKE_SIZE];
        reader.read_exact(&mut s2_data)?;

        // Verify S2 is echo of our C1 (optional strict checking)
        // In practice, many implementations don't strictly verify this
        Ok(())
    }

    /// Read C0 + C1 (client version + handshake data)
    fn read_c0_c1<R: Read>(&mut self, reader: &mut R) -> RtmpResult<()> {
        // Read C0 (version)
        let version = reader.read_u8()?;
        if version != RTMP_VERSION {
            return Err(RtmpError::HandshakeFailed(format!(
                "Unsupported RTMP version: {}",
                version
            )));
        }

        // Read C1 (1536 bytes)
        let mut c1_data = vec![0u8; RTMP_HANDSHAKE_SIZE];
        reader.read_exact(&mut c1_data)?;

        // Store peer data for S2 echo
        self.peer_data = Some(c1_data);

        Ok(())
    }

    /// Send S0 + S1 (server version + handshake data)
    fn send_s0_s1<W: Write>(&self, writer: &mut W) -> RtmpResult<()> {
        // Send S0 (version)
        writer.write_u8(RTMP_VERSION)?;

        // Send S1 (timestamp + zero + random data)
        writer.write_u32::<BigEndian>(self.timestamp)?;
        writer.write_u32::<BigEndian>(0)?; // Zero field
        writer.write_all(&self.random_bytes)?;

        Ok(())
    }

    /// Read C2 (should be echo of S1)
    fn read_c2<R: Read>(&self, reader: &mut R) -> RtmpResult<()> {
        let mut c2_data = vec![0u8; RTMP_HANDSHAKE_SIZE];
        reader.read_exact(&mut c2_data)?;

        // Verify C2 is echo of our S1 (optional strict checking)
        Ok(())
    }

    /// Send S2 (echo of C1)
    fn send_s2<W: Write>(&self, writer: &mut W) -> RtmpResult<()> {
        if let Some(ref peer_data) = self.peer_data {
            writer.write_all(peer_data)?;
        } else {
            return Err(RtmpError::HandshakeFailed(
                "No peer data available for S2".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for RtmpHandshake {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple handshake for testing without async
pub struct SimpleHandshake;

impl SimpleHandshake {
    /// Perform simple client handshake
    pub fn client_handshake<S>(stream: &mut S) -> RtmpResult<()>
    where
        S: Read + Write,
    {
        let mut handshake = RtmpHandshake::new();

        // Send C0 + C1
        handshake.send_c0_c1(stream)?;

        // Read S0 + S1
        handshake.read_s0_s1(stream)?;

        // Send C2
        handshake.send_c2(stream)?;

        // Read S2
        handshake.read_s2(stream)?;

        Ok(())
    }

    /// Perform simple server handshake
    pub fn server_handshake<S>(stream: &mut S) -> RtmpResult<()>
    where
        S: Read + Write,
    {
        let mut handshake = RtmpHandshake::new();

        // Read C0 + C1
        handshake.read_c0_c1(stream)?;

        // Send S0 + S1
        handshake.send_s0_s1(stream)?;

        // Read C2
        handshake.read_c2(stream)?;

        // Send S2
        handshake.send_s2(stream)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_handshake_creation() {
        let handshake = RtmpHandshake::new();
        assert!(!handshake.random_bytes.is_empty());
        assert_eq!(handshake.random_bytes.len(), RTMP_HANDSHAKE_SIZE - 8);
        assert!(handshake.peer_data.is_none());
    }

    #[test]
    fn test_c0_c1_format() {
        let handshake = RtmpHandshake::new();
        let mut buffer = Vec::new();

        handshake.send_c0_c1(&mut buffer).unwrap();

        // Should be 1 + 1536 bytes
        assert_eq!(buffer.len(), 1 + RTMP_HANDSHAKE_SIZE);

        // First byte should be version
        assert_eq!(buffer[0], RTMP_VERSION);

        // Next 4 bytes should be timestamp
        let timestamp = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]);
        assert_eq!(timestamp, handshake.timestamp);

        // Next 4 bytes should be zero
        let zero = u32::from_be_bytes([buffer[5], buffer[6], buffer[7], buffer[8]]);
        assert_eq!(zero, 0);
    }

    #[test]
    fn test_handshake_roundtrip() {
        // Simulate client and server buffers
        let mut client_to_server = Vec::new();
        let mut server_to_client = Vec::new();

        // Client sends C0 + C1
        let client_handshake = RtmpHandshake::new();
        client_handshake.send_c0_c1(&mut client_to_server).unwrap();

        // Server reads C0 + C1
        let mut server_handshake = RtmpHandshake::new();
        let mut cursor = Cursor::new(&client_to_server);
        server_handshake.read_c0_c1(&mut cursor).unwrap();

        // Server sends S0 + S1
        server_handshake.send_s0_s1(&mut server_to_client).unwrap();

        // Verify server stored client data
        assert!(server_handshake.peer_data.is_some());
        assert_eq!(
            server_handshake.peer_data.as_ref().unwrap().len(),
            RTMP_HANDSHAKE_SIZE
        );
    }

    #[test]
    fn test_simple_handshake() {
        let mut _client_buf: Vec<u8> = Vec::new();
        let mut _server_buf: Vec<u8> = Vec::new();

        // Simulate handshake data exchange
        let client_data = {
            let handshake = RtmpHandshake::new();
            let mut buf = Vec::new();
            handshake.send_c0_c1(&mut buf).unwrap();
            buf
        };

        let server_response = {
            let mut handshake = RtmpHandshake::new();
            let mut cursor = Cursor::new(&client_data);
            handshake.read_c0_c1(&mut cursor).unwrap();

            let mut buf = Vec::new();
            handshake.send_s0_s1(&mut buf).unwrap();
            buf
        };

        // Verify response format
        assert_eq!(server_response.len(), 1 + RTMP_HANDSHAKE_SIZE);
        assert_eq!(server_response[0], RTMP_VERSION);
    }

    #[test]
    fn test_invalid_version() {
        let mut handshake = RtmpHandshake::new();
        let invalid_data = vec![0xFF; 1 + RTMP_HANDSHAKE_SIZE]; // Invalid version
        let mut cursor = Cursor::new(invalid_data);

        let result = handshake.read_c0_c1(&mut cursor);
        assert!(result.is_err());

        if let Err(RtmpError::HandshakeFailed(msg)) = result {
            assert!(msg.contains("Unsupported RTMP version"));
        } else {
            panic!("Expected HandshakeFailed error");
        }
    }
}
