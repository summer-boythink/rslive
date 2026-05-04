# Issue #1: RTMP Server EAGAIN (os error 35) on Handshake

## Status
**Partially Fixed** - Requires verification on target hardware

## Description
RTMP server fails during handshake with `Resource temporarily unavailable (os error 35)` error. This occurs when FFmpeg tries to connect and push a stream.

## Error Log
```
New client connected: 192.168.31.190:34778
New connection: 0
Connection 0 handshake failed: IO error: Resource temporarily unavailable (os error 35)
Connection 0 error: IO error: Resource temporarily unavailable (os error 35)
```

## Root Cause Analysis

### Primary Issue
The `TcpStream` was not explicitly set to blocking mode on some systems, causing `read_exact` to return `EAGAIN` when data hasn't arrived yet.

### Secondary Issue
`TcpListener::incoming()` iterator may return `EAGAIN` on non-blocking sockets in certain Linux kernel configurations.

## Attempted Fixes

### Fix 1: Set Listener to Blocking Mode
```rust
listener.set_nonblocking(false)?;
```
**Location**: `src/protocol/rtmp/server.rs:172`

### Fix 2: Set Connection Socket to Blocking Mode
```rust
stream.set_nonblocking(false)?;
```
**Location**: `src/protocol/rtmp/server.rs:313`

### Fix 3: Replace Iterator with Loop
Changed from `listener.incoming()` to `listener.accept()` loop with EAGAIN handling.

## Verification Steps

```bash
# Terminal 1: Start server
./target/release/rslive-server

# Terminal 2: Test with FFmpeg
ffmpeg -re -i demo.fl4 -c:v libx264 -c:a aac -f flv rtmp://127.0.0.1:1935/live/test

# Should see:
# New client connected: 127.0.0.1:xxxxx
# (no handshake error)
```

## Related Issues
- [#2] HLS Server CORS Issues
- [#4] RTMP-HLS Integration Gap

## Recommendations

1. **Add TCP Keepalive**: Prevent connection drops during idle periods
2. **Add Detailed Logging**: Log each handshake step for debugging
3. **Add Connection Timeout**: Fail fast on stuck handshakes
4. **Test on Multiple Platforms**: macOS, Ubuntu x86_64, ARM64

## Design Consideration

The current threading model spawns a new thread per connection:
```rust
thread::spawn(move || {
    handle_client_connection(...)
});
```

This may not scale well for 1000+ concurrent connections. Consider:
- Thread pool (rayon or custom)
- Async/await for I/O (tokio)
- Epoll/kqueue for event-driven I/O
