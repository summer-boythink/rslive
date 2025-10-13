pub mod chunk;
pub mod client;
pub mod connection;
pub mod handshake;
pub mod message;
pub mod server;

use std::io;

/// RTMP protocol version
pub const RTMP_VERSION: u8 = 3;

/// Default RTMP port
pub const RTMP_DEFAULT_PORT: u16 = 1935;

/// Default chunk size
pub const RTMP_DEFAULT_CHUNK_SIZE: u32 = 128;

/// Maximum chunk size
pub const RTMP_MAX_CHUNK_SIZE: u32 = 0xFFFFFF;

/// RTMP handshake size
pub const RTMP_HANDSHAKE_SIZE: usize = 1536;

/// RTMP message header sizes
pub const RTMP_MESSAGE_HEADER_SIZE_12: u8 = 0;
pub const RTMP_MESSAGE_HEADER_SIZE_8: u8 = 1;
pub const RTMP_MESSAGE_HEADER_SIZE_4: u8 = 2;
pub const RTMP_MESSAGE_HEADER_SIZE_1: u8 = 3;

/// RTMP message types
pub mod message_type {
    /// Set Chunk Size
    pub const SET_CHUNK_SIZE: u8 = 1;
    /// Abort Message
    pub const ABORT_MESSAGE: u8 = 2;
    /// Acknowledgement
    pub const ACKNOWLEDGEMENT: u8 = 3;
    /// Window Acknowledgement Size
    pub const WINDOW_ACKNOWLEDGEMENT_SIZE: u8 = 5;
    /// Set Peer Bandwidth
    pub const SET_PEER_BANDWIDTH: u8 = 6;
    /// Audio Message
    pub const AUDIO: u8 = 8;
    /// Video Message
    pub const VIDEO: u8 = 9;
    /// AMF3 Data Message
    pub const AMF3_DATA: u8 = 15;
    /// AMF3 Shared Object
    pub const AMF3_SHARED_OBJECT: u8 = 16;
    /// AMF3 Command Message
    pub const AMF3_COMMAND: u8 = 17;
    /// AMF0 Data Message
    pub const AMF0_DATA: u8 = 18;
    /// AMF0 Shared Object
    pub const AMF0_SHARED_OBJECT: u8 = 19;
    /// AMF0 Command Message
    pub const AMF0_COMMAND: u8 = 20;
    /// Aggregate Message
    pub const AGGREGATE: u8 = 22;
}

/// RTMP chunk stream IDs
pub mod chunk_stream_id {
    /// Control chunk stream
    pub const CONTROL: u32 = 2;
    /// Command chunk stream
    pub const COMMAND: u32 = 3;
    /// Audio chunk stream
    pub const AUDIO: u32 = 4;
    /// Video chunk stream
    pub const VIDEO: u32 = 5;
}

/// RTMP command names
pub mod command {
    pub const CONNECT: &str = "connect";
    pub const CONNECT_RESULT: &str = "_result";
    pub const CONNECT_ERROR: &str = "_error";
    pub const CALL: &str = "call";
    pub const CREATE_STREAM: &str = "createStream";
    pub const CLOSE_STREAM: &str = "closeStream";
    pub const DELETE_STREAM: &str = "deleteStream";
    pub const PUBLISH: &str = "publish";
    pub const PLAY: &str = "play";
    pub const PLAY2: &str = "play2";
    pub const PAUSE: &str = "pause";
    pub const SEEK: &str = "seek";
    pub const ON_STATUS: &str = "onStatus";
    pub const ON_METADATA: &str = "onMetaData";
    pub const ON_CUE_POINT: &str = "onCuePoint";
    pub const ON_FI: &str = "onFI";
    pub const FC_UNPUBLISH: &str = "FCUnpublish";
    pub const FC_PUBLISH: &str = "FCPublish";
    pub const FC_SUBSCRIBE: &str = "FCSubscribe";
    pub const FC_UNSUBSCRIBE: &str = "FCUnsubscribe";
}

/// RTMP status codes
pub mod status {
    pub const NETCONNECTION_CONNECT_SUCCESS: &str = "NetConnection.Connect.Success";
    pub const NETCONNECTION_CONNECT_FAILED: &str = "NetConnection.Connect.Failed";
    pub const NETCONNECTION_CONNECT_CLOSED: &str = "NetConnection.Connect.Closed";
    pub const NETCONNECTION_CONNECT_REJECTED: &str = "NetConnection.Connect.Rejected";

    pub const NETSTREAM_PUBLISH_START: &str = "NetStream.Publish.Start";
    pub const NETSTREAM_PUBLISH_FAILED: &str = "NetStream.Publish.Failed";
    pub const NETSTREAM_UNPUBLISH_SUCCESS: &str = "NetStream.Unpublish.Success";

    pub const NETSTREAM_PLAY_START: &str = "NetStream.Play.Start";
    pub const NETSTREAM_PLAY_STOP: &str = "NetStream.Play.Stop";
    pub const NETSTREAM_PLAY_FAILED: &str = "NetStream.Play.Failed";
    pub const NETSTREAM_PLAY_STREAMNOTFOUND: &str = "NetStream.Play.StreamNotFound";
    pub const NETSTREAM_PLAY_RESET: &str = "NetStream.Play.Reset";
    pub const NETSTREAM_PLAY_PUBLISHNOTIFY: &str = "NetStream.Play.PublishNotify";
    pub const NETSTREAM_PLAY_UNPUBLISHNOTIFY: &str = "NetStream.Play.UnpublishNotify";
}

/// RTMP error types
#[derive(Debug, thiserror::Error)]
pub enum RtmpError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid chunk format: {0}")]
    InvalidChunkFormat(u8),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    #[error("Invalid chunk stream ID: {0}")]
    InvalidChunkStreamId(u32),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("AMF error: {0}")]
    Amf(String),

    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Timeout")]
    Timeout,
}

/// RTMP result type
pub type RtmpResult<T> = Result<T, RtmpError>;

/// RTMP configuration
#[derive(Debug, Clone)]
pub struct RtmpConfig {
    /// Maximum chunk size
    pub chunk_size: u32,
    /// Window acknowledgement size
    pub window_ack_size: u32,
    /// Peer bandwidth
    pub peer_bandwidth: u32,
    /// Connection timeout in seconds
    pub timeout: u64,
    /// Enable authentication
    pub enable_auth: bool,
    /// Maximum concurrent connections
    pub max_connections: usize,
}

impl Default for RtmpConfig {
    fn default() -> Self {
        Self {
            chunk_size: RTMP_DEFAULT_CHUNK_SIZE,
            window_ack_size: 2500000,
            peer_bandwidth: 2500000,
            timeout: 30,
            enable_auth: false,
            max_connections: 1000,
        }
    }
}

impl RtmpConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_chunk_size(mut self, chunk_size: u32) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn with_window_ack_size(mut self, window_ack_size: u32) -> Self {
        self.window_ack_size = window_ack_size;
        self
    }

    pub fn with_peer_bandwidth(mut self, peer_bandwidth: u32) -> Self {
        self.peer_bandwidth = peer_bandwidth;
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_auth(mut self, enable_auth: bool) -> Self {
        self.enable_auth = enable_auth;
        self
    }

    pub fn with_max_connections(mut self, max_connections: usize) -> Self {
        self.max_connections = max_connections;
        self
    }
}

// Re-export main types
pub use chunk::*;
pub use client::RtmpClient;
pub use connection::{
    ConnectionState, ConnectionStats, RtmpConnection, StreamInfo as ConnectionStreamInfo,
};
pub use handshake::{RtmpHandshake, SimpleHandshake};
pub use message::*;
pub use server::{EventHandlers, RtmpServer, ServerStats, StreamInfo};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtmp_config() {
        let config = RtmpConfig::new()
            .with_chunk_size(4096)
            .with_timeout(60)
            .with_auth(true);

        assert_eq!(config.chunk_size, 4096);
        assert_eq!(config.timeout, 60);
        assert_eq!(config.enable_auth, true);
    }

    #[test]
    fn test_message_types() {
        assert_eq!(message_type::AUDIO, 8);
        assert_eq!(message_type::VIDEO, 9);
        assert_eq!(message_type::AMF0_COMMAND, 20);
    }
}
