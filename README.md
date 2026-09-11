# MinUI — Rust 重写版

> MinUI 是一个为复古掌机打造的极简游戏启动器和 libretro 前端。
> 原作者：Shaun Inman。核心哲学：**没有设置、没有封面图、没有主题，开机即玩。**

本仓库是 MinUI 从 C 到 Rust 的完全重写（原 C 版约 12,000 行，Rust 版预计 8,000-10,000 行）。

## 项目介绍

| 组件 | 职责 |
|------|------|
| **minui**（crates/minui） | SD 卡文件浏览、最近游戏、启动游戏 |
| **minarch**（crates/minarch） | 游戏内前端：libretro 核心加载、存档、菜单、音频 |
| **common**（crates/common） | 零依赖基础设施：`Platform` trait、电源/输入/音频类型 |
| **render**（crates/render） | 像素渲染层：文字、按钮、电池、缩放器 |
| **clock / minput**（crates/） | 日期设置 / 按键诊断工具 |
| **platforms/tg5040** | TrimUI Smart Pro / Brick 平台实现（SDL2 + keymon/show） |
| **xtask** | 构建辅助：toolchain 流水线（源码 → 发布 zip） |

支持 TrimUI Smart Pro / Brick 设备（tg5040 平台），未来可扩展其余 11 个原版平台。

**进程模型**：minui 与 minarch 是两个独立进程，经 `/tmp` 文件协议通信——这部分沿用原版设计。

## 文档

- [快速上手](docs/getting-started.md)——环境、编译、打包发布
- [架构](docs/architecture.md)——运行逻辑、模块依赖、workspace 契约、跨模块设计决策
- **模块详解**（docs/modules/）：
  - [common](docs/modules/common.md) · [render](docs/modules/render.md) · [minui](docs/modules/minui.md) · [minarch](docs/modules/minarch.md)
  - [clock](docs/modules/clock.md) · [minput](docs/modules/minput.md) · [platform-tg5040](docs/modules/platform-tg5040.md)
  - [xtask](docs/modules/xtask.md) · [cores](docs/modules/cores.md)

## 快速开始

```sh
# 真机构建发布（详见 docs/getting-started.md——需容器引擎）
cargo xtask toolchain all --platform tg5040 --device smart

# 宿主直跑检查（通用层）
cargo xtask test && cargo xtask lint
```
