# 协议实现计划

## 协议优先级矩阵

| 协议 | 优先级 | 难度 | 依赖 | 用途 |
|------|--------|------|------|------|
| RTMP | P0 | 中 | 无 | 传统推流、播放 |
| FLV | P0 | 低 | RTMP | HTTP 流式传输 |
| HLS | P0 | 中 | 无 | 移动端播放 |
| SRT | P1 | 高 | 无 | 专业广播、低延迟 |
| WebRTC | P1 | 高 | 无 | 浏览器实时通信 |
| DASH | P2 | 中 | 无 | 自适应流媒体 |
| RTSP | P2 | 中 | 无 | IP 摄像头 |
| MPEG-TS | P2 | 低 | 无 | 广播传输 |
| RTP/RTCP | P2 | 中 | 无 | 实时传输 |

## RTMP (P0) - 当前重点

### 已完成模块
- [x] AMF0 编解码器 (完整 13 种类型)
- [x] AMF3 基础编解码器
- [x] Chunk 分块/重组逻辑
- [x] 握手协议 (Simple Handshake)
- [x] 基础服务器框架

### 待实现功能

#### 阶段 1: 完善核心功能 (2 周)
- [ ] **Complex Handshake (S1/S2 验证)**
  - HMAC-SHA256 验证
  - 时间戳验证
  - 随机数验证

- [ ] **完整的发布/播放转发**
  - 实现 broadcast_to_stream 方法
  - 订阅者管理优化
  - 流数据转发队列

- [ ] **共享对象 (Shared Object)**
  - SO 消息处理
  - 状态同步机制

#### 阶段 2: 性能优化 (2 周)
- [ ] **异步化改造**
  ```rust
  // 当前: 阻塞 I/O
  pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> RtmpResult<RtmpChunk>
  
  // 目标: 异步 I/O
  pub async fn read_chunk<R: AsyncRead + Unpin>(
      &mut self, 
      reader: &mut R
  ) -> RtmpResult<RtmpChunk>
  ```

- [ ] **零拷贝优化**
  - 使用 `BytesMut` 替代 `Vec<u8>`
  - 实现 `BufMut` trait 支持
  - 避免 chunk 重组时的数据复制

- [ ] **连接池化**
  - 预分配缓冲区池
  - 对象池复用 Chunk/ Message 对象

#### 阶段 3: 高级特性 (1 周)
- [ ] **录制功能**
  - FLV 文件写入
  - 分段录制
  - 自动清理策略

- [ ] **统计和监控**
  - 比特率计算
  - 帧率统计
  - 延迟测量

### RTMP 模块结构

```
src/protocol/rtmp/
├── mod.rs              # 公共导出和常量
├── handshake.rs        # 握手实现 (Simple + Complex)
├── chunk.rs            # Chunk 编解码 (当前已完成)
├── message.rs          # 消息类型定义和解析
├── connection.rs       # 连接状态管理
├── server.rs           # 服务器实现 (当前阻塞式)
├── client.rs           # 客户端实现
├── publisher.rs        # 发布者逻辑 (新增)
├── subscriber.rs       # 订阅者逻辑 (新增)
├── stream_router.rs    # 流路由管理 (新增)
└── recording.rs        # 录制功能 (新增)
```

## FLV (P0) - HTTP 流式传输

### 实现计划 (1 周)

#### FLV 封装/解封装
```rust
pub struct FlvEncoder;
pub struct FlvDecoder;

impl FlvEncoder {
    pub fn encode_header(has_video: bool, has_audio: bool) -> Bytes;
    pub fn encode_tag(tag: FlvTag) -> Bytes;
    pub fn encode_video_frame(frame: VideoFrame) -> FlvTag;
    pub fn encode_audio_frame(frame: AudioFrame) -> FlvTag;
}
```

#### HTTP-FLV 服务器
- 基于 HTTP 的 FLV 流传输
- 支持 Range 请求（时移回放）
- 与 RTMP 共享流数据源

#### 模块结构
```
src/protocol/flv/
├── mod.rs
├── header.rs           # FLV Header 处理
├── tag.rs              # Tag 类型定义
├── encoder.rs          # 编码器
├── decoder.rs          # 解码器
└── http_server.rs      # HTTP-FLV 服务器
```

## HLS (P0) - 苹果生态必备

### 实现计划 (2 周)

#### HLS 生成器
```rust
pub struct HlsGenerator {
    segment_duration: Duration,
    playlist_type: PlaylistType, // VOD / Event / Live
    segments: Vec<MediaSegment>,
}

impl HlsGenerator {
    pub fn add_segment(&mut self, data: Bytes, timestamp: Duration);
    pub fn generate_m3u8(&self) -> String;
    pub fn get_segment(&self, index: usize) -> Option<&MediaSegment>;
}
```

#### 核心功能
- [ ] TS 片段生成 (fMP4 支持可选)
- [ ] 主播放列表 (Master Playlist)
- [ ] 多码率自适应 (ABR)
- [ ] DVR 时移功能
- [ ] Low-Latency HLS (LL-HLS) 支持

#### 模块结构
```
src/protocol/hls/
├── mod.rs
├── m3u8.rs             # 播放列表生成
├── segment.rs          # 片段管理
├── generator.rs        # HLS 生成器
├── server.rs           # HTTP 服务器
└── ll_hls.rs           # 低延迟 HLS 支持
```

## SRT (P1) - 下一代推流协议

### 为什么需要 SRT
1. **更低延迟**: 相比 RTMP 的 TCP，SRT 基于 UDP 可实现 < 100ms 延迟
2. **抗丢包**: ARQ + FEC 机制在 20% 丢包率下仍可流畅传输
3. **现代加密**: 内置 AES-128/192/256 加密
4. **穿透防火墙**: Rendezvous 模式无需端口映射

### 实现计划 (4 周)

#### 核心组件
```rust
pub struct SrtSocket {
    state: SrtState,
    config: SrtConfig,
    // UDP socket + SRT 协议层
}

pub struct SrtConfig {
    latency: Duration,        // 缓冲区延迟 (默认 120ms)
    encryption: Option<AesConfig>,
    fec: FecConfig,           // 前向纠错配置
    bandwidth: BandwidthMode, // 带宽模式
}
```

#### 实现步骤
1. **Week 1**: UDP 基础 + 握手协议 (INDUCTION/CONCLUSION)
2. **Week 2**: 数据传输 + ARQ 机制 (ACK/NAK/ACKACK)
3. **Week 3**: 拥塞控制 + 流量控制
4. **Week 4**: FEC + 加密 + 优化

#### 模块结构
```
src/protocol/srt/
├── mod.rs
├── packet.rs           # SRT 数据包定义
├── handshake.rs        # 握手实现
├── socket.rs           # SRT Socket
├── arq.rs              # 自动重传请求
├── fec.rs              # 前向纠错
├── crypto.rs           # AES 加密
├── congestion.rs       # 拥塞控制
└── listener.rs         # 服务器监听
```

## WebRTC (P1) - 浏览器支持

### 为什么需要 WebRTC
1. **浏览器原生支持**: 无需插件，现代浏览器都内置
2. **超低延迟**: 可实现 < 100ms 的端到端延迟
3. **P2P 能力**: 支持点对点传输，降低服务器负载
4. **广泛生态**: 会议、直播、物联网等多场景

### 实现计划 (6 周)

#### 核心组件
```rust
pub struct WebRtcPeer {
    signaling: SignalingClient,
    ice_agent: IceAgent,
    dtls: DtlsConnection,
    srtp: SrtpSession,
    sctp: Option<SctpAssociation>, // DataChannel
}
```

#### 实现步骤
1. **Week 1-2**: ICE (Interactive Connectivity Establishment)
   - STUN/TURN 客户端
   - Candidate 收集和连接检查

2. **Week 3**: DTLS (Datagram TLS)
   - 基于 rustls 的 DTLS 实现或使用 native library
   - 证书指纹验证

3. **Week 4**: SRTP (Secure RTP)
   - RTP 包加密/解密
   - 密钥派生

4. **Week 5**: SDP 处理
   - Offer/Answer 生成和解析
   - 编协商

5. **Week 6**: DataChannel (SCTP over DTLS)
   - 可靠/不可靠消息传输
   - 多流支持

#### 模块结构
```
src/protocol/webrtc/
├── mod.rs
├── peer.rs             # PeerConnection
├── sdp.rs              # SDP 处理
├── ice/
│   ├── mod.rs
│   ├── agent.rs
│   ├── stun.rs
│   └── turn.rs
├── dtls/
│   ├── mod.rs
│   └── connection.rs
├── srtp/
│   ├── mod.rs
│   ├── session.rs
│   └── packet.rs
├── sctp/
│   ├── mod.rs
│   └── association.rs
└── signaling.rs        # 信令接口
```

## DASH (P2) - 自适应流媒体

### 实现计划 (2 周)
- MPD (Media Presentation Description) 生成
- SegmentTemplate / SegmentTimeline 支持
- 动态码率切换
- WebM / ISO BMFF (fMP4) 片段生成

## RTSP (P2) - IP 摄像头支持

### 实现计划 (2 周)
- SDP 会话描述
- SETUP/PLAY/PAUSE/TEARDOWN 命令
- RTP over TCP (Interleaved) / UDP
- 基础认证 (Digest/Basic)

## 协议转换矩阵

目标是为任意输入协议提供到任意输出协议的转换能力：

| 输入 \ 输出 | RTMP | FLV | HLS | SRT | WebRTC | DASH |
|------------|------|-----|-----|-----|--------|------|
| RTMP       | -    | ✓   | ✓   | ✓   | ✓      | ✓    |
| FLV        | ✓    | -   | ✓   | ✓   | ✓      | ✓    |
| SRT        | ✓    | ✓   | ✓   | -   | ✓      | ✓    |
| WebRTC     | ✗    | ✗   | ✓   | ✓   | -      | ✓    |
| RTSP       | ✓    | ✓   | ✓   | ✓   | ✓      | ✓    |

**注**: WebRTC 输出到 RTMP/FLV 不可行（WebRTC 使用 Web 编解码器，RTMP 使用传统编解码器）

## 通用抽象层

### MediaFrame 通用帧结构
```rust
pub struct MediaFrame {
    pub timestamp: Duration,
    pub track_id: u32,
    pub data: Bytes,
    pub frame_type: FrameType,
    pub codec: CodecType,
    pub is_keyframe: bool,
}

pub enum FrameType {
    Video,
    Audio,
    Data,      // 元数据、字幕等
}

pub enum CodecType {
    // 视频
    H264,
    H265,
    AV1,
    VP8,
    VP9,
    // 音频
    AAC,
    Opus,
    G711A,
    G711U,
}
```

### StreamRouter 流路由
```rust
pub struct StreamRouter {
    sources: HashMap<String, Arc<dyn StreamSource>>,
    sinks: HashMap<String, Vec<Arc<dyn StreamSink>>>,
}

pub trait StreamSource: Send + Sync {
    fn subscribe(&self) -> mpsc::Receiver<MediaFrame>;
}

pub trait StreamSink: Send + Sync {
    fn publish(&self, frame: MediaFrame) -> Result<()>;
}
```
