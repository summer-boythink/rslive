use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    time::{Duration, Instant},
};

use super::{
    RtmpConfig, RtmpError, RtmpResult,
    chunk::{RtmpChunk, RtmpChunkHandler},
    chunk_stream_id, command,
    handshake::{RtmpHandshake, SimpleHandshake},
    message::{AmfCommand, ControlMessage, RtmpMessage},
    message_type, status,
};
use crate::protocol::amf0::Amf0Value;

/// Connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// Initial state
    Init,
    /// Handshake in progress
    Handshaking,
    /// Connected but not authenticated
    Connected,
    /// Publishing a stream
    Publishing,
    /// Playing a stream
    Playing,
    /// Connection is closing
    Closing,
    /// Connection is closed
    Closed,
    /// Error state
    Error(String),
}

/// Stream information
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub stream_id: u32,
    pub stream_name: String,
    pub is_publishing: bool,
    pub is_playing: bool,
    pub start_time: Instant,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

impl StreamInfo {
    pub fn new(stream_id: u32, stream_name: String) -> Self {
        Self {
            stream_id,
            stream_name,
            is_publishing: false,
            is_playing: false,
            start_time: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
        }
    }
}

/// RTMP Connection
#[derive(Debug)]
pub struct RtmpConnection {
    /// Connection configuration
    pub config: RtmpConfig,
    /// Connection state
    pub state: ConnectionState,
    /// Chunk handler for processing messages
    pub chunk_handler: RtmpChunkHandler,
    /// Next transaction ID for commands
    next_transaction_id: f64,
    /// Active streams
    streams: HashMap<u32, StreamInfo>,
    /// Next stream ID
    next_stream_id: u32,
    /// Connection start time
    start_time: Instant,
    /// Total bytes sent
    bytes_sent: u64,
    /// Total bytes received
    bytes_received: u64,
    /// Last activity time
    last_activity: Instant,
    /// Application name
    app_name: String,
    /// Client info
    client_info: HashMap<String, Amf0Value>,
}

impl RtmpConnection {
    /// Create new RTMP connection
    pub fn new(config: RtmpConfig) -> Self {
        Self {
            chunk_handler: RtmpChunkHandler::new(config.chunk_size),
            state: ConnectionState::Init,
            config,
            next_transaction_id: 1.0,
            streams: HashMap::new(),
            next_stream_id: 1,
            start_time: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
            last_activity: Instant::now(),
            app_name: String::new(),
            client_info: HashMap::new(),
        }
    }

    /// Get next transaction ID
    pub fn next_transaction_id(&mut self) -> f64 {
        let id = self.next_transaction_id;
        self.next_transaction_id += 1.0;
        id
    }

    /// Get next stream ID
    pub fn next_stream_id(&mut self) -> u32 {
        let id = self.next_stream_id;
        self.next_stream_id += 1;
        id
    }

    /// Update connection state
    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
        self.last_activity = Instant::now();
    }

    /// Check if connection has timed out
    pub fn is_timed_out(&self) -> bool {
        let timeout = Duration::from_secs(self.config.timeout);
        self.last_activity.elapsed() > timeout
    }

    /// Get connection duration
    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Add stream
    pub fn add_stream(&mut self, stream_name: String) -> u32 {
        let stream_id = self.next_stream_id();
        let stream_info = StreamInfo::new(stream_id, stream_name);
        self.streams.insert(stream_id, stream_info);
        stream_id
    }

    /// Remove stream
    pub fn remove_stream(&mut self, stream_id: u32) -> Option<StreamInfo> {
        self.streams.remove(&stream_id)
    }

    /// Get stream info
    pub fn get_stream(&self, stream_id: u32) -> Option<&StreamInfo> {
        self.streams.get(&stream_id)
    }

    /// Get stream info mutable
    pub fn get_stream_mut(&mut self, stream_id: u32) -> Option<&mut StreamInfo> {
        self.streams.get_mut(&stream_id)
    }

    /// Perform client-side handshake
    pub fn client_handshake<S>(&mut self, stream: &mut S) -> RtmpResult<()>
    where
        S: Read + Write,
    {
        self.set_state(ConnectionState::Handshaking);

        SimpleHandshake::client_handshake(stream)?;

        self.set_state(ConnectionState::Connected);
        Ok(())
    }

    /// Perform server-side handshake
    pub fn server_handshake<S>(&mut self, stream: &mut S) -> RtmpResult<()>
    where
        S: Read + Write,
    {
        self.set_state(ConnectionState::Handshaking);

        SimpleHandshake::server_handshake(stream)?;

        self.set_state(ConnectionState::Connected);
        Ok(())
    }

    /// Send RTMP message
    pub fn send_message<W: Write>(
        &mut self,
        writer: &mut W,
        message: &RtmpMessage,
    ) -> RtmpResult<()> {
        // Split message into chunks
        let chunks = self.chunk_handler.create_chunks(
            message,
            chunk_stream_id::COMMAND, // Default to command stream
            self.config.chunk_size,
        );

        // Send all chunks
        for chunk in chunks {
            self.chunk_handler.write_chunk(writer, &chunk)?;
        }

        self.bytes_sent += message.payload.len() as u64;
        self.last_activity = Instant::now();
        Ok(())
    }

    /// Read and process RTMP chunk
    pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> RtmpResult<Option<RtmpMessage>> {
        let chunk = self.chunk_handler.read_chunk(reader)?;
        self.bytes_received += chunk.data.len() as u64;

        let message = self.chunk_handler.process_chunk(chunk)?;
        self.last_activity = Instant::now();

        Ok(message)
    }

    /// Send connect command
    pub fn send_connect<W: Write>(
        &mut self,
        writer: &mut W,
        app: &str,
        flash_ver: &str,
        tc_url: &str,
    ) -> RtmpResult<f64> {
        let transaction_id = self.next_transaction_id();
        let command = AmfCommand::connect(transaction_id, app, flash_ver, tc_url);

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            0,
        )?;

        self.send_message(writer, &message)?;
        self.app_name = app.to_string();
        Ok(transaction_id)
    }

    /// Send connect result
    pub fn send_connect_result<W: Write>(
        &mut self,
        writer: &mut W,
        transaction_id: f64,
    ) -> RtmpResult<()> {
        let mut properties = HashMap::new();
        properties.insert(
            "fmsVer".to_string(),
            Amf0Value::String("FMS/3,0,1,123".to_string()),
        );
        properties.insert("capabilities".to_string(), Amf0Value::Number(31.0));

        let mut information = HashMap::new();
        information.insert("level".to_string(), Amf0Value::String("status".to_string()));
        information.insert(
            "code".to_string(),
            Amf0Value::String(status::NETCONNECTION_CONNECT_SUCCESS.to_string()),
        );
        information.insert(
            "description".to_string(),
            Amf0Value::String("Connection succeeded".to_string()),
        );

        let command = AmfCommand::result(
            transaction_id,
            Amf0Value::Object(properties),
            Amf0Value::Object(information),
        );

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            0,
        )?;

        self.send_message(writer, &message)
    }

    /// Send create stream command
    pub fn send_create_stream<W: Write>(&mut self, writer: &mut W) -> RtmpResult<f64> {
        let transaction_id = self.next_transaction_id();
        let command = AmfCommand::create_stream(transaction_id);

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            0,
        )?;

        self.send_message(writer, &message)?;
        Ok(transaction_id)
    }

    /// Send create stream result
    pub fn send_create_stream_result<W: Write>(
        &mut self,
        writer: &mut W,
        transaction_id: f64,
        stream_id: u32,
    ) -> RtmpResult<()> {
        let command = AmfCommand::result(
            transaction_id,
            Amf0Value::Null,
            Amf0Value::Number(stream_id as f64),
        );

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            0,
        )?;

        self.send_message(writer, &message)
    }

    /// Send publish command
    pub fn send_publish<W: Write>(
        &mut self,
        writer: &mut W,
        stream_name: &str,
        publish_type: &str,
    ) -> RtmpResult<f64> {
        let transaction_id = self.next_transaction_id();
        let command = AmfCommand::publish(transaction_id, stream_name, publish_type);

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            1, // Use stream ID 1
        )?;

        self.send_message(writer, &message)?;

        // Mark stream as publishing
        if let Some(stream) = self.streams.get_mut(&1) {
            stream.is_publishing = true;
        }

        Ok(transaction_id)
    }

    /// Send play command
    pub fn send_play<W: Write>(
        &mut self,
        writer: &mut W,
        stream_name: &str,
        start: f64,
        duration: f64,
        reset: bool,
    ) -> RtmpResult<f64> {
        let transaction_id = self.next_transaction_id();
        let command = AmfCommand::play(transaction_id, stream_name, start, duration, reset);

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            1, // Use stream ID 1
        )?;

        self.send_message(writer, &message)?;

        // Mark stream as playing
        if let Some(stream) = self.streams.get_mut(&1) {
            stream.is_playing = true;
        }

        Ok(transaction_id)
    }

    /// Send onStatus command
    pub fn send_on_status<W: Write>(
        &mut self,
        writer: &mut W,
        level: &str,
        code: &str,
        description: &str,
        stream_id: u32,
    ) -> RtmpResult<()> {
        let command = AmfCommand::on_status(level, code, description);

        let message = RtmpMessage::create_amf0_command(
            &command.command_name,
            command.transaction_id,
            Some(command.command_object),
            command.arguments,
            0,
            stream_id,
        )?;

        self.send_message(writer, &message)
    }

    /// Send control message
    pub fn send_control_message<W: Write>(
        &mut self,
        writer: &mut W,
        control_msg: ControlMessage,
    ) -> RtmpResult<()> {
        let message = control_msg.to_rtmp_message(0)?;
        self.send_message(writer, &message)
    }

    /// Set chunk size
    pub fn set_chunk_size<W: Write>(&mut self, writer: &mut W, chunk_size: u32) -> RtmpResult<()> {
        let control_msg = ControlMessage::SetChunkSize(chunk_size);
        self.send_control_message(writer, control_msg)?;

        // Update local chunk handler
        self.chunk_handler.set_chunk_size(chunk_size);
        Ok(())
    }

    /// Send acknowledgement
    pub fn send_acknowledgement<W: Write>(
        &mut self,
        writer: &mut W,
        sequence_number: u32,
    ) -> RtmpResult<()> {
        let control_msg = ControlMessage::Acknowledgement(sequence_number);
        self.send_control_message(writer, control_msg)
    }

    /// Send window acknowledgement size
    pub fn send_window_ack_size<W: Write>(
        &mut self,
        writer: &mut W,
        window_size: u32,
    ) -> RtmpResult<()> {
        let control_msg = ControlMessage::WindowAckSize(window_size);
        self.send_control_message(writer, control_msg)
    }

    /// Send peer bandwidth
    pub fn send_peer_bandwidth<W: Write>(
        &mut self,
        writer: &mut W,
        bandwidth: u32,
        limit_type: u8,
    ) -> RtmpResult<()> {
        let control_msg = ControlMessage::SetPeerBandwidth {
            size: bandwidth,
            limit_type,
        };
        self.send_control_message(writer, control_msg)
    }

    /// Process incoming message and generate appropriate response
    pub fn process_message<W: Write>(
        &mut self,
        writer: &mut W,
        message: &RtmpMessage,
    ) -> RtmpResult<()> {
        match message.header.message_type {
            message_type::AMF0_COMMAND => {
                let command = message.parse_amf0_command()?;
                self.handle_command(writer, &command, message.header.message_stream_id)?;
            }
            message_type::SET_CHUNK_SIZE => {
                let control_msg =
                    ControlMessage::parse(message.header.message_type, &message.payload)?;
                if let ControlMessage::SetChunkSize(chunk_size) = control_msg {
                    self.chunk_handler.set_chunk_size(chunk_size);
                }
            }
            message_type::ACKNOWLEDGEMENT => {
                // Handle acknowledgement
            }
            message_type::WINDOW_ACKNOWLEDGEMENT_SIZE => {
                // Handle window ack size
            }
            message_type::SET_PEER_BANDWIDTH => {
                // Handle peer bandwidth
            }
            message_type::AUDIO => {
                // Handle audio data
                self.handle_audio_message(message)?;
            }
            message_type::VIDEO => {
                // Handle video data
                self.handle_video_message(message)?;
            }
            _ => {
                // Unknown message type
            }
        }
        Ok(())
    }

    /// Handle AMF command
    fn handle_command<W: Write>(
        &mut self,
        writer: &mut W,
        command: &AmfCommand,
        stream_id: u32,
    ) -> RtmpResult<()> {
        match command.command_name.as_str() {
            command::CONNECT => {
                self.handle_connect_command(writer, command)?;
            }
            command::CREATE_STREAM => {
                self.handle_create_stream_command(writer, command)?;
            }
            command::PUBLISH => {
                self.handle_publish_command(writer, command, stream_id)?;
            }
            command::PLAY => {
                self.handle_play_command(writer, command, stream_id)?;
            }
            _ => {
                // Unknown command
            }
        }
        Ok(())
    }

    fn handle_connect_command<W: Write>(
        &mut self,
        writer: &mut W,
        command: &AmfCommand,
    ) -> RtmpResult<()> {
        // Extract connection info
        if let Amf0Value::Object(ref connect_obj) = command.command_object {
            self.client_info = connect_obj.clone();
            if let Some(Amf0Value::String(app)) = connect_obj.get("app") {
                self.app_name = app.clone();
            }
        }

        // Send control messages
        self.send_window_ack_size(writer, self.config.window_ack_size)?;
        self.send_peer_bandwidth(writer, self.config.peer_bandwidth, 2)?; // Dynamic limit
        self.set_chunk_size(writer, self.config.chunk_size)?;

        // Send connect result
        self.send_connect_result(writer, command.transaction_id)?;

        Ok(())
    }

    fn handle_create_stream_command<W: Write>(
        &mut self,
        writer: &mut W,
        command: &AmfCommand,
    ) -> RtmpResult<()> {
        let stream_id = self.next_stream_id();
        let stream_info = StreamInfo::new(stream_id, String::new());
        self.streams.insert(stream_id, stream_info);

        self.send_create_stream_result(writer, command.transaction_id, stream_id)?;
        Ok(())
    }

    fn handle_publish_command<W: Write>(
        &mut self,
        writer: &mut W,
        command: &AmfCommand,
        stream_id: u32,
    ) -> RtmpResult<()> {
        if let Some(Amf0Value::String(stream_name)) = command.arguments.first() {
            // Update stream info
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.stream_name = stream_name.clone();
                stream.is_publishing = true;
            }

            // Send onStatus
            self.send_on_status(
                writer,
                "status",
                status::NETSTREAM_PUBLISH_START,
                &format!("Started publishing stream '{}'", stream_name),
                stream_id,
            )?;

            self.set_state(ConnectionState::Publishing);
        }
        Ok(())
    }

    fn handle_play_command<W: Write>(
        &mut self,
        writer: &mut W,
        command: &AmfCommand,
        stream_id: u32,
    ) -> RtmpResult<()> {
        if let Some(Amf0Value::String(stream_name)) = command.arguments.first() {
            // Update stream info
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.stream_name = stream_name.clone();
                stream.is_playing = true;
            }

            // Send onStatus
            self.send_on_status(
                writer,
                "status",
                status::NETSTREAM_PLAY_START,
                &format!("Started playing stream '{}'", stream_name),
                stream_id,
            )?;

            self.set_state(ConnectionState::Playing);
        }
        Ok(())
    }

    fn handle_audio_message(&mut self, message: &RtmpMessage) -> RtmpResult<()> {
        // Update stream statistics
        if let Some(stream) = self.streams.get_mut(&message.header.message_stream_id) {
            stream.bytes_received += message.payload.len() as u64;
        }
        Ok(())
    }

    fn handle_video_message(&mut self, message: &RtmpMessage) -> RtmpResult<()> {
        // Update stream statistics
        if let Some(stream) = self.streams.get_mut(&message.header.message_stream_id) {
            stream.bytes_received += message.payload.len() as u64;
        }
        Ok(())
    }

    /// Close connection gracefully
    pub fn close<W: Write>(&mut self, writer: &mut W) -> RtmpResult<()> {
        self.set_state(ConnectionState::Closing);

        // Close all streams
        let stream_ids: Vec<u32> = self
            .streams
            .iter()
            .filter_map(|(stream_id, stream)| {
                if stream.is_publishing {
                    Some(*stream_id)
                } else {
                    None
                }
            })
            .collect();

        for stream_id in stream_ids {
            self.send_on_status(
                writer,
                "status",
                status::NETSTREAM_UNPUBLISH_SUCCESS,
                "Stream unpublished",
                stream_id,
            )?;
        }

        self.streams.clear();
        self.set_state(ConnectionState::Closed);
        Ok(())
    }

    /// Get connection statistics
    pub fn get_stats(&self) -> ConnectionStats {
        ConnectionStats {
            state: self.state.clone(),
            duration: self.duration(),
            bytes_sent: self.bytes_sent,
            bytes_received: self.bytes_received,
            stream_count: self.streams.len(),
            app_name: self.app_name.clone(),
        }
    }
}

/// Connection statistics
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub state: ConnectionState,
    pub duration: Duration,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub stream_count: usize,
    pub app_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_connection_creation() {
        let config = RtmpConfig::default();
        let connection = RtmpConnection::new(config);

        assert_eq!(connection.state, ConnectionState::Init);
        assert_eq!(connection.streams.len(), 0);
        assert_eq!(connection.next_transaction_id, 1.0);
    }

    #[test]
    fn test_transaction_id_generation() {
        let config = RtmpConfig::default();
        let mut connection = RtmpConnection::new(config);

        assert_eq!(connection.next_transaction_id(), 1.0);
        assert_eq!(connection.next_transaction_id(), 2.0);
        assert_eq!(connection.next_transaction_id(), 3.0);
    }

    #[test]
    fn test_stream_management() {
        let config = RtmpConfig::default();
        let mut connection = RtmpConnection::new(config);

        let stream_id = connection.add_stream("test_stream".to_string());
        assert_eq!(stream_id, 1);
        assert_eq!(connection.streams.len(), 1);

        let stream = connection.get_stream(stream_id).unwrap();
        assert_eq!(stream.stream_name, "test_stream");
        assert!(!stream.is_publishing);
        assert!(!stream.is_playing);

        let removed = connection.remove_stream(stream_id);
        assert!(removed.is_some());
        assert_eq!(connection.streams.len(), 0);
    }

    #[test]
    fn test_connect_command() {
        let config = RtmpConfig::default();
        let mut connection = RtmpConnection::new(config);
        let mut buffer = Vec::new();

        let transaction_id = connection
            .send_connect(
                &mut buffer,
                "live",
                "WIN 32,0,0,137",
                "rtmp://localhost/live",
            )
            .unwrap();

        assert_eq!(transaction_id, 1.0);
        assert_eq!(connection.app_name, "live");
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_state_management() {
        let config = RtmpConfig::default();
        let mut connection = RtmpConnection::new(config);

        assert_eq!(connection.state, ConnectionState::Init);

        connection.set_state(ConnectionState::Connected);
        assert_eq!(connection.state, ConnectionState::Connected);

        connection.set_state(ConnectionState::Publishing);
        assert_eq!(connection.state, ConnectionState::Publishing);
    }
}
