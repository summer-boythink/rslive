# 快速修复指南

## 🔥 最紧急的问题

### 1. RTMP 推流失败 (EAGAIN 错误)
**症状**: `Cannot read RTMP handshake response`

**修复状态**: ✅ 已尝试修复
```bash
# 重新编译
cargo build --release --bin rslive-server

# 测试
./target/release/rslive-server
# 另开终端
ffmpeg -re -i test.mp4 -c:v libx264 -c:a aac -f flv rtmp://127.0.0.1:1935/live/test
```

**如果还失败**:
- 检查防火墙: `sudo ufw allow 1935/tcp`
- 检查端口占用: `sudo lsof -i :1935`
- 尝试 localhost: `rtmp://127.0.0.1:1935/...` 而非 IP

---

### 2. HLS 播放 404 错误
**症状**: `GET http://.../index.m3u8 404 (Not Found)`

**根本原因**: ⚠️ **这是正常的！**

HLS 需要**先推流**才能播放:
```bash
# 步骤 1: 启动服务器
./rslive-server

# 步骤 2: FFmpeg 推流 (保持运行!)
ffmpeg -re -i test.mp4 -c:v libx264 -c:a aac -f flv rtmp://127.0.0.1:1935/live/stream1

# 步骤 3: 等待 5-10 秒，然后播放
# http://127.0.0.1:8080/hls/live/stream1/index.m3u8
```

**注意**: 没有推流时访问 HLS URL 会返回 404 + 提示信息

---

### 3. CORS 跨域错误
**症状**: `CORS policy: No 'Access-Control-Allow-Origin' header`

**修复状态**: ✅ 已修复

**使用方式**:
```bash
# 不要用 file:// 打开 HTML
# 应该用 HTTP 服务器

cd docs
python3 -m http.server 8081

# 然后访问
# http://localhost:8081/1.html
```

**关键**: Web 页面和 HLS 服务器地址必须一致:
- ❌ 页面 `file://...`, HLS `http://127.0.0.1:8080/...`
- ✅ 页面 `http://127.0.0.1:8081/...`, HLS `http://127.0.0.1:8080/...`

---

## 🚨 关键限制 (必须了解)

### RTMP 和 HLS 尚未连接！

即使推流成功，**HLS 仍然没有数据**！

```
当前架构:
FFmpeg ──► RTMP Server    (数据到这里就停了)
                        
Browser ◄── HLS Server    (这里永远收不到数据)
```

**这是架构设计问题，不是配置问题！**

**解决方案**:
1. 实现 RTMP → StreamRouter 桥接 (需要开发)
2. 或: 手动注入测试数据 (开发测试)

---

## ✅ 当前可用的功能

| 功能 | 状态 | 说明 |
|------|------|------|
| RTMP 服务器监听 | ✅ 工作 | 可以接收连接 |
| RTMP 握手 | ⚠️ 可能有问题 | EAGAIN 已尝试修复 |
| HLS 服务器 | ✅ 工作 | 可以服务 HTTP 请求 |
| CORS | ✅ 已修复 | 允许跨域访问 |
| 流路由 | ✅ 编译通过 | 但 RTMP 未使用 |
| fMP4 Muxer | ✅ 已实现 | 测试通过 |
| MPEG-TS Muxer | ✅ 已实现 | 测试通过 |

---

## 🔧 诊断命令

```bash
# 1. 检查服务器是否运行
curl http://127.0.0.1:8080/health

# 2. 检查端口监听
sudo ss -tlnp | grep -E '1935|8080'

# 3. 测试 RTMP 端口
telnet 127.0.0.1 1935

# 4. 查看 HLS 播放列表 (推流后)
curl http://127.0.0.1:8080/hls/live/stream1/index.m3u8

# 5. 查看系统日志
journalctl -u rslive-server -f
```

---

## 📊 预期行为

### 正常流程
```
1. 启动服务器
   └── 日志: "RTMP Server listening on 0.0.0.0:1935"
   └── 日志: "HLS server starting addr=0.0.0.0:8080"

2. FFmpeg 推流
   └── 服务器日志: "New client connected: 192.168.x.x:xxxxx"
   └── FFmpeg 持续输出 frame=xxx fps=xx ...

3. 等待片段生成
   └── 需要几秒到几十秒
   └── 取决于关键帧间隔

4. 浏览器播放
   └── HLS 播放列表返回 m3u8 内容
   └── 视频开始播放
```

### 如果卡在步骤 2
- 检查 EAGAIN 错误
- 检查防火墙
- 检查端口占用

### 如果卡在步骤 4
- 确认推流是否成功
- 检查 HLS URL 是否正确
- 检查 CORS (浏览器控制台)

---

## 🆘 紧急修复检查清单

如果推流还是失败:

- [ ] 使用最新编译版本
- [ ] 服务器和 FFmpeg 在同一机器测试 (localhost)
- [ ] 关闭防火墙测试
- [ ] 使用 diagnostic 脚本: `./diagnose.sh`
- [ ] 检查服务器日志是否有 panic
- [ ] 尝试不同 FFmpeg 版本

---

## 📚 完整文档

- [所有问题列表](README.md)
- [使用指南](../docs/usage-guide.md)

---

*最后更新: 2026-05-04*
