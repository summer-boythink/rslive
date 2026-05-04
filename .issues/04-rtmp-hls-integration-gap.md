# Issue #4: RTMP-HLS Integration Gap (Critical Architecture Issue)

## Status
**Critical** - Core feature not implemented

## Summary
The RTMP server and HLS server are completely separate components. **There is no data flow from RTMP to HLS.** This means even after fixing all other bugs, pushing a stream via RTMP will NOT generate HLS segments.

## Architecture Diagram

### Current (Broken)
```
FFmpeg ──RTMP──► RTMP Server
                    │ (no connection)
HLS Server ◄──── HlsPackager
                    │ (empty, no data)
                 Browser
```

### Required (Working)
```
FFmpeg ──RTMP──► RTMP Server ──► StreamRouter ──► HlsPackager ──► HLS Server ──► Browser
                (receives)       (forwards)       (segments)       (serves)
```

## Missing Components

### 1. RTMP to StreamRouter Bridge

**Location Needed**: `src/protocol/rtmp/server.rs` or new module

**Current State**:
```rust
// RTMP server just handles chunks, doesn't forward frames
fn handle_video_message(...) {
    // Currently just calls event handler
    if let Some(ref handler) = self.event_handlers.on_video {
        handler(connection_id, data, timestamp);
    }
}
```

**Required Implementation**:
```rust
// Need to decode FLV tags into MediaFrame and publish to StreamRouter
fn handle_video_message(...) {
    // 1. Parse FLV video tag
    let frame = parse_flv_video_tag(data, timestamp)?;
    
    // 2. Get or create StreamPublisher for this stream
    let publisher = self.get_publisher(stream_key)?;
    
    // 3. Publish frame (async or sync)
    publisher.publish(frame)?;
}
```

### 2. HLS Packager Integration

**Location Needed**: `src/protocol/hls/packager.rs` integration

**Current State**: Packager creates empty segments

**Required**:
```rust
// Subscribe to StreamRouter and generate segments
async fn run_packager(stream_id: StreamId, router: Arc<StreamRouter>) {
    let subscriber = router.subscribe(&stream_id)?;
    
    loop {
        let frame = subscriber.recv().await?;
        packager.process_frame(frame).await?;
    }
}
```

### 3. Server Binary Integration

**Location**: `src/bin/server.rs`

**Current State**: RTMP and HLS servers run independently

```rust
// Currently no connection between them
self.start_rtmp_server();  // Spawns thread, no router integration
let hls_handle = self.start_hls_server().await?;  // Uses router, but router is empty
```

**Required**:
```rust
// RTMP server needs to use the same router
let router = Arc::new(StreamRouter::new(config));

// Pass router to RTMP server
rtmp_server.set_router(Arc::clone(&router));

// HLS server already uses router
let hls_server = HlsServer::new(
    Arc::clone(&router),  // Same router!
    ...
);
```

## Implementation Plan

### Phase 1: FLV Tag Parsing
```rust
// src/protocol/flv/decoder.rs or new file
pub fn decode_video_tag(data: &[u8], timestamp: u32) -> Result<MediaFrame> {
    // Parse FLV video tag header
    let frame_type = (data[0] >> 4) & 0x0f;
    let codec_id = data[0] & 0x0f;
    
    // Extract H.264 NAL units
    let nal_data = &data[1..];  // Skip FLV header
    
    // Create MediaFrame
    MediaFrame::new(
        track_id,
        Timestamp::from_millis(timestamp as u64),
        FrameType::Video(...),
        CodecType::H264,
        Bytes::from(nal_data),
    )
}
```

### Phase 2: RTMP Frame Publishing
```rust
// src/protocol/rtmp/server.rs
impl RtmpServer {
    fn handle_video_data(&self, conn_id: usize, data: &[u8], timestamp: u32) {
        // Parse stream key from connection
        let stream_key = self.get_stream_key(conn_id);
        
        // Get or create publisher
        let publisher = self.publishers
            .entry(stream_key.clone())
            .or_insert_with(|| {
                self.router.publish(StreamId::new(stream_key)).unwrap()
            });
        
        // Decode and publish
        let frame = decode_video_tag(data, timestamp).unwrap();
        publisher.try_publish(frame).unwrap();
    }
}
```

### Phase 3: Auto-Start Packager
```rust
// src/protocol/hls/packager_manager.rs
impl HlsPackagerManager {
    pub fn auto_create_on_publish(&self, router: Arc<StreamRouter>) {
        // Watch for new streams in router
        tokio::spawn(async move {
            loop {
                for stream_id in router.stream_ids() {
                    if self.get_packager(&stream_id).is_none() {
                        self.create_packager_for_stream(stream_id, router.clone());
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }
}
```

## Workaround (Manual Testing)

Until this is implemented, you can test HLS by manually creating segments:

```rust
// Test code to inject frames directly
let router = Arc::new(StreamRouter::new(config));
let publisher = router.publish(StreamId::new("test")).unwrap();

// Create fake frames
let frame = MediaFrame::new(...);
publisher.publish(frame).await?;
```

## Impact

- **Severity**: Critical - Core feature broken
- **User Impact**: Cannot use RTMP → HLS workflow
- **Workaround**: None (without code changes)

## Related Issues

- [#1] RTMP Server EAGAIN Error - Blocks RTMP connection
- [#2] HLS Server CORS Issues - Blocks browser playback
- [#3] StreamRouter Design Issues - Affects frame forwarding

## Priority

**P0** - Must fix before any meaningful usage
