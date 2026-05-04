# Issue #3: StreamRouter Design Issues

## Status
**Known Issues** - Architecture needs review

## Overview
StreamRouter is designed for zero-copy frame forwarding between publishers and subscribers, but has several implementation issues.

## Issues

### Issue 3.1: Broken `unsubscribe()` Implementation

**Severity**: High
**Location**: `src/media/router.rs:475-493`

**Problem**: The `unsubscribe` method always removes the first subscriber instead of the specific one:

```rust
pub fn unsubscribe(&self, stream_id: &StreamId) {
    if let Some(mut entry) = self.streams.get_mut(stream_id) {
        // BUG: Always removes first subscriber!
        if !entry.subscribers.is_empty() {
            entry.subscribers.remove(0);
        }
    }
}
```

**Impact**: 
- Wrong subscriber gets removed
- Active subscribers may keep receiving data
- Subscriber count becomes inconsistent

**Fix Required**:
```rust
pub fn unsubscribe(&self, stream_id: &StreamId, subscriber_id: usize) {
    if let Some(mut entry) = self.streams.get_mut(stream_id) {
        if let Some(pos) = entry.subscribers.iter()
            .position(|s| s.is_disconnected()) {  // Or track subscriber IDs
            entry.subscribers.remove(pos);
        }
    }
}
```

---

### Issue 3.2: Subscriber Identification Missing

**Severity**: Medium
**Location**: `src/media/router.rs:418-472`

**Problem**: Subscribers are stored as raw `Sender<MediaFrame>` without unique IDs:

```rust
subscribers: Vec<Sender<MediaFrame>>,  // Can't identify specific subscriber
```

**Impact**:
- Cannot selectively remove subscribers
- Cannot track per-subscriber metrics
- Cannot implement subscriber-specific features (bitrate limiting, etc.)

**Fix Required**:
```rust
struct Subscriber {
    id: usize,
    sender: Sender<MediaFrame>,
    metadata: SubscriberMetadata,
}

struct SubscriberMetadata {
    connected_at: Instant,
    ip: Option<SocketAddr>,
    user_agent: Option<String>,
}
```

---

### Issue 3.3: Double-Lock in `publish()`

**Severity**: Medium
**Location**: `src/media/router.rs:150-221`

**Problem**: The `publish()` method acquires the stream lock multiple times:

```rust
// First lock
if let Some(entry) = self.streams.get(&self.stream_id) {
    // ... use entry ...
}
drop(entry);

// Second lock
if !disconnected.is_empty() {
    if let Some(mut entry) = self.streams.get_mut(&self.stream_id) {
        // ...
    }
}

// Third lock
if let Some(entry) = self.streams.get(&self.stream_id) {
    // ...
}
```

**Impact**:
- Unnecessary lock overhead
- Potential race conditions between locks
- Inconsistent state view between locks

**Fix Required**:
```rust
pub async fn publish(&self, frame: MediaFrame) -> MediaResult<()> {
    let entry = self.streams.get(&self.stream_id)
        .ok_or(MediaError::StreamNotFound)?;
    
    // Do everything with single lock hold
    let subscribers = entry.subscribers.clone();
    // ... send to all subscribers ...
    
    Ok(())
}
```

---

### Issue 3.4: No Backpressure for `Block` Strategy

**Severity**: High
**Location**: `src/media/router.rs:205-207`

**Problem**: `BackpressureStrategy::Block` doesn't actually block properly:

```rust
BackpressureStrategy::Block => {
    let _ = sender.send_async(frame.share()).await;  // Ignores result!
}
```

**Impact**:
- Slow subscribers can block entire publisher
- No timeout means potentially infinite block
- Channel closure not handled

**Fix Required**:
```rust
BackpressureStrategy::Block => {
    match tokio::time::timeout(
        Duration::from_millis(100),
        sender.send_async(frame.share())
    ).await {
        Ok(Ok(())) => {},
        Ok(Err(_)) => { /* Channel closed, remove subscriber */ },
        Err(_) => { /* Timeout, frame dropped */ },
    }
}
```

---

### Issue 3.5: Frame Dropping Logic is Broken

**Severity**: Medium
**Location**: `src/media/router.rs:197-216`

**Problem**: Both `DropOld` and `DropNew` strategies do the same thing:

```rust
BackpressureStrategy::DropOld | BackpressureStrategy::DropNew => {
    if !sender.is_full() {
        let _ = sender.try_send(frame.share());
    }
}
```

**Expected Behavior**:
- `DropOld`: Remove oldest frames from channel, then send new frame
- `DropNew`: Drop current frame if channel is full

**Current Behavior**: Both just skip sending if channel is full

**Fix Required**:
```rust
BackpressureStrategy::DropOld => {
    // Drain old frames until we can send
    while sender.is_full() {
        let _ = sender.try_recv();  // Drop oldest
    }
    let _ = sender.try_send(frame.share());
}
BackpressureStrategy::DropNew => {
    if sender.is_full() {
        return Ok(());  // Drop this frame
    }
    let _ = sender.try_send(frame.share());
}
```

---

### Issue 3.6: No Publisher Cleanup on Drop

**Severity**: Medium
**Location**: `src/media/router.rs:122-145`

**Problem**: `StreamPublisher` doesn't implement `Drop`, so streams may not be cleaned up when publisher disconnects.

**Fix Required**:
```rust
impl Drop for StreamPublisher {
    fn drop(&self) {
        self.streams.remove(&self.stream_id);
    }
}
```

---

### Issue 3.7: Race Condition in `subscribe()`

**Severity**: High
**Location**: `src/media/router.rs:418-472`

**Problem**: The `subscribe()` method checks limits and adds subscriber in two separate lock operations:

```rust
// First: Check subscriber count (lock #1)
let entry = self.streams.get(stream_id)?;
if entry.subscribers.len() >= self.config.max_subscribers {
    return Err(...);
}
drop(entry);  // Lock released!

// Second: Add subscriber (lock #2)
if let Some(mut entry) = self.streams.get_mut(stream_id) {
    entry.subscribers.push(sender);  // May exceed limit!
}
```

**Impact**: TOCTOU race condition - limit can be exceeded

**Fix Required**: Single atomic operation:
```rust
pub fn subscribe(&self, stream_id: &StreamId) -> MediaResult<StreamSubscriber> {
    let mut entry = self.streams
        .get_mut(stream_id)
        .ok_or(MediaError::StreamNotFound)?;
    
    if entry.subscribers.len() >= self.config.max_subscribers {
        return Err(MediaError::Router("Max subscribers reached".into()));
    }
    
    let (sender, receiver) = flume::bounded(self.config.channel_buffer);
    entry.subscribers.push(sender);
    
    Ok(StreamSubscriber::new(...))
}
```

---

## Performance Considerations

1. **Lock Contention**: DashMap is good, but frequent get/get_mut calls may cause contention
2. **Memory Allocation**: `Vec::remove(0)` is O(n), consider `VecDeque` for subscribers
3. **Frame Cloning**: `frame.share()` clones Arc, but data is not actually zero-copy on receive

## Recommendations

1. **Use Actor Pattern**: Channel-based message passing instead of locks
2. **Implement Proper Metrics**: Per-subscriber and per-stream metrics
3. **Add Circuit Breaker**: Disconnect slow subscribers automatically
4. **Implement Adaptive Bitrate**: Select quality based on subscriber bandwidth
