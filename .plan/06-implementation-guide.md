# 实施指南

## 如何开始

### 第一步：环境准备

```bash
# 安装 Rust (如果尚未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装必要工具
rustup component add clippy rustfmt
rustup target add x86_64-unknown-linux-gnu

# 安装 cargo-watch (开发热重载)
cargo install cargo-watch
```

### 第二步：创建项目结构

```bash
# 当前目录结构
rslive/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs
│   └── protocol/
│       ├── mod.rs
│       ├── amf0/
│       ├── amf3/
│       └── rtmp/
├── examples/
└── .plan/

# 目标结构
rslive/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── CHANGELOG.md
├── LICENSE
├── src/
│   ├── lib.rs                    # 库入口
│   ├── bin/                      # 可执行文件
│   │   └── server.rs             # rslive-server 二进制
│   ├── protocol/                 # 协议实现
│   │   ├── mod.rs
│   │   ├── amf0/
│   │   ├── amf3/
│   │   ├── rtmp/
│   │   ├── flv/                  # 新增
│   │   ├── hls/                  # 新增
│   │   ├── srt/                  # 新增
│   │   └── webrtc/               # 新增
│   ├── media/                    # 媒体处理 (新增)
│   │   ├── mod.rs
│   │   ├── frame.rs              # MediaFrame 定义
│   │   ├── router.rs             # 流路由
│   │   └── codec/                # 编解码抽象
│   ├── server/                   # 服务器框架 (新增)
│   │   ├── mod.rs
│   │   ├── config.rs             # 配置管理
│   │   └── metrics.rs            # 监控指标
│   └── util/                     # 工具函数 (新增)
│       ├── mod.rs
│       ├── bytes.rs              # 字节处理
│       └── pool.rs               # 对象池
├── examples/
│   ├── simple_server.rs
│   ├── simple_publisher.rs
│   └── simple_player.rs
├── tests/                        # 集成测试
│   └── integration_tests.rs
├── benches/                      # 基准测试
│   └── chunk_benchmark.rs
└── .plan/                        # 规划文档 (已创建)
```

### 第三步：更新 Cargo.toml

```toml
[package]
name = "rslive"
version = "0.1.0"
edition = "2024"
authors = ["Your Name <your.email@example.com>"]
description = "High-performance live streaming library in Rust"
license = "MIT OR Apache-2.0"
repository = "https://github.com/yourusername/rslive"
keywords = ["streaming", "rtmp", "hls", "webrtc", "media"]
categories = ["network-programming", "multimedia"]
rust-version = "1.75"

[features]
default = ["rtmp", "flv", "hls"]
rtmp = []
flv = ["rtmp"]
hls = []
srt = []
webrtc = []
full = ["rtmp", "flv", "hls", "srt", "webrtc"]

[dependencies]
# 异步运行时
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec", "net"] }
tokio-stream = "0.1"

# 字节处理
bytes = "1.5"
bytesize = "1.3"

# 并发
parking_lot = "0.12"
dashmap = "6"
crossbeam = "0.8"
flume = "0.11"

# 序列化
byteorder = "1.5"

# 错误处理
thiserror = "1.0"
anyhow = "1.0"

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 配置
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# 网络
socket2 = "0.5"

# 时间
chrono = "0.4"

# 集合
indexmap = "2"

# HTTP (for HLS/HTTP-FLV)
axum = { version = "0.7", optional = true }
tower = { version = "0.4", optional = true }

# 指标
metrics = { version = "0.23", optional = true }
metrics-exporter-prometheus = { version = "0.14", optional = true }

# 对象池 (未来使用)
# object-pool = "0.6"

[dev-dependencies]
tokio-test = "0.4"
criterion = { version = "0.5", features = ["async_tokio"] }
pretty_assertions = "1.4"

[[bin]]
name = "rslive-server"
path = "src/bin/server.rs"

[[bench]]
name = "chunk_benchmark"
harness = false
```

---

## 开发工作流

### 1. 代码风格

```bash
# 格式化代码
cargo fmt

# 检查代码
cargo clippy -- -D warnings

# 运行测试
cargo test

# 检查所有功能
cargo check --all-features
```

### 2. 提交规范

使用 [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(rtmp): implement async chunk reading
fix(server): resolve broadcast race condition
docs(readme): update installation guide
perf(chunk): reduce memory allocations
refactor(amf0): simplify encoder logic
test(hls): add segment generation tests
```

### 3. 分支策略

```
main          → 稳定分支，可发布
  ↓
develop       → 开发分支，功能集成
  ↓
feature/xxx   → 功能分支
hotfix/xxx    → 紧急修复
release/xxx   → 发布准备
```

---

## 快速开始模板

### 创建一个简单的 RTMP 服务器

```rust
// examples/simple_server.rs
use rslive::rtmp::RtmpServer;
use tracing::{info, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    // 创建服务器
    let server = RtmpServer::builder()
        .bind("0.0.0.0:1935")
        .on_connect(|conn_id, app| {
            info!("Client {} connected to app: {}", conn_id, app);
            true // 允许连接
        })
        .on_publish(|conn_id, stream_name| {
            info!("Client {} publishing stream: {}", conn_id, stream_name);
            true // 允许发布
        })
        .on_play(|conn_id, stream_name| {
            info!("Client {} playing stream: {}", conn_id, stream_name);
            true // 允许播放
        })
        .build();
    
    info!("RTMP Server starting on rtmp://0.0.0.0:1935");
    
    // 运行服务器
    if let Err(e) = server.run().await {
        error!("Server error: {}", e);
    }
    
    Ok(())
}
```

### 运行示例

```bash
# 启动服务器
cargo run --example simple_server

# 使用 FFmpeg 推流测试
ffmpeg -re -i test.mp4 -c copy -f flv rtmp://localhost:1935/live/stream1

# 使用 FFplay 播放测试
ffplay rtmp://localhost:1935/live/stream1
```

---

## 关键实现模式

### 1. 异步状态机

```rust
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub enum ConnectionState {
    Handshaking(HandshakingState),
    Connected(ConnectedState),
    Publishing(PublishingState),
    Playing(PlayingState),
    Closed,
}

impl Future for Connection {
    type Output = Result<()>;
    
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match &mut self.state {
                ConnectionState::Handshaking(state) => {
                    match state.poll(cx)? {
                        Poll::Ready(()) => {
                            self.state = ConnectionState::Connected(ConnectedState::new());
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                ConnectionState::Connected(state) => {
                    match state.poll(cx)? {
                        Poll::Ready(next) => self.state = next,
                        Poll::Pending => return Poll::Pending,
                    }
                }
                // ... 其他状态
                ConnectionState::Closed => return Poll::Ready(Ok(())),
            }
        }
    }
}
```

### 2. 背压处理

```rust
use tokio::sync::mpsc;

pub struct StreamPublisher {
    sender: mpsc::Sender<MediaFrame>,
    config: PublisherConfig,
}

impl StreamPublisher {
    pub async fn publish(&mut self, frame: MediaFrame) -> Result<()> {
        // 使用 try_send 检查背压
        match self.sender.try_send(frame) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(frame)) => {
                // 缓冲区满，根据策略处理
                match self.config.backpressure_strategy {
                    BackpressureStrategy::Drop => {
                        // 丢弃旧帧
                        self.sender.recv().await.ok();
                        self.sender.send(frame).await.map_err(|_| Error::ChannelClosed)
                    }
                    BackpressureStrategy::Block => {
                        // 阻塞等待
                        self.sender.send(frame).await.map_err(|_| Error::ChannelClosed)
                    }
                    BackpressureStrategy::DropNew => {
                        // 丢弃新帧（适合实时场景）
                        Ok(())
                    }
                }
            }
            Err(e) => Err(Error::ChannelClosed),
        }
    }
}
```

### 3. 零拷贝转发

```rust
use bytes::Bytes;
use std::sync::Arc;

pub struct MediaFrame {
    pub timestamp: Duration,
    pub data: Bytes,  // 引用计数，可安全克隆
    pub codec: CodecType,
    pub is_keyframe: bool,
}

pub struct StreamRouter {
    subscribers: DashMap<String, Vec<mpsc::Sender<Arc<MediaFrame>>>>,
}

impl StreamRouter {
    pub async fn broadcast(&self, stream_name: &str, frame: MediaFrame) {
        let frame = Arc::new(frame);  // 只增加引用计数
        
        if let Some(subs) = self.subscribers.get(stream_name) {
            for sender in subs.value() {
                // 发送 Arc 指针，无数据复制
                let _ = sender.send(Arc::clone(&frame)).await;
            }
        }
    }
}
```

---

## 测试策略

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chunk_encode_decode() {
        let chunk = RtmpChunk {
            header: RtmpChunkHeader {
                format: 0,
                chunk_stream_id: 4,
                timestamp: 1000,
                message_length: 100,
                message_type_id: 8,  // Audio
                message_stream_id: 1,
                extended_timestamp: None,
            },
            data: Bytes::from(vec![0xAF; 100]),
        };
        
        let mut buf = BytesMut::new();
        chunk.encode(&mut buf).unwrap();
        
        let decoded = RtmpChunk::decode(&mut buf.freeze()).unwrap();
        assert_eq!(chunk.header, decoded.header);
        assert_eq!(chunk.data, decoded.data);
    }
    
    #[tokio::test]
    async fn test_server_accept_connection() {
        let server = RtmpServer::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr();
        
        // 启动服务器
        tokio::spawn(async move {
            server.run().await.unwrap();
        });
        
        // 连接测试
        let client = TcpStream::connect(addr).await.unwrap();
        assert!(client.peer_addr().is_ok());
    }
}
```

### 集成测试

```rust
// tests/integration_tests.rs
use rslive::rtmp::RtmpServer;

#[tokio::test]
async fn test_publish_and_play() {
    // 启动服务器
    let server = RtmpServer::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr();
    
    tokio::spawn(async move {
        server.run().await.unwrap();
    });
    
    // 等待服务器就绪
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // 创建发布者
    let mut publisher = RtmpClient::connect(addr).await.unwrap();
    publisher.publish("test_stream").await.unwrap();
    
    // 创建播放器
    let mut player = RtmpClient::connect(addr).await.unwrap();
    player.play("test_stream").await.unwrap();
    
    // 发布数据
    let frame = VideoFrame::test_keyframe();
    publisher.send_video(frame).await.unwrap();
    
    // 验证播放器收到数据
    let received = player.recv_video().await.unwrap();
    assert!(received.is_keyframe);
}
```

### 基准测试

```rust
// benches/chunk_benchmark.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rslive::rtmp::RtmpChunk;

fn bench_chunk_encode(c: &mut Criterion) {
    let chunk = create_test_chunk(1024);
    
    c.bench_function("chunk_encode", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            black_box(chunk.encode(&mut buf)).unwrap();
        });
    });
}

fn bench_chunk_decode(c: &mut Criterion) {
    let chunk = create_test_chunk(1024);
    let mut buf = BytesMut::new();
    chunk.encode(&mut buf).unwrap();
    let data = buf.freeze();
    
    c.bench_function("chunk_decode", |b| {
        b.iter(|| {
            let mut cursor = std::io::Cursor::new(&data);
            black_box(RtmpChunk::decode(&mut cursor)).unwrap();
        });
    });
}

criterion_group!(benches, bench_chunk_encode, bench_chunk_decode);
criterion_main!(benches);
```

---

## 调试技巧

### 1. 使用 tracing 进行结构化日志

```rust
use tracing::{info, debug, instrument};

#[instrument(skip(self, data))]
pub async fn process_chunk(&mut self, data: Bytes) -> Result<()> {
    debug!(chunk_size = data.len(), "Processing chunk");
    
    // ... 处理逻辑
    
    info!(stream_id, "Stream started");
    Ok(())
}
```

### 2. 性能分析

```bash
# 使用 flamegraph 生成火焰图
cargo install flamegraph
sudo cargo flamegraph --bin rslive-server

# 使用 perf 分析
perf record --call-graph dwarf cargo run --release
perf report
```

### 3. 内存分析

```bash
# 使用 valgrind (Linux)
valgrind --tool=memcheck --leak-check=full ./target/release/rslive-server

# 使用 heaptrack (Linux)
heaptrack ./target/release/rslive-server
heaptrack_gui heaptrack.rslive-server.xxx.gz
```

---

## 部署指南

### Docker 部署

```dockerfile
# Dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin rslive-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/rslive-server /usr/local/bin/
EXPOSE 1935 8080
CMD ["rslive-server", "--config", "/etc/rslive/config.toml"]
```

```bash
# 构建镜像
docker build -t rslive:latest .

# 运行容器
docker run -d \
  -p 1935:1935 \
  -p 8080:8080 \
  -v /path/to/config:/etc/rslive \
  rslive:latest
```

### Kubernetes 部署

```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: rslive
spec:
  replicas: 3
  selector:
    matchLabels:
      app: rslive
  template:
    metadata:
      labels:
        app: rslive
    spec:
      containers:
      - name: rslive
        image: rslive:latest
        ports:
        - containerPort: 1935
          name: rtmp
        - containerPort: 8080
          name: http
        resources:
          requests:
            memory: "128Mi"
            cpu: "100m"
          limits:
            memory: "1Gi"
            cpu: "1000m"
---
apiVersion: v1
kind: Service
metadata:
  name: rslive
spec:
  selector:
    app: rslive
  ports:
  - port: 1935
    targetPort: 1935
    name: rtmp
  - port: 8080
    targetPort: 8080
    name: http
```

---

## 社区贡献指南

### 如何贡献

1. **Fork 项目**
2. **创建功能分支**: `git checkout -b feature/my-feature`
3. **提交更改**: `git commit -am 'feat: add some feature'`
4. **推送到分支**: `git push origin feature/my-feature`
5. **创建 Pull Request**

### 代码审查清单

- [ ] 代码通过 `cargo clippy` 检查
- [ ] 代码通过 `cargo fmt` 格式化
- [ ] 新增功能包含单元测试
- [ ] 所有测试通过 `cargo test`
- [ ] 文档已更新（README、API docs）
- [ ] CHANGELOG.md 已更新

---

## 资源链接

- **RTMP 规范**: https://rtmp.veriskope.com/
- **HLS 规范**: https://datatracker.ietf.org/doc/html/rfc8216
- **SRT 文档**: https://srtlab.github.io/srt-docs/
- **WebRTC 规范**: https://www.w3.org/TR/webrtc/
- **Tokio 文档**: https://tokio.rs/
- **Rust 异步编程**: https://rust-lang.github.io/async-book/

---

## 下一步行动

1. ✅ **阅读规划文档**: 已完成
2. ⬜ **设置开发环境**: 安装 Rust 和工具
3. ⬜ **创建项目结构**: 按照指南创建目录
4. ⬜ **实现异步化**: 将现有 RTMP 代码迁移到 Tokio
5. ⬜ **编写测试**: 确保现有功能通过测试
6. ⬜ **性能基准**: 建立性能基线
7. ⬜ **开始 Phase 1**: 完善 RTMP 核心功能
