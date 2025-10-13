use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
};

use crate::protocol::{amf0::Amf0Value, amf3::Amf3Value};

use super::{RtmpError, RtmpResult, message_type};

/// RTMP message header
#[derive(Debug, Clone, PartialEq)]
pub struct RtmpMessageHeader {
    /// Message type
    pub message_type: u8,
    /// Payload length
    pub payload_length: u32,
    /// Timestamp
    pub timestamp: u32,
    /// Message stream ID
    pub message_stream_id: u32,
}

impl RtmpMessageHeader {
    pub fn new(
        message_type: u8,
        payload_length: u32,
        timestamp: u32,
        message_stream_id: u32,
    ) -> Self {
        Self {
            message_type,
            payload_length,
            timestamp,
            message_stream_id,
        }
    }
}

/// RTMP message
#[derive(Debug, Clone)]
pub struct RtmpMessage {
    /// Message header
    pub header: RtmpMessageHeader,
    /// Message payload
    pub payload: Vec<u8>,
}

impl RtmpMessage {
    pub fn new(header: RtmpMessageHeader, payload: Vec<u8>) -> Self {
        Self { header, payload }
    }

    /// Create a control message
    pub fn create_control_message(message_type: u8, timestamp: u32, data: Vec<u8>) -> Self {
        let header = RtmpMessageHeader::new(message_type, data.len() as u32, timestamp, 0);
        Self::new(header, data)
    }

    /// Create a command message using AMF0
    pub fn create_amf0_command(
        command_name: &str,
        transaction_id: f64,
        command_object: Option<Amf0Value>,
        arguments: Vec<Amf0Value>,
        timestamp: u32,
        stream_id: u32,
    ) -> RtmpResult<Self> {
        let mut payload = Vec::new();

        // Encode command name
        let command_name_value = Amf0Value::String(command_name.to_string());
        crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &command_name_value)
            .map_err(|e| RtmpError::Amf(format!("Failed to encode command name: {}", e)))?;

        // Encode transaction ID
        let transaction_id_value = Amf0Value::Number(transaction_id);
        crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &transaction_id_value)
            .map_err(|e| RtmpError::Amf(format!("Failed to encode transaction ID: {}", e)))?;

        // Encode command object
        if let Some(obj) = command_object {
            crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &obj)
                .map_err(|e| RtmpError::Amf(format!("Failed to encode command object: {}", e)))?;
        } else {
            crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &Amf0Value::Null)
                .map_err(|e| RtmpError::Amf(format!("Failed to encode null: {}", e)))?;
        }

        // Encode arguments
        for arg in arguments {
            crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &arg)
                .map_err(|e| RtmpError::Amf(format!("Failed to encode argument: {}", e)))?;
        }

        let header = RtmpMessageHeader::new(
            message_type::AMF0_COMMAND,
            payload.len() as u32,
            timestamp,
            stream_id,
        );

        Ok(Self::new(header, payload))
    }

    /// Parse AMF0 command message
    pub fn parse_amf0_command(&self) -> RtmpResult<AmfCommand> {
        if self.header.message_type != message_type::AMF0_COMMAND {
            return Err(RtmpError::Protocol(format!(
                "Expected AMF0 command message, got type: {}",
                self.header.message_type
            )));
        }

        let mut cursor = std::io::Cursor::new(&self.payload);
        let mut decoder = crate::protocol::amf0::Amf0Decoder::new();

        // Parse command name
        let command_name = match decoder.decode(&mut cursor)? {
            Amf0Value::String(name) => name,
            _ => return Err(RtmpError::Amf("Command name must be a string".to_string())),
        };

        // Parse transaction ID
        let transaction_id = match decoder.decode(&mut cursor)? {
            Amf0Value::Number(id) => id,
            _ => {
                return Err(RtmpError::Amf(
                    "Transaction ID must be a number".to_string(),
                ));
            }
        };

        // Parse command object
        let command_object = decoder.decode(&mut cursor)?;

        // Parse additional arguments
        let mut arguments = Vec::new();
        while cursor.position() < self.payload.len() as u64 {
            match decoder.decode(&mut cursor) {
                Ok(arg) => arguments.push(arg),
                Err(e) => {
                    if cursor.position() >= self.payload.len() as u64 {
                        break;
                    }
                    return Err(RtmpError::Amf(format!("Failed to parse argument: {}", e)));
                }
            }
        }

        Ok(AmfCommand {
            command_name,
            transaction_id,
            command_object,
            arguments,
        })
    }

    /// Create a data message using AMF0
    pub fn create_amf0_data(
        data_name: &str,
        values: Vec<Amf0Value>,
        timestamp: u32,
        stream_id: u32,
    ) -> RtmpResult<Self> {
        let mut payload = Vec::new();

        // Encode data name
        let data_name_value = Amf0Value::String(data_name.to_string());
        crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &data_name_value)
            .map_err(|e| RtmpError::Amf(format!("Failed to encode data name: {}", e)))?;

        // Encode values
        for value in values {
            crate::protocol::amf0::Amf0Encoder::encode_value(&mut payload, &value)
                .map_err(|e| RtmpError::Amf(format!("Failed to encode data value: {}", e)))?;
        }

        let header = RtmpMessageHeader::new(
            message_type::AMF0_DATA,
            payload.len() as u32,
            timestamp,
            stream_id,
        );

        Ok(Self::new(header, payload))
    }

    /// Create audio message
    pub fn create_audio_message(audio_data: Vec<u8>, timestamp: u32, stream_id: u32) -> Self {
        let header = RtmpMessageHeader::new(
            message_type::AUDIO,
            audio_data.len() as u32,
            timestamp,
            stream_id,
        );
        Self::new(header, audio_data)
    }

    /// Create video message
    pub fn create_video_message(video_data: Vec<u8>, timestamp: u32, stream_id: u32) -> Self {
        let header = RtmpMessageHeader::new(
            message_type::VIDEO,
            video_data.len() as u32,
            timestamp,
            stream_id,
        );
        Self::new(header, video_data)
    }
}

/// AMF command structure
#[derive(Debug, Clone)]
pub struct AmfCommand {
    pub command_name: String,
    pub transaction_id: f64,
    pub command_object: Amf0Value,
    pub arguments: Vec<Amf0Value>,
}

impl AmfCommand {
    pub fn new(
        command_name: String,
        transaction_id: f64,
        command_object: Amf0Value,
        arguments: Vec<Amf0Value>,
    ) -> Self {
        Self {
            command_name,
            transaction_id,
            command_object,
            arguments,
        }
    }

    /// Create a connect command
    pub fn connect(transaction_id: f64, app: &str, flash_ver: &str, tc_url: &str) -> Self {
        let mut connect_object = HashMap::new();
        connect_object.insert("app".to_string(), Amf0Value::String(app.to_string()));
        connect_object.insert(
            "flashVer".to_string(),
            Amf0Value::String(flash_ver.to_string()),
        );
        connect_object.insert("tcUrl".to_string(), Amf0Value::String(tc_url.to_string()));
        connect_object.insert("fpad".to_string(), Amf0Value::Boolean(false));
        connect_object.insert("capabilities".to_string(), Amf0Value::Number(15.0));
        connect_object.insert("audioCodecs".to_string(), Amf0Value::Number(3575.0));
        connect_object.insert("videoCodecs".to_string(), Amf0Value::Number(252.0));
        connect_object.insert("videoFunction".to_string(), Amf0Value::Number(1.0));

        Self::new(
            "connect".to_string(),
            transaction_id,
            Amf0Value::Object(connect_object),
            Vec::new(),
        )
    }

    /// Create a result command
    pub fn result(transaction_id: f64, properties: Amf0Value, information: Amf0Value) -> Self {
        Self::new(
            "_result".to_string(),
            transaction_id,
            properties,
            vec![information],
        )
    }

    /// Create an error command
    pub fn error(transaction_id: f64, properties: Amf0Value, information: Amf0Value) -> Self {
        Self::new(
            "_error".to_string(),
            transaction_id,
            properties,
            vec![information],
        )
    }

    /// Create a create stream command
    pub fn create_stream(transaction_id: f64) -> Self {
        Self::new(
            "createStream".to_string(),
            transaction_id,
            Amf0Value::Null,
            Vec::new(),
        )
    }

    /// Create a publish command
    pub fn publish(transaction_id: f64, stream_name: &str, publish_type: &str) -> Self {
        Self::new(
            "publish".to_string(),
            transaction_id,
            Amf0Value::Null,
            vec![
                Amf0Value::String(stream_name.to_string()),
                Amf0Value::String(publish_type.to_string()),
            ],
        )
    }

    /// Create a play command
    pub fn play(
        transaction_id: f64,
        stream_name: &str,
        start: f64,
        duration: f64,
        reset: bool,
    ) -> Self {
        Self::new(
            "play".to_string(),
            transaction_id,
            Amf0Value::Null,
            vec![
                Amf0Value::String(stream_name.to_string()),
                Amf0Value::Number(start),
                Amf0Value::Number(duration),
                Amf0Value::Boolean(reset),
            ],
        )
    }

    /// Create an onStatus command
    pub fn on_status(level: &str, code: &str, description: &str) -> Self {
        let mut status_object = HashMap::new();
        status_object.insert("level".to_string(), Amf0Value::String(level.to_string()));
        status_object.insert("code".to_string(), Amf0Value::String(code.to_string()));
        status_object.insert(
            "description".to_string(),
            Amf0Value::String(description.to_string()),
        );

        Self::new(
            "onStatus".to_string(),
            0.0,
            Amf0Value::Null,
            vec![Amf0Value::Object(status_object)],
        )
    }
}

/// Control message types
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Set chunk size
    SetChunkSize(u32),
    /// Abort message
    AbortMessage(u32),
    /// Acknowledgement
    Acknowledgement(u32),
    /// Window acknowledgement size
    WindowAckSize(u32),
    /// Set peer bandwidth
    SetPeerBandwidth { size: u32, limit_type: u8 },
}

impl ControlMessage {
    /// Parse control message from payload
    pub fn parse(message_type: u8, payload: &[u8]) -> RtmpResult<Self> {
        let mut cursor = std::io::Cursor::new(payload);

        match message_type {
            message_type::SET_CHUNK_SIZE => {
                let chunk_size = cursor.read_u32::<BigEndian>()?;
                Ok(ControlMessage::SetChunkSize(chunk_size))
            }
            message_type::ABORT_MESSAGE => {
                let chunk_stream_id = cursor.read_u32::<BigEndian>()?;
                Ok(ControlMessage::AbortMessage(chunk_stream_id))
            }
            message_type::ACKNOWLEDGEMENT => {
                let sequence_number = cursor.read_u32::<BigEndian>()?;
                Ok(ControlMessage::Acknowledgement(sequence_number))
            }
            message_type::WINDOW_ACKNOWLEDGEMENT_SIZE => {
                let window_size = cursor.read_u32::<BigEndian>()?;
                Ok(ControlMessage::WindowAckSize(window_size))
            }
            message_type::SET_PEER_BANDWIDTH => {
                let size = cursor.read_u32::<BigEndian>()?;
                let limit_type = cursor.read_u8()?;
                Ok(ControlMessage::SetPeerBandwidth { size, limit_type })
            }
            _ => Err(RtmpError::InvalidMessageType(message_type)),
        }
    }

    /// Encode control message to bytes
    pub fn encode(&self) -> RtmpResult<Vec<u8>> {
        let mut buf = Vec::new();

        match self {
            ControlMessage::SetChunkSize(chunk_size) => {
                buf.write_u32::<BigEndian>(*chunk_size)?;
            }
            ControlMessage::AbortMessage(chunk_stream_id) => {
                buf.write_u32::<BigEndian>(*chunk_stream_id)?;
            }
            ControlMessage::Acknowledgement(sequence_number) => {
                buf.write_u32::<BigEndian>(*sequence_number)?;
            }
            ControlMessage::WindowAckSize(window_size) => {
                buf.write_u32::<BigEndian>(*window_size)?;
            }
            ControlMessage::SetPeerBandwidth { size, limit_type } => {
                buf.write_u32::<BigEndian>(*size)?;
                buf.write_u8(*limit_type)?;
            }
        }

        Ok(buf)
    }

    /// Get message type
    pub fn message_type(&self) -> u8 {
        match self {
            ControlMessage::SetChunkSize(_) => message_type::SET_CHUNK_SIZE,
            ControlMessage::AbortMessage(_) => message_type::ABORT_MESSAGE,
            ControlMessage::Acknowledgement(_) => message_type::ACKNOWLEDGEMENT,
            ControlMessage::WindowAckSize(_) => message_type::WINDOW_ACKNOWLEDGEMENT_SIZE,
            ControlMessage::SetPeerBandwidth { .. } => message_type::SET_PEER_BANDWIDTH,
        }
    }

    /// Create RTMP message
    pub fn to_rtmp_message(&self, timestamp: u32) -> RtmpResult<RtmpMessage> {
        let payload = self.encode()?;
        Ok(RtmpMessage::create_control_message(
            self.message_type(),
            timestamp,
            payload,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::rtmp::command;

    #[test]
    fn test_control_message_set_chunk_size() {
        let msg = ControlMessage::SetChunkSize(4096);
        let payload = msg.encode().unwrap();
        assert_eq!(payload.len(), 4);

        let parsed = ControlMessage::parse(message_type::SET_CHUNK_SIZE, &payload).unwrap();
        match parsed {
            ControlMessage::SetChunkSize(size) => assert_eq!(size, 4096),
            _ => panic!("Expected SetChunkSize"),
        }
    }

    #[test]
    fn test_amf_command_connect() {
        let cmd = AmfCommand::connect(1.0, "live", "WIN 32,0,0,137", "rtmp://localhost/live");
        assert_eq!(cmd.command_name, "connect");
        assert_eq!(cmd.transaction_id, 1.0);

        if let Amf0Value::Object(obj) = &cmd.command_object {
            assert!(obj.contains_key("app"));
            assert!(obj.contains_key("tcUrl"));
        } else {
            panic!("Expected object command object");
        }
    }

    #[test]
    fn test_rtmp_message_creation() {
        let audio_data = vec![0xAF, 0x01, 0x02, 0x03];
        let msg = RtmpMessage::create_audio_message(audio_data.clone(), 1000, 1);

        assert_eq!(msg.header.message_type, message_type::AUDIO);
        assert_eq!(msg.header.payload_length, 4);
        assert_eq!(msg.header.timestamp, 1000);
        assert_eq!(msg.header.message_stream_id, 1);
        assert_eq!(msg.payload, audio_data);
    }

    #[test]
    fn test_amf0_command_message() {
        let cmd = AmfCommand::create_stream(2.0);
        let msg = RtmpMessage::create_amf0_command(
            &cmd.command_name,
            cmd.transaction_id,
            Some(cmd.command_object.clone()),
            cmd.arguments.clone(),
            0,
            0,
        )
        .unwrap();

        let parsed_cmd = msg.parse_amf0_command().unwrap();
        assert_eq!(parsed_cmd.command_name, "createStream");
        assert_eq!(parsed_cmd.transaction_id, 2.0);
    }
}
