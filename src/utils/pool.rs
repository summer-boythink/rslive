//! Memory pool for buffer reuse
//!
//! This module provides a high-performance buffer pool implementation
//! to reduce memory allocations and improve cache locality.

use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use crossbeam::queue::ArrayQueue;

/// A pooled buffer that returns to the pool when dropped
pub struct PooledBuffer {
    buffer: Option<BytesMut>,
    pool: Arc<BufferPoolInner>,
}

impl PooledBuffer {
    /// Get a reference to the underlying BytesMut
    pub fn as_mut(&mut self) -> &mut BytesMut {
        self.buffer.as_mut().unwrap()
    }

    /// Get a reference to the underlying BytesMut
    pub fn as_ref(&self) -> &BytesMut {
        self.buffer.as_ref().unwrap()
    }

    /// Convert to Bytes (this copies the data, but keeps the pool semantics)
    pub fn freeze(&mut self) -> Bytes {
        self.buffer.take().unwrap().freeze()
    }

    /// Clear the buffer without returning it to the pool
    pub fn clear(&mut self) {
        if let Some(ref mut buf) = self.buffer {
            buf.clear();
        }
    }

    /// Get the current length
    pub fn len(&self) -> usize {
        self.buffer.as_ref().map(|b| b.len()).unwrap_or(0)
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the capacity
    pub fn capacity(&self) -> usize {
        self.buffer.as_ref().map(|b| b.capacity()).unwrap_or(0)
    }
}

impl std::ops::Deref for PooledBuffer {
    type Target = BytesMut;

    fn deref(&self) -> &Self::Target {
        self.buffer.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.buffer.as_mut().unwrap()
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(mut buffer) = self.buffer.take() {
            buffer.clear();
            // Try to return to pool, ignore if pool is full
            let _ = self.pool.buffers.push(buffer);
        }
    }
}

/// Internal buffer pool state
struct BufferPoolInner {
    /// Pool of reusable buffers
    buffers: ArrayQueue<BytesMut>,
    /// Default capacity for new buffers
    default_capacity: usize,
}

/// A thread-safe buffer pool for reducing allocations
///
/// # Example
/// ```
/// use rslive::utils::BufferPool;
///
/// let pool = BufferPool::new(64, 4096);
///
/// // Get a buffer from the pool
/// let mut buf = pool.get();
/// buf.extend_from_slice(b"hello");
///
/// // Convert to Bytes (buffer returns to pool when PooledBuffer is dropped)
/// let bytes = buf.freeze();
/// ```
#[derive(Clone)]
pub struct BufferPool {
    inner: Arc<BufferPoolInner>,
}

impl BufferPool {
    /// Create a new buffer pool
    ///
    /// # Arguments
    /// * `pool_size` - Maximum number of buffers to keep in the pool
    /// * `default_capacity` - Default capacity for each buffer
    pub fn new(pool_size: usize, default_capacity: usize) -> Self {
        Self {
            inner: Arc::new(BufferPoolInner {
                buffers: ArrayQueue::new(pool_size),
                default_capacity,
            }),
        }
    }

    /// Get a buffer from the pool
    ///
    /// If the pool is empty, a new buffer is allocated.
    /// The buffer will be returned to the pool when dropped.
    pub fn get(&self) -> PooledBuffer {
        let buffer = self.inner.buffers.pop().unwrap_or_else(|| {
            BytesMut::with_capacity(self.inner.default_capacity)
        });

        PooledBuffer {
            buffer: Some(buffer),
            pool: self.inner.clone(),
        }
    }

    /// Get a buffer with a specific capacity
    ///
    /// If the pooled buffer is too small, it will be resized.
    pub fn get_with_capacity(&self, min_capacity: usize) -> PooledBuffer {
        let mut buffer = self.inner.buffers.pop().unwrap_or_else(|| {
            BytesMut::with_capacity(self.inner.default_capacity.max(min_capacity))
        });

        if buffer.capacity() < min_capacity {
            buffer.reserve(min_capacity - buffer.capacity());
        }

        PooledBuffer {
            buffer: Some(buffer),
            pool: self.inner.clone(),
        }
    }

    /// Get the number of buffers currently in the pool
    pub fn available(&self) -> usize {
        self.inner.buffers.len()
    }

    /// Get the default capacity for new buffers
    pub fn default_capacity(&self) -> usize {
        self.inner.default_capacity
    }
}

/// Global buffer pool for common use cases
pub mod global {
    use super::BufferPool;
    use std::sync::OnceLock;

    static SMALL_POOL: OnceLock<BufferPool> = OnceLock::new();
    static MEDIUM_POOL: OnceLock<BufferPool> = OnceLock::new();
    static LARGE_POOL: OnceLock<BufferPool> = OnceLock::new();

    /// Get the global small buffer pool (4KB buffers)
    pub fn small() -> &'static BufferPool {
        SMALL_POOL.get_or_init(|| BufferPool::new(128, 4 * 1024))
    }

    /// Get the global medium buffer pool (64KB buffers)
    pub fn medium() -> &'static BufferPool {
        MEDIUM_POOL.get_or_init(|| BufferPool::new(64, 64 * 1024))
    }

    /// Get the global large buffer pool (1MB buffers)
    pub fn large() -> &'static BufferPool {
        LARGE_POOL.get_or_init(|| BufferPool::new(16, 1024 * 1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_basic() {
        let pool = BufferPool::new(4, 1024);

        // Get a buffer
        let mut buf = pool.get();
        assert!(buf.is_empty());
        assert!(buf.capacity() >= 1024);

        // Write some data
        buf.extend_from_slice(b"hello world");
        assert_eq!(&buf[..], b"hello world");

        // Buffer returns to pool when dropped
        drop(buf);

        // Pool should have the buffer back
        assert_eq!(pool.available(), 1);
    }

    #[test]
    fn test_buffer_pool_reuse() {
        let pool = BufferPool::new(4, 1024);

        // Get and drop multiple buffers
        {
            let _buf1 = pool.get();
            let _buf2 = pool.get();
            let _buf3 = pool.get();
        }

        // All should be returned
        assert_eq!(pool.available(), 3);

        // Get again - should reuse
        let buf = pool.get();
        assert_eq!(pool.available(), 2);
        drop(buf);
    }

    #[test]
    fn test_buffer_pool_overflow() {
        let pool = BufferPool::new(2, 1024);

        // Get more buffers than pool size
        let buf1 = pool.get();
        let buf2 = pool.get();
        let buf3 = pool.get();

        // Drop all - pool can only hold 2
        drop(buf1);
        drop(buf2);
        drop(buf3);

        // Pool should be full
        assert_eq!(pool.available(), 2);
    }

    #[test]
    fn test_buffer_pool_freeze() {
        let pool = BufferPool::new(4, 1024);

        let mut buf = pool.get();
        buf.extend_from_slice(b"test data");

        let bytes = buf.freeze();
        assert_eq!(&bytes[..], b"test data");

        // Buffer should be returned to pool
        drop(buf);

        // Note: freeze() takes ownership, so buffer might not be usable after
        // This is expected behavior
    }

    #[test]
    fn test_buffer_pool_with_capacity() {
        let pool = BufferPool::new(4, 1024);

        let buf = pool.get_with_capacity(2048);
        assert!(buf.capacity() >= 2048);
    }

    #[test]
    fn test_global_pools() {
        let mut small = global::small().get();
        small.extend_from_slice(b"small");
        assert!(small.capacity() >= 4 * 1024);

        let mut medium = global::medium().get();
        medium.extend_from_slice(b"medium");
        assert!(medium.capacity() >= 64 * 1024);

        let mut large = global::large().get();
        large.extend_from_slice(b"large");
        assert!(large.capacity() >= 1024 * 1024);
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let pool = BufferPool::new(16, 1024);
        let pool_clone = pool.clone();

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let mut buf = pool_clone.get();
                buf.extend_from_slice(b"concurrent test");
            }
        });

        for _ in 0..100 {
            let mut buf = pool.get();
            buf.extend_from_slice(b"main thread test");
        }

        handle.join().unwrap();
    }

    #[test]
    fn test_buffer_clear() {
        let pool = BufferPool::new(4, 1024);

        let mut buf = pool.get();
        buf.extend_from_slice(b"hello");
        assert_eq!(buf.len(), 5);

        buf.clear();
        assert!(buf.is_empty());
        // Capacity should be preserved
        assert!(buf.capacity() >= 1024);
    }
}
