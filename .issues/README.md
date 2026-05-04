# rslive 代码审查报告

## 概述

本次审查对 rslive 流媒体服务器代码库进行了全面分析，识别了架构、实现、安全和性能方面的问题。

## 严重级别说明

| 级别 | 含义 | 数量 |
|------|------|------|
| 🔴 Critical | 核心功能无法工作 | 1 |
| 🟠 High | 严重影响使用或稳定性 | 4 |
| 🟡 Medium | 影响功能或性能 | 8 |
| 🟢 Low | 代码质量问题 | 6 |

## 问题清单

### 🔴 Critical (必须立即修复)

#### [#4] RTMP-HLS 集成缺口
- **状态**: 未实现
- **影响**: RTMP 推流无法生成 HLS 片段，核心功能不可用
- **解决**: 需要实现 RTMP → StreamRouter → HlsPackager 的数据流

### 🟠 High (需要尽快修复)

#### [#1] RTMP Server EAGAIN 错误
- **状态**: 部分修复，需验证
- **症状**: `Resource temporarily unavailable (os error 35)`
- **修复**: 已添加 `set_nonblocking(false)`，需在目标硬件测试

#### [#3.1] StreamRouter `unsubscribe()` 实现错误
- **状态**: 未修复
- **影响**: 总是移除第一个订阅者而非特定订阅者
- **位置**: `src/media/router.rs:475-493`

#### [#3.7] StreamRouter 订阅者数量竞争条件
- **状态**: 未修复
- **影响**: 可能超出 `max_subscribers` 限制
- **位置**: `src/media/router.rs:418-472`

### 🟡 Medium (建议修复)

#### [#2] HLS Server CORS 和路由问题
- **状态**: 已修复
- **修复**: 添加 CORS 中间件，修正 axum 路由格式

#### [#3.2] 订阅者缺少唯一标识
- **影响**: 无法管理特定订阅者
- **建议**: 添加 `Subscriber` struct 包含 ID 和元数据

#### [#3.3] `publish()` 多次获取锁
- **影响**: 性能开销和潜在竞争条件
- **建议**: 单锁完成所有操作

#### [#3.4] `BackpressureStrategy::Block` 实现不完整
- **影响**: 可能无限阻塞
- **建议**: 添加超时处理

#### [#3.5] 帧丢弃策略实现相同
- **影响**: `DropOld` 和 `DropNew` 行为一致
- **建议**: 实现真正的不同策略

#### [#5.2] HLS 存储内存无限制
- **影响**: 可能内存溢出
- **建议**: 添加基于内存的限制

#### [#5.4] 流结束时未关闭订阅者通道
- **影响**: 订阅者可能永远挂起
- **建议**: 发送 EOS 标记或关闭通道

### 🟢 Low (改进建议)

#### [#6.1] 错误处理不一致
- 建议: 统一使用 `thiserror`

#### [#6.2] 缺少文档
- 建议: 为所有公共 API 添加 rustdoc

#### [#6.3] 魔法数字
- 建议: 使用命名常量

#### [#6.5] 测试覆盖率低
- 建议: HLS 模块需要更多测试

#### [#6.6] Clippy 警告
- 建议: 修复并添加 CI 检查

#### [#6.9] Feature flag 不一致
- 建议: 使特性真正可选

## 架构问题

### 核心架构缺口

当前架构存在致命缺口：**RTMP 和 HLS 服务器之间没有数据连接**。

```
当前状态:
  FFmpeg ──► RTMP Server    (孤立)
  Browser ◄── HLS Server    (孤立)
              ▲
              └── 没有数据源！

需要的状态:
  FFmpeg ──► RTMP Server ──► StreamRouter ──► HlsPackager ──► HLS Server ──► Browser
```

### 设计决策问题

1. **RTMP 服务器使用线程模型**: 每个连接一个线程，不适合高并发
2. **StreamRouter 使用同步锁**: 在异步环境中可能有问题
3. **HLS Packager 自动创建**: 但从不接收数据

## 优先级建议

### 第一阶段: 核心功能 (P0)
1. 实现 RTMP → StreamRouter 桥接
2. 实现 StreamRouter → HlsPackager 连接
3. 验证 EAGAIN 修复

### 第二阶段: 稳定性 (P1)
1. 修复 StreamRouter 订阅者管理
2. 添加资源限制和清理
3. 改进错误处理

### 第三阶段: 质量 (P2)
1. 完善文档
2. 提高测试覆盖率
3. 性能优化

## 测试建议

### 关键测试用例

```bash
# 1. RTMP 推流测试
ffmpeg -re -i test.mp4 -c:v libx264 -c:a aac -f flv rtmp://localhost:1935/live/test

# 2. HLS 播放测试
curl http://localhost:8080/hls/live/test/index.m3u8

# 3. 并发测试
# 启动多个 FFmpeg 实例推不同流

# 4. 长时间运行测试
# 运行 24 小时检查内存泄漏

# 5. 错误恢复测试
# 中断推流后恢复，检查 HLS 连续性
```

## 性能基准

建议添加的基准测试:

- [ ] 单流最大并发订阅者
- [ ] 最大同时流数量
- [ ] 帧转发延迟
- [ ] 内存使用量随时间变化
- [ ] HLS 片段生成延迟

## 相关文档

- [详细问题列表](01-rtmp-server-eagain-error.md)
- [HLS 服务器问题](02-hls-server-cors-issues.md)
- [StreamRouter 设计问题](03-stream-router-design-issues.md)
- [RTMP-HLS 集成缺口](04-rtmp-hls-integration-gap.md)
- [内存安全问题](05-memory-safety-concerns.md)
- [代码质量问题](06-code-quality-issues.md)

## 下一步行动

1. ✅ 修复 EAGAIN 错误 (已尝试)
2. ✅ 修复 CORS 问题 (已完成)
3. 🔄 实现 RTMP-HLS 数据流 (关键)
4. ⏳ 修复 StreamRouter 订阅者管理
5. ⏳ 添加资源限制
6. ⏳ 完善测试覆盖

---

*审查日期: 2026-05-04*
*审查人: Claude Code*
*代码版本: main 分支*
