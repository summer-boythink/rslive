# 下一步开发计划

## 当前项目状态总览

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        rslive 模块完成度                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ✅ RTMP Server/Client    ████████████████████  100%                    │
│  ✅ AMF0/AMF3 编解码      ████████████████████  100%                    │
│  ✅ FLV Muxer/Demuxer     ████████████████████  100%                    │
│  ✅ HTTP-FLV Server       ████████████████████  100%                    │
│  ✅ Media Router          ████████████████████  100%                    │
│  ✅ BufferPool            ████████████████████  100%                    │
│  ⚠️  HLS M3U8             ████████████████████  100%                    │
│  ⚠️  HLS MPEG-TS          ████████░░░░░░░░░░░░   40% (基础框架有，需完善) │
│  ❌ HLS fMP4              ░░░░░░░░░░░░░░░░░░░░    0% (空实现)           │
│  ❌ Server Binary         ██░░░░░░░░░░░░░░░░░░   10% (仅 placeholder)   │
│  ❌ SRT                   ░░░░░░░░░░░░░░░░░░░░    0%                    │
│  ❌ WebRTC                ░░░░░░░░░░░░░░░░░░░░    0%                    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 优先级排序

| 优先级 | 任务 | 原因 | 预计时间 |
|--------|------|------|----------|
| P0 | 完善 MPEG-TS Muxer | HLS 功能的核心，当前实现不完整 | 3-4 天 |
| P0 | 实现 fMP4 Muxer | LL-HLS 必需，现代流媒体标准 | 3-4 天 |
| P1 | 集成服务器二进制 | 让 rslive-server 真正可用 | 2 天 |
| P1 | RTMP→HLS 转码管道 | 实现完整的协议转换链路 | 2 天 |
| P2 | 测试覆盖率提升 | 确保代码质量 | 2 天 |
| P2 | 性能基准测试验证 | 验证优化效果 | 1 天 |

---

## 任务 1: 完善 MPEG-TS Muxer (P0)

### 问题分析

当前 `src/protocol/hls/segment.rs` 中的 `encode_ts_segment()` 是简化实现：

```rust
// 当前问题:
// 1. PAT/PMT CRC32 是占位符 (全 0)
// 2. PES 包结构不完整，缺少必要的标志位
// 3. 没有处理 PCR (Program Clock Reference)
// 4. 没有处理 TS 包的 adaptation field
// 5. 连续性计数器未实现
// 6. TS 包固定 188 字节未实现
```

### 实现计划

#### 文件结构

```
src/protocol/hls/
├── mpegts/
│   ├── mod.rs           # 导出 + 常量定义
│   ├── pat.rs           # PAT (Program Association Table)
│   ├── pmt.rs           # PMT (Program Map Table)
│   ├── pes.rs           # PES (Packetized Elementary Stream)
│   ├── ts_packet.rs     # TS 包封装
│   └── muxer.rs         # TsMuxer 主入口
└── segment.rs           # 修改为调用 muxer
```

#### 核心结构设计

```rust
// src/protocol/hls/mpegts/mod.rs

/// MPEG-TS 常量
pub const TS_PACKET_SIZE: usize = 188;
pub const TS_SYNC_BYTE: u8 = 0x47;
pub const PAT_PID: u16 = 0x0000;
pub const PMT_PID: u16 = 0x1000;
pub const VIDEO_PID: u16 = 0x100;
pub const AUDIO_PID: u16 = 0x101;

/// TS Muxer 主入口
pub struct TsMuxer {
    config: TsMuxerConfig,
    pat_generator: PatGenerator,
    pmt_generator: PmtGenerator,
    pes_video: PesEncoder,
    pes_audio: PesEncoder,
    continuity_counter: ContinuityCounter,
    pcr_handler: PcrHandler,
}

/// 配置
pub struct TsMuxerConfig {
    pub video_pid: u16,
    pub audio_pid: u16,
    pub pcr_pid: u16,
    pub video_codec: CodecType,
    pub audio_codec: CodecType,
}
```

#### 步骤 1: TS 包封装 (ts_packet.rs)

```rust
/// TS 包头
#[derive(Debug, Clone)]
pub struct TsPacketHeader {
    pub sync_byte: u8,              // 0x47
    pub transport_error_indicator: bool,
    pub payload_unit_start_indicator: bool,
    pub transport_priority: bool,
    pub pid: u16,
    pub transport_scrambling_control: u8,
    pub adaptation_field_control: u8,  // 01=只有负载, 10=只有适配, 11=两者都有
    pub continuity_counter: u8,        // 0-15 循环
}

/// TS 包
pub struct TsPacket {
    header: TsPacketHeader,
    adaptation_field: Option<AdaptationField>,
    payload: Vec<u8>,
}

impl TsPacket {
    pub const SIZE: usize = 188;

    /// 编码为字节
    pub fn encode(&self) -> [u8; 188] {
        let mut buf = [0u8; 188];
        // ... 编码逻辑
        buf
    }
}

/// 适配字段 (用于 PCR 和填充)
pub struct AdaptationField {
    pub pcr: Option<PcrValue>,
    pub opcr: Option<PcrValue>,
    pub splice_countdown: Option<i8>,
    pub stuffing_bytes: usize,
}

/// PCR 值 (Program Clock Reference)
#[derive(Debug, Clone, Copy)]
pub struct PcrValue {
    pub base: u64,      // 33 bits, 90kHz
    pub extension: u16, // 9 bits, 27MHz
}
```

#### 步骤 2: PAT/PMT 实现 (pat.rs, pmt.rs)

```rust
// pat.rs

/// PAT 生成器
pub struct PatGenerator {
    table_id: u8,
    section_syntax_indicator: bool,
    transport_stream_id: u16,
    version_number: u8,
    current_next_indicator: bool,
    section_number: u8,
    last_section_number: u8,
    program_info: Vec<ProgramInfo>,
}

struct ProgramInfo {
    program_number: u16,
    pid: u16,  // PMT PID
}

impl PatGenerator {
    /// 生成 PAT TS 包
    pub fn generate(&self) -> Vec<TsPacket> {
        // 构建 PAT section
        // 计算 CRC32
        // 分成 188 字节的 TS 包
    }
}

// pmt.rs

/// PMT 生成器
pub struct PmtGenerator {
    program_number: u16,
    pcr_pid: u16,
    streams: Vec<StreamInfo>,
}

struct StreamInfo {
    stream_type: u8,      // 0x1B=H.264, 0x0F=AAC
    elementary_pid: u16,
    descriptors: Vec<u8>,
}

impl PmtGenerator {
    pub fn new(video_pid: u16, audio_pid: u16, video_codec: CodecType, audio_codec: CodecType) -> Self {
        // ...
    }

    pub fn generate(&self) -> Vec<TsPacket> {
        // 构建 PMT section
        // 计算 CRC32
        // 分成 TS 包
    }
}
```

#### 步骤 3: PES 编码 (pes.rs)

```rust
/// PES 编码器
pub struct PesEncoder {
    stream_id: u8,        // 0xE0=视频, 0xC0=音频
    stream_type: StreamType,
}

pub enum StreamType {
    VideoH264,
    VideoH265,
    AudioAac,
    AudioMp3,
}

impl PesEncoder {
    /// 将帧编码为 PES 包
    pub fn encode(&self, frame: &MediaFrame) -> PesPacket {
        let mut pes = PesPacket::new();

        // PES start code: 0x000001
        // Stream ID
        // PES packet length
        // Optional PES header (PTS/DTS)
        // Payload data

        pes
    }
}

/// PES 包
pub struct PesPacket {
    start_code: [u8; 3],      // 0x00 0x00 0x01
    stream_id: u8,
    packet_length: u16,
    optional_header: Option<PesOptionalHeader>,
    payload: Bytes,
}

pub struct PesOptionalHeader {
    pub pts_dts_flags: u8,
    pub pts: Option<u64>,     // 33 bits, 90kHz
    pub dts: Option<u64>,
    pub escr: Option<u64>,
}
```

#### 步骤 4: TsMuxer 组装 (muxer.rs)

```rust
/// TS Muxer
pub struct TsMuxer {
    config: TsMuxerConfig,
    pat: PatGenerator,
    pmt: PmtGenerator,
    continuity: HashMap<u16, u8>,  // PID -> counter
    pcr_interval: Duration,
    last_pcr_time: Option<Timestamp>,
}

impl TsMuxer {
    pub fn new(config: TsMuxerConfig) -> Self { ... }

    /// 创建新的 TS 段
    pub fn create_segment(&mut self, frames: &[MediaFrame]) -> Bytes {
        let mut output = Vec::new();

        // 1. 写入 PAT
        let pat_packets = self.pat.generate();
        for packet in pat_packets {
            output.extend_from_slice(&packet.encode());
        }

        // 2. 写入 PMT
        let pmt_packets = self.pmt.generate();
        for packet in pmt_packets {
            output.extend_from_slice(&packet.encode());
        }

        // 3. 处理每个帧
        let mut need_pcr = true;
        for frame in frames {
            // 将帧分割成 TS 包
            let ts_packets = self.frame_to_ts_packets(frame, need_pcr)?;
            need_pcr = false;

            for packet in ts_packets {
                output.extend_from_slice(&packet.encode());
            }
        }

        Bytes::from(output)
    }

    fn frame_to_ts_packets(&mut self, frame: &MediaFrame, insert_pcr: bool) -> Vec<TsPacket> {
        // 1. 创建 PES 包
        let pes = if frame.is_video() {
            self.video_pes.encode(frame)
        } else {
            self.audio_pes.encode(frame)
        };

        // 2. 将 PES 分割成多个 TS 包
        let pid = if frame.is_video() { self.config.video_pid } else { self.config.audio_pid };

        let mut packets = Vec::new();
        let mut data = pes.encode();  // PES 编码为字节

        let mut first = true;
        while !data.is_empty() {
            let payload_len = if first && insert_pcr {
                // 第一个包可能需要 adaptation field 放 PCR
                184 - 8  // 8 字节 adaptation field
            } else {
                184
            };

            let payload: Vec<u8> = data.drain(..payload_len.min(data.len())).collect();

            let packet = TsPacket {
                header: TsPacketHeader {
                    payload_unit_start_indicator: first,
                    pid,
                    continuity_counter: self.get_next_counter(pid),
                    ..
                },
                adaptation_field: if first && insert_pcr {
                    Some(AdaptationField::with_pcr(self.calculate_pcr(frame)))
                } else {
                    None
                },
                payload,
            };

            packets.push(packet);
            first = false;
        }

        packets
    }
}
```

#### 步骤 5: 修改 segment.rs

```rust
// src/protocol/hls/segment.rs

/// Encode frames to MPEG-TS segment
fn encode_ts_segment(frames: &[MediaFrame]) -> HlsResult<Bytes> {
    // 检测编解码器
    let (video_codec, audio_codec) = detect_codecs(frames);

    let config = TsMuxerConfig {
        video_pid: 0x100,
        audio_pid: 0x101,
        pcr_pid: 0x100,
        video_codec,
        audio_codec,
    };

    let mut muxer = TsMuxer::new(config);
    muxer.create_segment(frames)
}
```

---

## 任务 2: 实现 fMP4 Muxer (P0)

### 为什么需要 fMP4

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    MPEG-TS vs fMP4 对比                                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  MPEG-TS                        fMP4 (CMAF)                             │
│  ─────────                      ───────────                             │
│  188字节固定包                  基于Box的灵活结构                        │
│  每包188字节开销                更低的容器开销                           │
│  PAT/PMT开销                    init segment仅需一次                     │
│  解析复杂度高                   解析简单                                 │
│  延迟较高 (通常6秒)             支持LL-HLS (延迟<2秒)                    │
│  广泛兼容                       现代浏览器优先支持                       │
│                                                                          │
│  结论: HLS 需要同时支持两种格式                                          │
│        - MPEG-TS: 最大兼容性                                             │
│        - fMP4: 低延迟场景                                                │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 文件结构

```
src/protocol/hls/
├── fmp4/
│   ├── mod.rs           # 导出 + 常量
│   ├── boxes.rs         # Box 定义 (ftyp, moov, moof, mdat)
│   ├── init_segment.rs  # 初始化段 (ftyp + moov)
│   ├── media_segment.rs # 媒体段 (moof + mdat)
│   └── muxer.rs         # Fmp4Muxer 主入口
```

### 核心结构设计

```rust
// boxes.rs

/// MP4 Box 基础 trait
pub trait Mp4Box {
    fn box_type(&self) -> FourCC;
    fn box_size(&self) -> u32;
    fn write(&self, writer: &mut impl Write) -> io::Result<()>;
}

/// Four Character Code
#[derive(Debug, Clone, Copy)]
pub struct FourCC([u8; 4]);

impl FourCC {
    pub const FTYP: Self = Self(*b"ftyp");
    pub const MOOV: Self = Self(*b"moov");
    pub const MOOF: Self = Self(*b"moof");
    pub const MDAT: Self = Self(*b"mdat");
    pub const MVHD: Self = Self(*b"mvhd");
    pub const TRAK: Self = Self(*b"trak");
    pub const TKHD: Self = Self(*b"tkhd");
    pub const MDIA: Self = Self(*b"mdia");
    pub const MFHD: Self = Self(*b"mfhd");
    pub const TRAF: Self = Self(*b"traf");
    pub const TFHD: Self = Self(*b"tfhd");
    pub const TRUN: Self = Self(*b"trun");
}

/// ftyp box (File Type Box)
pub struct FtypBox {
    pub major_brand: FourCC,
    pub minor_version: u32,
    pub compatible_brands: Vec<FourCC>,
}

/// moov box (Movie Box) - 初始化段核心
pub struct MoovBox {
    pub mvhd: MvhdBox,  // Movie Header
    pub trak: Vec<TrakBox>,  // Track(s)
}

/// mvhd box (Movie Header Box)
pub struct MvhdBox {
    pub creation_time: u64,
    pub modification_time: u64,
    pub timescale: u32,
    pub duration: u64,
    pub rate: u32,     // 0x00010000 = 1.0
    pub volume: u16,   // 0x0100 = 1.0
}

/// trak box (Track Box)
pub struct TrakBox {
    pub tkhd: TkhdBox,     // Track Header
    pub mdia: MdiaBox,     // Media
}

/// tkhd box (Track Header Box)
pub struct TkhdBox {
    pub track_id: u32,
    pub duration: u64,
    pub width: u32,    // 16.16 fixed point
    pub height: u32,   // 16.16 fixed point
}

/// moof box (Movie Fragment Box)
pub struct MoofBox {
    pub mfhd: MfhdBox,     // Movie Fragment Header
    pub traf: Vec<TrafBox>, // Track Fragments
}

/// mfhd box
pub struct MfhdBox {
    pub sequence_number: u32,
}

/// traf box (Track Fragment Box)
pub struct TrafBox {
    pub tfhd: TfhdBox,     // Track Fragment Header
    pub trun: TrunBox,     // Track Fragment Run
}

/// tfhd box
pub struct TfhdBox {
    pub track_id: u32,
    pub base_data_offset: Option<u64>,
    pub sample_description_index: Option<u32>,
    pub default_sample_duration: Option<u32>,
    pub default_sample_size: Option<u32>,
    pub default_sample_flags: Option<u32>,
}

/// trun box (Track Fragment Run Box)
pub struct TrunBox {
    pub sample_count: u32,
    pub data_offset: i32,
    pub samples: Vec<Sample>,
}

pub struct Sample {
    pub duration: u32,
    pub size: u32,
    pub flags: u32,
    pub composition_time_offset: i32,
}

/// mdat box (Media Data Box)
pub struct MdatBox {
    pub data: Bytes,
}
```

### Init Segment 生成

```rust
// init_segment.rs

/// fMP4 初始化段生成器
pub struct InitSegmentBuilder {
    video_track: Option<TrackConfig>,
    audio_track: Option<TrackConfig>,
    timescale: u32,
}

struct TrackConfig {
    track_id: u32,
    codec: CodecType,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
}

impl InitSegmentBuilder {
    pub fn new() -> Self { ... }

    pub fn add_video_track(&mut self, codec: CodecType, width: u32, height: u32) {
        self.video_track = Some(TrackConfig { ... });
    }

    pub fn add_audio_track(&mut self, codec: CodecType, sample_rate: u32, channels: u16) {
        self.audio_track = Some(TrackConfig { ... });
    }

    /// 生成初始化段
    pub fn build(&self) -> Bytes {
        let mut output = Vec::new();

        // 1. ftyp box
        let ftyp = FtypBox {
            major_brand: FourCC::new("iso5"),
            minor_version: 512,
            compatible_brands: vec![
                FourCC::new("iso5"),
                FourCC::new("iso6"),
                FourCC::new("mp41"),
            ],
        };
        ftyp.write(&mut output).unwrap();

        // 2. moov box (包含所有初始化信息)
        let moov = self.build_moov();
        moov.write(&mut output).unwrap();

        Bytes::from(output)
    }

    fn build_moov(&self) -> MoovBox {
        let mut moov = MoovBox {
            mvhd: self.build_mvhd(),
            trak: Vec::new(),
        };

        if let Some(ref video) = self.video_track {
            moov.trak.push(self.build_video_trak(video));
        }

        if let Some(ref audio) = self.audio_track {
            moov.trak.push(self.build_audio_trak(audio));
        }

        moov
    }

    fn build_video_trak(&self, config: &TrackConfig) -> TrakBox {
        // 包含 tkhd, mdia (hdlr, minf, stbl, stsd 等)
        // stsd 包含编解码器特定信息 (avcC, hvcC 等)
    }
}
```

### Media Segment 生成

```rust
// media_segment.rs

/// fMP4 媒体段生成器
pub struct MediaSegmentBuilder {
    sequence_number: u32,
    video_samples: Vec<SampleEntry>,
    audio_samples: Vec<SampleEntry>,
    video_data: Vec<u8>,
    audio_data: Vec<u8>,
}

struct SampleEntry {
    duration: u32,
    size: u32,
    is_sync: bool,
    pts: u64,
    dts: u64,
}

impl MediaSegmentBuilder {
    pub fn new(sequence_number: u32) -> Self { ... }

    pub fn add_video_frame(&mut self, frame: &MediaFrame) {
        let entry = SampleEntry {
            duration: self.calculate_duration(frame),
            size: frame.size() as u32,
            is_sync: frame.is_keyframe(),
            pts: frame.pts.as_90kHz(),
            dts: frame.dts.as_90kHz(),
        };
        self.video_samples.push(entry);
        self.video_data.extend_from_slice(&frame.data);
    }

    pub fn add_audio_frame(&mut self, frame: &MediaFrame) {
        // ...
    }

    /// 生成媒体段
    pub fn build(self) -> Bytes {
        let mut output = Vec::new();

        // 1. moof box
        let moof = self.build_moof();
        moof.write(&mut output).unwrap();

        // 2. mdat box
        let mdat = MdatBox {
            data: Bytes::from([self.video_data, self.audio_data].concat()),
        };
        mdat.write(&mut output).unwrap();

        Bytes::from(output)
    }

    fn build_moof(&self) -> MoofBox {
        MoofBox {
            mfhd: MfhdBox {
                sequence_number: self.sequence_number,
            },
            traf: self.build_trafs(),
        }
    }

    fn build_trafs(&self) -> Vec<TrafBox> {
        let mut trafs = Vec::new();

        if !self.video_samples.is_empty() {
            trafs.push(self.build_video_traf());
        }

        if !self.audio_samples.is_empty() {
            trafs.push(self.build_audio_traf());
        }

        trafs
    }

    fn build_video_traf(&self) -> TrafBox {
        let base_data_offset = 0; // moof 大小

        TrafBox {
            tfhd: TfhdBox {
                track_id: 1,
                base_data_offset: Some(base_data_offset),
                default_sample_duration: None,
                default_sample_size: None,
                ..Default::default()
            },
            trun: TrunBox {
                sample_count: self.video_samples.len() as u32,
                data_offset: 0, // 计算偏移
                samples: self.video_samples.iter().map(|s| Sample {
                    duration: s.duration,
                    size: s.size,
                    flags: if s.is_sync { 0x02000000 } else { 0x01000000 },
                    composition_time_offset: (s.pts as i32) - (s.dts as i32),
                }).collect(),
            },
        }
    }
}
```

### Fmp4Muxer 主入口

```rust
// muxer.rs

/// fMP4 Muxer
pub struct Fmp4Muxer {
    init_segment: Option<Bytes>,
    sequence_number: AtomicU32,
    config: Fmp4Config,
}

impl Fmp4Muxer {
    pub fn new(config: Fmp4Config) -> Self { ... }

    /// 生成初始化段 (只需要一次)
    pub fn init_segment(&mut self, video_config: Option<VideoConfig>, audio_config: Option<AudioConfig>) -> Bytes {
        let mut builder = InitSegmentBuilder::new();

        if let Some(vc) = video_config {
            builder.add_video_track(vc.codec, vc.width, vc.height);
        }

        if let Some(ac) = audio_config {
            builder.add_audio_track(ac.codec, ac.sample_rate, ac.channels);
        }

        let init = builder.build();
        self.init_segment = Some(init.clone());
        init
    }

    /// 生成媒体段
    pub fn create_segment(&self, frames: &[MediaFrame]) -> Bytes {
        let seq = self.sequence_number.fetch_add(1, Ordering::SeqCst);
        let mut builder = MediaSegmentBuilder::new(seq);

        for frame in frames {
            if frame.is_video() {
                builder.add_video_frame(frame);
            } else if frame.is_audio() {
                builder.add_audio_frame(frame);
            }
        }

        builder.build()
    }
}
```

---

## 任务 3: 集成服务器二进制 (P1)

### 当前问题

`src/bin/server.rs` 只是一个 placeholder：

```rust
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("🚀 rslive-server starting...");
    // 没有实际服务器启动代码
    Ok(())
}
```

### 目标架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        rslive-server 架构                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│   ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                │
│   │ RTMP Server │    │ HTTP-FLV    │    │ HLS Server  │                │
│   │  :1935      │    │  :8080      │    │  :8081      │                │
│   └──────┬──────┘    └──────┬──────┘    └──────┬──────┘                │
│          │                  │                  │                        │
│          └──────────────────┼──────────────────┘                        │
│                             │                                           │
│                    ┌────────▼────────┐                                  │
│                    │  StreamRouter   │                                  │
│                    │  (中央路由)      │                                  │
│                    └────────┬────────┘                                  │
│                             │                                           │
│          ┌──────────────────┼──────────────────┐                        │
│          │                  │                  │                        │
│   ┌──────▼──────┐    ┌──────▼──────┐    ┌──────▼──────┐                │
│   │   Stream    │    │   Stream    │    │   Stream    │                │
│   │  "live/cam1"│    │  "live/cam2"│    │  "live/cam3"│                │
│   └─────────────┘    └─────────────┘    └─────────────┘                │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 实现代码

```rust
// src/bin/server.rs

use anyhow::Result;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, error};

use rslive::media::{StreamRouter, RouterConfig};
use rslive::rtmp::RtmpServer;
use rslive::flv::HttpFlvServer;
use rslive::hls::{HlsServer, HlsPackagerManager, PackagerConfig};
use rslive::protocol::hls::segment::MemorySegmentStorage;

#[derive(Debug, Clone)]
struct ServerConfig {
    rtmp_addr: String,
    http_flv_addr: String,
    hls_addr: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            rtmp_addr: "0.0.0.0:1935".to_string(),
            http_flv_addr: "0.0.0.0:8080".to_string(),
            hls_addr: "0.0.0.0:8081".to_string(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    let config = ServerConfig::default();

    info!("🚀 rslive-server starting...");
    info!("RTMP: {}", config.rtmp_addr);
    info!("HTTP-FLV: {}", config.http_flv_addr);
    info!("HLS: {}", config.hls_addr);

    // 创建中央流路由器
    let router = Arc::new(StreamRouter::new(RouterConfig::default()));

    // 创建 HLS Packager Manager
    let packager_config = PackagerConfig::default();
    let segment_storage = Arc::new(MemorySegmentStorage::new(100));
    let packager_manager = Arc::new(HlsPackagerManager::new(packager_config, segment_storage));

    // 启动各协议服务器
    let (rtmp_handle, http_flv_handle, hls_handle) = {
        // RTMP Server
        let router_clone = Arc::clone(&router);
        let packager_clone = Arc::clone(&packager_manager);
        let rtmp_addr = config.rtmp_addr.clone();
        let rtmp_handle = tokio::spawn(async move {
            let mut server = RtmpServer::with_defaults();
            // 设置事件回调
            server.on_publish(|stream_key| {
                info!("Publisher connected: {}", stream_key);
            });
            server.on_unpublish(|stream_key| {
                info!("Publisher disconnected: {}", stream_key);
            });
            server.on_play(|stream_key| {
                info!("Player connected: {}", stream_key);
            });

            if let Err(e) = server.listen(&rtmp_addr).await {
                error!("RTMP server error: {}", e);
            }
        });

        // HTTP-FLV Server
        let router_clone = Arc::clone(&router);
        let http_flv_addr = config.http_flv_addr.clone();
        let http_flv_handle = tokio::spawn(async move {
            let server = HttpFlvServer::new(router_clone, http_flv_addr);
            if let Err(e) = server.run().await {
                error!("HTTP-FLV server error: {}", e);
            }
        });

        // HLS Server
        let router_clone = Arc::clone(&router);
        let packager_clone = Arc::clone(&packager_manager);
        let hls_addr = config.hls_addr.clone();
        let hls_handle = tokio::spawn(async move {
            let server = HlsServer::new(
                router_clone,
                packager_clone,
                Default::default(),
                Default::default(),
            );
            if let Err(e) = server.run().await {
                error!("HLS server error: {}", e);
            }
        });

        (rtmp_handle, http_flv_handle, hls_handle)
    };

    info!("✅ All servers started");

    // 等待关闭信号
    match signal::ctrl_c().await {
        Ok(()) => info!("Shutdown signal received"),
        Err(err) => error!("Unable to listen for shutdown signal: {}", err),
    }

    // 优雅关闭
    rtmp_handle.abort();
    http_flv_handle.abort();
    hls_handle.abort();

    info!("🛑 Server stopped");

    Ok(())
}
```

---

## 任务 4: RTMP → HLS 转码管道 (P1)

### 目标

实现完整的 RTMP 推流到 HLS 输出的转换管道。

### 架构

```
RTMP Push → Chunk Decode → Message Parse → Frame Extract → HLS Package
    │            │              │               │               │
    ▼            ▼              ▼               ▼               ▼
  Socket     RtmpChunk     RtmpMessage     MediaFrame      TS/fMP4
                                                          Segment
```

### 实现位置

在 `src/protocol/hls/` 添加 `transcoder.rs`:

```rust
// src/protocol/hls/transcoder.rs

/// RTMP 到 HLS 转码器
pub struct RtmpToHlsTranscoder {
    router: Arc<StreamRouter>,
    packager_manager: Arc<HlsPackagerManager>,
    active_streams: DashMap<StreamId, TranscodeHandle>,
}

struct TranscodeHandle {
    packager: Arc<HlsPackager>,
    task: JoinHandle<()>,
}

impl RtmpToHlsTranscoder {
    pub fn new(router: Arc<StreamRouter>, packager_manager: Arc<HlsPackagerManager>) -> Self {
        Self {
            router,
            packager_manager,
            active_streams: DashMap::new(),
        }
    }

    /// 开始转码一个流
    pub fn start_transcode(&self, stream_id: StreamId) -> MediaResult<()> {
        // 1. 订阅流
        let subscriber = self.router.subscribe(&stream_id)?;

        // 2. 创建 HLS packager
        let packager = self.packager_manager.create_packager(stream_id.clone());

        // 3. 启动转码任务
        let stream_id_clone = stream_id.clone();
        let task = tokio::spawn(async move {
            loop {
                match subscriber.recv().await {
                    Ok(frame) => {
                        if let Err(e) = packager.process_frame(frame).await {
                            tracing::error!("Failed to process frame: {}", e);
                        }
                    }
                    Err(MediaError::ChannelClosed) => break,
                    Err(e) => {
                        tracing::error!("Error receiving frame: {}", e);
                        break;
                    }
                }
            }
            tracing::info!("Transcode task ended for stream: {}", stream_id_clone.as_str());
        });

        self.active_streams.insert(stream_id, TranscodeHandle { packager, task });

        Ok(())
    }

    /// 停止转码
    pub fn stop_transcode(&self, stream_id: &StreamId) {
        if let Some((_, handle)) = self.active_streams.remove(stream_id) {
            handle.task.abort();
        }
    }
}
```

---

## 任务 5: 测试覆盖率提升 (P2)

### 当前测试情况

```
模块                  测试覆盖率
────────────────────────────────
protocol/rtmp         ~60%
protocol/amf0         ~70%
protocol/amf3         ~70%
protocol/flv          ~50%
protocol/hls          ~30%  ← 需要重点补充
media                 ~80%
utils                 ~90%
```

### 需要添加的测试

#### HLS 测试

```rust
// src/protocol/hls/mpegts/tests.rs

#[test]
fn test_ts_packet_encoding() {
    let packet = TsPacket::new()
        .with_pid(0x100)
        .with_payload(vec![1, 2, 3, 4]);

    let encoded = packet.encode();
    assert_eq!(encoded.len(), 188);
    assert_eq!(encoded[0], 0x47); // sync byte
}

#[test]
fn test_pat_generation() {
    let pat = PatGenerator::new()
        .with_program(1, 0x1000);

    let packets = pat.generate();
    assert!(!packets.is_empty());

    // 验证 CRC32 正确
    let data = packets[0].encode();
    // CRC32 应该在最后 4 字节
    assert_ne!(&data[184..188], &[0, 0, 0, 0]);
}

#[test]
fn test_pes_encoding() {
    let frame = MediaFrame::video(
        1,
        Timestamp::from_millis(1000),
        VideoFrameType::Keyframe,
        CodecType::H264,
        Bytes::from(vec![0; 100]),
    );

    let pes = PesEncoder::new(StreamType::VideoH264).encode(&frame);

    // 验证 PES start code
    assert_eq!(&pes.data[0..3], &[0, 0, 1]);
    // 验证 stream_id (video)
    assert_eq!(pes.data[3], 0xE0);
}

#[test]
fn test_full_ts_segment() {
    let frames = create_test_frames(100);

    let segment = encode_ts_segment(&frames).unwrap();

    // 验证以 PAT 开始
    assert_eq!(segment[0], 0x47);
    // 验证大小合理
    assert!(segment.len() > 1000);
}
```

#### fMP4 测试

```rust
// src/protocol/hls/fmp4/tests.rs

#[test]
fn test_init_segment() {
    let mut builder = InitSegmentBuilder::new();
    builder.add_video_track(CodecType::H264, 1920, 1080);
    builder.add_audio_track(CodecType::AAC, 48000, 2);

    let init = builder.build();

    // 验证 ftyp box
    assert_eq!(&init[4..8], b"ftyp");
    // 验证 moov box
    assert!(init.windows(4).any(|w| w == b"moov"));
}

#[test]
fn test_media_segment() {
    let frames = create_test_frames(30);

    let segment = encode_fmp4_segment(&frames).unwrap();

    // 验证 moof box
    assert!(segment.windows(4).any(|w| w == b"moof"));
    // 验证 mdat box
    assert!(segment.windows(4).any(|w| w == b"mdat"));
}
```

---

## 任务 6: 性能基准测试验证 (P2)

### 运行现有基准测试

```bash
cargo bench
```

### 预期结果

```
rtmp_chunk/create_chunks_1kb    time:   [1.2345 µs 1.3456 µs 1.4567 µs]
rtmp_chunk/process_chunks_1kb   time:   [2.3456 µs 2.4567 µs 2.5678 µs]

amf_encoding/amf0_encode_string time:   [234.56 ns 245.67 ns 256.78 ns]
amf_encoding/amf3_encode_string time:   [345.67 ns 356.78 ns 367.89 ns]

flv_encoding/flv_encode_video   time:   [1.4567 µs 1.5678 µs 1.6789 µs]

buffer_pool/pool_get_small      time:   [45.678 ns 56.789 ns 67.890 ns]
buffer_pool/direct_allocation   time:   [234.56 ns 245.67 ns 256.78 ns]

concurrent/dashmap_read         time:   [1.2345 µs 1.3456 µs 1.4567 µs]
concurrent/mutex_hashmap_read   time:   [5.6789 µs 6.7890 µs 7.8901 µs]
```

### 添加 HLS 基准测试

```rust
// benches/protocol_bench.rs 添加

fn bench_hls_encoding(c: &mut Criterion) {
    use rslive::hls::mpegts::TsMuxer;

    let mut group = c.benchmark_group("hls_encoding");

    let frames = create_test_frames(100);

    group.bench_function("mpegts_segment_100_frames", |b| {
        let mut muxer = TsMuxer::new(Default::default());
        b.iter(|| {
            black_box(muxer.create_segment(&frames))
        })
    });

    group.bench_function("fmp4_segment_100_frames", |b| {
        let muxer = Fmp4Muxer::new(Default::default());
        b.iter(|| {
            black_box(muxer.create_segment(&frames))
        })
    });

    group.finish();
}
```

---

## 实施顺序

```
Week 1:
├── Day 1-2: MPEG-TS muxer 核心结构 (ts_packet, pat, pmt)
├── Day 3-4: MPEG-TS PES 编码 + TsMuxer 组装
└── Day 5:   MPEG-TS 测试

Week 2:
├── Day 1-2: fMP4 Box 定义 + Init Segment
├── Day 3-4: fMP4 Media Segment + Muxer
└── Day 5:   fMP4 测试

Week 3:
├── Day 1:   服务器二进制集成
├── Day 2:   RTMP→HLS 转码管道
├── Day 3:   测试补充
├── Day 4:   性能基准测试
└── Day 5:   文档更新 + 代码审查
```

---

## 验收标准

### MPEG-TS Muxer

- [ ] 能生成合法的 PAT/PMT (CRC32 正确)
- [ ] PES 包含正确的 PTS/DTS
- [ ] PCR 每 100ms 插入一次
- [ ] TS 包连续性计数器正确递增
- [ ] 生成的 TS 文件能用 FFmpeg 播放

### fMP4 Muxer

- [ ] Init Segment 包含正确的 avcC/hvcC
- [ ] Media Segment 的 trun box 正确
- [ ] 生成的 fMP4 能用 Safari 播放
- [ ] 支持 LL-HLS 部分 segment

### 服务器集成

- [ ] FFmpeg 能推流到 rslive-server
- [ ] VLC 能播放 HTTP-FLV
- [ ] Safari 能播放 HLS
- [ ] Ctrl+C 能优雅关闭

### 性能

- [ ] 1000 并发连接下内存 < 500MB
- [ ] HLS segment 生成延迟 < 10ms
- [ ] 基准测试显示 DashMap 优于 Mutex<HashMap> 5x+
