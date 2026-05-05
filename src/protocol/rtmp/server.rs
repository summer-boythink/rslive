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
use crate::media::{MediaFrame, StreamId, StreamPublisher, StreamRouter, Timestamp};
use crate::protocol::amf0::Amf0Value;
use crate::protocol::flv::{AudioTagHeader, VideoTagHeader};
use crate::media::{CodecType, FrameType};
use crate::media::frame::{AudioFrameType, VideoFrameType};

/// RTMP Server for handling incoming client connections
pub struct RtmpServer {
    /// Server configuration
    config: RtmpConfig,
    /// Server listening address
    listen_addr: Option<SocketAddr>,
    /// Active connections (using DashMap for better concurrency)
    connections: Arc<DashMap<usize, Arc<Mutex<RtmpConnection>>>>,
    /// Connection counter (using atomic for lock-free increment)
    next_connection_id: AtomicUsize,
    /// Server running state (using atomic for lock-free check)
    is_running: AtomicBool,
    /// Event handlers
    event_handlers: EventHandlers,
    /// Active streams (stream_name -> StreamInfo)
    streams: Arc<DashMap<String, StreamInfo>>,
    /// Reference to StreamRouter for forwarding media frames
    router: Option<Arc<StreamRouter>>,
    /// Active publishers (stream_name -> StreamPublisher)
    publishers: Arc<DashMap<String, StreamPublisher>>,
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
            connections: Arc::new(DashMap::new()),
            next_connection_id: AtomicUsize::new(0),
            is_running: AtomicBool::new(false),
            event_handlers: EventHandlers::default(),
            streams: Arc::new(DashMap::new()),
            router: None,
            publishers: Arc::new(DashMap::new()),
        }
    }

    /// Set the StreamRouter for forwarding media frames
    pub fn set_router(&mut self, router: Arc<StreamRouter>) {
        self.router = Some(router);
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

        // Set listener to blocking mode (avoid EAGAIN errors)
        listener.set_nonblocking(false)?;

        self.listen_addr = Some(listener.local_addr()?);
        self.is_running.store(true, Ordering::SeqCst);

        eprintln!("[RTMP] Server listening on {}", addr);
        std::io::Write::flush(&mut std::io::stderr()).ok();

        // Accept connections
        loop {
            if !self.is_running.load(Ordering::SeqCst) {
                break;
            }

            match listener.accept() {
                Ok((tcp_stream, peer_addr)) => {
                    eprintln!("[RTMP] New client connected: {}", peer_addr);
                    std::io::Write::flush(&mut std::io::stderr()).ok();

                    let connection_id = self.get_next_connection_id();
                    let connection = Arc::new(Mutex::new(RtmpConnection::new(self.config.clone())));

                    // Store connection
                    self.connections.insert(connection_id, connection.clone());

                    // Clone references for the spawned thread
                    let connections = Arc::clone(&self.connections);
                    let streams = Arc::clone(&self.streams);
                    let router = self.router.clone();
                    let publishers = Arc::clone(&self.publishers);

                    thread::spawn(move || {
                        eprintln!("[RTMP] Connection {}: Handler thread started", connection_id);
                        std::io::Write::flush(&mut std::io::stderr()).ok();
                        if let Err(e) = Self::handle_client_connection(
                            connection_id,
                            tcp_stream,
                            connection,
                            connections,
                            streams,
                            router,
                            publishers,
                        ) {
                            eprintln!("[RTMP] Connection {}: Handler error: {}", connection_id, e);
                            std::io::Write::flush(&mut std::io::stderr()).ok();
                        }
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // EAGAIN - temporary resource unavailable, retry
                    eprintln!("[RTMP] accept() would block, retrying...");
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) => {
                    eprintln!("[RTMP] Failed to accept connection: {}", e);
                    std::io::Write::flush(&mut std::io::stderr()).ok();
                    thread::sleep(Duration::from_millis(100));
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
        connections: Arc<DashMap<usize, Arc<Mutex<RtmpConnection>>>>,
        streams: Arc<DashMap<String, StreamInfo>>,
        router: Option<Arc<StreamRouter>>,
        publishers: Arc<DashMap<String, StreamPublisher>>,
    ) -> RtmpResult<()> {
        eprintln!("[RTMP] Connection {}: Starting handler", connection_id);
        std::io::Write::flush(&mut std::io::stderr()).ok();

        // Ensure blocking mode for reliable handshake
        stream.set_nonblocking(false)?;

        // Set timeouts and TCP options
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;

        // Disable Nagle's algorithm for lower latency
        stream.set_nodelay(true)?;

        eprintln!("[RTMP] Connection {}: Starting handshake", connection_id);
        std::io::Write::flush(&mut std::io::stderr()).ok();

        // Perform handshake with better error handling
        {
            let mut conn = connection.lock().unwrap();
            match conn.server_handshake(&mut stream) {
                Ok(_) => {
                    eprintln!("[RTMP] Connection {}: Handshake successful", connection_id);
                    std::io::Write::flush(&mut std::io::stderr()).ok();
                }
                Err(e) => {
                    eprintln!("[RTMP] Connection {}: Handshake failed: {}", connection_id, e);
                    std::io::Write::flush(&mut std::io::stderr()).ok();
                    return Err(e);
                }
            }
        }

        // Main message processing loop
        eprintln!("[RTMP] Connection {}: Entering main loop", connection_id);
        std::io::Write::flush(&mut std::io::stderr()).ok();

        loop {
            let message = match {
                let mut conn = connection.lock().unwrap();

                // Check for timeout
                if conn.is_timed_out() {
                    eprintln!("[RTMP] Connection {}: Connection timed out", connection_id);
                    break;
                }

                conn.read_chunk(&mut stream)
            } {
                Ok(msg) => msg,
                Err(RtmpError::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // Normal connection close or timeout
                    eprintln!("[RTMP] Connection {}: Connection closed ({:?})", connection_id, e.kind());
                    break;
                }
                Err(e) => {
                    eprintln!("[RTMP] Connection {} read error: {}", connection_id, e);
                    break;
                }
            };

            if let Some(msg) = message {
                if let Err(e) = Self::process_client_message(
                    connection_id,
                    &mut stream,
                    &connection,
                    &streams,
                    &msg,
                    &router,
                    &publishers,
                ) {
                    println!("Connection {} message error: {}", connection_id, e);
                    break;
                }
            }
        }

        // Clean up connection (DashMap - no explicit lock needed)
        connections.remove(&connection_id);

        // Clean up publishers for this connection
        Self::cleanup_publishers(connection_id, &streams, &publishers, &router);

        // Remove from any streams
        Self::cleanup_connection_streams(connection_id, &streams);

        eprintln!("[RTMP] Connection {}: Closed", connection_id);
        std::io::Write::flush(&mut std::io::stderr()).ok();
        Ok(())
    }

    /// Clean up publishers when connection closes
    fn cleanup_publishers(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
        publishers: &DashMap<String, StreamPublisher>,
        router: &Option<Arc<StreamRouter>>,
    ) {
        // Find all streams published by this connection and remove their publishers
        for entry in streams.iter() {
            if entry.publisher_id == connection_id {
                let stream_name = entry.key().clone();
                publishers.remove(&stream_name);

                // 确保在中央 Router 中也将这个流注销，避免重推流被占用
                if let Some(router) = router {
                    router.unpublish(&StreamId::new(stream_name.as_str()));
                }

                println!("Removed publisher for stream: {}", stream_name);
            }
        }
    }

    /// Process message from client
    fn process_client_message(
        connection_id: usize,
        stream: &mut TcpStream,
        connection: &Arc<Mutex<RtmpConnection>>,
        streams: &DashMap<String, StreamInfo>,
        message: &RtmpMessage,
        router: &Option<Arc<StreamRouter>>,
        publishers: &DashMap<String, StreamPublisher>,
    ) -> RtmpResult<()> {
        match message.header.message_type {
            message_type::AMF0_COMMAND => {
                let command = message.parse_amf0_command()?;
                Self::handle_command(connection_id, stream, connection, streams, &command, router, publishers)?;
            }
            message_type::AUDIO => {
                Self::handle_audio_message_static(connection_id, streams, message, router, publishers)?;
            }
            message_type::VIDEO => {
                Self::handle_video_message_static(connection_id, streams, message, router, publishers)?;
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
        router: &Option<Arc<StreamRouter>>,
        publishers: &DashMap<String, StreamPublisher>,
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

                    // Create StreamPublisher in router if router is configured
                    if let Some(router) = router {
                        let stream_id = StreamId::new(stream_name.as_str());

                        // 尝试发布流，如果发现已有残留的僵尸推流端，则踢掉旧的重新发布
                        let publish_result = router.publish(stream_id.clone()).or_else(|_| {
                            router.unpublish(&stream_id);
                            router.publish(stream_id.clone())
                        });

                        match publish_result {
                            Ok(publisher) => {
                                publishers.insert(stream_name.clone(), publisher);
                                println!("Created StreamPublisher for stream: {}", stream_name);
                            }
                            Err(e) => {
                                eprintln!("Failed to create StreamPublisher for {}: {}", stream_name, e);
                            }
                        }
                    }

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

    /// Static handler for audio messages (used in connection threads)
    fn handle_audio_message_static(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
        message: &RtmpMessage,
        _router: &Option<Arc<StreamRouter>>,
        publishers: &DashMap<String, StreamPublisher>,
    ) -> RtmpResult<()> {
        // Find stream name by publisher ID
        let stream_name = Self::get_stream_name_by_publisher_static(connection_id, streams);
        if stream_name.is_none() {
            return Ok(());
        }
        let stream_name = stream_name.unwrap();

        // Get publisher for this stream
        let publisher = match publishers.get(&stream_name) {
            Some(p) => p,
            None => return Ok(()), // No publisher yet, skip
        };

        // Parse FLV audio tag header
        let header = match AudioTagHeader::decode(&message.payload) {
            Some(h) => h,
            None => return Ok(()), // Invalid header, skip
        };

        // Determine header length
        let header_len = if header.aac_packet_type.is_some() { 2 } else { 1 };
        let frame_data = bytes::Bytes::copy_from_slice(&message.payload[header_len..]);

        // Map sound format to codec
        let codec = match header.sound_format {
            crate::protocol::common::SoundFormat::Aac => CodecType::AAC,
            crate::protocol::common::SoundFormat::Mp3 => CodecType::Mp3,
            crate::protocol::common::SoundFormat::G711ALaw => CodecType::G711A,
            crate::protocol::common::SoundFormat::G711MuLaw => CodecType::G711U,
            _ => return Ok(()), // Unsupported codec, skip
        };

        let frame_type = if header.is_sequence_header() {
            AudioFrameType::SequenceHeader
        } else {
            AudioFrameType::Raw
        };

        // Create timestamp from message timestamp
        let pts = Timestamp::from_millis(message.header.timestamp as u64);

        // Create MediaFrame
        let frame = MediaFrame::new(
            2, // Audio track ID
            pts,
            FrameType::Audio(frame_type),
            codec,
            frame_data,
        );

        // Publish frame using try_publish (non-blocking)
        let _ = publisher.try_publish(frame);

        Ok(())
    }

    /// Static handler for video messages (used in connection threads)
    fn handle_video_message_static(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
        message: &RtmpMessage,
        _router: &Option<Arc<StreamRouter>>,
        publishers: &DashMap<String, StreamPublisher>,
    ) -> RtmpResult<()> {
        // Find stream name by publisher ID
        let stream_name = Self::get_stream_name_by_publisher_static(connection_id, streams);
        if stream_name.is_none() {
            return Ok(());
        }
        let stream_name = stream_name.unwrap();

        // Get publisher for this stream
        let publisher = match publishers.get(&stream_name) {
            Some(p) => p,
            None => return Ok(()), // No publisher yet, skip
        };

        // Parse FLV video tag header
        let header = match VideoTagHeader::decode(&message.payload) {
            Some(h) => h,
            None => return Ok(()), // Invalid header, skip
        };

        // Determine header length
        let header_len = if header.avc_packet_type.is_some() { 5 } else { 1 };
        let frame_data = bytes::Bytes::copy_from_slice(&message.payload[header_len..]);

        // Map codec ID
        let codec = match header.codec_id {
            crate::protocol::common::VideoCodecId::Avc => CodecType::H264,
            crate::protocol::common::VideoCodecId::Hevc => CodecType::H265,
            _ => return Ok(()), // Unsupported codec, skip
        };

        // Map frame type
        let frame_type = if header.is_sequence_header() {
            VideoFrameType::SequenceHeader
        } else {
            match header.frame_type {
                crate::protocol::common::VideoFrameType::Keyframe => VideoFrameType::Keyframe,
                crate::protocol::common::VideoFrameType::Interframe => VideoFrameType::Interframe,
                crate::protocol::common::VideoFrameType::DisposableInterframe => VideoFrameType::DisposableInterframe,
                crate::protocol::common::VideoFrameType::GeneratedKeyframe => VideoFrameType::GeneratedKeyframe,
                _ => VideoFrameType::Interframe,
            }
        };

        // Calculate timestamps
        let pts = Timestamp::from_millis(message.header.timestamp as u64);
        let dts = if header.composition_time >= 0 {
            pts - std::time::Duration::from_millis(header.composition_time as u64)
        } else {
            pts + std::time::Duration::from_millis((-header.composition_time) as u64)
        };

        // Create MediaFrame with DTS
        let frame = MediaFrame::with_dts(
            1, // Video track ID
            pts,
            dts,
            FrameType::Video(frame_type),
            codec,
            frame_data,
        );

        // Publish frame using try_publish (non-blocking)
        let _ = publisher.try_publish(frame);

        Ok(())
    }

    /// Get stream name by publisher connection ID (static version)
    fn get_stream_name_by_publisher_static(
        connection_id: usize,
        streams: &DashMap<String, StreamInfo>,
    ) -> Option<String> {
        for entry in streams.iter() {
            if entry.publisher_id == connection_id {
                return Some(entry.key().clone());
            }
        }
        None
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
