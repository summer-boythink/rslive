# RTMP 协议原理

## 一句话概括

RTMP 就像**快递公司的智能分拣系统**——把大包裹拆成小箱子，贴上标签，通过不同流水线高效运送，最后在目的地重新组装。

## 核心概念比喻

### 1. 握手过程：三人接头暗号

想象两个特工见面，需要确认对方身份：

```
客户端：抛出一个橙子（C0: 版本号 3）+ 一张写有随机密码的纸条（C1: 1536字节随机数据）
服务端：接过橙子，确认是同一个品种（S0: 版本匹配）
        然后回传一张写有自己密码的纸条（S1）
        再把客户端的纸条原样返回（S2: "你的暗号我收到了"）
客户端：把服务端的纸条原样返回（C2: "握手成功，开始正事"）
```

这就是经典的 **C0/C1/C2 和 S0/S1/S2** 六步握手。代码中 `handshake.rs` 实现了这个过程：

```rust
// 客户端发送 C0 + C1
writer.write_u8(RTMP_VERSION)?;           // C0: 一个字节版本号
writer.write_u32::<BigEndian>(timestamp)?;  // C1: 时间戳
writer.write_u32::<BigEndian>(0)?;          // C1: 零字段
writer.write_all(&self.random_bytes)?;      // C1: 随机数据
```

### 2. Chunk（块）：集装箱运输

RTMP 的核心智慧在于 **Chunk（分块）**。想象你要运送一辆汽车：

- **不分块**：整辆车上路，占用整条高速公路，其他车都得等着
- **分块后**：拆成零件，装进标准化集装箱，与其他货物混装运输

```
原始消息（可能很大，比如 10MB 的视频帧）
    ↓
切分成多个 Chunk（默认每个 128 字节）
    ↓
每个 Chunk 可以走不同的"流水线"（Chunk Stream）
    ↓
接收方按序号重新组装
```

#### Chunk Header 的四种格式（精妙的压缩）

就像快递单可以简化填写：

| 格式 | 字节数 | 比喻 | 使用场景 |
|------|--------|------|----------|
| Type 0 | 11字节 | 完整快递单 | 第一个包，写清所有信息 |
| Type 1 | 7字节 | 省略收件人 | 同一流水线的后续包，沿用上一个的收件人 |
| Type 2 | 3字节 | 只写时间 | 连内容类型都和之前一样，只更新时间 |
| Type 3 | 0字节 | 空白单 | 所有信息都和前一个完全一样，只传数据 |

```rust
// chunk.rs 中的实现
match format {
    RTMP_MESSAGE_HEADER_SIZE_12 => { /* Type 0: 完整头 */ }
    RTMP_MESSAGE_HEADER_SIZE_8  => { /* Type 1: 省略流ID */ }
    RTMP_MESSAGE_HEADER_SIZE_4  => { /* Type 2: 只有时间差 */ }
    RTMP_MESSAGE_HEADER_SIZE_1  => { /* Type 3: 无头部 */ }
}
```

### 3. 消息类型：快递分类

RTMP 定义了多种消息类型，就像快递公司的包裹分类：

```
┌─────────────────────────────────────────────────────┐
│  控制消息（1-7）     →  系统指令，如"改变集装箱大小"  │
│  音频消息（8）       →  音频包裹                      │
│  视频消息（9）       →  视频包裹                      │
│  AMF命令（17/20）    →  元数据包裹（JSON类似的格式）   │
└─────────────────────────────────────────────────────┘
```

### 4. AMF 命令：远程控制面板

AMF（Action Message Format）是 RTMP 的"语言"，用于发送命令：

```
客户端 → 服务端                          服务端 → 客户端
─────────────────                        ─────────────────
connect    我要连接这个应用               _result      连接成功
createStream  创建一个流通道              _result      流ID是1
publish    我要开始直播                   onStatus     可以开始
play       我要观看这个流                 onStatus     开始播放
```

## 数据流动图

```
发送方                                    接收方
  │                                         │
  │  1. 构建 AMF 命令对象                    │
  │     AmfCommand::connect(...)            │
  │              │                          │
  │  2. 编码为 RTMP Message                  │
  │     RtmpMessage::create_amf0_command()  │
  │              │                          │
  │  3. 切分成 Chunks                        │
  │     chunk_handler.create_chunks()       │
  │              │                          │
  │     ┌────────────────────────┐          │
  │     │ Chunk 1 │ Chunk 2 │... │  ──────► │
  │     └────────────────────────┘          │
  │                                         │  4. 重组 Message
  │                                         │     process_chunk()
  │                                         │
  │                                         │  5. 解析 AMF 命令
  │                                         │     parse_amf0_command()
  │                                         │
  ▼                                         ▼
```

## 关键设计智慧

### 1. 为什么是 128 字节？

默认 Chunk 大小是 128 字节，这是精心设计的：

- **太小**（如 16 字节）：头部开销太大，效率低
- **太大**（如 4096 字节）：延迟高，不能及时传输紧急数据
- **128 字节**：平衡了效率和延迟

### 2. 时间戳的巧妙设计

RTMP 使用 **时间戳增量** 而非绝对时间戳：

```rust
// 第一个 Chunk：绝对时间戳
timestamp: 1000

// 后续 Chunk：时间戳增量
timestamp_delta: 0  // 同一帧的不同部分
```

当时间戳超过 24 位最大值（16777215 毫秒 ≈ 4.6 小时）时，使用 **Extended Timestamp** 扩展：

```rust
if timestamp >= 0xFFFFFF {
    header.timestamp = 0xFFFFFF;  // 标记位
    header.extended_timestamp = Some(timestamp);  // 真实时间戳
}
```

### 3. Chunk Stream ID 的压缩

Chunk Stream ID 的编码也充满智慧：

```
0-63:     1字节（直接编码）
64-319:   2字节（第一个字节=0，第二个字节=ID-64）
320-65599: 3字节（第一个字节=1，后面两个字节编码）
```

## 实际应用场景

```
直播推流：
OBS/ffmpeg → RTMP推流 → RTMP服务器 → 分发给观众

流程：
1. TCP连接建立
2. RTMP握手（C0/C1 → S0/S1 → C2 → S2）
3. connect命令建立应用连接
4. createStream创建流通道
5. publish开始推流
6. 持续发送音视频Chunk
7. 音视频数据被服务器转发给播放者
```

## 代码导航

| 文件 | 功能 | 关键结构 |
|------|------|----------|
| `mod.rs` | 协议常量定义 | `RTMP_VERSION`, `message_type` |
| `handshake.rs` | 握手实现 | `RtmpHandshake` |
| `chunk.rs` | 分块处理 | `RtmpChunkHandler`, `RtmpChunk` |
| `message.rs` | 消息定义 | `RtmpMessage`, `AmfCommand` |
| `connection.rs` | 连接状态管理 | `RtmpConnection` |
| `server.rs` | 服务端实现 | `RtmpServer` |
| `client.rs` | 客户端实现 | `RtmpClient` |
