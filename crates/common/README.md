# common — MinUI 基础设施层

`common` 是 MinUI Rust 重写中**唯一零第三方依赖的 crate**，为所有上层代码提供共享的类型定义、硬件抽象接口和工具函数。minui 与 minarch 的代码只会从三个来源 `use`：`common::*`（类型与 trait）、`render::*`（画东西）、`platform_*`（硬件，仅装配层）。

## 模块定位与职责

- **零依赖**：全部基于 Rust 标准库——`Vec<u16>` 像素缓冲、`std::fs` 文件 I/O；修改 common 只重编译依赖它的 crate
- **Platform trait**：硬件抽象接口（27 方法 + 24 关联常量），SDL 等实现细节完全隔离在平台 crate
- **可测试**：184 个单元测试在无 GPU/无 SDL 的 CI 环境直接运行

## 内部结构（子组件）

```
crates/common/src/
├── lib.rs          ← crate 根
├── platform.rs     ← Platform trait（27 方法 + 24 关联常量：设备常量/能力常量/语义键/布局方法）
├── video.rs        ← VideoBuffer、Rect、Asset 图集元数据、布局常量（单一来源）、VsyncMode
├── input.rs        ← 位掩码常量、InputState（含摇杆 laxis/raxis）、tapped_menu、ModKeys
├── audio.rs        ← AudioFrame、AudioRingBuffer（无锁 SPSC）、Resampler
├── power.rs        ← PowerState 状态机、update（检测者）、faux_sleep、控制标志
├── paths.rs        ← /tmp 协议常量 + SDCARD 家族路径派生函数（纯字符串拼接）
└── utils.rs        ← 文件 I/O（7）、字符串匹配（4）、路径处理（5）——共 16 个工具函数
```

## 快速了解

- **架构位置**：common（零依赖）← render / platform；上层 minui/minarch 只依赖 common + render + 平台
- **路径知识分层**：原语（`SDCARD_PATH`/`PLATFORM`）在 trait 关联常量，派生在 `paths` 函数，/tmp 协议常量在 `paths` 常量区块
- **装配职责**：`power::update` 是纯函数（检测者）——输入由装配层每帧收集传入，动作由装配层执行

## 详细文档

完整文档（核心概念、Platform trait 全表、音频管线、电源状态机、路径/工具函数详解、跨模块决策、否决清单、设计决策记录、FAQ）见 [docs/modules/common.md](../../docs/modules/common.md)。
