//! `minarch` — MinUI 游戏内前端
//!
//! 负责加载 libretro 核心、运行游戏循环、管理存档和配置。
//! 与 minui（启动器）通过 shell 脚本和 `/tmp` 文件通信。
//!
//! ## 模块
//!
//! | 模块 | 职责 |
//! |------|------|
//! | `libretro` | libretro ABI 地基：类型/常量/加载器/回调桥 |
//! | `game` | 游戏文件组织：zip 解压/m3u 探测/换碟、振动清零（已实现；实例由装配层持有） |
//! | `core` | libretro 核心接口封装：会话生命周期、布局计算、视频 pending 帧管线、快进帧预算、薄静态层（已实现；实例由装配层持有、handler 注册/pending 帧 flip/时钟注入留装配） |
//! | `environment` | libretro 环境回调：30 case 分发、运行时状态与 hook 注册（已实现；handler 注册留装配） |
//! | `savestate` | 存档快照持久化：快照路径命名与读写（纯逻辑层，已实现；序列化 FFI/槽位编排留装配） |
//! | `sram` | 电池存档持久化：SRAM/RTC 路径与读写（纯逻辑层，已实现；core 内存访问与 load/quit 调用点已由 core.rs 闭环，其余调用点留装配） |
//! | `config` | 前端/核心配置：OptionList 注册表、前端选项表、cfg 解析/序列化（纯逻辑层，已实现；文件 IO/平台联动留后续） |
//! | `audio` | 音频队列引擎：缓冲、重采样、快进门控、薄静态层（已实现；init/drain/ff 接线留装配） |
//! | `vibration` | 振动管理：排队 + 帧去抖状态机、薄静态层（已实现；hook 接线/tick 调用留装配） |
//! | `menu` | 游戏内菜单 UI：主菜单状态机、选项子菜单框架、菜单绘制、存档交互编排（已实现；PAD 轮询/主循环驱动/平台联动留装配） |
//! | `controls` | 物理按键 → libretro 按键映射（纯映射层，已实现；handler 接线留装配） |
//! | `hdmi` | HDMI 热插拔检测：变化检测状态机（纯逻辑层，已实现；装配层接线留主循环 change） |
//!
//! 本文件是 crate 根（lib 目标）：`pub mod` 声明让 `tests/` 集成测试
//! 可以链接 crate API。二进制入口在 `main.rs`。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `minarch.c` 约 4800 行单文件。Rust 版拆分为 lib（可测试的模块库）+ bin（薄入口）两个目标——原 C 没有对应物，这是为 FFI 集成测试引入的结构（详见 design.md 决策 4）。
//!

pub mod assembly;
pub mod audio;
pub mod config;
pub mod controls;
pub mod core;
pub mod environment;
pub mod game;
pub mod hdmi;
pub mod libretro;
pub mod menu;
pub mod savestate;
pub mod sram;
pub mod vibration;
