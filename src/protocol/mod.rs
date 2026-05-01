pub mod amf0;
pub mod amf3;
pub mod common;
pub mod flv;
pub mod hls;
pub mod rtmp;

/// Comprehensive examples demonstrating AMF3 and RTMP usage
pub mod examples {
    use crate::protocol::{amf0::Amf0Value, amf3::Amf3Value, rtmp::*};
    use std::collections::HashMap;

    /// Example showing AMF3 encoding and decoding
    pub fn amf3_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("=== AMF3 Example ===");

        // Create a complex AMF3 object
        let user_data = Amf3Value::object(
            "User".to_string(),
            vec![
                ("id".to_string(), Amf3Value::Integer(12345)),
                ("name".to_string(), Amf3Value::String("Alice".to_string())),
                (
                    "email".to_string(),
                    Amf3Value::String("alice@example.com".to_string()),
                ),
                ("active".to_string(), Amf3Value::True),
                ("score".to_string(), Amf3Value::Double(98.5)),
                (
                    "tags".to_string(),
                    Amf3Value::array(vec![
                        Amf3Value::String("developer".to_string()),
                        Amf3Value::String("rust".to_string()),
                        Amf3Value::String("amf".to_string()),
                    ]),
                ),
                (
                    "metadata".to_string(),
                    Amf3Value::ByteArray(vec![0x01, 0x02, 0x03, 0x04]),
                ),
            ],
        );

        // Encode to bytes
        let encoded = super::amf3::encode(&user_data)?;
        println!("Encoded AMF3 data: {} bytes", encoded.len());

        // Decode back
        let decoded = super::amf3::decode(&encoded)?;
        println!("Successfully decoded AMF3 object: {}", decoded.type_name());

        // Verify roundtrip
        assert_eq!(user_data, decoded);
        println!("✓ AMF3 roundtrip successful!");

        Ok(())
    }

    /// Example showing RTMP message creation and parsing
    pub fn rtmp_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== RTMP Example ===");

        // Create a connect command
        let connect_cmd =
            AmfCommand::connect(1.0, "live", "WIN 32,0,0,137", "rtmp://localhost:1935/live");

        // Create RTMP message from command
        let connect_msg = RtmpMessage::create_amf0_command(
            &connect_cmd.command_name,
            connect_cmd.transaction_id,
            Some(connect_cmd.command_object.clone()),
            connect_cmd.arguments.clone(),
            0,
            0,
        )?;

        println!(
            "Created RTMP connect message: {} bytes",
            connect_msg.payload.len()
        );

        // Parse the command back
        let parsed_cmd = connect_msg.parse_amf0_command()?;
        println!("Parsed command: {}", parsed_cmd.command_name);
        println!("Transaction ID: {}", parsed_cmd.transaction_id);

        // Create a publish command
        let publish_cmd = AmfCommand::publish(2.0, "my_stream", "live");
        let publish_msg = RtmpMessage::create_amf0_command(
            &publish_cmd.command_name,
            publish_cmd.transaction_id,
            Some(publish_cmd.command_object.clone()),
            publish_cmd.arguments.clone(),
            1000,
            1,
        )?;

        println!(
            "Created publish message: {} bytes",
            publish_msg.payload.len()
        );

        // Create control messages
        let chunk_size_msg = ControlMessage::SetChunkSize(4096);
        let rtmp_chunk_msg = chunk_size_msg.to_rtmp_message(0)?;
        println!(
            "Created chunk size control message: {} bytes",
            rtmp_chunk_msg.payload.len()
        );

        // Create audio/video messages
        let audio_data = vec![0xAF, 0x01, 0x16, 0x44, 0x40, 0x00]; // Sample AAC audio
        let audio_msg = RtmpMessage::create_audio_message(audio_data, 2000, 1);
        println!("Created audio message: {} bytes", audio_msg.payload.len());

        let video_data = vec![0x17, 0x01, 0x00, 0x00, 0x00, 0x01, 0x42, 0x00, 0x20]; // Sample H.264 video
        let video_msg = RtmpMessage::create_video_message(video_data, 2000, 1);
        println!("Created video message: {} bytes", video_msg.payload.len());

        println!("✓ RTMP message creation successful!");

        Ok(())
    }

    /// Example showing chunk handling
    pub fn chunk_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== RTMP Chunk Example ===");

        let mut chunk_handler = RtmpChunkHandler::new(128);

        // Create a large message that will be split into chunks
        let _metadata = vec![0xFF; 1000]; // 1000 bytes of metadata
        let metadata_msg = RtmpMessage::create_amf0_data(
            "onMetaData",
            vec![Amf0Value::String("test metadata".to_string())],
            5000,
            1,
        )?;

        // Split message into chunks
        let chunks = chunk_handler.create_chunks(&metadata_msg, 4, 128);
        println!("Split message into {} chunks", chunks.len());

        for (i, chunk) in chunks.iter().enumerate() {
            println!(
                "  Chunk {}: format={}, size={} bytes",
                i + 1,
                chunk.header.format,
                chunk.data.len()
            );
        }

        // Simulate processing chunks to reconstruct message
        let mut reconstructed_messages = Vec::new();
        for chunk in chunks {
            if let Some(msg) = chunk_handler.process_chunk(chunk)? {
                reconstructed_messages.push(msg);
            }
        }

        println!(
            "Reconstructed {} complete messages",
            reconstructed_messages.len()
        );
        println!("✓ Chunk handling successful!");

        Ok(())
    }

    /// Example showing AMF0 to AMF3 interoperability
    pub fn interop_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== AMF0/AMF3 Interoperability Example ===");

        // Create AMF0 data
        let mut amf0_obj = HashMap::new();
        amf0_obj.insert("name".to_string(), Amf0Value::String("Test".to_string()));
        amf0_obj.insert("value".to_string(), Amf0Value::Number(42.0));
        let amf0_data = Amf0Value::Object(amf0_obj);

        // Encode AMF0
        let amf0_encoded = super::amf0::encode(&amf0_data)?;
        println!("AMF0 encoded size: {} bytes", amf0_encoded.len());

        // Create equivalent AMF3 data
        let amf3_data = Amf3Value::object(
            "".to_string(), // Anonymous object
            vec![
                ("name".to_string(), Amf3Value::String("Test".to_string())),
                ("value".to_string(), Amf3Value::Double(42.0)),
            ],
        );

        // Encode AMF3
        let amf3_encoded = super::amf3::encode(&amf3_data)?;
        println!("AMF3 encoded size: {} bytes", amf3_encoded.len());

        // AMF3 is typically more compact due to reference tables
        if amf3_encoded.len() <= amf0_encoded.len() {
            println!("✓ AMF3 encoding is more compact or equal");
        } else {
            println!("! AMF3 encoding is larger (may vary with data complexity)");
        }

        // Both can be embedded in RTMP messages
        let amf0_msg = RtmpMessage::create_amf0_data("setDataFrame", vec![amf0_data], 1000, 1)?;
        println!("AMF0 RTMP message: {} bytes", amf0_msg.payload.len());

        println!("✓ Interoperability example completed!");

        Ok(())
    }

    /// Run all examples
    pub fn run_all_examples() -> Result<(), Box<dyn std::error::Error>> {
        println!("🚀 Running rslive Protocol Examples");
        println!("=====================================");

        amf3_example()?;
        rtmp_example()?;
        chunk_example()?;
        interop_example()?;

        println!("\n🎉 All examples completed successfully!");
        Ok(())
    }

    /// Example showing RTMP server usage
    pub fn rtmp_server_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== RTMP Server Example ===");

        // Create server with event handlers
        let server = super::rtmp::RtmpServer::with_defaults()
            .on_connect(|id, _command| {
                println!("Client {} connected", id);
                true // Accept connection
            })
            .on_publish(|id, stream_name| {
                println!("Client {} started publishing '{}'", id, stream_name);
                true // Allow publishing
            })
            .on_play(|id, stream_name| {
                println!("Client {} started playing '{}'", id, stream_name);
                true // Allow playing
            })
            .on_disconnect(|id| {
                println!("Client {} disconnected", id);
            });

        println!("RTMP Server configured with event handlers");

        // Get server stats
        let stats = server.get_stats();
        println!("Server stats: {:?}", stats);

        println!("✓ RTMP server example completed!");
        Ok(())
    }

    /// Example showing RTMP client usage
    pub fn rtmp_client_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== RTMP Client Example ===");

        let client = super::rtmp::RtmpClient::with_defaults();

        // Show URL parsing
        println!("Client created successfully");
        println!("App name: '{}'", client.get_app_name());
        println!("Connected: {}", client.is_connected());

        // Show stats
        let stats = client.get_stats();
        println!(
            "Client stats - State: {:?}, Duration: {:?}",
            stats.state, stats.duration
        );

        println!("✓ RTMP client example completed!");
        Ok(())
    }

    /// Example showing RTMP handshake process
    pub fn rtmp_handshake_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== RTMP Handshake Example ===");

        // Create handshake instances
        let client_handshake = super::rtmp::RtmpHandshake::new();
        let server_handshake = super::rtmp::RtmpHandshake::new();

        println!("Client timestamp: {}", client_handshake.timestamp);
        println!("Server timestamp: {}", server_handshake.timestamp);
        println!(
            "Random bytes length: {}",
            client_handshake.random_bytes.len()
        );

        // Show handshake data size
        println!(
            "Random bytes per handshake: {} bytes",
            super::rtmp::RTMP_HANDSHAKE_SIZE
        );

        println!("✓ RTMP handshake example completed!");
        Ok(())
    }

    /// Example showing RTMP connection management
    pub fn rtmp_connection_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== RTMP Connection Example ===");

        let config = super::rtmp::RtmpConfig::default()
            .with_chunk_size(4096)
            .with_timeout(30)
            .with_max_connections(1000);

        let mut connection = super::rtmp::RtmpConnection::new(config);

        println!("Connection created with state: {:?}", connection.state);

        // Add some streams
        let stream1_id = connection.add_stream("live_stream1".to_string());
        let stream2_id = connection.add_stream("live_stream2".to_string());

        println!("Created streams: {} and {}", stream1_id, stream2_id);

        // Get transaction IDs
        let tx1 = connection.next_transaction_id();
        let tx2 = connection.next_transaction_id();

        println!("Transaction IDs: {} and {}", tx1, tx2);

        // Show connection stats
        let stats = connection.get_stats();
        println!(
            "Connection stats - Streams: {}, State: {:?}",
            stats.stream_count, stats.state
        );

        println!("✓ RTMP connection example completed!");
        Ok(())
    }

    /// Example showing advanced RTMP message handling
    pub fn rtmp_advanced_example() -> Result<(), Box<dyn std::error::Error>> {
        println!("\n=== Advanced RTMP Example ===");

        // Create various RTMP messages
        let connect_cmd = super::rtmp::AmfCommand::connect(
            1.0,
            "myapp",
            "WIN 32,0,0,137",
            "rtmp://localhost:1935/myapp",
        );

        let connect_msg = super::rtmp::RtmpMessage::create_amf0_command(
            &connect_cmd.command_name,
            connect_cmd.transaction_id,
            Some(connect_cmd.command_object),
            connect_cmd.arguments,
            0,
            0,
        )?;

        println!("Connect message size: {} bytes", connect_msg.payload.len());

        // Create control messages
        let chunk_size_ctrl = super::rtmp::ControlMessage::SetChunkSize(8192);
        let ack_ctrl = super::rtmp::ControlMessage::Acknowledgement(12345);
        let window_ctrl = super::rtmp::ControlMessage::WindowAckSize(2500000);

        println!(
            "Control message types: {:?}, {:?}, {:?}",
            chunk_size_ctrl.message_type(),
            ack_ctrl.message_type(),
            window_ctrl.message_type()
        );

        // Create media messages
        let audio_data = vec![0xAF, 0x01, 0x16, 0x44]; // Sample AAC audio header
        let video_data = vec![0x17, 0x01, 0x00, 0x00]; // Sample H.264 video header

        let audio_msg = super::rtmp::RtmpMessage::create_audio_message(audio_data, 1000, 1);
        let video_msg = super::rtmp::RtmpMessage::create_video_message(video_data, 1000, 1);

        println!(
            "Audio message: {} bytes, Video message: {} bytes",
            audio_msg.payload.len(),
            video_msg.payload.len()
        );

        // Chunk handling
        let chunk_handler = super::rtmp::RtmpChunkHandler::new(4096);
        let chunks = chunk_handler.create_chunks(&connect_msg, 3, 128);

        println!("Message split into {} chunks", chunks.len());
        for (i, chunk) in chunks.iter().enumerate() {
            println!(
                "  Chunk {}: format={}, data_size={}",
                i + 1,
                chunk.header.format,
                chunk.data.len()
            );
        }

        println!("✓ Advanced RTMP example completed!");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::examples::*;

    #[test]
    fn test_examples() {
        assert!(run_all_examples().is_ok());
        assert!(rtmp_server_example().is_ok());
        assert!(rtmp_client_example().is_ok());
        assert!(rtmp_handshake_example().is_ok());
        assert!(rtmp_connection_example().is_ok());
        assert!(rtmp_advanced_example().is_ok());
    }
}
