# 开发路线图

## 版本规划

```
v0.1.x  (当前) → 基础 RTMP 功能
    ↓
v0.2.x  → 异步化改造 + FLV 支持
    ↓
v0.3.x  → HLS 支持 + 性能优化
    ↓
v0.4.x  → SRT 协议支持
    ↓
v0.5.x  → WebRTC 支持
    ↓
v1.0.0  → 生产就绪 + 稳定 API
```

---

## Phase 1: 基础加固 (2-3 周) → v0.1.x

### 目标
完成 RTMP 核心功能，使其能够处理实际的推流和播放场景。

### 任务清单

#### Week 1: 完善 RTMP 服务器
- [ ] 修复 `broadcast_to_stream` 实现
  - 当前: placeholder 实现 (server.rs:268)
  - 目标: 完整的发布/订阅转发
  
- [ ] 实现 Complex Handshake
  - HMAC-SHA256 验证
  - S1/S2 时间戳和随机数验证
  
- [ ] 添加连接心跳机制
  - Ping/Pong 处理
  - 超时检测优化

#### Week 2: 流管理优化
- [ ] 流路由表实现
  ```rust
  pub struct StreamRouter {
      streams: DashMap<String, Stream>,
  }
  
  struct Stream {
      publisher: Arc<dyn Publisher>,
      subscribers: Vec<Arc<dyn Subscriber>>,
  }
  ```

- [ ] 订阅者管理
  - 动态订阅/取消订阅
  - 订阅者缓冲区（处理不同速度）

- [ ] 流统计信息
  - 比特率计算
  - 帧率统计
  - 连接质量指标

#### Week 3: 测试与文档
- [ ] 单元测试覆盖
  - Chunk 编解码 100% 覆盖
  - 消息处理流程测试
  
- [ ] 集成测试
  - 使用 FFmpeg 推流测试
  - VLC/FFplay 播放测试
  
- [ ] 示例程序
  - simple_server: 基础服务器
  - simple_publisher: 推流客户端
  - simple_player: 播放客户端

### 里程碑
- ✅ FFmpeg 可以成功推流到 rslive
- ✅ VLC 可以播放 rslive 的流
- ✅ 单服务器支持 1000+ 并发连接

---

## Phase 2: 异步化改造 (3-4 周) → v0.2.0

### 目标
将阻塞 I/O 架构迁移到 Tokio 异步运行时，实现高并发支持。

### 任务清单

#### Week 1-2: 核心异步化
- [ ] 依赖迁移
  ```toml
  [dependencies]
  tokio = { version = "1", features = ["full"] }
  bytes = "1.5"
  tokio-util = "0.7"
  ```

- [ ] RTMP Chunk 异步改造
  ```rust
  // 改造前
  pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> RtmpResult<RtmpChunk>
  
  // 改造后
  pub async fn read_chunk<R: AsyncRead + Unpin>(
      &mut self,
      reader: &mut R
  ) -> RtmpResult<RtmpChunk>
  ```

- [ ] 服务器异步化
  ```rust
  pub async fn listen(&mut self, addr: &str) -> RtmpResult<()>
  
  pub async fn handle_client(
      &self,
      stream: TcpStream
  ) -> RtmpResult<()>
  ```

- [ ] 移除所有 `std::sync::Mutex` + `thread::spawn`

#### Week 3: 零拷贝改造
- [ ] 数据类型替换
  - `Vec<u8>` → `Bytes` / `BytesMut`
  - `HashMap<K, Arc<Mutex<V>>>` → `DashMap<K, V>`
  
- [ ] 实现缓冲区池
  ```rust
  pub struct BufferPool {
      pool: Pool<BytesMut>,
  }
  ```

- [ ] 验证零拷贝路径
  - 推流 → 播放 全程无数据复制

#### Week 4: FLV 支持
- [ ] FLV 封装器
  ```rust
  pub struct FlvEncoder;
  impl FlvEncoder {
      pub fn encode_header(...) -> Bytes;
      pub fn encode_video_frame(...) -> FlvTag;
      pub fn encode_audio_frame(...) -> FlvTag;
  }
  ```

- [ ] HTTP-FLV 服务器
  - 基于 hyper/axum
  - 支持 Range 请求

- [ ] 录制功能
  - RTMP 流转 FLV 文件
  - 分段录制支持

### 里程碑
- ✅ 单服务器支持 10,000+ 并发连接
- ✅ 内存占用 < 100MB/万连接
- ✅ CPU 占用 < 10%/万连接
- ✅ HTTP-FLV 播放正常

---

## Phase 3: HLS 支持 (2-3 周) → v0.3.0

### 目标
添加 HLS 协议支持，使移动端和 Safari 可以播放。

### 任务清单

#### Week 1: TS 封装器
- [ ] MPEG-TS 封装
  ```rust
  pub struct TsEncoder;
  impl TsEncoder {
      pub fn encode_pes_packet(...) -> Bytes;
      pub fn create_pat() -> Bytes;
      pub fn create_pmt(...) -> Bytes;
  }
  ```

- [ ] PAT/PMT 生成
- [ ] PES 包封装
- [ ] PCR (Program Clock Reference) 计算

#### Week 2: HLS 生成器
- [ ] M3U8 播放列表生成
  ```rust
  pub struct HlsGenerator {
      segment_duration: Duration,
      segments: Vec<MediaSegment>,
  }
  ```

- [ ] 媒体片段管理
  - 滑动窗口保留策略
  - DVR 时移支持
  
- [ ] 主播放列表 (Master Playlist)
  - 多码率支持

#### Week 3: HTTP 服务器 + 优化
- [ ] HLS HTTP 服务器
  - M3U8 请求处理
  - TS 片段请求处理
  
- [ ] Low-Latency HLS (LL-HLS) 支持
  - Partial Segment
  - Preload Hint
  - Blocking Playlist Reload

### 里程碑
- ✅ Safari 可以播放 HLS 流
- ✅ iOS 设备播放正常
- ✅ 端到端延迟 < 5 秒 (标准 HLS)
- ✅ 端到端延迟 < 2 秒 (LL-HLS)

---

## Phase 4: SRT 协议 (4-5 周) → v0.4.0

### 目标
实现 SRT 协议，提供比 RTMP 更低延迟和更好网络适应性。

### 任务清单

#### Week 1: 基础架构
- [ ] SRT 包结构定义
  ```rust
  pub struct SrtPacket {
      header: SrtHeader,
      payload: Bytes,
  }
  ```

- [ ] UDP Socket 封装
- [ ] 握手协议 (INDUCTION/CONCLUSION)

#### Week 2: 数据传输
- [ ] 序列号管理
- [ ] 时间戳处理
- [ ] 发送/接收缓冲区

#### Week 3: ARQ 机制
- [ ] ACK (Acknowledgement) 包
- [ ] NAK (Negative ACK) 包
- [ ] 重传队列管理

#### Week 4: 拥塞控制 + FEC
- [ ] LiveCC (Live Congestion Control)
- [ ] FileCC (File Congestion Control)
- [ ] FEC 前向纠错
  - XOR 方案
  - Reed-Solomon 方案

#### Week 5: 加密 + 优化
- [ ] AES-128/192/256 加密
- [ ] 性能调优
- [ ] 与 RTMP/HLS 互通

### 里程碑
- ✅ FFmpeg 可以通过 SRT 推流
- ✅ SRT → HLS 转发正常
- ✅ 20% 丢包率下播放流畅
- ✅ 延迟 < 150ms (SRT 到 SRT)

---

## Phase 5: WebRTC (5-6 周) → v0.5.0

### 目标
实现 WebRTC 支持，使浏览器可以直接播放实时流。

### 任务清单

#### Week 1-2: ICE 实现
- [ ] STUN 客户端/服务器
- [ ] TURN 客户端
- [ ] ICE Candidate 收集
- [ ] ICE 连接检查

#### Week 3: DTLS + SRTP
- [ ] DTLS 握手
  - 基于 rustls 或 ring
- [ ] SRTP 密钥派生
- [ ] RTP/SRTP 加密解密

#### Week 4: SDP + 媒体
- [ ] SDP Offer/Answer 生成
- [ ] 编解码协商
- [ ] RTP 包封装/解封装

#### Week 5: DataChannel
- [ ] SCTP over DTLS
- [ ] DataChannel 打开/关闭
- [ ] 可靠/不可靠传输

#### Week 6: 集成 + WHIP/WHEP
- [ ] WHIP (WebRTC-HTTP Ingestion Protocol)
- [ ] WHEP (WebRTC-HTTP Egress Protocol)
- [ ] 与现有协议互通

### 里程碑
- ✅ Chrome/Firefox/Safari 可以播放
- ✅ 延迟 < 500ms (WebRTC 播放)
- ✅ 支持 WHIP 推流
- ✅ 支持 WHEP 播放

---

## Phase 6: 生产就绪 (4-6 周) → v1.0.0

### 目标
完善监控、配置、部署等企业级特性，发布 1.0 稳定版。

### 任务清单

#### Week 1-2: 监控和可观测性
- [ ] Prometheus 指标导出
  - 连接数、流数、比特率
  - 延迟分布 (Histogram)
  - 错误率
  
- [ ] OpenTelemetry 追踪
  - 请求链路追踪
  - 性能热点分析
  
- [ ] 结构化日志
  - JSON 格式日志
  - 可配置的日志级别

#### Week 3: 配置管理
- [ ] 配置文件 (YAML/TOML)
  ```yaml
  server:
    rtmp:
      port: 1935
      chunk_size: 4096
    hls:
      enabled: true
      segment_duration: 2s
  ```

- [ ] 热重载
  - 信号触发重载
  - 配置验证
  - 优雅切换

- [ ] 环境变量支持

#### Week 4: 安全特性
- [ ] 访问控制
  - HTTP Callback 鉴权
  - JWT Token 验证
  - IP 白名单/黑名单
  
- [ ] TLS 支持
  - RTMPS (RTMP over TLS)
  - HTTPS for HLS
  - DTLS for WebRTC
  
- [ ] 速率限制
  - 连接数限制
  - 带宽限制

#### Week 5-6: 部署和文档
- [ ] Docker 镜像
  - 多阶段构建
  - 最小镜像体积
  
- [ ] Kubernetes 部署示例
  - StatefulSet (单实例)
  - Deployment + Service (多实例)
  
- [ ] 完整文档
  - API 文档 (docs.rs)
  - 用户指南
  - 部署指南
  - 性能调优指南

### 里程碑
- ✅ 完整的监控面板 (Grafana)
- ✅ 配置文件热重载
- ✅ TLS 加密传输
- ✅ Docker 一键部署
- ✅ API 稳定，向后兼容保证

---

## 时间线总览

```
2024 Q4 (10-12月)
├── 10月: Phase 1 - RTMP 基础加固
└── 11-12月: Phase 2 - 异步化 + FLV

2025 Q1 (1-3月)
├── 1月: Phase 3 - HLS 支持
└── 2-3月: Phase 4 - SRT 协议

2025 Q2 (4-6月)
├── 4-5月: Phase 5 - WebRTC
└── 6月: Phase 6 - 生产就绪 + v1.0.0
```

---

## 关键决策点

### 决策 1: io_uring 支持时机
- **选项 A**: Phase 2 同时实现
- **选项 B**: Phase 6 作为优化项
- **建议**: 选项 B，先稳定基本架构

### 决策 2: WebRTC 实现方式
- **选项 A**: 纯 Rust 实现 (工作量巨大)
- **选项 B**: 基于 webrtc-rs 库封装
- **建议**: 选项 B，缩短开发周期

### 决策 3: 编解码器支持
- **选项 A**: 依赖 FFmpeg (功能全，体积大)
- **选项 B**: 纯 Rust 编解码器 (功能有限)
- **建议**: 初期选项 B (如 rav1e, svt-av1)，长期支持选项 A
