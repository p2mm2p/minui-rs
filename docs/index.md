# MinUI (Rust) — 文档首页

> MinUI 是一个为复古掌机打造的极简游戏启动器和 libretro 前端。
> 本仓库是 MinUI 从 C 到 Rust 的**完全重写**（原 C 代码见 Shaun Inman 的 [MinUI](https://github.com/shauninman/MinUI)）。
> 核心哲学：**没有设置、没有封面图、没有主题，开机即玩。**

## 文档地图

| 页面 | 内容 |
|------|------|
| [快速上手](getting-started.md) | 环境准备、编译、打包、发布（xtask 命令速查） |
| [架构](architecture.md) | 运行逻辑、模块职责与依赖方向、关键技术、workspace 契约、跨模块设计决策 |
| [模块：common](modules/common.md) | 基础设施层：Platform trait、音频、电源状态机、路径与工具函数 |
| [模块：render](modules/render.md) | 像素渲染层：文字、药丸按钮、电池、硬件状态栏、缩放器 |
| [模块：minui](modules/minui.md) | 启动器：SD 卡浏览、最近游戏、游戏启动协议 |
| [模块：minarch](modules/minarch.md) | 游戏内前端：libretro 加载、存档、菜单、HDMI、音频 |
| [模块：clock](modules/clock.md) | 日期时间设置工具 |
| [模块：minput](modules/minput.md) | 按键诊断工具 |
| [模块：platform-tg5040](modules/platform-tg5040.md) | TrimUI Smart Pro / Brick 平台实现（SDL2 + keymon + show） |
| [模块：xtask](modules/xtask.md) | 构建辅助：toolchain 流水线与 doc/test/lint |
| [模块：cores](modules/cores.md) | libretro 核心构建系统（通用层 + tg5040 平台声明） |

## 速览

- **语言**：Rust（edition 2024，workspace 多 crate 布局，详见 [架构](architecture.md)）
- **进程模型**：minui 与 minarch 是两个独立进程，经 shell 脚本与 `/tmp` 文件通信（沿用原版设计）
- **硬件抽象**：`Platform` trait 是所有硬件访问的唯一通道，编译期单态化
- **目标设备**：TrimUI Smart Pro / TrimUI Brick（`platforms/tg5040`）
