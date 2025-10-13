pub mod decode;
pub mod encode;

use std::collections::HashMap;

// 使用 enum 定义 AMF0 的数据类型
#[derive(Debug, Clone, PartialEq)]
pub enum Amf0Value {
    Number(f64),
    Boolean(bool),
    String(String),
    Object(HashMap<String, Amf0Value>),
    MovieClip, // 0x04 - unsupported but defined
    Null,
    Undefined,
    Reference(u16),                        // 0x07 - reference to object in cache
    EcmaArray(HashMap<String, Amf0Value>), // 0x08 - associative array
    ObjectEnd,                             // 0x09 - object end marker
    StrictArray(Vec<Amf0Value>),           // 0x0A - strict array
    Date(f64),                             // 0x0B - date as milliseconds since epoch
    LongString(String),                    // 0x0C - long string (>65535 chars)
    Unsupported,                           // 0x0D - unsupported marker
    RecordSet,                             // 0x0E - unsupported but defined
    XmlDocument(String),                   // 0x0F - XML document as string
    TypedObject {
        class_name: String,
        object: HashMap<String, Amf0Value>,
    }, // 0x10 - typed object
    Amf3Object(Vec<u8>),                   // 0x11 - AMF3 object
}

// AMF0 markers/type identifiers
pub const AMF0_NUMBER_MARKER: u8 = 0x00;
pub const AMF0_BOOLEAN_MARKER: u8 = 0x01;
pub const AMF0_STRING_MARKER: u8 = 0x02;
pub const AMF0_OBJECT_MARKER: u8 = 0x03;
pub const AMF0_MOVIECLIP_MARKER: u8 = 0x04;
pub const AMF0_NULL_MARKER: u8 = 0x05;
pub const AMF0_UNDEFINED_MARKER: u8 = 0x06;
pub const AMF0_REFERENCE_MARKER: u8 = 0x07;
pub const AMF0_ECMA_ARRAY_MARKER: u8 = 0x08;
pub const AMF0_OBJECT_END_MARKER: u8 = 0x09;
pub const AMF0_STRICT_ARRAY_MARKER: u8 = 0x0A;
pub const AMF0_DATE_MARKER: u8 = 0x0B;
pub const AMF0_LONG_STRING_MARKER: u8 = 0x0C;
pub const AMF0_UNSUPPORTED_MARKER: u8 = 0x0D;
pub const AMF0_RECORDSET_MARKER: u8 = 0x0E;
pub const AMF0_XML_DOCUMENT_MARKER: u8 = 0x0F;
pub const AMF0_TYPED_OBJECT_MARKER: u8 = 0x10;
pub const AMF0_ACMPLUS_OBJECT_MARKER: u8 = 0x11;

// Boolean constants
pub const AMF0_BOOLEAN_FALSE: u8 = 0x00;
pub const AMF0_BOOLEAN_TRUE: u8 = 0x01;

// String length limits
pub const AMF0_STRING_MAX: usize = 65535;

// Re-export encoder and decoder
pub use decode::Amf0Decoder;
pub use encode::Amf0Encoder;

/// Utility functions for common AMF0 operations
impl Amf0Value {
    /// Create a new AMF0 object from key-value pairs
    pub fn object(pairs: Vec<(String, Amf0Value)>) -> Self {
        let mut map = HashMap::new();
        for (key, value) in pairs {
            map.insert(key, value);
        }
        Amf0Value::Object(map)
    }

    /// Create a new AMF0 strict array from values
    pub fn array(values: Vec<Amf0Value>) -> Self {
        Amf0Value::StrictArray(values)
    }

    /// Create a new AMF0 ECMA array from key-value pairs
    pub fn ecma_array(pairs: Vec<(String, Amf0Value)>) -> Self {
        let mut map = HashMap::new();
        for (key, value) in pairs {
            map.insert(key, value);
        }
        Amf0Value::EcmaArray(map)
    }

    /// Check if this value is null or undefined
    pub fn is_null_or_undefined(&self) -> bool {
        matches!(self, Amf0Value::Null | Amf0Value::Undefined)
    }

    /// Get the type name of this AMF0 value
    pub fn type_name(&self) -> &'static str {
        match self {
            Amf0Value::Number(_) => "Number",
            Amf0Value::Boolean(_) => "Boolean",
            Amf0Value::String(_) => "String",
            Amf0Value::Object(_) => "Object",
            Amf0Value::MovieClip => "MovieClip",
            Amf0Value::Null => "Null",
            Amf0Value::Undefined => "Undefined",
            Amf0Value::Reference(_) => "Reference",
            Amf0Value::EcmaArray(_) => "EcmaArray",
            Amf0Value::ObjectEnd => "ObjectEnd",
            Amf0Value::StrictArray(_) => "StrictArray",
            Amf0Value::Date(_) => "Date",
            Amf0Value::LongString(_) => "LongString",
            Amf0Value::Unsupported => "Unsupported",
            Amf0Value::RecordSet => "RecordSet",
            Amf0Value::XmlDocument(_) => "XmlDocument",
            Amf0Value::TypedObject { .. } => "TypedObject",
            Amf0Value::Amf3Object(_) => "Amf3Object",
        }
    }
}

/// Convenience functions for encoding/decoding
pub fn encode(value: &Amf0Value) -> Result<Vec<u8>, std::io::Error> {
    let mut buffer = Vec::new();
    Amf0Encoder::encode_value(&mut buffer, value)?;
    Ok(buffer)
}

pub fn decode(data: &[u8]) -> Result<Amf0Value, std::io::Error> {
    let mut cursor = std::io::Cursor::new(data);
    Amf0Decoder::decode_value(&mut cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_encode_decode_string() {
        let value = Amf0Value::String("hello".to_string());
        let mut buffer = Vec::new();

        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_object() {
        let mut obj = HashMap::new();
        obj.insert("key1".to_string(), Amf0Value::String("value1".to_string()));
        obj.insert("key2".to_string(), Amf0Value::Number(123.45));
        let value = Amf0Value::Object(obj);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_strict_array() {
        let arr = vec![
            Amf0Value::Number(1.0),
            Amf0Value::String("test".to_string()),
            Amf0Value::Boolean(true),
        ];
        let value = Amf0Value::StrictArray(arr);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_long_string() {
        let long_str = "a".repeat(70000); // > 65535 chars
        let value = Amf0Value::LongString(long_str);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_number() {
        let value = Amf0Value::Number(3.14159);
        let mut buffer = Vec::new();

        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_boolean() {
        // Test true
        let value = Amf0Value::Boolean(true);
        let mut buffer = Vec::new();

        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);

        // Test false
        let value = Amf0Value::Boolean(false);
        let mut buffer = Vec::new();

        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_null() {
        let value = Amf0Value::Null;
        let mut buffer = Vec::new();

        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_undefined() {
        let value = Amf0Value::Undefined;
        let mut buffer = Vec::new();

        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_ecma_array() {
        let mut arr = HashMap::new();
        arr.insert("0".to_string(), Amf0Value::String("first".to_string()));
        arr.insert("1".to_string(), Amf0Value::Number(42.0));
        arr.insert("length".to_string(), Amf0Value::Number(2.0));
        let value = Amf0Value::EcmaArray(arr);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_date() {
        let timestamp = 1234567890123.0; // Milliseconds since epoch
        let value = Amf0Value::Date(timestamp);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_xml_document() {
        let xml = r#"<?xml version="1.0"?><root><item>value</item></root>"#;
        let value = Amf0Value::XmlDocument(xml.to_string());

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_typed_object() {
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), Amf0Value::String("John".to_string()));
        obj.insert("age".to_string(), Amf0Value::Number(30.0));

        let value = Amf0Value::TypedObject {
            class_name: "Person".to_string(),
            object: obj,
        };

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_unsupported() {
        let value = Amf0Value::Unsupported;

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_amf3_object() {
        let amf3_data = vec![0x01, 0x02, 0x03, 0x04]; // Dummy AMF3 data
        let value = Amf0Value::Amf3Object(amf3_data.clone());

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        // Verify the marker is correct
        assert_eq!(buffer[0], AMF0_ACMPLUS_OBJECT_MARKER);
        // Verify the AMF3 data follows
        assert_eq!(&buffer[1..], &amf3_data);
    }

    #[test]
    fn test_string_vs_long_string() {
        // Regular string
        let short_str = "a".repeat(100);
        let value1 = Amf0Value::String(short_str.clone());

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value1).unwrap();

        // Should encode as regular string (marker 0x02)
        assert_eq!(buffer[0], AMF0_STRING_MARKER);

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();
        assert_eq!(value1, decoded_value);

        // Long string - force it by creating LongString directly
        let value2 = Amf0Value::LongString(short_str);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value2).unwrap();

        // Should encode as long string (marker 0x0C)
        assert_eq!(buffer[0], AMF0_LONG_STRING_MARKER);

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();
        assert_eq!(value2, decoded_value);
    }

    #[test]
    fn test_complex_nested_object() {
        let mut inner_obj = HashMap::new();
        inner_obj.insert("inner_num".to_string(), Amf0Value::Number(123.45));
        inner_obj.insert("inner_bool".to_string(), Amf0Value::Boolean(true));

        let mut main_obj = HashMap::new();
        main_obj.insert(
            "string_prop".to_string(),
            Amf0Value::String("test".to_string()),
        );
        main_obj.insert(
            "array_prop".to_string(),
            Amf0Value::StrictArray(vec![
                Amf0Value::Number(1.0),
                Amf0Value::String("item".to_string()),
            ]),
        );
        main_obj.insert("object_prop".to_string(), Amf0Value::Object(inner_obj));
        main_obj.insert("null_prop".to_string(), Amf0Value::Null);

        let value = Amf0Value::Object(main_obj);

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();

        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_encode_decode_reference() {
        // References work within a decoder context where objects are cached
        let mut decoder = Amf0Decoder::new();

        // First, add an object to the reference cache by decoding it
        let obj_value = Amf0Value::Object({
            let mut map = HashMap::new();
            map.insert("test".to_string(), Amf0Value::Number(123.0));
            map
        });

        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &obj_value).unwrap();

        let mut reader = Cursor::new(&buffer);
        let decoded_obj = decoder.decode(&mut reader).unwrap();
        assert_eq!(obj_value, decoded_obj);

        // Now test reference encoding/decoding (just verify the format)
        let ref_value = Amf0Value::Reference(0); // Reference to first cached object
        let mut ref_buffer = Vec::new();
        Amf0Encoder::encode_value(&mut ref_buffer, &ref_value).unwrap();

        // Verify the marker and reference ID are correct
        assert_eq!(ref_buffer[0], AMF0_REFERENCE_MARKER);
        assert_eq!(ref_buffer.len(), 3); // marker + 2 bytes for u16

        // Test decoding the reference (should return the cached object)
        let mut ref_reader = Cursor::new(&ref_buffer);
        let decoded_ref = decoder.decode(&mut ref_reader).unwrap();
        assert_eq!(decoded_ref, obj_value); // Should match the originally cached object
    }

    #[test]
    fn test_edge_cases() {
        // Test empty string
        let value = Amf0Value::String("".to_string());
        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();
        assert_eq!(value, decoded_value);

        // Test empty object
        let value = Amf0Value::Object(HashMap::new());
        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();
        assert_eq!(value, decoded_value);

        // Test empty array
        let value = Amf0Value::StrictArray(vec![]);
        let mut buffer = Vec::new();
        Amf0Encoder::encode_value(&mut buffer, &value).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded_value = Amf0Decoder::decode_value(&mut reader).unwrap();
        assert_eq!(value, decoded_value);
    }

    #[test]
    fn test_utility_functions() {
        // Test object creation utility
        let obj = Amf0Value::object(vec![
            ("name".to_string(), Amf0Value::String("test".to_string())),
            ("value".to_string(), Amf0Value::Number(42.0)),
        ]);

        if let Amf0Value::Object(map) = &obj {
            assert_eq!(map.len(), 2);
            assert_eq!(map["name"], Amf0Value::String("test".to_string()));
            assert_eq!(map["value"], Amf0Value::Number(42.0));
        } else {
            panic!("Expected Object");
        }

        // Test array creation utility
        let arr = Amf0Value::array(vec![
            Amf0Value::String("first".to_string()),
            Amf0Value::Number(1.0),
        ]);

        if let Amf0Value::StrictArray(vec) = &arr {
            assert_eq!(vec.len(), 2);
            assert_eq!(vec[0], Amf0Value::String("first".to_string()));
            assert_eq!(vec[1], Amf0Value::Number(1.0));
        } else {
            panic!("Expected StrictArray");
        }

        // Test type names
        assert_eq!(Amf0Value::Number(1.0).type_name(), "Number");
        assert_eq!(Amf0Value::String("test".to_string()).type_name(), "String");
        assert_eq!(Amf0Value::Null.type_name(), "Null");

        // Test null/undefined check
        assert!(Amf0Value::Null.is_null_or_undefined());
        assert!(Amf0Value::Undefined.is_null_or_undefined());
        assert!(!Amf0Value::Number(0.0).is_null_or_undefined());
    }

    #[test]
    fn test_convenience_encode_decode() {
        let value = Amf0Value::object(vec![
            (
                "message".to_string(),
                Amf0Value::String("hello".to_string()),
            ),
            ("count".to_string(), Amf0Value::Number(123.0)),
        ]);

        // Test convenience encode
        let encoded = encode(&value).unwrap();

        // Test convenience decode
        let decoded = decode(&encoded).unwrap();

        assert_eq!(value, decoded);
    }

    #[test]
    fn test_performance_benchmark() {
        // Create a complex nested structure for performance testing
        let mut large_object = HashMap::new();

        // Add 100 properties with mixed types
        for i in 0..100 {
            large_object.insert(
                format!("key_{}", i),
                match i % 5 {
                    0 => Amf0Value::Number(i as f64),
                    1 => Amf0Value::String(format!("value_{}", i)),
                    2 => Amf0Value::Boolean(i % 2 == 0),
                    3 => Amf0Value::array(vec![
                        Amf0Value::Number(i as f64),
                        Amf0Value::String("nested".to_string()),
                    ]),
                    _ => Amf0Value::object(vec![(
                        "nested_key".to_string(),
                        Amf0Value::Number(i as f64),
                    )]),
                },
            );
        }

        let value = Amf0Value::Object(large_object);

        // Measure encoding performance
        let start = std::time::Instant::now();
        let encoded = encode(&value).unwrap();
        let encode_time = start.elapsed();

        // Measure decoding performance
        let start = std::time::Instant::now();
        let decoded = decode(&encoded).unwrap();
        let decode_time = start.elapsed();

        println!("Performance benchmark:");
        println!("  Encoded size: {} bytes", encoded.len());
        println!("  Encode time: {:?}", encode_time);
        println!("  Decode time: {:?}", decode_time);

        assert_eq!(value, decoded);

        // Ensure reasonable performance (should be much faster than 10ms for this size)
        assert!(encode_time.as_millis() < 10);
        assert!(decode_time.as_millis() < 10);
    }
}
