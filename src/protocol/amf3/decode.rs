use byteorder::{BigEndian, ReadBytesExt};
use std::{
    collections::HashMap,
    io::{self, Read},
};

use super::{Amf3Value, *};

pub struct Amf3Decoder {
    /// String reference table
    string_table: Vec<String>,
    /// Object reference table
    object_table: Vec<Amf3Value>,
    /// Class definition reference table
    trait_table: Vec<ClassDefinition>,
}

#[derive(Debug, Clone)]
struct ClassDefinition {
    class_name: String,
    is_dynamic: bool,
    is_externalizable: bool,
    properties: Vec<String>,
}

impl Amf3Decoder {
    pub fn new() -> Self {
        Self {
            string_table: Vec::new(),
            object_table: Vec::new(),
            trait_table: Vec::new(),
        }
    }

    /// Decode AMF3 value from reader
    pub fn decode<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let marker = reader.read_u8()?;

        match marker {
            AMF3_UNDEFINED_MARKER => Ok(Amf3Value::Undefined),
            AMF3_NULL_MARKER => Ok(Amf3Value::Null),
            AMF3_FALSE_MARKER => Ok(Amf3Value::False),
            AMF3_TRUE_MARKER => Ok(Amf3Value::True),
            AMF3_INTEGER_MARKER => self.decode_integer(reader),
            AMF3_DOUBLE_MARKER => self.decode_double(reader),
            AMF3_STRING_MARKER => self.decode_string(reader),
            AMF3_XML_DOC_MARKER => self.decode_xml_doc(reader),
            AMF3_DATE_MARKER => self.decode_date(reader),
            AMF3_ARRAY_MARKER => self.decode_array(reader),
            AMF3_OBJECT_MARKER => self.decode_object(reader),
            AMF3_XML_MARKER => self.decode_xml(reader),
            AMF3_BYTEARRAY_MARKER => self.decode_byte_array(reader),
            AMF3_VECTOR_INT_MARKER => self.decode_vector_int(reader),
            AMF3_VECTOR_UINT_MARKER => self.decode_vector_uint(reader),
            AMF3_VECTOR_DOUBLE_MARKER => self.decode_vector_double(reader),
            AMF3_VECTOR_OBJECT_MARKER => self.decode_vector_object(reader),
            AMF3_DICTIONARY_MARKER => self.decode_dictionary(reader),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported AMF3 marker: 0x{:02x}", marker),
            )),
        }
    }

    /// Static method for one-shot decoding
    pub fn decode_value<R: Read>(reader: &mut R) -> Result<Amf3Value, io::Error> {
        let mut decoder = Self::new();
        decoder.decode(reader)
    }

    fn decode_integer<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let value = self.read_u29_int(reader)?;

        // Convert unsigned 29-bit to signed 29-bit
        let signed_value = if value & 0x10000000 != 0 {
            // For values with bit 28 set, extend sign bit to create negative number
            (value | 0xE0000000) as i32
        } else {
            value as i32
        };

        Ok(Amf3Value::Integer(signed_value))
    }

    fn decode_double<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let value = reader.read_f64::<BigEndian>()?;
        Ok(Amf3Value::Double(value))
    }

    fn decode_string<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let string = self.read_string_with_table(reader)?;
        Ok(Amf3Value::String(string))
    }

    fn decode_xml_doc<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid XML doc reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New XML document
        let length = (info >> 1) as usize;
        let mut buf = vec![0u8; length];
        reader.read_exact(&mut buf)?;

        let xml_string = String::from_utf8(buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
        })?;

        let xml_doc = Amf3Value::XmlDoc(xml_string);
        self.object_table.push(xml_doc.clone());
        Ok(xml_doc)
    }

    fn decode_date<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid date reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New date
        let timestamp = reader.read_f64::<BigEndian>()?;
        let date = Amf3Value::Date(timestamp);
        self.object_table.push(date.clone());
        Ok(date)
    }

    fn decode_array<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing array
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid array reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New array
        let dense_length = (info >> 1) as usize;
        let mut associative = HashMap::new();
        let mut dense = Vec::new();

        // Add placeholder to object table for circular references
        let placeholder = Amf3Value::Array {
            dense: Vec::new(),
            associative: HashMap::new(),
        };
        self.object_table.push(placeholder);
        let array_index = self.object_table.len() - 1;

        // Read associative portion (key-value pairs until empty string)
        loop {
            let key = self.read_string_with_table(reader)?;
            if key.is_empty() {
                break;
            }
            let value = self.decode(reader)?;
            associative.insert(key, value);
        }

        // Read dense portion
        for _ in 0..dense_length {
            let value = self.decode(reader)?;
            dense.push(value);
        }

        let array = Amf3Value::Array { dense, associative };
        self.object_table[array_index] = array.clone();
        Ok(array)
    }

    fn decode_object<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid object reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        let trait_info = info >> 1;

        let (class_def, is_trait_reference) = if trait_info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing trait
            let trait_reference = (trait_info >> 1) as usize;
            if trait_reference >= self.trait_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid trait reference: {}", trait_reference),
                ));
            }
            (self.trait_table[trait_reference].clone(), true)
        } else {
            // New trait definition
            let trait_flags = trait_info >> 1;
            let is_externalizable = trait_flags & 0x01 != 0;
            let is_dynamic = trait_flags & 0x02 != 0;
            let property_count = (trait_flags >> 2) as usize;

            let class_name = self.read_string_with_table(reader)?;
            let mut properties = Vec::new();

            for _ in 0..property_count {
                let property_name = self.read_string_with_table(reader)?;
                properties.push(property_name);
            }

            let class_def = ClassDefinition {
                class_name,
                is_dynamic,
                is_externalizable,
                properties,
            };

            self.trait_table.push(class_def.clone());
            (class_def, false)
        };

        // Add placeholder for circular references
        let placeholder = Amf3Value::Object {
            class_name: class_def.class_name.clone(),
            is_dynamic: class_def.is_dynamic,
            is_externalizable: class_def.is_externalizable,
            properties: class_def.properties.clone(),
            values: HashMap::new(),
        };
        self.object_table.push(placeholder);
        let object_index = self.object_table.len() - 1;

        let mut values = HashMap::new();

        if class_def.is_externalizable {
            // For externalizable objects, the object handles its own serialization
            // We'll read it as a byte array for now
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Externalizable objects not fully supported",
            ));
        }

        // Read sealed properties
        for property_name in &class_def.properties {
            let value = self.decode(reader)?;
            values.insert(property_name.clone(), value);
        }

        // Read dynamic properties if object is dynamic
        if class_def.is_dynamic {
            loop {
                let key = self.read_string_with_table(reader)?;
                if key.is_empty() {
                    break;
                }
                let value = self.decode(reader)?;
                values.insert(key, value);
            }
        }

        let object = Amf3Value::Object {
            class_name: class_def.class_name,
            is_dynamic: class_def.is_dynamic,
            is_externalizable: class_def.is_externalizable,
            properties: class_def.properties,
            values,
        };

        self.object_table[object_index] = object.clone();
        Ok(object)
    }

    fn decode_xml<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid XML reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New XML
        let length = (info >> 1) as usize;
        let mut buf = vec![0u8; length];
        reader.read_exact(&mut buf)?;

        let xml_string = String::from_utf8(buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
        })?;

        let xml = Amf3Value::Xml(xml_string);
        self.object_table.push(xml.clone());
        Ok(xml)
    }

    fn decode_byte_array<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid ByteArray reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New ByteArray
        let length = (info >> 1) as usize;
        let mut buf = vec![0u8; length];
        reader.read_exact(&mut buf)?;

        let byte_array = Amf3Value::ByteArray(buf);
        self.object_table.push(byte_array.clone());
        Ok(byte_array)
    }

    fn decode_vector_int<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid VectorInt reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New VectorInt
        let length = (info >> 1) as usize;
        let fixed = reader.read_u8()? != 0;
        let mut items = Vec::with_capacity(length);

        for _ in 0..length {
            let item = reader.read_i32::<BigEndian>()?;
            items.push(item);
        }

        let vector = Amf3Value::VectorInt { fixed, items };
        self.object_table.push(vector.clone());
        Ok(vector)
    }

    fn decode_vector_uint<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid VectorUint reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New VectorUint
        let length = (info >> 1) as usize;
        let fixed = reader.read_u8()? != 0;
        let mut items = Vec::with_capacity(length);

        for _ in 0..length {
            let item = reader.read_u32::<BigEndian>()?;
            items.push(item);
        }

        let vector = Amf3Value::VectorUint { fixed, items };
        self.object_table.push(vector.clone());
        Ok(vector)
    }

    fn decode_vector_double<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid VectorDouble reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New VectorDouble
        let length = (info >> 1) as usize;
        let fixed = reader.read_u8()? != 0;
        let mut items = Vec::with_capacity(length);

        for _ in 0..length {
            let item = reader.read_f64::<BigEndian>()?;
            items.push(item);
        }

        let vector = Amf3Value::VectorDouble { fixed, items };
        self.object_table.push(vector.clone());
        Ok(vector)
    }

    fn decode_vector_object<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid VectorObject reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New VectorObject
        let length = (info >> 1) as usize;
        let fixed = reader.read_u8()? != 0;
        let type_name = self.read_string_with_table(reader)?;

        let mut items = Vec::with_capacity(length);
        for _ in 0..length {
            let item = self.decode(reader)?;
            items.push(item);
        }

        let vector = Amf3Value::VectorObject {
            fixed,
            type_name,
            items,
        };
        self.object_table.push(vector.clone());
        Ok(vector)
    }

    fn decode_dictionary<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing object
            let reference = (info >> 1) as usize;
            if reference >= self.object_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid Dictionary reference: {}", reference),
                ));
            }
            return Ok(self.object_table[reference].clone());
        }

        // New Dictionary
        let length = (info >> 1) as usize;
        let weak_keys = reader.read_u8()? != 0;

        let mut pairs = Vec::with_capacity(length);
        for _ in 0..length {
            let key = self.decode(reader)?;
            let value = self.decode(reader)?;
            pairs.push((key, value));
        }

        let dictionary = Amf3Value::Dictionary { weak_keys, pairs };
        self.object_table.push(dictionary.clone());
        Ok(dictionary)
    }

    /// Read a U29 variable-length integer
    fn read_u29_int<R: Read>(&mut self, reader: &mut R) -> Result<u32, io::Error> {
        let mut result = 0u32;

        // Read first byte
        let byte1 = reader.read_u8()?;
        if (byte1 & 0x80) == 0 {
            // 1 byte: 0xxxxxxx
            return Ok(byte1 as u32);
        }

        // Read second byte
        let byte2 = reader.read_u8()?;
        result = ((byte1 & 0x7F) as u32) << 7 | (byte2 & 0x7F) as u32;
        if (byte2 & 0x80) == 0 {
            // 2 bytes: 1xxxxxxx 0xxxxxxx
            return Ok(result);
        }

        // Read third byte
        let byte3 = reader.read_u8()?;
        result = result << 7 | (byte3 & 0x7F) as u32;
        if (byte3 & 0x80) == 0 {
            // 3 bytes: 1xxxxxxx 1xxxxxxx 0xxxxxxx
            return Ok(result);
        }

        // Read fourth byte (uses all 8 bits)
        let byte4 = reader.read_u8()?;
        result = result << 8 | byte4 as u32;

        Ok(result)
    }

    /// Read a string with reference table support
    fn read_string_with_table<R: Read>(&mut self, reader: &mut R) -> Result<String, io::Error> {
        let info = self.read_u29_int(reader)?;

        if info & AMF3_REFERENCE_BIT == 0 {
            // Reference to existing string
            let reference = (info >> 1) as usize;
            if reference >= self.string_table.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid string reference: {}", reference),
                ));
            }
            return Ok(self.string_table[reference].clone());
        }

        // New string
        let length = (info >> 1) as usize;
        if length == 0 {
            return Ok(String::new());
        }

        let mut buf = vec![0u8; length];
        reader.read_exact(&mut buf)?;

        let string = String::from_utf8(buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
        })?;

        // Add to string table (only non-empty strings)
        self.string_table.push(string.clone());
        Ok(string)
    }
}

impl Default for Amf3Decoder {
    fn default() -> Self {
        Self::new()
    }
}
