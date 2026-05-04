# Issue #5: Memory Safety and Resource Leak Concerns

## Status
**Under Review** - Potential issues identified

## Overview
Several areas in the codebase may have memory safety issues, resource leaks, or undefined behavior under certain conditions.

## Issues

### Issue 5.1: Potential Deadlock in RTMP Server

**Severity**: High
**Location**: `src/protocol/rtmp/server.rs:300-340`

**Problem**: Multiple Mutex locks held simultaneously:

```rust
fn handle_client_connection(...) {
    let mut conn = connection.lock().unwrap();  // Lock #1
    conn.server_handshake(&mut stream)?;
    // ...
    loop {
        let mut conn = connection.lock().unwrap();  // Lock #1 again (reentrant?)
        conn.read_chunk(&mut stream)?;
        // ...
        Self::process_client_message(
            &mut stream,
            &connection,  // Passed to another function that may lock
            ...
        )?;
    }
}

fn process_client_message(..., connection: &Arc<Mutex<RtmpConnection>>) {
    let mut conn = connection.lock().unwrap();  // Lock #1 - DEADLOCK if already held!
    conn.process_message(...)?;
}
```

**Analysis**: 
- The code uses `std::sync::Mutex` which is not reentrant
- If `process_client_message` is called while lock is held, it will deadlock
- However, in current code the lock is dropped before calling, so it may be safe

**Recommendation**: Use `parking_lot::Mutex` or restructure to avoid passing `Arc<Mutex<>>`

---

### Issue 5.2: Unbounded Memory Growth in HLS Storage

**Severity**: Medium
**Location**: `src/protocol/hls/segment.rs:199-227`

**Problem**: `MemorySegmentStorage` has max_segments but doesn't account for segment size:

```rust
pub struct MemorySegmentStorage {
    segments: Arc<DashMap<u64, Segment>>,
    max_segments: usize,  // Count-based limit only
}

fn enforce_limits(&self) {
    while self.segments.len() > self.max_segments {
        // Remove oldest...
    }
}
```

**Impact**: 
- Each segment can be several MB
- 100 segments × 2MB = 200MB (may be acceptable)
- But 100 segments × 10MB = 1GB (problematic)

**Fix**:
```rust
pub struct MemorySegmentStorage {
    segments: Arc<DashMap<u64, Segment>>,
    max_segments: usize,
    max_memory_bytes: usize,  // Add memory-based limit
    current_memory: AtomicUsize,
}
```

---

### Issue 5.3: Frame Data Cloning in Router

**Severity**: Medium
**Location**: `src/media/router.rs:148-221`

**Problem**: Despite "zero-copy" claims, frames are cloned multiple times:

```rust
// In publish()
cache.push(frame.share());  // Clone #1

// For each subscriber
sender.try_send(frame.share());  // Clone #2..N

// In try_publish() - same frame cloned again
cache.push(frame.share());
```

**Analysis**:
- `frame.share()` clones the `Arc<Bytes>` (cheap - just increments refcount)
- But the frame struct itself is cloned (contains multiple fields)
- True zero-copy would pass references without any cloning

**Recommendation**: This is likely acceptable for performance, but documentation should clarify what "zero-copy" means

---

### Issue 5.4: Unclosed Channels on Stream End

**Severity**: Medium
**Location**: `src/media/router.rs:474-493`

**Problem**: When stream is unpublished, subscriber channels may not be properly closed:

```rust
pub fn unpublish(&self, stream_id: &StreamId) {
    self.streams.remove(stream_id);  // Removes entire stream
    // Subscribers are left hanging with open channels!
}
```

**Impact**:
- Subscriber `recv()` calls will hang indefinitely
- Memory leak from channel buffers
- No notification to subscribers that stream ended

**Fix**:
```rust
pub fn unpublish(&self, stream_id: &StreamId) {
    if let Some((_, state)) = self.streams.remove(stream_id) {
        // Signal all subscribers that stream is ending
        for sender in &state.subscribers {
            let _ = sender.send(MediaFrame::eos_frame());  // Send EOS marker
        }
        // Channels will be dropped when senders go out of scope
    }
}
```

---

### Issue 5.5: Potential Panic in UTF-8 Conversion

**Severity**: Low
**Location**: `src/protocol/amf0/decode.rs` (various)

**Problem**: Several places use `unwrap()` on UTF-8 conversion:

```rust
let string = String::from_utf8(bytes).unwrap();  // Panic on invalid UTF-8
```

**Impact**: 
- Malicious/malformed AMF data can crash the server
- Not resilient to bad input

**Fix**:
```rust
let string = String::from_utf8(bytes)
    .map_err(|_| RtmpError::InvalidData("Invalid UTF-8 in AMF string".into()))?;
```

---

### Issue 5.6: Integer Overflow in Timestamp Calculations

**Severity**: Low
**Location**: Various timestamp conversions

**Problem**: Conversions between different time units may overflow:

```rust
// src/protocol/hls/fmp4/mod.rs:66
pub fn nanos_to_timescale(nanos: u64, timescale: u32) -> u64 {
    ((nanos as u128) * (timescale as u128) / 1_000_000_000) as u64
}
```

**Analysis**: This uses 128-bit arithmetic, so it's actually safe. But other places may not:

```rust
// Potential overflow if pts is large
let diff_ms = next_frame.pts.as_millis() - frame.pts.as_millis();
```

**Recommendation**: Audit all arithmetic operations for overflow potential

---

### Issue 5.7: Unbounded Task Spawning

**Severity**: Medium
**Location**: `src/protocol/rtmp/server.rs:197-207`

**Problem**: Each connection spawns a new thread without limit:

```rust
thread::spawn(move || {
    handle_client_connection(...)
});
```

**Impact**:
- 10,000 connections = 10,000 threads
- Can exhaust system resources
- No backpressure on connection acceptance

**Fix**: Use thread pool:
```rust
use rayon::ThreadPool;

pub struct RtmpServer {
    thread_pool: ThreadPool,
    // ...
}

// In listen()
self.thread_pool.spawn(move || {
    handle_client_connection(...)
});
```

---

### Issue 5.8: Mutex Poisoning Not Handled

**Severity**: Low
**Location**: Various `lock().unwrap()` calls

**Problem**: If a thread panics while holding a lock, the lock is poisoned:

```rust
let mut conn = connection.lock().unwrap();  // Panic if poisoned
```

**Impact**:
- Server may crash on lock poisoning
- No recovery mechanism

**Fix**:
```rust
let mut conn = connection.lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
```

Or use `parking_lot::Mutex` which doesn't have poisoning.

---

## Recommendations

1. **Use parking_lot**: Replace all `std::sync::Mutex` with `parking_lot::Mutex`
   - No poisoning
   - Faster
   - Better ergonomics

2. **Add Resource Limits**:
   - Max connections per IP
   - Max memory per stream
   - Max total memory

3. **Add Graceful Degradation**:
   - Drop frames instead of blocking
   - Reject connections when overloaded
   - Circuit breakers for slow subscribers

4. **Memory Profiling**:
   - Add metrics for memory usage
   - Monitor for leaks in long-running tests

## Testing Recommendations

```rust
#[test]
fn test_memory_leak() {
    let router = StreamRouter::new(config);
    
    // Create and destroy 1000 streams
    for i in 0..1000 {
        let publisher = router.publish(StreamId::new(format!("stream-{}", i))).unwrap();
        // Publish some frames...
        drop(publisher);
        router.unpublish(&StreamId::new(format!("stream-{}", i)));
    }
    
    // Check router.stream_count() is 0
    assert_eq!(router.stream_count(), 0);
}
```
