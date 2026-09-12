# common — MinUI 基础设施层

`common` 是 MinUI Rust 重写中**唯一零第三方依赖的 crate**，为所有上层代码提供共享的类型定义、硬件抽象接口和工具函数。它在 workspace 中的架构位置：

```
            ┌──────────┐   ┌──────────┐
            │  minui   │   │ minarch  │    ← 二进制 crate
            └─────┬────┘   └─────┬────┘
                  │              │
       ┌──────────┴──────┬───────┴──────────┐
       │                 │                  │
       ▼                 ▼                  ▼
 ┌───────────┐   ┌──────────────┐   ┌──────────────┐
 │  render   │   │    common     │   │ platform-*   │  ← 库 crate
 │ (fontdue  │   │  （零依赖）   │   │ (SDL2/        │
 │  +png)    │   │              │   │  fbdev…)      │
 └───────────┘   └──────────────┘   └──────────────┘
```

minui 和 minarch 的代码中只会出现三个 `use` 来源：`common::*`（类型和 trait）、`render::*`（画东西）、`platform_*`（硬件，仅 `main.rs` 中一处）。common 是底层的"通用语言"——它定义了什么是一个 `VideoBuffer`、什么是一次按键、什么是音频帧、什么是平台能做的事。

---

## 内容地图

| 章节 | 回答的问题 |
|------|-----------|
| [为什么需要 common crate](#为什么需要-common-crate) | 为什么类型和接口要抽成零依赖 crate？ |
| [依赖关系](#依赖关系) | common 与上层 crate 的依赖方向？ |
| [核心概念](#核心概念) | `<P: Platform>` 泛型 / VideoBuffer / InputState / RGB565 |
| [模块总览](#模块总览) | 六个模块各管什么、对应哪些 C 文件 |
| [platform — Platform trait](#platformplatform-trait) | 27 方法 + 24 常量怎么设计、怎么用、怎么实现 |
| [audio — 音频管线](#audio音频管线) | 音频帧 / 环形缓冲 / 重采样怎么工作 |
| [video — 视频类型与常量](#video视频类型与常量) | VideoBuffer / 图集坐标表 / 布局常量 |
| [input — 输入状态](#input输入状态) | 位掩码、查询方法、菜单轻触检测 |
| [power — 电源管理状态机](#power电源管理状态机) | 电池/睡眠状态机、修饰键逻辑、装配层职责 |
| [paths — 平台路径派生函数族](#paths平台路径派生函数族) | 路径常量与派生函数、为什么是函数不是宏 |
| [utils — 文件与字符串工具](#utils文件与字符串工具) | 16 个工具函数分三类 |
| [跨模块设计决策](#跨模块设计决策) | 类型归属规则、布局常量单一来源 |
| [C → Rust 迁移对照表](#c--rust-迁移对照表) | 原 C 体系 vs Rust 版全维度对照 |
| [测试策略](#测试策略) | 纯逻辑如何无硬件测试 |
| [否决方案清单](#否决方案清单) | 被否决的设计（防 AI 误引入） |
| [常见问题](#常见问题) | 高频疑问答疑 |

---

## 为什么需要 common crate

原版 MinUI 的 C 代码将类型定义、平台接口、渲染逻辑和工具函数混合在两个文件中：


| 文件          | 行数     | 包含内容                                                                                  |
| ----------- | ------ | ------------------------------------------------------------------------------------- |
| `api.h`     | ~220   | 34 个平台函数宏（`#define GFX_clear PLAT_clearVideo`）、结构体（`PAD_Context`、`SND_Frame`）、枚举、颜色常量 |
| `api.c`     | ~1,723 | 渲染函数的实现（`GFX_blitPill`、`GFX_blitButton`）、音频管线（环形缓冲+重采样）、电源管理、工具函数                     |
| `defines.h` | ~70    | UI 布局常量（`PILL_SIZE`、`BUTTON_SIZE`）、路径宏（`#define ROMS_PATH SDCARD_PATH "/Roms"`）       |
| `utils.c`   | ~205   | 文件 I/O、字符串匹配、路径处理                                                                     |


这带来四个问题：

1. **SDL 耦合**：`api.c` 的渲染代码直接操作 `SDL_Surface*`、`TTF_Font*`——你想测试文件 I/O 工具函数？你需要先初始化 SDL
2. **全局可变状态**：`gfx`（视频）、`pad`（输入）、`snd`（音频）都是全局变量——没有模块边界，任何函数都能读写任何状态
3. **宏转发**：`#define GFX_clear PLAT_clearVideo` 让平台函数在编译时被替换——数据流不可追踪，IDE 无法跳转
4. **无模块边界**：1700 行的 `api.c` 是单文件——环形缓冲和按键检测混在一起

Rust 版的回应：


| 原 C 问题 | Rust 解决方案                                                 |
| ------ | --------------------------------------------------------- |
| SDL 耦合 | common 零第三方依赖——`Vec<u16>` 当像素缓冲、`std::fs` 做文件 I/O         |
| 全局变量   | `InputState`/`PowerState`/`AudioRingBuffer` 都是值类型，调用方持有实例 |
| 宏转发    | `Platform` trait 在编译期绑定实现——数据流清晰，IDE 支持跳转                 |
| 单文件    | 7 个子模块按语义拆分——platform、video、input、audio、power、utils、paths |


```rust
// C 方式：渲染函数内部访问全局变量
// GFX_blitPill(gfx.mode == MODE_MAIN ? ASSET_DARK_GRAY_PILL : ASSET_BLACK_PILL, ...);

// Rust 方式：调用方通过参数传入状态
let pill_asset = if status.mode_main { Asset::DarkGrayPill } else { Asset::BlackPill };
// 参数来自调用方持有的 HardwareStatus struct
```

## 依赖关系

```
              common（零依赖）
                ↑         ↑
                │         └──────────────────┐
                │                            │
             render                     platforms/tg5040
          (fontdue + png)             (sdl2 + sdl2-sys + libc)
                ↑                            ↑
                └──────────┬─────────────────┘
                           │
                    minui    minarch
           （两者均直接依赖 common + render + platform）
```

- **common**：不依赖任何第三方 crate。所有功能基于 Rust 标准库——`Vec<u16>`、`std::fs`、`std::path`、`String`/`&str`
- **render**：依赖 `common` + `fontdue`（字体栅格化）+ `png`（图集解码）。不依赖平台
- **platforms/tg5040**：依赖 `common` + `sdl2` + `sdl2-sys` + `libc`。SDL 的使用完全局限在 `Tg5040` 结构体内部
- **minui / minarch**：依赖 `common` + `render` + 平台 crate（通过 feature flag 选择）

**零依赖的工程意义**：

- **编译速度**：修改 common → 只重编译它和依赖它的 crate；修改 render → common 不受影响，跳过重编译
- **CI 可测试性**：common 的 121 个测试在无 GPU、无 SDL 的 CI 环境直接运行——只需 `cargo test -p common`
- **可移植性**：common 不承诺任何 OS 抽象。`std::fs::read_to_string` 在 Linux 掌机、macOS 开发机、Windows CI 上都可用

## 核心概念

在深入各模块之前，需要理解 common crate 的四个基础概念。它们是理解所有类型和 trait 的"通用语言"。

### `<P: Platform>` 泛型——硬件抽象的"USB 规范"

`Platform` trait 是 MinUI 的硬件抽象层。minui 和 minarch 的所有代码只认识这个 trait——不知道也不关心底层是 SDL2、framebuffer + evdev 还是其他任何实现。

**通俗类比**：USB 规范定义了"供电 + 数据传输"的标准。任何符合规范的设备——U 盘、键盘、风扇——插上去都能工作。`Platform` trait 就是 MinUI 的 USB 规范，`Tg5040` 是具体的设备。换一台掌机只需要换平台实现——minui 和 minarch 的代码一行不用改。

`**<P: Platform>` 是什么？** Rust 的**泛型参数（generic parameter）**——编译期的"占位符"，在编译时被替换为具体类型：

```rust
// minui 的入口函数：P 是泛型——编译时才知道是哪个平台
fn run<P: Platform>(platform: &mut P) {
    platform.init_video();              // ← P 在这台设备上是 Tg5040
    let mut screen = VideoBuffer::new(
        P::SCREEN_WIDTH, P::SCREEN_HEIGHT  // SCREEN_WIDTH 在 Tg5040 上 = 640
    );
    // ...
    platform.flip(&screen, true);
}

// 编译时，Rust 为每个平台生成一份专用代码：
// run::<Tg5040>()      → 一份机器码（所有调用都直接跳转，零间接开销）
// run::<Miyoo>()       → 另一份机器码（如果以后有）
```

**为什么不用 `dyn Platform`（运行时多态）？** `dyn trait` 通过虚表（vtable）在运行时查找函数地址——每次调用多一次指针间接跳转。在 400MHz ARM 掌机上，每帧只有 17ms 预算（60fps），数十次平台调用累积的虚表开销不可接受。泛型单态化在编译时展开——所有调用都是直接跳转，与手写函数调用完全相同的机器码。

**为什么 Platform 是单个 trait 而非拆分 Video/Input/Audio？** 每个平台都是完整的硬件集合——不存在"只实现视频不实现音频"的情况。拆分子 trait 会增加 trait 边界上的泛型约束复杂度，对实现者没有实际收益。

### VideoBuffer——纯像素画布

**什么是像素缓冲（pixel buffer）**：一块连续内存区域，每个像素用固定格式（RGB565）存储颜色。类比：一张方格纸，每格一种颜色。

```rust
pub struct VideoBuffer {
    pub pixels: Vec<u16>,   // RGB565 像素数据
    pub width: u32,          // 可见宽度（像素）
    pub height: u32,         // 可见高度（像素）
    pub pitch: u32,          // 每行像素数（>= width）
}
```

**pitch（行距）是什么？** pitch 是内存中每行像素的数量。在绝大多数情况下 `pitch == width`。但某些硬件要求像素行按 2/4/8 像素对齐到内存边界——这时 `pitch > width`，每行末尾有不可见的"填充像素"：

```
pitch == width（最常见）:              pitch > width（硬件对齐）:
┌───┬───┬───┬───┐                    ┌───┬───┬───┬───┬───┬───┐
│ A │ B │ C │ D │                    │ A │ B │ C │ D │   │   │
├───┼───┼───┼───┤                    ├───┼───┼───┼───┼───┼───┤
│ E │ F │ G │ H │                    │ E │ F │ G │ H │   │   │
└───┴───┴───┴───┘                    └───┴───┴───┴───┴───┴───┘
pixels[1*4+2] = G                    pixels[1*6+2] = G
```

MinUI 当前在 tg5040 平台使用 `pitch == width`。`VideoBuffer::new(w, h)` 创建的缓冲区默认 `pitch = width`。`fill_rect` 在填充时会按 `pitch` 计算每行的起始偏移，确保在 `pitch > width` 的硬件上也能正确渲染。

`**fill_rect` 为什么不做边界裁剪？** 渲染循环中 `fill_rect` 被高频调用——每个像素增加一条 `if` 检查累积极大。一次性验证 rect 合法性，比逐像素检查高效得多。这是设计与性能的权衡——调用方负责合法的坐标，渲染函数负责快速绘制。

### InputState——位掩码按键

使用 `u32` 位掩码表示按键状态——每位对应一个物理按键。与原版 C 的设计完全一致：

```
pressed 字段（u32）的位分配：
┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬──...──┬─────┬─────┬─────┐
│ UP  │DOWN │LEFT │RIGHT│  A  │  B  │  X  │  Y  │START│  ...  │MENU │PLUS │MINUS│
│bit0 │bit1 │bit2 │bit3 │bit4 │bit5 │bit6 │bit7 │bit8 │       │bit16│bit17│bit18│
└─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴──...──┴─────┴─────┴─────┘
  32 位中 21 位用于物理按键，其余保留
```

`InputState` 包含四个 `u32` 字段，分别表示不同的按键状态：

```
时间线（以 A 键为例）：

pressed         ░░░░░░░░████████████████████████████░░░░░░░░
                ↑ 按下                              ↑ 释放

just_pressed    ░░░░░░░░████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
                ↑ 仅按下后的第一帧为 1

just_released   ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░████░░░░░
                                                   ↑ 仅释放后的第一帧为 1

just_repeated   ░░░░░░░░░░░░░░░████░░░░░░░░░░░░░░████░░░░░░
                            ↑ 300ms       ↑ 100ms 间隔重复
```

- **pressed**：按键当前是否被按住（持续为 1）
- **just_pressed**：按键是否在**本帧**刚被按下（仅一帧为 1，下一帧自动清零）
- **just_released**：按键是否在**本帧**刚被释放（仅一帧为 1）
- **just_repeated**：用于文本输入场景的重复信号（首次 300ms 延迟，之后每 100ms 重复）

**为什么用 u32 位掩码而不是 `[bool; 21]`？** ARM 处理器上 `u32` 的位运算（AND/OR/SHIFT）各只需 1 个 CPU 周期。多个按键的同时检测可以一次完成：

```rust
// 检查 A 是否刚被按下：一次按位与 = 1 周期
if input.just_pressed & BTN_A != 0 { /* ... */ }

// bool 数组版本：需要索引 + 比较 = 3+ 周期
// if input.just_pressed[BTN_ID_A] { /* ... */ }
```

在主循环每帧可能检查 10+ 次按键的场景下，差异显著。

### RGB565 色彩格式

**为什么用 16 位色？** 这是掌机硬件的物理约束：

- 大多数复古掌机的 LCD 控制器使用 RGB565 接口
- 内存考量：640×480×2 字节 = 614KB vs 640×480×4 字节（RGBA）= 1.2MB——帧缓冲占用量差一倍
- 色觉考量：人眼对绿色最敏感——绿色通道分配 6 位（64 级），红蓝各 5 位（32 级）

```
RGB565 16 位布局：
┌──────────────┬─────────────┬─────────────┐
│  R (5 bits)  │  G (6 bits) │  B (5 bits) │
│  bits 15-11  │  bits 10-5  │  bits 4-0   │
└──────────────┴─────────────┴─────────────┘

纯红: 0xF800 = 11111 000000 00000
纯绿: 0x07E0 = 00000 111111 00000
纯蓝: 0x001F = 00000 000000 11111
纯白: 0xFFFF = 11111 111111 11111
纯黑: 0x0000 = 00000 000000 00000
```

颜色常量定义在 `common::video`：


| 常量               | 值        | 颜色  |
| ---------------- | -------- | --- |
| `RGB_WHITE`      | `0xFFFF` | 纯白  |
| `RGB_BLACK`      | `0x0000` | 纯黑  |
| `RGB_LIGHT_GRAY` | `0xCE79` | 浅灰  |
| `RGB_GRAY`       | `0x9CD3` | 中灰  |
| `RGB_DARK_GRAY`  | `0x4208` | 深灰  |


## 模块总览


| 模块         | 关键类型                                       | 行数   | 职责                     | 对应 C                        |
| ---------- | ------------------------------------------ | ---- | ---------------------- | --------------------------- |
| `platform` | `Platform` trait                           | — | 硬件抽象接口（27 方法 + 24 关联常量） | `api.h` PLAT_* 宏            |
| `video`    | `VideoBuffer`、`Rect`、`Asset`、布局常量          | 904  | 视频基本类型、精灵图集坐标表、UI 常量   | `defines.h` + `api.c` GFX_* |
| `input`    | `InputState`、`BTN_*`、`tapped_menu`         | 664  | 按键位掩码、输入状态查询、菜单轻触检测    | `api.c` PAD_*               |
| `audio`    | `AudioFrame`、`AudioRingBuffer`、`Resampler` | 553  | 音频帧类型、无锁环形缓冲、采样率重采样    | `api.c` SND_*               |
| `power`    | `PowerState`、`BatteryStatus`、`CpuSpeed`    | 955  | 电源状态机、睡眠/唤醒控制、电量监控     | `api.c` PWR_*               |
| `utils`    | 16 个工具函数                                   | 443  | 文件 I/O、字符串匹配、ROM 路径处理  | `utils.c`                   |

> 行数为 `wc -l` 实测值（2026-08-09）。`input`/`power` 在 tg5040 实现过程中扩展（ANALOG 键位、mod_keys 参数）——行数随实现演进，如需精确值以源码为准。


---

## platform——Platform trait

`Platform` trait 是整个 MinUI Rust 重写的**架构中枢**。minui（启动器）和 minarch（游戏内前端）仅通过此 trait 访问硬件——不感知底层是 SDL2、SDL1.2 还是 framebuffer + evdev。每个支持的掌机设备对应一个此 trait 的实现。

```
              ┌─────────────────────────────┐
              │  minui        minarch        │  ← 只 import Platform
              │  fn run<P: Platform>(...)    │
              └──────────────┬──────────────┘
                             │ trait 方法调用（编译期单态化）
              ┌──────────────▼──────────────┐
              │      Platform trait          │  ← 定义在 common（零依赖）
              │  27 方法 + 24 关联常量        │
              └──────┬──────────┬───────────┘
                     │          │
       ┌─────────────▼──┐  ┌────▼─────────────┐
       │ platform-tg5040 │  │ platform-miyoo…  │  ← 平台实现（SDL2 等）
       │    (SDL2)       │  │    (future)      │
       └─────────────────┘  └──────────────────┘
```

**设计原则**：

- **单一 trait**：不拆分 Video/Input/Audio 子 trait——每个平台都是完整的硬件集合，不存在"只实现视频不实现输入"的情况
- **渲染原语而非 Surface**：上层操作 `VideoBuffer`（纯像素数组），平台在 `flip` 内部处理像素拷贝和呈现。SDL 类型对上层完全不可见
- **关联常量**：屏幕尺寸、缩放倍率等编译期确定的值使用 `const`——上层可用它们声明定长数组（栈分配，零堆开销）
- **不返回 `Result`**：嵌入式设备初始化失败意味着无法运行（没有回退方案），直接 panic 匹配原 C 行为——让看门狗重启设备

### 完整方法列表

`Platform` trait 共 **27 个方法 + 24 个关联常量**（16 个设备常量 + 8 个按键能力常量），按 8 个功能分组。

#### 关联常量（24 个）

编译期确定的硬件参数。上层通过 `P::SCREEN_WIDTH` 访问（`P` 是泛型参数）。设备常量 16 个：


| 常量                    | 类型     | 含义                                              | tg5040 值 |
| --------------------- | ------ | ----------------------------------------------- | -------- |
| `SCREEN_WIDTH`        | `u32`  | 屏幕物理宽度（像素）                                      | 1280     |
| `SCREEN_HEIGHT`       | `u32`  | 屏幕物理高度（像素）                                      | 720      |
| `SCALE`               | `u32`  | 整数缩放倍率（UI 元素绘制时乘以该值）                            | 2        |
| `BYTES_PER_PIXEL`     | `u8`   | 每像素字节数（2 = RGB565）                              | 2        |
| `HAS_HDMI`            | `bool` | 设备是否有 HDMI 能力（编译期常量，与运行时 `is_hdmi_active()` 不同） | true     |
| `HAS_POWER_BUTTON`    | `bool` | 是否有物理电源键（影响关机提示文案）                              | true     |
| `HAS_POWEROFF_BUTTON` | `bool` | 是否有独立关机键                                        | false    |
| `SUPPORTS_OVERSCAN`   | `bool` | 是否支持过扫描区域                                       | false    |
| `DEVICE_MODEL`        | `&str` | 设备型号名称（编译期确定）                                  | "TrimUI Smart Pro" |
| `SDCARD_PATH`         | `&str` | SD 卡挂载根路径                                           | "/mnt/SDCARD" |
| `PLATFORM`            | `&str` | 平台代码（`.system/{code}` 与 `platforms/{code}` 目录名、xtask `--platform` 参数） | "tg5040" |
| `BTN_SLEEP`           | `u32`  | 睡眠键位（前端与 keymon 协调的语义键）                       | BTN_POWER |
| `BTN_MOD_BRIGHTNESS`  | `u32`  | 亮度修饰键（语义键，同下）                                  | BTN_MENU |
| `BTN_MOD_VOLUME`      | `u32`  | 音量修饰键                                                | BTN_NONE |
| `BTN_MOD_PLUS`        | `u32`  | 加键修饰键                                                | BTN_PLUS |
| `BTN_MOD_MINUS`       | `u32`  | 减键修饰键                                                | BTN_MINUS |


**按键能力常量（8 个）**：`HAS_L2/R2/L3/R3/LS/RS/VOLUME/MENU`——声明平台有哪些按键/轴（默认 `false` 保守语义：平台未声明即无此键），消费方是 minput 面板渲染等。注意能力声明与 `poll_input` 的事件翻译是**两层职责**：`HAS_*` 回答"平台有没有"，映射表回答"事件来了映射成什么"。

- `**DEVICE_MODEL**`：设备型号名称（编译期确定，如 `"TrimUI Smart Pro"`）。对应原 C `PLAT_getModel()`（读环境变量 `TRIMUI_MODEL`），Rust 版改为编译期常量——"编译期固定值 → 关联常量，运行时状态 → 方法"的 trait 分层约定。消费方：minui（版本页）

**为什么 `SCREEN_WIDTH`/`SCREEN_HEIGHT` 是关联常量而非方法？** 它们是编译期确定的值——不会在运行时改变。作为常量，上层可以在栈上声明定长数组 `[u16; SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize]`——零堆分配。作为方法则需要运行时调用和堆分配。

**语义键（`BTN_SLEEP`/`BTN_MOD_*`）为什么也是关联常量？** 它们是"前端与 keymon 守护进程的协调常量"——keymon 硬编码同套 evdev 约定、前端识别组合键（MENU+PLUS=调亮度）不误开菜单；minui/minarch/平台三方共享 → trait 单一来源，装配层从 `P::BTN_MOD_*` 组装 `ModKeys` 传入（平台 crate 不再暴露这些常量——见 tg5040 文档）。

#### 视频（4 方法）——管理物理屏幕


| 方法           | 签名                                                           | 默认实现 |
| ------------ | ------------------------------------------------------------ | ---- |
| `init_video` | `fn init_video(&mut self)`                                   | 无    |
| `quit_video` | `fn quit_video(&mut self)`                                   | 无    |
| `set_vsync`  | `fn set_vsync(&mut self, mode: VsyncMode)`                   | 空操作  |
| `flip`       | `fn flip(&mut self, buffer: &VideoBuffer, wait_vsync: bool)` | 无    |


- `**init_video**`：初始化视频子系统。平台内部创建窗口、渲染器、纹理等 SDL 资源。**不返回 `VideoBuffer`**——上层通过 `SCREEN_WIDTH`/`SCREEN_HEIGHT` 自行创建画布。`init_video` 只初始化平台内部资源，不暴露任何 SDL 对象。调用时机：minui/minarch 启动时
- `**quit_video**`：销毁视频子系统资源。调用时机：退出前
- `**set_vsync**`：设置垂直同步模式。默认空操作——部分平台（如 tg5040）在创建渲染器时已决定 vsync 行为。调用时机：minui 启动时（固定 STRICT）、minarch 读取配置后
- `**flip**`：将 `VideoBuffer` 的像素提交到物理屏幕。平台负责将 `buffer.pixels` 拷贝到内部 SDL 纹理并呈现。`wait_vsync` 参数由上层帧计时逻辑计算

`flip` 的数据流：

```
  VideoBuffer.pixels           SDL_Texture (GPU 显存)          物理屏幕 (LCD)
       │                              │                            │
       └─── SDL_UpdateTexture() ──────→│                            │
                                       └─── SDL_RenderCopy() ──────→│
                                                                     └── SDL_RenderPresent()
```

#### 输入（4 方法）——读取物理按键


| 方法            | 签名                                       | 默认实现       |
| ------------- | ---------------------------------------- | ---------- |
| `init_input`  | `fn init_input(&mut self)`               | 无          |
| `quit_input`  | `fn quit_input(&mut self)`               | 无          |
| `poll_input`  | `fn poll_input(&mut self) -> InputState` | 无          |
| `reset_input` | `fn reset_input(&mut self)`             | 空操作 |


- `**reset_input**`：清空平台跨帧输入状态。睡眠前/唤醒后各调一次（对应 C 两次 `PAD_reset`——睡眠瞬间的按键状态不残留，见「faux_sleep 流程」）。默认空操作；tg5040 覆盖为清 `PadState`（连重复计时与摇杆原值——与 C 保留 `repeat_at` 的有意差异见其文档）。注意与 `InputState::reset`（清除本帧 just_* 字段）职责不同
- `**poll_input**`：轮询输入设备，返回当前帧的输入状态。平台负责 SDL 事件循环 + 按键重复逻辑（首次按下 300ms 后开始重复，之后每 100ms 重复一次）——`InputState.just_repeated` 位由平台设置。调用时机：主循环每帧开始时

#### 音频（5 方法）——推送音频帧到硬件


| 方法                 | 签名                                                            | 默认实现                 |
| ------------------ | ------------------------------------------------------------- | -------------------- |
| `pick_sample_rate` | `fn pick_sample_rate(&self, requested: u32, max: u32) -> u32` | `requested.min(max)` |
| `init_audio`       | `fn init_audio(&mut self, sample_rate: u32) -> u32`           | 无                    |
| `quit_audio`       | `fn quit_audio(&mut self)`                                    | 无                    |
| `pause_audio`      | `fn pause_audio(&mut self, pause: bool)`                      | 无                    |
| `push_audio`       | `fn push_audio(&mut self, frames: &[AudioFrame]) -> usize`    | 无                    |


- `**pick_sample_rate**`：协商采样率。纯查询方法（`&self`）——在 `init_audio` 之前调用，不影响平台状态。**为什么独立于 `init_audio`？** 分离了纯查询和带副作用操作——`pick_sample_rate` 可以被调用多次来探测硬件能力。默认实现是 `requested.min(max)`——12 个 C 平台中 11 个用此逻辑
- `**init_audio**`：初始化音频设备。参数为 `pick_sample_rate` 协商后的值。返回硬件实际使用的采样率（可能与请求值不同——为重采样器提供基准）
- `**push_audio**`：推送音频帧到平台内部的播放缓冲。返回实际接受的帧数——缓冲满时为 0，调用方可选择丢弃或重试。平台内部负责缓冲管理
- `**pause_audio**`：暂停/恢复音频输出——用于睡眠/唤醒。调用时机：`PWR_fauxSleep` 中

#### 电源（3 方法）——设备管理


| 方法                   | 签名                                              | 默认实现 |
| -------------------- | ----------------------------------------------- | ---- |
| `power_off`          | `fn power_off(&mut self)`                       | 无    |
| `get_battery_status` | `fn get_battery_status(&self) -> BatteryStatus` | 无    |
| `set_cpu_speed`      | `fn set_cpu_speed(&mut self, speed: CpuSpeed)`  | 无    |


- `**get_battery_status**`：返回电池状态快照。每帧由 `PWR_update` 调用——不跨帧缓存，每次都是新鲜数据。`BatteryStatus` 只反映硬件状态；跨帧的逻辑推导由 `PowerState` 负责

#### 硬件（4 方法）——背光 / 震动 / 网络 / HDMI


| 方法                  | 签名                                             | 默认实现       |
| ------------------- | ---------------------------------------------- | ---------- |
| `enable_backlight`  | `fn enable_backlight(&mut self, enable: bool)`  | 无          |
| `set_rumble`        | `fn set_rumble(&mut self, strength: u8)`        | 空操作        |
| `is_online`         | `fn is_online(&self) -> bool`                   | 返回 `false` |
| `is_hdmi_active`    | `fn is_hdmi_active(&self) -> bool`              | 返回 `false` |


- `**enable_backlight**`：开关背光——`prepare_sleep` 关、`complete_wake` 开（对应原 C `PLAT_enableBacklight`）。调用时机：睡眠/唤醒、关机
- `**set_rumble**`：设置震动强度（0=关闭）。默认空操作——不是所有设备都有震动硬件（tg5040 没有）
- `**is_online**`：WiFi 连接状态。默认 `false`——无 WiFi 的设备无需覆盖。对应 C `PLAT_isOnline()`
- `**is_hdmi_active**`：HDMI 当前是否连接（运行时检测，不同于 `HAS_HDMI` 编译期常量）。默认 `false`——无 HDMI 的设备无需覆盖。被 `PWR_update`（睡眠阻止）和 `PWR_powerOff`（关机分辨率）使用

#### 睡眠/唤醒生命周期（3 方法）


| 方法              | 签名                              | 默认实现       |
| --------------- | ------------------------------- | ---------- |
| `prepare_sleep` | `fn prepare_sleep(&mut self)`   | 无          |
| `should_wake`   | `fn should_wake(&self) -> bool` | 返回 `false` |
| `complete_wake` | `fn complete_wake(&mut self)`   | 无          |


这三个方法覆盖睡眠/唤醒生命周期，替代原 C 代码中散落的 `system("killall -STOP keymon.elf")` 等平台特定命令：`prepare_sleep` 关背光、暂停音频、停止按键守护进程、sync 文件系统；`should_wake` 在 `PWR_fauxSleep` 的唤醒等待循环中每 200ms 查询一次唤醒事件（电源键/合盖打开）；`complete_wake` 反向恢复。

#### 布局（2 方法）——平台差异化布局值

| 方法 | 签名 | 默认实现 |
| ---- | ---- | ---- |
| `main_row_count` | `fn main_row_count(&self) -> u32` | 返回 `video::MAIN_ROW_COUNT`（通用默认 6） |
| `padding` | `fn padding(&self) -> u32` | 返回 `video::PADDING`（通用默认 10） |

平台差异的布局值（行数/留白）经方法覆盖表达（tg5040：smart 8/40、brick 7/5）。**为什么是方法而非关联常量？** 既有平台（my355/rg35xxplus）存在 `on_hdmi` 运行时分支（HDMI 插拔时 6↔8 行切换，platform.c:447）——运行时状态只能方法表达；统一用方法避免同一概念两套形态（详见 tg5040 文档）。

#### 时间（2 方法）


| 方法              | 签名                                                | 默认实现 |
| --------------- | ------------------------------------------------- | ---- |
| `now_ms`        | `fn now_ms(&self) -> u32`                         | 无    |
| `set_date_time` | `fn set_date_time(&mut self, y,m,d,h,min,sec: i32)` | 空操作  |


单调时钟，毫秒精度。替代 SDL 的 `SDL_GetTicks()`。被 `PWR_update`（空闲计时）、`FrameTimer`（帧计时）、`tapped_menu`（轻触超时检测）等多个模块使用。

- `**set_date_time**`：设置系统时间。默认空操作——**不是"设备没有 RTC"**：原版 C 全部 13 平台都经 `date + hwclock` 系统命令设时（api.c:1717-1722，依赖系统命令而非硬件直写），默认空实现只是"依赖系统命令、属平台实现细节"的**覆盖点**（与 `set_rumble` 真无硬件不同）；tg5040 用 `Command` 免 shell 转义实现

### 默认实现（10 个方法）

提供默认实现的方法允许平台**按需覆盖**——没有对应硬件的设备无需写空函数体：


| 方法                 | 默认行为                 | 哪些平台需覆盖            | 哪些不需              |
| ------------------ | -------------------- | ------------------ | ----------------- |
| `set_vsync`        | 空操作                  | 需要在运行时切换 vsync 的平台 | tg5040（创建渲染器时已决定） |
| `should_wake`      | 返回 `false`           | 支持合盖唤醒或电源键唤醒的平台    | 无此类硬件的平台          |
| `pick_sample_rate` | `requested.min(max)` | 有特殊采样率限制的平台        | 绝大多数平台            |
| `set_rumble`       | 空操作                  | 有震动硬件的平台           | tg5040（无震动）       |
| `is_online`        | 返回 `false`           | 有 WiFi 的平台         | 纯离线掌机             |
| `is_hdmi_active`   | 返回 `false`           | 有 HDMI 输出的平台       | 纯手持设备             |
| `reset_input`      | 空操作                  | 有睡眠功能的平台（全部）    | 无                  |
| `main_row_count`   | 返回 `video::MAIN_ROW_COUNT`（6） | 布局值非通用默认的平台（tg5040 smart 8 / brick 7） | 采用通用布局的平台 |
| `padding`          | 返回 `video::PADDING`（10） | 同上（tg5040 smart 40 / brick 5） | 采用通用布局的平台 |
| `set_date_time`    | 空操作                  | 需真设时的平台（原版 13 平台全经 `date`+`hwclock`） | 无——默认空实现是覆盖点，非"无 RTC" |


**设计原则**："不是所有设备都有震动/HDMI/WiFi/关机键"。默认实现让平台实现者只需关注自己设备上真正存在的硬件功能。

### 方法依赖图谱

不同上层模块依赖 `Platform` trait 的不同子集：

```
minui 依赖:                 minarch 依赖:
  init_video / quit_video     init_video / quit_video
  flip                        flip
  poll_input                  poll_input
  init_input / quit_input     init_audio / quit_audio
  now_ms                      push_audio
  device_model                pick_sample_rate
  set_cpu_speed               pause_audio
                              set_cpu_speed
                              set_rumble
                              is_hdmi_active

PWR_update 依赖:            PWR_fauxSleep 依赖:
  poll_input                  prepare_sleep
  get_battery_status          complete_wake
  now_ms                      enable_backlight
                              pause_audio
                              should_wake
                              reset_input
                              now_ms
```

**设计中蕴含的分工**：minui 不知道音频的存在（它不播放声音）——所以 minui 的依赖列表里没有任何音频方法。电源管理相关方法对 minui 和 minarch 都是透明的——它们只需调用 `PWR_update(...)`（返回动作信号），睡眠/关机的实际执行（`faux_sleep`/`power_off`）由装配层完成。**`PWR_fauxSleep` 依赖的 `reset_input`**：睡眠前/唤醒后各调用一次（对应 C 的两次 `PAD_reset`），防止睡眠按键事件残留导致唤醒后误触发。

### 在 minui/minarch 中使用

`main()` 函数中的典型初始化序列：

```rust
fn main() {
    // 平台实例——只在 main.rs 中创建一次
    #[cfg(feature = "platform-tg5040")]
    let mut platform = platform_tg5040::Tg5040::new();

    run(&mut platform);
}

fn run<P: Platform>(platform: &mut P) {
    // 1. 初始化硬件按固定顺序
    platform.init_video();
    let mut screen = VideoBuffer::new(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);
    platform.init_input();
    platform.init_audio(
        platform.pick_sample_rate(44100, 48000)
    );

    // 2. 加载资源
    let atlas = load_atlas(".system/res/assets@2x.png").expect("图集缺失");
    let font_data = std::fs::read(FONT_PATH).expect("字体缺失");
    let font = Font::from_bytes(font_data, FontSettings::default()).expect("字体无效");

    // 3. 主循环
    let mut power_state = PowerState::new();
    loop {
        let input = platform.poll_input();
        let battery = platform.get_battery_status();
        let now = platform.now_ms();

        let (dirty, show_setting) = PWR_update(
            &mut power_state, &input, &battery, now,
            Some(|| platform.prepare_sleep()),
            Some(|| platform.complete_wake()),
        );
        if dirty && power_state.should_power_off {
            // 渲染关机提示...
            platform.flip(&screen, false);
            platform.power_off();
        }

        // 渲染...
        screen.fill_rect(screen_rect, RGB_BLACK);
        // ... blit UI ...
        platform.flip(&screen, true);
    }

    // 4. 反初始化（正常退出路径，掌机上几乎不会走到）
    platform.quit_input();
    platform.quit_audio();
    platform.quit_video();
}
```

### 平台实现者的视角

tg5040 是第一个完整实现 `Platform` trait 的平台（全部 27 个方法已实现，见 `platforms/tg5040/src/lib.rs`）。以下是它的实现要点——未来平台的移植指引：

```rust
impl Platform for Tg5040 {
    // 关联常量——编译期确定，按 feature 区分设备（正向条件，画布 = 物理分辨率）
    #[cfg(feature = "smart")]
    const SCREEN_WIDTH: u32 = 1280;   // smart：TrimUI Smart Pro 物理宽
    #[cfg(feature = "brick")]
    const SCREEN_WIDTH: u32 = 1024;   // brick：TrimUI Brick 物理宽
    const SCREEN_HEIGHT: u32 = 720;   // （brick 下为 768，同样用 #[cfg] 区分）
    const SCALE: u32 = 2;             // 图集倍率 + 布局倍率（brick 下为 3）
    const BYTES_PER_PIXEL: u8 = 2;
    const HAS_HDMI: bool = true;
    const HAS_POWER_BUTTON: bool = true;
    const HAS_POWEROFF_BUTTON: bool = false;
    const SUPPORTS_OVERSCAN: bool = false;
    #[cfg(feature = "smart")]
    const DEVICE_MODEL: &str = "TrimUI Smart Pro";
    #[cfg(feature = "brick")]
    const DEVICE_MODEL: &str = "TrimUI Brick";

    // 27 个方法已全部实现（无未实现方法）
    // 17 个无默认实现的方法：视频/输入/音频/电源各子系统（见各平台 README）
    // 10 个有默认实现的方法无需覆盖（或按需覆盖）：
    //   is_online / set_date_time / set_vsync / set_rumble / should_wake /
    //   reset_input / pick_sample_rate / main_row_count / padding / is_hdmi_active
    // ...
}
```

**为什么示例值是真实实现而非示意值？** 早期版本曾用示意值（`SCREEN_WIDTH=640` 等）演示 trait 实现——示意值容易让读者误以为是真实硬件参数。tg5040 实现定稿后，示例直接采用真实值（1280×720、画布=物理、feature 区分设备），与 `platforms/tg5040/src/lib.rs` 完全一致。

### 与原 C PLAT_* 体系的对比

原 C `api.h` 声明了 34 个 `PLAT_*` 函数（通过 `#define` 宏转发到平台实现）。Rust `Platform` trait 收敛为 27 个方法（去向表如下，表格末行的合计按 27 计）：


| 去向                       | 数量  | 示例                                                                                                  |
| ------------------------ | --- | --------------------------------------------------------------------------------------------------- |
| render crate 替代          | 5   | `PLAT_getScaler`、`PLAT_blitRenderer`、`PLAT_setNearestNeighbor`、`PLAT_setSharpness`、`PLAT_setEffect` |
| 上层用 VideoBuffer 替代       | 3   | `PLAT_clearVideo`（→ `fill_rect(BLACK)`）、`PLAT_clearAll`、`PLAT_resizeVideo`                          |
| 空函数                      | 2   | `PLAT_setVideoScaleClip`（tg5040 空实现）、`PLAT_setEffectColor`                                          |
| 合并到 poll_input           | 2   | `PLAT_initLid`、`PLAT_lidChanged`（合盖检测 → 平台内部处理，翻译为 `BTN_SLEEP`）                                     |
| Overlay 不需要              | 3   | `PLAT_initOverlay`、`PLAT_quitOverlay`、`PLAT_enableOverlay`（低电量直接渲染到主 VideoBuffer）                   |
| SDL_Delay → now_ms+sleep | 1   | `PLAT_vsync`                                                                                        |
| 新增（C 无直接对应）              | 5   | `prepare_sleep`、`complete_wake`、`now_ms`、`is_hdmi_active`（运行时检测）、`set_vsync`（显式控制）                  |


---

## audio——音频管线

audio 模块提供音频数据类型和处理工具。**不涉及任何平台相关的音频设备操作**——打开/关闭设备、实际播放由 `Platform` trait 实现负责。模块职责：


| 类型                | 职责                           |
| ----------------- | ---------------------------- |
| `AudioFrame`      | 单帧立体声音频采样（左+右声道，各一个 `i16`）   |
| `AudioRingBuffer` | 固定容量的环形音频缓冲（SPSC），无内部锁——可选工具 |
| `Resampler`       | 音频重采样器（直通 + 最近邻插值），纯数学转换     |


### 完整音频管线

从模拟器核心到扬声器的数据流：

```
┌─────────────────────────────────────────────────────────┐
│  模拟器核心（libretro core，独立线程）                     │
│  retro_audio_sample_batch(left, right)  ← 回调，每帧 N 帧音频 │
└──────────────────────┬──────────────────────────────────┘
                       │ AudioFrame 切片
                       ▼
┌─────────────────────────────────────────────────────────┐
│  minarch 音频引擎（主线程）                                │
│                                                         │
│  for each input_frame:                                  │
│    result = resampler.process(frame)                     │
│    if result.write_output:                              │
│        output_buffer.push(frame)    ← 上采样会重复写入     │
│    if result.advance_input:                             │
│        next_frame()                 ← 下采样会跳过帧       │
│                                                         │
│  platform.push_audio(&output_buffer) ← 批量推送           │
│  // 返回实际接受帧数——满时丢弃或重试                        │
└──────────────────────┬──────────────────────────────────┘
                       │ 通过 Platform trait
                       ▼
┌─────────────────────────────────────────────────────────┐
│  AudioRingBuffer（common::audio）                         │
│                                                         │
│  固定容量环形缓冲——SPSC（单生产者单消费者）                   │
│  无内部锁——&mut self 在编译期保证互斥                       │
│                                                         │
│  ┌───┬───┬───┬───┬───┬───┬───┬───┐                      │
│  │ A │ B │ C │   │   │   │   │   │  ← 填充区             │
│  └───┴───┴───┴───┴───┴───┴───┴───┘                      │
│    ↑ read_pos=0      ↑ write_pos=3   filled=3           │
│                  生产者(主线程)    消费者(SDL回调线程)       │
└──────────────────────┬──────────────────────────────────┘
                       │ pop(&mut [AudioFrame])
                       ▼
┌─────────────────────────────────────────────────────────┐
│  平台层（platform-tg5040，SDL 音频回调线程）                │
│                                                         │
│  Tg5040 {                                               │
│      ring_buffer: Mutex<AudioRingBuffer>,                │
│      audio_device: SDL_AudioDevice,                      │
│  }                                                      │
│                                                         │
│  push_audio(&mut self, frames):                         │
│    self.ring_buffer.lock().push(frames)   ← 生产者        │
│                                                         │
│  SDL 音频回调（独立线程）:                                 │
│  ┌───────────────────────────────────────────┐           │
│  │ sdl_audio_callback(out: &mut [u8], len) {  │           │
│  │     let frames = to_frames_mut(out);       │           │
│  │     let read = self.ring_buffer            │           │
│  │         .lock().pop(frames);  ← 消费者      │           │
│  │     if read == 0 {                         │           │
│  │         out.fill(0);  // 静音               │           │
│  │     }                                      │           │
│  │ }                                          │           │
│  └───────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────┘
```

**关键分界**：主线程（生产者）和 SDL 音频回调线程（消费者）是两个独立线程。它们通过 `Mutex<AudioRingBuffer>` 同步——Mutex 提供线程安全的内部可变性，AudioRingBuffer 本身不包含任何锁。

### AudioFrame——立体声采样单元

```rust
pub struct AudioFrame {
    pub left: i16,   // 左声道采样值（-32768 到 32767）
    pub right: i16,  // 右声道采样值
}
```

一帧音频 = 一个采样时刻的左右声道各一个值 = 4 字节。这是 CD 音质的 16-bit PCM 格式。模拟器核心每帧产生 735-1470 帧音频（44100Hz / 60fps ≈ 735 帧/帧到 48000Hz / 60fps ≈ 800 帧/帧），主循环将它们批量推送到平台。

### AudioRingBuffer——无锁环形缓冲

#### 数据结构

```rust
pub struct AudioRingBuffer {
    buffer: Vec<AudioFrame>,   // 后端存储（固定容量）
    capacity: usize,            // 总容量（帧数）
    read_pos: usize,            // 消费者读取位置
    write_pos: usize,           // 生产者写入位置
    filled: usize,              // 当前已填充帧数（0..=capacity）
}
```

#### 为什么不需要内部锁？

`push` 和 `pop` 的签名都是 `&mut self`——Rust 的借用规则在编译期保证**同一时刻只能有一个可变引用**指向该缓冲：

```rust
pub fn push(&mut self, frames: &[AudioFrame]) -> usize { /* ... */ }
pub fn pop(&mut self, out: &mut [AudioFrame]) -> usize { /* ... */ }
```

在单线程轮询场景（如主线程中直接 push→pop），直接调用即可——零锁开销。在跨线程场景（主线程 push + SDL 回调线程 pop），调用方用 `Mutex<AudioRingBuffer>` 包裹：

```rust
// 平台结构体中：
struct Tg5040 {
    ring_buffer: Mutex<AudioRingBuffer>,  // Mutex 提供内部可变性
    // ...
}

// 生产者（主线程）：
self.ring_buffer.lock().unwrap().push(&frames);
// Mutex::lock → &mut AudioRingBuffer → push(&mut self, ...)

// 消费者（SDL 回调线程）：
let read = self.ring_buffer.lock().unwrap().pop(out);
```

**为什么不是 AudioRingBuffer 内部加锁？** 分离缓冲逻辑和同步策略。单线程测试场景不需要任何锁——直接用 `&mut self` 调用，零开销。将 Mutex 放在平台层让每个平台根据自己的线程模型选择同步策略。

#### 为什么是 all-or-nothing push？

```rust
pub fn push(&mut self, frames: &[AudioFrame]) -> usize {
    if frames.len() > self.capacity - self.filled {
        return 0;  // 空间不够——不做部分写入
    }
    // ... 完整写入
    frames.len()
}
```

音频帧是连续的——部分写入会破坏立体声配对。一帧 = 左声道 + 右声道，部分写入可能切断一个立体声对。调用方（minarch 音频引擎）如何处理返回 0：丢弃这批帧（空缓冲使 SDL 回调输出静音），或短暂自旋等待消费者腾出空间。

#### 环形缓冲绕回

```
初始状态（capacity=8，空）:
┌───┬───┬───┬───┬───┬───┬───┬───┐
│   │   │   │   │   │   │   │   │  read=0, write=0, filled=0
└───┴───┴───┴───┴───┴───┴───┴───┘

push 5 frames:
┌───┬───┬───┬───┬───┬───┬───┬───┐
│ 0 │ 1 │ 2 │ 3 │ 4 │   │   │   │  read=0, write=5, filled=5
└───┴───┴───┴───┴───┴───┴───┴───┘

pop 3 frames:
┌───┬───┬───┬───┬───┬───┬───┬───┐
│   │   │   │ 3 │ 4 │   │   │   │  read=3, write=5, filled=2
└───┴───┴───┴───┴───┴───┴───┴───┘

push 5 more frames（write_pos 从 5 绕回到 2）:
┌───┬───┬───┬───┬───┬───┬───┬───┐
│ 6 │ 7 │   │ 3 │ 4 │ 5 │ 6'│ 7'│  read=3, write=2, filled=7
└───┴───┴───┴───┴───┴───┴───┴───┘
```

`write_pos` 和 `read_pos` 各自独立循环——当越过 `capacity-1` 时绕回到 0。读写位置相互追逐：`write_pos` 追上 `read_pos` → 缓冲满；`read_pos` 追上 `write_pos` → 缓冲空。

### Resampler——采样率转换

#### 为什么需要重采样？

模拟器核心通常产生 44100Hz 音频（CD 标准采样率），但部分硬件只能播放 48000Hz（如某些 Allwinner 芯片的 HDMI 音频路径）。直接用 44100 的数据喂给期望 48000 的硬件 → 音调偏移：音频听起来更快、更高（因为波形被压缩了 ~8.8%）。

`Resampler` 提供两种模式：

- **Passthrough**：输入采样率 == 输出采样率——每帧写入并消费，零处理
- **Nearest（最近邻）**：输入采样率 ≠ 输出采样率——通过 Diff 累加器选择性跳帧或复制帧

#### Diff 累加器算法

核心思想：用 diff 计数器追踪输入时钟和输出时钟的相对偏移。当输出时钟"领先"时写入帧，当输入时钟"追上"时前进到下一输入帧。

```
Nearest 模式核心循环（与原 C api.c:1009-1018 完全一致）:

diff = 0 （初始值）

for each input_frame:
    if diff < sample_rate_out:      ← 输出时钟"领先"——本帧应该被输出
        write_output = true           写入缓冲区
        diff += sample_rate_in        diff 前进一帧输入所代表的"距离"

    if diff >= sample_rate_out:     ← 输入时钟"追上"输出——消费输入帧
        advance_input = true         前进到下一帧
        diff -= sample_rate_out       diff 后退一帧输出所代表的"距离"
```

#### 上采样示例（44100Hz → 48000Hz）

当输出速率高于输入速率时，某些输入帧被**复制**以填充输出时钟：

```
Frame 1: diff=0 < 48000 → write, diff=44100
          diff=44100 < 48000? NO → 不 advance
         （第一帧被写入，但不前进——它将在输出时钟追上之前被再次处理）

Frame 1: diff=44100 < 48000 → write, diff=88200   ← 同一帧被写入两次！
          diff=88200 >= 48000 → advance, diff=40200
         （第二帧才消费 Frame 1）


输出: [F1, F1, ...] ← Frame 1 出现两次

结果: 44100 个输入帧 → ~48000 个输出帧（≈1.088x 上采样比）
```

#### 下采样示例（48000Hz → 44100Hz）

当输出速率低于输入速率时，某些输入帧被**跳过**：

```
Frame 1: diff=0 < 44100 → write, diff=48000
          diff=48000 >= 44100 → advance, diff=3900

Frame 2: diff=3900 < 44100 → write, diff=51900
          diff=51900 >= 44100 → advance, diff=7800

Frame 3: diff=7800 < 44100 → write, diff=55800
          diff=55800 >= 44100 → advance, diff=11700

Frame 4: diff=11700 < 44100 → write, diff=59700
          diff=59700 >= 44100 → advance, diff=15600

... 经过约 12 帧 ...

Frame 12: diff=0 < 44100 → write, diff=48000
           diff=48000 >= 44100 → advance, diff=3900
           ← 被写入

Frame 13: diff=3900 < 44100 → write, diff=51900
           diff=51900 >= 44100 → advance, diff=7800
           ← 被写入

Frame 14: diff=7800 < 44100 → write, diff=55800
           diff=55800 >= 44100 → advance, diff=11700
           ← 被写入，但 diff 还没追上? 继续...

Frame 15: diff=11700 < 44100 → write, diff=59700
           diff=59700 >= 44100 → advance, diff=15600
           ← 被写入

Frame 16: diff=15600 < 44100 → write, diff=63600
           diff=63600 >= 44100 → advance, diff=19500
           ← 被写入

... 经过大量帧后，偶尔有一帧: diff=43000 < 44100 → write, diff=91000
                               diff=91000 >= 44100 → advance, diff=46900
                               ← 写入但前进

           下下一帧: diff=46900 >= 44100 → NOT write（跳过！）
                     diff=46900-44100=2800，advance

结果: 48000 个输入帧 → ~44100 个输出帧（≈0.919x 下采样比）
每约 12.25 帧跳 1 帧
```

#### `ResampleResult` 双信号协议

```rust
pub struct ResampleResult {
    pub advance_input: bool,  // 调用方是否前进到下一个输入帧
    pub write_output: bool,   // 本帧是否应该写入输出缓冲
}
```

两个 `bool` 有四种组合：


| write_output | advance_input | 含义                 | 何时出现                       |
| ------------ | ------------- | ------------------ | -------------------------- |
| true         | true          | 正常输出并前进            | Passthrough 模式、Nearest 普通帧 |
| true         | false         | 输出但不前进——下帧仍处理同一输入帧 | 上采样时（复制帧）                  |
| false        | true          | 不输出但前进——本输入帧被跳过    | 下采样时（跳帧）                   |
| false        | false         | 不输出也不前进            | 不会出现（每次调用至少产生一个 true）      |


**为什么是两个 bool 而不是 enum？** 直接对应 C 代码的双变量 `consumed` 和 `frame_written`——方便逐行对照验证行为一致性。

---

## video——视频类型与常量

`video` 模块定义渲染管线中的纯数据类型：像素缓冲、坐标、精灵图集系统的元数据、UI 布局常量。它是 render crate 和上层代码之间的"数据协议"。

### VideoBuffer 与 fill_rect

```rust
pub struct VideoBuffer {
    pub pixels: Vec[[ORCA_RICH_MD:ed8b7bba36012943fc27b8046d4c0055:inline-html:%3Cu16%3E]],   // RGB565 像素数据
    pub width: u32,          // 可见宽度
    pub height: u32,         // 可见高度
    pub pitch: u32,          // 每行像素数
}
```

`VideoBuffer::new(w, h)` 创建 `pixels.len() == (w * h)` 且 `pitch == width` 的缓冲区。

`fill_rect(rect, color)` 是最基础的渲染原语——用 RGB565 颜色填充矩形区域内所有像素。**不做边界裁剪**——调用方确保 rect 完全位于画布内。

```rust
let mut buf = VideoBuffer::new(640, 480);
buf.fill_rect(Rect { x: 0, y: 0, w: 320, h: 240 }, RGB_BLACK);
// 擦除画布上半部分为黑色
```

### Asset 精灵图集系统

精灵图集是一张 100×64（未缩放）的 PNG 图片，包含 MinUI 全部 UI 素材。使用机制：

1. **启动时**：`render::load_atlas()` 一次性加载整张 PNG → `render::Atlas`（RGB565 像素数组）
2. **渲染时**：`render::blit_asset()` 从 Atlas 中裁切指定 Asset 的矩形区域 → 复制到目标 VideoBuffer

`common::video` 提供图集的**元数据**——不包含实际像素（那是 render 的职责）：

`**Asset` 枚举**：26 个精灵的标识符（`#[repr(usize)]`，可直接转为数组索引）：

```
Index 0  → WhitePill（单色素材——形状在精灵图集中，颜色由 ASSET_RGBS 填充）
Index 1  → BlackPill
...
Index 13 → Hole（单色素材最后一个）
Index 14 → Colors（分隔符——非渲染成员，标记单色素材与彩色素材的边界）
Index 15 → BatteryIcon（彩色素材——直接使用精灵图集中的原像素颜色）
...
Index 25 → Wifi
Index 26 → Count（成员数量）
```

`**ASSET_RECTS**`：`[Rect; 26]`——每个 Asset 在图集中的源矩形坐标（未缩放）。画布尺寸 100×64，坐标均在此范围内。

`**ASSET_RGBS**`：`[u16; 14]`——索引 0-13 单色素材的颜色值。对应 C 代码中在精灵的透明区域用 `SDL_FillRect` 填充纯色的逻辑。

### 布局常量

所有 UI 布局常量**唯一**定义在 `common::video`——这是项目的强制执行原则：


| 常量               | 值   | 来源 C           | 含义           |
| ---------------- | --- | -------------- | ------------ |
| `PILL_SIZE`      | 30  | `defines.h:51` | 药丸按钮基准尺寸     |
| `BUTTON_SIZE`    | 20  | `defines.h:52` | 按钮 sprite 尺寸 |
| `BUTTON_MARGIN`  | 5   | `defines.h:53` | 按钮与文字间距      |
| `BUTTON_PADDING` | 12  | `defines.h:54` | 按钮组内部填充      |
| `PADDING`        | 10  | `defines.h:63` | 页面边缘留白       |
| `SETTINGS_SIZE`  | 4   | `defines.h:55` | 设置滑条高度       |
| `SETTINGS_WIDTH` | 80  | `defines.h:56` | 设置滑条宽度       |
| `BRIGHTNESS_MIN` | 0   | `defines.h:8`  | 亮度最小值        |
| `BRIGHTNESS_MAX` | 10  | `defines.h:9`  | 亮度最大值        |
| `VOLUME_MIN`     | 0   | `defines.h:6`  | 音量最小值        |
| `VOLUME_MAX`     | 20  | `defines.h:7`  | 音量最大值        |


**为什么不能在其他 crate 中本地定义？**

```rust
// ❌ 错误——两个 crate 各自定义"相同"的常量，编译器不保证它们相等
// crates/render/src/button.rs:  const BUTTON_SIZE: u32 = 20;
// crates/minarch/src/menu.rs:   const BUTTON_SIZE: u32 = 20;

// ✅ 正确——唯一来源
// crates/render/src/button.rs:  use common::video::BUTTON_SIZE;
// crates/minarch/src/menu.rs:   use common::video::BUTTON_SIZE;
```

C 代码的 `#define` 在头文件中——`#include "defines.h"` 解决了单一来源。Rust 没有预处理器——常量通过 `use` 共享。如果两个 crate 各自定义 `const`，修改一个忘记另一个会导致不一致的布局偏移。

### VsyncMode

```rust
pub enum VsyncMode {
    Off = 0,      // 不等待——画面可能撕裂
    Lenient = 1,  // 尝试等待——但不严格
    Strict = 2,   // 严格等待——保证无撕裂
}
```

`FRAME_BUDGET_MS = 17`：每帧时间预算（约 60fps）。上层帧计时器用此值计算是否需要跳过 vsync。

---

## input——输入状态

`input` 模块定义按键位掩码常量、输入状态结构体和菜单轻触检测逻辑。与 C 的 `PAD_*` 系列函数完全对应。

### 位掩码常量

```rust
pub const BTN_ID_DPAD_UP: usize = 0;
pub const BTN_ID_DPAD_DOWN: usize = 1;
// ... 共 25 个按键，索引 0-24 ...
pub const BTN_ID_POWEROFF: usize = 20;
pub const BTN_ID_ANALOG_UP: usize = 21;    // 模拟摇杆方向（平台 poll_input 翻译）
pub const BTN_ID_ANALOG_DOWN: usize = 22;
pub const BTN_ID_ANALOG_LEFT: usize = 23;
pub const BTN_ID_ANALOG_RIGHT: usize = 24;
pub const BTN_ID_COUNT: usize = 25;        // 与原版 defines.h 一致

pub const BTN_DPAD_UP: u32 = 1 << BTN_ID_DPAD_UP;   // = 0b000...0001
pub const BTN_DPAD_DOWN: u32 = 1 << BTN_ID_DPAD_DOWN; // = 0b000...0010
pub const BTN_A: u32 = 1 << BTN_ID_A;                  // = 0b000...0100
// ... 共 25 个掩码（含 BTN_ANALOG_UP/DOWN/LEFT/RIGHT）

// 方向键组合掩码
pub const BTN_UP: u32 = BTN_DPAD_UP;
pub const BTN_DOWN: u32 = BTN_DPAD_DOWN;
pub const BTN_LEFT: u32 = BTN_DPAD_LEFT;
pub const BTN_RIGHT: u32 = BTN_DPAD_RIGHT;
```

**ANALOG 键位**（21-24）是**模拟摇杆方向**——摇杆轴超过 deadzone 时由平台
`poll_input` 翻译为方向键位（原版 `PAD_setAnalog`）。`u32` 位掩码容纳
25 位，查询方法与普通键位完全一致。原版 defines.h 同样是 25 个键位——
本平台（tg5040）当前无摇杆消费方（**TrimUI Brick Pro** 双摇杆落地时使用）。

### InputState——查询方法

```rust
pub struct InputState {
    pub pressed: u32,
    pub just_pressed: u32,
    pub just_released: u32,
    pub just_repeated: u32,
    pub laxis: (i32, i32),   // 左摇杆原始轴值 (x, y)
    pub raxis: (i32, i32),   // 右摇杆原始轴值 (x, y)
}
```

每个字段是一个 `u32` 位掩码——25 个按键 x 4 个状态。`laxis`/`raxis`
是摇杆**原始轴值**（对应原版 C `pad.laxis/raxis`，`PAD_Axis`），由平台
`poll_input` 随帧快照填充（SDL 原始值 -32768..32767 透传，无缩放），
供 minarch 应答 libretro 核心的 `RETRO_DEVICE_ANALOG` 查询。查询方法：

```rust
// 单个按键检测
input.just_pressed(BTN_A)        // A 在本帧刚按下
input.is_pressed(BTN_A)          // A 当前按住
input.just_released(BTN_A)       // A 在本帧刚释放
input.just_repeated(BTN_A)       // A 在本帧产生重复信号

// 批量检测
input.any_pressed()              // 任何键被按住
input.any_just_pressed()         // 任何键在本帧刚被按下
input.reset()                    // 清除本帧的 just_pressed/just_released/just_repeated
```

典型主循环使用模式：

```rust
loop {
    let input = platform.poll_input();  // 读取本帧输入

    if input.just_pressed(BTN_UP) {
        selected = selected.saturating_sub(1);  // 光标上移
    }
    if input.just_pressed(BTN_DOWN) {
        selected += 1;                          // 光标下移
    }
    if input.just_pressed(BTN_A) {
        open_entry(selected);                   // 确认选择
    }
    if input.just_pressed(BTN_B) {
        go_back();                              // 返回上级
    }
}
```

### tapped_menu——250ms 轻触检测

`tapped_menu` 是一个自由函数（非方法），用于检测 MENU 键是否做了"短按"（区别于"长按进入睡眠"）：

```rust
/// MENU 键轻触状态（字段私有——经 MenuTapState::new() 构造）
pub struct MenuTapState { /* menu_start, ignore_menu */ }

/// 平台修饰键映射（对应 C platform.h 的 BTN_MOD_* 宏）
pub struct ModKeys {
    pub brightness: u32,   // P::BTN_MOD_BRIGHTNESS（trait 关联常量）
    pub volume: u32,       // P::BTN_MOD_VOLUME
    pub plus: u32,         // P::BTN_MOD_PLUS
    pub minus: u32,        // P::BTN_MOD_MINUS
}

pub fn tapped_menu(
    state: &InputState, tap: &mut MenuTapState, now_ms: u32,
    mod_keys: ModKeys,                     // 平台常量（调用方传入）
) -> bool
```

时序逻辑：

```
MENU 按下 → menu_start = now_ms
       │
       ├── < 250ms 内释放 → tapped_menu = true（触发菜单）
       │
       ├── >= 250ms → ignore_menu = true（长按——将触发睡眠，不触发菜单）
       │
       └── 按住 MENU 且按了 mod_keys.plus/minus → ignore_menu = true
           （仅当 mod_keys.brightness == BTN_MENU 时——调亮度不触发菜单）
```

**`mod_keys` 参数为什么存在**：原版 `PAD_tappedMenu` 的修饰键条件
（`BTN_MOD_BRIGHTNESS == BTN_MENU`）是平台宏编译期展开——不同设备的
修饰键不同（tg5040 为 MENU+PLUS；m17/trimuismart 为 START+R1）。common
是跨平台库，硬编码 `BTN_MENU`/`BTN_PLUS` 会把 tg5040 的语义泄漏成
m17 类平台的 bug——所以修饰键常量归 `Platform` trait 关联常量
（`BTN_MOD_*`），由调用方（装配层）从 `P::BTN_MOD_*` 组装 `ModKeys`
传入，与 `power::update` 的 `mute` 参数同一模式
（**平台定义常量 → 参数传递 → common 消费**）。

`MenuTapState` 替代了原 C 的两个 `static` 局部变量 `menu_start` 和 `ignore_menu`。

---

## power——电源管理状态机

`power` 模块定义电源相关类型和 `update` 状态机——一个每帧调用的函数，统一处理充电检测、空闲睡眠、电源键长按关机、亮度/音量调节的修饰键逻辑。

### 类型层次

```rust
// CPU 速度档位——各平台的具体频率由 Platform::set_cpu_speed 实现决定
pub enum CpuSpeed { Menu, Powersave, Normal, Performance }

// 电池状态快照——每帧由 Platform::get_battery_status() 获取，不跨帧缓存
pub struct BatteryStatus { pub charging: bool, pub percentage: u8 }

// 电源状态机的跨帧记忆——由调用方持有，每帧传入 update
pub struct PowerState {
    // ── 控制标志（由装配层按场景设置）──
    pub can_sleep: bool,           // 睡眠键是否响应（false = 按睡眠键无效）
    pub can_poweroff: bool,        // 电源键是否可关机（false = 如 HDMI 输出防误触）
    pub can_autosleep: bool,       // 是否允许 30 秒无操作自动睡眠（false = 如游戏运行中）
    pub requested_sleep: bool,     // 硬件请求睡眠（预留钩子，当前无代码设置）
    pub requested_wake: bool,      // 硬件请求唤醒（预留钩子）
    pub should_warn: bool,         // 低电量警告是否启用（warn() 控制）
    // ── 跨帧计时状态 ──
    pub last_input_at: u32,        // 最后一次按键时刻（自动睡眠倒计时）
    pub checked_charge_at: u32,    // 上次充电检查时刻（CHARGE_DELAY = 1000ms 间隔）
    pub setting_shown_at: u32,     // 设置面板开始显示时刻
    pub show_setting: u8,          // 设置面板状态（0=无，1=亮度，2=音量）——跨帧记忆
    pub power_pressed_at: u32,     // 电源键按下时刻（长按 ≥1000ms 关机检测）
    pub mod_unpressed_at: u32,     // 修饰键最后松开时刻
    pub was_muted: bool,           // 上一帧静音状态（切换检测）
    pub was_charging: bool,        // 上一帧充电状态（切换检测）
}
```

**控制标志与控制方法**：`can_*` 三标志由装配层按场景设置——`disable_sleep()`（如 `!HAS_POWER_BUTTON` 设备）、`disable_auto_sleep()`（游戏运行中禁自动睡眠）、`warn(enable)`（低电量警告）。`requested_sleep`/`requested_wake` 是硬件请求预留钩子。辅助自由函数：`ignore_setting_input(btn, show_setting)`（设置面板消费 PLUS/MINUS——面板弹出时 PLUS/MINUS 不再触发修饰键逻辑）、`prevent_auto_sleep(battery, hdmi_active, can_autosleep)`（充电/HDMI 时刷新 last_input_at 阻止倒计时）。

### BatteryStatus vs PowerState——为什么要分开？

```
per-frame                                                         跨帧记忆
┌──────────────────────────────┐                    ┌──────────────────────────┐
│ BatteryStatus               │                    │ PowerState               │
│ - 硬件状态快照               │                    │ - 电源状态机跨帧记忆       │
│ - Platform::get_battery_     │   每帧传入          │ - 上次按键时刻            │
│   status() 返回              │ ──────────────────→│ - 充电状态变化检测        │
│ - 不跨帧缓存                 │   update()          │ - 睡眠/关机触发标志       │
│ - 生命周期：一次函数调用     │                    │ - 生命周期：跨帧持久      │
└──────────────────────────────┘                    └──────────────────────────┘
```

分开的理由：`BatteryStatus` 来自硬件（`Platform::get_battery_status()` 可能读 `/sys/class/power_supply/`），`PowerState` 来自逻辑推导（"上次充电状态是什么？""空闲超时了吗？"）。把它们合并到一个结构体会模糊数据来源——硬件快照和逻辑记忆需要不同的生命周期。

### update() 状态机

每帧调用一次，按优先级处理：

```
update(state, input, battery, mute, mod_keys, sleep_btn, hdmi_active, now_ms)
  → (Option<PowerAction>, dirty, show_setting)

PowerAction = Sleep | PowerOff    // None = 无事发生

1. 充电状态变化检测（每 1000ms 检查一次——`checked_charge_at` + `CHARGE_DELAY` 门控，避免每帧读 sysfs）
   was_charging != battery.charging → dirty = true

2. 用户输入检测
   any_just_pressed() → last_input_at = now_ms

3. 电源键长按（≥ 1000ms）或独立关机键释放
   → 返回 Some(PowerAction::PowerOff)
   → 调用方执行：before_sleep → 渲染关机提示 → platform.power_off()

4. 手动睡眠（sleep_btn 释放，can_sleep 门控）
   → 返回 Some(PowerAction::Sleep)；触发时写回 `last_input_at = now_ms`、`power_pressed_at = 0` 并消费 `requested_sleep`
   → 调用方执行：before_sleep → faux_sleep(...) → after_sleep

5. 自动睡眠（空闲 30s + 未充电 + 未 HDMI + can_autosleep）
   充电/HDMI/can_autosleep=false 时刷新 last_input_at（阻止期间持续重置计时）
   → 返回 Some(PowerAction::Sleep) → 同上

6. 静音切换检测
   mute != was_muted → show_setting = 2（弹出音量条）

7. 修饰键逻辑（250ms MOD_DELAY / 500ms SETTING_DELAY）
   → 按住亮度/音量修饰键弹出对应设置条；超时且修饰键松开后隐藏

8. 设置条显示时强制重绘（keymon 一帧延迟补偿）
```

**`mod_keys` 参数**：平台修饰键映射（`ModKeys { brightness, volume, plus, minus }`——对应原版 `BTN_MOD_*` 宏）。原版是编译期宏（各平台 defines.h 不同值），Rust 由调用方从平台 crate 常量构造传入——与 `mute` 参数同一模式。**`sleep_btn` 参数**：平台睡眠键（`BTN_SLEEP`，tg5040 = `BTN_POWER`，对应 platform.h:111）——原版编译期宏，参数化同模式。**`hdmi_active` 参数**：调用方每帧从 `platform.is_hdmi_active()` 收集（原版内部调 `GetHDMI()`），用于自动睡眠阻止判定。**跨帧面板状态**：原版 `show_setting` 是调用方持有的变量（`_show_setting` 指针），Rust 归入 `PowerState.show_setting`——`update` 从该字段起始判定、返回前写回，返回值是当帧快照（供渲染）。

### faux_sleep 流程

```rust
pub fn faux_sleep(platform: &mut P, state: &PowerState) {
    platform.reset_input();      // 睡前清输入（对应 C 的 PAD_reset #1）
    platform.prepare_sleep();    // 关背光、暂停音频、杀 keymon、sync 文件系统

    loop {                       // 最多 120 秒超时
        std::thread::sleep(Duration::from_millis(200));
        if platform.should_wake() { break; }
        if 充电中 { break; /* 不关机 */ }
    }

    platform.complete_wake();    // 开背光、恢复音频、启动 keymon
    platform.reset_input();      // 醒后清输入（对应 C 的 PAD_reset #2）
}
```

**注**：`std::thread::sleep` 在嵌入式设备上可能不可用——未来平台实现可能需要提供 `delay_ms` 钩子。

**两次 `reset_input` 的意义**：睡眠前清——睡眠瞬间的按键状态不残留；唤醒后清——唤醒键（如 `BTN_POWER` 释放）若未被 `should_wake` 完全消费，不会在下一帧被识别为"再次手动睡眠"（否则会陷入无限睡眠循环）。这正是 C 版 `PWR_fauxSleep` 前后各调一次 `PAD_reset`（api.c:1687-1694）的语义。

### 在主循环中集成

```rust
let mut power_state = PowerState::new();
loop {
    let input = platform.poll_input();
    let battery = platform.get_battery_status();
    let mute = /* 平台 settings 读取（keymon 写入的共享内存），如
                   platform_tg5040::settings::SettingsHandle::init().mute() */;
    let mod_keys = ModKeys { brightness: P::BTN_MOD_BRIGHTNESS, volume: P::BTN_MOD_VOLUME,
                                plus: P::BTN_MOD_PLUS, minus: P::BTN_MOD_MINUS };
    let sleep_btn = P::BTN_SLEEP;
    let hdmi_active = platform.is_hdmi_active();
    let now = platform.now_ms();

    let (action, dirty, show_setting) = power::update(
        &mut power_state, &input, &battery, mute, mod_keys, sleep_btn, hdmi_active, now,
    );

    match action {
        Some(PowerAction::Sleep) => {
            before_sleep();                     // 1. 存档/降频等前置（可能写 auto_resume.txt）
            power::faux_sleep(&mut platform, &mut power_state); // 2. 阻塞睡眠
            after_sleep();                      // 3. 唤醒后清理（可能删 auto_resume.txt）
        }
        Some(PowerAction::PowerOff) => {
            before_sleep();                     // quicksave + auto_resume 标记（不可省略）
            screen.fill_rect(screen_rect, RGB_BLACK);
            // 渲染"正在关机…"
            platform.flip(&screen, false);
            power::power_off(&mut platform);    // 进程在此退出
        }
        None => {}
    }

    // ... 正常渲染循环（dirty / show_setting）...
}
```

**关键架构差异（vs C 原版）**：C 的 `PWR_update` 是**执行者**——它内部直接调用 `PWR_fauxSleep()`（阻塞）和 `PWR_powerOff()`，`before_sleep`/`after_sleep` 回调在 update 内部紧邻执行（api.c:1534-1558）。Rust 版 `update` 是**检测者**——睡眠/关机需要 `&mut Platform`，update 保持纯函数可测，因此**执行权移交调用方**：update 只返回 `Option<PowerAction>` 信号，调用方在同一帧内执行完整动作序列。回调序列（`before_sleep → faux_sleep → after_sleep`）的执行权跟随"谁执行睡眠"，不跟随"谁检测睡眠"——这是本项目与原 C 最核心的电源架构差异。

### 修饰键逻辑（设置条弹出/隐藏）

`update` 的修饰键部分对应原版 `api.c:1571-1591`：

- **`delay_settings = mod_keys.brightness == BTN_MENU`**：原版是编译期常量（`BTN_MOD_BRIGHTNESS==BTN_MENU`——11 个平台恒真、m17 类恒假，各平台编译出不同代码）；Rust 参数化后运行期比较——**行为等价**（每次调用比一次 u32，开销可忽略）。它控制"设置条是否需修饰键松开才隐藏"：亮度修饰键是 MENU 的设备（tg5040），MENU 松开才算调节结束；不是 MENU 的设备（m17），设置条立即隐藏
- **`SETTING_DELAY` 500ms**：设置条显示超时后自动隐藏（需修饰键未按住）
- **`MOD_DELAY` 250ms**：修饰键按住**延迟**才弹设置条——防误触（用户只是路过 MENU 键时不会弹出亮度条）
- **`mod_unpressed_at`**："修饰键最后松开时刻"——逻辑上是"未按住修饰键的最后时刻"（原版注释 "this feels backwards but is correct"）：按住修饰键时不刷新，松开后开始计 MOD_DELAY 延迟——所以从"松开修饰键"到"再次按住弹设置条"需要 250ms，防止按住 MENU 拖动时的抖动误触
- **重复触发分支**：`mod_keys.volume == BTN_NONE || mod_keys.brightness == BTN_NONE` 的设备（tg5040 无音量修饰键）——按住修饰键期间持续按 PLUS/MINUS（重复事件）也会刷新设置条（原版 `!BTN_MOD_VOLUME || !BTN_MOD_BRIGHTNESS` 编译期分支）

### 装配层职责与调用方生命周期契约（update 的输入从哪来、动作往哪去）

`update` 是**纯函数**——它不感知任何平台，输入全靠调用方收集，动作由调用方执行。谁是调用方？**minui/minarch 的主循环**——它们是"装配层"：

- 同时依赖 common（trait 定义处）与平台 crate（trait 实现处）——**平台实现永远不向上依赖，上层负责装配**
- 每帧从平台 + settings 收集数据，喂给 common 的纯函数，消费返回值并执行动作

```
minui/minarch 主循环（每帧）——装配层
│
│  ① 收集（全部从平台拿）
│     platform.poll_input()          → InputState    → update 的 input
│     platform.get_battery_status()  → BatteryStatus → update 的 battery
│     platform.now_ms()              → u32           → update 的 now_ms
│     settings 模块（平台 crate）     → bool          → update 的 mute
│     P::BTN_MOD_*（trait 常量）     → ModKeys       → update 的 mod_keys
│     P::BTN_SLEEP（trait 常量）     → u32           → update 的 sleep_btn
│     platform.is_hdmi_active()      → bool          → update 的 hdmi_active
│     PowerState::new()（启动时创建一次）              → &mut state（跨帧记忆，
│       含 show_setting 面板状态）
│
│  ② 调用 update → 得到 (Option<PowerAction>, dirty, show_setting)
│  ③ 消费返回值：
│     Some(PowerAction::Sleep)   → 同一帧内执行 before_sleep → faux_sleep → after_sleep
│     Some(PowerAction::PowerOff)→ 同一帧内执行 before_sleep → 渲染关机消息 → power_off
│     None                       → 无事发生
│     dirty 决定重绘，show_setting 决定弹哪个设置条（数值渲染时再读一次 settings）
```

**为什么是参数传入而非 C 的直接调用？** C 的 `PWR_update` 内部直接访问全局 `pad`、调用 `GetMute()`/`PLAT_getBatteryStatus()`——Rust 没有跨模块共享全局状态的机制，`update` 想访问什么就得通过参数拿到什么。这是"调用方收集数据传入"模式：`InputState`/`BatteryStatus`/`now_ms` 早如此，`mute`/`mod_keys`/`sleep_btn`/`hdmi_active` 补齐了最后几块平台值。

**为什么动作也由调用方执行？** C 的 `PWR_update` 是执行者（内部直接 `PWR_powerOff`/`PWR_fauxSleep`，api.c:1534-1558），所以 `before_sleep`/`after_sleep` 回调在 update 内部。Rust 版 `update` 保持纯函数（可单测），而睡眠/关机需要 `&mut Platform`——只有装配层持有平台引用。因此**回调序列的执行权跟随"谁执行睡眠"**：`before_sleep` 必须与 `faux_sleep` 紧邻（否则出现"存档了但没睡"的时序 bug），`after_sleep` 只有执行睡眠的一方才知道何时唤醒。C 版的 `after_sleep` 参数在 Rust 版结构中**不可能**由 update 调用——这正是签名重构的原因。

**两类函数的区分**：`update` 是纯函数（签名无 `Platform`）；`faux_sleep`/`power_off` 通过泛型 `<P: Platform>` 访问平台方法（睡眠要调 `prepare_sleep`/`complete_wake`/`reset_input`），由主循环在收到动作信号时调用。

**当前状态**：minui/minarch 仍是占位（无主循环），`update` 的装配代码在后续变更中接入——本 README 的集成示例是未来主循环的蓝图。**给未来调用方的契约提醒**：Sleep 分支的三步（`before_sleep → faux_sleep → after_sleep`）顺序不可调换、不可省略——minarch 的 `before_sleep` 会写 `auto_resume.txt`（quicksave 标记）、`after_sleep` 会删它；PowerOff 分支的 `before_sleep` 不可省略（否则下次开机无恢复数据）。

---

## paths——平台路径派生函数族

`paths` 模块提供对应 C 原版 `defines.h:13-27` 的 SDCARD 家族路径派生函数。全部**纯字符串拼接**——零文件系统操作、零依赖、不感知具体平台（原语值由调用方传入）。

### 为什么是函数而不是宏/常量？

C 用 `#define` 宏在编译期拼接（`#define ROMS_PATH SDCARD_PATH "/Roms"`）。Rust 的 trait 关联常量**不能引用其他关联常量**（E0401，`const ROMS_PATH = Self::SDCARD_PATH;` 是编译错误），所以派生路径用自由函数表达。原语（`SDCARD_PATH`/`PLATFORM`）是平台知识，放在 `Platform` trait 关联常量上；派生是通用逻辑，放在 `common::paths`。

### 常量区块（跨进程协议）与函数表（13 个派生）

**常量区块**（绝对路径或纯值，无拼接——区别于需派生的函数；被 minui 与 minarch **双进程**读写，故放 common）：

| 常量 | 值 | 读写方 |
|------|-----|--------|
| `RESUME_SLOT_PATH` | `/tmp/resume_slot.txt` | minui 写（续玩槽位）、minarch 读+删 |
| `CHANGE_DISC_PATH` | `/tmp/change_disc.txt` | minarch 写（换碟请求）、minui 读+删 |
| `AUTO_RESUME_SLOT` | `9` | minui（auto_resume 写默认槽）、minarch（读缺省回落） |

**派生函数表（13 个）**

| 函数 | 对应 C 宏 | 示例（`"/mnt/SDCARD"`、`"tg5040"`） |
|------|----------|----------------------------------|
| `get_roms_path(sdcard)` | `ROMS_PATH` | `"/mnt/SDCARD/Roms"` |
| `get_system_path(sdcard, platform)` | `SYSTEM_PATH` | `"/mnt/SDCARD/.system/tg5040"` |
| `get_paks_path(sdcard, platform)` | `PAKS_PATH` | `"/mnt/SDCARD/.system/tg5040/paks"` |
| `get_res_path(sdcard)` | `RES_PATH` | `"/mnt/SDCARD/.system/res"` |
| `get_userdata_path(sdcard, platform)` | `USERDATA_PATH` | `"/mnt/SDCARD/.userdata/tg5040"` |
| `get_shared_userdata_path(sdcard)` | `SHARED_USERDATA_PATH` | `"/mnt/SDCARD/.userdata/shared"` |
| `get_recent_path(sdcard)` | `RECENT_PATH` | `"/mnt/SDCARD/.userdata/shared/.minui/recent.txt"` |
| `get_auto_resume_path(sdcard)` | `AUTO_RESUME_PATH` | `"/mnt/SDCARD/.userdata/shared/.minui/auto_resume.txt"` |
| `get_simple_mode_path(sdcard)` | `SIMPLE_MODE_PATH` | `"/mnt/SDCARD/.userdata/shared/enable-simple-mode"` |
| `get_faux_recent_path(sdcard)` | `FAUX_RECENT_PATH` | `"/mnt/SDCARD/Recently Played"` |
| `get_collections_path(sdcard)` | `COLLECTIONS_PATH` | `"/mnt/SDCARD/Collections"` |
| `get_tools_dir(sdcard, platform)` | `Tools/{PLATFORM}`（minui.c:749） | `"/mnt/SDCARD/Tools/tg5040"` |
| `get_version_txt_path(sdcard)` | `ROOT_SYSTEM_PATH` `"version.txt"`（defines.h:14） | `"/mnt/SDCARD/.system/version.txt"` |

### 扁平签名约定

每个函数只接受根原语参数（`sdcard_path`，需要时加 `platform`）——"一切路径都从 `SDCARD_PATH`（和 `PLATFORM`）派生"的单一心智模型。调用方无需先构造中间值再传：

```rust
use common::paths;
use common::platform::Platform;

// 原语来自 trait 常量，派生来自 paths 函数
let roms = paths::get_roms_path(P::SDCARD_PATH);
let paks = paths::get_paks_path(P::SDCARD_PATH, P::PLATFORM);
```

### 形态判定与范围边界

**"常量 vs 派生函数"的判定规则**：绝对路径或纯值（无 `sdcard_path`/`platform` 拼接）→ `pub const`（如上 3 个）；需要拼接 → 派生函数。minui 独有、无跨进程消费方的路径（如 `/tmp/last.txt`）**不进本模块**——"谁使用谁定义"，留在 minui 的 `launch.rs`。

---

## utils——文件与字符串工具

`utils` 模块提供 minui 和 minarch 共用的工具函数。全部基于 Rust 标准库，零第三方依赖。

### 文件 I/O（7 个函数）


| 函数         | 签名                                                          | 对应 C                  |
| ---------- | ----------------------------------------------------------- | --------------------- |
| `exists`   | `fn exists(path: &str) -> bool`                             | `access(path, F_OK)`  |
| `touch`    | `fn touch(path: &str) -> io::Result<()>`                    | `open+close(O_CREAT)` |
| `get_file` | `fn get_file(path: &str) -> Option<String>`                 | `getFile()`→buffer    |
| `put_file` | `fn put_file(path: &str, contents: &str) -> io::Result<()>` | `putFile()`           |
| `get_int`  | `fn get_int(path: &str) -> Option<i32>`                     | `getInt()`            |
| `put_int`  | `fn put_int(path: &str, value: i32) -> io::Result<()>`      | `putInt()`            |
| `remove_file` | `fn remove_file(path: &str) -> io::Result<()>`           | `unlink()`            |

`remove_file` 是 `std::fs::remove_file` 的薄包装——错误透传（NotFound 语义与 std 一致）。**为什么进 common？** minui 的 `launch.rs`（auto_resume 删标记）与 `recents.rs`（消费换碟请求删文件）双模块共用才入 common（"多模块引用才进 common"原则）。


与 C 的关键区别：C 使用栈上 `char[256]` 缓冲区（`getFile` 易缓冲区溢出），Rust 使用 `String`——堆分配 + 编译期保证无溢出。

### 字符串匹配（4 个函数）


| 函数                | 签名                                                         | 对应 C               |
| ----------------- | ---------------------------------------------------------- | ------------------ |
| `exact_match`     | `fn exact_match(a: &str, b: &str) -> bool`                 | `exactMatch()`     |
| `prefix_match`    | `fn prefix_match(pre: &str, s: &str) -> bool`              | `prefixMatch()`    |
| `suffix_match`    | `fn suffix_match(suf: &str, s: &str) -> bool`              | `suffixMatch()`    |
| `contains_string` | `fn contains_string(haystack: &str, needle: &str) -> bool` | `containsString()` |


**大小写语义（对齐 C）**：`prefix_match`/`suffix_match`/`contains_string` 使用 ASCII 大小写不敏感匹配（对齐 C 的 `strncasecmp`/`strcasestr`），`exact_match` 保持大小写敏感（对齐 C 的 `strncmp`）。折叠使用 ASCII 语义（`eq_ignore_ascii_case`/`to_ascii_lowercase`），不用 Unicode 折叠（`to_lowercase`）——与 C 逐字节行为一致。`hide` 的 `.disabled` 后缀判断统一复用 `suffix_match`。

### 路径处理（5 个函数）

全部严格对照 C `utils.c` 实现。路径相关的 3 个函数已**参数化**——不再引用模块内路径常量，参数由调用方从 `Platform::SDCARD_PATH`/`Platform::PLATFORM` 与 `common::paths` 派生函数传入：


| 函数                  | 签名                                                                                 | 用途            | C 代码行             |
| ------------------- | ---------------------------------------------------------------------------------- | ------------- | ----------------- |
| `hide`              | `fn hide(filename: &str) -> bool`                                                   | 文件隐藏判断        | `utils.c:32-33`   |
| `get_display_name`  | `fn get_display_name(path: &str, platform: &str) -> String`                         | 六步管线处理显示名     | `utils.c:36-73`   |
| `get_emu_name`      | `fn get_emu_name(path: &str, roms_path: &str) -> String`                            | 从路径提取模拟器名     | `utils.c:74-102`  |
| `get_emu_path`      | `fn get_emu_path(emu_name: &str, sdcard_path: &str, platform: &str, paks_path: &str) -> String` | 构造模拟器 .pak 路径 | `utils.c:103-107` |
| `trim_sorting_meta` | `fn trim_sorting_meta(name: &mut String)`                                           | 剥离排序前缀        | `utils.c:123-137` |


'`get_emu_path` 的行为边界：优先候选 `{sdcard}/Emus/{platform}/{emu}.pak/launch.sh` 存在性检查 → 回退 `{paks}/Emus/{emu}.pak/launch.sh`——回退也不保证存在（与 C `getEmuPath` 一致，`exists` 判断由调用方做）。`hide` 三条件：`.` 开头 / `.disabled` 后缀（大小写不敏感）/ 精确等于 `map.txt`（大小写敏感）。`trim_sorting_meta`：数字前缀后非 `)` 则不动（如 "FF7" 不去除）。

`get_display_name` 的六步管线：

```
1. 若以 "/tg5040" 结尾（大小写不敏感）→ 剥离平台段（隐藏 Tools 路径中的平台目录）
2. 提取最后一个 '/' 之后的文件名
3. 循环去末尾 1-4 字符扩展名（.gb → .p8 → 直到不满足）
4. 循环去末尾括号标记 '(' / '['（Game (USA) (GB) → Game (USA) → Game）
5. 若结果为空 → 回退到步骤 4 之前的状态
6. 去除末尾空白
```

示例（平台代码 `"tg5040"`）：

```rust
use common::paths;

let sdcard = "/mnt/SDCARD";
let roms = paths::get_roms_path(sdcard);   // "/mnt/SDCARD/Roms"
let paks = paths::get_paks_path(sdcard, "tg5040"); // "/mnt/SDCARD/.system/tg5040/paks"

// 路径解析函数纯字符串处理，参数显式传入
get_display_name("/mnt/SDCARD/Tools/tg5040/sometool.pak", "tg5040"); // "sometool"
get_emu_name("/mnt/SDCARD/Roms/Game Boy (GB)/Pokemon Red.gb", &roms); // "GB"
get_emu_path("GB", sdcard, "tg5040", &paks);
```

---

## 跨模块设计决策

### 类型归属规则

类型的所在模块由其**语义域**决定——不是随意放置：


| 类型                | 所在模块    | 语义域    | 为什么不在其他模块         |
| ----------------- | ------- | ------ | ----------------- |
| `VsyncMode`       | `video` | 视觉呈现概念 | 与视频刷新相关，非硬件能力     |
| `CpuSpeed`        | `power` | 电源管理概念 | CPU 频率调节是电源策略的一部分 |
| `BatteryStatus`   | `power` | 硬件快照   | 虽然来自平台，但由电源状态机消费  |
| `AudioFrame`      | `audio` | 音频数据类型 | 自然归属              |
| `FRAME_BUDGET_MS` | `video` | 帧计时概念  | 属于显示管线的计时约束       |


**原则**："类型定义在语义域所在的模块，实例由调用方持有。"

### 布局常量单一来源

所有 UI 布局常量（`PILL_SIZE`、`BUTTON_SIZE` 等 11 个）定义在 `common::video`。业务 crate 通过 `use common::video::XXX` 引用。C 的 `#define` 通过 `#include` 共享——Rust 的常量通过模块系统的 `use` 共享。禁止在业务 crate 中重复定义相同的常量。

### 路径体系（paths 模块 + trait 常量）

路径知识的归属分两层（对应 C 原版的 `platform.h` + `defines.h`）：

- **原语**：`Platform::SDCARD_PATH`（各平台不同，C 13 平台各不相同）与 `Platform::PLATFORM`（平台代码）——编译期关联常量，由各平台实现提供
- **派生**：`common::paths` 的 13 个纯字符串派生函数（对应 C `defines.h:13-27` 宏族）——零 I/O 零依赖，只做 `format!` 拼接

```
C 原版:  defines.h  #define ROMS_PATH SDCARD_PATH "/Roms"   （宏拼接）
Rust 版: paths::get_roms_path(P::SDCARD_PATH)              （函数拼接）
```

utils 的路径解析函数（`get_display_name`/`get_emu_name`/`get_emu_path`）全部参数化——不引用任何路径常量，参数由调用方从 trait 常量与 paths 派生函数传入。

---

## C → Rust 迁移对照表


| C 定义                                                         | 位置                | Rust 定义                                                          | 位置                 |
| ------------------------------------------------------------ | ----------------- | ---------------------------------------------------------------- | ------------------ |
| `PAD_Context` + `PAD_*` 宏                                    | `api.h` / `api.c` | `InputState` + 查询方法                                              | `input.rs`         |
| `PAD_justPressed(MENU)` 等                                    | `api.c`           | `tapped_menu()`                                                  | `input.rs`         |
| `BTN_UP/DOWN/...` `#define`                                  | `api.h`           | `BTN_*` `const u32`                                              | `input.rs`         |
| `SND_Frame { left, right }`                                  | `api.h:203`       | `AudioFrame`                                                     | `audio.rs:54`      |
| `SND_Context` 环形缓冲                                           | `api.c:928`       | `AudioRingBuffer`                                                | `audio.rs:84`      |
| `SND_resampleNone` / `SND_resampleNear`                      | `api.c:1000`      | `Resampler`                                                      | `audio.rs:208`     |
| `PWR_Context` 全局变量                                           | `api.c:72`        | `PowerState`                                                     | `power.rs:77`      |
| `PWR_update`                                                 | `api.c:115`       | `update()`                                                       | `power.rs`         |
| `PWR_fauxSleep`                                              | `api.c:221`       | `faux_sleep()`                                                   | `power.rs`         |
| `PWR_powerOff`                                               | `api.c:213`       | `power_off()`                                                    | `power.rs`         |
| `ASSET_*` 枚举                                                 | `api.h`           | `Asset` 枚举                                                       | `video.rs`         |
| `asset_rects[]`                                              | `api.c`           | `ASSET_RECTS`                                                    | `video.rs`         |
| `asset_rgbs[]`                                               | `api.c`           | `ASSET_RGBS`                                                     | `video.rs`         |
| UI 布局常量 (`PILL_SIZE`等)                                       | `defines.h`       | 布局常量                                                             | `video.rs:112-127` |
| Vsync 常量 (`VSYNC_OFF` 等)                                     | `api.h`           | `VsyncMode` 枚举                                                   | `video.rs`         |
| `exists`, `touch`, `getFile`, `putFile`                      | `utils.c`         | `exists`, `touch`, `get_file`, `put_file`                        | `utils.rs`         |
| `getInt`, `putInt`                                           | `utils.c`         | `get_int`, `put_int`                                             | `utils.rs`         |
| `exactMatch`, `prefixMatch`, `suffixMatch`, `containsString` | `utils.c`         | `exact_match`, `prefix_match`, `suffix_match`, `contains_string` | `utils.rs`         |
| `hide`                                                       | `utils.c:32`      | `hide`                                                           | `utils.rs:117`     |
| `getDisplayName`                                             | `utils.c:36`      | `get_display_name`                                               | `utils.rs:139`     |
| `getEmuName`                                                 | `utils.c:74`      | `get_emu_name`                                                   | `utils.rs:205`     |
| `getEmuPath`                                                 | `utils.c:103`     | `get_emu_path`                                                   | `utils.rs:240`     |
| `trimSortingMeta`                                            | `utils.c:123`     | `trim_sorting_meta`                                              | `utils.rs:256`     |


---

## 测试策略

common crate 有 **184 个单元测试**（+1 忽略——平台 mock 断言组），分布在各个模块的 `#[cfg(test)] mod tests` 中：


| 模块      | 测试数 | 类型                          | 需要特殊环境？ |
| ------- | --- | --------------------------- | ------- |
| `video` | 39  | 枚举顺序、坐标正确性、fill_rect 行为、常量值 | 否       |
| `input` | 21  | 位掩码查询、`tapped_menu` 时序      | 否       |
| `audio` | 18  | 环形缓冲边界、重采样器数学               | 否       |
| `power` | 21  | 状态机转换、充电检测、睡眠触发             | 否       |
| `utils` | 22  | 文件隐藏、显示名处理、模拟器名提取、路径构造、排序前缀 | 否       |


**全部测试可在无 GPU、无 SDL、无字体文件的 CI 环境运行**——只需 `cargo test -p common`。下表数字为早期快照，以实测为准。这是零依赖设计的最直接受益：类型和纯逻辑的测试不需要任何硬件或系统依赖。

测试遵循项目 TDD 规范——先写测试定义预期行为，再实现函数使测试通过，最后重构并确保测试仍通过。

---

## 否决方案清单

以下是在 common 开发过程中**被否决的方案**及否决理由——遇到相似想法时先查此清单（防止 AI 后续生成作出错误决策）。注意：**否决不是永久决定**——若需求变化，被否决的方案可以复活（见下方 BTN_ID_ANALOG_* 案例）。

### 类型设计（implement-common-types）

| 方案 | 否决理由 |
|------|---------|
| 全部自由函数（`just_pressed(state, btn)`） | `state.just_pressed(btn)` 在可发现性、链式调用、IDE 自动补全方面都优于自由函数。项目在 Rust 中重写，就应该用 Rust 的做法 |
| `MenuTapState` 字段放入 `InputState` | `InputState` 是每帧的输入快照，不应包含跨帧的 UI 状态 |
| `tapped_menu` 用 `&mut self` 内部修改字段 | `InputState` 的四个字段由 `poll_input()` 每帧填充——混入 UI 状态破坏数据流单向性 |
| ~~不引入 `BTN_ID_ANALOG_*`~~（**已复活**） | 07-29 否决（当时无摇杆需求）→ **08-08 因 tg5040 摇杆需求复活**：`BTN_ID_ANALOG_UP/DOWN/LEFT/RIGHT`（编号 21-24）由 implement-tg5040-input 变更加入。**案例教训：否决记录的是"当时的需求下不做"，需求变化后应重新评估而非机械遵守** |

### 架构与依赖

| 方案 | 否决理由 |
|------|---------|
| 运行时 `dyn Platform` 选择平台（scaffold-workspace 决策 1） | 嵌入式设备不需要运行时多态——静态分发零开销且更简单 |
| 纯像素操作（blit_pill 等）放 common，只有依赖字体/图片的放 render（scaffold-workspace 决策 2） | 渲染就是渲染——按功能归类比按依赖归类更直观 |
| `AudioRingBuffer` 用 `VecDeque`（implement-common-audio 决策 2） | `VecDeque` 是动态大小双端队列——音频缓冲需要固定容量（避免运行时分配）、连续存储（批量操作） |
| `AudioRingBuffer` 内部加锁（`Mutex` 作字段） | 调用方可能在不同同步上下文使用缓冲（有的需 Mutex、有的无锁）——强制内部锁剥夺灵活性 |
| `AudioRingBuffer` 用 `crossbeam` channel | 引入第三方依赖（违反零依赖原则）；channel 是 MPSC/SPMC 通信原语，对音频缓冲过重 |
| `AudioRingBuffer` 用 unsafe + 无锁 SPSC | Rust 新手需理解 unsafe 和内存序；无性能基准表明需要无锁（SDL 锁竞争极低） |
| utils 路径常量作为函数参数传入（implement-common-utils 决策 1） | 这些路径在 C 中本就是编译期常量——参数化增加调用负担且改变签名，违反"非必要则保持一致" |
| `pick_sample_rate` 不给默认实现（define-platform-trait 决策 5） | 11/12 的平台实现完全一致——默认实现消除重复，少数例外（miyoomini）可覆盖 |

### 音频（implement-common-audio）

| 方案 | 否决理由 |
|------|---------|
| `AudioFrame` 用 `[i16; 2]` 或 `(i16, i16)` | 命名字段 `.left`/`.right` 更可读，且与 C 代码对应关系清晰 |
| 重采样用线性插值 | 原 C 只有最近邻——MinUI 是游戏启动器，最近邻对游戏音频足够，增加复杂度无收益 |

---

## 常见问题

### 为什么 Platform trait 方法不返回 Result？

嵌入式掌机上，初始化失败没有"优雅降级"——视频初始化失败意味着没有画面、音频初始化失败意味着没有声音。原 C 代码的策略是直接放弃（让看门狗重启设备）。Rust 继承了这个策略：用 `panic!` 而非 `Result`，匹配 C 的错误处理哲学。

### 为什么 AudioRingBuffer 内部不加锁？

锁是同步策略——不同的使用场景需要不同的策略。单线程场景（测试）不需要锁。多线程场景（SDL 回调）需要锁。把锁的选择权留给调用方（平台实现者）——她们可以根据自己的线程模型选择 `Mutex<AudioRingBuffer>`、`RwLock<AudioRingBuffer>` 或直接裸用（单线程）。

### 为什么不用 `dyn Platform`？

`dyn trait` 通过虚表（vtable）做运行时分发——每次调用多一次指针间接跳转。在 400MHz ARM 上，主循环中每帧可能有数十次 trait 方法调用（poll_input、flip、now_ms、get_battery_status……）——累积的虚表开销显著。泛型单态化在编译期展开——所有调用都是直接跳转，机器码与手写函数调用完全一致。

### pitch &gt; width 的硬件有哪些？

某些 ARM Mali GPU 要求纹理行按 8 像素对齐。在这种硬件上，640 像素宽的帧缓冲需要 `pitch = 640 → 向上取整到 8 的倍数 = 640`（已经对齐）。320 像素宽 → `pitch = 320 → 320`（也对齐）。最可能触发 pitch &gt; width 的场景是奇数值的缩放结果——如 272×480 的屏幕在 2x 缩放到 544×960 时，544 / 8 = 68 精确对齐所以没问题。当前 tg5040 平台 pitch == width 始终成立。

### 为什么 ASSET_RECTS 存储未缩放坐标？

C 版本在编译时用 `SCALE4()` 宏预乘 scale 值存储到 `asset_rects[]`——每个平台的图集坐标表都不同（scale 不同）。Rust 存储原始 100×64 画布上的坐标，由 `blit_asset` 在运行时 × scale。这意味着同一个坐标表可以用于不同 scale 的平台——源数据统一，缩放由渲染层负责。

---

---

## 设计决策记录（后期扩展条目）

「跨模块设计决策」「否决方案清单」章节记录了早期定稿决策；以下条目补录 trait 面扩展期（布局/能力/语义键）与契约重构的决策。

### 决策：平台差异布局值用 trait 方法（main_row_count/padding）

- **结论**：行数/留白差异经 `Platform` trait 方法 `main_row_count()`/`padding()` 覆盖表达（tg5040 smart 8/40、brick 7/5），默认返回 `video::MAIN_ROW_COUNT`（6）/`PADDING`（10）。
- **为什么**：my355/rg35xxplus 等平台的 `on_hdmi` 是**运行时状态**（HDMI 插拔切 6↔8 行，platform.c:447）——编译期关联常量无法表达，只能方法；统一用方法避免"常量为主、方法为辅"两套形态。布局常量（`PILL_SIZE` 等跨平台共享值）仍在 `common::video` 单一来源——方法表达平台差异、常量表达共享值，两层分工。
- **被否决**：平台 crate 定义本地常量（违反布局常量单一来源）；全部值入关联常量（HDMI 场景无法表达）。
- **产生的问题**：平台实现须同时填常量与方法（12.9 移植清单项）。

### 决策：按键能力常量（HAS_*）与语义键（BTN_SLEEP/BTN_MOD_*）入 trait

- **结论**：8 个 `HAS_L2/R2/L3/R3/LS/RS/VOLUME/MENU`（默认 false）+ 5 个语义键均为 trait 关联常量。
- **为什么**：能力常量消费方是 minput（跨 crate 面板渲染），语义键是"前端与 keymon 协调常量"——跨 minui/minarch/平台共享 → trait 单一来源；默认 false 保守语义（平台未声明即无此键）。
- **被否决**：常量留在平台 crate、装配层 cfg 引用（早期形态——上层引用平台私有模块需白名单机制，平台自治后白名单删除）；`BTN_RESUME` 入 trait（minui 单消费者功能键，本地常量即可，YAGNI）；`mod_keys()` 平台组装函数（组装是装配层职责，trait 只提供常量）。
- **产生的问题**：`ModKeys` 组装点从平台 crate 移到装配层（minui/minarch main.rs）；早期文档"从平台 crate input 模块取常量"的表述随之废弃（tg5040 input 模块已全 pub(crate)）。

### 决策：set_date_time 默认空实现 = 覆盖点而非"无 RTC"

- **结论**：`set_date_time` 默认空操作是"依赖系统命令、属平台实现细节"的**覆盖点**——原版 C 全部 13 平台都经 common 层 `date + hwclock` 命令设时（api.c:1717-1722），**没有平台不支持**；语义与 `set_rumble`（真无震动硬件）不同。
- **为什么**：设时经系统命令与硬件无关——放 trait 是为让平台实现选择命令形态（tg5040 用 `std::process::Command` 免 shell 转义）；曾误述为"不是所有设备都有 RTC"，已订正。
- **被否决**：删除默认实现（每个平台都要写）；默认实现内直接调 `date` 命令（common 不执行系统命令——平台细节）。
- **产生的问题**：早期文档错误表述被多处引用，迁移期统一订正。

### 决策：PowerState 契约重构（检测者 vs 执行者 + 控制标志）

- **结论**：`update` 10 参数返回 `(Option<PowerAction>, bool, u8)`，只检测不执行；`PowerState` 含 `can_sleep/can_poweroff/can_autosleep` 控制标志与 `requested_*` 预留钩子（对应 C `PWR_Context` 去除 battery_pt 等 4 字段——电池监控从 pthread 改帧内 1000ms 轮询）。
- **为什么**：C 的 update 是执行者（内部直接摸全局、执行睡眠）；Rust 的 update 保持纯函数可测，睡眠需 `&mut Platform` 只有装配层持有——执行权移交调用方；充电检测每 1000ms 一次避免每帧读 sysfs（pthread 移除后的帧内轮询节奏）。
- **被否决**：保留 C 的执行者结构（update 内调 faux_sleep——需要平台引用，破坏纯函数性）；充电状态每帧检查（sysfs 读开销）。
- **产生的问题**：装配层须为每个 PowerAction 提供完整动作序列（顺序不可调换，README 集成示例与 minarch/tg5040 文档均记录）；睡眠分支触发后须写回 `last_input_at`/`power_pressed_at` 并消费 `requested_sleep`（细节在 update 状态机注释）。
