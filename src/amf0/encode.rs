use byteorder::{BigEndian, WriteBytesExt};
use std::io::{self, Write};

use crate::amf0::{Amf0Value, *};

pub struct Amf0Encoder {
    /// Reference cache for handling AMF0 references
    ref_cache: Vec<*const Amf0Value>,
}

impl Amf0Encoder {
    pub fn new() -> Self {
        Self {
            ref_cache: Vec::new(),
        }
    }

    /// Encode AMF0 value to writer
    pub fn encode<W: Write>(
        &mut self,
        writer: &mut W,
        value: &Amf0Value,
    ) -> Result<usize, io::Error> {
        match value {
            Amf0Value::Number(n) => self.encode_number(writer, *n),
            Amf0Value::Boolean(b) => self.encode_boolean(writer, *b),
            Amf0Value::String(s) => self.encode_string(writer, s),
            Amf0Value::Object(obj) => self.encode_object(writer, obj),
            Amf0Value::MovieClip => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MovieClip type is not supported for encoding",
            )),
            Amf0Value::Null => self.encode_null(writer),
            Amf0Value::Undefined => self.encode_undefined(writer),
            Amf0Value::Reference(ref_id) => self.encode_reference(writer, *ref_id),
            Amf0Value::EcmaArray(arr) => self.encode_ecma_array(writer, arr),
            Amf0Value::ObjectEnd => self.encode_object_end(writer),
            Amf0Value::StrictArray(arr) => self.encode_strict_array(writer, arr),
            Amf0Value::Date(timestamp) => self.encode_date(writer, *timestamp),
            Amf0Value::LongString(s) => self.encode_long_string(writer, s),
            Amf0Value::Unsupported => self.encode_unsupported(writer),
            Amf0Value::RecordSet => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RecordSet type is not supported for encoding",
            )),
            Amf0Value::XmlDocument(xml) => self.encode_xml_document(writer, xml),
            Amf0Value::TypedObject { class_name, object } => {
                self.encode_typed_object(writer, class_name, object)
            }
            Amf0Value::Amf3Object(data) => self.encode_amf3_object(writer, data),
        }
    }

    /// Static method for one-shot encoding
    pub fn encode_value<W: Write>(writer: &mut W, value: &Amf0Value) -> Result<usize, io::Error> {
        let mut encoder = Self::new();
        encoder.encode(writer, value)
    }

    fn encode_number<W: Write>(&mut self, writer: &mut W, value: f64) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_NUMBER_MARKER)?;
        writer.write_f64::<BigEndian>(value)?;
        Ok(9) // 1 byte marker + 8 bytes f64
    }

    fn encode_boolean<W: Write>(
        &mut self,
        writer: &mut W,
        value: bool,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_BOOLEAN_MARKER)?;
        writer.write_u8(if value {
            AMF0_BOOLEAN_TRUE
        } else {
            AMF0_BOOLEAN_FALSE
        })?;
        Ok(2) // 1 byte marker + 1 byte value
    }

    fn encode_string<W: Write>(&mut self, writer: &mut W, value: &str) -> Result<usize, io::Error> {
        if value.len() > AMF0_STRING_MAX {
            return self.encode_long_string(writer, value);
        }

        writer.write_u8(AMF0_STRING_MARKER)?;
        writer.write_u16::<BigEndian>(value.len() as u16)?;
        writer.write_all(value.as_bytes())?;
        Ok(3 + value.len()) // 1 byte marker + 2 bytes length + string bytes
    }

    fn encode_object<W: Write>(
        &mut self,
        writer: &mut W,
        object: &std::collections::HashMap<String, Amf0Value>,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_OBJECT_MARKER)?;
        let mut total_bytes = 1; // marker

        for (key, value) in object {
            // Write key (without marker, just length + string)
            writer.write_u16::<BigEndian>(key.len() as u16)?;
            writer.write_all(key.as_bytes())?;
            total_bytes += 2 + key.len();

            // Write value
            let value_bytes = self.encode(writer, value)?;
            total_bytes += value_bytes;
        }

        // Write object end marker (empty string + end marker)
        writer.write_u16::<BigEndian>(0)?; // empty string length
        writer.write_u8(AMF0_OBJECT_END_MARKER)?;
        total_bytes += 3;

        Ok(total_bytes)
    }

    fn encode_null<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_NULL_MARKER)?;
        Ok(1)
    }

    fn encode_undefined<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_UNDEFINED_MARKER)?;
        Ok(1)
    }

    fn encode_reference<W: Write>(
        &mut self,
        writer: &mut W,
        ref_id: u16,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_REFERENCE_MARKER)?;
        writer.write_u16::<BigEndian>(ref_id)?;
        Ok(3) // 1 byte marker + 2 bytes reference ID
    }

    fn encode_ecma_array<W: Write>(
        &mut self,
        writer: &mut W,
        array: &std::collections::HashMap<String, Amf0Value>,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_ECMA_ARRAY_MARKER)?;
        writer.write_u32::<BigEndian>(array.len() as u32)?;
        let mut total_bytes = 5; // marker + length

        for (key, value) in array {
            // Write key
            writer.write_u16::<BigEndian>(key.len() as u16)?;
            writer.write_all(key.as_bytes())?;
            total_bytes += 2 + key.len();

            // Write value
            let value_bytes = self.encode(writer, value)?;
            total_bytes += value_bytes;
        }

        // Write object end marker
        writer.write_u16::<BigEndian>(0)?;
        writer.write_u8(AMF0_OBJECT_END_MARKER)?;
        total_bytes += 3;

        Ok(total_bytes)
    }

    fn encode_object_end<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_OBJECT_END_MARKER)?;
        Ok(1)
    }

    fn encode_strict_array<W: Write>(
        &mut self,
        writer: &mut W,
        array: &[Amf0Value],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_STRICT_ARRAY_MARKER)?;
        writer.write_u32::<BigEndian>(array.len() as u32)?;
        let mut total_bytes = 5; // marker + length

        for value in array {
            let value_bytes = self.encode(writer, value)?;
            total_bytes += value_bytes;
        }

        Ok(total_bytes)
    }

    fn encode_date<W: Write>(
        &mut self,
        writer: &mut W,
        timestamp: f64,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_DATE_MARKER)?;
        writer.write_f64::<BigEndian>(timestamp)?;
        writer.write_u16::<BigEndian>(0)?; // timezone offset (unused)
        Ok(11) // 1 byte marker + 8 bytes timestamp + 2 bytes timezone
    }

    fn encode_long_string<W: Write>(
        &mut self,
        writer: &mut W,
        value: &str,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_LONG_STRING_MARKER)?;
        writer.write_u32::<BigEndian>(value.len() as u32)?;
        writer.write_all(value.as_bytes())?;
        Ok(5 + value.len()) // 1 byte marker + 4 bytes length + string bytes
    }

    fn encode_unsupported<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_UNSUPPORTED_MARKER)?;
        Ok(1)
    }

    fn encode_xml_document<W: Write>(
        &mut self,
        writer: &mut W,
        xml: &str,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_XML_DOCUMENT_MARKER)?;
        writer.write_u32::<BigEndian>(xml.len() as u32)?;
        writer.write_all(xml.as_bytes())?;
        Ok(5 + xml.len()) // 1 byte marker + 4 bytes length + XML bytes
    }

    fn encode_typed_object<W: Write>(
        &mut self,
        writer: &mut W,
        class_name: &str,
        object: &std::collections::HashMap<String, Amf0Value>,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_TYPED_OBJECT_MARKER)?;
        let mut total_bytes = 1; // marker

        // Write class name
        writer.write_u16::<BigEndian>(class_name.len() as u16)?;
        writer.write_all(class_name.as_bytes())?;
        total_bytes += 2 + class_name.len();

        // Write object properties
        for (key, value) in object {
            writer.write_u16::<BigEndian>(key.len() as u16)?;
            writer.write_all(key.as_bytes())?;
            total_bytes += 2 + key.len();

            let value_bytes = self.encode(writer, value)?;
            total_bytes += value_bytes;
        }

        // Write object end marker
        writer.write_u16::<BigEndian>(0)?;
        writer.write_u8(AMF0_OBJECT_END_MARKER)?;
        total_bytes += 3;

        Ok(total_bytes)
    }

    fn encode_amf3_object<W: Write>(
        &mut self,
        writer: &mut W,
        data: &[u8],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF0_ACMPLUS_OBJECT_MARKER)?;
        writer.write_all(data)?;
        Ok(1 + data.len()) // marker + AMF3 data
    }
}

impl Default for Amf0Encoder {
    fn default() -> Self {
        Self::new()
    }
}
