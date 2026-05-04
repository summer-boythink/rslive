# rslive-server 使用指南

## 快速开始

### 1. 启动服务器

```bash
# 默认启动
./rslive-server

# 或者自定义端口
./rslive-server --rtmp-port 1935 --hls-port 8080

# 低延迟模式（适合直播互动场景）
./rslive-server --low-latency
```

启动后会看到：
```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│   🚀 rslive-server v0.1.0                                   │
│                                                             │
│   High-performance streaming server                         │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  Protocol    │  Bind Address                                │
├──────────────┼──────────────────────────────────────────────┤
│  RTMP        │  rtmp://0.0.0.0:1935                         │
│  HLS         │  http://0.0.0.0:8080                         │
│  HTTP-FLV    │  http://0.0.0.0:8081                         │
└─────────────────────────────────────────────────────────────┘
```

---

## FFmpeg 推流

### 基础推流

```bash
# 推送到 rtmp://localhost:1935/live/stream1
ffmpeg -re -i input.mp4 \
    -c:v libx264 -preset veryfast -b:v 2000k \
    -c:a aac -b:a 128k \
    -f flv rtmp://localhost:1935/live/stream1
```

### 关键参数说明

| 参数 | 说明 |
|------|------|
| `-re` | 按输入帧率读取（模拟直播） |
| `-c:v libx264` | 使用 H.264 编码（HLS 必需） |
| `-preset veryfast` | 编码速度预设 |
| `-b:v 2000k` | 视频码率 2Mbps |
| `-c:a aac` | 使用 AAC 音频编码 |
| `-f flv` | 输出格式 FLV（RTMP 封装） |

### 多码率推流（ABR）

```bash
# 720p 高质量
ffmpeg -re -i input.mp4 \
    -c:v libx264 -preset veryfast -b:v 3000k -s 1280x720 \
    -c:a aac -b:a 128k \
    -f flv rtmp://localhost:1935/live/stream1_720p

# 480p 中质量
ffmpeg -re -i input.mp4 \
    -c:v libx264 -preset veryfast -b:v 1000k -s 854x480 \
    -c:a aac -b:a 96k \
    -f flv rtmp://localhost:1935/live/stream1_480p

# 360p 低质量
ffmpeg -re -i input.mp4 \
    -c:v libx264 -preset veryfast -b:v 500k -s 640x360 \
    -c:a aac -b:a 64k \
    -f flv rtmp://localhost:1935/live/stream1_360p
```

### 摄像头直播

```bash
# macOS 使用 AVFoundation
ffmpeg -f avfoundation -framerate 30 -video_size 1280x720 -i "0:0" \
    -c:v libx264 -preset ultrafast -b:v 2000k \
    -c:a aac -b:a 128k \
    -f flv rtmp://localhost:1935/live/camera

# Linux 使用 V4L2
ffmpeg -f v4l2 -framerate 30 -video_size 1280x720 -i /dev/video0 \
    -f pulse -i default \
    -c:v libx264 -preset ultrafast -b:v 2000k \
    -c:a aac -b:a 128k \
    -f flv rtmp://localhost:1935/live/camera

# Windows 使用 DirectShow
ffmpeg -f dshow -i video="Integrated Webcam":audio="Microphone" \
    -c:v libx264 -preset ultrafast -b:v 2000k \
    -c:a aac -b:a 128k \
    -f flv rtmp://localhost:1935/live/camera
```

### 屏幕录制

```bash
# macOS 屏幕录制
ffmpeg -f avfoundation -i "3:none" -r 30 \
    -c:v libx264 -preset ultrafast \
    -f flv rtmp://localhost:1935/live/screen

# Linux 屏幕录制
ffmpeg -f x11grab -r 30 -s 1920x1080 -i :0.0 \
    -c:v libx264 -preset ultrafast \
    -f flv rtmp://localhost:1935/live/screen
```

---

## Web 播放

### 1. 播放 HLS 流

#### 使用 hls.js（推荐）

```html
<!DOCTYPE html>
<html>
<head>
    <title>HLS Player</title>
    <script src="https://cdn.jsdelivr.net/npm/hls.js@1.4.0/dist/hls.min.js"></script>
    <style>
        body { margin: 0; background: #000; display: flex; justify-content: center; align-items: center; height: 100vh; }
        video { width: 100%; max-width: 1280px; }
        .info { position: absolute; top: 10px; left: 10px; color: #fff; font-family: monospace; }
    </style>
</head>
<body>
    <div class="info" id="info">Loading...</div>
    <video id="video" controls autoplay muted></video>

    <script>
        const video = document.getElementById('video');
        const info = document.getElementById('info');

        // HLS 播放列表 URL
        const hlsUrl = 'http://localhost:8080/hls/live/stream1/index.m3u8';

        if (Hls.isSupported()) {
            const hls = new Hls({
                debug: false,
                enableWorker: true,
                lowLatencyMode: false, // 设为 true 启用低延迟模式
                backBufferLength: 90,
            });

            hls.loadSource(hlsUrl);
            hls.attachMedia(video);

            hls.on(Hls.Events.MANIFEST_PARSED, function() {
                info.textContent = 'Stream loaded, starting playback...';
                video.play();
            });

            hls.on(Hls.Events.ERROR, function(event, data) {
                info.textContent = 'Error: ' + data.type;
                console.error('HLS Error:', data);
            });

            hls.on(Hls.Events.LEVEL_SWITCHED, function(event, data) {
                info.textContent = 'Quality: ' + data.level;
            });

        } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
            // Safari 原生支持 HLS
            video.src = hlsUrl;
            video.addEventListener('loadedmetadata', function() {
                video.play();
            });
        } else {
            info.textContent = 'HLS not supported in this browser';
        }
    </script>
</body>
</html>
```

#### 使用 Video.js

```html
<!DOCTYPE html>
<html>
<head>
    <title>Video.js HLS Player</title>
    <link href="https://vjs.zencdn.net/8.3.0/video-js.css" rel="stylesheet" />
    <script src="https://vjs.zencdn.net/8.3.0/video.min.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/videojs-contrib-hls@5.15.0/dist/videojs-contrib-hls.min.js"></script>
</head>
<body>
    <video
        id="my-video"
        class="video-js vjs-default-skin vjs-big-play-centered"
        controls
        preload="auto"
        width="1280"
        height="720"
        data-setup='{}'>
        <source src="http://localhost:8080/hls/live/stream1/index.m3u8" type="application/x-mpegURL">
    </video>

    <script>
        var player = videojs('my-video', {
            html5: {
                hls: {
                    overrideNative: true,
                    limitRenditionByPlayerDimensions: true,
                    smoothQualityChange: true,
                }
            }
        });
        player.play();
    </script>
</body>
</html>
```

### 2. 多码率自适应播放

```html
<!DOCTYPE html>
<html>
<head>
    <title>Multi-Quality HLS Player</title>
    <script src="https://cdn.jsdelivr.net/npm/hls.js@1.4.0/dist/hls.min.js"></script>
    <style>
        body { margin: 0; background: #000; display: flex; flex-direction: column; align-items: center; padding: 20px; }
        video { width: 100%; max-width: 1280px; }
        .controls { margin-top: 10px; color: #fff; font-family: monospace; }
        select { padding: 5px 10px; font-size: 14px; }
    </style>
</head>
<body>
    <video id="video" controls autoplay muted></video>
    <div class="controls">
        <label>Quality: </label>
        <select id="quality">
            <option value="-1">Auto</option>
        </select>
        <span id="stats"></span>
    </div>

    <script>
        const video = document.getElementById('video');
        const qualitySelect = document.getElementById('quality');
        const stats = document.getElementById('stats');

        const hls = new Hls({
            enableWorker: true,
            lowLatencyMode: false,
        });

        hls.loadSource('http://localhost:8080/hls/live/stream1/index.m3u8');
        hls.attachMedia(video);

        hls.on(Hls.Events.MANIFEST_PARSED, function(event, data) {
            console.log('Manifest parsed, levels:', data.levels);

            // 填充质量选择器
            data.levels.forEach((level, index) => {
                const option = document.createElement('option');
                option.value = index;
                option.textContent = `${level.height}p (${Math.round(level.bitrate / 1000)}kbps)`;
                qualitySelect.appendChild(option);
            });

            video.play();
        });

        // 手动切换质量
        qualitySelect.addEventListener('change', function() {
            const level = parseInt(this.value);
            hls.currentLevel = level; // -1 = auto, 0+ = specific level
        });

        // 显示统计信息
        setInterval(() => {
            if (hls) {
                const level = hls.currentLevel;
                const loadLevel = hls.loadLevel;
                stats.textContent = `Current: ${level}, Loading: ${loadLevel}`;
            }
        }, 1000);
    </script>
</body>
</html>
```

### 3. 低延迟直播 (LL-HLS)

```html
<!DOCTYPE html>
<html>
<head>
    <title>Low-Latency HLS Player</title>
    <script src="https://cdn.jsdelivr.net/npm/hls.js@1.4.0/dist/hls.min.js"></script>
    <style>
        body { margin: 0; background: #000; display: flex; flex-direction: column; align-items: center; padding: 20px; }
        video { width: 100%; max-width: 1280px; }
        .latency-info { color: #0f0; font-family: monospace; margin-top: 10px; }
    </style>
</head>
<body>
    <video id="video" controls autoplay muted playsinline></video>
    <div class="latency-info" id="latency">Measuring latency...</div>

    <script>
        const video = document.getElementById('video');
        const latencyInfo = document.getElementById('latency');

        const hls = new Hls({
            enableWorker: true,
            lowLatencyMode: true,           // 启用低延迟模式
            backBufferLength: 10,            // 减少缓冲
            liveSyncDurationCount: 2,        // 同步到最新 2 个片段
            liveMaxLatencyDurationCount: 5,  // 最大延迟 5 个片段
            maxBufferLength: 10,             // 最大缓冲 10 秒
            maxMaxBufferLength: 15,          // 绝对最大缓冲 15 秒
        });

        hls.loadSource('http://localhost:8080/hls/live/stream1/index.m3u8');
        hls.attachMedia(video);

        hls.on(Hls.Events.MANIFEST_PARSED, function() {
            video.play();
        });

        // 显示延迟统计
        setInterval(() => {
            if (video.readyState >= 2) {
                const buffered = video.buffered;
                if (buffered.length > 0) {
                    const bufferEnd = buffered.end(buffered.length - 1);
                    const currentTime = video.currentTime;
                    const latency = bufferEnd - currentTime;
                    latencyInfo.textContent = `Buffer Latency: ${latency.toFixed(2)}s | ` +
                                              `Buffer Length: ${(bufferEnd - buffered.start(0)).toFixed(2)}s`;
                }
            }
        }, 500);
    </script>
</body>
</html>
```

---

## 完整工作流程

### 场景 1: 直播推流 + Web 播放

> ⚠️ **重要提示**：HLS 流需要**先推流**才能播放！如果直接访问 HLS URL 会返回 404。

```bash
# 1. 启动服务器
./rslive-server

# 2. 使用 FFmpeg 推流（必须先执行这步！）
ffmpeg -re -i input.mp4 -c:v libx264 -c:a aac -f flv rtmp://localhost:1935/live/stream1

# 3. 在浏览器打开 player.html（使用上面的代码）
#    访问 http://localhost:8080/hls/live/stream1/index.m3u8
```

### 场景 2: 多码率 ABR 直播

```bash
# 1. 启动服务器（低延迟模式）
./rslive-server --low-latency

# 2. 启动多个 FFmpeg 推不同码率
ffmpeg -re -i input.mp4 -c:v libx264 -b:v 3000k -s 1280x720 -c:a aac -f flv rtmp://localhost:1935/live/stream1_720p &
ffmpeg -re -i input.mp4 -c:v libx264 -b:v 1000k -s 854x480 -c:a aac -f flv rtmp://localhost:1935/live/stream1_480p &

# 3. 使用多码率播放器（上面的多码率自适应播放示例）
```

### 场景 3: 实时监控

```bash
# 1. 启动服务器
./rslive-server --hls-port 8080

# 2. 摄像头推流
ffmpeg -f avfoundation -framerate 30 -video_size 1280x720 -i "0:0" \
    -c:v libx264 -preset ultrafast -tune zerolatency -b:v 2000k \
    -c:a aac -b:a 128k \
    -f flv rtmp://localhost:1935/live/camera

# 3. 浏览器打开监控页面
#    播放 http://localhost:8080/hls/live/camera/index.m3u8
```

---

## API 端点

### HLS 相关

| 端点 | 说明 |
|------|------|
| `GET /hls/{stream}/master.m3u8` | 主播放列表（多码率） |
| `GET /hls/{stream}/index.m3u8` | 媒体播放列表 |
| `GET /hls/{stream}/segment/{idx}` | 媒体段（TS 或 fMP4） |
| `GET /health` | 健康检查 |

**注意**：`{stream}` 是流名称，例如 `live/stream1`

### 示例流地址

```
# 推流地址
rtmp://localhost:1935/live/stream1

# 播放地址（HLS）
http://localhost:8080/hls/live/stream1/index.m3u8

# 主播放列表（如果有多个码率）
http://localhost:8080/hls/live/stream1/master.m3u8
```

---

## 故障排除

### 常见问题

**1. 推流失败 (Cannot read RTMP handshake response)**
```bash
# 检查服务器是否运行
curl http://localhost:8080/health

# 检查端口是否被占用
lsof -i :1935
lsof -i :8080

# 检查防火墙设置
# 确保服务器监听的端口可以被 FFmpeg 访问
```

**2. CORS 跨域错误 (Access-Control-Allow-Origin)**
```javascript
// 浏览器控制台报错：
// Access to XMLHttpRequest at 'http://...' from origin 'http://...' has been blocked by CORS policy

// 解决方案：
// 1. 确保 HLS 服务器 CORS 配置正确（默认允许所有来源 *）
// 2. 检查请求 URL 和页面来源是否匹配
// 3. 如果使用域名，确保访问方式一致（都用 IP 或都用域名）

// 建议：Web 服务器和 HLS 服务器使用相同的地址访问
// 例如都用 http://192.168.1.100:8080/ 或都用 http://localhost:8080/
```

**3. HLS 播放返回 404 (Not Found)**
```bash
# 错误：GET http://.../index.m3u8 net::ERR_FAILED 404

# 原因和解决：
# 1. 流还没有被推流 - 必须先运行 FFmpeg 推流！
ffmpeg -re -i input.mp4 -c:v libx264 -c:a aac -f flv rtmp://localhost:1935/live/stream1

# 2. 推流和应用名称路径不匹配
# 推流：rtmp://localhost:1935/live/stream1
# 播放：http://localhost:8080/hls/live/stream1/index.m3u8
# 注意路径结构：/hls/{app}/{stream}/index.m3u8

# 3. 检查流是否在线
curl http://localhost:8080/hls/live/stream1/index.m3u8
```

**2. 播放卡顿**
```bash
# 1. 检查 FFmpeg 编码参数
# 增加关键帧间隔（GOP）
ffmpeg -i input -g 50 -keyint_min 50 ...

# 2. 降低码率或分辨率
ffmpeg -i input -b:v 1000k -s 854x480 ...

# 3. 使用更快的编码预设
ffmpeg -i input -preset ultrafast ...
```

**3. 延迟过高**
```bash
# 启用低延迟模式
./rslive-server --low-latency

# FFmpeg 使用 zerolatency tune
ffmpeg -i input -tune zerolatency -preset ultrafast ...

# 播放器配置低延迟参数（见上面的 LL-HLS 示例）
```

**4. 跨域问题 (CORS)**
服务器默认允许所有来源 (`*`)，如果需要限制：
```rust
// 在 HlsServerConfig 中设置
HlsServerConfig {
    cors_origin: Some("https://yourdomain.com".to_string()),
    ...
}
```

---

## 性能优化建议

### 服务器端

1. **使用低延迟模式**
   ```bash
   ./rslive-server --low-latency
   ```

2. **限制资源使用**
   ```bash
   ./rslive-server --max-streams 100 --max-segments 50
   ```

3. **调整日志级别**
   ```bash
   ./rslive-server --log-level warn  # 生产环境减少日志
   ```

### FFmpeg 端

1. **使用硬件加速**
   ```bash
   # macOS (VideoToolbox)
   ffmpeg -i input -c:v h264_videotoolbox ...

   # Linux (VA-API)
   ffmpeg -vaapi_device /dev/dri/renderD128 -i input -c:v h264_vaapi ...

   # NVIDIA (NVENC)
   ffmpeg -i input -c:v h264_nvenc ...
   ```

2. **优化编码参数**
   ```bash
   ffmpeg -i input \
       -c:v libx264 \
       -preset veryfast \
       -tune zerolatency \
       -g 50 \
       -keyint_min 25 \
       -sc_threshold 0 \
       ...
   ```

### 播放器端

1. **使用合适的缓冲策略**
   - 直播场景：小缓冲（2-5秒）
   - 点播场景：大缓冲（10-30秒）

2. **启用 Worker 解码**
   ```javascript
   const hls = new Hls({
       enableWorker: true,
       ...
   });
   ```

---

## 生产环境部署

### Docker 部署

```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin rslive-server

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/rslive-server /usr/local/bin/
EXPOSE 1935 8080 8081
ENTRYPOINT ["rslive-server"]
```

```yaml
# docker-compose.yml
version: '3'
services:
  rslive:
    build: .
    ports:
      - "1935:1935"
      - "8080:8080"
      - "8081:8081"
    command: ["--low-latency", "--log-level", "warn"]
```

### Nginx 反向代理

```nginx
server {
    listen 80;
    server_name stream.example.com;

    location /hls/ {
        proxy_pass http://localhost:8080/hls/;
        proxy_http_version 1.1;

        # CORS headers
        add_header Access-Control-Allow-Origin * always;
        add_header Access-Control-Allow-Methods 'GET, OPTIONS' always;

        # Cache control for playlists vs segments
        location ~ \.m3u8$ {
            proxy_pass http://localhost:8080;
            add_header Cache-Control "no-cache" always;
        }

        location ~ \.(ts|m4s)$ {
            proxy_pass http://localhost:8080;
            add_header Cache-Control "max-age=3600" always;
        }
    }
}
```

---

## 总结

| 组件 | 地址 | 用途 |
|------|------|------|
| RTMP 推流 | `rtmp://localhost:1935/live/:stream` | FFmpeg 推流 |
| HLS 播放 | `http://localhost:8080/hls/:stream/index.m3u8` | Web 播放 |
| 健康检查 | `http://localhost:8080/health` | 监控 |

这样你就可以实现：
- **推流端**: FFmpeg 推送 RTMP
- **服务端**: rslive-server 接收并转封装为 HLS
- **播放端**: 浏览器使用 hls.js 或 Video.js 播放