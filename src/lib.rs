//! # rslive
//!
//! A comprehensive Rust implementation of Adobe's Action Message Format (AMF) protocol.
//! This library provides encoding and decoding capabilities for AMF0 format, which is
//! commonly used in Flash applications and RTMP streaming.
//!
//! ## Features
//!
//! - Complete AMF0 value type support (Number, Boolean, String, Object, Array, etc.)
//! - Efficient encoding and decoding with proper error handling
//! - Reference handling for circular object references
//! - Support for complex nested data structures
//! - Utility functions for common operations
//! - Comprehensive test coverage
//!
//! ## Quick Start
//!
//! ```rust
//! use rslive::amf0::{Amf0Value, encode, decode};
//! use std::collections::HashMap;
//!
//! // Create an AMF0 object
//! let mut obj = HashMap::new();
//! obj.insert("name".to_string(), Amf0Value::String("John Doe".to_string()));
//! obj.insert("age".to_string(), Amf0Value::Number(30.0));
//! obj.insert("active".to_string(), Amf0Value::Boolean(true));
//!
//! let value = Amf0Value::Object(obj);
//!
//! // Encode to bytes
//! let encoded = encode(&value).unwrap();
//! println!("Encoded {} bytes", encoded.len());
//!
//! // Decode back to value
//! let decoded = decode(&encoded).unwrap();
//! assert_eq!(value, decoded);
//! ```
//!
//! ## AMF0 Value Types
//!
//! The library supports all AMF0 data types:
//!
//! - **Number**: 64-bit IEEE 754 floating point numbers
//! - **Boolean**: True/false values
//! - **String**: UTF-8 strings up to 65535 characters
//! - **LongString**: UTF-8 strings longer than 65535 characters
//! - **Object**: Key-value pairs (similar to HashMap)
//! - **StrictArray**: Indexed arrays (similar to Vec)
//! - **EcmaArray**: Associative arrays with numeric indices
//! - **Date**: Timestamps as milliseconds since epoch
//! - **Null**: Null values
//! - **Undefined**: Undefined values
//! - **Reference**: References to previously encoded objects
//! - **TypedObject**: Objects with class names
//! - **XmlDocument**: XML document strings
//! - **Unsupported**: Marker for unsupported types
//!
//! ## Advanced Usage
//!
//! ### Creating Complex Objects
//!
//! ```rust
//! use rslive::amf0::Amf0Value;
//!
//! // Using utility functions
//! let user = Amf0Value::object(vec![
//!     ("id".to_string(), Amf0Value::Number(1.0)),
//!     ("profile".to_string(), Amf0Value::object(vec![
//!         ("name".to_string(), Amf0Value::String("John".to_string())),
//!         ("skills".to_string(), Amf0Value::array(vec![
//!             Amf0Value::String("Rust".to_string()),
//!             Amf0Value::String("JavaScript".to_string()),
//!         ])),
//!     ])),
//! ]);
//! ```
//!
//! ### Manual Encoding/Decoding
//!
//! ```rust
//! use rslive::amf0::{Amf0Encoder, Amf0Decoder, Amf0Value};
//! use std::io::Cursor;
//!
//! let value = Amf0Value::String("Hello, AMF!".to_string());
//!
//! // Manual encoding
//! let mut buffer = Vec::new();
//! let bytes_written = Amf0Encoder::encode_value(&mut buffer, &value).unwrap();
//! println!("Encoded {} bytes", bytes_written);
//!
//! // Manual decoding
//! let mut reader = Cursor::new(&buffer);
//! let decoded = Amf0Decoder::decode_value(&mut reader).unwrap();
//! assert_eq!(value, decoded);
//! ```

pub mod protocol;

// Re-export for backwards compatibility and convenience
pub mod amf0 {
    pub use crate::protocol::amf0::*;
}

pub mod amf3 {
    pub use crate::protocol::amf3::*;
}

pub mod rtmp {
    pub use crate::protocol::rtmp::*;
}
