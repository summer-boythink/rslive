# rslive 开发计划

本目录包含 rslive 项目的完整开发规划文档。

## 文档索引

| 文档 | 说明 | 优先级 |
|------|------|--------|
| [01-overview.md](./01-overview.md) | 项目概览、架构设计、技术选型 | 必读 |
| [02-protocols.md](./02-protocols.md) | 协议实现计划 (RTMP/FLV/HLS/SRT/WebRTC) | 必读 |
| [03-performance.md](./03-performance.md) | 性能优化策略和方案 | 必读 |
| [04-roadmap.md](./04-roadmap.md) | 开发路线图和版本规划 | 必读 |
| [05-competitive-analysis.md](./05-competitive-analysis.md) | 竞品分析和差异化优势 | 推荐阅读 |
| [06-implementation-guide.md](./06-implementation-guide.md) | 实施指南、开发工作流 | 开发时参考 |

## 快速导航

### 如果你是项目新手
阅读顺序：
1. [01-overview.md](./01-overview.md) - 了解项目定位
2. [05-competitive-analysis.md](./05-competitive-analysis.md) - 了解市场定位
3. [06-implementation-guide.md](./06-implementation-guide.md) - 开始开发

### 如果你要制定开发计划
阅读顺序：
1. [04-roadmap.md](./04-roadmap.md) - 了解开发阶段
2. [02-protocols.md](./02-protocols.md) - 了解技术细节
3. [03-performance.md](./03-performance.md) - 了解优化方向

### 如果你要评估技术方案
重点关注：
- [03-performance.md](./03-performance.md) - 性能优化策略
- [05-competitive-analysis.md](./05-competitive-analysis.md) - 竞品对比

## 核心亮点

### 性能目标
- 单核并发：10,000+ 连接
- 内存占用：< 50MB/千流
- P99 延迟抖动：< 1ms (无 GC)

### 关键优化
1. **Tokio 异步化** - 高并发支持
2. **零拷贝架构** - 使用 Bytes 避免数据复制
3. **内存池化** - 预分配缓冲区
4. **无锁设计** - DashMap 替代 Mutex<HashMap>

### 协议支持路线图
```
RTMP (已有基础) → FLV (v0.2) → HLS (v0.3) → SRT (v0.4) → WebRTC (v0.5)
```

## 下一步行动

见 [06-implementation-guide.md](./06-implementation-guide.md) 的 "下一步行动" 部分。
