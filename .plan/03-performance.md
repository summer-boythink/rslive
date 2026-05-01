# 性能优化策略

## 当前瓶颈分析

### 1. 阻塞 I/O 架构
**现状**: 当前实现使用 `std::io::Read/Write` + `std::thread::spawn`

```rust
// 当前实现 (server.rs:195)
thread::spawn(move || {
    if let Err(e) = Self::handle_client_connection(...) {
        eprintln!("Connection {} error: {}", connection_id, e);
    }
});
```

**问题**:
- 每个连接一个线程，内存开销大 (~2MB/线程)
- 线程切换开销高
- 无法处理 C10K 问题

**目标**: 迁移到 Tokio 异步运行时

### 2. 数据复制过多
**现状**: 大量使用 `Vec<u8>` 和 `clone()`

```rust
// chunk.rs:521
let payload = state.partial_message.clone(); // 不必要的复制
```

**目标**: 使用 `Bytes`/`BytesMut` 实现零拷贝

### 3. 锁竞争
**现状**: 使用 `std::sync::Mutex` 保护共享状态

```rust
// server.rs:187
self.connections.lock().unwrap().insert(connection_id, connection.clone());
```

**目标**: 使用无锁数据结构或更高效的并发原语

---

## 优化策略矩阵

| 优化项 | 预期提升 | 难度 | 优先级 | 实施方案 |
|--------|----------|------|--------|----------|
| 异步化 (Tokio) | 10x 并发 | 中 | P0 | 全链路 async/await |
| 零拷贝 (Bytes) | 30% 吞吐 | 低 | P0 | 替换 Vec<u8> |
| 内存池化 | 50% 延迟 | 中 | P0 | object-pool crate |
| io_uring (Linux) | 2x 吞吐 | 高 | P1 | tokio-uring |
| SIMD 优化 | 20% CPU | 中 | P1 | std::simd / packed_simd |
| DPDK (可选) | 10x 吞吐 | 高 | P2 | 专用网卡场景 |

---

## 详细优化方案

### 1. Tokio 异步化改造 (P0)

#### 架构对比

**当前阻塞模型**:
```
[Main Thread] → spawn(thread per connection)
                     ↓
            [Thread Pool] - blocking I/O
                     ↓
            [Connection Handler]
```

**目标异步模型**:
```
[Tokio Runtime - Multi-thread]
         ↓
    [TcpListener]
         ↓ (accept)
    [spawn task per connection]
         ↓ (async/await)
    [Non-blocking I/O]
```

#### 迁移步骤

1. **依赖更新** (Cargo.toml)
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }
bytes = "1.5"
futures = "0.3"
```

2. **核心 trait 改造**
```rust
// 当前
pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> RtmpResult<RtmpChunk>

// 目标
pub async fn read_chunk<R: AsyncRead + Unpin>(
    &mut self, 
    reader: &mut R
) -> RtmpResult<RtmpChunk>
```

3. **服务器改造**
```rust
pub struct RtmpServer {
    // 移除 Arc<Mutex<...>>
    connections: DashMap<usize, Arc<RtmpConnection>>,
}

impl RtmpServer {
    pub async fn run(&mut self, addr: &str) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        
        loop {
            let (stream, addr) = listener.accept().await?;
            let conn_id = self.next_connection_id();
            
            // spawn 轻量级 task 而非 OS thread
            tokio::spawn(async move {
                handle_connection(conn_id, stream).await
            });
        }
    }
}
```

#### 预期收益
- **并发能力**: 从 ~1000 连接/GB RAM → ~10000+ 连接/GB RAM
- **延迟**: 减少线程切换开销
- **可扩展性**: 更好的 CPU 利用率

---

### 2. 零拷贝架构 (P0)

#### 问题场景

**当前 RTMP Chunk 处理**:
```rust
// 1. TCP 读取到 buffer
let mut data = vec![0u8; bytes_to_read];
reader.read_exact(&mut data)?;  // 复制 1: 内核 → 用户空间

// 2. 重组消息时复制
state.partial_message.extend_from_slice(&chunk.data); // 复制 2

// 3. 发送时再次复制
writer.write_all(&chunk.data)?; // 复制 3

// 总共 3 次复制！
```

#### 零拷贝方案

使用 `bytes::Bytes` (引用计数缓冲区):
```rust
use bytes::{Bytes, BytesMut, Buf, BufMut};

// 1. 从 pool 获取缓冲区
let mut buf = self.buffer_pool.get().await; // BytesMut

// 2. 读取到缓冲区
stream.read_buf(&mut buf).await?; // 直接写入，无复制

// 3. 分割数据（无复制，仅引用计数）
let chunk_data = buf.split_to(chunk_size).freeze(); // Bytes

// 4. 转发（引用计数 +1，无数据复制）
sender.send(chunk_data.clone()).await?;

// 5. 写入（直接使用底层引用）
stream.write_all(&chunk_data).await?;
```

#### 关键改造点

1. **Chunk 结构体**
```rust
// 当前
pub struct RtmpChunk {
    pub header: RtmpChunkHeader,
    pub data: Vec<u8>,
}

// 目标
pub struct RtmpChunk {
    pub header: RtmpChunkHeader,
    pub data: Bytes, // 引用计数，可克隆共享
}
```

2. **Message 结构体**
```rust
pub struct RtmpMessage {
    pub header: RtmpMessageHeader,
    pub payload: Bytes, // 多个 chunk 组合后的数据
}
```

3. **帧转发**
```rust
// 发布者 → 订阅者 零拷贝转发
pub async fn broadcast(&self, frame: MediaFrame) {
    let frame = Arc::new(frame); // 只复制 Arc 指针
    
    for subscriber in &self.subscribers {
        let frame = Arc::clone(&frame);
        subscriber.send(frame).await.ok();
    }
}
```

#### 预期收益
- **内存**: 减少 60-80% 的内存分配
- **CPU**: 减少 memcpy 开销
- **延迟**: 降低 GC 压力（虽然 Rust 无 GC，但减少 drop 开销）

---

### 3. 内存池化 (P0)

#### 问题
频繁分配/释放缓冲区导致:
- allocator 压力
- 内存碎片
- 页表抖动

#### 方案: 对象池

```rust
use object_pool::Pool;
use bytes::BytesMut;

pub struct BufferPool {
    pool: Pool<BytesMut>,
    capacity: usize,
}

impl BufferPool {
    pub fn new(size: usize, capacity: usize) -> Self {
        Self {
            pool: Pool::new(size, || BytesMut::with_capacity(capacity)),
            capacity,
        }
    }
    
    pub fn get(&self) -> Reusable<BytesMut> {
        let mut buf = self.pool.pull(|| BytesMut::with_capacity(self.capacity));
        buf.clear(); // 重置但不释放内存
        buf
    }
}
```

#### 使用场景
- TCP 读取缓冲区
- RTMP Chunk 缓冲区
- HLS Segment 缓冲区
- WebRTC RTP 包缓冲区

#### 预期收益
- **延迟稳定性**: P99 延迟降低 30%
- **内存占用**: 峰值内存更可控

---

### 4. io_uring 支持 (P1)

#### 适用场景
- Linux 5.10+ 内核
- 极高并发 (>10K 连接)
- 追求极致 I/O 性能

#### 方案
```rust
#[cfg(target_os = "linux")]
use tokio_uring::net::TcpListener;

#[cfg(not(target_os = "linux"))]
use tokio::net::TcpListener;

pub async fn run_server() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        tokio_uring::start(async {
            // 使用 io_uring 的代码
        })?;
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        // 使用标准 Tokio
    }
}
```

#### io_uring 优势
- 系统调用批量提交
- 零拷贝文件 I/O (splice)
- 减少用户/内核态切换

---

### 5. SIMD 优化 (P1)

#### 应用场景
1. **HLS TS 封装**: CRC32 计算
2. **FLV 封装**: Timestamp 转换
3. **视频处理**: 基础图像操作 (缩放、格式转换)

#### 方案
```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

pub fn fast_crc32(data: &[u8]) -> u32 {
    // 使用 SSE4.2 CRC32 指令
    unsafe {
        let mut crc: u32 = 0;
        for chunk in data.chunks_exact(8) {
            let val = u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
                chunk[4], chunk[5], chunk[6], chunk[7]
            ]);
            crc = _mm_crc32_u64(crc as u64, val) as u32;
        }
        crc
    }
}
```

---

### 6. 无锁数据结构 (P1)

#### 替换方案

| 当前 | 替换 | 场景 |
|------|------|------|
| `Mutex<HashMap>` | `DashMap` | 连接管理 |
| `Mutex<Vec>` | `crossbeam::queue::ArrayQueue` | 任务队列 |
| `Arc<Mutex<T>>` | `ArcSwap` | 配置热更新 |

#### DashMap 示例
```rust
use dashmap::DashMap;

pub struct StreamManager {
    // 替代 Mutex<HashMap<...>>
    streams: DashMap<String, StreamInfo>,
}

impl StreamManager {
    pub fn get_stream(&self, name: &str) -> Option<Ref<String, StreamInfo>> {
        self.streams.get(name) // 无锁读取
    }
    
    pub fn register_stream(&self, name: String, info: StreamInfo) {
        self.streams.insert(name, info); // 细粒度锁
    }
}
```

---

### 7. 批处理优化 (P2)

#### 发送批处理
```rust
pub struct BatchedSender<T> {
    buffer: Vec<T>,
    sender: mpsc::Sender<Vec<T>>,
    batch_size: usize,
    flush_interval: Duration,
}

impl<T> BatchedSender<T> {
    pub async fn send(&mut self, item: T) {
        self.buffer.push(item);
        
        if self.buffer.len() >= self.batch_size {
            self.flush().await;
        }
    }
    
    async fn flush(&mut self) {
        if !self.buffer.is_empty() {
            let batch = std::mem::take(&mut self.buffer);
            self.sender.send(batch).await.ok();
        }
    }
}
```

#### 收益
- 减少系统调用次数
- 提高缓存命中率

---

## 性能测试计划

### 基准测试 (Criterion)
```rust
#[bench]
fn bench_chunk_decode(b: &mut Bencher) {
    let data = create_test_chunk();
    let mut handler = RtmpChunkHandler::new(128);
    
    b.iter(|| {
        let mut cursor = Cursor::new(&data);
        black_box(handler.read_chunk(&mut cursor).unwrap());
    });
}
```

### 负载测试 (自定义)
```rust
#[tokio::test]
async fn test_concurrent_connections() {
    let server = spawn_server().await;
    
    let mut handles = vec![];
    for i in 0..10_000 {
        handles.push(tokio::spawn(async move {
            let client = connect_to_server().await?;
            publish_stream(client, generate_test_stream()).await
        }));
    }
    
    // 监控 CPU、内存、延迟
}
```

### 监控指标
- `connection_count`: 活跃连接数
- `bytes_in/bytes_out`: 吞吐率
- `frame_latency`: 帧处理延迟
- `buffer_pool_usage`: 缓冲区池使用率
- `lock_contention`: 锁竞争时间

---

## 优化检查清单

- [ ] 所有 I/O 操作改为异步
- [ ] 使用 Bytes/BytesMut 替代 Vec<u8>
- [ ] 实现缓冲区池化
- [ ] 使用 DashMap 替代 Mutex<HashMap>
- [ ] 启用 io_uring (Linux)
- [ ] SIMD 优化 CRC32 等计算
- [ ] 实现批处理发送
- [ ] 添加详细性能指标
- [ ] 基准测试覆盖关键路径
