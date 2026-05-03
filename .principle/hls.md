# HLS 协议原理

## 一句话概括

HLS 就像一个**智能点餐系统**——菜单（M3U8）告诉你有什么菜，你按需点菜（TS/fMP4 分片），厨房按顺序上菜，支持不同档位（多码率）。

## 核心概念比喻

### 1. HLS 架构：餐厅三层结构

```
┌─────────────────────────────────────────────────────────┐
│  Master Playlist（主菜单）                                │
│  "本店提供 1080p、720p、480p 三种套餐"                      │
│                                                          │
│  ┌─────────────────┐  ┌─────────────────┐               │
│  │ 1080p 套餐      │  │ 720p 套餐       │  ...          │
│  │ 带宽: 4Mbps     │  │ 带宽: 2Mbps     │               │
│  └────────┬────────┘  └────────┬────────┘               │
└───────────┼─────────────────────┼────────────────────────┘
            │                     │
            ▼                     ▼
┌───────────────────────────────────────────────────────────┐
│  Media Playlist（子菜单）                                  │
│  "1080p 套餐包含以下 6 道菜"                                │
│                                                            │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐  │
│  │ 分片1  │ │ 分片2  │ │ 分片3  │ │ 分片4  │ │ 分片5  │  │
│  │ 6秒    │ │ 6秒    │ │ 6秒    │ │ 6秒    │ │ 6秒    │  │
│  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘  │
└───────────────────────────────────────────────────────────┘
            │
            ▼
┌───────────────────────────────────────────────────────────┐
│  Segment（菜品）                                           │
│  "每个分片是一段 6 秒的视频"                                 │
│                                                            │
│  内容: MPEG-TS 或 fMP4 格式的音视频数据                      │
└───────────────────────────────────────────────────────────┘
```

### 2. M3U8 播放列表：菜单文件

M3U8 本质是文本文件，用 `#` 开头的标签（Tag）来描述：

#### Master Playlist（主播放列表）

```
#EXTM3U                          ← 这是 M3U 文件
#EXT-X-VERSION:4                 ← 版本号

#EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1920x1080
high/index.m3u8                  ← 高清套餐入口

#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720
mid/index.m3u8                   ← 标清套餐入口

#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low/index.m3u8                   ← 流畅套餐入口
```

#### Media Playlist（媒体播放列表）

```
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:6          ← 每个分片最长 6 秒
#EXT-X-MEDIA-SEQUENCE:0          ← 第一个分片的编号

#EXTINF:6.0,                     ← 这个分片时长 6 秒
segment0.ts                      ← 分片文件名
#EXTINF:6.0,
segment1.ts
#EXTINF:6.0,
segment2.ts
#EXT-X-ENDLIST                   ← 直播结束标记（直播流没有这个）
```

### 3. 三种播放类型：餐厅营业模式

| 类型 | 比喻 | 特点 | M3U8 标记 |
|------|------|------|-----------|
| VOD | 自助餐（固定菜品） | 完整的录制视频，可任意拖拽 | `#EXT-X-PLAYLIST-TYPE:VOD` + `#EXT-X-ENDLIST` |
| Event | 宴会（持续上菜） | 直播但可回看，播放列表不断增长 | `#EXT-X-PLAYLIST-TYPE:EVENT` |
| Live | 流水席（滚动上菜） | 纯直播，旧分片被删除 | 无特殊标记，无 `#EXT-X-ENDLIST` |

### 4. 分片格式：菜品容器

#### MPEG-TS（传统格式）

```
┌────────────────────────────────────────────────┐
│  TS Packet (188字节)                            │
│  ├─ 同步字节 0x47                               │
│  ├─ PID（包类型标识）                           │
│  ├─ 调整字段                                    │
│  └─ 负载数据（音视频）                          │
└────────────────────────────────────────────────┘

多个 TS Packet 组成一个 Segment
```

#### fMP4（现代格式）

```
┌────────────────────────────────────────────────┐
│  fMP4 Segment                                  │
│  ├─ moov box（初始化信息）                      │
│  ├─ moof box（电影片段元数据）                  │
│  └─ mdat box（媒体数据）                        │
└────────────────────────────────────────────────┘

优势：更紧凑，支持 CMAF 标准
```

## HLS 直播流程

### 服务端流程

```
推流端                         HLS 服务端                       播放端
  │                              │                               │
  │ RTMP 推流                    │                               │
  │─────────────────────────────►│                               │
  │                              │                               │
  │                    ┌─────────┴─────────┐                     │
  │                    │ 1. 接收音视频帧    │                     │
  │                    │ 2. 缓存到队列      │                     │
  │                    │ 3. 每 6 秒生成分片  │                     │
  │                    │ 4. 更新播放列表    │                     │
  │                    └─────────┬─────────┘                     │
  │                              │                               │
  │                              │    HTTP 请求 M3U8             │
  │                              │◄──────────────────────────────│
  │                              │                               │
  │                              │    返回播放列表                 │
  │                              │───────────────────────────────►│
  │                              │                               │
  │                              │    HTTP 请求 TS 分片           │
  │                              │◄──────────────────────────────│
  │                              │                               │
  │                              │    返回 TS 数据                │
  │                              │───────────────────────────────►│
  │                              │                               │
  │                              │    循环请求下一个分片...        │
  │                              │◄──────────────────────────────►│
```

### 播放列表滑动窗口

直播时，播放列表是一个**滑动窗口**：

```
时间 0 秒时：
播放列表: [segment0.ts]

时间 6 秒时：
播放列表: [segment0.ts, segment1.ts]

时间 12 秒时：
播放列表: [segment0.ts, segment1.ts, segment2.ts]

时间 18 秒时（保持 3 个分片）：
播放列表: [segment1.ts, segment2.ts, segment3.ts]  ← segment0.ts 被移除
```

```rust
// m3u8.rs 中的实现
pub fn trim_segments(&mut self, max_count: usize) {
    while self.segments.len() > max_count {
        self.segments.remove(0);          // 移除最旧的分片
        self.media_sequence += 1;          // 序列号递增
    }
}
```

## Low-Latency HLS（低延迟 HLS）

传统 HLS 延迟约 3 个分片时长（18 秒），LL-HLS 将延迟降至 2-3 秒。

### 核心技术：分片预加载

```
传统 HLS：
┌─────────────────────────────────────────┐
│ 等待 6 秒 → 完成分片 → 请求 → 下载 → 播放  │
│ 总延迟: ~18 秒                           │
└─────────────────────────────────────────┘

LL-HLS：
┌─────────────────────────────────────────┐
│ 每 200ms 生成部分分片 → 即刻请求 → 下载    │
│ 总延迟: ~2-3 秒                          │
└─────────────────────────────────────────┘
```

### LL-HLS 新增标签

```
#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES
          ↑ 服务端支持阻塞式刷新

#EXT-X-PART:DURATION=0.2,URI="segment_p0.m4s",INDEPENDENT=YES
          ↑ 部分分片（200ms）

#EXT-X-PRELOAD-HINT:TYPE=PART,URI="segment_p1.m4s"
          ↑ 预加载提示（告诉客户端即将有这个部分）
```

```rust
// m3u8.rs 中的 LL-HLS 支持
pub fn for_low_latency(target_duration: Duration) -> Self {
    Self {
        low_latency: true,
        version: 6,  // LL-HLS 需要 version 6
        server_control: Some(ServerControl {
            can_block_reload: true,
            ..
        }),
        ..
    }
}
```

## 关键设计智慧

### 1. 为什么是 6 秒？

分片时长的权衡：

| 时长 | 优点 | 缺点 |
|------|------|------|
| 太短（1秒） | 延迟低 | 播放列表频繁更新，服务器压力大 |
| 太长（10秒） | 服务器轻松 | 延迟高，切换码率慢 |
| 6 秒 | 平衡 | 标准选择 |

### 2. 为什么用文本格式？

M3U8 是纯文本格式，原因：

- **可读性**：调试时直接打开看
- **兼容性**：任何 HTTP 服务器都能托管
- **简单性**：解析器实现简单

### 3. 多码率切换原理

```
客户端检测网络状况
        │
        ├── 带宽充足（> 4Mbps）→ 请求 1080p 播放列表
        │
        ├── 带宽一般（~ 2Mbps）→ 请求 720p 播放列表
        │
        └── 带宽紧张（< 1Mbps）→ 请求 480p 播放列表

切换时，根据 #EXT-X-DISCONTINUITY 标记处理不连续性
```

## 实际数据流示例

```rust
// segment.rs 中的分片结构
pub struct Segment {
    pub sequence_number: u64,     // 分片编号
    pub duration: Duration,       // 时长
    pub data: Bytes,              // TS/fMP4 数据
    pub is_independent: bool,     // 是否独立（可独立解码）
    pub program_date_time: Option<DateTime<Utc>>,  // 节目时间
}

// packager.rs 中的打包器
pub struct HlsPackager {
    target_duration: Duration,    // 目标分片时长
    current_segment: Vec<u8>,     // 当前正在构建的分片
    segment_start_time: Timestamp,
}
```

## 与其他协议对比

| 特性 | HLS | RTMP | HTTP-FLV |
|------|-----|------|----------|
| 传输协议 | HTTP | TCP | HTTP |
| 延迟 | 10-30秒 | 2-5秒 | 2-5秒 |
| 防火墙穿透 | 极好 | 一般 | 极好 |
| 多码率 | 原生支持 | 需要额外实现 | 需要额外实现 |
| 首屏速度 | 较慢 | 快 | 快 |
| iOS 原生支持 | 是 | 否 | 否 |

## 代码导航

| 文件 | 功能 | 关键结构 |
|------|------|----------|
| `mod.rs` | 配置定义 | `HlsConfig` |
| `m3u8.rs` | 播放列表生成 | `MediaPlaylist`, `MasterPlaylist`, `SegmentEntry` |
| `segment.rs` | 分片管理 | `Segment`, `MemorySegmentStorage` |
| `packager.rs` | 打包器 | `HlsPackager`, `HlsPackagerManager` |
| `server.rs` | HTTP 服务 | `HlsServer` |
