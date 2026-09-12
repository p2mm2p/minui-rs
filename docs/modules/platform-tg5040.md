# platform-tg5040 — TrimUI Smart Pro / Brick 平台实现

`platform-tg5040` 是 MinUI 在 tg5040 芯片平台上的硬件适配层。它实现了 `common::platform::Platform` trait，内部使用 SDL2 进行视频渲染、输入轮询和音频播放，用 libc 直读 evdev 与 sysfs 完成系统级按键和电源管理。

**目标设备**：TrimUI Smart Pro 和 TrimUI Brick。两台设备共享同一芯片平台（tg5040），差异（分辨率、布局、输入映射、LED 路径等）**在编译期**通过 Cargo feature（`smart` 默认 / `brick`）区分——不拆分为两个平台 crate，也不做任何运行时设备判断。

**当前状态**：✅ **平台已全部实现**（`src/` 与 `src/bin/` 共 6 个源文件 3,506 行，全部方法已实现）。剩余未验证项均为"真机验证"性质（开发机上无真实硬件），统一记录在各子系统章节的「真机验证清单」中。

---

## 内容地图

| # | 章节 | 回答的问题 |
|---|------|-----------|
| 1 | [为什么需要平台 crate](#1-为什么需要平台-crate) | 为什么硬件抽象要独立成一个 crate？ |
| 2 | [目标设备与硬件参数](#2-目标设备与硬件参数) | 两台设备长什么样？SCALE 到底是什么？ |
| 3 | [依赖关系](#3-依赖关系) | 本 crate 依赖谁？为什么用 libc 不用 evdev crate？ |
| 4 | [核心概念](#4-核心概念) | 阅读后面所有章节前需要建立的心智模型 |
| 5 | [模块总览](#5-模块总览) | 源码文件布局 + Tg5040 结构体 |
| 6 | [系统架构与启动流程](#6-系统架构与启动流程) | 从硬件上电到 minui.elf 运行的完整链路 |
| 7 | [video — 视频](#7-video--视频) | SDL 渲染三方法（init/flip/quit）怎么实现 |
| 8 | [input — 输入](#8-input--输入) | 应用级按键：SDL joystick → InputState |
| 9 | [audio — 音频](#9-audio--音频) | SDL 回调双线程架构与缓冲策略 |
| 10 | [power — 电源与硬件](#10-power--电源与硬件) | sysfs 读写、睡眠唤醒、关机链 |
| 11 | [settings 与 keymon — 系统设置](#11-settings-与-keymon--系统设置) | 系统级按键守护进程与共享内存设置 |
| 12 | [设备区分机制：运行时 vs 编译期](#12-设备区分机制运行时-vs-编译期) | 本平台最核心的架构决策（≥500 行） |
| 13 | [打包与安装](#13-打包与安装) | boot.sh / 平台子 xtask / 安装图去向 |
| 14 | [与原 C tg5040 实现的对比](#14-与原-c-tg5040-实现的对比) | 全维度对照表 |
| 15 | [测试策略](#15-测试策略) | 三层方案与可测性设计 |
| 16 | [否决方案清单](#16-否决方案清单) | 所有被否决的方案（防止 AI 误引入） |
| 17 | [常见问题 FAQ](#17-常见问题-faq) | 新手高频疑问 |
| 18 | [公开 API 说明](#18-公开-api-说明) | 对外暴露的类型 |

---

## 1. 为什么需要平台 crate

### 1.1 "USB 规范"与"USB 设备"

`common` crate 里的 `Platform` trait 是硬件抽象的**接口契约**——它定义了"一台 MinUI 设备必须能做什么"：初始化视频、轮询输入、播放音频、管理电源。trait 本身不关心具体硬件，它是"USB 规范"。

本 crate 是**第一个具体的"USB 设备"**——把 tg5040 这台真实机器的硬件差异（屏幕分辨率、按键布局、sysfs 路径、音频设备）翻译成 trait 的标准接口。上层代码（minui、minarch）只看到 `Platform` trait，**完全不知道 SDL2、evdev、sysfs 的存在**：

```
                 ┌─────────────────────┐
                 │   common::Platform  │  ← trait 定义（零依赖，纯 Rust）
                 │   27 方法 + 24 常量 │
                 └──────────┬──────────┘
                            │ impl Platform for Tg5040
                 ┌──────────▼──────────┐
                 │   platform-tg5040   │  ← 本 crate（"USB 设备"）
                 │   SDL2 窗口/纹理    │
                 │   SDL2 音频回调     │
                 │   SDL2 事件轮询     │
                 │   libc: evdev/sysfs │
                 └─────────────────────┘
```

### 1.2 为什么 SDL 必须锁在这个 crate 里

**设计约束**：common crate 不应感知 SDL 的存在——搜索 common 源码中是否包含 "sdl" 字样，必须返回零结果。

为什么？对比原版 C：`api.c` 约 1,700 行直接依赖 SDL、SDL_ttf、SDL_image，任何改动都可能触发级联编译。Rust 版把依赖层级严格分离——**SDL 的使用完全封装在 `Tg5040` 结构体内部，不通过 trait 暴露任何 SDL 类型**。common 的改动不会触发平台的重新编译，反之亦然。

### 1.3 为什么 smart/brick 共用一个 crate

两台设备共享同一芯片平台（tg5040），差异只是**外围参数**（分辨率、按键码、LED 路径）。若拆成两个 crate，trait 实现会复制 90% 的代码——这是 C 原版用 `is_brick` 变量在一个 `platform.c` 里处理两台设备的动机，Rust 版用**编译期 feature** 达到同样效果（详见第 12 章「设备区分机制」）。

### 1.4 什么时候需要一个新平台 crate

给未来平台实现者的指南：硬件走一遍 `Platform` trait 的 27 个方法——视频用什么 API（SDL？framebuffer？）、输入走什么通道（joystick？evdev？）、音频怎么播放——每个平台把这些答案封装在自己的 crate 里，对外只暴露"实现了 `Platform` trait 的结构体"。tg5040 是全项目的**第一个样板**，后续 11 个平台照此模式移植。

---

## 2. 目标设备与硬件参数

**画布 = 物理分辨率**——`SCREEN_WIDTH/HEIGHT` 等于物理屏分辨率，flip 恒为 1:1 无缩放。按设备编译期确定（`smart` 默认 / `brick`）：

### 2.1 smart（TrimUI Smart Pro，物理屏 1280×720）

| 参数 | 值 | 说明 |
|------|-----|------|
| `SCREEN_WIDTH` | 1280 | 画布宽度 = 物理屏宽度 |
| `SCREEN_HEIGHT` | 720 | 画布高度 = 物理屏高度（16:9） |
| `SCALE` | 2 | 图集倍率 + 布局倍率（使用 `assets@2x.png` 图集） |

### 2.2 brick（TrimUI Brick，物理屏 1024×768）

| 参数 | 值 | 说明 |
|------|-----|------|
| `SCREEN_WIDTH` | 1024 | 画布宽度 = 物理屏宽度 |
| `SCREEN_HEIGHT` | 768 | 画布高度 = 物理屏高度（4:3） |
| `SCALE` | 3 | 图集倍率 + 布局倍率（使用 `assets@3x.png` 图集） |

### 2.3 两台设备共用

| 参数 | 值 | 说明 |
|------|-----|------|
| `BYTES_PER_PIXEL` | 2 | RGB565 格式，每像素 2 字节 |
| `HAS_HDMI` | true | 支持 HDMI 输出（`is_hdmi_active` 恒 false——无输入检测） |
| `HAS_POWER_BUTTON` | true | 有物理电源键（影响关机提示文案） |
| `HAS_POWEROFF_BUTTON` | false | 无独立关机键 |
| `SUPPORTS_OVERSCAN` | false | 不支持过扫描区域 |

以上常量在 `src/lib.rs` 的 `impl Platform for Tg5040` 中定义，用 `#[cfg(feature = "brick")]` / `#[cfg(feature = "smart")]` 区分。

**设备按键能力常量**（`HAS_L2/R2/L3/R3/LS/RS/VOLUME/MENU`，8 个）：声明平台有哪些按键/轴，供 minput 面板渲染等消费。trait 默认 `false`（平台未声明即无此键——保守语义），tg5040 的取值（lib.rs 常量区）：

| 常量 | smart | brick | 依据 |
|------|-------|-------|------|
| `HAS_L2` / `HAS_R2` | true | true | 肩键 AXIS_L2/R2（ABSZ/RABSZ 触发键） |
| `HAS_L3` / `HAS_R3` | **false** | **true** | 摇杆可点击——brick 有 L3/R3（JOY 9/10），smart 无 |
| `HAS_LS` / `HAS_RS` | true | true | 双摇杆（AXIS_LX=0/RX=3） |
| `HAS_VOLUME` | true | true | 音量键（CODE_PLUS=128/JOY_PLUS） |
| `HAS_MENU` | true | true | MENU 键（JOY_MENU=8） |

**能力声明与事件翻译是两层职责**：`HAS_*` 只回答"平台有没有这个键"（供上层 UI 决定渲染/功能），`poll_input` 的事件翻译（第 8.2 节映射表）回答"事件来了映射成什么"——能力常量不参与事件翻译。

### 2.4 SCALE 语义解读（重要）

**SCALE = 图集倍率 + 布局倍率，与画布→屏幕缩放无关。**

```
SCALE 的两个用途（render crate 内部）:
  ① 图集裁切:  blit_asset 从图集取像素 = ASSET_RECTS[30×30] × SCALE
               SCALE=2 → 从 assets@2x.png（200×128）裁切 60×60 像素
  ② 布局倍率:  上层布局元素尺寸 = 布局值(如 PILL_SIZE=30) × SCALE
               SCALE=2 → pill 在画布上 60×60 像素

SCALE 不是: "逻辑画布放大到物理屏的倍率"——flip 恒为 1:1（画布 = 物理屏），
            那个倍率永远是 1。
```

**常见误解**：SCALE=2 容易让人以为是"640×360 画布放大 2 倍到 1280×720"——**这是错误的**。画布就是 1280×720（物理分辨率），SCALE=2 只决定"UI 元素多大"（30→60px）和"用哪倍图集"（@2x）。这也解释了为什么**不能**把 SCREEN 设为物理尺寸后把 SCALE 改成 1——那样元素会变成 30px（只有设计的一半大），且图集裁切从 @2x 变成 @1x。

### 2.5 物理 vs 逻辑画布——设计讨论与解疑

**C 原版为何是物理画布**：`gfx.screen` 就是 1280×720 的 SDL surface（= 物理分辨率），`SCALE2()/SCALE4()` 宏在**绘制时**乘坐标（`SCALE2(1,1,30,30)` → 60×60 画布像素）。C 从来没有"逻辑画布"概念——坐标缩放发生在绘制时，画布一直是物理尺寸。

**为何曾想引入逻辑画布**：`implement-tg5040-structure` 阶段曾设想"上层画 640×360 逻辑画布，flip GPU 放大 2x 到物理屏"——动机是省 CPU（画布缩小到 1/4，渲染更快）。

**逻辑画布方案为何被推翻**——数学不自洽：

```
物理屏 UI 元素尺寸 = 布局值(30) × SCALE × flip 缩放

逻辑画布方案: 30 × 2（SCALE）× 2（flip GPU 放大）= 120px
            → 是 C 原版（30×2×1 = 60px）的 2 倍大！UI 元素大一倍
且 8 行 × 60px = 480 > 360 画布高度——主界面列表装不下
```

备选"逻辑画布 + SCALE=1 + @1x 图集 + flip 2x"（元素 30×1×2=60px 正确、CPU 省 4 倍）——但 @1x 图集被 GPU 放大 2x 后**像素边缘变粗**，画面精美度不可接受（明确决策：画面精美不可牺牲）。

**最终方案**：画布 = 物理分辨率（smart 1280×720/SCALE=2、brick 1024×768/SCALE=3），flip 1:1，@2x/@3x 图集 1:1 呈现——与 C 原版完全一致，画面清晰。代价是 CPU 渲染 921K 像素/帧（性能风险与缓解见下）。

**常见解疑**：

1. **"物理尺寸 = SCREEN × SCALE 吗？"**——逻辑画布方案下 smart 成立（640×2=1280）但 brick 不成立（341×3=1023 ≠ 1024，341 是 1024/3 向下取整）。这正是逻辑画布方案的缺陷之一（取整误差）。画布=物理后此问题彻底消失（SCREEN 直接等于物理屏，无推导）。

2. **"为什么不能 SCREEN=物理尺寸 + SCALE=1？"**——SCALE=1 时布局元素 = PILL_SIZE×1 = 30px（只有设计的一半大），且图集裁切用 @1x（元素更粗）。SCALE 必须 = 2/3 才能保持 UI 元素物理尺寸与 C 一致。

3. **"纹理为什么要跟画布同尺寸？"**——`SDL_UpdateTexture` **不做缩放**：上传的数据尺寸必须等于纹理尺寸。画布（VideoBuffer）是上层画的（1280×720），纹理只能 = 画布尺寸，呈现时 RenderCopy 1:1。这就是"画布=物理 → 纹理=物理、flip 1:1"的推导链。

### 2.6 性能风险与缓解（透明记录）

画布=物理的代价：CPU 渲染 1280×720 = 921K 像素/帧（逻辑画布的 4 倍）。开销分解：

```
① fill_rect 清屏（全屏单色）: 1.8MB 连续内存写 → slice::fill（memset 级）~2ms ✅
② blit 元素（pill/图标）:     memcpy 级（copy_from_slice）→ <1ms ✅
③ 文字渲染（fontdue 混合）:   无缓存时最大变量（可能 5-10ms）
乐观合计 3-5ms / 悲观合计 10-15ms（17ms 帧预算内）
```

**缓解只采用第 1 层（编译器级）**——已就位：`fill_rect` 用 `slice::fill`（memset 级）、blit 用 `copy_from_slice`、release 编译 + LLVM 向量化。真机验证用 `now_ms()` 量化 flip 前后耗时（见第 7 章「真机验证清单」）。

**被否决的缓解方案**（防止误引入）：第 2 层 GlyphCache 字形缓存（改变 render 架构）、第 3 层脏矩形增量重绘（改变上层主循环架构）——均因改变架构被否决（详见第 16 章否决方案清单）。若实测文字渲染拖后腿，记录为已知限制，不在本架构内解决。

## 3. 依赖关系

### 3.1 依赖方向

```
common（零依赖，Platform trait 定义处）
  ↑
  ├── platform-tg5040（平台 lib：sdl2 + sdl2-sys + libc）
  │     ↑
  │     ├── show（平台自治 bin：+ common、png——自实现 PNG 解码）
  │     └── keymon（平台自治 bin：+ libc；不直接依赖 common）
  │
  └── minui / minarch / clock / minput（装配层）
        依赖 common + render + platform-tg5040（可选，经 feature 透传）
        额外第三方：minarch = libloading + flate2；clock = libc
```

依赖方向严格遵守单向：**平台 crate 依赖 common，永远不向上依赖**（render、minui、minarch 不出现在本 crate 的依赖链中——`cargo tree -p platform-tg5040 --invert` 可验证）。render 的渲染函数由上层调用，平台只提供画布（`VideoBuffer`）给上层画。

### 3.2 各依赖的职责

依赖各司其职：

| 依赖 | 用途 | 对应 C 原版 |
|------|------|------------|
| `common` | `Platform` trait + 基础类型（`InputState`/`VideoBuffer`/`AudioRingBuffer`） | `platform.h` 头文件 |
| `sdl2` | 视频（window/canvas/texture）、音频（回调设备）、输入（joystick 事件） | SDL2 链接 |
| `libc` | `shm_open`/`mmap`（settings 共享内存）、`ioctl`（亮度） | 标准 C 库调用 |
| `png`（show） | show 的 `load_png` 自实现 PNG 解码（纯 Rust，无 C 依赖） | SDL_image（`IMG_Load`） |

**`unsafe_textures` feature**：sdl2 crate 的 `Texture` 默认带生命周期参数（`Texture<'r>`），直接作为结构体字段会有生命周期纠缠——启用该 feature 去掉生命周期，使纹理可安全存储在 `Tg5040` 结构体中。

**`png` 依赖说明**：show 是平台自治 bin（独立 crate `platform-tg5040-show`）——不复用 render，自实现图片解码，因此 show crate 依赖 `png`。这**不违反**"平台不依赖 render"的依赖方向约束（render 是上层 crate，png 是纯第三方解码库；`cargo tree -p platform-tg5040` 中 render 仍不出现）。C 原版各平台 show.c 同样自治用 SDL_image 加载 PNG。

**声明位置**：本节全部依赖（内部 path 与第三方）集中声明于根 `[workspace.dependencies]`，本 crate 与 show/keymon 的清单只写 `workspace = true`——版本、features、path 单一来源（见 architecture.md §5.1 与 §6 决策记录）。依赖 key 恒为包名 `platform-tg5040`（**不使用 rename**）：上层写 `platform-tg5040 = { workspace = true, optional = true }` 并以 `--features platform-tg5040/<device>` 透传；show/keymon 写 `platform-tg5040 = { workspace = true }`、设备 feature 转发写作 `smart = ["platform-tg5040/smart"]`——全项目同一形态。

### 3.3 平台自治结构：纯 lib + show/keymon 独立 crate

平台 crate 是**纯库**（给 minui/minarch 用）。show（启动画面工具）与 keymon（系统按键守护）是**平台自治 bin，拆为独立 crate**（`platforms/tg5040/show`、`platforms/tg5040/keymon`）——由平台子 xtask（`platforms/tg5040/xtask`，包名 `tg5040-xtask`）编译并装配进发布包。show/keymon 依赖平台 lib 的 pub 面（`Tg5040`/`SettingsHandle`），evdev 读取模块随 keymon 迁入其 crate（平台 lib 不再暴露 evdev）。show 是平台自治工具——C 原版中 show 只在 8/12 平台存在（各平台 makefile.copy 自治复制），本平台以独立 crate 自包含实现，是否打包由平台决定。

### 3.4 为什么 evdev 读取用 libc 而不用 evdev crate？

keymon 需要读 Linux 内核输入设备（`/dev/input/event*`）。Rust 生态有现成的 `evdev` crate，但 keymon **用 libc 直接实现**（evdev 模块随 keymon 在 `platform-tg5040-keymon/src/evdev.rs`）：

- evdev 的核心操作就是 `open` + `read` 一个固定布局的结构体（`struct input_event`）——libc 十几行代码即可覆盖，无需引入依赖
- keymon crate 的第三方依赖只有 `libc`——依赖面最小化（config.yaml：非必要不增加复杂度）
- 对应原版 C 就是直接 `open`/`read` 系统调用——libc 是"最忠实对应"

唯一的代价是 `libc::read` 的 unsafe 必须集中管理——集中在 evdev 模块一个文件（约 200 行），带完整 `# Safety` 章节。

---

## 4. 核心概念

在深入各子系统章节之前，先建立四个心智模型。每个概念都有对应的深度章节，这里只给结论和直觉。

### 4.1 双通道输入架构（最重要）

本平台的输入由**两个独立通道**构成——理解输入系统的前提：

```
通道 A：应用级按键（minui/minarch 进程内）    通道 B：系统级按键（keymon 独立进程）
┌──────────────────────────────┐            ┌──────────────────────────────┐
│ SDL_Joystick                 │            │ evdev 直读 /dev/input/event0-3 │
│  十字键 / A / B / X / Y       │            │  MENU(314/315/316)            │
│  START / SELECT / L1 / R1    │            │  PLUS(115) / MINUS(114)       │
│  JOY_MENU(8) / 摇杆           │            │  MUTE(1) / JACK(2)            │
└──────────────┬───────────────┘            └──────────────┬───────────────┘
               │ SDL_PollEvent                              │ 60fps 轮询
               ▼                                            ▼
        poll_input → InputState                    设置值修改（亮度/音量/静音/耳机）
        tapped_menu（菜单轻触检测）                        │
               │                                          ▼
               │                            settings 模块（共享内存 + 硬件 + 持久化）
               │                                          │
               └────────── 读设置值显示 UI ◀───────────────┘
```

- **通道 A**（第 8 章）：游戏按键——十字键、A/B/X/Y、START/SELECT 等，由 minui/minarch **进程内**消费 SDL joystick 事件
- **通道 B**（第 11 章）：系统按键——音量、亮度、静音、耳机插拔，由 **keymon 独立进程**通过 evdev 直读硬件，任何时刻都能响应（即使 minui/minarch 崩溃）

**为什么需要两个通道？** 系统级按键（音量/亮度）必须在**任何时刻**都能响应——游戏运行中、菜单中、甚至 minui/minarch 崩溃黑屏时。所以它们由 keymon 这个**独立进程**监控，不依赖任何上层程序的生死。

### 4.2 画布 = 物理分辨率

`SCREEN_WIDTH/HEIGHT` 直接等于物理屏分辨率，flip 恒为 1:1 无缩放（第 2 章已详述）。上层渲染逻辑（render crate）拿到 `VideoBuffer` 后按物理分辨率作画，平台 flip 时原样上传呈现。

### 4.3 编译期设备区分

smart/brick 的差异（分辨率/按键映射/LED 路径）全部用 `#[cfg(feature = "smart")]` / `#[cfg(feature = "brick")]` **正向**编译期区分，**没有** C 原版的 `is_brick` 运行时变量，也**没有** `#[cfg(not(...))]` 负向逻辑。两个设备 feature 互斥且必选、**无默认**——`src/lib.rs` 顶部的两条 `compile_error!` 断言强制"恰好启用一个"（设备选择必须显式，未指定或同时指定两个都会编译报错）。结论：每个设备编译出独立二进制、独立安装包（第 12 章有完整推导）。构建命令示例：`cargo test -p platform-tg5040 --features smart`（或 `--features brick`）、`cargo build -p minui --features platform-tg5040/brick --release`。

### 4.4 RGB565 与 VideoBuffer

- **RGB565**：每像素 2 字节（5 位红 + 6 位绿 + 5 位蓝）——掌机屏幕的标准格式，也是 `VideoBuffer` 的存储格式
- **VideoBuffer**（common crate）：`Vec<u16>` 像素 + pitch/height 的画布抽象——上层画布，平台 flip 的输入。字节布局：行连续、每行 `pitch × 2` 字节
- 本 crate 不产生像素，只做两件事：**提供画布约定**（常量）和**把画布内容搬到屏幕**（flip）

---

## 5. 模块总览

### 5.1 文件布局

```
platforms/tg5040/
├── Cargo.toml          ← 依赖声明 + feature（smart/brick）——纯 lib，无 bin
├── install/            ← boot.sh / update.sh / 安装图（第 13 章）
├── cores/              ← 平台 libretro 核心声明 + 补丁（make 构建，第 13 章）
├── src/
│   ├── lib.rs          ← ★ Tg5040 结构体 + Platform trait 实现（1,010 行）
│   ├── input.rs        ← ★ 通道 A：SDL 事件 → InputState 纯逻辑状态机（718 行）
│   ├── settings.rs     ← ★ 系统设置：共享内存 + 硬件 + 持久化（libmsettings 对应，735 行）
│   └── power.rs        ← ★ 电源/背光/CPU/睡眠/关机（sysfs 读写纯函数，286 行）
├── show/               ← show 平台自治 bin（独立 crate platform-tg5040-show）
│   ├── Cargo.toml      ← 依赖平台 lib/common/png
│   └── src/main.rs     ← ☆ 启动画面显示工具（自实现 PNG 解码）
├── keymon/             ← keymon 系统按键守护（独立 crate platform-tg5040-keymon）
│   ├── Cargo.toml      ← 依赖平台 lib/libc
│   └── src/
│       ├── main.rs     ← ★ 通道 B：系统级按键守护进程入口（565 行）
│       └── evdev.rs    ← ★ 内核 input_event 读取封装（随 keymon 迁入，192 行）
└── xtask/              ← 平台子 xtask（独立 crate tg5040-xtask，平台自治打包编排）
    ├── Cargo.toml      ← 仅 clap
    └── src/main.rs     ← 编译 show/keymon + cores make + 装配复制（父 xtask 调用）
```

| 文件 | 职责 | 对应原 C |
|------|------|---------|
| `src/lib.rs` | 结构体 + trait 实现 + 事件适配层（`translate_event`） | `platform.c` / `platform.h` |
| `src/input.rs` | 输入映射常量 + `PadState` 状态机（零 SDL 依赖，可单测） | `platform.h` 映射 + `api.c` 的 `PAD_*`（1180-1365） |
| `src/settings.rs` | Settings 结构体 + SettingsHandle（四重角色） | `libmsettings/msettings.c` |
| `src/power.rs` | sysfs 读写纯函数 + 档位映射 | `platform.c` 的 PLAT_* 电源函数 |
| `keymon/src/main.rs` | keymon 主循环 + 信号处理 + mute 监控线程 | `keymon.c` |
| `keymon/src/evdev.rs` | `InputEvent`（内核 ABI）+ 设备读取（随 keymon 迁出平台 lib） | `keymon.c` 的设备打开/读取部分 |
| `show/src/main.rs` | show 启动画面：加载 PNG → 居中 → sleep → 退出（自实现 load_png/blit_buffer） | `show/show.c` |
| `xtask/src/main.rs` | 平台自治打包：编译 show/keymon、cores make、装配复制 | `makefile.copy` + 平台各 make |

### 5.2 Tg5040 结构体（内部结构）

所有 C 全局变量（`vid`、`pad`、`snd` 等）收拢为结构体字段（`src/lib.rs:68`）：

```rust
pub struct Tg5040 {
    sdl: Option<sdl2::Sdl>,                             // SDL 总开关（活得最久，drop 时 SDL_Quit）
    canvas: Option<sdl2::render::WindowCanvas>,         // window+renderer 合体
    texture: Option<sdl2::render::Texture>,             // RGB565 流纹理，物理尺寸
    joystick: Option<sdl2::joystick::Joystick>,         // 游戏手柄（init_input 填充）
    audio_device: Option<sdl2::audio::AudioDevice<AudioCallbackImpl>>,  // 音频设备
    audio_buffer: Arc<Mutex<AudioRingBuffer>>,          // 环形缓冲（回调线程共享）
    sample_rate_in: u32,                                // 模拟器产生采样率
    sample_rate_out: u32,                               // 硬件实际采样率
    pad: crate::input::PadState,                        // 输入跨帧状态（按键掩码+重复计时+摇杆）
    event_pump: Option<RefCell<sdl2::EventPump>>,       // SDL 事件泵（poll_input 与 should_wake 共用）
}
```

设计要点：

- **`Option<T>` 字段**：`new()` 时全部为 `None`——`new()` 零 SDL 调用、无副作用，可在无 SDL 环境构造（单元测试不需要真实设备）。SDL 资源按子系统**"用到时初始化"**（`init_video` 填视频字段、`init_input` 填输入字段、`init_audio` 填音频字段）
- **无 `is_brick` 字段**——设备差异全部由 `#[cfg(feature)]` 编译期处理（对应 C 的 `is_brick` 全局变量，详见第 12 章）
- **`audio_buffer` 用 `Arc<Mutex<...>>`**：SDL 音频回调在独立线程执行，回调结构与主线程经 `Arc` 共享环形缓冲的所有权（详见第 9 章）
- **`event_pump` 用 `RefCell`**：`should_wake(&self)` 是共享引用（trait 签名）而事件泵消费需可变借用——单线程主循环内 `RefCell` 安全，且 `RefCell` 非 `Sync` 正好阻止跨线程（SDL 事件队列要求主线程）

### 5.3 为什么是 canvas + sdl 而不是 window + renderer？

**C 原版**有两个独立对象：`vid.window`（SDL_CreateWindow）+ `vid.renderer`（SDL_CreateRenderer(window, ...)）——C 的 API 允许分开创建。

**sdl2 crate 没有安全 API 独立创建 renderer**——唯一安全路径是 `window.into_canvas()...build()`，它**消费 window** 并返回 `Canvas<Window>`（window + renderer 焊死合体）。如果照搬 C 的分离字段，`renderer` 字段将**无法用安全代码填充**（独立 `RendererContext` 只能 `unsafe from_ll` 手搓）。所以合并为 `canvas: Option<WindowCanvas>`——这是 sdl2 设计使然，不是 Rust 的能力缺失。

**`sdl: Option<Sdl>` 的生命周期**：`sdl2::init()` 返回 `Sdl` 对象，drop 时自动调用 `SDL_Quit` 关闭整个 SDL。如果 init_video 里局部持有 Sdl——函数返回时 Sdl drop → SDL_Quit → 已存入结构体的 canvas 变成**悬空**（后续 flip 是未定义行为）。所以 Sdl 必须活得比所有 SDL 对象久，存进结构体。

**关键事实**：`sdl2::init()` = `SDL_Init(0)`——**只初始化 SDL 核心，不碰任何子系统**！子系统通过 `sdl.video()`（内部 `SDL_InitSubSystem(SDL_INIT_VIDEO)`）/ `sdl.audio()` / `sdl.joystick()` **按需初始化**——完美匹配 C 的"用到时初始化"（minui 进程永不调用 `sdl.audio()`，不会白白初始化音频设备）。

**为什么 `new()` 不直接存 SDL 字段？** 因为 SDL2 的初始化在掌机上是"要么成功，要么设备重启"——没有优雅降级。Rust 的 `Tg5040::new()` 不初始化 SDL（`init_*` 才做），这样 `new()` 总是成功，初始化失败发生在可控的位置。同时 `new()` 无副作用 → 可在无 SDL 环境构造（单元测试不需要真实设备）。

## 6. 系统架构与启动流程

本章从"硬件上电到 Rust minui.elf 运行"完整揭示 tg5040 平台的启动链、文件去向和内部存储结构。这是理解"为什么 keymon 必须保留"、"为什么 boot.sh 可以写 sysfs"、"为什么安装图要放包根目录"等所有设计决策的基础。

### 6.1 系统架构全景

#### 6.1.1 完整启动链路

从按下电源键到看到 MinUI 主界面的完整路径：

```
══════════════════════════════════════════════════════════════════
                        硬件上电
══════════════════════════════════════════════════════════════════
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│  设备固件 ROM（不可写，出厂预装）                                  │
│                                                                 │
│  /etc/main  ──→  main.sh（系统启动入口，MinUI 首次安装时替换）      │
│      │                                                          │
│      ├── 等待 SD 卡挂载（循环检测 /proc/mounts）                   │
│      ├── 检查 /mnt/SDCARD/.tmp_update/updater 是否存在？          │
│      │     ├── 存在（有更新包）→ exec .tmp_update/updater         │
│      │     │     └──→ boot.sh（安装/更新流程 —— 见下文 ①）          │
│      │     └── 不存在 → exec /etc/main.original（原厂系统）        │
│      └── 首次安装后，原厂 /etc/main 备份为 /etc/main.original       │
└─────────────────────────────────────────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────────────┐
│  /usr/trimui/bin/runtrimui.sh（每次开机入口）                     │
│      │ 首次安装时由 install.sh 从 SYSTEM/tg5040/dat/ 复制到此      │
│      │ 原厂脚本备份为 runtrimui-original.sh                        │
│      │                                                          │
│      ├── 等待 SD 卡挂载                                          │
│      ├── 检查 /mnt/SDCARD/.tmp_update/updater 是否存在？          │
│      │     └── 存在 → exec .tmp_update/updater（安装/更新流程）     │
│      └── 不存在 → 正常启动路径 ↓                                   │
└─────────────────────────────────────────────────────────────────┘
                          │
══════════════════════════════════════════════════════════════════
                  SD 卡（用户数据，可读写）
══════════════════════════════════════════════════════════════════
                          │
┌─────────────────────────▼───────────────────────────────────────┐
│  MinUI.pak/launch.sh（真正的启动循环）                            │
│                                                                 │
│  1. 环境变量: PLATFORM=tg5040, SDCARD_PATH=/mnt/SDCARD, ...      │
│  2. GPIO 初始化: 震动/背光/DIP 开关                               │
│  3. 启动守护进程: keymon &（后台——亮度/音量/Menu 响应）            │
│  4. 进入循环:                                                    │
│     ┌──────────────────────────────────────────────────┐        │
│     │  minui                ← Rust 编译产物（启动器）    │        │
│     │    ↓ 用户选 ROM → 写 /tmp/next                     │        │
│     │  eval $(cat /tmp/next) ← shell 脚本启动 minarch    │        │
│     │    ↓ 游戏退出 → 回到循环                            │        │
│     └──────────────────────────────────────────────────┘        │
│  5. 关机: exec shutdown                                          │
└─────────────────────────────────────────────────────────────────┘
```

#### 6.1.2 逐文件讲解（按启动顺序）

**`/etc/main`（= main.sh）**——系统启动入口

- **谁创建**：MinUI 首次安装时由 `MainUI` 脚本复制（`cp main.sh /etc/main`）
- **什么时候执行**：**每次开机**（固件最早执行点）
- **做什么**：等待 SD 卡挂载 → 检查 `.tmp_update/updater` 是否存在
  ```sh
  # 核心逻辑（简化）
  UPDATER_PATH=/mnt/SDCARD/.tmp_update/updater
  if [ -f "$UPDATER_PATH" ]; then
      exec "$UPDATER_PATH"          # 有更新包 → 进 boot.sh
  else
      exec /etc/main.original       # 正常 → 原厂系统
  fi
  ```
- **为什么重要**：这是 MinUI 在设备上改写的**唯二文件之一**（另一个是 `/usr/trimui/bin/runtrimui.sh`）。删掉 `/etc/main` 并恢复 `.original` 即回原厂——**拔卡即可**（这两个修改在 ROM 内部而不是 SD 卡上，但原厂备份保留了回退能力）

**`runtrimui.sh`**——每次开机的 SD 卡入口

- **谁创建**：MinUI 首次安装时由 `install.sh` 复制（`cp SYSTEM/tg5040/dat/runtrimui.sh /usr/trimui/bin/`）
- **什么时候执行**：**每次开机**（main.sh 之后，stock firmware 调用）
- **做什么**：与 main.sh 类似——等 SD 卡 → 检查 updater → 有则进 boot.sh / 无则跳过（继续后续）
  ```sh
  # 核心逻辑
  if [ -f "$UPDATER_PATH" ]; then exec "$UPDATER_PATH"; fi
  # 否则继续（本脚本无 exec，返回到 stock 流程的下一步）
  ```
- **为什么重要**：虽然非首次后实际上不需要这个检测（updater 只在安装时出现），但**保留它是防止用户重复刷机时出错**（MinUI 作者的设计经验）

**`boot.sh`**——安装/更新检测

- **谁创建**：平台子 xtask（`tg5040-xtask`）装配时复制 `install/boot.sh` 为 `BOOT/common/tg5040.sh`（设备上变为 `.tmp_update/tg5040.sh`）
- **什么时候执行**：**安装时**（main.sh 或 runtrimui.sh 检测到 updater 存在时调用）
- **做什么**：CPU 调速 → 关 LED → 显示安装画面（`./show`）→ unzip MinUI.zip 到 SD 卡 → 调用 `install.sh` → 重启
- **Rust 版修正**：设备检测代码已移除（编译期 feature 已确定设备——详见第 12 章「设备区分机制」与第 13 章「打包与安装」）

**`install.sh`**（= update.sh）——系统部署

- **谁创建**：平台子 xtask（`tg5040-xtask`）装配时复制 `install/update.sh` 为 `SYSTEM/tg5040/bin/install.sh`
- **什么时候执行**：**每次 boot.sh 启动时**（boot.sh 末尾调 `install.sh`）
- **做什么**：替换 `/usr/trimui/bin/runtrimui.sh`（首次）/ 拷贝 brick 历史数据迁移（tg3040→tg5040）
  ```sh
  # 核心：替换 stock 的 runtrimui.sh
  if [ -f /usr/trimui/bin/runtrimui.sh ] && [ ! -f /usr/trimui/bin/runtrimui-original.sh ]; then
      mv /usr/trimui/bin/runtrimui.sh /usr/trimui/bin/runtrimui-original.sh
      cp ./runtrimui.sh /usr/trimui/bin/
  fi
  ```
- **为什么重要**：只有首次安装时执行替换；后续每次 boot.sh 运行它是为了做 brick 数据迁移等长尾任务

**`launch.sh`**（`MinUI.pak/launch.sh`）——真正的启动循环

- **谁创建**：skeleton 文件（MinUI.zip 解压直接到位）
- **什么时候执行**：**每次开机**（正常启动路径的最终脚本）
- **做什么**：环境变量 → GPIO 初始化 → **后台启动 keymon** → 循环执行 `minui`（选游戏 → shell 脚本启动 `minarch` → 退出回到循环）

这是整个架构的**核心循环**——"选游戏"和"玩游戏"是两个独立进程（minui ↔ minarch），通过 `/tmp/next` 文本文件通信。

#### 6.1.3 两套路径的区分

很多混乱源于把"安装时"和"每次开机"混为一谈。显式区分：

```
安装时（每个设备一生执行 1 次）:           每次开机（之后每次启动）:
  main.sh 检测 updater → boot.sh          main.sh → runtrimui.sh
    → unzip MinUI.zip 到 SD 卡               → launch.sh（keymon & + minui 循环）
    → install.sh 替换 ROM 文件
    → 重启
```

### 6.2 文件结构和去向

所有文件的来源、中间路径、设备目标位置和复制责任方：

| 文件 | 来源 | 打包路径（中间） | 设备目标 | 作用 | 复制者 |
|------|------|----------------|---------|------|--------|
| `main.sh` | `skeleton/BOOT/trimui/app/` | 直接（skeleton） | `/etc/main` | 系统启动入口 | MainUI（首次安装） |
| `MainUI` | `skeleton/BOOT/trimui/app/` | 直接（skeleton） | 执行一次 | 安装部署脚本（cp main.sh + .tmp_update） | 无（直接执行） |
| `runtrimui.sh` | `skeleton/BOOT/trimui/app/` | `SYSTEM/tg5040/dat/` | `/usr/trimui/bin/runtrimui.sh` | 每次开机入口 | `install.sh`（首次安装） |
| `boot.sh` | `install/boot.sh` | `BOOT/common/tg5040.sh` | `.tmp_update/tg5040.sh` | 安装/更新检测 | 平台子 xtask（装配） |
| `install.sh` | `install/update.sh` | `SYSTEM/tg5040/bin/install.sh` | `.system/tg5040/bin/install.sh` | 系统部署（替换 ROM 文件） | 平台子 xtask（装配） |
| 安装图 | `install/{,*}.png`（smart/brick 不同） | `BOOT/common/tg5040/` | .tmp_update 目录 | 安装/更新画面 | 平台子 xtask（装配，按 device 选源） |
| `launch.sh` | `skeleton/SYSTEM/tg5040/paks/MinUI.pak/launch.sh` | 直接（MinUI.zip 解压） | 直接（`/mnt/SDCARD/` 下） | 真正的启动循环（keymon & + minui 循环） | xtask（最终 MinUI.zip 打包） |
| `keymon` | **Rust 编译产物**（`cargo build -p platform-tg5040-keymon --features smart`，无 `.elf` 后缀） | `SYSTEM/tg5040/bin/` | `.system/tg5040/bin/` | 亮度/音量/静音/耳机守护进程（evdev 直读 GPIO） | 平台子 xtask（编译 + 复制） |
| `show` | **Rust 编译产物**（`cargo build -p platform-tg5040-show --features smart`，无 `.elf` 后缀） | `SYSTEM/tg5040/bin/` + `BOOT/common/tg5040/` | `.system/tg5040/bin/` + `.tmp_update/tg5040/` | 安装/更新画面显示（boot.sh 的 `./show ./$ACTION.png`） | 平台子 xtask（编译 + 复制两处） |
| `minui` | **Rust 编译产物**（`cargo build -p minui`，无 `.elf` 后缀） | `SYSTEM/tg5040/bin/` | `.system/tg5040/bin/` | MinUI 启动器（文件浏览、ROM 选择） | xtask（system 复制） |
| `minarch` | **Rust 编译产物**（`cargo build -p minarch`，无 `.elf` 后缀） | `SYSTEM/tg5040/bin/` | `.system/tg5040/bin/` | MinUI 游戏内前端（libretro 核心加载） | xtask（system 复制） |

**注意**：skeleton 中的 `keymon` 文件是**空壳文件**（内容 `#!/bin/sh\nexit 0`）——真实的 `keymon` 由 `platform-tg5040-keymon` crate 的 Rust 源码编译产出（`keymon/src/main.rs`，全 Rust 实现，见第 11 章）。这是与 C 原版打包链的一个差异：keymon/show 的编译从 C make 链移入 cargo 构建（独立 crate——平台子 xtask 执行 `cargo build -p platform-tg5040-keymon --features smart`（设备 feature 必选）编译）。**命名约定**：Rust 版产物统一无 `.elf` 后缀（skeleton 的 launch.sh 引用同名），与 C 原版（`keymon.elf` 等）的分叉是刻意的命名统一（见 xtask 文档「无 .elf 后缀」）。

### 6.3 内部存储结构（非 SD 卡部分）

#### 6.3.1 两个存储域

TrimUI 设备有两个隔离的存储域：

```
设备固件 ROM（只读，出厂预装）                 SD 卡（用户数据，可读写）
────────────────────────────────       ────────────────────────────────
/etc/main             ← 系统启动入口    /mnt/SDCARD/
/etc/main.original    ← 原厂备份
                                       .tmp_update/
 /usr/trimui/bin/                        ├── updater       → boot.sh 调用
 runtrimui.sh         ← 每次开机入口     └── tg5040.sh     → boot.sh 本身
/usr/trimui/bin/
 runtrimui-original.sh ← 原厂备份       .system/tg5040/
                                       ├── bin/minui  ← Rust 编译（无 .elf 后缀）
                                       ├── bin/keymon ← Rust 编译（keymon 重写后）
                                       ├── dat/runtrimui.sh
                                       └── paks/MinUI.pak/launch.sh
```

#### 6.3.2 MinUI 在 ROM 中只修改两个文件

```
/etc/main                        → 替换为 main.sh（备份 .original）
/usr/trimui/bin/runtrimui.sh     → 替换为 runtrimui.sh（备份 .original）

其余全部文件在 SD 卡上：
  ├── 二进制: minui / minarch / keymon / show（无 .elf 后缀）
  ├── 脚本:   launch.sh / boot.sh / update.sh / runtrimui.sh（dat/ 副本）
  ├── 资源:   图集 / 字体 / 配置 / libretro 核心
  └── 用户数据: ROMs / Saves / .userdata/
```

#### 6.3.3 为什么拔卡即回原厂

ROM 中只改了 `main` 和 `runtrimui.sh`——**两个文件**。原厂版本各备份为 `.original`。

```
"拔卡"的意思是把 SD 卡拔掉，但 /etc/main 还在——
设备开机时 /etc/main 检测 updater 不存在 → exec /etc/main.original（原厂备份）
→ 设备行为完全恢复原厂 ✅

"卸载"更彻底：把 main.original 恢复为 /etc/main，删除 runtrimui.sh 并恢复 original
→ ROM 回到从未安装过 MinUI 的状态 ✅

MinUI 本身不需要"卸载脚本"——拔卡即回原厂。
重新插入 SD 卡 → 检测 updater → 重新安装 → 继续使用 MinUI。
```

#### 6.3.4 为什么 keymon 必须独立于 minui/minarch

`keymon` 是独立二进制进程，不内嵌进 minui/minarch，原因：

1. **evdev 直读**：keymon 通过 Linux evdev（`/dev/input/event*`）直读硬件按键事件——绕过 SDL。按键 scancode 与 SDL 的 JOY_* 完全不同（如 PLUS 的 evdev scancode=115，SDL JOY_PLUS=128）
2. **亮度/音量调节**：通过 settings 模块写硬件（`/dev/disp` ioctl / `amixer`）——这是硬件级操作，非 SDL 领域
3. **硬件 GPIO 监控**：mute 线程每 200ms 轮询 GPIO243（静音开关）和耳机插拔（CODE_JACK=2）——纯硬件事件
4. **独立进程不随 minui/minarch 崩溃**：即使 Rust 程序 panic，keymon 还活着——音量/亮度仍能调节

**Rust 的 `poll_input` 的职责**：只做"按了什么键"的**物理按键映射**（JOY_*/CODE_* → BTN_*）——与 keymon 的"亮度/音量怎么变"是上下层关系，不冲突（双通道架构，见第 4.1 节）。

---

## 7. video — 视频

视频子系统是平台最先实现的部分（`implement-tg5040-video` 变更，2026-08-02 归档）。本平台的视频设计围绕一个核心决策：**画布 = 物理分辨率，flip 恒 1:1**（第 2 章推导），本节讲三个方法（`init_video`/`flip`/`quit_video`）的具体实现。

### 7.1 视频字段设计（决策点 1：canvas + sdl）

视频相关的 `Tg5040` 字段（完整结构体见第 5.2 节）：

```rust
sdl: Option<sdl2::Sdl>,                     // SDL 总开关（drop 时 SDL_Quit）
canvas: Option<sdl2::render::WindowCanvas>, // window+renderer 合体
texture: Option<sdl2::render::Texture>,     // RGB565 流纹理，物理尺寸
```

- **为什么是 canvas 而不是 window + renderer**：sdl2 crate 无安全 API 独立创建 renderer（`into_canvas()` 消费 window）——已归档的决策（详见第 5.3 节的完整推理）
- **`sdl` 必须活得比所有 SDL 对象久**：局部持有会在函数返回时 drop → `SDL_Quit` → 已存入结构体的 canvas 悬空（UB）——所以 `sdl` 存进结构体
- **纹理 = 物理尺寸**（决策点 2）：`SDL_UpdateTexture` **不做缩放**——上传的数据必须与纹理同尺寸。画布（VideoBuffer）= 物理分辨率，纹理也只能 = 物理尺寸，flip 呈现 1:1

### 7.2 init_video —— 视频初始化（决策点 5）

`src/lib.rs` 的 `init_video()` 完整流程（8 步）：

```
1. sdl2::init()                        → 存 self.sdl
   = SDL_Init(0)——只初始化核心，不碰子系统
2. sdl.video()                          → 内部 SDL_InitSubSystem(SDL_INIT_VIDEO)，按需初始化
3. video.window("", 1280, 720).build()  → 默认位置 Undefined、默认显示
4. window.into_canvas().accelerated()
   .present_vsync().build()             → = SDL_CreateRenderer(ACCELERATED|PRESENTVSYNC)
5. sdl.mouse().show_cursor(false)       → = SDL_ShowCursor(0)——掌机无鼠标
6. hint::set("SDL_RENDER_SCALE_QUALITY", "0")   → 最近邻采样
7. canvas.texture_creator()
   .create_texture_streaming(RGB565, 1280, 720) → 物理尺寸流纹理
8. 全部 .expect("...") panic            → init 失败无回退（见下）
```

**为什么失败是 panic 而不是 `Result`？**（决策点 3）`Platform` trait 的 `init_video` 返回 `VideoBuffer`（无 Result）——这是归档 trait 的既定契约：SDL 初始化在掌机上"要么成功，要么设备重启"，没有优雅降级路径。所有 `.expect("...")` 的 panic 消息都指明失败阶段（如 "failed to create window"），便于真机排障。

**对应 C 原版**：`PLAT_initVideo`（platform.c）——`SDL_CreateWindow` + `SDL_CreateRenderer` + `SDL_CreateTexture` 三步，Rust 版只是按 sdl2 crate 的安全 API 重组为链式调用，语义一一对应。

### 7.3 flip —— 画面提交（决策点 2+4）

`flip(buffer, wait_vsync)` 把上层画好的 `VideoBuffer` 提交到物理屏幕：

```rust
fn flip(&mut self, buffer: &VideoBuffer, _wait_vsync: bool) {
    let bytes = buffer_as_bytes(buffer);          // ① RGB565 像素 → 字节切片
    self.texture.as_ref().unwrap().update(None, bytes, pitch * 2).unwrap(); // ② 1:1 上传
    self.canvas.as_ref().unwrap().copy(self.texture.as_ref().unwrap(), None, None).unwrap(); // ③ 1:1 呈现
    self.canvas.as_ref().unwrap().present();      // ④ PRESENTVSYNC 阻塞到垂直同步
}
```

**`buffer_as_bytes` 抽纯函数**（`src/lib.rs:274`）：`VideoBuffer`（`Vec<u16>`）转 SDL 字节切片涉及 `unsafe from_raw_parts`——抽成独立纯函数的好处：

1. **unsafe 集中一处**——flip 本身无 unsafe，前置条件在 `# Safety` 章节明确：
   ```
   # Safety
   前置条件：buffer.pitch × buffer.height × 2 ≤ buffer.pixels.len()
   ```
2. **可单元测试**——pitch=width、pitch>width（硬件对齐）、空 buffer 三种边界

### 7.4 wait_vsync 语义说明（决策点 6，重要）

**`wait_vsync` 参数被忽略（接收但不用）**。原因：

1. **PRESENTVSYNC 已固定 vsync**：`into_canvas().present_vsync()` 在创建 renderer 时已开启垂直同步——`present()` 天然阻塞到 vsync，wait_vsync 参数没有用武之地
2. **与 C 一致**：原版 `PLAT_flip(screen, ignored)` 同样忽略该参数
3. **双重节流降帧率**：若 flip 内部再按 wait_vsync 做帧预算延迟，会与 PRESENTVSYNC 叠加成双重节流（帧率下降）

**帧率控制在上层**：主循环用 `now_ms()`（`SDL_GetTicks` 包装）计时 + `std::thread::sleep` 补足帧预算——这是 common/上层的主循环职责，平台只保证"present 阻塞到 vsync"。

**`now_ms` 的实现细节**：`src/lib.rs` 用 `unsafe { sdl2::sys::SDL_GetTicks() }`（FFI 调用，带 `# Safety` 章节）而非 `TimerSubsystem::ticks()`——后者要求先初始化 TIMER 子系统，与"无需子系统初始化"的设计冲突。`SDL_GetTicks` 内部对时钟懒初始化，任何阶段调用都安全（对应原版 `api.c` 的 `#define ms SDL_GetTicks`）。

### 7.5 quit_video —— 视频清理（决策点 7）

`quit_video()` 三步清理：

```
1. 清屏保险：canvas.clear() + present() × 3
   → 对应 C clearVideo 3 次——清空 vsync 双缓冲，防退出后屏幕残留最后一帧
2. RAII 按序销毁：texture = None → canvas = None → sdl = None
   → 字段 drop 顺序保证 Texture 先于 Canvas、Canvas 先于 Sdl（SDL_Quit）
3. 清 framebuffer：std::process::Command 执行 cat /dev/zero > /dev/fb0
   → 对应 C——防 fbcon 残留（切换进程时显示旧 framebuffer）
```

**为什么不用 `Option::take` 的顺序问题？** Rust 的字段 drop 顺序是声明顺序——`texture` 在 `canvas` 之前声明，drop 时自动先销毁纹理再销毁画布，最后 `sdl`（`SdlDrop` 触发 `SDL_Quit`）。RAII 把 C 时代"手动按序 Destroy"的崩溃源（顺序错误）彻底消除。

### 7.6 视频相关否决方案（决策点 1-7 汇总）

| 方案 | 否决理由 |
|------|---------|
| `unsafe from_ll` 创建独立 RendererContext | 避免不必要 unsafe；sdl2 设计就是 canvas 合体 |
| 保留 window 字段 + 用 Canvas | `into_canvas()` 消费 window——所有权冲突，无法同时持有 |
| sdl 不存结构体（局部持有） | 函数返回时 Sdl drop → SDL_Quit → canvas 悬空 UB |
| 纹理建逻辑尺寸 + GPU 放大 | 已随"画布=物理"消失（UpdateTexture 不缩放，纹理必须=画布尺寸） |
| flip 内部做帧预算延迟 | flip 不知道帧开始时刻（bool 参数信息不足）；与 PRESENTVSYNC 双重节流降帧率 |
| Platform trait 增加 sync 方法 | 推翻已归档 trait；违背"帧计时在上层"分工 |
| sdl2::sys 手动 InitSubSystem | 绕过封装引入 unsafe，不需要（sdl2::init + sdl.video() 已按需） |
| quit_video 只置 None（无清屏保险） | 退出残留最后一帧——切换瞬间显示旧画面 |
| 照搬 C 的 LockTexture+FillRect 清屏 | `canvas.clear()` 一步等效；Rust 无软件 surface |
| mock canvas（抽象 trait） | sdl2 Canvas 是具体类型——抽象=过度设计（config 禁止类型体操） |

### 7.7 真机验证清单

| 验证项 | 说明 |
|--------|------|
| flip 性能 | `now_ms()` 量化 flip 前后耗时（帧预算 17ms 内的实际余量） |
| 清屏保险 | 退出 minui 切到 minarch 时无上一帧残留 |
| vsync 行为 | PRESENTVSYNC 下 present() 阻塞节奏（60fps） |

## 8. input — 输入

本平台 crate 的 `input` 模块实现**通道 A**（应用级按键，`implement-tg5040-input` 变更，2026-08-08 归档）：minui/minarch 进程内消费 SDL joystick 事件，产出 `InputState` 供 UI 查询。系统级按键（通道 B）见第 11 章。

### 8.1 分层：纯逻辑层 + 适配层（可测性设计）

```
minui/minarch 进程（poll_input，每帧）
┌──────────────────────────────────────────────┐
│ SDL 事件泵（EventPump，Tg5040 持有）           │
│   ├─ JOYBUTTONDOWN/UP → JOY_* 映射           │
│   ├─ JOYHATMOTION     → 十字键状态机          │
│   ├─ JOYAXISMOTION    → L2/R2 触发 + 摇杆方向  │
│   └─ KEYDOWN/UP       → CODE_* 兜底           │
│              │ AppInputEvent（抽象事件）       │
│              ▼                               │
│ input::process_frame（纯逻辑，可单测）          │
│   ├─ repeat 扫描（300ms 首重复 + 100ms 间隔）  │
│   └─ 键位状态更新 → InputState                 │
└──────────────────────────────────────────────┘
```

**分层**：`src/input.rs` 是**纯逻辑层**（事件抽象 `AppInputEvent` + 跨帧状态 `PadState` + `process_frame`，零 SDL 依赖可单测）；sdl2 事件到 `AppInputEvent` 的转换（`translate_event`，`src/lib.rs:157`）在适配层——状态机测试不依赖 SDL，适配层只做机械转换。

### 8.2 事件源映射表（对照原版 platform.h）

常量定义在 `src/input.rs`（`src/input.rs:36` 起）：

| 事件类型 | 常量 | 值（smart / brick） | 键位 |
|---------|------|---------------------|------|
| JOYBUTTON | JOY_A / JOY_B / JOY_X / JOY_Y | 1 / 0 / 3 / 2 | BTN_A/B/X/Y |
| JOYBUTTON | JOY_L1 / JOY_R1 | 4 / 5 | BTN_L1/R1 |
| JOYBUTTON | JOY_SELECT / JOY_START | 6 / 7 | BTN_SELECT/START |
| JOYBUTTON | JOY_MENU | 8 | BTN_MENU |
| JOYBUTTON | JOY_POWER | 102 | BTN_POWER |
| JOYBUTTON | JOY_PLUS / JOY_MINUS | 128/129（smart）、14/13（brick） | BTN_PLUS/MINUS |
| JOYBUTTON | JOY_L3 / JOY_R3 | brick only 9 / 10 | BTN_L3/R3 |
| HAT | 十字键 | JOY_UP/DOWN/LEFT/RIGHT 无定义 | BTN_DPAD_*（4 方向状态机） |
| AXIS | AXIS_L2 / AXIS_R2 | 2 / 5 | BTN_L2/R2（val>0 按下） |
| AXIS | AXIS_LX/LY/RX/RY | 0/1/3/4 | ANALOG 方向键位 + laxis/raxis 原始值 |
| KEY | CODE_POWER | 102 | BTN_POWER（兜底通道） |

smart/brick 差异用 `#[cfg]` 区分（`src/input.rs`）：

```rust
// brick：PLUS/MINUS 是独立物理按键（14/13）；L3/R3 存在
#[cfg(feature = "brick")]
pub const JOY_PLUS: u8 = 14;
#[cfg(feature = "brick")]
pub const JOY_L3: u8 = 9;
// smart：PLUS/MINUS 是 128/129；L3/R3 无定义
#[cfg(not(feature = "brick"))]
pub const JOY_PLUS: u8 = 128;
```

**设备语义键**（对应原版 platform.h 的 `BTN_SLEEP`/`BTN_MOD_*` 宏，已归 `Platform` trait 关联常量，定义在 `src/lib.rs` 常量区）：

```rust
const BTN_SLEEP: u32 = BTN_POWER;              // 电源键（睡眠键位，BTN_SLEEP == BTN_POWER）
const BTN_MOD_BRIGHTNESS: u32 = BTN_MENU;      // 亮度修饰键（MENU 按住 + PLUS/MINUS = 调亮度）
const BTN_MOD_VOLUME: u32 = BTN_NONE;          // 无音量修饰键（tg5040 的 PLUS/MINUS 直接调音量）
const BTN_MOD_PLUS: u32 = BTN_PLUS;
const BTN_MOD_MINUS: u32 = BTN_MINUS;
```

为什么是**参数传入**而非 common 硬编码：`tapped_menu` 在 common crate（跨平台库）——`BTN_MOD_BRIGHTNESS == BTN_MENU` 只在 11 个平台成立（m17/trimuismart 是 `BTN_START`+`BTN_R1`），硬编码会把 tg5040 的语义泄漏成所有平台的 bug（implement-tg5040-input 变更 design 决策 5）。与 `power::update` 的 `mute` 参数同一模式：**平台定义常量 → 参数传递 → common 消费**。

### 8.3 poll_input 状态机

`poll_input` 消费 SDL 事件泵，把四类事件翻译为 `InputState`（对应原版 `PLAT_pollInput`，api.c:1180-1365）。状态机逻辑是纯函数（事件序列 + 跨帧状态 → 更新），按原版语义逐行等价实现：

```
1. 帧首 repeat 扫描
   对 pressed 掩码的每个键位：tick >= repeat_at[id]
     → just_repeated |= btn，repeat_at[id] += 100
   节奏：首次 300ms，之后每 100ms

2. JOYBUTTONDOWN/UP
   按 JOY_* 映射表匹配（含 MENU 三路冗余 JOY_MENU/JOY_MENU_ALT/JOY_MENU_ALT2）
   按下 → just_pressed + just_repeated + repeat_at = tick+300
   释放 → 清 is_pressed/just_repeated，置 just_released

3. JOYHATMOTION（十字键状态机）
   对角线（LEFTUP 等）→ 同时置两方向；CENTERED → 全部释放
   原版语义细节（逐行等价保留）：
     · 释放路径无条件置 just_released（即使该方向本来没按）
     · 按下时 just_pressed + just_repeated 同时置位

4. JOYAXISMOTION
   AXIS_L2/R2 为触发按钮（val > 0 按下）——"虚假释放"防御：
     没按过则不置 just_released（原版注释 "axis will fire off what
     looks like a release before the first press"）
   AXIS_LX/LY/RX/RY 驱动 analog 方向键位（deadzone 0x4000，
     对应原版 PAD_setAnalog）并存储 laxis/raxis 原始值

5. KEYDOWN/UP
   按 CODE_* 映射——兜底 PLUS/MINUS/POWER 的键盘事件到达
```

**analog 摇杆**：LX/LY 轴超过 deadzone（0x4000）时驱动 `BTN_ANALOG_*` 方向键位（带重复计时，对应原版 `PAD_setAnalog`）；`laxis`/`raxis` 存原始值并随 `InputState` 帧快照带出，供 minarch 应答 libretro 核心的 `RETRO_DEVICE_ANALOG` 查询（implement-analog-sticks 已闭环，对应原版 `pad.laxis/raxis` 直读）。**TrimUI Brick Pro**（2026-07 发布，双霍尔摇杆 + 可点击 L3/R3，同 A133P 芯片 + 同 1024×768 分辨率，大概率复用 brick feature）落地时天然复用此承载。

### 8.4 should_wake 与事件泵

`should_wake`（睡眠等待循环中 200ms 调用）消费事件泵：`POWER` 释放事件 → 返回 `true` 并**吃掉该事件**（防止残留到 `poll_input`——原版注释 "do it here so we eat the input"）。

事件泵（`EventPump`）由 `Tg5040` 持有、`poll_input` 与 `should_wake` 共用——与原版全局 `SDL_PollEvent` 语义一致。`RefCell` 包装因 `should_wake(&self)` 是共享引用（trait 签名）而事件泵消费需可变借用——单线程主循环安全，且 `RefCell` 非 Sync 正好阻止跨线程（SDL 事件队列要求主线程）。

tg5040 无合盖——不处理 `lid` 逻辑（对应原版 `PLAT_shouldWake` 的 lid 分支）。

### 8.5 init/quit_input

`init_input` 初始化 SDL joystick 子系统并打开 0 号手柄（对应原版 `PLAT_initInput`）；`quit_input` 关闭手柄。**joystick 打开失败不 panic**——`poll_input` 仍可工作（键盘事件通道可用），与 evdev 打开失败容错先例一致（第 11 章）。

### 8.6 PLUS/MINUS 的 SDL 可达性疑点 + 真机验证清单

以下行为在开发机（macOS 手柄/无手柄）无法完全确认，**真机验证**：

| 验证项 | 疑点 | 影响 |
|--------|------|------|
| PLUS/MINUS（128/129）的 SDL 事件类型 | SDL2 joystick button 索引上限 32——128/129 可能报不出（原版可能有定制驱动）；也可能是键盘事件（CODE_PLUS 兜底） | 不可达时 UI 内 PLUS/MINUS 功能受限（音量调节走 keymon 通道不受影响） |
| POWER（102）的事件类型 | joystick button vs 键盘 scancode | 电源键长按/唤醒路径 |
| hat 方向与对角线 | 4 方向 + 对角线事件序列 | 菜单导航 |
| 摇杆 deadzone 手感 | 0x4000 阈值是否合适 | analog 方向键灵敏度 |
| joystick 打开失败的行为 | 无手柄/驱动异常 | poll_input 容错 |

### 8.7 reset 语义（睡眠前后两次 reset_input）

`reset_input`（trait 方法）清空 `PadState` 的按键状态，睡眠/唤醒路径各调用一次（`faux_sleep` 两次 PAD_reset 的 Rust 化，对应原版 api.c:1687-1694——睡眠前清一次防止"睡前的按键状态泄漏到唤醒后"，唤醒后再清一次保证干净起点）。与 C 的**有意差异**：Rust 版连 `repeat_at` 计时与摇杆原值一起清零（C 保留 `repeat_at` 跨睡眠残留）；`InputState::reset`（common 侧）只清帧快照字段，两者职责不同——`reset_input` 面向平台跨帧状态，`InputState::reset` 面向每帧输入结构。

**开发机已知限制**：Homebrew 的 sdl2-compat（SDL3 翻译层）运行时发 SDL2 传统事件值，与 sdl2-sys 预生成绑定（2.32 新值）错位——事件消费（poll_input）在 macOS 开发机不可测（sdl2 crate 转换 panic），真机/标准 SDL2 无此问题。

---

## 9. audio — 音频

音频子系统（`implement-tg5040-audio` 变更，2026-08-08 归档）采用 **SDL callback 模式**——与 C 原版 `api.c` 的 `SND_*` 系列对应。

### 9.1 SDL 回调架构（谁在播放声音）

本平台音频是**双线程**架构——播放发生在 SDL 的音频线程，而不是主线程：

```
主线程（minarch 音频引擎）                SDL 音频线程
┌─────────────────────────────┐          ┌─────────────────────────────┐
│ push_audio(frames)          │          │ AudioCallbackImpl::callback │
│   └→ Mutex<AudioRingBuffer> │◄─Arc────►│   └→ fill_output：pop 帧     │
│      环形缓冲（4000 帧）      │  共享     │      填充 out（i16 交错立体声）│
│      生产者（push）          │          │      不足 → 静音填充          │
└─────────────────────────────┘          └─────────────────────────────┘
```

- **`Arc` 是必须的**：SDL 回调闭包（`AudioCallbackImpl` 结构体，`src/lib.rs:205`）在独立音频线程执行——回调的生命周期独立于 `Tg5040` 的借用，必须经 `Arc` 共享环形缓冲的所有权（回调结构只持 `Arc<Mutex<AudioRingBuffer>>`，正好满足 sdl2 的 `AudioCallback: Send` 约束）
- **锁**：回调线程 pop + 主线程 push 经 `Mutex` 串行化——回调每 512 采样（SAMPLES）触发一次，与 push 频率相比低频，锁竞争极小
- **回调填充逻辑是纯函数**（`fill_output`，`src/lib.rs:238`）：SDL 回调本体只做"锁 + 调纯函数"——可单测，不依赖 SDL 线程

### 9.2 缓冲策略（4000 帧 / 静音填充 / 背压）

**容量 4000 帧（≈83ms@48kHz）**：原版 `buffer_seconds × sample_rate / frame_rate` = 5×48000/60——除 `frame_rate`（游戏帧率）是"游戏帧计量"残留（缓冲容量与游戏帧率无关），Rust 版固定 `AUDIO_BUFFER_FRAMES = 4000`（`src/lib.rs:198`），不随采样率/fps 变化。

**静音填充**：环形缓冲不足时输出全零（静音）——原版 `SND_audioCallback` 的部分不足处理是"倒序回放"已输出的采样（`*--in` 伪回声，api.c:977-981），非标准做法，**不复制**（否决项，见第 16 章）。

**push 背压**：`push_audio` 满时返回实际接受的帧数（0 = 满）——由调用方（minarch）决定丢弃或重试。原版 `SND_batchSamples` 缓冲满时解锁 + `SDL_Delay(1)` 重试 ×10（等待 ≈10ms）——那是 minarch 单线程时代的背压策略，Rust 版 minarch 有独立音频引擎线程，背压交还调用方更干净（trait 契约"调用方可选择丢弃或重试"）。

### 9.3 init_audio 与采样率协商

`init_audio(sample_rate) → u32`：初始化 SDL audio 子系统 + `open_playback`（`freq: Some(sample_rate)`、`channels: Some(2)`、`samples: Some(512)`——对应原版 `spec_in` 配置），**返回硬件实际采样率**。

为什么需要返回值：SDL 打开设备时可能与请求值协商出不同采样率（如 tg5040 硬件支持 44.1k/48k）——返回值作为 minarch 侧重采样器的基准（`sample_rate_out` 字段）。实际值经 `Arc<AtomicU32>` 从回调闭包回传——闭包参数 `AudioSpec` 含实际 freq，但闭包返回的是回调结构，无法直接返回值。

### 9.4 与原版差异表（对照 api.c SND_*）

| 原版（api.c SND_*） | Rust 版 | 差异理由 |
|---------------------|---------|---------|
| `SDL_OpenAudio` + `SND_audioCallback` | `open_playback` + `AudioCallbackImpl`（sdl2 crate） | Rust 安全 API |
| 重采样在 push 路径（`SND_resampleNone/Near`） | **minarch 侧**（common `Resampler`） | common 管线定稿——push 接收已重采样帧 |
| `SDL_LockAudio/UnlockAudio`（全局音频锁） | `Mutex<AudioRingBuffer>`（仅缓冲锁） | SDL2 回调线程模型——回调自身在音频线程，无需锁音频 |
| 缓冲容量 `5 × sample_rate / frame_rate` | 固定 4000 帧 | 除 fps 是游戏帧计量残留 |
| 缓冲满等待 10ms 重试 | 返回实际接受帧数（0=满） | trait 契约"调用方可重试/丢弃" |
| underrun"倒序回放"伪回声 | 静音填充 | 标准做法 |
| `SND_quit`（PauseAudio+CloseAudio+free） | drop `AudioDevice`（Option 置 None） | sdl2 drop 自动关闭 |
| SAMPLES=512、MAX_SAMPLE_RATE=48000 | `samples: Some(512)`、`freq: Some(sample_rate)` | 原版常量对应 |

### 9.5 sdl2 音频 API 科普

- **`open_playback(None, &AudioSpecDesired, |spec| callback)`**：打开播放设备——`AudioSpecDesired` 三个字段：`freq`（采样率）、`channels`（声道数）、`samples`（SDL 内部缓冲大小，2 的幂）
- **`AudioCallback` trait**：`type Channel = i16`（AUDIO_S16 格式）+ `fn callback(&mut self, out: &mut [i16])`——`out` 是交错立体声（左左右右...），每 2 个 `i16` 一帧（`AudioFrame` 对应原版 `SND_Frame`）
- **`AudioDevice::pause()/resume()`**：对应原版 `SDL_PauseAudio`——睡眠/唤醒时暂停播放（`prepare_sleep`/`complete_wake` 调用，见第 10 章）

### 9.6 真机验证清单

| 验证项 | 说明 |
|--------|------|
| 声音输出 | 经 settings 音量/静音（keymon 通道）的实际播放 |
| 采样率协商 | 模拟器 44.1k/48k → 硬件实际采样率（`init_audio` 返回值） |
| underrun 可闻性 | 静音填充在高负载下是否产生可闻断续 |
| pause/resume | 睡眠/唤醒路径的播放暂停与恢复 |

## 10. power — 电源与硬件

电源子系统（`implement-tg5040-power` 变更，2026-08-08 归档）实现 Platform trait 的电源相关方法（`power_off`/`get_battery_status`/`enable_backlight`/`set_cpu_speed`/`prepare_sleep`/`complete_wake`/`set_rumble`/`set_date_time`/`is_online`/`is_hdmi_active`）——代码在 `src/power.rs`（286 行）。

### 10.1 sysfs 路径表（硬件都在哪）

本平台的所有电源/硬件操作都是 **sysfs 节点读写**——Linux 的"一切皆文件"哲学：硬件寄存器被内核暴露为虚拟文件系统路径，读写文件即控制硬件：

| 功能 | 路径 | 读写 | 对应原版 |
|------|------|------|---------|
| 充电状态 | `/sys/class/power_supply/axp2202-usb/online` | 读（1=充电中） | platform.c:477 |
| 电量 | `/sys/class/power_supply/axp2202-battery/capacity` | 读（0-100） | platform.c:480 |
| CPU 频率 | `/sys/devices/system/cpu/cpu0/cpufreq/scaling_setspeed` | 写（Hz） | platform.c:545 |
| 振动电机 | `/sys/class/gpio/gpio227/value` | 写（1/0） | platform.c:557 |
| 网络状态 | `/sys/class/net/wlan0/operstate` | 读（"up"/"down"） | platform.c:488 |
| LED | `/sys/class/led_anim/max_scale[_lr/_f1f2]` | 写（0-60） | platform.c:508-510 |

**读写是纯函数**（`src/power.rs` 的 `read_int`/`write_int`——路径参数化可测）：

```rust
/// 读取 sysfs 整数节点。失败返回默认值（设备节点缺失/权限不足——开发机）
fn read_int(path: &str, default: i32) -> i32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(default)
}

/// 写入 sysfs 节点。失败静默（无权限/节点缺失）
fn write_int(path: &str, value: i32) {
    let _ = std::fs::write(path, value.to_string());
}
```

对应原版 `getInt`/`putInt`（utils.c）——但 Rust 版**默认值由调用方显式传**（`getInt` 失败隐式返回 0 有歧义：真实读到的 0 和读失败无法区分）。

**电量档位映射**（`battery_charge_level` 纯函数）：把原始百分比量化为 10/20/40/60/80/100 档——原版注释 "worry less about battery and more about the game you're playing"（减少 UI 电量波动）：

```rust
/// 电量档位映射（对应原版 platform.c:481-486 的 if 链）
fn battery_charge_level(raw: i32) -> u8 {
    if raw > 80 { 100 } else if raw > 60 { 80 } else if raw > 40 { 60 }
    else if raw > 20 { 40 } else if raw > 10 { 20 } else { 10 }
}
```

`get_battery_status` 返回 `BatteryStatus { charging, percentage }`（common 类型）——电量读取频率由调用方（minui/minarch 主循环）控制。

**CPU 频率表**（`set_cpu_speed`）：`Menu=600000`、`Powersave=1200000`、`Normal=1608000`、`Performance=2000000`（Hz，对应原版 platform.c:546-555，smart/brick 同表）。

### 10.2 睡眠/唤醒流程（SIGSTOP keymon + 硬件静音）

`faux_sleep`（common 层）调平台的 `prepare_sleep`/`complete_wake`：

```
准备睡眠（prepare_sleep）                    唤醒（complete_wake）
┌─────────────────────────────┐          ┌─────────────────────────────┐
│ ① pause_audio(true) 暂停播放 │          │ ① killall -CONT keymon      │
│ ② set_raw_volume(0) 硬件静音 │          │ ② enable_backlight(true)    │
│ ③ enable_backlight(false)   │          │ ③ set_volume(volume()) 恢复  │
│ ④ killall -STOP keymon      │          │ ④ pause_audio(false) 恢复播放 │
│ ⑤ sync() 文件系统落盘        │          │ ⑤ sync()                    │
└─────────────────────────────┘          └─────────────────────────────┘
      │ 等待唤醒（should_wake 200ms 轮询）
      │ 120 秒超时 → 充电中再等 60s，否则 power_off
```

**为什么 SIGSTOP/SIGCONT**：睡眠时 keymon（独立进程）仍在运行——按键可能误触发音量/亮度调节。`killall -STOP` 暂停进程（内核默认信号动作，keymon 无需任何处理代码），唤醒时 `-CONT` 继续——这是"按键在睡眠中不响应"的实现方式。对应原版 `PWR_enterSleep`/`PWR_exitSleep`（api.c:1641-1663）。

**`set_raw_volume` 与 `set_mute` 的区别**（容易混淆）：睡眠静音用 `set_raw_volume(0)`——**只写硬件**（amixer），不改 mute 状态字段（睡眠是临时静音，唤醒后恢复音量）；`set_mute(true)` 会改静音状态（持久化语义，影响 `volume()` 返回值和 keymon 的静音联动）——**睡眠路径绝不碰 mute 字段**。这也是 `settings::set_raw_volume` pub 化的原因（原版 `SetRawVolume(MUTE_VOLUME_RAW=0)`，`MUTE_VOLUME_RAW` 常量集中在 `power::MUTE_VOLUME_RAW = 0`）。

**brick 的背光细节**：`enable_backlight(true)` 恢复亮度时 brick 先写 raw 8 再恢复（对应原版 `if (is_brick) SetRawBrightness(8); SetBrightness(GetBrightness())`——brick 背光硬件特性）。

### 10.3 关机链（/tmp/minui_exec → launch.sh → shutdown）

`power_off` 不直接执行关机命令——它是关机链的第一环：

```
power_off
  ├─ unlink /tmp/minui_exec  ← ① break launch.sh 的 while 循环
  ├─ sleep(2s)               ← ② 给存档/清理留时间
  ├─ set_raw_volume(0) 静音
  ├─ enable_backlight(false)
  ├─ quit_audio()            ← ③ 礼貌清理（exit 不跑 Drop——显式关音频）
  └─ exit(0)                 ← ④ launch.sh 循环 break 后，
                                 PLATFORM/bin/shutdown 执行真正关机
```

**为什么不直接调 shutdown？** 与 C 原版一致（platform.c:527-539）：`exit(0)` 后由 launch.sh 循环 break 触发关机链——shutdown 由平台脚本处理（`PLATFORM/bin/shutdown`），Rust 进程不越权。**否决**直接调 shutdown 脚本（见第 16 章）。

**`exit(0)` 不跑 Drop**——所以清理顺序是显式的：`quit_audio()` 对应原版 `SND_quit`（音频设备跨线程——显式关闭避免回调线程残留）；SDL 窗口等由进程退出清理。

### 10.4 硬件杂项

| 方法 | 实现 | 对应原版 |
|------|------|---------|
| `set_rumble(strength)` | 写 gpio227——`(strength != 0 && !settings::mute())` 时写 1 否则 0 | platform.c:558-560（**静音时不振**） |
| `set_date_time(...)` | `Command::new("date").args(["-s", ...])` + `hwclock --utc -w` | api.c:1717-1722（system 命令——Command 免 shell 转义） |
| `is_online()` | 读 `/sys/class/net/wlan0/operstate` 前缀匹配 "up" | platform.c:488（原版在 getBatteryStatus 顺带刷新——Rust 每次读，简单） |
| `is_hdmi_active()` | 恒 `false` | tg5040 无 HDMI 输入检测（原版 GetHDMI 返回 0） |

### 10.5 与原版差异表

| 原版 | Rust 版 | 差异理由 |
|------|---------|---------|
| `system("killall -STOP keymon.elf")`（api.c:1653） | `Command::new("killall")` | **免 shell 注入**——参数直接传递（system 的字符串拼接有注入风险） |
| `system("date -s ...; hwclock ...")`（api.c:1719） | 两条 `Command` 调用 | 同上 |
| `getInt`/`putInt`（utils.c） | `read_int(path, default)`/`write_int(path, value)` 纯函数 | 显式默认值（getInt 失败隐式 0 有歧义）；路径参数化可测 |
| fb0 blank 写 0/4（platform.c:516/521 注释掉） | 不实现（亮度控制已覆盖关屏） | 原版自己都注释掉了 |
| `MUTE_VOLUME_RAW`（各平台宏） | `power::MUTE_VOLUME_RAW = 0` | 常量集中 |
| `PLAT_enableLED`（三路写） | `set_led`（smart/brick cfg 区分） | 编译期分支替代 `is_brick` 运行期判断 |
| pid 扫描（/proc 遍历）找 keymon | `killall` 命令（失败静默） | 原版就是 killall——Command 是最忠实对应 |

### 10.6 真机验证清单

| 验证项 | 说明 |
|--------|------|
| 电量读数 | axp2202 节点实际值 + 档位映射 |
| 背光切换 | 睡眠关/唤醒恢复 + LED 指示 |
| CPU 频率 | 四档切换生效（`/sys/.../scaling_cur_freq` 确认） |
| 睡眠唤醒 | SIGSTOP keymon 后按键不响应；唤醒恢复 |
| 关机链 | launch.sh 循环 break → PLATFORM/bin/shutdown 真关机 |
| 振动静音联动 | 静音时 set_rumble 抑制 |

## 11. settings 与 keymon — 系统设置

系统设置子系统（`implement-tg5040-settings-keymon` 变更，2026-08-07 归档）实现**通道 B**（系统级按键）与设置值的全链路。本章从"为什么需要两个通道"讲起，到 keymon 守护进程的实现细节。

### 11.1 双通道输入架构（keymon 的定位）

本平台的输入由**两个独立的通道**构成（第 4.1 节的完整图）：

- **通道 A（应用级）**：minui/minarch 进程内，SDL joystick 事件 → `InputState`（第 8 章）
- **通道 B（系统级）**：keymon **独立进程**，evdev 直读硬件 → 亮度/音量/静音/耳机

**为什么需要两个通道？** 系统级按键（音量/亮度）必须在**任何时刻**都能响应——游戏运行中、菜单中、甚至 minui/minarch 崩溃黑屏时。所以它们由 keymon 这个**独立进程**监控，不依赖任何上层程序的生死。而菜单轻触（250ms 内按下并释放 MENU）是**应用级**交互，在 minui 进程内通过 SDL joystick 事件完成（`tapped_menu`），**不经过 keymon 的 IPC**——keymon 监控 MENU 键只是为了区分"MENU+PLUS = 调亮度"还是"PLUS = 调音量"。

### 11.2 libmsettings 是什么（原版 C 科普）

原版 C 中，设置值（亮度/音量/静音/耳机）的读写被封装在一个独立共享库 `libmsettings.so` 里，它承担四个角色：

| 角色 | 说明 |
|------|------|
| **跨进程共享面** | keymon 改亮度/音量，minui/minarch 显示同一个值——通过 POSIX 共享内存 `shm_open("/SharedSettings")` + `mmap` 实现。注意：共享内存**本质就是一个文件**（`/dev/shm` 是 tmpfs 内存文件系统，重启即清空），mmap 只是把它映射进各进程的地址空间 |
| **硬件操作封装** | `SetRawBrightness` → `ioctl(/dev/disp)`；`SetRawVolume` → `amixer` 命令。每个平台的硬件路径不同，所以每个平台有一份独立的 msettings.c，但接口（msettings.h）统一 |
| **持久化** | 每次设置变更写 `{USERDATA_PATH}/msettings.bin` + `sync()`，开机时恢复上次的音量亮度 |
| **Host/Client 仲裁** | `shm_open(O_CREAT\|O_EXCL)` 先到先得：第一个创建成功的进程是 host（负责从磁盘加载初始值），后续进程是 client（只连接现成内存） |

**为什么需要共享内存而不是每个进程直接读写文件？** 因为设置值是**跨进程共享的状态**——keymon 调高了音量，minui 的 UI 必须显示同一个值。共享内存让所有进程看到同一份数据（零拷贝、微秒级），而文件是"断电后的记忆"（持久化）。原版是双轨：运行时用共享内存，变更时落盘。

### 11.3 Rust 的对应实现：settings 模块

libmsettings 的四个角色在 Rust 中收拢为平台 crate 的 `settings.rs` 模块（735 行，`src/settings.rs`）。

#### 11.3.1 Settings 结构体（磁盘格式兼容）

```rust
// settings.rs——字段顺序/类型与原版 msettings.c 一致
/// 共享内存中的设置数据（keymon 与 minui/minarch 通过它对话）
///
/// 字段顺序/类型与原版 C `Settings` 结构体保持一致——意义在于
/// `msettings.bin` 磁盘文件格式兼容（从原版 MinUI 升级时音量/亮度
/// 设置可保留），而非共享内存兼容。
///
/// 共享内存的读写双方（keymon 写、minui/minarch 读）都是本 crate 的
/// 代码，同一构建产物编译出两个二进制，布局一致性由编译器保证——
/// 无需 `#[repr(C)]`。
struct Settings {
    version: i32,        // SETTINGS_VERSION = 3（未来兼容）
    brightness: i32,     // 0-10
    headphones: i32,     // 耳机音量 0-20
    speaker: i32,        // 扬声器音量 0-20
    mute: i32,           // 静音开关
    unused: [i32; 2],    // 预留
    jack: i32,           // 耳机插拔状态
}
```

**为什么不需要 `#[repr(C)]`**：`#[repr(C)]` 的意义是"布局与 C 编译器生成的布局一致，供 FFI 或跨语言共享"——本项目的共享内存读写双方**都是我们自己的 Rust 代码**（keymon 和 minui/minarch 由同一份 `platform-tg5040` 源码编译，cargo 一次构建产出两个二进制），布局一致性由编译器对同一结构体定义天然保证。加 `#[repr(C)]` 反而引入"假设有其他 C 进程会读这块内存"的错误暗示。

**那为什么字段还要保持与原版 C 一致？** 因为 `msettings.bin` 是**磁盘上的持久化文件**——如果用户设备上有原版 MinUI 留下的 msettings.bin，字段顺序/类型一致时 Rust 版能直接读出旧的音量亮度设置（升级友好）。字段用 `i32`（Rust 的 4 字节有符号整数，与 C 的 `int` 在 ARM64 上同宽）即可，不需要任何 FFI 注解。

**全 Rust 的最大收益**：keymon（写）和 minui/minarch（读）共享**同一个 crate 里的同一个结构体**——布局一致性由编译器保证（同一构建产物）。C 时代"keymon 一份布局、minui 一份布局"的双份定义漂移风险直接消除。**且由于没有原厂二进制兼容需求，无需 `#[repr(C)]`——这是一次彻底的纯 Rust 重写，不遗留任何跨语言边界。**（对比：`InputEvent` 跨越 Rust/内核边界，必须 `#[repr(C)]`——见 11.5.1）

#### 11.3.2 SettingsHandle（mmap 句柄 + 方法族）

```rust
pub struct SettingsHandle {
    map: *mut Settings,        // mmap 映射指针（Drop 时 munmap，host 时 shm_unlink）
    is_host: bool,             // O_EXCL 创建者（见 Host/Client 仲裁）
    lock: Mutex<()>,           // 进程内锁（keymon 的 mute 线程与主循环并发访问）
}

impl SettingsHandle {
    pub fn init() -> SettingsHandle;
    pub fn brightness(&self) -> u8;   // 0-10
    pub fn volume(&self) -> u8;       // 0-20（mute 时返回 0，同原版 GetVolume）
    pub fn mute(&self) -> bool;
    pub fn jack(&self) -> bool;
    pub fn set_brightness(&self, value: u8);
    pub fn set_volume(&self, value: u8);
    pub fn set_mute(&self, value: bool);
    pub fn set_jack(&self, value: bool);
}
```

**为什么 setter 也用 `&self` 而非 `&mut self`？** `map` 裸指针 + 内部 `lock: Mutex<()>` 使共享引用即可安全写共享内存——这正是 mute 监控线程经 `Arc<SettingsHandle>` 与主循环共享句柄的前提（`&mut self` 无法在线程间共享）。进程内线程并发由 `lock` 隔离；跨进程并发是设计意图（字段级 4 字节对齐访问在 ARM64 上原子，同原版）。

#### 11.3.3 持久化与硬件操作

设置变更（`set_brightness`/`set_volume`/`set_jack`）同时完成三件事：**共享内存字段更新 + 硬件操作 + 持久化落盘**（对应原版 `SaveSettings`：写 `{USERDATA_PATH}/msettings.bin` + `sync()`）。

- `set_brightness`：亮度值 → raw 映射表（smart/brick 各一张，`#[cfg(feature = "brick")]` 区分，与原版 `is_brick` 分支对应）→ `libc::ioctl` 写 `/dev/disp`（`DISP_LCD_SET_BRIGHTNESS` 0x102）
- `set_volume`：音量值 × 5 → `amixer` 命令（`std::process::Command`，对应原版 `system()`）——'digital volume' 反向映射 + 'DAC volume' 开关
- `set_mute` **不落盘**——mute 状态不持久化（对应原版 `SetMute` 不调 `SaveSettings`，且 host 初始化时 `mute = 0`）

### 11.4 Host/Client 仲裁：照抄原版

Rust 版**照抄原版的先到先得逻辑**，不做额外机制：

```rust
// settings::init() 的核心逻辑（对应原版 InitSettings）
let fd = libc::shm_open(c"/SharedSettings", O_RDWR | O_CREAT | O_EXCL, 0o644);
if fd == -1 && errno() == libc::EEXIST {
    // 已存在 → 我是 client：连接现成共享内存
} else {
    // 创建成功 → 我是 host：ftruncate 尺寸 + 从 msettings.bin 加载初始值
}
```

**"keymon 必然最先启动"靠部署时序保证**：keymon 是设备开机链的组件（launch.sh 第 3 步启动），minui 要等 SD 卡挂载、launch.sh 执行后才启动——领先数秒，`O_EXCL` 竞争必然 keymon 赢。

**已知残余风险（与原版一致，透明记录）**：host 创建共享内存后需要从磁盘加载初始值，这个几微秒的窗口内如果 client 抢先连接，会读到全零内存（`version=0`），其 `set_volume(get_volume())` 可能把音量写成 0。原版靠"keymon 必然先启动"的约定从未触发此窗口，Rust 版保持同样约定，**不增加**幂等 repair 或强制 host 等额外机制。

**否决的方案**：

| 方案 | 内容 | 否决理由 |
|------|------|---------|
| 幂等 repair | version 字段作就绪标志，无效即自修 | 更健壮（任意启动顺序安全），但选择与原版对应——非必要不增加复杂度 |
| 强制 host | client 永不创建共享内存，不存在就等待重试 | 引入等待逻辑；与原厂进程（带 O_CREAT）共处时语义复杂 |

### 11.5 keymon 守护进程（通道 B 的实现）

keymon 是独立二进制（独立 crate `platform-tg5040-keymon` 的 `src/main.rs`，565 行）——launch.sh 第 3 步后台启动，60fps 主循环轮询 evdev。

#### 11.5.1 evdev：内核输入读取（`keymon/src/evdev.rs`，随 keymon 迁出平台 lib）

keymon 的数据源是 Linux 内核输入设备 `/dev/input/event0-3`。内核把按键事件写成固定布局的结构体：

```rust
#[repr(C)]                      // ★ 必须——数据源是内核（跨 Rust/内核边界）
struct InputEvent {
    sec: i64,        // timeval.tv_sec
    usec: i64,       // timeval.tv_usec
    kind: u16,       // type（EV_KEY / EV_SW）
    code: u16,
    value: i32,      // 0=RELEASED 1=PRESSED 2=REPEAT
}
```

**为什么 `InputEvent` 有 `#[repr(C)]` 而 `Settings` 没有？** 对比见下：

| | 读写双方 | `#[repr(C)]]` |
|---|---|---|
| `Settings` | 都是本 crate 的 Rust 代码（同一构建产物） | ❌ 不需要 |
| `InputEvent` | Rust 读 / 内核写（`libc::read` 从设备文件读取） | ✅ 必须有（布局与 Linux 内核 `struct input_event` 一致） |

设备打开细节：`open(O_RDONLY | O_NONBLOCK | O_CLOEXEC)`——**单设备打开失败不 panic**（fd 无效时跳过该设备继续，对应原版 `open` 失败返回 -1 的行为）；`O_NONBLOCK` 保证无事件时 `read` 立即返回（对应原版 keymon.c）。`libc::read` 的 unsafe 集中于此模块，带 `# Safety` 章节（前置条件：缓冲区为 `InputEvent` 布局、fd 有效）。

#### 11.5.2 主循环（60fps）

```rust
// keymon/src/main.rs——对应原版 keymon.c 的 main
const CODE_MENU0: u16 = 314;   // MENU 键三路冗余（物理键 + 备用 scancode）
const CODE_MENU1: u16 = 315;
const CODE_MENU2: u16 = 316;
const CODE_PLUS: u16 = 115;    // 音量加
const CODE_MINUS: u16 = 114;   // 音量减
```

主循环每帧（`thread::sleep(16ms)`，原版 `usleep(16666)`）：

```
1. 读四个 evdev 设备 → 按键事件流
2. menu_pressed 跟踪 MENU 按下状态（区分"MENU+PLUS=调亮度"还是"PLUS=调音量"）
3. 音量/亮度调节（对应原版循环逻辑）：
   - MENU 按下时 PLUS/MINUS → 亮度 brightness ± 1（clamp 0-10）
   - MENU 未按下时 PLUS/MINUS → 音量 volume ± 1（clamp 0-20）
   - 长按重复：首次 300ms，之后每 100ms 重复一次
4. EV_SW 开关事件：JACK(2) → set_jack，MUTE(1) → set_mute
5. 睡眠输入忽略：循环间隔 > 1000ms（被 SIGSTOP 挂起过）时，
   期间到达的按键事件全部丢弃并清空按键状态（对应原版 ignore 标志）
```

按键码（314/315/316/115/114）是 **evdev scancode**，与通道 A 的 SDL `JOY_*` 值（128/129 等）完全不同——两条通道各自映射，互不干扰。

#### 11.5.3 SIGTERM 优雅退出

keymon 注册 `SIGTERM` 处理器（`libc::sigaction`），设置原子退出标志（`AtomicBool`，对应原版 `volatile int quit`）。主循环每帧检查标志，置位时：关闭设备 fd、`SettingsHandle` drop（`munmap`，host 时 `shm_unlink`）、进程退出。

**SIGSTOP/SIGCONT 无需处理代码**——内核默认信号动作即停止/继续进程（原版 `killall -STOP/-CONT keymon.elf` 直接生效），发送方是 `prepare_sleep`/`complete_wake`（第 10.2 节）。

#### 11.5.4 mute 监控线程

keymon 启动 mute 监控线程（`std::thread`，对应原版 `watchMute` pthread）：每 200ms 轮询 `/sys/class/gpio/gpio243/value`（`std::fs` 读取），值变化时调用 `set_mute`（经 `SettingsHandle` 的 `Mutex` 隔离进程内并发）——值未变化不重复设置。这就是"静音硬件开关"→ 设置状态 → 所有进程共享的完整链路。

### 11.6 与 Platform trait 的边界（为什么亮度/音量不在 trait 里）

**这是 C 的领域划分，Rust 忠实继承**：

| C 领域 | 内容 | Rust 对应 |
|--------|------|-----------|
| `api.c` / `PLAT_*` | 平台抽象（视频/输入/音频/电源） | `Platform` trait（27 方法 + 24 关联常量） |
| `libmsettings.so` | 系统设置（亮度/音量/静音/耳机） | 平台 crate 的 `settings` 模块 |

亮度/音量**不属于平台抽象**，而是"系统设置"领域——所以 `Platform` trait 不承载设置读写，settings 是 trait 之外的**第二个接口面**。minui/minarch 读取设置值时，在 `#[cfg(feature = "platform-tg5040")]` 下直接调用平台 crate 的 settings 函数——这与原版 minui.c 链接 libmsettings 直接调函数的行为完全一致。

接口面收敛为两类（pub 面由 Rust 可见性强制，无运行时检查）：

- `settings::SettingsHandle`——跨进程系统设置（本节所述）
- `Tg5040` 类型本身——仅装配层实例化（`Tg5040::new()`）

设备语义键（`BTN_SLEEP`/`BTN_MOD_*`）已归 `Platform` trait 关联常量；
硬件实现模块（power/input）的函数一律 `pub(crate)`——上层 crate 只能
引用上述两类接口（早期曾由 xtask lint 维护引用白名单，平台自治后删除，
改由 `pub(crate)` 可见性在编译期强制）。

```rust
// minui 显示音量条时（cfg 分支选择平台，与平台初始化代码同一模式）
#[cfg(feature = "platform-tg5040")]
let volume = platform_tg5040::settings::SettingsHandle::init().volume();
```

**power::update 的调用方参数**：原版 `PWR_update` 内部直接摸全局（`GetMute()`/`PAD_*`/`GetHDMI()`）。Rust 的 `power::update` 在 common crate——common 不能依赖平台 crate（依赖方向：platform → common），所以全部输入由**调用方每帧收集传入**（详见 common 文档「装配层职责」）。重构后的契约为 10 参数 + `PowerAction` 返回值：

```rust
// common::power（restructure-power-update-contract 后的契约）
pub fn update(
    state: &mut PowerState,
    input: &InputState,
    battery: &BatteryStatus,
    mute: bool,                    // 调用方从 settings 读取
    mod_keys: ModKeys,             // 平台语义键常量组装（BTN_MOD_*，trait 关联常量）
    now_ms: u32,
    sleep_btn: u32,                // 平台 BTN_SLEEP（trait 关联常量，本平台 = BTN_POWER）
    hdmi_active: bool,             // 调用方从 is_hdmi_active() 读取（本平台恒 false）
    before_sleep: Option<fn()>,
    after_sleep: Option<fn()>,
) -> (Option<PowerAction>, bool, u8);   // 动作信号：update 只检测不执行，调用方执行动作序列
```

### 11.7 睡眠/唤醒的 keymon 交互

原版在睡眠时挂起 keymon（睡眠中按键不响应），唤醒时恢复（api.c:1653/1658）：

| 事件 | 原版 C | Rust 对应 |
|------|--------|-----------|
| 进入睡眠 | `killall -STOP keymon.elf` + `SetRawVolume(0)` 静音 + 关背光 | `prepare_sleep`：暂停音频 + 硬件静音（不改 mute 字段）+ 关背光 + `killall -STOP keymon` + sync |
| 唤醒 | `killall -CONT keymon.elf` + `SetVolume(GetVolume())` 恢复 | `complete_wake`：`killall -CONT keymon` + 开背光 + 音量恢复 + 恢复音频 + sync |
| 关机 | `SetRawVolume(MUTE_VOLUME_RAW)` | `power_off`：静音（关机由系统 shutdown 处理） |

发送方（`prepare_sleep`/`complete_wake`）在电源变更中实现（第 10.2 节）——keymon 自身无需处理 SIGSTOP/SIGCONT（内核默认信号动作即停止/继续进程）。

### 11.8 多平台复用模式

原版 12 个平台的 keymon + libmsettings **骨架完全统一**，差异只有两处：

| 差异点 | 说明 |
|--------|------|
| keymon 按键码 | 每台设备的 MENU/PLUS/MINUS 的 evdev code 不同 |
| settings 硬件路径 | 亮度 ioctl 寄存器、音量 amixer 通道名不同 |

Rust 版继承这个模式：**settings 模块 + keymon bin 的骨架在平台 crate 内固化**，未来新平台只需改按键码常量表和硬件操作函数——这也是本次为 tg5040 设计时特意保持"与原版一致"的原因，降低后续 11 个平台移植时的认知负担。

## 12. 设备区分机制：运行时 vs 编译期

本平台最核心的架构决策：**用编译期 feature 区分 smart/brick，而非像 C 原版那样用运行时环境变量**。这是对原 C 实现的一个根本性偏离——本章节完整解释：C 是怎么做的、为什么 C 能做到一个二进制、Rust 为什么不能照搬、我们怎么解决的、影响到了什么。

### 12.1 C 原版的运行时检测完整流程

#### 12.1.1 整条链路

原版 C 的 tg5040 代码在**运行时**判断设备，完整链路如下：

```
┌─────────────────────────────────────────────────────────────────────┐
│ 1. 硬件上电（安装/更新时）                                            │
│                                                                     │
│ 2. install/boot.sh 检测设备（安装时执行一次）                          │
│    TRIMUI_MODEL=`strings /usr/trimui/bin/MainUI | grep ^Trimui`     │
│    if [ "$TRIMUI_MODEL" = "Trimui Brick" ]; then                     │
│        DEVICE="brick"        ← shell 环境变量                        │
│    fi                                                                │
│    # smart 设备：DEVICE 未设置（空）→ 默认 smart                      │
│                                                                     │
│ 3. boot.sh 启动 minui/minarch                                        │
│    exec "$SYSTEM_PATH/.../MinUI.pak/launch.sh"                       │
│    → 子进程继承 DEVICE 环境变量                                        │
│                                                                     │
│ 4. C 代码运行时读取                                                 │
│    char* device = getenv("DEVICE");    ← platform.c:59              │
│    is_brick = exactMatch("brick", device);                          │
│    # smart: device=NULL → 0 → is_brick=0                            │
│    # brick: device="brick" → 1 → is_brick=1                         │
│                                                                     │
│ 5. is_brick 全局变量驱动 12 处设备差异                                │
└─────────────────────────────────────────────────────────────────────┘
```

#### 12.1.2 `getenv` 机制

`getenv("DEVICE")` 是 C 标准库函数——读取**当前进程的环境变量块**：

- 进程启动时，操作系统把环境变量（键值对数组）放入进程内存
- libc 维护 `environ` 全局指针指向这个数组
- `getenv` 线性查找键名，返回值指针（找不到返回 `NULL`）

关键点：**环境变量在进程启动时由 boot.sh 通过 `exec` 传入**——`exec` 替换当前进程映像但保留环境变量块，所以 `launch.sh` → `minui.elf` 一路继承 `DEVICE=brick`。

#### 12.1.3 `is_brick` 的 12 处使用地图

| # | 位置 | 用途 |
|---|------|------|
| 1-3 | `platform.h:90-91` | `JOY_L3/JOY_R3 (is_brick?9:NA) / (is_brick?10:NA)`——按键映射 |
| 4-5 | `platform.h:95-96` | `JOY_PLUS (is_brick?14:128)`、`JOY_MINUS (is_brick?13:129)`——按键映射 |
| 6 | `platform.h:120` | `FIXED_SCALE (is_brick?3:2)`——缩放倍率 |
| 7 | `platform.h:121` | `FIXED_WIDTH (is_brick?1024:1280)`——物理宽 |
| 8 | `platform.h:122` | `FIXED_HEIGHT (is_brick?768:720)`——物理高 |
| 9 | `platform.h:130` | `MAIN_ROW_COUNT (is_brick?7:8)`——UI 布局 |
| 10 | `platform.h:131` | `PADDING (is_brick?5:40)`——UI 布局 |
| 11 | `platform.c:503-504` | `PLAT_enableLED` 中 `if (is_brick)` 写额外 2 个 LED 路径 |
| 12 | `msettings.c:133` | `SetBrightness` 中 `if (is_brick)` 用不同的亮度映射表 |

### 12.2 为什么 C 能做到一个二进制

#### 12.2.1 宏展开的真相：替换的是"表达式"，不是"值"

```c
// platform.h —— 预处理器指令
#define FIXED_WIDTH (is_brick?1024:1280)

// 编译器在编译时把源码中的 FIXED_WIDTH 文本替换为 (is_brick?1024:1280)
// 但 is_brick 是运行时全局变量（platform.c 里 int is_brick = 0;）
// → 这个三元表达式的值在运行时才确定！
```

**关键辨析**：

```
#define FIXED_WIDTH (is_brick?1024:1280)
     ↑                              ↑
  编译时：标识符 → 表达式文本         运行时：is_brick 的值 → 三元结果
```

预处理器（编译第一阶段）只做**文本替换**——把 `FIXED_WIDTH` 替换成 `(is_brick?1024:1280)` 这串字符。`is_brick` 是普通全局变量，它的赋值（`is_brick = exactMatch("brick", device)`）在 `PLAT_initVideo` 运行时执行。所以宏展开后代码里的 `(is_brick?1024:1280)` 是**运行时三元表达式**——同一个二进制在不同设备上算出不同值。

#### 12.2.2 上层代码的运行时读取

```c
// minarch.c —— 游戏画面缩放目标尺寸
DEVICE_WIDTH = screen->w;   // screen 是 SDL_Surface*，w 字段是运行时值
DEVICE_HEIGHT = screen->h;
```

上层从不使用编译期常量做尺寸计算——全部通过 `gfx.screen->w` 等**运行时读取**。这是 C 单二进制的第二根支柱：没有任何编译期尺寸假设。

### 12.3 Rust 为什么不能照搬

#### 12.3.1 Platform trait 关联常量是编译期值

```rust
// common/src/platform.rs —— 归档的 trait 设计
pub trait Platform {
    const SCREEN_WIDTH: u32;   // 关联常量——值在编译期确定
    const SCREEN_HEIGHT: u32;
    const SCALE: u32;
    // ...
}
```

Rust 的关联常量（associated const）是**编译期值**——不可能在 `new()` 或 `init_video()` 时改变。上层代码用它做：

```rust
// minui/minarch 的装配层代码
let mut screen = VideoBuffer::new(P::SCREEN_WIDTH, P::SCREEN_HEIGHT);
// VideoBuffer 的 pixels 是 Vec<u16>——尺寸在编译期确定
```

如果照搬 C 的运行时检测，`SCREEN_WIDTH` 需要运行时可变——但 trait 定义为常量，改 trait 意味着推翻已归档的 `define-platform-trait` 设计（影响 common/render/minui/minarch 全部引用）。

#### 12.3.2 代码对比

```c
// C：运行时确定（一个二进制两台设备）
FIXED_WIDTH (is_brick?1024:1280)   // 运行时三元
int w = gfx.screen->w;              // 运行时读取
```

```rust
// Rust：编译期确定（feature 选设备），画布 = 物理分辨率
#[cfg(feature = "brick")]
const SCREEN_WIDTH: u32 = 1024;     // 编译期常量（物理宽）
let w = P::SCREEN_WIDTH;            // 编译期已知
```

### 12.4 Rust 的解决方案：feature 编译期区分

#### 12.4.1 定义

```toml
# platforms/tg5040/Cargo.toml
[features]
# 设备 feature 互斥且必选、无默认——src/lib.rs 顶部的 compile_error! 断言
# 强制"恰好启用一个"（未指定或同时指定两个都编译报错，无静默回落）。
smart = []
brick = []
```

#### 12.4.2 使用

```rust
// 编译期条件编译——smart 编译时 brick 分支的代码根本不存在。
// 条件全部正向：smart 分支用 #[cfg(feature = "smart")]、brick 分支用
// #[cfg(feature = "brick")]——不使用 #[cfg(not(...))] 负向逻辑
// 条件全部正向（无 not(...) 负向逻辑——见第 12.8 节）。
// 常量为物理分辨率（画布 = 物理屏，flip 1:1）。
#[cfg(feature = "brick")]
const SCREEN_WIDTH: u32 = 1024;
#[cfg(feature = "smart")]
const SCREEN_WIDTH: u32 = 1280;
```

#### 12.4.3 编译流程

```
cargo build -p minui --features platform-tg5040/smart    cargo build -p minui --features platform-tg5040/brick
            │                                         │
            ▼                                         ▼
     smart 二进制（1280×720）                 brick 二进制（1024×768）
            │                                         │
            ▼                                         ▼
     smart 独立安装包                          brick 独立安装包
```

#### 12.4.4 对比表

| 维度 | C 原版 | Rust 版 |
|------|--------|---------|
| 设备区分 | 运行时 `getenv` + `is_brick` | 编译期 feature + `#[cfg]` |
| 二进制 | 1 个服务两台 | 2 个（每设备一份） |
| 发行 | 1 个 MinUI.zip | 每设备独立包 |
| 正确性保证 | 无（运行时空分支） | 编译器保证（smart 二进制不可能执行 brick 分支） |
| 代码体积 | 含全部设备分支 | 只含本设备分支（死代码消除） |

### 12.5 本次改变影响的内容

#### 12.5.1 分辨率

```rust
// 画布 = 物理分辨率（fix-tg5040-resolution-design 修正）
// SCALE 语义 = 图集倍率 + 布局倍率，与 flip 无关（flip 恒 1:1）
#[cfg(feature = "brick")]
const SCREEN_WIDTH: u32 = 1024;    // 物理屏宽度
#[cfg(feature = "brick")]
const SCREEN_HEIGHT: u32 = 768;    // 物理屏高度（4:3）
#[cfg(feature = "brick")]
const SCALE: u32 = 3;              // @3x 图集 + 布局倍率

#[cfg(not(feature = "brick"))]
const SCREEN_WIDTH: u32 = 1280;    // 物理屏宽度
#[cfg(not(feature = "brick"))]
const SCREEN_HEIGHT: u32 = 720;    // 物理屏高度（16:9）
#[cfg(not(feature = "brick"))]
const SCALE: u32 = 2;              // @2x 图集 + 布局倍率
```

**逻辑**：SCREEN_WIDTH/HEIGHT 直接等于物理屏分辨率（非推导值）——flip 恒 1:1 无缩放。SCALE 是图集倍率 + 布局倍率（详见第 2.4 节 SCALE 语义解读）。C 的 `FIXED_WIDTH (is_brick?1024:1280)` 宏被拆成两组编译期常量，且 Rust 的 SCREEN 就是 C 的 FIXED_WIDTH 本身。

**历史修正说明**：早期（implement-tg5040-structure）曾用逻辑分辨率 640×360/341×256（意图 flip GPU 放大）——数学不自洽（物理元素 = 布局×SCALE×flip = C 的 2 倍大），且 @1x 图集放大方案损害画面精美（已否决）。修正为画布=物理分辨率后：smart 元素 30×2×1=60px ✅ 与 C 一致；brick 30×3×1=90px ✅ 与 C 的 FIXED_SCALE=3 一致；341 取整问题消失（画布=1024 物理宽，无推导）。

#### 12.5.2 布局

```c
// C：每设备不同的布局常量（运行时三元）
#define MAIN_ROW_COUNT (is_brick?7:8)   // 主界面行数
#define PADDING (is_brick?5:40)          // 页面留白
```

Rust 版布局常量在 `common::video` 单一来源（`PILL_SIZE` 等跨平台共享）；**平台差异的布局值经 `Platform` trait 方法覆盖表达**（`main_row_count()`/`padding()`，本平台实现：smart=8/40、brick=7/5，对应 C 宏 `MAIN_ROW_COUNT (is_brick?7:8)`/`PADDING (is_brick?5:40)`）：

```rust
// src/lib.rs（真实代码）——布局查询方法（trait 默认实现由平台覆盖）
fn main_row_count(&self) -> u32 {
    #[cfg(feature = "brick")] { 7 }
    #[cfg(feature = "smart")] { 8 }
}
fn padding(&self) -> u32 {
    #[cfg(feature = "brick")] { 5 }
    #[cfg(feature = "smart")] { 40 }
}
```

为什么是**方法**而非关联常量？平台差异值若都能编译期固定，常量就够了；但既有平台（my355/rg35xxplus 等）存在 `on_hdmi` 运行时分支（HDMI 插拔时行数 6↔8 切换，platform.c:447）——**运行时状态只能方法表达**。统一用方法（而非"常量为主、方法为辅"）避免同一概念两套形态。

#### 12.5.3 按键映射（已实现——真实代码见第 8.2 节）

```c
// C：brick 有 L3/R3，PLUS/MINUS 键位不同
#define JOY_L3   (is_brick?9:JOY_NA)
#define JOY_R3   (is_brick?10:JOY_NA)
#define JOY_PLUS (is_brick?14:128)
#define JOY_MINUS(is_brick?13:129)
```

Rust 版输入映射已在 `src/input.rs` 实现，用 `#[cfg(feature = "brick")]` 区分：

```rust
// src/input.rs——完整映射表见第 8.2 节
#[cfg(feature = "brick")]
pub const JOY_PLUS: u8 = 14;
#[cfg(not(feature = "brick"))]
pub const JOY_PLUS: u8 = 128;
```

#### 12.5.4 灯光（LED，已实现——见第 10 章）

```c
// C：brick 有额外两个 LED 路径
static void PLAT_enableLED(int enable) {
    if (enable) {
        putInt(LED_PATH1, 60);                       // 两台都有
        if (is_brick) putInt(LED_PATH2, 60);         // brick 特有
        if (is_brick) putInt(LED_PATH3, 60);         // brick 特有
    } else { /* 对称的关闭逻辑 */ }
}
```

Rust 版在 `src/power.rs`（`set_led`）用 `#[cfg]` 区分：

```rust
// src/power.rs——真实代码（简化）
#[cfg(feature = "brick")]
fn set_led(&self, enable: bool) {
    write_int(LED_PATH1, 60);   // 两台都有
    write_int(LED_PATH2, 60);   // brick 特有
    write_int(LED_PATH3, 60);   // brick 特有
}
#[cfg(not(feature = "brick"))]
fn set_led(&self, enable: bool) {
    write_int(LED_PATH1, 60);   // smart 只有第一个
}
```

#### 12.5.5 device_model

```c
// C：运行时读环境变量
char* PLAT_getModel(void) {
    char* model = getenv("TRIMUI_MODEL");
    if (model) return model;
    return "Trimui Smart Pro";
}
```

Rust 版编译期确定，不读环境变量：

```rust
// src/lib.rs——真实代码
// 关联常量形态（编译期固定值 → 常量，trait 分层约定）
#[cfg(feature = "smart")]
const DEVICE_MODEL: &str = "TrimUI Smart Pro";
#[cfg(feature = "brick")]
const DEVICE_MODEL: &str = "TrimUI Brick";
```

### 12.6 发行模型变化

| 维度 | C 原版 | Rust 版 |
|------|--------|---------|
| 安装包 | 1 个 MinUI.zip（smart/brick 通用） | 每设备独立包（下载对应设备固件） |
| 安装时检测 | boot.sh 用 `strings` 检测设备 → 设 DEVICE | 无检测（包就是本设备的） |
| 运行时判断 | `getenv` + `is_brick` | 无（编译期已固定） |

**为什么可以接受这个变化？** 用户视角：smart 和 brick 外观差异大（屏幕比例、按键布局不同），用户购买时就知道自己买的是哪台——它们本质上是两台不同的机器。用户不会把 SD 卡从 smart 拔下来插到 brick（SD 卡内的系统是针对设备编译的）。C 的"单二进制 + SD 卡互换"价值在此场景不存在。

**boot.sh 的对应修改**（`implement-tg5040-structure` 变更，细节见第 13 章）：

```
移除前（3 处依赖 DEVICE）:
  ① strings 检测型号 + DEVICE="brick" 写入
  ② if [ "$DEVICE" = "brick" ]; then 关额外 LED; fi
  ③ ./show.elf ./$DEVICE/$ACTION.png    ← 按设备选图

移除后:
  ① 无检测（每设备独立包）
  ② 无条件关 3 个 LED 路径（设备特有的路径不存在时 echo 失败无害）
  ③ ./show ./$ACTION.png            ← 图与 show 同在包根目录 tg5040/（平台子 xtask 装配放入）
```

**安装图的打包约束**：`./$ACTION.png` 要求安装图在**包根目录**。C 原版的图按设备分目录（smart 图在根目录、brick 图在 `brick/` 子目录）——Rust 版打包时（平台子 xtask 装配）把对应设备的图复制到包根目录。**禁止**用 `if [ -d ./brick ]` 目录判断选图——C 打包是全复制，smart 包内也有 brick/ 子目录，目录判断会误判。

### 12.7 打包架构（平台子 xtask 全流程自治）

#### 12.7.1 分工

| 负责方 | 职责 | 对应 C 原版 |
|--------|------|------------|
| xtask（父） | 通用二进制编译（minui/minarch/clock/minput）+ 通用二进制检测/复制 + 全局步骤 + 调平台子 xtask | 顶层 makefile 的 `build`/`system`/`package` |
| `platforms/<platform>/xtask`（平台子 xtask，包名 `<platform>-xtask`） | ① 编译 show/keymon（`cargo build -p platform-<p>-show|-keymon --features <device>`）② 编译 libretro.so（`cores/` 的 make，PLATFORM 经 `UNION_PLATFORM=tg5040` 环境变量传入）③ 复制 show/keymon/.so/其余资源到 build/ ④ 复制 install/ | `platform/makefile.copy` + 平台各 make（show/keymon/cores） |

#### 12.7.2 平台子 xtask 的调用契约与流程

父 xtask 的 platform 步骤执行 `cargo run --quiet -p <platform>-xtask -- <device>`（命名约定：包名 `<platform>-xtask`）。build 目录与 target 由平台子 xtask 依约定自定位（不传参）：build 目录为 workspace 根的 `build/`，产物 target 固定 `aarch64-unknown-linux-gnu`。全流程（`platforms/tg5040/xtask/src/main.rs`）：

```
tg5040-xtask <smart|brick>（编译步骤经工具链容器执行——`podman run ... minui-toolchain`）:
  ① 编译 show/keymon   podman run ... cargo build -p platform-tg5040-show --features <device> --target aarch64... --release
                        podman run ... cargo build -p platform-tg5040-keymon --features <device> --target aarch64... --release
  ② 编译 libretro.so   podman run ... bash -c "cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make"
                        （全流程必跑，克隆/编译进度透传；容器设 CROSS_COMPILE=/usr/bin/aarch64-linux-gnu-；
                          PLATFORM 走环境变量而非命令行——命令行会经 MAKEOVERRIDES 泄漏给子 make，
                          覆盖 Makefile.libretro 内部 PLATFORM = libretro，对齐 C 原版 setup-env.sh）
  ③ 复制产物           show → SYSTEM/tg5040/bin/ 与 BOOT/common/tg5040/
                        keymon → SYSTEM/tg5040/bin/
                        stock .so（6 个）→ SYSTEM/tg5040/cores/
                        extras .so（按映射表，一核多 pak）→ EXTRAS/Emus/tg5040/<pak>/
  ④ 复制 install/      boot.sh → BOOT/common/tg5040.sh
                        update.sh → SYSTEM/tg5040/bin/install.sh
                        安装图按 device 选源 → BOOT/common/tg5040/
```

#### 12.7.3 复制目标目录说明

```
tg5040-xtask smart 执行后的目标结构（build 目录由父 xtask setup 建立）:

<build>/                          ← 父 xtask 已复制 skeleton 到此
├── BOOT/
│   ├── common/
│   │   ├── tg5040.sh             ← install/boot.sh（bootloader 调用，成为 .tmp_update/tg5040.sh）
│   │   └── tg5040/               ← 安装图目录 + show（boot.sh 的 ./show ./$ACTION.png 在此读取）
│   │       ├── show              ← 编译产物（show 与图同目录，boot.sh CWD 相对调用）
│   │       ├── installing.png    ← smart: install/installing.png
│   │       └── updating.png      ←       install/updating.png
│   └── trimui/app/               ← skeleton 原有，不动
├── SYSTEM/
│   └── tg5040/
│       ├── bin/
│       │   ├── install.sh        ← install/update.sh（系统更新脚本）
│       │   ├── show              ← 编译产物
│       │   └── keymon            ← 编译产物
│       └── cores/                ← 6 个 stock _libretro.so
└── EXTRAS/Emus/tg5040/           ← extras 核心按映射表复制（P8/MGBA/SGB/PCE/PKM/NGP/NGPC/SUPA/VB.pak）

brick 时：安装图来自 install/brick/ 子目录（brick 的图被"提升"到包根目录，
boot.sh 无需知道）；show/keymon 以 --features brick 编译（show 分辨率 1024×768）。
```

#### 12.7.4 device 参数如何决定安装图与 show 分辨率

```
父 xtask platform 步骤透传 device:
  cargo run --quiet -p tg5040-xtask -- smart
  cargo run --quiet -p tg5040-xtask -- brick

平台子 xtask 收到 device 后:
  smart → show 以 --features smart 编译（1280×720）+ 安装图来自 install/*.png（根目录）
  brick → show 以 --features brick 编译（1024×768）+ 安装图来自 install/brick/*.png
```

**为什么图要按参数选择而非固定一份？** 两台设备的安装画面图片不同（brick 有专门设计的图）——运行时不再检测设备，图的选择必须在打包时完成。**show 也必须按 device 编译**——它的分辨率经 `Tg5040::SCREEN_WIDTH/HEIGHT` 取编译期值（smart 1280×720 / brick 1024×768），`compile_error!` 强制显式选一个设备 feature。

#### 12.7.5 与 C makefile.copy 的对应

```makefile
# C 原版 makefile.copy（tg5040）——平台子 xtask 装配逻辑的前身
$(PLATFORM):
	cp ./workspace/$@/install/boot.sh ./build/BOOT/common/$@.sh
	cp ./workspace/$@/install/update.sh ./build/SYSTEM/$@/bin/install.sh
	cp ./workspace/$@/install/*.png ./build/BOOT/common/$@/        ← 全复制（含 brick/ 子目录）
	cp -r ./workspace/$@/install/brick ./build/BOOT/common/$@/
	cp ./workspace/$@/show/show.elf ./build/SYSTEM/$@/bin/          ← show 复制两处
	cp ./workspace/$@/show/show.elf ./build/BOOT/common/$@/
	# ...（unzip、DinguxCommander 等平台资源）
```

Rust 版平台子 xtask 的改进（相对 sh 脚本）：
- **编译纳入**：show/keymon 的编译（原 C 各自 make）+ 装配（原 makefile.copy）统一在平台子 xtask，平台自治完整
- **可测试**：装配逻辑是 Rust 纯函数（`copy_autonomous_bins`/`copy_stock_cores`/`copy_extras_cores`/`copy_install` 等），`cargo test -p tg5040-xtask` 覆盖落点
- **cores 复制表数据化**：`EXTRAS_CORES` 常量数组表达 `.so → pak` 映射（含一核多 pak），对照 C 原版 `MinUI/makefile` 的 `cores` 目标

#### 12.7.6 父 xtask 调用流程

```
cargo xtask toolchain all --platform tg5040 --device smart:
  setup    （清空 build/ + 复制 skeleton + 删 .keep/*.meta + 写 build/hash.txt）
  build     cargo build -p minui --features platform-tg5040/smart --target aarch64-unknown-linux-gnu --release
            cargo build -p minarch --features platform-tg5040/smart --target aarch64-unknown-linux-gnu --release
            cargo build -p clock/minput ...（通用 4 个；platform lib 作为依赖连带编译）
  system   （检测 4 个通用二进制完整性 + 复制 → build/SYSTEM/tg5040/bin/）
  platform  cargo run --quiet -p tg5040-xtask -- smart（平台子 xtask 全流程）
  special   （BOOT/common → .tmp_update、BOOT/trimui → BASE/ 等）
  tidy      （SYSTEM/tg5040/bin/install.sh → SYSTEM/tg3040/paks/MinUI.pak/launch.sh）
  package   （version.txt/commits.txt + MinUI.zip + base/extras zip）
```

二进制命名：产物无 `.elf` 后缀（`minui`/`minarch`/`clock`/`minput`/`show`/`keymon`），skeleton 的 launch.sh 引用同名（Rust 版统一命名约定，与 C 原版 `.elf` 分叉）。

### 12.8 为什么条件可以写成正向 `feature(smart)`？

全平台代码使用 `#[cfg(feature = "smart")]` / `#[cfg(feature = "brick")]` **正向**条件，**没有** `#[cfg(not(...))]` 负向逻辑。早期版本曾用 `not(brick)`——因为 Cargo feature 是**叠加的**（默认 smart + 显式 brick 会同时启用），负向条件在叠加场景下恰好规避了重复定义。但负向逻辑有两大缺陷：

1. **不可读**：smart 分支散落在否定语气下（"非 brick 即 smart"的隐含假设）
2. **不可扩展**：未来出现第三设备变体时，`not(brick)` 无法表达"既非 brick 也非新设备"

新方案用 `compile_error!` 断言强制"恰好启用一个设备 feature"，从根上解决叠加问题：

```toml
[features]
default = []      # 无默认——不叠加任何设备 feature
smart = []
brick = []
```

`src/lib.rs` 顶部两条断言：

- 同时启用 smart + brick → 编译错误（互斥断言）
- 一个都没启用 → 编译错误（必选断言）

在断言保证"恰好一个"的前提下，正向条件不再有重复定义风险：

| 场景 | 行为 |
|------|------|
| `--features smart` | 恰好 smart → 只编译 smart 分支 ✅ |
| `--features brick` | 恰好 brick → 只编译 brick 分支 ✅ |
| 无 feature | 编译错误（必选断言）——设备选择必须显式 |
| `--features smart,brick` | 编译错误（互斥断言） |

对比旧方案：`not(brick)` 在"无 feature"时静默回落 smart（隐含默认），新方案用**编译错误取代静默回落**——这是刻意行为，设备选择必须显式（平台 feature 系统规范，见架构文档「平台 feature 系统」）。

### 12.9 新增设备的移植检查清单

本平台未来加入新设备（如 TrimUI Brick Pro——2026-07 发布，双霍尔摇杆 + 可点击 L3/R3，同 A133P 芯片 + 同 1024×768 分辨率，大概率复用 brick feature）时，按差异点逐项检查：

| # | 差异点 | 检查内容 | 对应章节 |
|---|--------|---------|---------|
| 1 | Cargo feature | 复用现有 feature 还是新增？（同屏同芯片 → 复用；新分辨率/新芯片 → 新增） | 12.4 |
| 2 | 关联常量 | `SCREEN_WIDTH/HEIGHT/SCALE` + 共用常量 + **8 个按键能力常量 `HAS_L2/R2/L3/R3/LS/RS/VOLUME/MENU`**（默认 false，按设备填） | 2 章 |
| 3 | 输入映射 | `JOY_*`/`CODE_*`/`AXIS_*` + 设备语义键 `BTN_SLEEP`/`BTN_MOD_*`（trait 关联常量） | 8.2 |
| 4 | 亮度 raw 映射表 | settings 的 `raw_brightness` 映射（每设备一张，`#[cfg]` 区分） | 11.3.3 |
| 5 | LED 路径 | `set_led` 的 sysfs 路径差异 | 10.4 |
| 6 | 布局常量 | **先检查 `common::video` 是否已有定义**——config.yaml 规定：禁止在业务 crate 重复定义布局常量 | 12.5.2 |
| 7 | 打包 | show/安装图是否按 device 区分（平台子 xtask 编译/装配） | 13 章 |
| 8 | 真机验证 | 新增设备跑一遍各子系统「真机验证清单」（第 7.7/8.6/9.6/10.6 节） | 各章 |

**两条铁律**（移植时不可违反）：

1. **设备差异编译期处理，禁止运行时判断**——不允许 `is_brick` 字段、不允许 `std::env::var("DEVICE")`、不允许运行时 `if` 分支区分设备
2. **先对照原版 C 实现**——config.yaml：做一切修改或新增前，先检查原 C 相应模块的实现逻辑和依赖关系，非必要则保持一致

### 12.10 小结：每节一句话

| 节 | 一句话 |
|----|--------|
| 12.1 | C 原版在运行时用 `getenv` + `is_brick` 判断设备（boot.sh 检测链 → 环境变量 → 12 处差异） |
| 12.2 | C 能做到单二进制是因为宏只替换"表达式文本"——值在运行时才求值 |
| 12.3 | Rust 不能照搬：Platform trait 关联常量是编译期值，trait 已归档不可改 |
| 12.4 | Rust 方案：正向 feature（smart/brick）编译期区分，每设备独立二进制 |
| 12.5 | 影响面：分辨率/布局/按键映射/LED/device_model 全部编译期确定 |
| 12.6 | 发行模型从"单包运行时检测"变为"每设备独立包"——用户不会互换 SD 卡，可接受 |
| 12.7 | 打包采用平台子 xtask 全流程自治：device 参数决定 show 编译 feature 与安装图来源 |
| 12.8 | 正向条件 + compile_error! 断言强制"恰好一个"——编译错误取代静默回落 |
| 12.9 | 新设备移植：按 8 项差异点检查，遵守"编译期区分 + 先对照 C"两条铁律 |

---

## 13. 打包与安装

第 12.6/12.7 节讲了发行模型与打包架构（平台子 xtask 全流程自治）——本章聚焦**具体脚本内容**：boot.sh 的三处修改、安装图约束、update.sh 的边界。

### 13.1 boot.sh 移除设备检测（3 处修改对照表）

`implement-tg5040-structure` 变更（2026-08-02 归档）移除 boot.sh 中**所有**依赖 `DEVICE` 环境变量的操作：

| # | 位置（原 C 行号） | 原代码 | 修改方式 |
|---|------------------|--------|---------|
| 1 | 23-26 | `TRIMUI_MODEL=\`strings /usr/trimui/bin/MainUI \| grep ^Trimui\`` + `if [ "$TRIMUI_MODEL" = "Trimui Brick" ]; then DEVICE="brick"; fi` | **删除**（设备检测 + 环境变量写入） |
| 2 | 30-33 | `if [ "$DEVICE" = "brick" ]; then echo 0 > max_scale_lr; echo 0 > max_scale_f1f2; fi` | 改为**无条件** `echo 0 >` 三个 LED 路径 |
| 3 | 41 | `./show.elf ./$DEVICE/$ACTION.png` | 改为 `./show ./$ACTION.png`（去 `.elf` 后缀——Rust 版统一命名约定） |

**不依赖 DEVICE 的部分保持不变**：`mount` 命令、CPU governor 设置、`LD_LIBRARY_PATH`/`PATH` 导出、unzip、install.sh 调用、launch.sh 调用。

### 13.2 安装图的打包约束

`./$ACTION.png` 要求安装图位于**包根目录**——平台子 xtask 装配时按 device 把对应设备的图复制到包根目录：

- `smart`：安装图来自 `install/installing.png`（根目录）
- `brick`：安装图来自 `install/brick/installing.png`（复制到包根目录，与 boot.sh 的 `./$ACTION.png` 固定路径配合）

**boot.sh 本身不做任何目录判断**——**禁止**用 `if [ -d ./brick ]` 目录存在性判断选图（C 原版打包是全复制，smart 包内也有 brick/ 子目录，目录判断会误判）。

### 13.3 update.sh 不涉及（边界说明）

**update.sh 不涉及 DEVICE**——其 `tg3040` 路径检测是历史数据迁移（判断文件系统状态而非设备身份），**不修改**。

### 13.4 打包职责一览

| 负责方 | 职责 |
|--------|------|
| xtask（父） | 通用二进制编译（minui/minarch/clock/minput）+ 通用二进制复制 + 调平台子 xtask（`cargo run --quiet -p tg5040-xtask -- <device>`） |
| 平台子 xtask（`tg5040-xtask`） | 编译 show/keymon + 编译 libretro.so（cores make）+ 复制 show/keymon/.so/install 到 build/（装配逻辑为 Rust 纯函数，`cargo test -p tg5040-xtask` 覆盖落点） |

平台子 xtask 的装配行为有自动化测试（`platforms/tg5040/xtask/src/main.rs` 的 tests——smart/brick 图源、stock/extras 落点、产物缺失报错）。

### 13.5 cores 中间产物清理

cores 编译会产生两类**中间产物**（.gitignore 已忽略，不入库）：`cores/src/`
（13 个上游仓库的克隆源码）与 `cores/output/`（编译出的 `*_libretro.so`）。
需要时可随时清理，下次构建自动重新克隆/编译：

```sh
# 推荐：平台子 xtask 的 cores-nuke 子命令（容器内执行 make nuke，不必进目录）
cargo run -p tg5040-xtask -- smart cores-nuke

# 等价：直接进 cores 目录跑 make nuke（nuke 目标无需 PLATFORM/UNION_PLATFORM）
cd platforms/tg5040/cores && make nuke
```

清理只删 src/output，保留 `cores/makefile`、`cores/patches/`、`cores/README.md`
声明文件。

### 13.6 show 的行为契约（平台自治 bin）

`show`（`platform-tg5040-show`，独立 crate）是 boot.sh 的启动画面工具：加载 PNG → 居中 → 延时 → 退出。契约（代码 `show/src/main.rs` 模块头）：

- 无参调用 → 打印用法、退出码 0（不报错）
- 图片不存在 → 静默退出码 0（安装图缺失不阻塞安装流程）
- 解码失败 → 静默退出码 0（同上——尽力显示，失败不影响主流程）
- 居中用 `saturating_sub` 防下溢（图大于屏时不 panic）
- 默认延时 2 秒；纯 sleep 实现，**不处理输入**（安装画面按键无效是设计意图——防误触中断安装）
- 自实现 `load_png`/`blit_buffer`（依赖 `png` crate），不复用 render——平台自治 bin 自包含（render 是上层 UI 渲染库，不承载平台自治工具原语；render::image 模块曾为此存在，show 迁出后已删除）

---

## 14. 与原 C tg5040 实现的对比

全维度对照表（各子系统的详细差异表见对应章节：视频 7.6、音频 9.4、电源 10.5、settings 11 章各处）：

| 维度 | 原版 C | Rust 版 |
|------|--------|---------|
| **硬件抽象** | `#define PLAT_xxx` 宏在编译时替换函数名 | `impl Platform for Tg5040`，编译期单态化 |
| **设备区分** | 运行时 `getenv("DEVICE")` + `is_brick` 全局变量 | `#[cfg(feature = "brick")]` 编译期区分 |
| **发行** | 1 个二进制服务两台设备 | 每设备独立二进制 + 独立安装包 |
| **内存安全** | 手动 malloc/free，全局可变状态（`gfx`、`pad`、`vid`、`snd`） | 所有状态封装在 `Tg5040` 结构体字段中 |
| **按键映射** | `#define BTN_UP 0` 宏 + switch-case 散落各处 | `common::input::BTN_ID_*` 集中定义，所有平台共享 |
| **音频缓冲** | 全局 `snd` 数组 + pthread 直接操作 + `SDL_LockAudio` | `AudioRingBuffer`（`common::audio`）+ `Mutex`（仅缓冲锁） |
| **电池读取** | 内联 `fopen("/sys/...")` | `get_battery_status()` 方法 + 纯函数档位映射，可测试 |
| **sysfs 读写** | `getInt`/`putInt`（失败隐式 0） | `read_int(path, default)`/`write_int` 纯函数（显式默认值） |
| **命令调用** | `system("...")` 字符串拼接 | `std::process::Command` 参数数组（免 shell 注入） |
| **睡眠** | `system("killall -STOP keymon.elf")` 散落各处 | 封装在 `prepare_sleep` / `complete_wake` 中 |
| **重采样** | push 路径内联（`SND_resampleNone/Near`） | common `Resampler`（minarch 侧） |
| **underrun** | "倒序回放"伪回声 | 静音填充（标准做法） |
| **计时** | `SDL_GetTicks` 宏 | `now_ms()`（FFI 封装，无需子系统初始化） |
| **时钟** | pthread + `usleep(16666)`（keymon） | `std::thread::sleep(16ms)` |
| **退出标志** | `volatile int quit` + sigaction | `AtomicBool` + `libc::sigaction` |

---

## 15. 测试策略

### 15.1 三层方案

平台代码涉及 SDL + 真实硬件——无法像 common 那样全部 headless 测试。采用三层策略：

| 层 | 方式 | 覆盖 | 环境 |
|----|------|------|------|
| ① 纯逻辑单测 | 单元测试（`#[cfg(test)] mod tests`）| 纯函数边界（buffer_as_bytes 字节转换、电量档位映射、read_int 默认值、回调填充、poll_input 状态机）、常量、new() 无副作用 | headless CI ✅ |
| ② 冒烟测试 | `SDL_VIDEODRIVER=dummy` + cargo test | init_video 不 panic + 字段填充、flip 调用序列、quit_video 清理 | headless CI ✅ |
| ③ 真机验证 | 开发机窗口（macOS）+ 真机 | flip 显示正确性（人工）+ 输入/音频/电源行为 | macOS / 真机 |

### 15.2 冒烟测试的关键：SDL dummy 驱动

**SDL 支持 dummy 视频驱动**——无真实窗口的虚拟环境。设置 `SDL_VIDEODRIVER=dummy` 后：

- `SDL_CreateWindow` 成功（虚拟）✅
- `SDL_CreateRenderer` 成功（已验证——探针测试输出 `RENDERER OK`）✅
- `clear()/present()` 是 no-op（不渲染，但调用序列正确）

意义：**init_video/quit_video/flip 可以在 CI 里自动跑"不 panic + 字段填充正确 + 调用序列正确"**——回归保护。显示正确性无法自动化（无头环境不能验证像素），留给③人工验证（与 C 相同）。

```rust
#[test]
fn video_smoke_init_quit_flip() {
    // 必须在 SDL_Init 前设置 dummy 驱动
    unsafe { std::env::set_var("SDL_VIDEODRIVER", "dummy") };
    let mut platform = Tg5040::new();
    platform.init_video();
    assert!(platform.canvas.is_some());   // 字段填充
    platform.flip(&buf, true);            // 调用序列（wait_vsync 两种值）
    platform.flip(&buf, false);
    platform.quit_video();
    assert!(platform.canvas.is_none());   // 清理
}
```

### 15.3 可测性设计：纯函数抽离清单

平台实现把"与 SDL/硬件强耦合的部分"和"纯逻辑"分离——纯逻辑全部可单测：

| 纯函数 | 位置 | 测试覆盖 |
|--------|------|---------|
| `buffer_as_bytes` | `src/lib.rs:274` | pitch=width、pitch>width（硬件对齐）、空 buffer |
| `battery_charge_level` | `src/power.rs` | 档位边界（>80/60/40/20/10/else） |
| `read_int`/`write_int` | `src/power.rs` | 节点缺失返回默认值、写入失败静默 |
| `fill_output` | `src/lib.rs:238` | 空缓冲全零、部分填充、满缓冲 |
| `process_frame`（poll_input 状态机）| `src/input.rs` | 事件序列 → InputState（repeat 节奏、HAT 对角线、虚假释放防御） |

**当前状态**：✅ 平台已全部实现（无未实现方法）。测试在实现变更中按 TDD 流程完成——剩余"验证"性质的清单是各子系统的真机验证项（第 7.7/8.6/9.6/10.6 节），不属于单元测试范畴。

**否决的方案**（防误引入）：

| 方案 | 否决理由 |
|------|---------|
| 只做①纯逻辑 + ③手动（无冒烟） | dummy 驱动已被验证支持 renderer——冒烟测试可行，不做是浪费 |
| mock canvas（抽象 trait） | sdl2 的 Canvas 是具体类型非 trait——抽象它是过度设计（config 禁止类型体操） |

---

## 16. 否决方案清单

以下是在本平台开发过程中**被否决的方案**及否决理由——遇到相似想法时先查此清单（防 AI 后续生成作出错误决策）：

### 16.1 分辨率相关（fix-tg5040-resolution-design 决策）

| 方案 | 否决理由 |
|------|---------|
| 逻辑画布 640×360 + SCALE=2 + flip GPU 放大 2x | 物理元素 = 布局×SCALE×flip = C 的 2 倍大；8 行×60px=480>360 画布装不下——数学不自洽 |
| 逻辑画布 + SCALE=1 + @1x 图集 + flip 2x | @1x 图集被 GPU 放大 2x 像素边缘变粗——**画面精美不可牺牲** |
| flip 1.5x 非整数倍 | 640×1.5=960≠1280，无意义 |
| 重新设计布局（8 行→4 行）适配逻辑画布 | 改变 UI 设计，违背与 C 一致性 |

### 16.2 视频实现相关（implement-tg5040-video 决策）

详见第 7.6 节表格（10 项：unsafe from_ll / 保留 window / 局部持有 sdl / 纹理逻辑尺寸 / flip 内延迟 / trait 加 sync / sdl2::sys 手动 Init / quit 只置 None / LockTexture+FillRect / mock canvas）。

### 16.3 性能优化相关（改变架构，均否决）

| 方案 | 否决理由 |
|------|---------|
| GlyphCache 字形缓存（render 级） | 改变 render 架构 |
| 脏矩形/增量重绘（应用级） | 改变上层主循环架构 |

**性能缓解只采用第 1 层（编译器级）**：`fill_rect` 用 `slice::fill`（memset 级）、blit 用 `copy_from_slice`、release 向量化——均已就位（第 2.6 节）。

### 16.4 settings 与 keymon 相关（implement-tg5040-settings-keymon 决策）

| 方案 | 否决理由 |
|------|---------|
| 幂等 repair（version 就绪标志） | 更健壮但与原版选择对应——非必要不增加复杂度 |
| 强制 host（client 等待重试） | 引入等待逻辑；与原厂进程共处时语义复杂 |
| `Settings` 加 `#[repr(C)]` | 读写双方都是本 crate Rust 代码——布局由编译器保证；repr(C) 引入"有其他 C 进程"的错误暗示 |

### 16.5 输入相关（implement-tg5040-input 决策）

| 方案 | 否决理由 |
|------|---------|
| 用 `evdev` crate 读取按键 | libc 十几行覆盖核心需求——依赖面最小化（见第 3.4 节） |
| common 硬编码 `BTN_MOD_BRIGHTNESS == BTN_MENU` | m17/trimuismart 是 `BTN_START`+`BTN_R1`——硬编码泄漏 tg5040 语义成跨平台 bug |

### 16.6 音频相关（implement-tg5040-audio 决策）

| 方案 | 否决理由 |
|------|---------|
| underrun 部分不足"倒序回放"（api.c:977-981） | 伪回声，非标准做法 |
| push 满时等待 10ms 重试（原版 `SND_batchSamples`） | minarch 有独立音频引擎线程——背压交还调用方更干净 |

### 16.7 电源相关（implement-tg5040-power 决策）

| 方案 | 否决理由 |
|------|---------|
| pid 扫描（/proc 遍历）找 keymon | 原版就是 killall 命令——Command 是最忠实对应 |
| `set_mute(true)` 替代硬件静音 | set_mute 改 mute 状态字段（持久化语义）——与"睡眠临时静音"不同 |
| 直接调 shutdown 脚本 | 原版 exit(0) 后由 launch.sh 循环 break 触发关机链，保持一致 |
| fb0 blank 写 0/4（platform.c:516/521 注释掉） | 原版自己都注释掉了——亮度控制已覆盖关屏 |

## 17. 常见问题 FAQ

### 17.1 为什么 `new()` 不初始化 SDL？

SDL2 的初始化在掌机上是"要么成功，要么设备重启"——没有优雅降级。`Tg5040::new()` 零 SDL 调用、无副作用：`new()` 总是成功，初始化失败发生在可控的 `init_*` 位置，且无副作用 → 可在无 SDL 环境构造（单元测试不需要真实设备）。SDL 资源按子系统**"用到时初始化"**（`init_video`/`init_input`/`init_audio`），对应 C 的分阶段初始化。

### 17.2 为什么 wait_vsync 被忽略？

`flip` 的 `wait_vsync` 参数接收但不用——三原因：① `present_vsync()` 在创建 renderer 时已固定垂直同步，`present()` 天然阻塞到 vsync；② 原版 `PLAT_flip(screen, ignored)` 同样忽略；③ flip 内再做帧预算延迟会与 PRESENTVSYNC 双重节流降帧率。**帧率控制在上层**：`now_ms()` + `std::thread::sleep`（见第 7.4 节）。

### 17.3 为什么纹理 = 物理尺寸？

`SDL_UpdateTexture` **不做缩放**——上传的数据尺寸必须等于纹理尺寸。画布（VideoBuffer）= 物理分辨率（画布=物理决策），纹理也只能 = 物理尺寸，flip 呈现 1:1。推导链：画布=物理 → 纹理=物理 → flip 1:1（见第 7.1 节）。

### 17.4 为什么 evdev 读取用 libc 而不用 evdev crate？

evdev 的核心操作就是 `open` + `read` 一个固定布局的结构体（`struct input_event`）——libc 十几行代码即可覆盖，无需引入依赖。`cargo tree -p platform-tg5040` 的第三方依赖只有 `sdl2` + `libc`——依赖面最小化（config.yaml：非必要不增加复杂度），且 libc 是原版 C 的"最忠实对应"（见第 3.4 节）。

### 17.5 为什么 Settings 无 `#[repr(C)]` 而 InputEvent 有？

`#[repr(C)]` 的意义是"布局与 C 编译器生成的布局一致"——需要它的前提是**存在跨语言边界**：

- `InputEvent`：数据源是 Linux 内核（`libc::read` 从设备文件读取）——**跨越 Rust/内核边界**，必须 `#[repr(C)]`（布局与内核 `struct input_event` 一致）
- `Settings`：读写双方（keymon 写、minui/minarch 读）都是本 crate 的 Rust 代码，同一构建产物——布局一致性由编译器保证，无需 `#[repr(C)]`（见第 11.3.1/11.5.1 节）

### 17.6 为什么 SettingsHandle 的 setter 用 `&self`？

`map` 裸指针 + 内部 `lock: Mutex<()>` 使共享引用即可安全写共享内存——这正是 mute 监控线程经 `Arc<SettingsHandle>` 与主循环共享句柄的前提（`&mut self` 无法在线程间共享）。进程内线程并发由 `lock` 隔离；跨进程并发是设计意图（见第 11.3.2 节）。

### 17.7 为什么音频缓冲是 4000 帧？跟 fps 有关吗？

原版容量 = `buffer_seconds × sample_rate / frame_rate` = 5×48000/60——其中除 `frame_rate`（游戏帧率）是"游戏帧计量"残留：**缓冲容量与游戏帧率无关**（缓冲按采样帧数计量，不是按游戏帧计量）。Rust 版固定 4000 帧（≈83ms@48kHz），不随采样率/fps 变化（见第 9.2 节）。

### 17.8 brick 逻辑分辨率 341 为什么不取整？

那是已废弃的逻辑画布方案的遗留问题：341 = 1024/3 向下取整（341×3 = 1023 ≠ 1024，取整误差）。**画布=物理分辨率后此问题彻底消失**——SCREEN 直接等于物理屏（1024），无推导、无取整（见第 2.5 节解疑 1）。

---

## 18. 公开 API 说明

本 crate 对外暴露的内容：

### 18.1 库侧

- **`Tg5040`**（`src/lib.rs:68`）：唯一对外类型——实现了 `common::platform::Platform` trait 的平台结构体
- **`settings` 模块**：`SettingsHandle`（设置读写）——trait 之外的"第二个接口面"（第 11.6 节）
- **`input` 模块**：映射常量（`JOY_*`/`CODE_*`/`AXIS_*`）均为 `pub(crate)`（内部实现细节）；设备语义键（`BTN_SLEEP`/`BTN_MOD_*`）已归 `Platform` trait 关联常量，经 `Tg5040` 读取（第 8.2 节）

```rust
use platform_tg5040::Tg5040;

let mut platform = Tg5040::new();
// Tg5040 实现了 Platform trait，所有硬件访问通过 trait 方法调用：
platform.init_video();
let input = platform.poll_input();
platform.flip(&screen, true);
```

`Tg5040::new()` 总是成功（不分配 SDL 资源），真正的初始化在 `init_video()` / `init_input()` / `init_audio()` 中完成。调用方负责按正确顺序调用这些方法（video → input → audio），这是 `minui`/`minarch` 的 `main()` 函数职责，不在本 crate 范围内。

### 18.2 bin 侧（平台自治 bin，独立 crate）

- **`keymon`**（`platform-tg5040-keymon`，`keymon/src/main.rs`）：系统级按键守护进程独立二进制——不直接对外 API，由 launch.sh 后台启动（第 11.5 节）
- **`show`**（`platform-tg5040-show`，`show/src/main.rs`）：启动画面显示工具（第 13 章）
- **`tg5040-xtask`**（`platforms/tg5040/xtask/`）：平台自治打包编排——编译上述两个 bin + cores make + 装配复制（第 12.7 节）

---

---

## 设计决策记录

平台各章的「决策点 N」「否决方案清单」已随模块讲解展开，本节收拢**跨子系统决策**与**演进史**（平台自治/能力归属），条目要素：结论 → 为什么 → 被否决 → 产生的问题。

### 决策：设备区分编译期化（feature 替代 is_brick 运行时检测）

- **结论**：smart/brick 差异全部 `#[cfg(feature)]` 编译期区分（正向条件 + `compile_error!` 双断言强制恰好一个），每设备独立二进制与安装包（第 12 章全推导）。
- **为什么**：Platform trait 关联常量是编译期值，无法在运行时切换（照搬 C 需推翻已归档 trait）；编译器保证"smart 二进制不可能执行 brick 分支"。
- **被否决**：运行时 `getenv("DEVICE")` + `is_brick`（C 原版做法——trait 常量不可变）；`#[cfg(not(feature = "brick"))]` 负向逻辑（早期版本——叠加 feature 时规避重复定义，但不可读、第三设备无法表达，被编译错误取代静默回落的方案替代）。
- **产生的问题**：发行从"单包通吃"变"每设备独立包"（用户不互换 SD 卡，可接受）；boot.sh 移除 3 处 DEVICE 依赖、安装图打包期选定（第 13 章）。

### 决策：画布 = 物理分辨率（分辨率方案修正史）

- **结论**：SCREEN = 物理屏（smart 1280×720/SCALE=2、brick 1024×768/SCALE=3），flip 恒 1:1（第 2 章完整推导）。
- **为什么**：逻辑画布（640×360 + flip 放大）数学不自洽——物理元素 = 布局×SCALE×flip = C 的 2 倍大、8 行装不下；@1x 图集放大损害画面精美。
- **被否决**：逻辑画布 + SCALE=2 + flip 2x；逻辑画布 + SCALE=1 + @1x + flip 2x（像素变粗）；flip 1.5x；重新设计布局适配逻辑画布（改变 UI 设计）。
- **产生的问题**：CPU 渲染 921K 像素/帧——缓解只采用第 1 层（编译器级），字形缓存/脏矩形因改变架构被否决，文字渲染若实测拖后腿记为已知限制。

### 决策：平台自治拆 crate（show/keymon/平台子 xtask 独立）

- **结论**：平台 lib 纯 lib；show/keymon 独立 crate（`platform-tg5040-show/-keymon`），平台子 xtask（`tg5040-xtask`）编译装配；父 xtask 收缩为通用层。
- **为什么**：lib+bin 混合时父 xtask 的 build 连带编译 bin（依赖图副作用）；平台侧工作（编译 + 装配）需要 Rust 载体（可测、命名约定统一）。
- **被否决**：保留 lib+bin 混合只把 xtask 清单移出；嵌套 workspace（与上层路径依赖冲突、双 Cargo.lock）；package.sh 扩展承担 4 件事；白名单豁免 show/keymon（白名单机制整体删除，改 pub(crate) 编译期强制）；render::image 模块承载 show 的 PNG 解码（show 自包含，image 随迁出删除）。
- **产生的问题**：平台子 xtask 与父 xtask 的目录/产物约定需靠约定自定位（build/ 与 target 不传参）；cores 的 UNION_PLATFORM 环境变量传递成为跨文档统一约定。

### 决策：语义键与能力常量归 trait（trait 面扩展）

- **结论**：`BTN_SLEEP`/`BTN_MOD_*`（语义键）与 `HAS_L2/R2/L3/R3/LS/RS/VOLUME/MENU`（能力常量）均为 `Platform` trait 关联常量；布局差异值（`main_row_count`/`padding`）为 trait 方法。
- **为什么**：语义键是"前端与 keymon 守护进程的协调常量"（keymon 硬编码同套 evdev 约定），跨 minui/minarch/平台共享 → 放 trait 单一来源；能力常量同理（minput 消费）；布局值因 my355 等平台的 `on_hdmi` 运行时分支只能方法表达。
- **被否决**：常量放平台 crate 由装配层 cfg 引用（早期形态——上层引用平台私有模块，白名单机制复杂）；`BTN_RESUME` 入 trait（minui 单消费者功能键，本地常量即可）。
- **产生的问题**：新增平台须同步填 8 个能力常量与 2 个布局方法（12.9 检查清单已列）；minput/布局方法的默认实现语义（未覆盖时用通用默认值）。
