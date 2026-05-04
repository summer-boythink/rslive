//! # rslive
//!
//! A high-performance, comprehensive Rust library for live streaming protocols
//! and multimedia processing, designed for high-performance streaming applications,
//! RTMP servers, and real-time media transmission.
//!
//! ## Features
//!
//! - **Protocol Support**: RTMP, FLV, HLS, SRT, WebRTC, DASH
//! - **Zero-Copy**: Efficient frame forwarding with shared memory
//! - **Async-First**: Built on Tokio for high concurrency
//! - **Modular Design**: Protocol adapters with unified abstractions
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use rslive::rtmp::RtmpServer;
//!
//! fn main() -> anyhow::Result<()> {
//!     let mut server = RtmpServer::with_defaults();
//!     server.listen("0.0.0.0:1935")?;
//!
//!     // Server is now running and accepting connections
//!     // Call server.stop() to shut down
//!     Ok(())
//! }
//! ```

pub mod media;
pub mod protocol;
pub mod utils;

// Re-export for convenience
pub use media::{CodecType, MediaFrame, RouterConfig, StreamRouter, Timestamp};
pub use utils::BufferPool;

// Protocol re-exports
pub mod rtmp {
    pub use crate::protocol::rtmp::*;
}

pub mod flv {
    pub use crate::protocol::flv::*;
}

pub mod hls {
    pub use crate::protocol::hls::*;
}

pub mod amf0 {
    pub use crate::protocol::amf0::*;
}

pub mod amf3 {
    pub use crate::protocol::amf3::*;
}

// Version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
