use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
};

use super::{
    RTMP_MESSAGE_HEADER_SIZE_1, RTMP_MESSAGE_HEADER_SIZE_4, RTMP_MESSAGE_HEADER_SIZE_8,
    RTMP_MESSAGE_HEADER_SIZE_12,
};
use super::{RtmpError, RtmpMessage, RtmpMessageHeader, RtmpResult};

/// RTMP chunk header
#[derive(Debug, Clone, PartialEq)]
pub struct RtmpChunkHeader {
    /// Chunk format (2 bits)
    pub format: u8,
    /// Chunk stream ID (6 bits for basic header, can be extended)
    pub chunk_stream_id: u32,
    /// Timestamp (24 bits, can be extended)
    pub timestamp: u32,
    /// Message length (24 bits)
    pub message_length: u32,
    /// Message type ID (8 bits)
    pub message_type_id: u8,
    /// Message stream ID (32 bits, little endian)
    pub message_stream_id: u32,
    /// Extended timestamp (32 bits)
    pub extended_timestamp: Option<u32>,
}

impl RtmpChunkHeader {
    pub fn new(
        format: u8,
        chunk_stream_id: u32,
        timestamp: u32,
        message_length: u32,
        message_type_id: u8,
        message_stream_id: u32,
    ) -> Self {
        let extended_timestamp = if timestamp >= 0xFFFFFF {
            Some(timestamp)
        } else {
            None
        };

        Self {
            format,
            chunk_stream_id,
            timestamp: if timestamp >= 0xFFFFFF {
                0xFFFFFF
            } else {
                timestamp
            },
            message_length,
            message_type_id,
            message_stream_id,
            extended_timestamp,
        }
    }

    /// Get the actual timestamp (including extended timestamp)
    pub fn get_timestamp(&self) -> u32 {
        self.extended_timestamp.unwrap_or(self.timestamp)
    }

    /// Check if extended timestamp is required
    pub fn needs_extended_timestamp(&self) -> bool {
        self.timestamp >= 0xFFFFFF || self.extended_timestamp.is_some()
    }
}

/// RTMP chunk
#[derive(Debug, Clone)]
pub struct RtmpChunk {
    /// Chunk header
    pub header: RtmpChunkHeader,
    /// Chunk data
    pub data: Vec<u8>,
}

impl RtmpChunk {
    pub fn new(header: RtmpChunkHeader, data: Vec<u8>) -> Self {
        Self { header, data }
    }
}

/// Chunk stream state for tracking partial messages
#[derive(Debug, Clone)]
pub struct ChunkStreamState {
    /// Last message header for this chunk stream
    pub last_header: Option<RtmpMessageHeader>,
    /// Partial message data
    pub partial_message: Vec<u8>,
    /// Expected message length
    pub expected_length: u32,
    /// Last timestamp
    pub last_timestamp: u32,
    /// Last timestamp delta
    pub last_timestamp_delta: u32,
}

impl ChunkStreamState {
    pub fn new() -> Self {
        Self {
            last_header: None,
            partial_message: Vec::new(),
            expected_length: 0,
            last_timestamp: 0,
            last_timestamp_delta: 0,
        }
    }

    /// Check if this stream has a partial message
    pub fn has_partial_message(&self) -> bool {
        !self.partial_message.is_empty()
    }

    /// Get remaining bytes needed for the current message
    pub fn remaining_bytes(&self) -> u32 {
        if self.expected_length > self.partial_message.len() as u32 {
            self.expected_length - self.partial_message.len() as u32
        } else {
            0
        }
    }

    /// Clear partial message state
    pub fn clear(&mut self) {
        self.partial_message.clear();
        self.expected_length = 0;
    }
}

/// RTMP chunk handler for encoding and decoding chunks
#[derive(Debug)]
pub struct RtmpChunkHandler {
    /// Current chunk size for incoming chunks
    pub chunk_size: u32,
    /// Chunk stream states for tracking partial messages
    chunk_streams: HashMap<u32, ChunkStreamState>,
}

impl RtmpChunkHandler {
    pub fn new(chunk_size: u32) -> Self {
        Self {
            chunk_size,
            chunk_streams: HashMap::new(),
        }
    }

    /// Set new chunk size
    pub fn set_chunk_size(&mut self, chunk_size: u32) {
        self.chunk_size = chunk_size;
    }

    /// Read chunk basic header (1-3 bytes)
    pub fn read_basic_header<R: Read>(&self, reader: &mut R) -> RtmpResult<(u8, u32)> {
        let first_byte = reader.read_u8()?;
        let format = (first_byte & 0xC0) >> 6; // Top 2 bits
        let chunk_stream_id = first_byte & 0x3F; // Bottom 6 bits

        let chunk_stream_id = match chunk_stream_id {
            0 => {
                // Chunk stream ID is (second byte + 64)
                let second_byte = reader.read_u8()? as u32;
                second_byte + 64
            }
            1 => {
                // Chunk stream ID is ((third byte * 256) + second byte + 64)
                let second_byte = reader.read_u8()? as u32;
                let third_byte = reader.read_u8()? as u32;
                (third_byte * 256) + second_byte + 64
            }
            _ => chunk_stream_id as u32,
        };

        Ok((format, chunk_stream_id))
    }

    /// Write chunk basic header
    pub fn write_basic_header<W: Write>(
        &self,
        writer: &mut W,
        format: u8,
        chunk_stream_id: u32,
    ) -> RtmpResult<()> {
        if chunk_stream_id < 64 {
            // 1 byte basic header
            let basic_header = (format << 6) | (chunk_stream_id as u8);
            writer.write_u8(basic_header)?;
        } else if chunk_stream_id < 320 {
            // 2 byte basic header
            writer.write_u8(format << 6)?; // cs id = 0
            writer.write_u8((chunk_stream_id - 64) as u8)?;
        } else {
            // 3 byte basic header
            writer.write_u8((format << 6) | 1)?; // cs id = 1
            let cs_id_minus_64 = chunk_stream_id - 64;
            writer.write_u8((cs_id_minus_64 & 0xFF) as u8)?;
            writer.write_u8(((cs_id_minus_64 >> 8) & 0xFF) as u8)?;
        }
        Ok(())
    }

    /// Read chunk message header based on format
    pub fn read_message_header<R: Read>(
        &self,
        reader: &mut R,
        format: u8,
        chunk_stream_id: u32,
    ) -> RtmpResult<RtmpChunkHeader> {
        let state = self.chunk_streams.get(&chunk_stream_id);

        match format {
            RTMP_MESSAGE_HEADER_SIZE_12 => {
                // Type 0: 11 bytes
                let timestamp = ReadExt::read_u24::<BigEndian>(reader)?;
                let message_length = ReadExt::read_u24::<BigEndian>(reader)?;
                let message_type_id = reader.read_u8()?;
                let message_stream_id = reader.read_u32::<byteorder::LittleEndian>()?;

                let extended_timestamp = if timestamp >= 0xFFFFFF {
                    Some(reader.read_u32::<BigEndian>()?)
                } else {
                    None
                };

                Ok(RtmpChunkHeader {
                    format,
                    chunk_stream_id,
                    timestamp,
                    message_length,
                    message_type_id,
                    message_stream_id,
                    extended_timestamp,
                })
            }
            RTMP_MESSAGE_HEADER_SIZE_8 => {
                // Type 1: 7 bytes
                let timestamp_delta = ReadExt::read_u24::<BigEndian>(reader)?;
                let message_length = ReadExt::read_u24::<BigEndian>(reader)?;
                let message_type_id = reader.read_u8()?;

                let extended_timestamp = if timestamp_delta >= 0xFFFFFF {
                    Some(reader.read_u32::<BigEndian>()?)
                } else {
                    None
                };

                let last_timestamp = state.map(|s| s.last_timestamp).unwrap_or(0);
                let timestamp = if timestamp_delta >= 0xFFFFFF {
                    last_timestamp + extended_timestamp.unwrap()
                } else {
                    last_timestamp + timestamp_delta
                };

                Ok(RtmpChunkHeader {
                    format,
                    chunk_stream_id,
                    timestamp: if timestamp_delta >= 0xFFFFFF {
                        0xFFFFFF
                    } else {
                        timestamp_delta
                    },
                    message_length,
                    message_type_id,
                    message_stream_id: state
                        .and_then(|s| s.last_header.as_ref())
                        .map(|h| h.message_stream_id)
                        .unwrap_or(0),
                    extended_timestamp,
                })
            }
            RTMP_MESSAGE_HEADER_SIZE_4 => {
                // Type 2: 3 bytes
                let timestamp_delta = ReadExt::read_u24::<BigEndian>(reader)?;

                let extended_timestamp = if timestamp_delta >= 0xFFFFFF {
                    Some(reader.read_u32::<BigEndian>()?)
                } else {
                    None
                };

                let last_timestamp = state.map(|s| s.last_timestamp).unwrap_or(0);
                let timestamp = if timestamp_delta >= 0xFFFFFF {
                    last_timestamp + extended_timestamp.unwrap()
                } else {
                    last_timestamp + timestamp_delta
                };

                Ok(RtmpChunkHeader {
                    format,
                    chunk_stream_id,
                    timestamp: if timestamp_delta >= 0xFFFFFF {
                        0xFFFFFF
                    } else {
                        timestamp_delta
                    },
                    message_length: state
                        .and_then(|s| s.last_header.as_ref())
                        .map(|h| h.payload_length)
                        .unwrap_or(0),
                    message_type_id: state
                        .and_then(|s| s.last_header.as_ref())
                        .map(|h| h.message_type)
                        .unwrap_or(0),
                    message_stream_id: state
                        .and_then(|s| s.last_header.as_ref())
                        .map(|h| h.message_stream_id)
                        .unwrap_or(0),
                    extended_timestamp,
                })
            }
            RTMP_MESSAGE_HEADER_SIZE_1 => {
                // Type 3: 0 bytes, reuse previous header
                if let Some(state) = state {
                    let timestamp = state.last_timestamp + state.last_timestamp_delta;

                    Ok(RtmpChunkHeader {
                        format,
                        chunk_stream_id,
                        timestamp: state.last_timestamp_delta,
                        message_length: state
                            .last_header
                            .as_ref()
                            .map(|h| h.payload_length)
                            .unwrap_or(0),
                        message_type_id: state
                            .last_header
                            .as_ref()
                            .map(|h| h.message_type)
                            .unwrap_or(0),
                        message_stream_id: state
                            .last_header
                            .as_ref()
                            .map(|h| h.message_stream_id)
                            .unwrap_or(0),
                        extended_timestamp: if state.last_timestamp_delta >= 0xFFFFFF {
                            Some(timestamp)
                        } else {
                            None
                        },
                    })
                } else {
                    Err(RtmpError::Protocol(
                        "Type 3 chunk without previous header".to_string(),
                    ))
                }
            }
            _ => Err(RtmpError::InvalidChunkFormat(format)),
        }
    }

    /// Write chunk message header
    pub fn write_message_header<W: Write>(
        &self,
        writer: &mut W,
        header: &RtmpChunkHeader,
    ) -> RtmpResult<()> {
        match header.format {
            RTMP_MESSAGE_HEADER_SIZE_12 => {
                // Type 0: 11 bytes
                WriteExt::write_u24::<BigEndian>(writer, header.timestamp)?;
                WriteExt::write_u24::<BigEndian>(writer, header.message_length)?;
                writer.write_u8(header.message_type_id)?;
                writer.write_u32::<byteorder::LittleEndian>(header.message_stream_id)?;

                if header.needs_extended_timestamp() {
                    writer.write_u32::<BigEndian>(header.get_timestamp())?;
                }
            }
            RTMP_MESSAGE_HEADER_SIZE_8 => {
                // Type 1: 7 bytes
                WriteExt::write_u24::<BigEndian>(writer, header.timestamp)?;
                WriteExt::write_u24::<BigEndian>(writer, header.message_length)?;
                writer.write_u8(header.message_type_id)?;

                if header.needs_extended_timestamp() {
                    writer.write_u32::<BigEndian>(header.get_timestamp())?;
                }
            }
            RTMP_MESSAGE_HEADER_SIZE_4 => {
                // Type 2: 3 bytes
                WriteExt::write_u24::<BigEndian>(writer, header.timestamp)?;

                if header.needs_extended_timestamp() {
                    writer.write_u32::<BigEndian>(header.get_timestamp())?;
                }
            }
            RTMP_MESSAGE_HEADER_SIZE_1 => {
                // Type 3: 0 bytes
                if header.needs_extended_timestamp() {
                    writer.write_u32::<BigEndian>(header.get_timestamp())?;
                }
            }
            _ => return Err(RtmpError::InvalidChunkFormat(header.format)),
        }
        Ok(())
    }

    /// Read a complete chunk from the stream
    pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> RtmpResult<RtmpChunk> {
        // Read basic header
        let (format, chunk_stream_id) = self.read_basic_header(reader)?;

        // Read message header
        let chunk_header = self.read_message_header(reader, format, chunk_stream_id)?;

        // Determine how many bytes to read for this chunk
        let state = self.chunk_streams.get(&chunk_stream_id).cloned();
        let bytes_to_read = if let Some(ref state) = state {
            if state.has_partial_message() {
                std::cmp::min(state.remaining_bytes(), self.chunk_size)
            } else {
                std::cmp::min(chunk_header.message_length, self.chunk_size)
            }
        } else {
            std::cmp::min(chunk_header.message_length, self.chunk_size)
        };

        // Read chunk data
        let mut data = vec![0u8; bytes_to_read as usize];
        reader.read_exact(&mut data)?;

        Ok(RtmpChunk {
            header: chunk_header,
            data,
        })
    }

    /// Write a chunk to the stream
    pub fn write_chunk<W: Write>(&self, writer: &mut W, chunk: &RtmpChunk) -> RtmpResult<()> {
        // Write basic header
        self.write_basic_header(writer, chunk.header.format, chunk.header.chunk_stream_id)?;

        // Write message header
        self.write_message_header(writer, &chunk.header)?;

        // Write chunk data
        writer.write_all(&chunk.data)?;

        Ok(())
    }

    /// Process a chunk and potentially return a complete message
    pub fn process_chunk(&mut self, chunk: RtmpChunk) -> RtmpResult<Option<RtmpMessage>> {
        let chunk_stream_id = chunk.header.chunk_stream_id;

        // Get or create chunk stream state
        let state = self
            .chunk_streams
            .entry(chunk_stream_id)
            .or_insert_with(ChunkStreamState::new);

        // Update state based on chunk header format
        match chunk.header.format {
            RTMP_MESSAGE_HEADER_SIZE_12 => {
                // New message
                state.clear();
                state.expected_length = chunk.header.message_length;
                state.last_timestamp = chunk.header.get_timestamp();
                state.last_timestamp_delta = 0;
                state.last_header = Some(RtmpMessageHeader::new(
                    chunk.header.message_type_id,
                    chunk.header.message_length,
                    chunk.header.get_timestamp(),
                    chunk.header.message_stream_id,
                ));
            }
            RTMP_MESSAGE_HEADER_SIZE_8 => {
                // New message with same stream ID
                state.clear();
                state.expected_length = chunk.header.message_length;
                let timestamp_delta = chunk.header.get_timestamp();
                state.last_timestamp += timestamp_delta;
                state.last_timestamp_delta = timestamp_delta;
                state.last_header = Some(RtmpMessageHeader::new(
                    chunk.header.message_type_id,
                    chunk.header.message_length,
                    state.last_timestamp,
                    chunk.header.message_stream_id,
                ));
            }
            RTMP_MESSAGE_HEADER_SIZE_4 => {
                // Continue message with new timestamp delta
                if !state.has_partial_message() {
                    // This shouldn't happen but handle gracefully
                    state.expected_length = state
                        .last_header
                        .as_ref()
                        .map(|h| h.payload_length)
                        .unwrap_or(0);
                }
                let timestamp_delta = chunk.header.get_timestamp();
                state.last_timestamp += timestamp_delta;
                state.last_timestamp_delta = timestamp_delta;
            }
            RTMP_MESSAGE_HEADER_SIZE_1 => {
                // Continue message with same timestamp delta
                if !state.has_partial_message() {
                    // This shouldn't happen but handle gracefully
                    state.expected_length = state
                        .last_header
                        .as_ref()
                        .map(|h| h.payload_length)
                        .unwrap_or(0);
                }
                state.last_timestamp += state.last_timestamp_delta;
            }
            _ => return Err(RtmpError::InvalidChunkFormat(chunk.header.format)),
        }

        // Add chunk data to partial message
        state.partial_message.extend_from_slice(&chunk.data);

        // Check if message is complete
        if state.partial_message.len() >= state.expected_length as usize {
            // Message is complete
            let header = state.last_header.as_ref().unwrap().clone();
            let payload = state.partial_message.clone();
            state.clear();

            Ok(Some(RtmpMessage::new(header, payload)))
        } else {
            // Message is still incomplete
            Ok(None)
        }
    }

    /// Split a message into chunks
    pub fn create_chunks(
        &self,
        message: &RtmpMessage,
        chunk_stream_id: u32,
        chunk_size: u32,
    ) -> Vec<RtmpChunk> {
        let mut chunks = Vec::new();
        let payload = &message.payload;
        let mut offset = 0;

        while offset < payload.len() {
            let is_first_chunk = offset == 0;
            let remaining = payload.len() - offset;
            let chunk_data_size = std::cmp::min(remaining, chunk_size as usize);

            let format = if is_first_chunk {
                RTMP_MESSAGE_HEADER_SIZE_12
            } else {
                RTMP_MESSAGE_HEADER_SIZE_1
            };

            let chunk_header = RtmpChunkHeader::new(
                format,
                chunk_stream_id,
                message.header.timestamp,
                message.header.payload_length,
                message.header.message_type,
                message.header.message_stream_id,
            );

            let chunk_data = payload[offset..offset + chunk_data_size].to_vec();
            chunks.push(RtmpChunk::new(chunk_header, chunk_data));

            offset += chunk_data_size;
        }

        chunks
    }
}

// Extension trait for reading u24
trait ReadExt: Read {
    fn read_u24<T: byteorder::ByteOrder>(&mut self) -> io::Result<u32>;
}

impl<R: Read> ReadExt for R {
    fn read_u24<T: byteorder::ByteOrder>(&mut self) -> io::Result<u32> {
        let mut buf = [0u8; 3];
        self.read_exact(&mut buf)?;
        Ok(T::read_u24(&buf))
    }
}

// Extension trait for writing u24
trait WriteExt: Write {
    fn write_u24<T: byteorder::ByteOrder>(&mut self, n: u32) -> io::Result<()>;
}

impl<W: Write> WriteExt for W {
    fn write_u24<T: byteorder::ByteOrder>(&mut self, n: u32) -> io::Result<()> {
        let mut buf = [0u8; 3];
        T::write_u24(&mut buf, n);
        self.write_all(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_basic_header_encoding() {
        let handler = RtmpChunkHandler::new(128);
        let mut buf = Vec::new();

        // Test 1-byte basic header
        handler.write_basic_header(&mut buf, 0, 5).unwrap();
        assert_eq!(buf, vec![0x05]); // format=0, cs_id=5

        buf.clear();

        // Test 2-byte basic header
        handler.write_basic_header(&mut buf, 1, 100).unwrap();
        assert_eq!(buf, vec![0x40, 36]); // format=1, cs_id=100 -> second_byte=36

        buf.clear();

        // Test 3-byte basic header
        handler.write_basic_header(&mut buf, 2, 400).unwrap();
        assert_eq!(buf, vec![0x81, 80, 1]); // format=2, cs_id=400
    }

    #[test]
    fn test_basic_header_decoding() {
        let handler = RtmpChunkHandler::new(128);

        // Test 1-byte
        let mut cursor = Cursor::new(vec![0x05]);
        let (format, cs_id) = handler.read_basic_header(&mut cursor).unwrap();
        assert_eq!(format, 0);
        assert_eq!(cs_id, 5);

        // Test 2-byte
        let mut cursor = Cursor::new(vec![0x40, 36]);
        let (format, cs_id) = handler.read_basic_header(&mut cursor).unwrap();
        assert_eq!(format, 1);
        assert_eq!(cs_id, 100);

        // Test 3-byte
        let mut cursor = Cursor::new(vec![0x81, 80, 1]);
        let (format, cs_id) = handler.read_basic_header(&mut cursor).unwrap();
        assert_eq!(format, 2);
        assert_eq!(cs_id, 400);
    }

    #[test]
    fn test_chunk_creation() {
        let handler = RtmpChunkHandler::new(128);
        let header = RtmpMessageHeader::new(8, 1000, 12345, 1); // Audio message
        let payload = vec![0xAF; 1000]; // 1000 bytes of audio data
        let message = RtmpMessage::new(header, payload);

        let chunks = handler.create_chunks(&message, 4, 128);

        // Should create 8 chunks (1000 / 128 = 7.8, rounded up to 8)
        assert_eq!(chunks.len(), 8);

        // First chunk should be type 0
        assert_eq!(chunks[0].header.format, RTMP_MESSAGE_HEADER_SIZE_12);

        // Remaining chunks should be type 3
        for chunk in &chunks[1..] {
            assert_eq!(chunk.header.format, RTMP_MESSAGE_HEADER_SIZE_1);
        }

        // All chunks except last should be 128 bytes
        for chunk in &chunks[..7] {
            assert_eq!(chunk.data.len(), 128);
        }

        // Last chunk should be remainder (1000 - 7*128 = 104)
        assert_eq!(chunks[7].data.len(), 104);
    }
}
