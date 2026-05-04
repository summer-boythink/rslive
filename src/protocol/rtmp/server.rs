use std::{
    collections::HashMap,
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use dashmap::DashMap;

use super::{
    RtmpConfig, RtmpError, RtmpResult,
    connection::{ConnectionState, ConnectionStats, RtmpConnection},
    message::{AmfCommand, RtmpMessage},
    message_type, status,
};
use crate::protocol::amf0::Amf0Value;

/// RTMP Server for handling incoming client connections
pub struct RtmpServer {
    /// Server configuration
    config: RtmpConfig,
    /// Server listening address
    listen_addr: Option<SocketAddr>,
    /// Active connections (using DashMap for better concurrency)
    connections: DashMap<usize, Arc<Mutex<RtmpConnection>>>,
    /// Connection counter (using atomic for lock-free increment)
    next_connection_id: AtomicUsize,
    /// Server running state (using atomic for lock-free check)
    is_running: AtomicBool,
    /// Event handlers
    event_handlers: EventHandlers,
    /// Active streams (stream_name -> StreamInfo)
    streams: DashMap<String, StreamInfo>,
}

/// Stream information on the server
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub stream_name: String,
    pub publisher_id: usize,
    pub subscribers: Vec<usize>,
    pub metadata: Option<HashMap<String, Amf0Value>>,
    pub is_live: bool,
}

impl StreamInfo {
    pub fn new(stream_name: String, publisher_id: usize) -> Self {
        Self {
            stream_name,
            publisher_id,
            subscribers: Vec::new(),
            metadata: None,
            is_live: true,
        }
    }
}

/// Event handler callbacks
pub struct EventHandlers {
    pub on_connect: Option<Box<dyn Fn(usize, &AmfCommand) -> bool + Send + Sync>>,
    pub on_publish: Option<Box<dyn Fn(usize, &str) -> bool + Send + Sync>>,
    pub on_play: Option<Box<dyn Fn(usize, &str) -> bool + Send + Sync>>,
    pub on_disconnect: Option<Box<dyn Fn(usize) + Send + Sync>>,
    pub on_audio: Option<Box<dyn Fn(usize, &[u8], u32) + Send + Sync>>,
    pub on_video: Option<Box<dyn Fn(usize, &[u8], u32) + Send + Sync>>,
    pub on_metadata: Option<Box<dyn Fn(usize, &HashMap<String, Amf0Value>) + Send + Sync>>,
}

impl Default for EventHandlers {
    fn default() -> Self {
        Self {
            on_connect: None,
            on_publish: None,
            on_play: None,
            on_disconnect: None,
            on_audio: None,
            on_video: None,
            on_metadata: None,
        }
    }
}

impl RtmpServer {
    /// Create new RTMP server
    pub fn new(config: RtmpConfig) -> Self {
        Self {
            config,
            listen_addr: None,
            connections: DashMap::new(),
            next_connection_id: AtomicUsize::new(0),
            is_running: AtomicBool::new(false),
            event_handlers: EventHandlers::default(),
            streams: DashMap::new(),
        }
    }

    /// Create server with default configuration
    pub fn with_defaults() -> Self {
        Self::new(RtmpConfig::default())
    }

    /// Set connection handler
    pub fn on_connect<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &AmfCommand) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.on_connect = Some(Box::new(handler));
        self
    }

    /// Set publish handler
    pub fn on_publish<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &str) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.on_publish = Some(Box::new(handler));
        self
    }

    /// Set play handler
    pub fn on_play<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &str) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.on_play = Some(Box::new(handler));
        self
    }

    /// Set disconnect handler
    pub fn on_disconnect<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_handlers.on_disconnect = Some(Box::new(handler));
        self
    }

    /// Set audio data handler
    pub fn on_audio<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &[u8], u32) + Send + Sync + 'static,
    {
        self.event_handlers.on_audio = Some(Box::new(handler));
        self
    }

    /// Set video data handler
    pub fn on_video<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &[u8], u32) + Send + Sync + 'static,
    {
        self.event_handlers.on_video = Some(Box::new(handler));
        self
    }

    /// Set metadata handler
    pub fn on_metadata<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize, &HashMap<String, Amf0Value>) + Send + Sync + 'static,
    {
        self.event_handlers.on_metadata = Some(Box::new(handler));
        self
    }

    /// Start listening on the specified address
    pub fn listen(&mut self, addr: &str) -> RtmpResult<()> {
        let listener = TcpListener::bind(addr)
            .map_err(|e| RtmpError::ConnectionFailed(format!("Failed to bind: {}", e)))?;

        self.listen_addr = Some(listener.local_addr()?);
        self.is_running.store(true, Ordering::SeqCst);

        println!("RTMP Server listening on {}", addr);

        // Accept connections
        for stream in listener.incoming() {
            if !self.is_running.load(Ordering::SeqCst) {
                break;
            }

            match stream {
                Ok(tcp_stream) => {
                    let connection_id = self.get_next_connection_id();
                    let connection = Arc::new(Mutex::new(RtmpConnection::new(self.config.clone())));

                    // Store connection (DashMap - no explicit lock needed)
                    self.connections.insert(connection_id, connection.clone());

                    // Clone references for the spawned thread
                    let connections = self.connections.clone();
                    let streams = self.streams.clone();

                    thread::spawn(move || {
                        if let Err(e) = Self::handle_client_connection(
                            connection_id,
                            tcp_stream,
                            connection,
                            connections,
                            streams,
                        ) {
                            eprintln!("Connection {} error: {}", connection_id, e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Failed to accept connection: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Stop the server
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);

        // Close all connections (DashMap - no explicit lock needed)
        self.connections.clear();

        // Clear all streams
        self.streams.clear();
    }

    /// Check if server is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Get server statistics
    pub fn get_stats(&self) -> ServerStats {
        ServerStats {
            connection_count: self.connections.len(),
            stream_count: self.streams.len(),
            is_running: self.is_running(),
            listen_addr: self.listen_addr,
        }
    }

    /// Get all active connections
    pub fn get_connections(&self) -> Vec<(usize, ConnectionStats)> {
        let mut result = Vec::new();

        for entry in self.connections.iter() {
            let (id, connection) = entry.pair();
            if let Ok(conn) = connection.try_lock() {
                result.push((*id, conn.get_stats()));
            }
        }

        result
    }

    /// Get all active streams
    pub fn get_streams(&self) -> Vec<StreamInfo> {
        self.streams
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Broadcast message to all subscribers of a stream
    pub fn broadcast_to_stream(&self, stream_name: &str, _message: &RtmpMessage) -> RtmpResult<()> {
        if let Some(stream_info) = self.streams.get(stream_name) {
            for &subscriber_id in &stream_info.subscribers {
                if let Some(connection) = self.connections.get(&subscriber_id) {
                    if let Ok(_conn) = connection.try_lock() {
                        // This would need access to the TcpStream, which requires refactoring
                        // For now, we'll leave this as a placeholder
                    }
                }
            }
        }

        Ok(())
    }

    /// Get next connection ID (lock-free using atomic)
    fn get_next_connection_id(&self) -> usize {
        self.next_connection_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Handle individual client connection
    fn handle_client_connection(
        connection_id: usize,
        mut stream: TcpStream,
        connection: Arc<Mutex<RtmpConnection>>,
        connections: DashMap<usize, Arc<Mutex<RtmpConnection>>>,
        streams: DashMap<String, StreamInfo>,
    ) -> RtmpResult<()> {
        println!("New connection: {}", connection_id);

        // Set timeouts
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;

        // Perform handshake
        {
            let mut conn = connection.lock().unwrap();
            conn.server_handshake(&mut stream)?;
        }

        // Main message processing loop
        loop {
            let message = {
                let mut conn = connection.lock().unwrap();

                // Check for timeout
                if conn.is_timed_out() {
                    break;
                }

                conn.read_chunk(&mut stream)?
            };

            if let Some(msg) = message {
                Self::process_client_message(
                    connection_id,
                    &mut stream,
                    &connection,
                    &streams,
                    &msg,
                )?;
            }
        }

        // Clean up connection (DashMap - no explicit lock needed)
        connections.remove(&connection_id);

        // Remove from any streams
        Self::cleanup_connection_streams(connection_id, &streams);

        println!("Connection {} closed", connection_id);
        Ok(())
    }

    /// Process message from client
    fn process_client_message(
        connection_id: usize,
        stream: &mut TcpStream,
        connection: &Arc<Mutex<RtmpConnection>>,
        streams: &DashMap<String, StreamInfo>,
        message: &RtmpMessage,
    ) -> RtmpResult<()> {
        match message.header.message_type {
            message_type::AMF0_COMMAND => {
                let command = message.parse_amf0_command()?;
                Self::handle_command(connection_id, stream, connection, streams, &command)?;
            }
            message_type::AUDIO => {
                Self::handle_audio_message(connection_id, streams, message)?;
            }
            message_type::VIDEO => {
                Self::handle_video_message(connection_id, streams, message)?;
            }
            message_type::AMF0_DATA => {
                Self::handle_data_message(connection_id, streams, message)?;
            }
            _ => {
                // Handle other control messages
                let mut conn = connection.lock().unwrap();
                conn.process_message(stream, message)?;
            }
        }
        Ok(())
    }

    /// Handle AMF command
    fn handle_command(
        connection_id: usize,
        stream: &mut TcpStream,
        connection: &Arc<Mutex<RtmpConnection>>,
        streams: &DashMap<String, StreamInfo>,
        command: &AmfCommand,
    ) -> RtmpResult<()> {
        let mut conn = connection.lock().unwrap();

        match command.command_name.as_str() {
            "connect" => {
                // Get config values before borrowing mutably
                let window_ack_size = conn.config.window_ack_size;
                let peer_bandwidth = conn.config.peer_bandwidth;
                let chunk_size = conn.config.chunk_size;

                // Send control messages
                conn.send_window_ack_size(stream, window_ack_size)?;
                conn.send_peer_bandwidth(stream, peer_bandwidth, 2)?;
                conn.set_chunk_size(stream, chunk_size)?;

                // Send connect result
                conn.send_connect_result(stream, command.transaction_id)?;

                conn.set_state(ConnectionState::Connected);
            }
            "createStream" => {
                let stream_id = conn.next_stream_id();
                conn.send_create_stream_result(stream, command.transaction_id, stream_id)?;
            }
            "publish" => {
                if let Some(Amf0Value::String(stream_name)) = command.arguments.first() {
                    // Register stream (DashMap - no explicit lock needed)
                    let stream_info = StreamInfo::new(stream_name.clone(), connection_id);
                    streams.insert(stream_name.clone(), stream_info);

                    // Send success status
                    conn.send_on_status(
                        stream,
                        "status",
                        status::NETSTREAM_PUBLISH_START,
                        &format!("Started publishing stream '{}'", stream_name),
                        1, // Use default stream ID
                    )?;

                    conn.set_state(ConnectionState::Publishing);
                }
            }
            "play" => {
                if let Some(Amf0Value::String(stream_name)) = command.arguments.first() {
                    // Add as subscriber (DashMap - fine-grained locking)
                    if let Some(mut stream_info) = streams.get_mut(stream_name) {
                        stream_info.subscribers.push(connection_id);
                    }

                    // Send success status
                    conn.send_on_status(
                        stream,
                        "status",
                        status::NETSTREAM_PLAY_START,
                        &format!("Started playing stream '{}'", stream_name),
                        1, // Use default stream ID
                    )?;

                    conn.set_state(ConnectionState::Playing);
                }
            }
            _ => {
                // Unknown command
            }
        }

        Ok(())
    }

    /// Handle audio message
    fn handle_audio_message(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
        _message: &RtmpMessage,
    ) -> RtmpResult<()> {
        // Find stream by publisher ID and broadcast to subscribers
        for entry in streams.iter() {
            if entry.publisher_id == connection_id {
                // TODO: Broadcast audio to subscribers
                // This requires access to subscriber connections and their TcpStreams
                break;
            }
        }
        Ok(())
    }

    /// Handle video message
    fn handle_video_message(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
        _message: &RtmpMessage,
    ) -> RtmpResult<()> {
        // Find stream by publisher ID and broadcast to subscribers
        for entry in streams.iter() {
            if entry.publisher_id == connection_id {
                // TODO: Broadcast video to subscribers
                // This requires access to subscriber connections and their TcpStreams
                break;
            }
        }
        Ok(())
    }

    /// Handle data message (metadata)
    fn handle_data_message(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
        _message: &RtmpMessage,
    ) -> RtmpResult<()> {
        // Parse metadata if it's onMetaData
        // This is simplified - real implementation would parse AMF data
        for entry in streams.iter_mut() {
            if entry.publisher_id == connection_id {
                // TODO: Parse and store metadata
                break;
            }
        }
        Ok(())
    }

    /// Clean up connection from all streams
    fn cleanup_connection_streams(connection_id: usize, streams: &DashMap<String, StreamInfo>) {
        let mut to_remove = Vec::new();

        for mut entry in streams.iter_mut() {
            if entry.publisher_id == connection_id {
                // Remove publisher stream
                to_remove.push(entry.key().clone());
            } else {
                // Remove from subscribers
                entry.subscribers.retain(|&id| id != connection_id);
            }
        }

        for stream_name in to_remove {
            streams.remove(&stream_name);
        }
    }
}

/// Server statistics
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub connection_count: usize,
    pub stream_count: usize,
    pub is_running: bool,
    pub listen_addr: Option<SocketAddr>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = RtmpServer::with_defaults();
        assert!(!server.is_running());
        assert_eq!(server.get_stats().connection_count, 0);
    }

    #[test]
    fn test_server_event_handlers() {
        let server = RtmpServer::with_defaults()
            .on_connect(|id, _| {
                println!("Client {} connected", id);
                true
            })
            .on_publish(|id, stream| {
                println!("Client {} publishing {}", id, stream);
                true
            })
            .on_disconnect(|id| {
                println!("Client {} disconnected", id);
            });

        // Event handlers are set (can't easily test the closures)
        assert!(server.event_handlers.on_connect.is_some());
        assert!(server.event_handlers.on_publish.is_some());
        assert!(server.event_handlers.on_disconnect.is_some());
    }

    #[test]
    fn test_server_stats() {
        let server = RtmpServer::with_defaults();
        let stats = server.get_stats();

        assert_eq!(stats.connection_count, 0);
        assert_eq!(stats.stream_count, 0);
        assert!(!stats.is_running);
        assert!(stats.listen_addr.is_none());
    }

    #[test]
    fn test_stream_info() {
        let stream_info = StreamInfo::new("test_stream".to_string(), 1);

        assert_eq!(stream_info.stream_name, "test_stream");
        assert_eq!(stream_info.publisher_id, 1);
        assert!(stream_info.subscribers.is_empty());
        assert!(stream_info.is_live);
        assert!(stream_info.metadata.is_none());
    }

    #[test]
    fn test_dashmap_concurrent_access() {
        let server = RtmpServer::with_defaults();

        // Test concurrent insert and read
        let conn_id = 1;
        let connection = Arc::new(Mutex::new(RtmpConnection::new(RtmpConfig::default())));
        server.connections.insert(conn_id, connection);

        assert_eq!(server.connections.len(), 1);
        assert!(server.connections.contains_key(&conn_id));

        server.connections.remove(&conn_id);
        assert_eq!(server.connections.len(), 0);
    }

    #[test]
    fn test_atomic_counter() {
        let server = RtmpServer::with_defaults();

        // Test atomic increment
        let id1 = server.get_next_connection_id();
        let id2 = server.get_next_connection_id();
        let id3 = server.get_next_connection_id();

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 2);
    }

    #[test]
    fn test_atomic_running_state() {
        let mut server = RtmpServer::with_defaults();

        assert!(!server.is_running());

        server.is_running.store(true, Ordering::SeqCst);
        assert!(server.is_running());

        server.stop();
        assert!(!server.is_running());
    }
}
