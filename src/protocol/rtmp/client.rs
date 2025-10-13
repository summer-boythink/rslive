use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

use super::{
    RtmpConfig, RtmpError, RtmpResult,
    connection::{ConnectionState, RtmpConnection},
    message::{AmfCommand, RtmpMessage},
    message_type, status,
};
use crate::protocol::amf0::Amf0Value;

/// RTMP Client for connecting to RTMP servers
#[derive(Debug)]
pub struct RtmpClient {
    /// TCP stream connection
    stream: Option<TcpStream>,
    /// RTMP connection handler
    connection: RtmpConnection,
    /// Server URL
    server_url: String,
    /// Application name
    app_name: String,
    /// Stream name for publishing/playing
    stream_name: Option<String>,
    /// Connection timeout
    timeout: Duration,
    /// Is client connected
    is_connected: bool,
}

impl RtmpClient {
    /// Create new RTMP client
    pub fn new(config: RtmpConfig) -> Self {
        let timeout = Duration::from_secs(config.timeout);
        Self {
            stream: None,
            connection: RtmpConnection::new(config),
            server_url: String::new(),
            app_name: String::new(),
            stream_name: None,
            timeout,
            is_connected: false,
        }
    }

    /// Create client with default configuration
    pub fn with_defaults() -> Self {
        Self::new(RtmpConfig::default())
    }

    /// Connect to RTMP server
    pub fn connect<A: ToSocketAddrs>(&mut self, addr: A, app: &str) -> RtmpResult<()> {
        // Connect TCP stream
        let stream =
            TcpStream::connect_timeout(&addr.to_socket_addrs()?.next().unwrap(), self.timeout)
                .map_err(|e| {
                    RtmpError::ConnectionFailed(format!("TCP connection failed: {}", e))
                })?;

        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;

        self.stream = Some(stream);
        self.app_name = app.to_string();
        self.server_url = format!(
            "rtmp://{}:{}/{}",
            addr.to_socket_addrs()?.next().unwrap().ip(),
            addr.to_socket_addrs()?.next().unwrap().port(),
            app
        );

        // Perform handshake
        if let Some(ref mut stream) = self.stream {
            self.connection.client_handshake(stream)?;
        }

        // Send connect command
        self.send_connect()?;

        // Wait for connect response
        self.wait_for_connect_response()?;

        self.is_connected = true;
        Ok(())
    }

    /// Connect to RTMP URL
    pub fn connect_url(&mut self, url: &str) -> RtmpResult<()> {
        let parsed_url = self.parse_rtmp_url(url)?;
        let addr = format!("{}:{}", parsed_url.host, parsed_url.port);
        self.connect(addr, &parsed_url.app)
    }

    /// Disconnect from server
    pub fn disconnect(&mut self) -> RtmpResult<()> {
        if let Some(ref mut stream) = self.stream {
            self.connection.close(stream)?;
        }
        self.stream = None;
        self.is_connected = false;
        self.connection.set_state(ConnectionState::Closed);
        Ok(())
    }

    /// Check if client is connected
    pub fn is_connected(&self) -> bool {
        self.is_connected
            && matches!(
                self.connection.state,
                ConnectionState::Connected | ConnectionState::Publishing | ConnectionState::Playing
            )
    }

    /// Start publishing a stream
    pub fn publish(&mut self, stream_name: &str, publish_type: &str) -> RtmpResult<()> {
        if !self.is_connected() {
            return Err(RtmpError::ConnectionFailed("Not connected".to_string()));
        }

        // Create stream
        let _stream_id = self.create_stream()?;

        // Send publish command
        if let Some(ref mut stream) = self.stream {
            self.connection
                .send_publish(stream, stream_name, publish_type)?;
        }

        // Wait for publish response
        self.wait_for_publish_response()?;

        self.stream_name = Some(stream_name.to_string());
        self.connection.set_state(ConnectionState::Publishing);

        Ok(())
    }

    /// Start playing a stream
    pub fn play(
        &mut self,
        stream_name: &str,
        start: f64,
        duration: f64,
        reset: bool,
    ) -> RtmpResult<()> {
        if !self.is_connected() {
            return Err(RtmpError::ConnectionFailed("Not connected".to_string()));
        }

        // Create stream
        let _stream_id = self.create_stream()?;

        // Send play command
        if let Some(ref mut stream) = self.stream {
            self.connection
                .send_play(stream, stream_name, start, duration, reset)?;
        }

        // Wait for play response
        self.wait_for_play_response()?;

        self.stream_name = Some(stream_name.to_string());
        self.connection.set_state(ConnectionState::Playing);

        Ok(())
    }

    /// Send audio data
    pub fn send_audio(&mut self, audio_data: &[u8], timestamp: u32) -> RtmpResult<()> {
        if !matches!(self.connection.state, ConnectionState::Publishing) {
            return Err(RtmpError::Protocol("Not in publishing state".to_string()));
        }

        let audio_msg = RtmpMessage::create_audio_message(audio_data.to_vec(), timestamp, 1);

        if let Some(ref mut stream) = self.stream {
            self.connection.send_message(stream, &audio_msg)?;
        }

        Ok(())
    }

    /// Send video data
    pub fn send_video(&mut self, video_data: &[u8], timestamp: u32) -> RtmpResult<()> {
        if !matches!(self.connection.state, ConnectionState::Publishing) {
            return Err(RtmpError::Protocol("Not in publishing state".to_string()));
        }

        let video_msg = RtmpMessage::create_video_message(video_data.to_vec(), timestamp, 1);

        if let Some(ref mut stream) = self.stream {
            self.connection.send_message(stream, &video_msg)?;
        }

        Ok(())
    }

    /// Send metadata
    pub fn send_metadata(&mut self, metadata: &HashMap<String, Amf0Value>) -> RtmpResult<()> {
        let values = vec![Amf0Value::Object(metadata.clone())];
        let metadata_msg = RtmpMessage::create_amf0_data("onMetaData", values, 0, 1)?;

        if let Some(ref mut stream) = self.stream {
            self.connection.send_message(stream, &metadata_msg)?;
        }

        Ok(())
    }

    /// Read next message from server
    pub fn read_message(&mut self) -> RtmpResult<Option<RtmpMessage>> {
        if let Some(ref mut stream) = self.stream {
            self.connection.read_chunk(stream)
        } else {
            Err(RtmpError::ConnectionFailed("No connection".to_string()))
        }
    }

    /// Process incoming messages (non-blocking)
    pub fn process_messages(&mut self) -> RtmpResult<Vec<RtmpMessage>> {
        let mut messages = Vec::new();

        while let Ok(Some(message)) = self.read_message() {
            // Process control messages automatically
            if let Some(ref mut stream) = self.stream {
                self.connection.process_message(stream, &message)?;
            }
            messages.push(message);
        }

        Ok(messages)
    }

    /// Get connection statistics
    pub fn get_stats(&self) -> super::connection::ConnectionStats {
        self.connection.get_stats()
    }

    /// Get current stream name
    pub fn get_stream_name(&self) -> Option<&str> {
        self.stream_name.as_deref()
    }

    /// Get application name
    pub fn get_app_name(&self) -> &str {
        &self.app_name
    }

    /// Get server URL
    pub fn get_server_url(&self) -> &str {
        &self.server_url
    }

    /// Send connect command to server
    fn send_connect(&mut self) -> RtmpResult<()> {
        let flash_ver = "WIN 32,0,0,137";
        let tc_url = format!("rtmp://localhost/{}", self.app_name);

        if let Some(ref mut stream) = self.stream {
            self.connection
                .send_connect(stream, &self.app_name, flash_ver, &tc_url)?;
        }

        Ok(())
    }

    /// Wait for connect response
    fn wait_for_connect_response(&mut self) -> RtmpResult<()> {
        let start_time = Instant::now();

        while start_time.elapsed() < self.timeout {
            if let Some(message) = self.read_message()? {
                if message.header.message_type == message_type::AMF0_COMMAND {
                    let command = message.parse_amf0_command()?;

                    match command.command_name.as_str() {
                        "_result" => {
                            // Connection successful
                            self.connection.set_state(ConnectionState::Connected);
                            return Ok(());
                        }
                        "_error" => {
                            let error_msg = self.extract_error_message(&command);
                            return Err(RtmpError::ConnectionFailed(error_msg));
                        }
                        _ => continue,
                    }
                }
            }
        }

        Err(RtmpError::Timeout)
    }

    /// Create a new stream
    fn create_stream(&mut self) -> RtmpResult<u32> {
        if let Some(ref mut stream) = self.stream {
            let transaction_id = self.connection.send_create_stream(stream)?;

            // Wait for response
            let start_time = Instant::now();
            while start_time.elapsed() < self.timeout {
                if let Some(message) = self.read_message()? {
                    if message.header.message_type == message_type::AMF0_COMMAND {
                        let command = message.parse_amf0_command()?;

                        if command.command_name == "_result"
                            && command.transaction_id == transaction_id
                        {
                            if let Some(Amf0Value::Number(stream_id)) = command.arguments.first() {
                                return Ok(*stream_id as u32);
                            }
                        }
                    }
                }
            }
        }

        Err(RtmpError::Timeout)
    }

    /// Wait for publish response
    fn wait_for_publish_response(&mut self) -> RtmpResult<()> {
        let start_time = Instant::now();

        while start_time.elapsed() < self.timeout {
            if let Some(message) = self.read_message()? {
                if message.header.message_type == message_type::AMF0_COMMAND {
                    let command = message.parse_amf0_command()?;

                    if command.command_name == "onStatus" {
                        if let Some(Amf0Value::Object(status_obj)) = command.arguments.first() {
                            if let Some(Amf0Value::String(code)) = status_obj.get("code") {
                                match code.as_str() {
                                    status::NETSTREAM_PUBLISH_START => return Ok(()),
                                    status::NETSTREAM_PUBLISH_FAILED => {
                                        let desc = status_obj
                                            .get("description")
                                            .and_then(|v| match v {
                                                Amf0Value::String(s) => Some(s.as_str()),
                                                _ => None,
                                            })
                                            .unwrap_or("Publish failed");
                                        return Err(RtmpError::Protocol(desc.to_string()));
                                    }
                                    _ => continue,
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(RtmpError::Timeout)
    }

    /// Wait for play response
    fn wait_for_play_response(&mut self) -> RtmpResult<()> {
        let start_time = Instant::now();

        while start_time.elapsed() < self.timeout {
            if let Some(message) = self.read_message()? {
                if message.header.message_type == message_type::AMF0_COMMAND {
                    let command = message.parse_amf0_command()?;

                    if command.command_name == "onStatus" {
                        if let Some(Amf0Value::Object(status_obj)) = command.arguments.first() {
                            if let Some(Amf0Value::String(code)) = status_obj.get("code") {
                                match code.as_str() {
                                    status::NETSTREAM_PLAY_START => return Ok(()),
                                    status::NETSTREAM_PLAY_FAILED => {
                                        let desc = status_obj
                                            .get("description")
                                            .and_then(|v| match v {
                                                Amf0Value::String(s) => Some(s.as_str()),
                                                _ => None,
                                            })
                                            .unwrap_or("Play failed");
                                        return Err(RtmpError::Protocol(desc.to_string()));
                                    }
                                    status::NETSTREAM_PLAY_STREAMNOTFOUND => {
                                        return Err(RtmpError::StreamNotFound(
                                            self.stream_name.clone().unwrap_or_default(),
                                        ));
                                    }
                                    _ => continue,
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(RtmpError::Timeout)
    }

    /// Extract error message from command
    fn extract_error_message(&self, command: &AmfCommand) -> String {
        if let Some(Amf0Value::Object(info)) = command.arguments.first() {
            if let Some(Amf0Value::String(desc)) = info.get("description") {
                return desc.clone();
            }
        }
        "Connection failed".to_string()
    }

    /// Parse RTMP URL
    fn parse_rtmp_url(&self, url: &str) -> RtmpResult<ParsedUrl> {
        if !url.starts_with("rtmp://") {
            return Err(RtmpError::Protocol("Invalid RTMP URL".to_string()));
        }

        let url_without_scheme = &url[7..]; // Remove "rtmp://"
        let parts: Vec<&str> = url_without_scheme.split('/').collect();

        if parts.is_empty() {
            return Err(RtmpError::Protocol("Invalid RTMP URL format".to_string()));
        }

        let host_port = parts[0];
        let host_port_parts: Vec<&str> = host_port.split(':').collect();

        let host = host_port_parts[0].to_string();
        let port = if host_port_parts.len() > 1 {
            host_port_parts[1]
                .parse::<u16>()
                .map_err(|_| RtmpError::Protocol("Invalid port number".to_string()))?
        } else {
            1935 // Default RTMP port
        };

        let app = if parts.len() > 1 {
            parts[1..].join("/")
        } else {
            "live".to_string() // Default app
        };

        Ok(ParsedUrl { host, port, app })
    }
}

impl Drop for RtmpClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

/// Parsed RTMP URL components
#[derive(Debug, Clone)]
struct ParsedUrl {
    host: String,
    port: u16,
    app: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = RtmpClient::with_defaults();
        assert!(!client.is_connected());
        assert_eq!(client.get_app_name(), "");
        assert!(client.get_stream_name().is_none());
    }

    #[test]
    fn test_url_parsing() {
        let client = RtmpClient::with_defaults();

        // Test basic URL
        let parsed = client.parse_rtmp_url("rtmp://localhost/live").unwrap();
        assert_eq!(parsed.host, "localhost");
        assert_eq!(parsed.port, 1935);
        assert_eq!(parsed.app, "live");

        // Test URL with port
        let parsed = client
            .parse_rtmp_url("rtmp://example.com:1936/myapp")
            .unwrap();
        assert_eq!(parsed.host, "example.com");
        assert_eq!(parsed.port, 1936);
        assert_eq!(parsed.app, "myapp");

        // Test URL with nested app
        let parsed = client
            .parse_rtmp_url("rtmp://localhost/app/stream")
            .unwrap();
        assert_eq!(parsed.host, "localhost");
        assert_eq!(parsed.port, 1935);
        assert_eq!(parsed.app, "app/stream");
    }

    #[test]
    fn test_invalid_url() {
        let client = RtmpClient::with_defaults();

        // Invalid scheme
        assert!(client.parse_rtmp_url("http://localhost/live").is_err());

        // Invalid port
        assert!(
            client
                .parse_rtmp_url("rtmp://localhost:invalid/live")
                .is_err()
        );
    }

    #[test]
    fn test_client_state() {
        let mut client = RtmpClient::with_defaults();
        assert_eq!(client.connection.state, ConnectionState::Init);

        client.connection.set_state(ConnectionState::Connected);
        client.is_connected = true; // Set the client as connected
        assert!(client.is_connected());

        client.connection.set_state(ConnectionState::Publishing);
        assert!(client.is_connected());

        client.connection.set_state(ConnectionState::Closed);
        client.is_connected = false; // Set the client as disconnected
        assert!(!client.is_connected());
    }
}
