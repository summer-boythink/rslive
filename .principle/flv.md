# FLV 协议原理

## 一句话概括

FLV 就像一个**俄罗斯套娃盒子**——最外层是文件头，里面套着一个个标签（Tag），每个标签里又装着视频或音频数据。

## 核心概念比喻

### 1. FLV 文件结构：三层套娃

```
┌─────────────────────────────────────────────────────┐
│  FLV Header（文件头）                                 │
│  "我是 FLV 文件，版本1，有视频和音频"                   │
├─────────────────────────────────────────────────────┤
│  Previous Tag Size 0（第一个标签前的占位符）           │
├─────────────────────────────────────────────────────┤
│  Tag 1（视频标签）                                    │
│  ┌───────────────────────────────────────────────┐  │
│  │ Tag Header: 类型=视频，大小=1000，时间=0ms     │  │
│  │ Tag Data: 视频数据（H.264 NAL单元）            │  │
│  └───────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────┤
│  Previous Tag Size（上一个标签的大小）                 │
├─────────────────────────────────────────────────────┤
│  Tag 2（音频标签）                                    │
│  Tag 3（音频标签）                                    │
│  Tag 4（视频标签）                                    │
│  ...                                                 │
└─────────────────────────────────────────────────────┘
```

### 2. FLV Header：身份证

每个 FLV 文件以 9 字节的头部开始，就像身份证标明身份：

```rust
// encoder.rs 中的实现
pub fn encode(&self) -> Bytes {
    let mut buf = BytesMut::with_capacity(FLV_HEADER_SIZE + PREVIOUS_TAG_SIZE);
    
    // 魔数 "FLV" - 就像身份证上的"公民"字样
    buf.extend_from_slice(FLV_HEADER_MAGIC);  // [0x46, 0x4C, 0x56]
    
    // 版本号 - 身份证版本
    buf.put_u8(self.version);  // 1
    
    // 标志位 - 个人属性（有视频？有音频？）
    buf.put_u8(self.flags.to_u8());  // 0x05 = 音频+视频
    
    // 头部大小 - 身份证长度
    buf.put_u32(self.header_size);  // 9
    
    // 第一个 PreviousTagSize 总是 0
    buf.put_u32(0);
    
    buf.freeze()
}
```

### 3. Tag：快递包裹

每个 Tag 就像一个快递包裹，包含：

```
┌────────────────────────────────────┐
│  Tag Header（快递单）               │
│  ├─ 类型（视频/音频/脚本）           │
│  ├─ 大小（包裹重量）                 │
│  ├─ 时间戳（发货时间）               │
│  └─ 流ID（快递单号）                 │
├────────────────────────────────────┤
│  Tag Data（包裹内容）               │
│  ├─ 视频标签头 + H.264数据           │
│  ├─ 音频标签头 + AAC数据             │
│  └─ 脚本数据（元数据）               │
├────────────────────────────────────┤
│  Previous Tag Size（上一个包裹大小） │
└────────────────────────────────────┘
```

### 4. Tag 类型：三种包裹

| 类型值 | 类型 | 比喻 | 内容 |
|--------|------|------|------|
| 8 | 音频 | 小件包裹 | AAC/MP3 音频帧 |
| 9 | 视频 | 大件包裹 | H.264/H.265 视频帧 |
| 18 | 脚本 | 文件袋 | onMetaData 元数据 |

### 5. 时间戳编码：巧妙的设计

FLV 时间戳采用 **混合编码**：

```
时间戳低位（3字节）  时间戳扩展（1字节）
      ↓                    ↓
[ TS << 16 | TS << 8 | TS ] [ TS >> 24 ]
```

这样设计的原因：
- **低 24 位**：可表示 0 ~ 16,777,215 毫秒（约 4.6 小时）
- **扩展 8 位**：总共 32 位，可表示约 49 天

```rust
// 时间戳编码（mod.rs）
buf.put_u8(((self.timestamp >> 16) & 0xFF) as u8);  // 高8位
buf.put_u8(((self.timestamp >> 8) & 0xFF) as u8);   // 中8位
buf.put_u8((self.timestamp & 0xFF) as u8);          // 低8位
buf.put_u8(((self.timestamp >> 24) & 0xFF) as u8);  // 扩展8位
```

## 视频 Tag 详解

### 视频标签头：5 字节精华

```
字节1: [帧类型(4bit) | 编码ID(4bit)]
字节2: AVC包类型
字节3-5: 组合时间（CTS）
后面: 实际视频数据
```

```rust
// VideoTagHeader 编码
let byte1 = ((self.frame_type as u8) << 4) | (self.codec_id as u8);
// 帧类型: 关键帧(1) 或 中间帧(2) 或 可丢弃帧(3)
// 编码ID: H.264(7) 或 H.265(12)
```

### 关键帧 vs 中间帧

想象动画片的制作：

- **关键帧（Keyframe/I帧）**：完整的画面，可以独立显示
  ```
  ┌───────────────┐
  │  完整画面      │  ← 可以直接显示
  │  (整个场景)    │
  └───────────────┘
  ```

- **中间帧（Interframe/P帧）**：只记录变化
  ```
  ┌───────────────┐
  │  只记录差异    │  ← 需要参考前一帧
  │  (人物移动)    │
  └───────────────┘
  ```

### AVC 包类型

| 类型 | 含义 | 比喻 |
|------|------|------|
| 0 | Sequence Header | 解码器说明书（SPS/PPS） |
| 1 | NALU | 实际视频画面 |
| 2 | End of Sequence | 视频结束标记 |

**Sequence Header 的重要性**：
```
就像看3D电影需要3D眼镜，解码器需要 Sequence Header 才能解码：
- SPS（Sequence Parameter Set）：图像参数
- PPS（Picture Parameter Set）：编码参数
```

## 音频 Tag 详解

### 音频标签头：2 字节精华

```
字节1: [格式(4bit) | 采样率(2bit) | 位深(1bit) | 声道(1bit)]
字节2: AAC包类型（仅AAC需要）
后面: 实际音频数据
```

```rust
// AudioTagHeader 编码
let byte1 = ((self.sound_format as u8) << 4)   // 格式: AAC(10)
          | ((self.sound_rate as u8) << 2)     // 采样率: 44kHz(3)
          | ((self.sound_size & 0x01) << 1)    // 位深: 16bit(1)
          | (self.sound_type & 0x01);          // 声道: 立体声(1)
```

### AAC 包类型

| 类型 | 含义 |
|------|------|
| 0 | AudioSpecificConfig（解码配置） |
| 1 | Raw AAC Data（音频数据） |

## Script Data：元数据宝典

Script Data 使用 AMF 编码，存储视频的"身份证信息"：

```rust
// encoder.rs 中的 ScriptData
pub struct ScriptData {
    pub duration: Option<f64>,      // 时长
    pub width: Option<f64>,         // 宽度
    pub height: Option<f64>,        // 高度
    pub frame_rate: Option<f64>,    // 帧率
    pub video_codec_id: Option<f64>,// 视频编码
    pub audio_codec_id: Option<f64>,// 音频编码
    pub encoder: Option<String>,    // 编码器名称
}
```

编码成 AMF 格式：
```
"onMetaData" + {duration: 120.0, width: 1920, height: 1080, ...}
```

## HTTP-FLV：实时流媒体

HTTP-FLV 是把 FLV 放在 HTTP 协议上传输，就像**高速公路上的特种运输车**：

```
客户端请求: GET /live/stream.flv HTTP/1.1
服务端响应: HTTP/1.1 200 OK
           Content-Type: video/x-flv
           
           [FLV Header]
           [Tag 1][Tag 2][Tag 3]...持续不断发送
```

### 与文件 FLV 的区别

| 特性 | 文件 FLV | HTTP-FLV |
|------|----------|----------|
| 结束 | 有明确结束 | 永不结束（直到直播结束） |
| 大小 | 固定 | 无限增长 |
| 定位 | 可 seek | 只能从头开始 |
| 用途 | 本地播放 | 直播流 |

## 编码流程图

```
视频帧（H.264）                         音频帧（AAC）
     │                                      │
     ▼                                      ▼
┌─────────────┐                      ┌─────────────┐
│ 视频标签头   │                      │ 音频标签头   │
│ + 帧类型     │                      │ + 格式信息   │
│ + 编码ID     │                      │ + 包类型     │
│ + CTS       │                      │             │
└──────┬──────┘                      └──────┬──────┘
       │                                    │
       ▼                                    ▼
┌─────────────────────────────────────────────────────┐
│                   Tag Header                         │
│  类型(1) + 大小(3) + 时间戳(4) + 流ID(3)             │
└─────────────────────────┬───────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────┐
│                   Previous Tag Size                  │
│                  （上一个 Tag 的总大小）               │
└─────────────────────────────────────────────────────┘
```

## 关键设计智慧

### 1. Previous Tag Size 的妙用

每个 Tag 后面跟着前一个 Tag 的大小，这不是冗余，而是为了：

- **反向解析**：可以从文件末尾向前解析
- **错误恢复**：解析出错时可以跳到下一个 Tag

### 2. 时间戳的扩展机制

24 位基础 + 8 位扩展 = 32 位总时间戳，这是向后兼容的典范：
- 老播放器：只读 24 位，支持 4.6 小时
- 新播放器：读取扩展位，支持 49 天

### 3. Sequence Header 的缓存

```rust
// encoder.rs 中的智慧
if data.len() > 5 && data[1] == 0 {
    // 这是 AVC Sequence Header，缓存起来
    self.sequence_headers_sent.video = Some(data.clone());
}
```

新观众加入时，先发送缓存的 Sequence Header，再发送当前帧，确保解码器能正常工作。

## 代码导航

| 文件 | 功能 | 关键结构 |
|------|------|----------|
| `mod.rs` | 格式定义 | `FlvHeader`, `FlvTagHeader`, `VideoTagHeader`, `AudioTagHeader` |
| `encoder.rs` | 编码器 | `FlvEncoder`, `ScriptData` |
| `decoder.rs` | 解码器 | `FlvDecoder` |
| `writer.rs` | 文件写入 | `FlvWriter` |
| `http_server.rs` | HTTP-FLV 服务 | `HttpFlvServer` |
