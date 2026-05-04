# Issue #6: Code Quality and Maintainability Issues

## Status
**Ongoing** - Collection of code quality issues

## Overview
Various code quality issues that don't affect functionality but impact maintainability and readability.

## Issues

### Issue 6.1: Inconsistent Error Handling

**Severity**: Medium
**Location**: Throughout codebase

**Problem**: Mix of `Result`, `Option`, and panics:

```rust
// Some places use unwrap (panic)
let string = String::from_utf8(bytes).unwrap();

// Some places ignore errors
let _ = sender.try_send(frame);

// Some places use proper error handling
sender.try_send(frame).map_err(|e| MediaError::SendFailed(e))?;
```

**Recommendation**: Standardize on `thiserror` for all error types and handle all errors explicitly.

---

### Issue 6.2: Missing Documentation

**Severity**: Medium
**Location**: Throughout codebase

**Problem**: Many public APIs lack documentation:

```rust
pub fn process_frame(&self, frame: MediaFrame) -> HlsResult<()> {
    // No docs explaining what this does
}
```

**Recommendation**: Add rustdoc comments for all public items:

```rust
/// Process a media frame and add it to the current segment.
///
/// This method will buffer frames until a segment boundary is reached
/// (based on keyframe or duration), then generate the segment.
///
/// # Arguments
/// * `frame` - The media frame to process
///
/// # Returns
/// * `Ok(())` if frame was successfully processed
/// * `Err(HlsError::...)` if frame processing failed
pub fn process_frame(&self, frame: MediaFrame) -> HlsResult<()> {
```

---

### Issue 6.3: Magic Numbers

**Severity**: Low
**Location**: Various

**Problem**: Hardcoded constants without explanation:

```rust
flags |= 0x010008;  // What does this mean?
writer.write_u16(writer, 0x00480000)?;  // 72 dpi? Why?
duration >= self.config.target_duration + Duration::from_secs(2)  // Why +2?
```

**Recommendation**: Define named constants:

```rust
const TFHD_DEFAULT_BASE_IS_MOOF: u32 = 0x010000;
const TFHD_DURATION_IS_EMPTY: u32 = 0x000008;
const DPI_72_IN_16_16: u32 = 0x00480000;
const SEGMENT_OVERHEAD_SECONDS: u64 = 2;
```

---

### Issue 6.4: TODO Comments Without Issue Tracking

**Severity**: Low
**Location**: Throughout codebase

**Problem**: Many TODOs scattered in code:

```rust
// TODO: Get actual bitrate
// TODO: Implement proper PES encapsulation
// TODO: Implement fMP4 muxer
// TODO: Add integration with StreamRouter
```

**Recommendation**: 
- Create GitHub issues for each TODO
- Use format: `// TODO(#123): Description`
- Prioritize and schedule implementation

---

### Issue 6.5: Test Coverage Gaps

**Severity**: Medium
**Location**: Test files

**Current Coverage** (estimated):
- protocol/rtmp: ~60%
- protocol/amf0: ~70%
- protocol/amf3: ~70%
- protocol/flv: ~50%
- protocol/hls: ~30% (needs improvement)
- media: ~80%
- utils: ~90%

**Missing Tests**:
- Error handling paths
- Concurrent access scenarios
- Resource exhaustion scenarios
- Network failure scenarios
- Protocol edge cases (invalid data, truncation)

**Recommendation**:
```rust
#[test]
fn test_publish_with_full_channel() {
    // Test DropOld strategy
}

#[test]
fn test_publish_with_disconnected_subscriber() {
    // Test subscriber cleanup
}

#[test]
fn test_invalid_rtmp_handshake() {
    // Test malformed handshake data
}

#[test]
fn test_concurrent_publish_subscribe() {
    // Test thread safety
}
```

---

### Issue 6.6: Clippy Warnings

**Severity**: Low
**Location**: Throughout codebase

**Current Warnings** (from `cargo clippy`):
- Unused imports/variables
- Functions that could be `const fn`
- Match statements that could be `if let`
- Unnecessary clones

**Recommendation**: Fix all warnings and add CI check:

```yaml
# .github/workflows/ci.yml
- name: Clippy
  run: cargo clippy --all-targets -- -D warnings
```

---

### Issue 6.7: Inconsistent Naming Conventions

**Severity**: Low
**Location**: Various

**Problem**: Mix of naming styles:

```rust
// Some use snake_case
fn handle_client_connection(...)

// Some use abbreviated names
fn calc_ts_timestamp(...)

// Some use full words
fn calculate_segment_duration(...)

// Some use C-style abbreviations
fn encode_ts_segment(...)  // ts = ?
```

**Recommendation**: Follow Rust naming conventions (RFC 430):
- `snake_case` for functions/variables
- `PascalCase` for types
- Full words preferred over abbreviations
- `TS` → `TransportStream` or `MpegTs`

---

### Issue 6.8: Unused Dependencies

**Severity**: Low
**Location**: `Cargo.toml`

**Potential unused deps** (need verification):
- `reqwest` - Only used if HLS needs to fetch remote segments
- `toml` - Only used if config files are implemented
- `regex` - Check if actually used

**Recommendation**: Audit dependencies:
```bash
cargo +nightly udeps  # Using cargo-udeps
```

---

### Issue 6.9: Feature Flag Inconsistency

**Severity**: Low
**Location**: `Cargo.toml`

**Current**:
```toml
[features]
default = ["rtmp", "flv", "hls"]
rtmp = []
flv = ["rtmp", "dep:axum", "dep:tower", ...]
hls = ["rtmp", "dep:axum", "dep:tower", ...]
```

**Problem**: 
- Features include dependencies but not all required code
- RTMP feature doesn't actually make RTMP optional
- HLS requires RTMP but shouldn't necessarily

**Recommendation**: Make features truly optional:
```toml
[features]
default = ["rtmp-server", "hls-server", "flv-server"]
rtmp-server = ["rtmp-protocol"]
hls-server = ["hls-protocol", "dep:axum"]
flv-server = ["flv-protocol", "dep:axum"]
```

---

### Issue 6.10: Binary Size Optimization

**Severity**: Low
**Location**: Release builds

**Current**: No size optimization settings

**Recommendation**: Add to `Cargo.toml`:

```toml
[profile.release]
opt-level = 3
lto = true          # Link-time optimization
codegen-units = 1   # Slower compile, better optimization
strip = true        # Strip debug symbols
panic = "abort"     # Smaller binary, no unwinding
```

Or use `strip` separately:
```bash
strip target/release/rslive-server
```

---

## Code Review Checklist

For new contributions, ensure:

- [ ] All public functions have rustdoc comments
- [ ] No `unwrap()` or `expect()` without justification
- [ ] All errors properly propagated with `?`
- [ ] No magic numbers (use named constants)
- [ ] Tests added for new functionality
- [ ] Clippy warnings fixed
- [ ] CHANGELOG.md updated

## Recommendations

1. **Enable CI/CD**: GitHub Actions for tests, clippy, fmt
2. **Code Coverage**: tarpaulin or codecov integration
3. **Documentation**: mdBook for user guide
4. **Benchmarks**: criterion for performance regression detection
5. **Fuzzing**: cargo-fuzz for protocol parsing
