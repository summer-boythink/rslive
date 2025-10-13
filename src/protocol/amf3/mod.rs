pub mod decode;
pub mod encode;

use std::collections::HashMap;

/// AMF3 data type enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum Amf3Value {
    /// Undefined type (0x00)
    Undefined,
    /// Null type (0x01)
    Null,
    /// False type (0x02)
    False,
    /// True type (0x03)
    True,
    /// Integer type (0x04) - 29-bit signed integer
    Integer(i32),
    /// Double type (0x05) - IEEE 754 double precision
    Double(f64),
    /// String type (0x06) - UTF-8 string with reference support
    String(String),
    /// XML Document type (0x07) - XML as string with reference support
    XmlDoc(String),
    /// Date type (0x08) - milliseconds since epoch with reference support
    Date(f64),
    /// Array type (0x09) - dense and associative array with reference support
    Array {
        dense: Vec<Amf3Value>,
        associative: HashMap<String, Amf3Value>,
    },
    /// Object type (0x0A) - generic object with traits and reference support
    Object {
        class_name: String,
        is_dynamic: bool,
        is_externalizable: bool,
        properties: Vec<String>,            // sealed properties
        values: HashMap<String, Amf3Value>, // all property values
    },
    /// XML type (0x0B) - XML as string with reference support
    Xml(String),
    /// ByteArray type (0x0C) - binary data with reference support
    ByteArray(Vec<u8>),
    /// VectorInt type (0x0D) - vector of integers
    VectorInt { fixed: bool, items: Vec<i32> },
    /// VectorUint type (0x0E) - vector of unsigned integers
    VectorUint { fixed: bool, items: Vec<u32> },
    /// VectorDouble type (0x0F) - vector of doubles
    VectorDouble { fixed: bool, items: Vec<f64> },
    /// VectorObject type (0x10) - vector of objects
    VectorObject {
        fixed: bool,
        type_name: String,
        items: Vec<Amf3Value>,
    },
    /// Dictionary type (0x11) - key-value pairs with weak keys
    Dictionary {
        weak_keys: bool,
        pairs: Vec<(Amf3Value, Amf3Value)>,
    },
}

// AMF3 type markers
pub const AMF3_UNDEFINED_MARKER: u8 = 0x00;
pub const AMF3_NULL_MARKER: u8 = 0x01;
pub const AMF3_FALSE_MARKER: u8 = 0x02;
pub const AMF3_TRUE_MARKER: u8 = 0x03;
pub const AMF3_INTEGER_MARKER: u8 = 0x04;
pub const AMF3_DOUBLE_MARKER: u8 = 0x05;
pub const AMF3_STRING_MARKER: u8 = 0x06;
pub const AMF3_XML_DOC_MARKER: u8 = 0x07;
pub const AMF3_DATE_MARKER: u8 = 0x08;
pub const AMF3_ARRAY_MARKER: u8 = 0x09;
pub const AMF3_OBJECT_MARKER: u8 = 0x0A;
pub const AMF3_XML_MARKER: u8 = 0x0B;
pub const AMF3_BYTEARRAY_MARKER: u8 = 0x0C;
pub const AMF3_VECTOR_INT_MARKER: u8 = 0x0D;
pub const AMF3_VECTOR_UINT_MARKER: u8 = 0x0E;
pub const AMF3_VECTOR_DOUBLE_MARKER: u8 = 0x0F;
pub const AMF3_VECTOR_OBJECT_MARKER: u8 = 0x10;
pub const AMF3_DICTIONARY_MARKER: u8 = 0x11;

// AMF3 encoding constants
pub const AMF3_INTEGER_MAX: i32 = 0x0FFFFFFF;
pub const AMF3_INTEGER_MIN: i32 = -0x10000000;

// Reference table constants
pub const AMF3_REFERENCE_BIT: u32 = 0x01;

// Re-export encoder and decoder
pub use decode::Amf3Decoder;
pub use encode::Amf3Encoder;

/// Utility functions for AMF3 operations
impl Amf3Value {
    /// Create a new AMF3 array with dense values
    pub fn array(values: Vec<Amf3Value>) -> Self {
        Amf3Value::Array {
            dense: values,
            associative: HashMap::new(),
        }
    }

    /// Create a new AMF3 array with associative properties
    pub fn associative_array(pairs: Vec<(String, Amf3Value)>) -> Self {
        let mut map = HashMap::new();
        for (key, value) in pairs {
            map.insert(key, value);
        }
        Amf3Value::Array {
            dense: Vec::new(),
            associative: map,
        }
    }

    /// Create a new AMF3 object
    pub fn object(class_name: String, properties: Vec<(String, Amf3Value)>) -> Self {
        let mut values = HashMap::new();
        let mut prop_names = Vec::new();

        for (key, value) in properties {
            prop_names.push(key.clone());
            values.insert(key, value);
        }

        Amf3Value::Object {
            class_name,
            is_dynamic: true,
            is_externalizable: false,
            properties: prop_names,
            values,
        }
    }

    /// Create a new AMF3 vector of integers
    pub fn vector_int(items: Vec<i32>, fixed: bool) -> Self {
        Amf3Value::VectorInt { fixed, items }
    }

    /// Create a new AMF3 dictionary
    pub fn dictionary(pairs: Vec<(Amf3Value, Amf3Value)>, weak_keys: bool) -> Self {
        Amf3Value::Dictionary { weak_keys, pairs }
    }

    /// Check if this value is null or undefined
    pub fn is_null_or_undefined(&self) -> bool {
        matches!(self, Amf3Value::Null | Amf3Value::Undefined)
    }

    /// Check if this value is a boolean
    pub fn is_boolean(&self) -> bool {
        matches!(self, Amf3Value::True | Amf3Value::False)
    }

    /// Get boolean value if this is a boolean type
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Amf3Value::True => Some(true),
            Amf3Value::False => Some(false),
            _ => None,
        }
    }

    /// Get the type name of this AMF3 value
    pub fn type_name(&self) -> &'static str {
        match self {
            Amf3Value::Undefined => "Undefined",
            Amf3Value::Null => "Null",
            Amf3Value::False => "Boolean",
            Amf3Value::True => "Boolean",
            Amf3Value::Integer(_) => "Integer",
            Amf3Value::Double(_) => "Double",
            Amf3Value::String(_) => "String",
            Amf3Value::XmlDoc(_) => "XmlDoc",
            Amf3Value::Date(_) => "Date",
            Amf3Value::Array { .. } => "Array",
            Amf3Value::Object { .. } => "Object",
            Amf3Value::Xml(_) => "Xml",
            Amf3Value::ByteArray(_) => "ByteArray",
            Amf3Value::VectorInt { .. } => "VectorInt",
            Amf3Value::VectorUint { .. } => "VectorUint",
            Amf3Value::VectorDouble { .. } => "VectorDouble",
            Amf3Value::VectorObject { .. } => "VectorObject",
            Amf3Value::Dictionary { .. } => "Dictionary",
        }
    }
}

/// Convenience functions for encoding/decoding
pub fn encode(value: &Amf3Value) -> Result<Vec<u8>, std::io::Error> {
    let mut buffer = Vec::new();
    Amf3Encoder::encode_value(&mut buffer, value)?;
    Ok(buffer)
}

pub fn decode(data: &[u8]) -> Result<Amf3Value, std::io::Error> {
    let mut cursor = std::io::Cursor::new(data);
    Amf3Decoder::decode_value(&mut cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_amf3_value_creation() {
        // Test basic value creation
        let string_val = Amf3Value::String("test".to_string());
        assert_eq!(string_val.type_name(), "String");

        let int_val = Amf3Value::Integer(42);
        assert_eq!(int_val.type_name(), "Integer");

        let array_val = Amf3Value::array(vec![
            Amf3Value::Integer(1),
            Amf3Value::String("hello".to_string()),
        ]);
        assert_eq!(array_val.type_name(), "Array");
    }

    #[test]
    fn test_boolean_helpers() {
        assert!(Amf3Value::True.is_boolean());
        assert!(Amf3Value::False.is_boolean());
        assert!(!Amf3Value::Null.is_boolean());

        assert_eq!(Amf3Value::True.as_boolean(), Some(true));
        assert_eq!(Amf3Value::False.as_boolean(), Some(false));
        assert_eq!(Amf3Value::Null.as_boolean(), None);
    }

    #[test]
    fn test_null_undefined_check() {
        assert!(Amf3Value::Null.is_null_or_undefined());
        assert!(Amf3Value::Undefined.is_null_or_undefined());
        assert!(!Amf3Value::True.is_null_or_undefined());
    }

    #[test]
    fn test_encode_decode_basic_types() {
        // Test undefined
        let value = Amf3Value::Undefined;
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);

        // Test null
        let value = Amf3Value::Null;
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);

        // Test booleans
        let value = Amf3Value::True;
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);

        let value = Amf3Value::False;
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_integer() {
        // Test small integer
        let value = Amf3Value::Integer(42);
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);

        // Test max integer
        let value = Amf3Value::Integer(AMF3_INTEGER_MAX);
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);

        // Test min integer
        let value = Amf3Value::Integer(AMF3_INTEGER_MIN);
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_double() {
        let value = Amf3Value::Double(3.14159);
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_string() {
        let value = Amf3Value::String("Hello, AMF3!".to_string());
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);

        // Test empty string
        let value = Amf3Value::String("".to_string());
        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_array() {
        let value = Amf3Value::array(vec![
            Amf3Value::Integer(1),
            Amf3Value::String("test".to_string()),
            Amf3Value::Double(3.14),
            Amf3Value::True,
        ]);

        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_object() {
        let value = Amf3Value::object(
            "TestClass".to_string(),
            vec![
                ("name".to_string(), Amf3Value::String("John".to_string())),
                ("age".to_string(), Amf3Value::Integer(30)),
                ("active".to_string(), Amf3Value::True),
            ],
        );

        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_byte_array() {
        let data = vec![0x01, 0x02, 0x03, 0x04, 0xFF];
        let value = Amf3Value::ByteArray(data);

        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_vector_int() {
        let items = vec![1, -2, 3, -4, 5];
        let value = Amf3Value::vector_int(items, true);

        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_date() {
        let timestamp = 1640995200000.0; // 2022-01-01 00:00:00 UTC
        let value = Amf3Value::Date(timestamp);

        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_encode_decode_xml() {
        let xml = r#"<?xml version="1.0"?><root><item>test</item></root>"#;
        let value = Amf3Value::Xml(xml.to_string());

        let encoded = encode(&value).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_complex_nested_structure() {
        let inner_array = Amf3Value::array(vec![
            Amf3Value::String("nested".to_string()),
            Amf3Value::Integer(42),
        ]);

        let main_object = Amf3Value::object(
            "ComplexObject".to_string(),
            vec![
                ("id".to_string(), Amf3Value::Integer(1001)),
                ("data".to_string(), inner_array),
                ("timestamp".to_string(), Amf3Value::Date(1640995200000.0)),
                ("metadata".to_string(), Amf3Value::Null),
            ],
        );

        let encoded = encode(&main_object).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(main_object, decoded);
    }

    #[test]
    fn test_utility_functions() {
        // Test associative array creation
        let assoc_array = Amf3Value::associative_array(vec![
            ("key1".to_string(), Amf3Value::String("value1".to_string())),
            ("key2".to_string(), Amf3Value::Integer(42)),
        ]);

        if let Amf3Value::Array { dense, associative } = &assoc_array {
            assert!(dense.is_empty());
            assert_eq!(associative.len(), 2);
            assert_eq!(associative["key1"], Amf3Value::String("value1".to_string()));
            assert_eq!(associative["key2"], Amf3Value::Integer(42));
        } else {
            panic!("Expected Array");
        }

        // Test dictionary creation
        let dict = Amf3Value::dictionary(
            vec![
                (
                    Amf3Value::String("key1".to_string()),
                    Amf3Value::Integer(100),
                ),
                (
                    Amf3Value::Integer(42),
                    Amf3Value::String("answer".to_string()),
                ),
            ],
            false,
        );

        if let Amf3Value::Dictionary { weak_keys, pairs } = &dict {
            assert!(!weak_keys);
            assert_eq!(pairs.len(), 2);
        } else {
            panic!("Expected Dictionary");
        }
    }

    #[test]
    fn test_performance_benchmark() {
        // Create a complex structure for performance testing
        let mut items = Vec::new();
        for i in 0..100 {
            items.push(Amf3Value::object(
                "Item".to_string(),
                vec![
                    ("id".to_string(), Amf3Value::Integer(i)),
                    ("name".to_string(), Amf3Value::String(format!("item_{}", i))),
                    ("value".to_string(), Amf3Value::Double(i as f64 * 3.14)),
                    ("active".to_string(), Amf3Value::True),
                ],
            ));
        }

        let large_array = Amf3Value::array(items);

        // Measure encoding performance
        let start = std::time::Instant::now();
        let encoded = encode(&large_array).unwrap();
        let encode_time = start.elapsed();

        // Measure decoding performance
        let start = std::time::Instant::now();
        let decoded = decode(&encoded).unwrap();
        let decode_time = start.elapsed();

        println!("AMF3 Performance benchmark:");
        println!("  Encoded size: {} bytes", encoded.len());
        println!("  Encode time: {:?}", encode_time);
        println!("  Decode time: {:?}", decode_time);

        assert_eq!(large_array, decoded);

        // Ensure reasonable performance
        assert!(encode_time.as_millis() < 50);
        assert!(decode_time.as_millis() < 50);
    }
}
