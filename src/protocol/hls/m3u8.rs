//! M3U8 playlist generation and parsing

use super::{HlsConfig, HlsError, HlsResult};
use std::fmt::Write;
use std::time::Duration;

/// Playlist type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistType {
    /// Live stream (sliding window)
    Live,
    /// Event stream (growing playlist, cannot seek)
    Event,
    /// Video on Demand (complete playlist)
    Vod,
}

impl PlaylistType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaylistType::Live => "LIVE",
            PlaylistType::Event => "EVENT",
            PlaylistType::Vod => "VOD",
        }
    }
}

/// Media playlist (variant playlist)
#[derive(Debug, Clone)]
pub struct MediaPlaylist {
    /// Target duration for segments
    pub target_duration: f64,
    /// Playlist type
    pub playlist_type: Option<PlaylistType>,
    /// Media sequence number
    pub media_sequence: u64,
    /// Discontinuity sequence
    pub discontinuity_sequence: u64,
    /// Whether playlist is ending
    pub end_list: bool,
    /// Allow cache directive
    pub allow_cache: Option<bool>,
    /// Playlist version
    pub version: u8,
    /// Segments
    pub segments: Vec<SegmentEntry>,
    /// If this is a Low-Latency HLS playlist
    pub low_latency: bool,
    /// Server control directives for LL-HLS
    pub server_control: Option<ServerControl>,
    /// Part information for LL-HLS
    pub parts: Vec<PartInfo>,
    /// Preload hint for LL-HLS
    pub preload_hint: Option<PreloadHint>,
}

impl MediaPlaylist {
    pub fn new(target_duration: Duration) -> Self {
        Self {
            target_duration: target_duration.as_secs_f64(),
            playlist_type: None,
            media_sequence: 0,
            discontinuity_sequence: 0,
            end_list: false,
            allow_cache: None,
            version: 3,
            segments: Vec::new(),
            low_latency: false,
            server_control: None,
            parts: Vec::new(),
            preload_hint: None,
        }
    }

    pub fn for_low_latency(target_duration: Duration) -> Self {
        Self {
            target_duration: target_duration.as_secs_f64(),
            playlist_type: Some(PlaylistType::Live),
            media_sequence: 0,
            discontinuity_sequence: 0,
            end_list: false,
            allow_cache: Some(false),
            version: 6, // LL-HLS requires version 6
            segments: Vec::new(),
            low_latency: true,
            server_control: Some(ServerControl {
                can_block_reload: true,
                hold_back: None,
                part_hold_back: None,
                can_skip_until: None,
            }),
            parts: Vec::new(),
            preload_hint: None,
        }
    }

    pub fn add_segment(&mut self, segment: SegmentEntry) {
        self.segments.push(segment);
    }

    pub fn add_partial_segment(&mut self, part: PartInfo) {
        self.parts.push(part);
    }

    pub fn set_preload_hint(&mut self, hint: PreloadHint) {
        self.preload_hint = Some(hint);
    }

    pub fn set_server_control(&mut self, control: ServerControl) {
        self.server_control = Some(control);
    }

    /// Remove old segments to maintain sliding window
    pub fn trim_segments(&mut self, max_count: usize) {
        while self.segments.len() > max_count {
            self.segments.remove(0);
            self.media_sequence += 1;
        }
    }

    /// Generate M3U8 content
    pub fn to_string(&self) -> String {
        let mut output = String::with_capacity(4096);

        // Header
        writeln!(output, "#EXTM3U").unwrap();
        writeln!(output, "#EXT-X-VERSION:{}", self.version).unwrap();
        writeln!(
            output,
            "#EXT-X-TARGETDURATION:{}",
            self.target_duration.ceil() as u64
        )
        .unwrap();
        writeln!(output, "#EXT-X-MEDIA-SEQUENCE:{}", self.media_sequence).unwrap();

        if self.discontinuity_sequence > 0 {
            writeln!(
                output,
                "#EXT-X-DISCONTINUITY-SEQUENCE:{}",
                self.discontinuity_sequence
            )
            .unwrap();
        }

        if let Some(playlist_type) = self.playlist_type {
            writeln!(output, "#EXT-X-PLAYLIST-TYPE:{}", playlist_type.as_str()).unwrap();
        }

        if let Some(allow_cache) = self.allow_cache {
            writeln!(
                output,
                "#EXT-X-ALLOW-CACHE:{}",
                if allow_cache { "YES" } else { "NO" }
            )
            .unwrap();
        }

        // Server control for LL-HLS
        if let Some(ref control) = self.server_control {
            write!(output, "#EXT-X-SERVER-CONTROL:").unwrap();
            if control.can_block_reload {
                write!(output, "CAN-BLOCK-RELOAD=YES").unwrap();
            }
            if let Some(hold_back) = control.hold_back {
                write!(output, ",HOLD-BACK={:.3}", hold_back).unwrap();
            }
            if let Some(part_hold_back) = control.part_hold_back {
                write!(output, ",PART-HOLD-BACK={:.3}", part_hold_back).unwrap();
            }
            if let Some(skip_until) = control.can_skip_until {
                write!(output, ",CAN-SKIP-UNTIL={:.3}", skip_until).unwrap();
            }
            writeln!(output).unwrap();
        }

        // Part information for LL-HLS
        if self.low_latency {
            for part in &self.parts {
                writeln!(
                    output,
                    "#EXT-X-PART:DURATION={:.3},URI=\"{}\"{}",
                    part.duration,
                    part.uri,
                    if part.independent { ",INDEPENDENT=YES" } else { "" }
                )
                .unwrap();
            }
        }

        // Segments
        for segment in &self.segments {
            if let Some(ref byterange) = segment.byterange {
                if let Some(offset) = byterange.offset {
                    writeln!(
                        output,
                        "#EXT-X-BYTERANGE:{}@{}",
                        byterange.length, offset
                    )
                    .unwrap();
                } else {
                    writeln!(output, "#EXT-X-BYTERANGE:{}", byterange.length).unwrap();
                }
            }

            if segment.discontinuity {
                writeln!(output, "#EXT-X-DISCONTINUITY").unwrap();
            }

            if let Some(program_date_time) = segment.program_date_time {
                writeln!(
                    output,
                    "#EXT-X-PROGRAM-DATE-TIME:{}",
                    program_date_time.to_rfc3339()
                )
                .unwrap();
            }

            writeln!(
                output,
                "#EXTINF:{:.3},\n{}",
                segment.duration, segment.uri
            )
            .unwrap();
        }

        // Preload hint for LL-HLS
        if let Some(ref hint) = self.preload_hint {
            writeln!(
                output,
                "#EXT-X-PRELOAD-HINT:TYPE={},URI=\"{}\"",
                hint.segment_type, hint.uri
            )
            .unwrap();
        }

        if self.end_list {
            writeln!(output, "#EXT-X-ENDLIST").unwrap();
        }

        output
    }
}

/// Segment entry in playlist
#[derive(Debug, Clone)]
pub struct SegmentEntry {
    /// Segment duration in seconds
    pub duration: f64,
    /// Segment URI
    pub uri: String,
    /// Byte range (for byte-range addressing)
    pub byterange: Option<Byterange>,
    /// Discontinuity flag
    pub discontinuity: bool,
    /// Program date time
    pub program_date_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Map information (for fMP4)
    pub map: Option<MapInfo>,
}

impl SegmentEntry {
    pub fn new(duration: f64, uri: impl Into<String>) -> Self {
        Self {
            duration,
            uri: uri.into(),
            byterange: None,
            discontinuity: false,
            program_date_time: None,
            map: None,
        }
    }

    pub fn with_byterange(mut self, length: u64, offset: Option<u64>) -> Self {
        self.byterange = Some(Byterange { length, offset });
        self
    }

    pub fn with_program_date_time(mut self, dt: chrono::DateTime<chrono::Utc>) -> Self {
        self.program_date_time = Some(dt);
        self
    }
}

/// Byte range specification
#[derive(Debug, Clone)]
pub struct Byterange {
    pub length: u64,
    pub offset: Option<u64>,
}

/// Map info for fMP4 initialization segment
#[derive(Debug, Clone)]
pub struct MapInfo {
    pub uri: String,
    pub byterange: Option<Byterange>,
}

/// Partial segment info for LL-HLS
#[derive(Debug, Clone)]
pub struct PartInfo {
    pub duration: f64,
    pub uri: String,
    pub independent: bool,
}

/// Preload hint for LL-HLS
#[derive(Debug, Clone)]
pub struct PreloadHint {
    pub segment_type: String,
    pub uri: String,
}

/// Server control directives
#[derive(Debug, Clone)]
pub struct ServerControl {
    pub can_block_reload: bool,
    pub hold_back: Option<f64>,
    pub part_hold_back: Option<f64>,
    pub can_skip_until: Option<f64>,
}

/// Master playlist (multi-variant)
#[derive(Debug, Clone)]
pub struct MasterPlaylist {
    pub variants: Vec<Variant>,
    pub version: u8,
}

impl MasterPlaylist {
    pub fn new() -> Self {
        Self {
            variants: Vec::new(),
            version: 4,
        }
    }

    pub fn add_variant(&mut self, variant: Variant) {
        self.variants.push(variant);
    }

    pub fn to_string(&self) -> String {
        let mut output = String::with_capacity(1024);

        writeln!(output, "#EXTM3U").unwrap();
        writeln!(output, "#EXT-X-VERSION:{}", self.version).unwrap();

        for variant in &self.variants {
            // Resolution info
            if let Some((width, height)) = variant.resolution {
                write!(
                    output,
                    "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}",
                    variant.bandwidth, width, height
                )
                .unwrap();
            } else {
                write!(output, "#EXT-X-STREAM-INF:BANDWIDTH={}", variant.bandwidth).unwrap();
            }

            // Frame rate
            if let Some(fps) = variant.frame_rate {
                write!(output, ",FRAME-RATE={:.3}", fps).unwrap();
            }

            // Codecs
            if let Some(ref codecs) = variant.codecs {
                write!(output, ",CODECS=\"{}\"", codecs).unwrap();
            }

            writeln!(output).unwrap();
            writeln!(output, "{}", variant.uri).unwrap();
        }

        output
    }
}

impl Default for MasterPlaylist {
    fn default() -> Self {
        Self::new()
    }
}

/// Variant stream definition
#[derive(Debug, Clone)]
pub struct Variant {
    /// Bandwidth in bits per second
    pub bandwidth: u64,
    /// Variant URI
    pub uri: String,
    /// Resolution (width, height)
    pub resolution: Option<(u32, u32)>,
    /// Frame rate
    pub frame_rate: Option<f64>,
    /// Codecs string
    pub codecs: Option<String>,
}

impl Variant {
    pub fn new(bandwidth: u64, uri: impl Into<String>) -> Self {
        Self {
            bandwidth,
            uri: uri.into(),
            resolution: None,
            frame_rate: None,
            codecs: None,
        }
    }

    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.resolution = Some((width, height));
        self
    }

    pub fn with_frame_rate(mut self, fps: f64) -> Self {
        self.frame_rate = Some(fps);
        self
    }

    pub fn with_codecs(mut self, codecs: impl Into<String>) -> Self {
        self.codecs = Some(codecs.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_playlist() {
        let mut playlist = MediaPlaylist::new(Duration::from_secs(6));
        playlist.playlist_type = Some(PlaylistType::Live);

        playlist.add_segment(SegmentEntry::new(6.0, "segment0.ts"));
        playlist.add_segment(SegmentEntry::new(6.0, "segment1.ts"));
        playlist.add_segment(SegmentEntry::new(6.0, "segment2.ts"));

        let output = playlist.to_string();
        assert!(output.contains("#EXTM3U"));
        assert!(output.contains("#EXT-X-TARGETDURATION:6"));
        assert!(output.contains("segment0.ts"));
    }

    #[test]
    fn test_low_latency_playlist() {
        let mut playlist = MediaPlaylist::for_low_latency(Duration::from_secs(4));

        // Add partial segments
        playlist.add_partial_segment(PartInfo {
            duration: 0.2,
            uri: "segment3_p0.m4s".to_string(),
            independent: true,
        });
        playlist.add_partial_segment(PartInfo {
            duration: 0.2,
            uri: "segment3_p1.m4s".to_string(),
            independent: false,
        });

        playlist.set_preload_hint(PreloadHint {
            segment_type: "PART".to_string(),
            uri: "segment3_p2.m4s".to_string(),
        });

        let output = playlist.to_string();
        assert!(output.contains("#EXT-X-SERVER-CONTROL"));
        assert!(output.contains("CAN-BLOCK-RELOAD=YES"));
        assert!(output.contains("#EXT-X-PART:"));
        assert!(output.contains("#EXT-X-PRELOAD-HINT:"));
    }

    #[test]
    fn test_master_playlist() {
        let mut master = MasterPlaylist::new();

        master.add_variant(
            Variant::new(2_000_000, "low/index.m3u8")
                .with_resolution(640, 360)
                .with_frame_rate(30.0)
                .with_codecs("avc1.42e00a,mp4a.40.2"),
        );

        master.add_variant(
            Variant::new(4_000_000, "high/index.m3u8")
                .with_resolution(1280, 720)
                .with_frame_rate(60.0)
                .with_codecs("avc1.640020,mp4a.40.2"),
        );

        let output = master.to_string();
        assert!(output.contains("#EXTM3U"));
        assert!(output.contains("BANDWIDTH=2000000"));
        assert!(output.contains("RESOLUTION=640x360"));
        assert!(output.contains("low/index.m3u8"));
    }
}
