use byteorder::{BigEndian, WriteBytesExt};
use std::{
    collections::HashMap,
    io::{self, Write},
};

use super::{Amf3Value, *};

pub struct Amf3Encoder {
    /// String reference table for deduplication
    string_table: HashMap<String, usize>,
    /// Object reference table for circular references
    object_table: Vec<*const Amf3Value>,
    /// Class definition reference table
    trait_table: Vec<ClassDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
struct ClassDefinition {
    class_name: String,
    is_dynamic: bool,
    is_externalizable: bool,
    properties: Vec<String>,
}

impl Amf3Encoder {
    pub fn new() -> Self {
        Self {
            string_table: HashMap::new(),
            object_table: Vec::new(),
            trait_table: Vec::new(),
        }
    }

    /// Encode AMF3 value to writer
    pub fn encode<W: Write>(
        &mut self,
        writer: &mut W,
        value: &Amf3Value,
    ) -> Result<usize, io::Error> {
        match value {
            Amf3Value::Undefined => self.encode_undefined(writer),
            Amf3Value::Null => self.encode_null(writer),
            Amf3Value::False => self.encode_false(writer),
            Amf3Value::True => self.encode_true(writer),
            Amf3Value::Integer(i) => self.encode_integer(writer, *i),
            Amf3Value::Double(d) => self.encode_double(writer, *d),
            Amf3Value::String(s) => self.encode_string(writer, s),
            Amf3Value::XmlDoc(xml) => self.encode_xml_doc(writer, xml),
            Amf3Value::Date(timestamp) => self.encode_date(writer, *timestamp),
            Amf3Value::Array { dense, associative } => {
                self.encode_array(writer, dense, associative)
            }
            Amf3Value::Object {
                class_name,
                is_dynamic,
                is_externalizable,
                properties,
                values,
            } => self.encode_object(
                writer,
                class_name,
                *is_dynamic,
                *is_externalizable,
                properties,
                values,
            ),
            Amf3Value::Xml(xml) => self.encode_xml(writer, xml),
            Amf3Value::ByteArray(bytes) => self.encode_byte_array(writer, bytes),
            Amf3Value::VectorInt { fixed, items } => self.encode_vector_int(writer, *fixed, items),
            Amf3Value::VectorUint { fixed, items } => {
                self.encode_vector_uint(writer, *fixed, items)
            }
            Amf3Value::VectorDouble { fixed, items } => {
                self.encode_vector_double(writer, *fixed, items)
            }
            Amf3Value::VectorObject {
                fixed,
                type_name,
                items,
            } => self.encode_vector_object(writer, *fixed, type_name, items),
            Amf3Value::Dictionary { weak_keys, pairs } => {
                self.encode_dictionary(writer, *weak_keys, pairs)
            }
        }
    }

    /// Static method for one-shot encoding
    pub fn encode_value<W: Write>(writer: &mut W, value: &Amf3Value) -> Result<usize, io::Error> {
        let mut encoder = Self::new();
        encoder.encode(writer, value)
    }

    fn encode_undefined<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_UNDEFINED_MARKER)?;
        Ok(1)
    }

    fn encode_null<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_NULL_MARKER)?;
        Ok(1)
    }

    fn encode_false<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_FALSE_MARKER)?;
        Ok(1)
    }

    fn encode_true<W: Write>(&mut self, writer: &mut W) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_TRUE_MARKER)?;
        Ok(1)
    }

    fn encode_integer<W: Write>(&mut self, writer: &mut W, value: i32) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_INTEGER_MARKER)?;
        // Handle sign extension for 29-bit integers
        let u29_value = if value < 0 {
            // For negative values, mask to 29 bits
            (value as u32) & 0x1FFFFFFF
        } else {
            value as u32
        };
        let bytes_written = self.write_u29_int(writer, u29_value)?;
        Ok(1 + bytes_written)
    }

    fn encode_double<W: Write>(&mut self, writer: &mut W, value: f64) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_DOUBLE_MARKER)?;
        writer.write_f64::<BigEndian>(value)?;
        Ok(9) // 1 byte marker + 8 bytes f64
    }

    fn encode_string<W: Write>(&mut self, writer: &mut W, value: &str) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_STRING_MARKER)?;
        let bytes_written = self.write_string_with_table(writer, value)?;
        Ok(1 + bytes_written)
    }

    fn encode_xml_doc<W: Write>(&mut self, writer: &mut W, xml: &str) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_XML_DOC_MARKER)?;

        // Check if this XML doc is already in object table
        let xml_value = Amf3Value::XmlDoc(xml.to_string());
        if let Some(reference) = self.find_object_reference(&xml_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New XML doc
        self.object_table.push(&xml_value as *const _);
        let length_info = (xml.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_all(xml.as_bytes())?;
        bytes_written += xml.len();

        Ok(1 + bytes_written)
    }

    fn encode_date<W: Write>(
        &mut self,
        writer: &mut W,
        timestamp: f64,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_DATE_MARKER)?;

        // Check if this date is already in object table
        let date_value = Amf3Value::Date(timestamp);
        if let Some(reference) = self.find_object_reference(&date_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New date
        self.object_table.push(&date_value as *const _);
        let mut bytes_written = self.write_u29_int(writer, 1)?; // New object marker
        writer.write_f64::<BigEndian>(timestamp)?;
        bytes_written += 8;

        Ok(1 + bytes_written)
    }

    fn encode_array<W: Write>(
        &mut self,
        writer: &mut W,
        dense: &[Amf3Value],
        associative: &HashMap<String, Amf3Value>,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_ARRAY_MARKER)?;

        // Check if this array is already in object table
        let array_value = Amf3Value::Array {
            dense: dense.to_vec(),
            associative: associative.clone(),
        };
        if let Some(reference) = self.find_object_reference(&array_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New array
        self.object_table.push(&array_value as *const _);
        let dense_length_info = (dense.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, dense_length_info as u32)?;

        // Write associative portion
        for (key, value) in associative {
            bytes_written += self.write_string_with_table(writer, key)?;
            bytes_written += self.encode(writer, value)?;
        }

        // Write empty string to end associative portion
        bytes_written += self.write_string_with_table(writer, "")?;

        // Write dense portion
        for value in dense {
            bytes_written += self.encode(writer, value)?;
        }

        Ok(1 + bytes_written)
    }

    fn encode_object<W: Write>(
        &mut self,
        writer: &mut W,
        class_name: &str,
        is_dynamic: bool,
        is_externalizable: bool,
        properties: &[String],
        values: &HashMap<String, Amf3Value>,
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_OBJECT_MARKER)?;

        // Check if this object is already in object table
        let object_value = Amf3Value::Object {
            class_name: class_name.to_string(),
            is_dynamic,
            is_externalizable,
            properties: properties.to_vec(),
            values: values.clone(),
        };
        if let Some(reference) = self.find_object_reference(&object_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // Add to object table
        self.object_table.push(&object_value as *const _);

        // Check if trait definition is already in trait table
        let class_def = ClassDefinition {
            class_name: class_name.to_string(),
            is_dynamic,
            is_externalizable,
            properties: properties.to_vec(),
        };

        let mut bytes_written = 0;

        if let Some(trait_reference) = self.find_trait_reference(&class_def) {
            // Use existing trait reference
            let trait_info = (trait_reference << 1) as u32;
            let object_info = (trait_info << 1) | 1;
            bytes_written += self.write_u29_int(writer, object_info)?;
        } else {
            // New trait definition
            self.trait_table.push(class_def);

            let mut trait_flags = properties.len() << 2;
            if is_dynamic {
                trait_flags |= 0x02;
            }
            if is_externalizable {
                trait_flags |= 0x01;
            }

            let trait_info = (trait_flags << 1) | 1;
            let object_info = (trait_info << 1) | 1;
            bytes_written += self.write_u29_int(writer, object_info as u32)?;

            // Write class name
            bytes_written += self.write_string_with_table(writer, class_name)?;

            // Write property names
            for property_name in properties {
                bytes_written += self.write_string_with_table(writer, property_name)?;
            }
        }

        if is_externalizable {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Externalizable objects not fully supported",
            ));
        }

        // Write sealed properties
        for property_name in properties {
            if let Some(value) = values.get(property_name) {
                bytes_written += self.encode(writer, value)?;
            } else {
                bytes_written += self.encode(writer, &Amf3Value::Undefined)?;
            }
        }

        // Write dynamic properties if object is dynamic
        if is_dynamic {
            for (key, value) in values {
                if !properties.contains(key) {
                    bytes_written += self.write_string_with_table(writer, key)?;
                    bytes_written += self.encode(writer, value)?;
                }
            }
            // Write empty string to end dynamic properties
            bytes_written += self.write_string_with_table(writer, "")?;
        }

        Ok(1 + bytes_written)
    }

    fn encode_xml<W: Write>(&mut self, writer: &mut W, xml: &str) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_XML_MARKER)?;

        // Check if this XML is already in object table
        let xml_value = Amf3Value::Xml(xml.to_string());
        if let Some(reference) = self.find_object_reference(&xml_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New XML
        self.object_table.push(&xml_value as *const _);
        let length_info = (xml.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_all(xml.as_bytes())?;
        bytes_written += xml.len();

        Ok(1 + bytes_written)
    }

    fn encode_byte_array<W: Write>(
        &mut self,
        writer: &mut W,
        bytes: &[u8],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_BYTEARRAY_MARKER)?;

        // Check if this ByteArray is already in object table
        let byte_array_value = Amf3Value::ByteArray(bytes.to_vec());
        if let Some(reference) = self.find_object_reference(&byte_array_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New ByteArray
        self.object_table.push(&byte_array_value as *const _);
        let length_info = (bytes.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_all(bytes)?;
        bytes_written += bytes.len();

        Ok(1 + bytes_written)
    }

    fn encode_vector_int<W: Write>(
        &mut self,
        writer: &mut W,
        fixed: bool,
        items: &[i32],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_VECTOR_INT_MARKER)?;

        let vector_value = Amf3Value::VectorInt {
            fixed,
            items: items.to_vec(),
        };
        if let Some(reference) = self.find_object_reference(&vector_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New VectorInt
        self.object_table.push(&vector_value as *const _);
        let length_info = (items.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_u8(if fixed { 1 } else { 0 })?;
        bytes_written += 1;

        for item in items {
            writer.write_i32::<BigEndian>(*item)?;
            bytes_written += 4;
        }

        Ok(1 + bytes_written)
    }

    fn encode_vector_uint<W: Write>(
        &mut self,
        writer: &mut W,
        fixed: bool,
        items: &[u32],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_VECTOR_UINT_MARKER)?;

        let vector_value = Amf3Value::VectorUint {
            fixed,
            items: items.to_vec(),
        };
        if let Some(reference) = self.find_object_reference(&vector_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New VectorUint
        self.object_table.push(&vector_value as *const _);
        let length_info = (items.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_u8(if fixed { 1 } else { 0 })?;
        bytes_written += 1;

        for item in items {
            writer.write_u32::<BigEndian>(*item)?;
            bytes_written += 4;
        }

        Ok(1 + bytes_written)
    }

    fn encode_vector_double<W: Write>(
        &mut self,
        writer: &mut W,
        fixed: bool,
        items: &[f64],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_VECTOR_DOUBLE_MARKER)?;

        let vector_value = Amf3Value::VectorDouble {
            fixed,
            items: items.to_vec(),
        };
        if let Some(reference) = self.find_object_reference(&vector_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New VectorDouble
        self.object_table.push(&vector_value as *const _);
        let length_info = (items.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_u8(if fixed { 1 } else { 0 })?;
        bytes_written += 1;

        for item in items {
            writer.write_f64::<BigEndian>(*item)?;
            bytes_written += 8;
        }

        Ok(1 + bytes_written)
    }

    fn encode_vector_object<W: Write>(
        &mut self,
        writer: &mut W,
        fixed: bool,
        type_name: &str,
        items: &[Amf3Value],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_VECTOR_OBJECT_MARKER)?;

        let vector_value = Amf3Value::VectorObject {
            fixed,
            type_name: type_name.to_string(),
            items: items.to_vec(),
        };
        if let Some(reference) = self.find_object_reference(&vector_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New VectorObject
        self.object_table.push(&vector_value as *const _);
        let length_info = (items.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_u8(if fixed { 1 } else { 0 })?;
        bytes_written += 1;

        bytes_written += self.write_string_with_table(writer, type_name)?;

        for item in items {
            bytes_written += self.encode(writer, item)?;
        }

        Ok(1 + bytes_written)
    }

    fn encode_dictionary<W: Write>(
        &mut self,
        writer: &mut W,
        weak_keys: bool,
        pairs: &[(Amf3Value, Amf3Value)],
    ) -> Result<usize, io::Error> {
        writer.write_u8(AMF3_DICTIONARY_MARKER)?;

        let dict_value = Amf3Value::Dictionary {
            weak_keys,
            pairs: pairs.to_vec(),
        };
        if let Some(reference) = self.find_object_reference(&dict_value) {
            let bytes_written = self.write_u29_int(writer, (reference << 1) as u32)?;
            return Ok(1 + bytes_written);
        }

        // New Dictionary
        self.object_table.push(&dict_value as *const _);
        let length_info = (pairs.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_u8(if weak_keys { 1 } else { 0 })?;
        bytes_written += 1;

        for (key, value) in pairs {
            bytes_written += self.encode(writer, key)?;
            bytes_written += self.encode(writer, value)?;
        }

        Ok(1 + bytes_written)
    }

    /// Write a U29 variable-length integer
    fn write_u29_int<W: Write>(&mut self, writer: &mut W, value: u32) -> Result<usize, io::Error> {
        let mut bytes_written = 0;

        if value < 0x80 {
            // 1 byte: 0xxxxxxx
            writer.write_u8(value as u8)?;
            bytes_written += 1;
        } else if value < 0x4000 {
            // 2 bytes: 1xxxxxxx 0xxxxxxx
            writer.write_u8(((value >> 7) | 0x80) as u8)?;
            writer.write_u8((value & 0x7F) as u8)?;
            bytes_written += 2;
        } else if value < 0x200000 {
            // 3 bytes: 1xxxxxxx 1xxxxxxx 0xxxxxxx
            writer.write_u8(((value >> 14) | 0x80) as u8)?;
            writer.write_u8((((value >> 7) & 0x7F) | 0x80) as u8)?;
            writer.write_u8((value & 0x7F) as u8)?;
            bytes_written += 3;
        } else {
            // 4 bytes: 1xxxxxxx 1xxxxxxx 1xxxxxxx xxxxxxxx
            // Mask to 29 bits for the encoding
            let masked_value = value & 0x1FFFFFFF;
            writer.write_u8(((masked_value >> 22) | 0x80) as u8)?;
            writer.write_u8((((masked_value >> 15) & 0x7F) | 0x80) as u8)?;
            writer.write_u8((((masked_value >> 8) & 0x7F) | 0x80) as u8)?;
            writer.write_u8((masked_value & 0xFF) as u8)?;
            bytes_written += 4;
        }

        Ok(bytes_written)
    }

    /// Write a string with reference table support
    fn write_string_with_table<W: Write>(
        &mut self,
        writer: &mut W,
        value: &str,
    ) -> Result<usize, io::Error> {
        // Empty strings are not added to the reference table
        if value.is_empty() {
            return self.write_u29_int(writer, 1); // Length 0 with new string flag
        }

        // Check if string is already in table
        if let Some(&reference) = self.string_table.get(value) {
            return self.write_u29_int(writer, (reference << 1) as u32);
        }

        // Add to string table
        let reference = self.string_table.len();
        self.string_table.insert(value.to_string(), reference);

        // Write new string
        let length_info = (value.len() << 1) | 1;
        let mut bytes_written = self.write_u29_int(writer, length_info as u32)?;
        writer.write_all(value.as_bytes())?;
        bytes_written += value.len();

        Ok(bytes_written)
    }

    /// Find object reference in table (simplified - in real implementation would use proper comparison)
    fn find_object_reference(&self, _value: &Amf3Value) -> Option<usize> {
        // For simplicity, we don't implement object reference lookup
        // In a real implementation, this would compare objects for equality
        None
    }

    /// Find trait reference in table
    fn find_trait_reference(&self, class_def: &ClassDefinition) -> Option<usize> {
        self.trait_table.iter().position(|def| def == class_def)
    }
}

impl Default for Amf3Encoder {
    fn default() -> Self {
        Self::new()
    }
}
