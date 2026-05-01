//! HLS packager for converting MediaFrames to HLS segments and playlists

use super::{
    HlsResult,
    m3u8::{MediaPlaylist, PartInfo, PreloadHint, SegmentEntry, ServerControl},
    segment::{Segment, SegmentFormat, SegmentStorage},
};
use crate::media::{MediaFrame, StreamId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;

/// Packager configuration
#[derive(Debug, Clone)]
pub struct PackagerConfig {
    /// Target segment duration
    pub target_duration: Duration,
    /// Number of segments to keep in playlist
    pub playlist_size: usize,
    /// Enable Low-Latency HLS
    pub low_latency: bool,
    /// Partial segment duration for LL-HLS
    pub partial_segment_duration: Duration,
    /// Segment format
    pub segment_format: SegmentFormat,
}

impl Default for PackagerConfig {
    fn default() -> Self {
        Self {
            target_duration: Duration::from_secs(6),
            playlist_size: 6,
            low_latency: false,
            partial_segment_duration: Duration::from_millis(200),
            segment_format: SegmentFormat::MpegTs,
        }
    }
}

impl PackagerConfig {
    pub fn for_low_latency() -> Self {
        Self {
            target_duration: Duration::from_secs(4),
            playlist_size: 12,
            low_latency: true,
            partial_segment_duration: Duration::from_millis(200),
            segment_format: SegmentFormat::Fmp4,
        }
    }
}

/// HLS packager state
#[derive(Debug)]
struct PackagerState {
    /// Current segment being built
    current_segment: Vec<MediaFrame>,
    /// Current partial segment for LL-HLS
    current_partial: Vec<MediaFrame>,
    /// Segment counter
    segment_index: u64,
    /// Partial segment counter
    partial_index: u64,
    /// Last keyframe position
    last_keyframe_index: Option<usize>,
    /// Media playlist
    playlist: MediaPlaylist,
}

impl PackagerState {
    fn new(config: &PackagerConfig) -> Self {
        Self {
            current_segment: Vec::new(),
            current_partial: Vec::new(),
            segment_index: 0,
            partial_index: 0,
            last_keyframe_index: None,
            playlist: if config.low_latency {
                MediaPlaylist::for_low_latency(config.target_duration)
            } else {
                MediaPlaylist::new(config.target_duration)
            },
        }
    }

    fn segment_duration(&self) -> Duration {
        if self.current_segment.is_empty() {
            return Duration::ZERO;
        }

        let first_pts = self.current_segment[0].pts;
        let last_pts = self.current_segment[self.current_segment.len() - 1].pts;
        last_pts.duration_since(first_pts)
    }

    fn partial_duration(&self) -> Duration {
        if self.current_partial.is_empty() {
            return Duration::ZERO;
        }

        let first_pts = self.current_partial[0].pts;
        let last_pts = self.current_partial[self.current_partial.len() - 1].pts;
        last_pts.duration_since(first_pts)
    }
}

/// HLS packager for generating segments and playlists
pub struct HlsPackager {
    stream_id: StreamId,
    config: PackagerConfig,
    state: RwLock<PackagerState>,
    storage: Arc<dyn SegmentStorage>,
}

impl HlsPackager {
    pub fn new(
        stream_id: StreamId,
        config: PackagerConfig,
        storage: Arc<dyn SegmentStorage>,
    ) -> Self {
        let state = PackagerState::new(&config);

        Self {
            stream_id,
            config,
            state: RwLock::new(state),
            storage,
        }
    }

    /// Process a media frame
    pub async fn process_frame(&self, frame: MediaFrame) -> HlsResult<()> {
        let mut state = self.state.write().await;

        // Add frame to current segment
        state.current_segment.push(frame.share());

        // Track keyframe positions
        if frame.is_keyframe() {
            state.last_keyframe_index = Some(state.current_segment.len() - 1);
        }

        // Check if we should finalize the segment
        if self.should_finalize_segment(&state) {
            self.finalize_segment(&mut state).await?;
        }

        // LL-HLS: Handle partial segments
        if self.config.low_latency {
            state.current_partial.push(frame);

            if state.partial_duration() >= self.config.partial_segment_duration {
                self.finalize_partial_segment(&mut state).await?;
            }
        }

        Ok(())
    }

    /// Check if segment should be finalized
    fn should_finalize_segment(&self, state: &PackagerState) -> bool {
        let duration = state.segment_duration();

        // Check if target duration is reached
        if duration >= self.config.target_duration {
            // For video segments, try to end on a keyframe
            if state.last_keyframe_index.is_some() {
                return true;
            }
            // If no keyframe found, allow slightly longer segments
            if duration >= self.config.target_duration + Duration::from_secs(2) {
                return true;
            }
        }

        false
    }

    /// Finalize current segment
    async fn finalize_segment(&self, state: &mut PackagerState) -> HlsResult<()> {
        if state.current_segment.is_empty() {
            return Ok(());
        }

        // Find a good split point (keyframe)
        let split_index = state
            .last_keyframe_index
            .unwrap_or(state.current_segment.len());

        // Take frames up to split point
        let frames: Vec<_> = state.current_segment.drain(..split_index).collect();

        if frames.is_empty() {
            return Ok(());
        }

        // Create segment
        let segment =
            Segment::from_frames(state.segment_index, &frames, self.config.segment_format)?;

        let segment_info = segment.info.clone();
        let segment_index = segment.info.index;

        // Store segment
        self.storage.store(&segment)?;

        // Add to playlist
        let entry = SegmentEntry::new(segment_info.duration.as_secs_f64(), segment_info.filename());
        state.playlist.add_segment(entry);

        // Trim playlist to maintain sliding window
        state.playlist.trim_segments(self.config.playlist_size);

        info!(
            stream_id = %self.stream_id.as_str(),
            segment_index = segment_index,
            duration = ?segment_info.duration,
            "Segment finalized"
        );

        state.segment_index += 1;

        // Update server control for LL-HLS
        if self.config.low_latency {
            let target_secs = self.config.target_duration.as_secs_f64();
            state.playlist.set_server_control(ServerControl {
                can_block_reload: true,
                hold_back: Some(target_secs * 3.0),
                part_hold_back: Some(target_secs * 0.6),
                can_skip_until: Some(target_secs * 6.0),
            });
        }

        Ok(())
    }

    /// Finalize partial segment (LL-HLS)
    async fn finalize_partial_segment(&self, state: &mut PackagerState) -> HlsResult<()> {
        if state.current_partial.is_empty() {
            return Ok(());
        }

        let frames: Vec<_> = state.current_partial.drain(..).collect();

        // Check if any frame is a keyframe
        let has_keyframe = frames.iter().any(|f| f.is_keyframe());

        // Create partial segment
        let part = PartInfo {
            duration: state.partial_duration().as_secs_f64(),
            uri: format!(
                "segment{}_p{}.m4s",
                state.segment_index, state.partial_index
            ),
            independent: has_keyframe,
        };

        state.playlist.add_partial_segment(part);

        // Keep only recent partials
        while state.playlist.parts.len() > 6 {
            state.playlist.parts.remove(0);
        }

        // Update preload hint
        state.playlist.set_preload_hint(PreloadHint {
            segment_type: "PART".to_string(),
            uri: format!(
                "segment{}_p{}.m4s",
                state.segment_index,
                state.partial_index + 1
            ),
        });

        state.partial_index += 1;

        Ok(())
    }

    /// Get current playlist
    pub async fn playlist(&self) -> MediaPlaylist {
        let state = self.state.read().await;
        state.playlist.clone()
    }

    /// Get playlist as string
    pub async fn playlist_string(&self) -> String {
        let playlist = self.playlist().await;
        playlist.to_string()
    }

    /// Get segment by index
    pub async fn get_segment(&self, index: u64) -> HlsResult<Option<Segment>> {
        self.storage.load(index)
    }

    /// Finalize all remaining data
    pub async fn finalize(&self) -> HlsResult<()> {
        let mut state = self.state.write().await;

        // Finalize current segment
        if !state.current_segment.is_empty() {
            let frames: Vec<_> = state.current_segment.drain(..).collect();

            let segment =
                Segment::from_frames(state.segment_index, &frames, self.config.segment_format)?;

            self.storage.store(&segment)?;

            let entry =
                SegmentEntry::new(segment.info.duration.as_secs_f64(), segment.info.filename());
            state.playlist.add_segment(entry);
        }

        // Mark playlist as ended for VOD
        state.playlist.end_list = true;

        Ok(())
    }
}

/// HLS packager manager for multiple streams
pub struct HlsPackagerManager {
    config: PackagerConfig,
    storage: Arc<dyn SegmentStorage>,
    packagers: dashmap::DashMap<StreamId, Arc<HlsPackager>>,
}

impl HlsPackagerManager {
    pub fn new(config: PackagerConfig, storage: Arc<dyn SegmentStorage>) -> Self {
        Self {
            config,
            storage,
            packagers: dashmap::DashMap::new(),
        }
    }

    /// Create packager for stream
    pub fn create_packager(&self, stream_id: StreamId) -> Arc<HlsPackager> {
        let packager = Arc::new(HlsPackager::new(
            stream_id.clone(),
            self.config.clone(),
            Arc::clone(&self.storage),
        ));

        self.packagers.insert(stream_id, Arc::clone(&packager));
        packager
    }

    /// Get packager for stream
    pub fn get_packager(&self, stream_id: &StreamId) -> Option<Arc<HlsPackager>> {
        self.packagers.get(stream_id).map(|p| Arc::clone(p.value()))
    }

    /// Remove packager
    pub async fn remove_packager(&self, stream_id: &StreamId) -> HlsResult<()> {
        if let Some((_, packager)) = self.packagers.remove(stream_id) {
            packager.finalize().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{CodecType, Timestamp, VideoFrameType};
    use bytes::Bytes;

    #[tokio::test]
    async fn test_packager() {
        let config = PackagerConfig::default();
        let storage: Arc<dyn SegmentStorage> =
            Arc::new(super::super::segment::MemorySegmentStorage::new(100));

        let packager = HlsPackager::new(StreamId::new("test"), config, storage);

        // Add frames
        for i in 0..100 {
            let frame = MediaFrame::video(
                1,
                Timestamp::from_millis(i as u64 * 100),
                if i % 30 == 0 {
                    VideoFrameType::Keyframe
                } else {
                    VideoFrameType::Interframe
                },
                CodecType::H264,
                Bytes::from(vec![0; 1000]),
            );

            packager.process_frame(frame).await.unwrap();
        }

        // Get playlist
        let playlist = packager.playlist().await;
        assert!(!playlist.segments.is_empty());
    }
}
