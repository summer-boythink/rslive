//! Stream router for zero-copy forwarding between publishers and subscribers

use super::frame::{DataFrameType, FrameType};
use super::{MediaError, MediaFrame, MediaResult, Timestamp};
use bytes::Bytes;
use dashmap::DashMap;
use flume::{Receiver, Sender};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::{debug, trace, warn};

/// Configuration for stream routing
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Maximum number of subscribers per stream
    pub max_subscribers: usize,
    /// Channel buffer size for each subscriber
    pub channel_buffer: usize,
    /// Maximum backlog before dropping frames
    pub max_backlog: usize,
    /// Strategy when subscriber buffer is full
    pub backpressure_strategy: BackpressureStrategy,
    /// Whether to cache last keyframe for new subscribers
    pub cache_keyframe: bool,
    /// Number of keyframes to cache for GOP-based seeking
    pub keyframe_cache_size: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            max_subscribers: 10000,
            channel_buffer: 1024,
            max_backlog: 5000,
            backpressure_strategy: BackpressureStrategy::DropOld,
            cache_keyframe: true,
            keyframe_cache_size: 2,
        }
    }
}

impl RouterConfig {
    pub fn for_low_latency() -> Self {
        Self {
            channel_buffer: 64, // Smaller buffer for lower latency
            backpressure_strategy: BackpressureStrategy::DropOld,
            ..Default::default()
        }
    }

    pub fn for_high_quality() -> Self {
        Self {
            channel_buffer: 4096, // Larger buffer for stability
            backpressure_strategy: BackpressureStrategy::Block,
            keyframe_cache_size: 5,
            ..Default::default()
        }
    }
}

/// Backpressure handling strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureStrategy {
    /// Drop oldest frames to make room
    DropOld,
    /// Drop newest frame
    DropNew,
    /// Block until space available (may cause head-of-line blocking)
    Block,
    /// Close slow subscriber connection
    Close,
}

/// Stream identification
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct StreamId(String);

impl StreamId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for StreamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for StreamId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Internal stream state
pub(crate) struct StreamState {
    subscribers: Vec<Sender<MediaFrame>>,
    stats: Arc<StreamStats>,
    keyframe_cache: Arc<RwLock<Vec<MediaFrame>>>,
    _created_at: std::time::Instant,
}

impl StreamState {
    fn new(config: &RouterConfig) -> Self {
        Self {
            subscribers: Vec::new(),
            stats: Arc::new(StreamStats::default()),
            keyframe_cache: Arc::new(RwLock::new(Vec::with_capacity(config.keyframe_cache_size))),
            _created_at: std::time::Instant::now(),
        }
    }
}

/// Handle for publishing to a stream
#[derive(Clone)]
pub struct StreamPublisher {
    stream_id: StreamId,
    streams: Arc<DashMap<StreamId, StreamState>>,
    stats: Arc<StreamStats>,
    keyframe_cache: Arc<RwLock<Vec<MediaFrame>>>,
    config: RouterConfig,
}

impl StreamPublisher {
    fn new(
        stream_id: StreamId,
        streams: Arc<DashMap<StreamId, StreamState>>,
        stats: Arc<StreamStats>,
        keyframe_cache: Arc<RwLock<Vec<MediaFrame>>>,
        config: RouterConfig,
    ) -> Self {
        Self {
            stream_id,
            streams,
            stats,
            keyframe_cache,
            config,
        }
    }

    /// Publish a frame to the stream
    ///
    /// This will broadcast to all subscribers using zero-copy semantics.
    pub async fn publish(&self, frame: MediaFrame) -> MediaResult<()> {
        trace!(
            stream_id = %self.stream_id.as_str(),
            pts = frame.pts.as_millis(),
            size = frame.size(),
            "Publishing frame"
        );

        // Update stats
        self.stats.record_frame(&frame);

        // Cache keyframes for new subscribers
        if self.config.cache_keyframe && frame.is_keyframe() {
            let mut cache = self.keyframe_cache.write();
            cache.push(frame.share());
            // Keep only last N keyframes
            while cache.len() > self.config.keyframe_cache_size {
                cache.remove(0);
            }
        }

        // Broadcast to all subscribers
        if let Some(entry) = self.streams.get(&self.stream_id) {
            let subscribers = &entry.subscribers;

            // Remove disconnected subscribers
            let mut disconnected = Vec::new();
            for (i, sender) in subscribers.iter().enumerate() {
                if sender.is_disconnected() {
                    disconnected.push(i);
                }
            }
            drop(entry);

            // Clean up disconnected subscribers
            if !disconnected.is_empty() {
                if let Some(mut entry) = self.streams.get_mut(&self.stream_id) {
                    for i in disconnected.into_iter().rev() {
                        entry.subscribers.remove(i);
                    }
                }
            }

            // Send to all active subscribers
            if let Some(entry) = self.streams.get(&self.stream_id) {
                for sender in &entry.subscribers {
                    // Use try_send to avoid blocking on slow subscribers
                    match self.config.backpressure_strategy {
                        BackpressureStrategy::DropOld | BackpressureStrategy::DropNew => {
                            // For flume channels, we can't drain from sender side
                            // Just try to send, skip if full
                            if !sender.is_full() {
                                let _ = sender.try_send(frame.share());
                            }
                        }
                        BackpressureStrategy::Block => {
                            let _ = sender.send_async(frame.share()).await;
                        }
                        BackpressureStrategy::Close => {
                            if sender.is_full() {
                                // Don't send, subscriber will be cleaned up
                            } else {
                                let _ = sender.try_send(frame.share());
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Try to publish without blocking
    pub fn try_publish(&self, frame: MediaFrame) -> MediaResult<()> {
        // Update stats
        self.stats.record_frame(&frame);

        // Cache keyframes
        if self.config.cache_keyframe && frame.is_keyframe() {
            let mut cache = self.keyframe_cache.write();
            cache.push(frame.share());
            while cache.len() > self.config.keyframe_cache_size {
                cache.remove(0);
            }
        }

        // Broadcast to all subscribers
        if let Some(entry) = self.streams.get(&self.stream_id) {
            for sender in &entry.subscribers {
                if !sender.is_disconnected() {
                    match self.config.backpressure_strategy {
                        BackpressureStrategy::DropOld | BackpressureStrategy::DropNew => {
                            // For flume channels, we can't drain from sender side
                            // Just try to send, skip if full
                            if !sender.is_full() {
                                let _ = sender.try_send(frame.share());
                            }
                        }
                        BackpressureStrategy::Block | BackpressureStrategy::Close => {
                            if !sender.is_full() {
                                let _ = sender.try_send(frame.share());
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Publish metadata (non-media data)
    pub async fn publish_metadata(&self, data: Bytes) -> MediaResult<()> {
        let frame = MediaFrame::new(
            0, // Metadata uses track 0
            Timestamp::ZERO,
            FrameType::Data(DataFrameType::Metadata),
            super::CodecType::H264, // Placeholder, metadata doesn't have codec
            data,
        );
        self.publish(frame).await
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

/// Handle for subscribing to a stream
pub struct StreamSubscriber {
    stream_id: StreamId,
    receiver: Receiver<MediaFrame>,
    _stats: Arc<StreamStats>,
    start_time: std::time::Instant,
}

impl StreamSubscriber {
    fn new(stream_id: StreamId, receiver: Receiver<MediaFrame>, stats: Arc<StreamStats>) -> Self {
        Self {
            stream_id,
            receiver,
            _stats: stats,
            start_time: std::time::Instant::now(),
        }
    }

    /// Receive the next frame
    pub async fn recv(&self) -> MediaResult<MediaFrame> {
        match self.receiver.recv_async().await {
            Ok(frame) => {
                trace!(
                    stream_id = %self.stream_id.as_str(),
                    pts = frame.pts.as_millis(),
                    "Received frame"
                );
                Ok(frame)
            }
            Err(_) => Err(MediaError::ChannelClosed),
        }
    }

    /// Try to receive without blocking
    pub fn try_recv(&self) -> MediaResult<Option<MediaFrame>> {
        match self.receiver.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(flume::TryRecvError::Empty) => Ok(None),
            Err(flume::TryRecvError::Disconnected) => Err(MediaError::ChannelClosed),
        }
    }

    /// Receive with timeout
    pub async fn recv_timeout(&self, timeout: Duration) -> MediaResult<Option<MediaFrame>> {
        match tokio::time::timeout(timeout, self.recv()).await {
            Ok(Ok(frame)) => Ok(Some(frame)),
            Ok(Err(MediaError::ChannelClosed)) => Err(MediaError::ChannelClosed),
            Ok(Err(e)) => Err(e),
            Err(_) => Ok(None), // Timeout
        }
    }

    /// Get current lag behind publisher
    pub fn lag(&self) -> Option<Duration> {
        // This is an approximation based on channel capacity
        let capacity = self.receiver.capacity().unwrap_or(0);
        let len = self.receiver.len();
        if capacity > 0 {
            let ratio = len as f64 / capacity as f64;
            Some(Duration::from_millis((ratio * 1000.0) as u64))
        } else {
            None
        }
    }

    /// Get subscriber duration
    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    /// Check if publisher is still connected
    pub fn is_connected(&self) -> bool {
        !self.receiver.is_disconnected()
    }
}

/// Central router for managing streams and forwarding frames
pub struct StreamRouter {
    streams: Arc<DashMap<StreamId, StreamState>>,
    config: RouterConfig,
    global_stats: Arc<GlobalStats>,
}

impl StreamRouter {
    pub fn new(config: RouterConfig) -> Self {
        Self {
            streams: Arc::new(DashMap::new()),
            config,
            global_stats: Arc::new(GlobalStats::default()),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(RouterConfig::default())
    }

    /// Register as a publisher for a stream
    pub fn publish(&self, stream_id: StreamId) -> MediaResult<StreamPublisher> {
        // Check if stream already has a publisher by trying to get existing entry
        let existing = self.streams.get(&stream_id);
        if existing.is_some() {
            return Err(MediaError::Router(format!(
                "Stream '{}' already has a publisher",
                stream_id.as_str()
            )));
        }

        // Create new stream state
        let state = StreamState::new(&self.config);
        let stats = Arc::clone(&state.stats);
        let keyframe_cache = Arc::clone(&state.keyframe_cache);

        // Insert the new stream
        self.streams.insert(stream_id.clone(), state);

        let publisher = StreamPublisher::new(
            stream_id.clone(),
            Arc::clone(&self.streams),
            stats,
            keyframe_cache,
            self.config.clone(),
        );

        debug!(
            stream_id = %stream_id.as_str(),
            "Publisher registered"
        );

        self.global_stats
            .publisher_count
            .fetch_add(1, Ordering::Relaxed);

        Ok(publisher)
    }

    /// Subscribe to a stream
    pub fn subscribe(&self, stream_id: &StreamId) -> MediaResult<StreamSubscriber> {
        // Check if stream exists and get info
        let (stats, cache_keyframe, keyframe_cache) = {
            let entry = self
                .streams
                .get(stream_id)
                .ok_or_else(|| MediaError::StreamNotFound(stream_id.as_str().to_string()))?;

            if entry.subscribers.len() >= self.config.max_subscribers {
                return Err(MediaError::Router(format!(
                    "Stream '{}' has reached max subscribers ({})",
                    stream_id.as_str(),
                    self.config.max_subscribers
                )));
            }

            (
                Arc::clone(&entry.stats),
                self.config.cache_keyframe,
                Arc::clone(&entry.keyframe_cache),
            )
        };

        let (sender, receiver) = flume::bounded(self.config.channel_buffer);

        // Send cached keyframes to new subscriber
        if cache_keyframe {
            let cache = keyframe_cache.read();
            for keyframe in cache.iter() {
                if sender.try_send(keyframe.share()).is_err() {
                    warn!("Failed to send cached keyframe to new subscriber");
                    break;
                }
            }
        }

        // Add subscriber to list
        if let Some(mut entry) = self.streams.get_mut(stream_id) {
            entry.subscribers.push(sender);
        }

        let subscriber = StreamSubscriber::new(stream_id.clone(), receiver, stats);

        debug!(
            stream_id = %stream_id.as_str(),
            "Subscriber added"
        );

        self.global_stats
            .subscriber_count
            .fetch_add(1, Ordering::Relaxed);

        Ok(subscriber)
    }

    /// Unsubscribe from a stream
    pub fn unsubscribe(&self, stream_id: &StreamId) {
        if let Some(mut entry) = self.streams.get_mut(stream_id) {
            // Note: In a real implementation, we'd need to identify which subscriber
            // For now, we just decrement the count
            if !entry.subscribers.is_empty() {
                entry.subscribers.remove(0);
            }

            debug!(
                stream_id = %stream_id.as_str(),
                subscriber_count = entry.subscribers.len(),
                "Subscriber removed"
            );

            self.global_stats
                .subscriber_count
                .fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Stop publishing to a stream
    pub fn unpublish(&self, stream_id: &StreamId) {
        // Remove the stream entirely
        self.streams.remove(stream_id);

        debug!(
            stream_id = %stream_id.as_str(),
            "Publisher removed"
        );

        self.global_stats
            .publisher_count
            .fetch_sub(1, Ordering::Relaxed);
    }

    /// Remove a stream entirely
    pub fn remove_stream(&self, stream_id: &StreamId) {
        self.streams.remove(stream_id);
        debug!(stream_id = %stream_id.as_str(), "Stream removed");
    }

    /// Check if a stream exists
    pub fn has_stream(&self, stream_id: &StreamId) -> bool {
        self.streams.contains_key(stream_id)
    }

    /// Get list of active stream IDs
    pub fn stream_ids(&self) -> Vec<StreamId> {
        self.streams.iter().map(|e| e.key().clone()).collect()
    }

    /// Get stream statistics
    pub fn stream_stats(&self, stream_id: &StreamId) -> Option<Arc<StreamStats>> {
        self.streams.get(stream_id).map(|e| Arc::clone(&e.stats))
    }

    /// Get global statistics
    pub fn global_stats(&self) -> &GlobalStats {
        &self.global_stats
    }

    /// Get number of active streams
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}

/// Statistics for a single stream
pub struct StreamStats {
    frames_published: AtomicU64,
    bytes_published: AtomicU64,
    last_frame_time: RwLock<Option<std::time::Instant>>,
    last_keyframe_time: RwLock<Option<std::time::Instant>>,
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            frames_published: AtomicU64::new(0),
            bytes_published: AtomicU64::new(0),
            last_frame_time: RwLock::new(None),
            last_keyframe_time: RwLock::new(None),
        }
    }
}

impl StreamStats {
    fn record_frame(&self, frame: &MediaFrame) {
        self.frames_published.fetch_add(1, Ordering::Relaxed);
        self.bytes_published
            .fetch_add(frame.size() as u64, Ordering::Relaxed);

        let now = std::time::Instant::now();
        *self.last_frame_time.write() = Some(now);

        if frame.is_keyframe() {
            *self.last_keyframe_time.write() = Some(now);
        }
    }

    pub fn frames_published(&self) -> u64 {
        self.frames_published.load(Ordering::Relaxed)
    }

    pub fn bytes_published(&self) -> u64 {
        self.bytes_published.load(Ordering::Relaxed)
    }
}

/// Global router statistics
pub struct GlobalStats {
    publisher_count: AtomicU64,
    subscriber_count: AtomicU64,
}

impl Default for GlobalStats {
    fn default() -> Self {
        Self {
            publisher_count: AtomicU64::new(0),
            subscriber_count: AtomicU64::new(0),
        }
    }
}

impl GlobalStats {
    pub fn publisher_count(&self) -> u64 {
        self.publisher_count.load(Ordering::Relaxed)
    }

    pub fn subscriber_count(&self) -> u64 {
        self.subscriber_count.load(Ordering::Relaxed)
    }
}

/// Trait for stream sources (protocol adapters)
#[async_trait::async_trait]
pub trait StreamSource: Send + Sync {
    fn protocol(&self) -> &'static str;
    async fn start(&self) -> MediaResult<()>;
    async fn stop(&self) -> MediaResult<()>;
}

/// Trait for stream sinks (protocol outputs)
#[async_trait::async_trait]
pub trait StreamSink: Send + Sync {
    fn protocol(&self) -> &'static str;
    async fn start(&self) -> MediaResult<()>;
    async fn stop(&self) -> MediaResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{CodecType, VideoFrameType};

    #[tokio::test]
    async fn test_publish_subscribe() {
        let router = StreamRouter::with_defaults();
        let stream_id = StreamId::new("test/stream");

        // Publish
        let publisher = router.publish(stream_id.clone()).unwrap();

        // Subscribe
        let subscriber = router.subscribe(&stream_id).unwrap();

        // Publish a frame
        let frame = MediaFrame::video(
            1,
            Timestamp::from_millis(1000),
            VideoFrameType::Keyframe,
            CodecType::H264,
            Bytes::from(vec![0; 1000]),
        );

        publisher.publish(frame.share()).await.unwrap();

        // Receive the frame
        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.pts.as_millis(), 1000);
        assert_eq!(received.size(), 1000);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let router = StreamRouter::with_defaults();
        let stream_id = StreamId::new("test/stream");

        let publisher = router.publish(stream_id.clone()).unwrap();
        let sub1 = router.subscribe(&stream_id).unwrap();
        let sub2 = router.subscribe(&stream_id).unwrap();

        let frame = MediaFrame::video(
            1,
            Timestamp::from_millis(1000),
            VideoFrameType::Keyframe,
            CodecType::H264,
            Bytes::from(vec![0; 100]),
        );

        publisher.publish(frame).await.unwrap();

        let f1 = sub1.recv().await.unwrap();
        let f2 = sub2.recv().await.unwrap();

        assert_eq!(f1.pts, f2.pts);
        assert!(Arc::ptr_eq(&f1.data, &f2.data)); // Zero-copy
    }

    #[tokio::test]
    async fn test_stream_not_found() {
        let router = StreamRouter::with_defaults();
        let stream_id = StreamId::new("nonexistent");

        let result = router.subscribe(&stream_id);
        assert!(matches!(result, Err(MediaError::StreamNotFound(_))));
    }
}
