use byteorder::{BigEndian, ReadBytesExt};
use std::{
    collections::HashMap,
    io::{self, Read},
};

use crate::protocol::amf0::{Amf0Value, *};

pub struct Amf0Decoder {
    /// Reference cache for handling AMF0 references
    ref_cache: Vec<Amf0Value>,
}

impl Amf0Decoder {
    pub fn new() -> Self {
        Self {
            ref_cache: Vec::new(),
        }
    }

    /// Decode AMF0 value from reader
    pub fn decode<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let marker = reader.read_u8()?;

        match marker {
            AMF0_NUMBER_MARKER => self.decode_number(reader),
            AMF0_BOOLEAN_MARKER => self.decode_boolean(reader),
            AMF0_STRING_MARKER => self.decode_string(reader),
            AMF0_OBJECT_MARKER => self.decode_object(reader),
            AMF0_MOVIECLIP_MARKER => Ok(Amf0Value::MovieClip), // Unsupported but defined
            AMF0_NULL_MARKER => Ok(Amf0Value::Null),
            AMF0_UNDEFINED_MARKER => Ok(Amf0Value::Undefined),
            AMF0_REFERENCE_MARKER => self.decode_reference(reader),
            AMF0_ECMA_ARRAY_MARKER => self.decode_ecma_array(reader),
            AMF0_OBJECT_END_MARKER => Ok(Amf0Value::ObjectEnd),
            AMF0_STRICT_ARRAY_MARKER => self.decode_strict_array(reader),
            AMF0_DATE_MARKER => self.decode_date(reader),
            AMF0_LONG_STRING_MARKER => self.decode_long_string(reader),
            AMF0_UNSUPPORTED_MARKER => Ok(Amf0Value::Unsupported),
            AMF0_RECORDSET_MARKER => Ok(Amf0Value::RecordSet), // Unsupported but defined
            AMF0_XML_DOCUMENT_MARKER => self.decode_xml_document(reader),
            AMF0_TYPED_OBJECT_MARKER => self.decode_typed_object(reader),
            AMF0_ACMPLUS_OBJECT_MARKER => self.decode_amf3_object(reader),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported AMF0 marker: 0x{:02x}", marker),
            )),
        }
    }

    /// Static method for one-shot decoding
    pub fn decode_value<R: Read>(reader: &mut R) -> Result<Amf0Value, io::Error> {
        let mut decoder = Self::new();
        decoder.decode(reader)
    }

    fn decode_number<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let number = reader.read_f64::<BigEndian>()?;
        Ok(Amf0Value::Number(number))
    }

    fn decode_boolean<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let byte = reader.read_u8()?;
        match byte {
            AMF0_BOOLEAN_FALSE => Ok(Amf0Value::Boolean(false)),
            AMF0_BOOLEAN_TRUE => Ok(Amf0Value::Boolean(true)),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid boolean value: 0x{:02x}", byte),
            )),
        }
    }

    fn decode_string<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let length = reader.read_u16::<BigEndian>()?;
        let mut buf = vec![0u8; length as usize];
        reader.read_exact(&mut buf)?;

        let string = String::from_utf8(buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
        })?;

        Ok(Amf0Value::String(string))
    }

    fn decode_object<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let mut object = HashMap::new();

        // Add object to reference cache before parsing (for circular references)
        self.ref_cache.push(Amf0Value::Object(HashMap::new()));
        let ref_index = self.ref_cache.len() - 1;

        loop {
            // Read key length
            let key_length = reader.read_u16::<BigEndian>()?;

            if key_length == 0 {
                // Check for object end marker
                let marker = reader.read_u8()?;
                if marker != AMF0_OBJECT_END_MARKER {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Expected object end marker, got: 0x{:02x}", marker),
                    ));
                }
                break;
            }

            // Read key
            let mut key_buf = vec![0u8; key_length as usize];
            reader.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 key: {}", e),
                )
            })?;

            // Read value
            let value = self.decode(reader)?;
            object.insert(key, value);
        }

        let result = Amf0Value::Object(object);
        // Update reference cache
        self.ref_cache[ref_index] = result.clone();

        Ok(result)
    }

    fn decode_reference<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let reference_id = reader.read_u16::<BigEndian>()?;

        if (reference_id as usize) >= self.ref_cache.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Invalid reference ID: {} (cache size: {})",
                    reference_id,
                    self.ref_cache.len()
                ),
            ));
        }

        Ok(self.ref_cache[reference_id as usize].clone())
    }

    fn decode_ecma_array<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        // Read associative count (but ignore it, treat as regular object)
        let _assoc_count = reader.read_u32::<BigEndian>()?;

        let mut array = HashMap::new();

        // Add to reference cache
        self.ref_cache.push(Amf0Value::EcmaArray(HashMap::new()));
        let ref_index = self.ref_cache.len() - 1;

        loop {
            let key_length = reader.read_u16::<BigEndian>()?;

            if key_length == 0 {
                let marker = reader.read_u8()?;
                if marker != AMF0_OBJECT_END_MARKER {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Expected object end marker in ECMA array",
                    ));
                }
                break;
            }

            let mut key_buf = vec![0u8; key_length as usize];
            reader.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 key: {}", e),
                )
            })?;

            let value = self.decode(reader)?;
            array.insert(key, value);
        }

        let result = Amf0Value::EcmaArray(array);
        self.ref_cache[ref_index] = result.clone();

        Ok(result)
    }

    fn decode_strict_array<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let length = reader.read_u32::<BigEndian>()?;
        let mut array = Vec::with_capacity(length as usize);

        // Add to reference cache
        self.ref_cache.push(Amf0Value::StrictArray(Vec::new()));
        let ref_index = self.ref_cache.len() - 1;

        for _ in 0..length {
            let value = self.decode(reader)?;
            array.push(value);
        }

        let result = Amf0Value::StrictArray(array);
        self.ref_cache[ref_index] = result.clone();

        Ok(result)
    }

    fn decode_date<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        // Read date as number (milliseconds since epoch)
        let timestamp = reader.read_f64::<BigEndian>()?;

        // Read and ignore timezone offset (2 bytes)
        let _timezone = reader.read_u16::<BigEndian>()?;

        Ok(Amf0Value::Date(timestamp))
    }

    fn decode_long_string<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        let length = reader.read_u32::<BigEndian>()?;
        let mut buf = vec![0u8; length as usize];
        reader.read_exact(&mut buf)?;

        let string = String::from_utf8(buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
        })?;

        Ok(Amf0Value::LongString(string))
    }

    fn decode_xml_document<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        // XML document is encoded as long string
        let length = reader.read_u32::<BigEndian>()?;
        let mut buf = vec![0u8; length as usize];
        reader.read_exact(&mut buf)?;

        let xml = String::from_utf8(buf).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 XML: {}", e),
            )
        })?;

        Ok(Amf0Value::XmlDocument(xml))
    }

    fn decode_typed_object<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
        // Read class name first
        let class_name_length = reader.read_u16::<BigEndian>()?;
        let mut class_name_buf = vec![0u8; class_name_length as usize];
        reader.read_exact(&mut class_name_buf)?;
        let class_name = String::from_utf8(class_name_buf).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 class name: {}", e),
            )
        })?;

        // Add to reference cache
        self.ref_cache.push(Amf0Value::TypedObject {
            class_name: String::new(),
            object: HashMap::new(),
        });
        let ref_index = self.ref_cache.len() - 1;

        // Read object properties
        let mut object = HashMap::new();
        loop {
            let key_length = reader.read_u16::<BigEndian>()?;

            if key_length == 0 {
                let marker = reader.read_u8()?;
                if marker != AMF0_OBJECT_END_MARKER {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Expected object end marker in typed object",
                    ));
                }
                break;
            }

            let mut key_buf = vec![0u8; key_length as usize];
            reader.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 key: {}", e),
                )
            })?;

            let value = self.decode(reader)?;
            object.insert(key, value);
        }

        let result = Amf0Value::TypedObject { class_name, object };
        self.ref_cache[ref_index] = result.clone();

        Ok(result)
    }

    fn decode_amf3_object<R: Read>(&mut self, _reader: &mut R) -> Result<Amf0Value, io::Error> {
        // For AMF3 objects, we need to parse the AMF3 data properly
        // For now, we'll return an error since AMF3 parsing is complex
        // In a full implementation, this would delegate to an AMF3 decoder
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "AMF3 object decoding not implemented - requires separate AMF3 parser",
        ))
    }
}

impl Default for Amf0Decoder {
    fn default() -> Self {
        Self::new()
    }
}
