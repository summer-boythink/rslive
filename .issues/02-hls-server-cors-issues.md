# Issue #2: HLS Server CORS and Routing Issues

## Status
**Fixed** - Pending comprehensive testing

## Description
Web browsers cannot access HLS streams due to:
1. Missing CORS headers
2. Incorrect route patterns (axum path parameters)
3. 404 errors when stream doesn't exist

## Error Log
```
Access to XMLHttpRequest at 'http://127.0.0.1:8080/hls/live/stream1/index.m3u8'
from origin 'http://127.0.0.1:8081' has been blocked by CORS policy:
No 'Access-Control-Allow-Origin' header is present on the requested resource.

GET http://127.0.0.1:8080/hls/live/stream1/index.m3u8 404 (Not Found)
```

## Applied Fixes

### Fix 1: Add CORS Middleware
```rust
use tower_http::cors::{Any, CorsLayer};

let cors_layer = CorsLayer::new()
    .allow_origin(Any)
    .allow_methods([http::Method::GET, http::Method::OPTIONS])
    .allow_headers(Any)
    .max_age(Duration::from_secs(86400));
```

### Fix 2: Correct Route Patterns
Changed from axum 0.6 style (`:param`) to 0.7 style (`{param}`):
```rust
// Before (incorrect)
.route("/hls/:stream/index.m3u8", ...)

// After (correct)
.route("/hls/{stream}/index.m3u8", ...)
.route("/hls/{stream}/segment/{idx}", ...)
```

### Fix 3: Better 404 Handling
Added informative error message when stream doesn't exist:
```rust
None => {
    return axum::response::Response::builder()
        .status(404)
        .body(Body::from(format!(
            "Stream '{}' not found. Start streaming with:\n\nffmpeg ...",
            stream_name
        )))
        .unwrap();
}
```

## Architecture Issue: Stream Registration Gap

### Problem
The HLS server's `has_stream()` check looks at `StreamRouter`, but:
- RTMP server doesn't register streams with `StreamRouter`
- `StreamRouter` is independent and unused by RTMP
- This creates a gap in the architecture

### Code Location
```rust
// src/protocol/hls/server.rs:148
if !state.router.has_stream(&stream_id) {
    return not_found(format!("Stream '{}' not found", stream_name));
}
```

### Current Workaround
Temporarily disabled strict stream existence check to allow polling until segments are available.

## Testing Checklist

- [ ] CORS preflight request (OPTIONS)
- [ ] GET request from different origin
- [ ] Master playlist (master.m3u8)
- [ ] Media playlist (index.m3u8)
- [ ] Segment files (segment/{idx})
- [ ] 404 response with helpful message
- [ ] Health check endpoint

## Browser Compatibility

| Browser | HLS.js Required | Notes |
|---------|----------------|-------|
| Safari | No | Native HLS support |
| Chrome | Yes | Use hls.js library |
| Firefox | Yes | Use hls.js library |
| Edge | Yes | Use hls.js library |
| Mobile Safari | No | Native HLS support |
| Chrome Android | Yes | Use hls.js library |

## Recommendations

1. **Add CORS preflight cache**: `Access-Control-Max-Age: 86400`
2. **Add security headers**: `X-Content-Type-Options: nosniff`
3. **Implement proper cache control**: Different cache times for playlists vs segments
4. **Add rate limiting**: Prevent abuse of playlist endpoints
5. **Add compression**: gzip for playlists, especially with many segments

## Dependencies Added

```toml
tower-http = { version = "0.5", optional = true, features = ["cors"] }
http = { version = "1.0", optional = true }
```
