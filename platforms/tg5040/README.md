# platform-tg5040 — TrimUI Smart Pro / Brick 平台实现

`platform-tg5040` 是 MinUI 在 tg5040 芯片平台上的硬件适配层。它实现 `common::platform::Platform` trait，内部使用 SDL2（视频/输入/音频）与 libc 直读 evdev/sysfs（系统级按键与电源管理）。

**目标设备**：TrimUI Smart Pro 与 TrimUI Brick。两台设备共享同一芯片平台，差异（分辨率、布局、按键映射等）**在编译期**通过 Cargo feature（`smart`/`brick`）区分——互斥必选无默认，不拆 crate、无运行时设备判断。

## 模块定位与职责

- **硬件抽象**：`Tg5040` 结构体实现 `Platform` trait（27 方法 + 24 关联常量），SDL 完全封装在结构体内部——上层只见 trait，不知 SDL/evdev/sysfs 存在
- **双通道输入**：通道 A = 进程内 SDL joystick → `InputState`；通道 B = keymon 独立进程 evdev 直读（音量/亮度/静音随时可调）
- **平台自治**：纯 lib + show/keymon 独立 crate（平台子 xtask 编译装配）
- **画布 = 物理分辨率**：flip 恒 1:1，SCALE = 图集/布局倍率（smart 2、brick 3）

## 内部结构（子组件）

```
platforms/tg5040/
├── src/lib.rs        ← Tg5040 + Platform trait 实现（含语义键/能力常量/布局方法）
├── src/input.rs      ← 通道 A：SDL 事件 → InputState 纯逻辑状态机
├── src/settings.rs   ← 系统设置：共享内存 + 硬件 + 持久化（libmsettings 对应）
├── src/power.rs      ← 电源/背光/CPU/睡眠/关机（sysfs 纯函数）
├── show/             ← show 启动画面工具（独立 crate，自实现 PNG 解码）
├── keymon/           ← keymon 系统按键守护（独立 crate，evdev 直读）
└── xtask/            ← 平台子 xtask：编译 show/keymon + cores make + 装配
```

## 快速了解

- **设备区分**：smart/brick 编译期 feature（正向 cfg + compile_error 断言）——每设备独立二进制与安装包
- **系统设置**：settings 模块 = C 版 libmsettings 的 Rust 化（共享内存跨进程 + msettings.bin 磁盘格式兼容）
- **睡眠链**：SIGSTOP keymon + 硬件静音 + 关背光 → 唤醒 SIGCONT + 恢复（keymon 无需处理代码）

## 详细文档

完整文档（启动链路、双通道输入、各子系统实现、设备区分推导、否决方案清单、设计决策记录、FAQ、真机验证清单）见 [docs/modules/platform-tg5040.md](../../docs/modules/platform-tg5040.md)。
