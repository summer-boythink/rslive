//! Performance benchmarks for rslive protocols
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use bytes::Bytes;
use std::io::Cursor;

// ============================================================================
// RTMP Chunk Benchmarks
// ============================================================================

fn bench_rtmp_chunk_processing(c: &mut Criterion) {
    use rslive::rtmp::{RtmpChunkHandler, RtmpMessageHeader, RtmpMessage};

    let mut group = c.benchmark_group("rtmp_chunk");

    // Benchmark: Create chunks from a message
    {
        let handler = RtmpChunkHandler::new(128);
        let header = RtmpMessageHeader::new(8, 1000, 12345, 1);
        let payload = Bytes::from(vec![0xAF; 1000]);
        let message = RtmpMessage::new(header, payload);

        group.bench_function("create_chunks_1kb", |b| {
            b.iter(|| {
                black_box(handler.create_chunks(&message, 4, 128))
            })
        });
    }

    // Benchmark: Process incoming chunks
    {
        let handler = RtmpChunkHandler::new(128);
        let header = RtmpMessageHeader::new(8, 1000, 12345, 1);
        let payload = Bytes::from(vec![0xAF; 1000]);
        let message = RtmpMessage::new(header, payload.clone());
        let chunks = handler.create_chunks(&message, 4, 128);

        group.bench_function("process_chunks_1kb", |b| {
            b.iter(|| {
                let mut handler = RtmpChunkHandler::new(128);
                for chunk in &chunks {
                    let _ = handler.process_chunk(chunk.clone());
                }
                black_box(handler)
            })
        });
    }

    // Benchmark: Different chunk sizes
    for chunk_size in [128, 512, 1024, 4096].iter() {
        let handler = RtmpChunkHandler::new(*chunk_size);
        let header = RtmpMessageHeader::new(8, 65536, 12345, 1);
        let payload = Bytes::from(vec![0xAF; 65536]);
        let message = RtmpMessage::new(header, payload);

        group.bench_with_input(
            BenchmarkId::new("create_chunks_size", chunk_size),
            chunk_size,
            |b, _| {
                b.iter(|| {
                    black_box(handler.create_chunks(&message, 4, *chunk_size))
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// AMF Encoding Benchmarks
// ============================================================================

fn bench_amf_encoding(c: &mut Criterion) {
    use rslive::amf0::{Amf0Encoder, Amf0Value};
    use rslive::amf3::{Amf3Encoder, Amf3Value};
    use std::collections::HashMap;

    let mut group = c.benchmark_group("amf_encoding");

    // AMF0 String encoding
    group.bench_function("amf0_encode_string", |b| {
        let value = Amf0Value::String("hello world".to_string());
        b.iter(|| {
            let mut buf = Vec::new();
            Amf0Encoder::encode_value(&mut buf, &value).unwrap();
            black_box(buf)
        })
    });

    // AMF0 Object encoding
    group.bench_function("amf0_encode_object", |b| {
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), Amf0Value::String("test".to_string()));
        obj.insert("value".to_string(), Amf0Value::Number(42.0));
        let value = Amf0Value::Object(obj);

        b.iter(|| {
            let mut buf = Vec::new();
            Amf0Encoder::encode_value(&mut buf, &value).unwrap();
            black_box(buf)
        })
    });

    // AMF3 String encoding (with reference table)
    group.bench_function("amf3_encode_string", |b| {
        let value = Amf3Value::String("hello world".to_string());
        b.iter(|| {
            let mut encoder = Amf3Encoder::new();
            let mut buf = Vec::new();
            encoder.encode(&mut buf, &value).unwrap();
            black_box(buf)
        })
    });

    // AMF3 Object encoding
    group.bench_function("amf3_encode_object", |b| {
        let value = Amf3Value::object(
            "TestClass".to_string(),
            vec![
                ("name".to_string(), Amf3Value::String("test".to_string())),
                ("value".to_string(), Amf3Value::Integer(42)),
            ],
        );

        b.iter(|| {
            let mut encoder = Amf3Encoder::new();
            let mut buf = Vec::new();
            encoder.encode(&mut buf, &value).unwrap();
            black_box(buf)
        })
    });

    // AMF3 Array encoding
    group.bench_function("amf3_encode_array_100", |b| {
        let items: Vec<Amf3Value> = (0..100)
            .map(|i| Amf3Value::Integer(i))
            .collect();
        let value = Amf3Value::array(items);

        b.iter(|| {
            let mut encoder = Amf3Encoder::new();
            let mut buf = Vec::new();
            encoder.encode(&mut buf, &value).unwrap();
            black_box(buf)
        })
    });

    group.finish();
}

// ============================================================================
// AMF Decoding Benchmarks
// ============================================================================

fn bench_amf_decoding(c: &mut Criterion) {
    use rslive::amf0::{Amf0Encoder, Amf0Decoder, Amf0Value};
    use rslive::amf3::{Amf3Encoder, Amf3Decoder, Amf3Value};
    use std::collections::HashMap;

    let mut group = c.benchmark_group("amf_decoding");

    // Pre-encode data for decoding benchmarks
    let amf0_string = {
        let value = Amf0Value::String("hello world".to_string());
        let mut buf = Vec::new();
        Amf0Encoder::encode_value(&mut buf, &value).unwrap();
        buf
    };

    let amf0_object = {
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), Amf0Value::String("test".to_string()));
        obj.insert("value".to_string(), Amf0Value::Number(42.0));
        let value = Amf0Value::Object(obj);
        let mut buf = Vec::new();
        Amf0Encoder::encode_value(&mut buf, &value).unwrap();
        buf
    };

    let amf3_string = {
        let value = Amf3Value::String("hello world".to_string());
        let mut encoder = Amf3Encoder::new();
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &value).unwrap();
        buf
    };

    let amf3_object = {
        let value = Amf3Value::object(
            "TestClass".to_string(),
            vec![
                ("name".to_string(), Amf3Value::String("test".to_string())),
                ("value".to_string(), Amf3Value::Integer(42)),
            ],
        );
        let mut encoder = Amf3Encoder::new();
        let mut buf = Vec::new();
        encoder.encode(&mut buf, &value).unwrap();
        buf
    };

    // AMF0 String decoding
    group.bench_function("amf0_decode_string", |b| {
        b.iter(|| {
            let mut cursor = Cursor::new(&amf0_string);
            let result = Amf0Decoder::decode_value(&mut cursor).unwrap();
            black_box(result)
        })
    });

    // AMF0 Object decoding
    group.bench_function("amf0_decode_object", |b| {
        b.iter(|| {
            let mut cursor = Cursor::new(&amf0_object);
            let result = Amf0Decoder::decode_value(&mut cursor).unwrap();
            black_box(result)
        })
    });

    // AMF3 String decoding
    group.bench_function("amf3_decode_string", |b| {
        b.iter(|| {
            let mut cursor = Cursor::new(&amf3_string);
            let result = Amf3Decoder::decode_value(&mut cursor).unwrap();
            black_box(result)
        })
    });

    // AMF3 Object decoding
    group.bench_function("amf3_decode_object", |b| {
        b.iter(|| {
            let mut cursor = Cursor::new(&amf3_object);
            let result = Amf3Decoder::decode_value(&mut cursor).unwrap();
            black_box(result)
        })
    });

    group.finish();
}

// ============================================================================
// FLV Encoding Benchmarks
// ============================================================================

fn bench_flv_encoding(c: &mut Criterion) {
    use rslive::flv::FlvEncoder;
    use rslive::media::{MediaFrame, Timestamp, VideoFrameType, CodecType};
    use bytes::Bytes;

    let mut group = c.benchmark_group("flv_encoding");

    // Create test video frame
    let video_frame = MediaFrame::video(
        1,
        Timestamp::from_millis(1000),
        VideoFrameType::Keyframe,
        CodecType::H264,
        Bytes::from(vec![0x67, 0x42, 0x00, 0x0A, 0x95, 0xA8]),  // Fake H.264 data
    );

    group.bench_function("flv_encode_video_frame", |b| {
        let mut encoder = FlvEncoder::video_audio();
        encoder.header(); // Send header once
        b.iter(|| {
            black_box(encoder.encode_frame(&video_frame).unwrap())
        })
    });

    // Benchmark with sequence headers
    group.bench_function("flv_encode_with_headers", |b| {
        let mut encoder = FlvEncoder::video_audio();
        encoder.header();
        b.iter(|| {
            black_box(encoder.encode_frame_with_headers(&video_frame).unwrap())
        })
    });

    group.finish();
}

// ============================================================================
// Buffer Pool Benchmarks
// ============================================================================

fn bench_buffer_pool(c: &mut Criterion) {
    use rslive::utils::BufferPool;

    let mut group = c.benchmark_group("buffer_pool");

    // Small pool
    let small_pool = BufferPool::new(128, 4 * 1024);

    group.bench_function("pool_get_small", |b| {
        b.iter(|| {
            let mut buf = small_pool.get();
            buf.extend_from_slice(b"test data");
            black_box(buf)
        })
    });

    // Large pool
    let large_pool = BufferPool::new(16, 1024 * 1024);

    group.bench_function("pool_get_large", |b| {
        b.iter(|| {
            let mut buf = large_pool.get();
            buf.extend_from_slice(&[0u8; 65536]);
            black_box(buf)
        })
    });

    // Compare with direct allocation
    group.bench_function("direct_allocation", |b| {
        b.iter(|| {
            let mut buf = Vec::with_capacity(4096);
            buf.extend_from_slice(b"test data");
            black_box(buf)
        })
    });

    // Concurrent access simulation
    group.bench_function("pool_reuse", |b| {
        b.iter(|| {
            {
                let mut buf = small_pool.get();
                buf.extend_from_slice(b"test data");
            } // Buffer returns to pool
            {
                let mut buf = small_pool.get();
                buf.extend_from_slice(b"more data");
            }
            black_box(small_pool.available())
        })
    });

    group.finish();
}

// ============================================================================
// Concurrent Access Benchmarks
// ============================================================================

fn bench_concurrent_access(c: &mut Criterion) {
    use dashmap::DashMap;
    use std::sync::Mutex;
    use std::collections::HashMap;

    let mut group = c.benchmark_group("concurrent");

    // DashMap vs Mutex<HashMap> read
    let dashmap = DashMap::new();
    for i in 0..1000 {
        dashmap.insert(i, format!("value_{}", i));
    }

    let mutex_map: Mutex<HashMap<i32, String>> = Mutex::new(
        (0..1000).map(|i| (i, format!("value_{}", i))).collect()
    );

    group.bench_function("dashmap_read", |b| {
        b.iter(|| {
            for i in 0..100 {
                let _ = dashmap.get(&(i % 1000));
            }
        })
    });

    group.bench_function("mutex_hashmap_read", |b| {
        b.iter(|| {
            for i in 0..100 {
                let map = mutex_map.lock().unwrap();
                let _ = map.get(&(i % 1000));
            }
        })
    });

    // DashMap vs Mutex<HashMap> write
    group.bench_function("dashmap_write", |b| {
        let map: DashMap<i32, String> = DashMap::new();
        b.iter(|| {
            for i in 0..100 {
                map.insert(i, format!("value_{}", i));
            }
            black_box(map.len())
        })
    });

    group.bench_function("mutex_hashmap_write", |b| {
        let map: Mutex<HashMap<i32, String>> = Mutex::new(HashMap::new());
        b.iter(|| {
            for i in 0..100 {
                let mut m = map.lock().unwrap();
                m.insert(i, format!("value_{}", i));
            }
            black_box(map.lock().unwrap().len())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_rtmp_chunk_processing,
    bench_amf_encoding,
    bench_amf_decoding,
    bench_flv_encoding,
    bench_buffer_pool,
    bench_concurrent_access,
);

criterion_main!(benches);
