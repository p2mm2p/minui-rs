# MinUI — Rust 重写完整报告

> 原项目：[shauninman/MinUI](https://github.com/shauninman/MinUI) — 复古掌机上的极简游戏启动器

---

## 目录

1. [系统概述：MinUI 是什么](#1-系统概述minui-是什么)
2. [系统运行全景：从按下电源到进入游戏](#2-系统运行全景从按下电源到进入游戏)
3. [SD 卡文件系统布局](#3-sd-卡文件系统布局)
4. [架构分层设计](#4-架构分层设计)
5. [核心数据结构全景](#5-核心数据结构全景)
   - 5.1 [数据模型层次](#51-数据模型层次)
   - 5.2 [Entry 的七态生命周期](#52-entry-的七态生命周期)
   - 5.3 [Button 位掩码设计](#53-button-位掩码设计)
   - 5.4 [Entry 结构体详解](#54-entry--文件系统条目的表示)
   - 5.5 [Directory 详解](#55-directory--一个屏幕的可浏览内容)
   - 5.6 [Recent 详解](#56-recent--最近游戏跨设备共享)
   - 5.7 [Button 和 PadContext 详解](#57-button-和-padcontext--输入抽象)
6. [关键技术机制深度解析](#6-关键技术机制深度解析)
   - 6.1 [ROM 到模拟器的映射](#61-rom-到模拟器的映射)
   - 6.2 [主机目录归类 (Collation)](#62-主机目录归类-collation)
   - 6.3 [滚动窗口算法](#63-滚动窗口算法)
   - 6.4 [双缓冲翻页机制](#64-双缓冲翻页机制)
   - 6.5 [多碟游戏处理 (M3U)](#65-多碟游戏处理-m3u)
   - 6.6 [存档槽位系统](#66-存档槽位系统)
   - 6.7 [电源管理状态机](#67-电源管理状态机)
   - 6.8 [名称映射 (map.txt)](#68-名称映射-maptxt)
   - 6.9 [Platform trait 详解](#69-platform-trait--支持-20-设备的秘诀)
   - 6.10 [Framebuffer 详解](#610-framebuffer--原始帧缓冲的抽象)
   - 6.11 [get_root() 算法详解](#611-get_root--最复杂的扫描函数)
   - 6.12 [open_rom() 流程详解](#612-open_rom--游戏启动的完整流程)
   - 6.13 [open_directory() 流程详解](#613-open_directory--目录导航含自动启动)
   - 6.14 [handle_launcher_input() 详解](#614-handle_launcher_input--导航输入的完整处理)
   - 6.15 [run() 主循环详解](#615-run--主事件循环)
   - 6.16 [fb_draw_text() 详解](#616-fb_draw_text--软件文字光栅化)
   - 6.17 [PowerManager 详解](#617-powermanager--电源管理状态机)
7. [原 C 代码到 Rust 的完整映射](#7-原-c-代码到-rust-的完整映射)
   - 7.1 [源文件映射](#71-源文件映射)
   - 7.2 [完整函数映射表](#72-完整函数映射表)
8. [Rust 重写的优化与改进](#8-rust-重写的优化与改进)
9. [如何新增一个平台](#9-如何新增一个平台)
10. [测试策略与覆盖](#10-测试策略与覆盖)
11. [当前模块架构](#11-当前模块架构)
12. [分步重写记录](#12-分步重写记录)

---

## 1. 系统概述：MinUI 是什么

### 1.1 一句话定义

MinUI 是一个运行在**国产 ARM Linux 复古掌机**上的**极简游戏启动器**（Launcher）。它替换掉原厂系统那个臃肿花哨的界面，只做一件事：**列出游戏 → 选游戏 → 玩 → 下次开机自动回到上次位置**。

### 1.2 运行环境

| 项目   | 详情                                            |
|------|-----------------------------------------------|
| 硬件   | 全志/瑞芯微/君正 ARM SoC，64-256MB RAM，480p/720p LCD  |
| 操作系统 | 裁剪过的 Linux 3.x/4.x，rootfs 约 20-80MB           |
| 图形   | Linux framebuffer (`/dev/fb0`)，无 X11/Wayland  |
| 输入   | `/dev/input/event*` 设备节点，直接读取 evdev 事件        |
| 显示   | RGB565 (16-bit) 像素格式，双缓冲 page flipping        |
| 字体   | 单个 .otf 文件，通过 SDL_ttf (C) / fontdue (Rust) 渲染 |
| 模拟器  | libretro 核心 (.so)，由 minarch 加载和驱动             |

> **背景：为什么是 RGB565 和 framebuffer？** 这些掌机的 LCD 控制器原生支持 16-bit RGB565 格式，不需要像桌面 GPU 那样做颜色空间转换。使用 framebuffer 直接写入硬件显示缓冲区，避免了 X11/Wayland 等显示服务器的开销——在 64MB RAM 的设备上，每一 KB 都很珍贵。MinUI 不需要窗口管理器：整个屏幕就是一块可写的像素数组。

### 1.3 支持的设备

20+ 种掌机：Anbernic RG35XX 系列、Miyoo Mini/Plus、Trimui Smart/Brick/Pro、Powkiddy RGB30、MagicX 系列、GKD Pixel 等。所有设备共享**同一张 SD 卡和同一套代码**，仅编译时切换平台配置文件。

### 1.4 核心设计哲学

- **零配置**：没有设置菜单，插入 SD 卡即用
- **极简 UI**：无封面图、无主题、无动画、无多余元素
- **自动恢复**：关机再开直接回到刚才玩的游戏，用户感觉不到中断
- **单 SD 卡跨设备**：一张卡可以在不同厂商的多个设备间共用
- **Pak 扩展系统**：第三方模拟器以 `.pak` 文件夹形式安装，无需重新编译

---

## 2. 系统运行全景：从按下电源到进入游戏

### 2.1 开机启动流程

```
设备上电
  │
  ├─ [1] Bootloader → 加载 Linux kernel
  │
  ├─ [2] Kernel 启动 → 挂载 rootfs → 执行 init 脚本
  │
  ├─ [3] 原厂 init 脚本 → MinUI 在 SD 卡上放置了修改过的启动脚本
  │     (install.sh / boot.sh)，劫持原厂启动流程
  │
  ├─ [4] 挂载 SD 卡 → 设置环境变量 → 启动 minui.elf
  │
  └─ [5] minui.elf 启动 → 显示游戏列表
```

关键设计：**MinUI 不修改原厂 kernel 和 rootfs**。它利用原厂固件已有的 SD 卡启动机制，只需在 SD 卡上放置正确的文件即可。

### 2.2 minui.elf 的生命周期

```
minui.elf 启动
  │
  ├─ autoResume() → 检查 /.userdata/shared/.minui/auto_resume.txt
  │  └─ 存在 → 直接启动游戏，返回（用户无感知的自动恢复）
  │
  ├─ [初始化阶段]
  │  ├─ GFX_init()    → 打开 /dev/fb0 → mmap → 设置双缓冲
  │  ├─ PAD_init()    → 打开 /dev/input/event* → 配置按键映射
  │  ├─ PWR_init()    → 读取电池 sysfs → 初始化休眠计时器
  │  └─ Menu_init()   → 扫描 SD 卡目录树 → 加载最近游戏
  │
  ├─ [主循环] while(!quit)
  │  ├─ poll_input()         → 读取硬件按键 → 更新 pad 状态
  │  ├─ PWR_update()         → 休眠计时器 / 自动关机 / 亮度音量
  │  ├─ handle_input()       → 方向键导航 / A确定 / B返回 / X恢复
  │  ├─ [if dirty] render()  → 清屏 → 硬件状态栏 → 列表 → 按钮提示
  │  │                        → flip() 翻页显示
  │  └─ [else] vsync_wait()  → 等待下一帧
  │
  └─ [退出]
     ├─ 写 /tmp/next → 退出 minui
     └─ 外层 shell 读取 /tmp/next → 执行 minarch.elf <core> <rom>
```

### 2.3 游戏启动后的交接机制

minui **不直接启动游戏**，而是通过一个临时文件接力：

```
minui.elf:
  queueNext("'/path/to/GB.pak/launch.sh' '/path/to/Zelda.gb'")
  → putFile("/tmp/next", cmd)
  → quit = 1
  → 退出

外层 shell:
  cmd=$(cat /tmp/next)
  exec $cmd

launch.sh:
  minarch.elf gambatte_libretro.so "Zelda.gb"
```

为什么不用 `fork()+exec()`？因为外层 shell 脚本需要在 minarch 退出后做清理工作（sync 存档、恢复亮度等），这种间接方式让 minui 保持简单。

> **背景：进程交接模式**。在资源受限的嵌入式 Linux 上，`fork()+exec()` 会复制整个进程地址空间（包括 mmap 的 framebuffer），开销很大。通过文件接力，minui 干净地退出并释放所有资源（关闭 fd、unmap 内存），然后由轻量的 shell 脚本启动下一阶段。这避免了进程树中的僵尸进程，也简化了信号处理。

### 2.4 存档恢复机制

MinUI 的无感恢复依赖两个文件：

| 文件                | 写入时机              | 读取时机                  | 内容          |
|-------------------|-------------------|-----------------------|-------------|
| `auto_resume.txt` | minarch 异常退出时（没电） | minui 启动时             | ROM 相对路径    |
| `recent.txt`      | 每次启动游戏            | minui 启动时             | 最近游戏列表（含别名） |
| `/tmp/last.txt`   | 每次游戏启动/退出         | minui 启动时（`loadLast`） | 上次浏览的目录路径   |

自动恢复流程：
```
autoResume():
  1. 检查 auto_resume.txt 是否存在
  2. 读取 ROM 相对路径 → 拼接 SD 卡完整路径
  3. 验证 ROM 文件仍然存在
  4. 验证对应模拟器仍然存在
  5. 写入存档槽位 9（自动恢复专用）
  6. 构造命令 → 写入 /tmp/next → 退出
  7. 删除 auto_resume.txt（防止重复恢复）
```

正常启动（A 键）使用默认槽位 8（隐藏存档），X 键从上次手动存档恢复。

---

## 3. SD 卡文件系统布局

```
<SDCARD>/                              ← 例如 /mnt/sdcard
│
├── Bios/                              ← 主机 BIOS 文件
│   ├── GB/
│   ├── GBA/
│   ├── PS/
│   └── ...
│
├── Roms/                              ← 游戏 ROM（MinUI 扫描此目录）
│   ├── Game Boy (GB)/                 ← 括号中的标签映射到模拟器
│   │   ├── Zelda.gb
│   │   └── Mario (World).gb
│   ├── Game Boy Advance (GBA)/
│   ├── Super Nintendo (SFC)/
│   ├── Sony PlayStation (PS)/
│   │   └── Final Fantasy VII/
│   │       ├── Disc 1.bin
│   │       └── Final Fantasy VII.m3u  ← 多碟播放列表
│   └── map.txt                        ← 可选的名称映射文件
│
├── Saves/                             ← 游戏存档（.srm）
│
├── .system/                           ← MinUI 系统文件（⚠ 更新时整体替换）
│   ├── <PLATFORM>/                    ← 如 rg35xx, miyoomini
│   │   ├── bin/
│   │   │   ├── minui.elf             ← 主启动器
│   │   │   ├── minarch.elf           ← libretro 宿主
│   │   │   ├── keymon.elf            ← 按键监控守护进程
│   │   │   └── install.sh            ← 安装/更新脚本
│   │   ├── lib/
│   │   │   └── libmsettings.so       ← 设置读写库
│   │   └── cores/
│   │       ├── gambatte_libretro.so   ← Game Boy 模拟核心
│   │       ├── gpsp_libretro.so       ← GBA 核心
│   │       └── ...
│   └── res/
│       ├── BPreplayBold-unhinted.otf ← 字体文件
│       └── assets@2x.png             ← UI 精灵图（C 版用，Rust 版程序化绘制）
│
├── .userdata/                         ← 用户数据（⚠ 更新时保留）
│   ├── <PLATFORM>/                    ← 平台专属用户数据
│   └── shared/                        ← 跨平台共享
│       ├── enable-simple-mode         ← 空文件，存在则启用简化模式
│       └── .minui/
│           ├── recent.txt             ← 最近游戏列表（相对路径）
│           ├── auto_resume.txt        ← 自动恢复标记
│           └── <EMU>/                 ← 按模拟器分
│               └── <romname>.txt      ← 存档槽位状态文件
│
├── Emus/                              ← 额外模拟器 Pak
│   └── <PLATFORM>/
│       └── MGBA.pak/
│           ├── launch.sh
│           └── mgba_libretro.so
│
├── Tools/                             ← 工具 Pak
│   └── <PLATFORM>/
│       └── Clock.pak/
│           └── launch.sh
│
└── Collections/                       ← 收藏列表（.txt 文件作为伪目录）
    └── My Favorites.txt
```

### 关键设计原则

- **`.system/` vs `.userdata/`**：前者存放可执行代码，升级时整体替换；后者存放用户数据，升级时保留。这是 MinUI OTA 更新的核心。
- **路径相对化**：`recent.txt` 中存储的是去掉 SDCARD_PATH 前缀的相对路径，使同一张 SD 卡可在不同设备间共享。
- **伪目录**：`Recently Played` 和 `Collections/*.txt` 不是真实目录，但在 UI 中表现为可浏览的目录。

---

## 4. 架构分层设计

### 4.1 四层架构

```
┌──────────────────────────────────────────────────┐
│  应用层    │  minui (启动器)    minarch (游戏宿主) │
│            │  state.rs         launch.rs          │
├──────────────────────────────────────────────────┤
│  服务层    │  渲染 (render.rs)   电源 (power.rs)   │
│            │  扫描 (scan.rs)    工具 (utils.rs)    │
├──────────────────────────────────────────────────┤
│  抽象层    │  Platform trait (platform.rs)        │
│            │  Framebuffer, 路径常量, 方法签名      │
├──────────────────────────────────────────────────┤
│  系统层    │  Linux framebuffer, evdev, sysfs     │
│            │  (每个平台独立实现 Platform trait)    │
└──────────────────────────────────────────────────┘
```

### 4.2 平台抽象：如何用一份代码支持 20+ 种设备

原 C 代码的策略：每个平台一个目录，包含 `platform.h`（常量 #define）和 `platform.c`（函数实现）。编译时通过 `-I` 引入对应平台的头文件。

Rust 的策略：**Platform trait + Cargo features**。

```rust
pub trait Platform: Send + Sized {
    // ── 屏幕参数（编译时常量）──
    const FIXED_WIDTH: u32;       // 如 640
    const FIXED_HEIGHT: u32;      // 如 480
    const FIXED_BPP: u8;          // 2 (RGB565)
    const FIXED_SCALE: u32;       // 2 (逻辑分辨率 = 物理/2)

    // ── 按键映射 ──
    const KEY_UP: i32;            // SDL keycode 或 evdev code
    const KEY_A: i32;
    // ... 16+ 按键

    // ── 运行时方法 ──
    fn init_video(&mut self) -> Result<Framebuffer, String>;
    fn poll_input(&mut self, pad: &mut PadContext);
    fn get_battery_status(&self) -> (bool, u8);
    fn power_off(&self) -> !;
    // ... 30+ 方法
}
```

编译时通过 Cargo features 选择：
```toml
[features]
platform-rg35xx = []
platform-miyoomini = []
```

代码中：
```rust
#[cfg(feature = "platform-rg35xx")]
pub struct Rg35xxPlatform { /* ... */ }

#[cfg(feature = "platform-rg35xx")]
impl Platform for Rg35xxPlatform { /* ... */ }
```

### 4.3 模块依赖关系图

```
                    state (主循环)
                   /    |     \
                  /     |      \
            launch   render   power
           /    \      |
          /      \     |
       utils    scan   |
          \      /     |
           paths       |
              \       /
           platform (trait)
```

- `state` 依赖所有模块，是胶水层
- `platform` 是最底层，被 `paths`、`render`、`state` 依赖
- `utils` 是纯工具函数，被 `scan`、`launch`、`state` 依赖
- `launch` 依赖 `scan` (roms_path, has_emu) 和 `utils` (escape, path_exists)

---

## 5. 核心数据结构全景

### 5.1 数据模型层次

```
MinUi (全局状态)
├── stack: Vec<Directory>         ← 导航栈，最后一个 = 当前目录
│   └── Directory
│       ├── path: String          ← 此目录的路径
│       ├── name: String          ← 显示名
│       ├── entries: Vec<Entry>   ← 条目列表（已排序）
│       ├── alphas: Vec<usize>    ← 字母索引 (L1/R1 快速跳转)
│       ├── selected: usize       ← 当前选中索引
│       ├── start: usize          ← 可视窗口起始
│       └── end: usize            ← 可视窗口结束
│
├── recents: Vec<Recent>          ← 最近游戏（持久化到 recent.txt）
│   └── Recent
│       ├── path: String          ← 相对路径（不含 SDCARD_PATH）
│       ├── alias: Option<String> ← 可选别名
│       └── available: bool       ← 模拟器是否可用
│
├── pad: PadContext               ← 当前按键状态
│   ├── is_pressed: Button        ← 当前帧按下的按钮（位掩码）
│   ├── just_pressed: Button      ← 刚按下（上升沿）
│   ├── just_released: Button     ← 刚释放（下降沿）
│   └── just_repeated: Button     ← 长按自动重复
│
├── restore_*: usize              ← 返回上级目录时的恢复状态
└── dirty: bool                   ← 是否需要重绘
```

### 5.2 Entry 的七态生命周期

```
                    文件系统
                       │
                       ▼
              [1] scan_dir() 扫描目录
                       │
                       ▼
              [2] create_entry() 构造 Entry
                  path=完整路径, name=显示名
                       │
                       ▼
              [3] make_directory() 建立索引
                  ├── 排序（大小写不敏感）
                  ├── 去重（同名条目 unique 字段）
                  └── 字母索引（alphas 数组）
                       │
              ┌────────┼────────┐
              ▼        ▼        ▼
        [4] ENTRY_DIR  [4] ENTRY_ROM  [4] ENTRY_PAK
         进入子目录     启动游戏        启动工具包
              │           │              │
              ▼           ▼              ▼
        [5] 递归扫描   openRom()     openPak()
                       │              │
                       ▼              ▼
                 [6] queueNext()  [6] queueNext()
                       │              │
                       ▼              ▼
                 [7] 退出 minui    [7] 退出 minui
```

### 5.3 Button 位掩码设计

```rust
pub struct Button(pub u32);  // 32 位，每个按钮占 1 bit

// 单独按钮
Button::A     = 0b_0000_0000_0001_0000   // bit 4
Button::UP    = Button::DPAD_UP | Button::ANALOG_UP  // 组合

// 检测
pad.just_pressed.contains(Button::A)     // A 刚被按下
pad.is_pressed.contains(Button::MENU)    // MENU 正被按住
pad.just_repeated.contains(Button::UP)   // UP 长按重复
```

对应 C 中：
```c
#define BTN_A (1 << BTN_ID_A)           // 0x0010
PAD_justPressed(BTN_A)                  // pad.just_pressed & BTN_A
PAD_isPressed(BTN_MENU)                 // pad.is_pressed & BTN_MENU
```

---

### 5.4 Entry —— 文件系统条目的表示

```rust
// C: typedef struct Entry { char* path; char* name; char* unique; int type; int alpha; }
struct Entry {
    path: String,           // 完整路径。C 中用 strdup 分配，Rust 由 String 管理
    name: String,           // 显示名。由 getDisplayName() 生成：去扩展名、去括号、去空白
    unique: Option<String>, // NULL → None。区分同名条目的唯一标识
    entry_type: EntryType,  // 枚举替代 C 的 int magic number
    alpha: usize,           // 指向父级 alphas 数组的索引，用于 L1/R1 字母快速跳转
}
```

**`path` vs `name` 的区别**：`path` 是机器使用的完整路径如 `/mnt/sdcard/Roms/Game Boy (GB)/Zelda (World) (USA) [v1.1].gb`，`name` 是用户看到的 `Zelda`。这个转换由 `get_display_name()` 完成：提取文件名 → 循环去除扩展名（最多 4 字符 + 点）→ 去除末尾 () 和 [] → 去除末尾空白。

**`unique` 的触发条件**：当两个相邻 Entry 的 `name` 完全相同时。例如目录下有两个 ROM，显示名都是 "Super Mario World"（分别是 USA 和 Japan 版本），`unique` 会被设置为各自的文件名。在 UI 中表现为：选中时先显示小字的 unique 文件名，下方显示大字 name。

**`alpha` 的构建**：`make_directory()` 排序后遍历 entries：
```rust
for (i, entry) in entries.iter().enumerate() {
    let first_char = entry.name.chars().next().unwrap_or(' ').to_ascii_lowercase();
    let letter_idx = if first_char.is_ascii_alphabetic() {
        (first_char as u8 - b'a') as isize + 1
    } else { 0 };
    if letter_idx != cur_alpha { alphas.push(i); cur_alpha = letter_idx; }
    entry.alpha = alphas.len().saturating_sub(1);
}
```


### 5.5 Directory —— 一个屏幕的可浏览内容

```rust
// C: typedef struct Directory { char* path; char* name; Array* entries; IntArray* alphas;
//                                int selected; int start; int end; }
struct Directory {
    path: String,           // 此目录的文件系统路径
    name: String,           // 显示名（由 getDisplayName 生成）
    entries: Vec<Entry>,    // 条目列表，已按 name 大小写不敏感排序
    alphas: Vec<usize>,     // alphas[i] = 第 i 个字母组在 entries 中的起始索引
    selected: usize,        // 当前光标位置（索引，非行号）
    start: usize,           // 可视窗口起始索引
    end: usize,             // 可视窗口结束索引（不含）
}
```

**滚动窗口不变量**：`start <= selected < end`，`end - start <= MAIN_ROW_COUNT`（通常为 6），`end <= total`。这是所有导航操作的基础。

**selected 与行号的关系**：`selected_row = selected - start`。当 selected 移出 [start, end) 范围时，滚动窗口跟随移动。

**alphas 的使用示例**：
```
entries: ["Adventure", "Bomberman", "Castlevania", "Contra", "Donkey Kong"]
alphas:  [0, 2, 4]  // A 从索引0开始，C 从索引2开始，D 从索引4开始
```

按 L1 跳转：取当前 entry.alpha → alpha-1 → alphas[alpha-1] → 跳转到该索引。按 R1 跳转同理。


### 5.6 Recent —— 最近游戏（跨设备共享）

```rust
// C: typedef struct Recent { char* path; char* alias; int available; }
struct Recent {
    path: String,           // 相对路径（已去掉 SDCARD_PATH 前缀！）
    alias: Option<String>,  // 可选别名（map.txt 映射或用户自定义）
    available: bool,        // 模拟器在当前设备上是否可用
}
```

**路径相对化的动机**：同一张 SD 卡插入不同设备，SDCARD_PATH 可能不同（`/mnt/sdcard` vs `/mnt/mmcblk0p1`）。保存相对路径使得最近游戏可以在设备间共享。

**添加算法** (`add_recent_direct`)：
1. 去除 SDCARD_PATH 前缀得到相对路径
2. 在 recents 中查找 `path == relative`
3. 已存在 → 从当前位置移除 → 插入到头部（bump to top）
4. 不存在 → 检查容量（max 24）→ 超出限制则 pop 末尾 → 插入头部
5. 持久化到 `recent.txt`（相对路径 + `\t` + 别名 + `\n`）

**保存格式** (`save_recents`)：
```
Roms/GB/Zelda.gb\tZelda
Roms/GBA/Metroid.gba
Roms/SFC/Super Mario World.sfc\tMario
```
每行一个条目，`\t` 后是可选的别名。读取时反向解析（`load_recents`）。


### 5.7 Button 和 PadContext —— 输入抽象

```rust
// C: 分散的 #define BTN_* 宏 + PAD_Context 结构体
struct Button(pub u32);  // 新类型模式包装 u32 位掩码

// 每个按钮占独立 bit:
// Button::A     = 0b_0000_0000_0001_0000  (bit 4 = 1 << BTN_ID_A)
// Button::UP    = Button::DPAD_UP | Button::ANALOG_UP  (组合按钮)
// Button::LEFT  = Button::DPAD_LEFT | Button::ANALOG_LEFT
```

**PadContext 的时序检测**：
```rust
struct PadContext {
    is_pressed: Button,      // 当前帧被按下的所有按钮
    just_pressed: Button,    // 当前帧刚按下的按钮（上升沿：之前未按下，现在按下）
    just_released: Button,   // 当前帧刚释放的按钮（下降沿）
    just_repeated: Button,   // 长按自动重复（首次300ms延迟，之后每100ms触发）
    repeat_at: [u32; COUNT], // 每个按钮下次触发 repeat 的时间戳
}
```

**just_pressed 的计算**（在 `poll_input` 中）：`just_pressed = is_pressed & !was_pressed`。即：当前帧按下的按钮中，去掉上一帧也在按下的。这是纯硬件去抖 + 边沿检测。

**just_repeated 的计算**：当 `is_pressed` 持续为真且当前时间 `>= repeat_at[btn_id]` 时，将该按钮加入 `just_repeated`，并更新 `repeat_at` 为当前时间 + 100ms（首次延迟为 300ms）。

**为什么需要组合按钮**：嵌入式掌机的 D-pad 和摇杆在 evdev 层面是不同的按键码，但对用户来说都是"方向"操作。`Button::UP = DPAD_UP | ANALOG_UP` 统一了这两种输入源。


---


## 6. 关键技术机制深度解析

### 6.1 ROM 到模拟器的映射

MinUI 通过**目录命名约定**自动发现模拟器：

```
路径: /mnt/sdcard/Roms/Game Boy (GB)/Zelda.gb
                                    ^^
                              这就是模拟器标签

解析流程 getEmuName():
  1. 路径在 ROMS_PATH 下 → 提取 Roms 子目录名 "Game Boy (GB)"
  2. 取末尾括号内容 → "GB"
  3. 查找 "GB.pak"  → 找到对应的 launch.sh
  4. launch.sh 调用 minarch.elf gambatte_libretro.so "Zelda.gb"
```

### 6.2 主机目录归类 (Collation)

同名主机的不同变体自动合并显示：

```
Roms/
├── Game Boy (GB)/       ← 归类前缀: "Roms/Game Boy ("
├── Game Boy (GBC)/      ← 匹配前缀 → 合并到 Game Boy 列表
├── Game Boy Color (GBC)/← 不匹配 ( "(" ≠ "C" ) → 单独列表
└── Game Boy Advance (GBA)/ ← 不匹配 → 单独列表
```

算法：截取到 `(` 为止作为归类前缀，通过 `prefix_match` 做匹配。

### 6.3 滚动窗口算法

列表项超过屏幕行数时维护可视窗口：

```
条目总数 = T, 屏幕行数 = N (通常 6)

不变量:
  start ≤ selected < end
  end - start ≤ N
  end ≤ T

按键处理:
  UP:    selected--; if selected < start: start--, end--
  DOWN:  selected++; if selected ≥ end:   start++, end++
  LEFT:  selected -= N (翻页上)
  RIGHT: selected += N (翻页下)
  L1:    跳转到上一个字母分组
  R1:    跳转到下一个字母分组
```

### 6.4 双缓冲翻页机制

MinUI 使用 ION 内存分配器（Android/Linux 内核的 DMA 内存管理接口）分配连续的物理内存作为帧缓冲。这允许显示引擎（DE）通过 DMA 直接从物理地址读取像素数据，无需 CPU 参与拷贝。

> **背景：为什么需要 ION？** 普通的 `malloc` 分配的是虚拟内存，物理地址可能不连续。显示引擎的 DMA 控制器需要连续的物理地址。ION 分配器从内核预留的连续内存池（如 PMEM）中分配，同时返回虚拟地址（给 CPU 用）和物理地址（给 DMA 用）。这避免了 CPU 做昂贵的 `memcpy` 到显示缓冲区。

```
ION 分配的内存:  [ PAGE_0 (640x480) ][ PAGE_1 (640x480) ]
                  ↑ 当前显示            ↑ 当前绘制

flip():
  1. DE_OVL_BA0 = fb_paddr + page * PAGE_SIZE  ← 硬件寄存器切换显示缓冲
  2. vsync_wait()                                ← 等待垂直同步（避免撕裂）
  3. page ^= 1                                   ← 交换前后台
  4. pixels = fb_vaddr + page * PAGE_SIZE        ← 更新 CPU 绘制指针
  5. memset(pixels, 0)                           ← 清除新后台（可选）
```

### 6.5 多碟游戏处理 (M3U)

PlayStation 等多碟游戏的处理：

```
目录结构:
Roms/PS/
├── Final Fantasy VII/
│   ├── Final Fantasy VII (Disc 1).cue
│   ├── Final Fantasy VII (Disc 2).cue
│   └── Final Fantasy VII (Disc 3).cue
└── Final Fantasy VII.m3u

m3u 文件内容:
Final Fantasy VII/Final Fantasy VII (Disc 1).cue
Final Fantasy VII/Final Fantasy VII (Disc 2).cue
Final Fantasy VII/Final Fantasy VII (Disc 3).cue

用户交互:
  点击 "Final Fantasy VII.m3u" → 显示 Disc 1 / Disc 2 / Disc 3
  选择 Disc 2 → 启动对应的 .cue 文件
```

存档关联：每个碟的存档槽位文件记录了当时的碟号，恢复存档时自动切换到正确的碟。

### 6.6 存档槽位系统

每个 ROM 有 10 个存档槽位（0-9）：

| 槽位  | 用途     | 触发方式            |
|-----|--------|-----------------|
| 0-7 | 手动存档   | 游戏内菜单选择         |
| 8   | 隐藏默认存档 | 按 A 键启动游戏（普通启动） |
| 9   | 自动恢复存档 | 设备没电/非正常关机后恢复   |

存档文件分为两种：

| 文件     | 路径格式                                               | 内容                 |
|--------|----------------------------------------------------|--------------------|
| 槽位选择文件 | `.userdata/shared/.minui/<EMU>/<ROM文件名>.txt`       | 存储要恢复的槽位号（如 "3"）   |
| 碟号记录文件 | `.userdata/shared/.minui/<EMU>/<ROM文件名>.<槽位号>.txt` | 多碟游戏专用，存储该槽位对应的碟路径 |

`ready_resume_path()` 检查槽位选择文件是否存在来决定是否显示 X RESUME 按钮；
`open_rom()` 读取槽位选择文件 → 根据槽位号查找碟号记录文件 → 切换到正确的碟。

### 6.7 电源管理状态机

```
         ┌──────────────────────────────┐
         │          活跃状态             │
         │   cpu_speed = Menu/Normal    │
         │   处理输入, 渲染, 计时器运行   │
         └──────┬──────────────┬────────┘
                │              │
      30s无操作  │              │ 按电源键
                ▼              ▼
         ┌──────────┐    ┌──────────┐
         │  休眠     │◄───│  手动休眠  │
         │  背光关闭  │    └──────────┘
         │  音频暂停  │
         │  等待唤醒   │
         └────┬───────┘
              │
    2分钟无操作│
              ▼
         ┌──────────┐
         │  自动关机  │
         │  显示提示   │
         │  执行关机   │
         └──────────┘
```

阻止自动休眠的条件（`PWR_preventAutosleep`）：
- 设备正在充电
- 自动休眠被禁用
- 正在 HDMI 输出

### 6.8 名称映射 (map.txt)

任何目录下可以放置 `map.txt` 来重命名条目或隐藏条目：

```
map.txt 格式:
<原始文件名><TAB><新显示名>

例如 Roms/map.txt:
Game Boy (GB)	Nintendo Game Boy
```

映射后的名称如果 `hide()` 返回 true（以 `.` 开头或以 `.disabled` 结尾），条目会被过滤掉。

> **背景：为什么需要 map.txt？** 不同地区的 ROM 发行商对同一游戏主机有不同命名（如 "Famicom" vs "NES"），ROM 文件名也可能包含版本号和 dump 信息。`map.txt` 让用户可以自定义显示名而不改变文件系统结构，也允许通过 `.disabled` 映射来隐藏特定目录而不删除文件。

---

### 6.9 Platform trait —— 支持 20+ 设备的秘诀

```rust
pub trait Platform: Send + Sized {
    // ── 编译时常量（每个平台不同）──
    const FIXED_WIDTH: u32;
    const FIXED_HEIGHT: u32;
    const FIXED_BPP: u8;
    const FIXED_SCALE: u32;
    const SDCARD_PATH: &'static str;
    const PLATFORM_TAG: &'static str;

    // ── 派生常量（由原始常量计算，有默认实现）──
    fn fixed_pitch() -> u32 { Self::FIXED_WIDTH * Self::FIXED_BPP as u32 }
    fn fixed_size()  -> u32 { Self::fixed_pitch() * Self::FIXED_HEIGHT }
    fn fixed_depth() -> u32 { Self::FIXED_BPP as u32 * 8 }

    // ── 按键映射（每个平台不同的物理键值）──
    const KEY_UP: i32;
    const KEY_DOWN: i32;
    const KEY_LEFT: i32;
    const KEY_RIGHT: i32;
    // ... 16+ 按键，可选按键有默认值 NA(-1)

    // ── 平台能力查询 ──
    fn has_power_button(&self) -> bool { Self::KEY_POWER != NA }
    fn has_menu_button(&self) -> bool { Self::KEY_MENU != NA }

    // ── 运行时方法 ──
    fn init_video(&mut self) -> Result<Framebuffer, String>;
    fn poll_input(&mut self, pad: &mut PadContext);
    fn flip(&mut self, fb: &Framebuffer, sync: bool);
    fn power_off(&self) -> !;
    // ... 30+ 方法
}
```

**为什么用关联常量而非字段**：`FIXED_WIDTH` 等值在编译时就已确定（每个平台的屏幕分辨率是不变的），用关联常量可以让编译器做常量折叠和内联优化。而运行时方法如 `poll_input` 需要访问平台的状态（文件描述符、mmap 地址），所以是 `&mut self` 方法。

**TestPlatform**：在测试中使用的平台实现，所有视频操作都在堆分配的 `Vec<u8>` 上进行，不依赖任何硬件：
```rust
struct TestPlatform { fb: Vec<u8>, battery_charging: bool, battery_level: u8, ... }
impl Platform for TestPlatform {
    fn init_video(&mut self) -> Result<Framebuffer, String> {
        Ok(Framebuffer { pixels: self.fb.as_mut_ptr(), width: 640, height: 480, pitch: 1280, bpp: 2 })
    }
    fn flip(&mut self, _fb: &Framebuffer, _sync: bool) { /* 内存操作，不需要硬件 */ }
    // ...
}
```


### 6.10 Framebuffer —— 原始帧缓冲的抽象

```rust
// C: SDL_Surface { pixels, w, h, pitch, format->BytesPerPixel }
struct Framebuffer {
    pixels: *mut u8,   // 指向 mmap 映射的 ION 内存或堆分配的测试缓冲
    width: u32,        // 像素宽度
    height: u32,       // 像素高度
    pitch: u32,        // 每行字节数（可能 > width * bpp，硬件对齐要求）
    bpp: u8,           // 每像素字节数（2=RGB565, 4=RGBA8888）
}
```

**pitch vs width * bpp**：硬件 framebuffer 的每行可能有额外的填充字节以满足对齐要求（如 32 字节对齐）。例如 638 像素宽的 RGB565 显示，`width * 2 = 1276`，但硬件可能要求 `pitch = 1280`。所有像素寻址操作必须使用 `y * pitch + x * bpp`，而非 `y * width * bpp + x * bpp`。

**双缓冲布局**：
```
ION 分配: [PAGE_0: 640×480×2 = 614400 bytes][PAGE_1: 614400 bytes]
              ↑ 当前显示                         ↑ 后台绘制

flip 操作: 交换硬件寄存器 DE_OVL_BA0 指向另一个 PAGE
          page ^= 1  (前台 ↔ 后台)
          pixels = fb_vaddr + page * PAGE_SIZE
```


### 6.11 get_root() —— 最复杂的扫描函数

此函数构建 minui 启动时看到的主屏幕。以下为算法级伪代码（非实际 Rust 代码），展示完整的处理流程：

```
get_root(sdcard, platform_tag, paks, has_recents, has_collections, simple_mode):
  root = []

  // 步骤 1：最近游戏
  if has_recents:
    root.push(Entry_new(FAUX_RECENT_PATH, ENTRY_DIR))
    // FAUX_RECENT_PATH = "/mnt/sdcard/Recently Played" (伪目录，不实际存在)

  // 步骤 2：扫描 Roms/ 下所有子目录
  entries = []
  for each dir in Roms/:
    if hide(dir.name): continue     // 跳过 .hidden 和 .disabled
    if !hasRoms(dir.name): continue // 检查①有模拟器②至少一个ROM文件
    entries.push(Entry_new(dir.path, ENTRY_DIR))

  // 步骤 3：排序 + 去重（同名目录只保留第一个）
  entries.sort_by(name, case_insensitive)
  prev = None
  for each entry in entries:
    if entry.name == prev.name: skip  // 去重
    prev = entry

  // 步骤 4：应用 Roms/map.txt（名称映射）
  if Roms/map.txt exists:
    load map {原始目录名 → 新名称}
    apply to entries; resort

  // 步骤 5：Collections
  if Collections/ has visible files:
    if entries is not empty:
      root.push(Entry_new(COLLECTIONS_PATH, ENTRY_DIR)) // 作为子目录
    else:
      promote_collections_to_root() // 没有系统时把收藏提升到根

  // 步骤 6：添加到 root
  root.append(entries)

  // 步骤 7：Tools
  if !simple_mode and Tools/<PLATFORM>/ exists:
    root.push(Entry_new(tools_path, ENTRY_DIR))

  return root
```


### 6.12 open_rom() —— 游戏启动的完整流程

```
open_rom(state, path, last, alias, sdcard, platform_tag, paks):
  // 1. 多碟游戏处理
  m3u = find_m3u(path)          // 向上两级查找 .m3u 文件
  recent_path = m3u or path      // 最近游戏记录用 m3u 路径（跨碟统一）
  if path is .m3u:
    path = get_first_disc(m3u)   // 取第一张碟作为启动路径

  // 2. 确定模拟器
  emu_name = get_emu_name(path, roms_path)  // e.g. "GB", "PS"
  // get_emu_name 算法：路径在 ROMS_PATH 下 → 取 Roms 子目录名 → 取括号内标签

  // 3. 存档恢复分支（用户按了 X 键）
  if state.should_resume:
    slot = read_file(slot_path)              // 读取存档槽位号
    put_file(RESUME_SLOT_PATH, slot)         // 写入 /tmp/resume_slot.txt
    if has_m3u:
      disc_slot_path = "{shared}/.minui/{emu}/{rom}.{slot}.txt"
      if disc_slot_path exists:
        disc_path = read_file(disc_slot_path) // 读取当时的碟号
        if disc_path is absolute:
          path = disc_path                    // 绝对路径直接用
        else:
          path = m3u_dir + "/" + disc_path    // 相对路径拼接
  else:
    put_int(RESUME_SLOT_PATH, 8)             // 默认槽位 8

  // 4. 找到模拟器路径
  emu_path = get_emu_path(emu_name, sdcard, platform_tag, paks)
  // 先找 <SDCARD>/Emus/<PLATFORM>/<emu>.pak/launch.sh
  // 再找 <PAKS_PATH>/Emus/<emu>.pak/launch.sh

  // 5. 加入最近游戏 + 保存位置
  add_recent(recent_path, alias)
  save_last(last or path)

  // 6. 构造 shell 命令 + 退出
  cmd = "'{emu_path}' '{path}'"
  queue_next(cmd)  // 写 /tmp/next, quit=1
```


### 6.13 open_directory() —— 目录导航（含自动启动）

```
open_directory(state, path, auto_launch, sdcard, platform_tag, paks):
  // 自动启动检测（仅 auto_launch=true 时）
  if auto_launch:
    // PS1 游戏：检查目录下是否有同名 .cue 文件
    if cue = find_cue(path):
      save_last(path); open_rom(cue, last=path); return

    // 多碟游戏：检查上级目录是否有同名 .m3u 文件
    m3u = "{parent}/{dirname}.m3u"
    if m3u exists:
      first_disc = get_first_disc(m3u)
      save_last(path); open_rom(first_disc, last=path); return

  // 恢复状态（从 closeDirectory 保存的）
  if restore_depth == stack.len() and top.selected == restore_relative:
    selected = restore_selected; start = restore_start; end = restore_end

  // 创建新目录 + 压栈
  entries = get_entries_for_path(path, sdcard, platform_tag, paks, recents, simple_mode)
  // get_entries_for_path 是分派器：
  //   path==SDCARD → get_root()
  //   path==FAUX_RECENT → get_recents_from_list()
  //   在 Collections 下的 .txt → get_collection()
  //   .m3u → get_discs()
  //   其他 → get_entries()

  dir = make_directory(path, entries, selected)  // 建立字母索引 + 同名处理
  dir.start = start; dir.end = end
  stack.push(dir)  // top = dir
```


### 6.14 handle_launcher_input() —— 导航输入的完整处理

```
handle_launcher_input(state, pad, now, main_rows, sdcard, platform_tag, paks):
  total = state.total_entries()
  if total == 0: return

  // 提取索引到局部变量（避免借用冲突）
  selected = dir.selected; start = dir.start; end = dir.end

  // UP: selected--; 到顶则跳到末尾
  if pad.just_repeated(UP):
    if selected == 0 && !pad.just_pressed(UP):  // 已在顶，stop
    elif selected == 0: selected=total-1; start=total-main_rows; end=total  // wrap
    else: selected--; if selected<start: start--; end--

  // DOWN: selected++; 到底则跳到开头（对称逻辑）

  // LEFT: 翻页上 (selected -= main_rows)
  // RIGHT: 翻页下 (selected += main_rows)

  // L1: 跳转到上一个字母组
  alpha = entries[selected].alpha
  if alpha > 0: selected = alphas[alpha-1]; 调整窗口

  // R1: 跳转到下一个字母组

  // 写回索引
  dir.selected = selected; dir.start = start; dir.end = end

  // 存档恢复检测（每次 dirty 变动时）
  if dirty and total > 0:
    entry = entries[selected]
    ready_resume(entry)  // 检测是否有存档 → 设置 can_resume

  // X键：从存档恢复
  if total > 0 and can_resume and pad.just_released(X):
    should_resume = true; entry_open(entry)

  // A键：打开条目
  if total > 0 and pad.just_pressed(A):
    entry_open(entry)

  // B键：返回上级（非根目录时）
  if pad.just_pressed(B) and stack.len() > 1:
    close_directory()
    // 返回后也检测新选中项的存档
    if total > 0: ready_resume(entries[selected])
```


### 6.15 run() —— 主事件循环

```
run(platform, renderer, power, sdcard, platform_tag, paks) -> Result<bool, String>:
  // 0. 自动恢复
  if auto_resume(sdcard, platform_tag, paks): return Ok(false)

  // 1. 初始化
  simple_mode = exists(enable-simple-mode文件)
  fb = platform.init_video()          // 打开 /dev/fb0 → mmap → Framebuffer
  platform.init_input()               // 打开 /dev/input/event*
  power.initialized = true
  if !has_power_button && !simple_mode: power.disable_sleep()
  init_menu(sdcard, platform_tag, paks) // 扫描目录 + 加载最近游戏 + 恢复位置
  platform.set_cpu_speed(CPU_SPEED_MENU)
  platform.set_vsync(VSYNC_STRICT)

  // 2. 主循环
  pad = PadContext::default()
  was_online = platform.is_online()
  while !quit && !power.poweroff_requested:
    // ── 输入 ──
    platform.poll_input(&mut pad)

    // ── 电源 ──
    dt_ms = elapsed since last frame (clamped to 100ms)
    power.update(dt_ms)                    // 设置提示计时器 + 休眠关机计时器
    if !power.is_asleep && !pad.any_pressed():
      if power.prevent_autosleep(has_hdmi):
        power.notify_activity()             // 重置空闲计时器（充电/禁用/HDMI）
      elif power.check_autosleep(dt_ms):
        dirty = true                        // 触发休眠
    if pad.any_just_pressed():
      power.notify_activity()               // 唤醒

    // ── 网络 ──
    if platform.is_online() != was_online: dirty = true

    // ── 亮度/音量 ──
    power.handle_setting_input(&pad, MENU, NONE, PLUS, MINUS)

    // ── 版本界面 ──
    if MENU tapped: show_version = !show_version; dirty = true
    if show_version:
      if B pressed or MENU tapped: show_version = false; dirty = true
    else:
      handle_launcher_input(...)

    // ── 渲染 ──
    if dirty && !power.is_asleep:
      render_frame(platform, renderer, power, &mut fb, sdcard, main_rows)
      platform.flip(&fb, true)             // 翻页 + VSync 等待
      dirty = false
    elif !power.is_asleep:
      platform.vsync_wait(0)               // 保活 VSync 节奏

    // ── 帧率控制 ──
    控制帧时间在 16ms（60fps）

    // ── HDMI ──
    if platform.hdmi_changed(): save_last(); sleep(4); quit

  // 3. 退出
  if power.poweroff_requested: platform.power_off()  // 不返回
  quit_menu(); platform.quit_input(); platform.quit_video()
  return Ok(true)
```

关键时序：每帧约 16ms（60fps）。`dt_ms` 被 clamp 到 100ms 以下防止跳帧导致休眠计时器异常快进。


### 6.16 fb_draw_text() —— 软件文字光栅化

```rust
fn fb_draw_text(fb, text, rect, color, font, px):
  cursor_x = rect.x
  baseline_y = rect.y + rect.h / 2
  total_h = measure text height
  y_offset = (rect.h - total_h) / 2  // 垂直居中

  for each char in text:
    // fontdue 光栅化：返回 (metrics, bitmap)
    // metrics: advance_width, height, width, xmin, ymin
    // bitmap: [u8] 每字节 0-255 alpha 值，width×height 布局
    (metrics, bitmap) = font.rasterize(char, px)

    glyph_top = rect.y + y_offset + (baseline_y - rect.y - metrics.height)
    for gy in 0..metrics.height:
      for gx in 0..metrics.width:
        alpha = bitmap[gy * metrics.width + gx]
        if alpha > 0:
          // Alpha 混合到 framebuffer
          existing = fb.pixels[screen_y * fb.pitch + screen_x * 2]
          blended = blend_rgb565(existing, color.0, alpha)
          fb.pixels[screen_y * fb.pitch + screen_x * 2] = blended

    cursor_x += metrics.advance_width
    if cursor_x >= rect.x + rect.w: break  // 超出区域停止
```

**为什么选择 fontdue？** 原 C 版依赖 SDL_ttf → FreeType，这是一个 C 库依赖链，需要在目标 ARM 设备上交叉编译三个动态库。fontdue 是纯 Rust 实现，`no_std` 兼容，无需任何 C 依赖。在桌面端通过 `cargo test` 直接测试字体渲染，在 ARM 端通过 cross-compilation 直接链接进二进制。字形质量与 FreeType 相当，但 API 更简洁——直接返回 `(Metrics, Bitmap)` 元组。

**blend_rgb565 算法**：将 RGB565 拆分为 R(5bit) G(6bit) B(5bit)，各自与 alpha 混合：
```rust
// bg, fg: u16 RGB565; alpha: u8 (0-255)
r = ((fg_r * alpha + bg_r * (255 - alpha)) / 255) & 0x1F
g = ((fg_g * alpha + bg_g * (255 - alpha)) / 255) & 0x3F
b = ((fg_b * alpha + bg_b * (255 - alpha)) / 255) & 0x1F
result = (r << 11) | (g << 5) | b
```


### 6.17 PowerManager —— 电源管理状态机

```rust
struct PowerManager {
    is_asleep: bool,              // 当前休眠状态
    sleep_disabled: bool,         // 禁止所有休眠（MENU 键兼任休眠键时）
    autosleep_disabled: bool,     // 禁止自动休眠但允许手动
    idle_time_ms: u32,            // 距上次用户输入的时间
    sleep_time_ms: u32,           // 休眠持续时间（用于自动关机倒计时）
    autosleep_timeout_ms: u32,    // 30_000ms
    autopoweroff_timeout_ms: u32, // 120_000ms
    battery_charge: u8,           // 0/10/20/40/60/80/100
    battery_charging: bool,
    brightness: u8,               // 0-10
    volume: u8,                   // 0-20
    show_setting: u8,             // 0=无, 1=亮度调整中, 2=音量调整中
    setting_display_timer: u32,   // 剩余显示时间（500ms 后自动隐藏）
    poweroff_requested: bool,     // 关机请求标志
    poweroff_disabled: bool,
}

// 每帧调用
fn update(&mut self, dt_ms: u32) -> bool:
  dirty = false
  if setting_display_timer > 0:
    setting_display_timer -= dt_ms
    if setting_display_timer <= 0:
      setting_display_timer = 0; show_setting = 0; dirty = true
  if is_asleep && !poweroff_disabled:
    sleep_time_ms += dt_ms
    if sleep_time_ms >= autopoweroff_timeout_ms:
      poweroff_requested = true
  dirty

// 阻止自动休眠的条件
fn prevent_autosleep(&self, has_hdmi: bool) -> bool:
  battery_charging || autosleep_disabled || has_hdmi
```

**为什么充电时阻止自动休眠**：用户在充电时可能正在下载 ROM 或等待充电完成，不应自动休眠。这是原 C 代码 `PWR_preventAutosleep()` 的语义。

---


## 7. 原 C 代码到 Rust 的完整映射

### 7.1 源文件映射

| 原 C 文件              | Rust 文件                          | 内容                                             |
|---------------------|----------------------------------|------------------------------------------------|
| `minui.c:16-208`    | `types.rs`                       | 数据结构 (Array, Hash, Entry, Directory, Recent)   |
| `minui.c:214-323`   | `scan.rs` → `make_directory()`   | 目录索引 (Directory_index)                         |
| `minui.c:324-942`   | `scan.rs`                        | 文件扫描 (getRoot, getEntries, getRecents 等)       |
| `minui.c:946-1208`  | `launch.rs`                      | 游戏启动 (queueNext, openRom, openPak, autoResume) |
| `minui.c:1212-1296` | `state.rs`                       | 位置持久化 + 菜单生命周期                                 |
| `minui.c:1300-1705` | `state.rs` → `run()`             | 主事件循环                                          |
| `defines.h`         | `types.rs` + `paths.rs`          | 枚举、路径宏、颜色常量                                    |
| `platform.h` (各平台)  | `platform.rs` → `Platform` trait | 平台关联常量                                         |
| `api.h`             | `platform.rs` → `Platform` trait | 平台方法签名                                         |
| `api.c`             | `render.rs` + `power.rs`         | 渲染和电源管理                                        |
| `utils.c`           | `utils.rs`                       | 字符串工具和文件 I/O                                   |

### 7.2 完整函数映射表

#### 数据结构 (C → Rust)

| C 类型/函数                            | Rust 等价                   | 备注           |
|------------------------------------|---------------------------|--------------|
| `Array` + `Array_*` 函数族            | `Vec<T>`                  | 标准库提供所有操作    |
| `Hash` (线性查找的 KV 对)                | `HashMap<String, String>` | O(1) 替代 O(n) |
| `IntArray` (定长 27 数组)              | `Vec<usize>`              | 不再限制最大 27    |
| `Entry` + `Entry_new/Free`         | `Entry` struct + `Clone`  | 所有权自动管理      |
| `Directory` + `Directory_new/Free` | `Directory` struct        | 所有权自动管理      |
| `Recent` + `Recent_new/Free`       | `Recent` struct           | 所有权自动管理      |

#### 工具函数 (C → Rust)

| C 函数                   | Rust 函数                  | 差异                                 |
|------------------------|--------------------------|------------------------------------|
| `prefixMatch`          | `prefix_match`           | `.eq_ignore_ascii_case()`          |
| `suffixMatch`          | `suffix_match`           | 同上                                 |
| `exactMatch`           | `exact_match`            | `==` 自动优化                          |
| `containsString`       | `contains_string`        | `.to_ascii_lowercase().contains()` |
| `hide`                 | `hide`                   | 完全等价                               |
| `getDisplayName`       | `get_display_name`       | 完全等价                               |
| `getEmuName`           | `get_emu_name`           | 完全等价                               |
| `getEmuPath`           | `get_emu_path`           | 完全等价                               |
| `normalizeNewline`     | `normalize_newline`      | 完全等价                               |
| `trimTrailingNewlines` | `trim_trailing_newlines` | `str::trim_end_matches`            |
| `trimSortingMeta`      | `skip_sorting_meta`      | 返回切片而非就地修改                         |
| `exists`               | `path_exists`            | `Path::exists()`                   |
| `putFile` / `getFile`  | `put_file` / `get_file`  | `fs::write` / `fs::read_to_string` |
| `putInt` / `getInt`    | `put_int` / `get_int`    | 完全等价                               |
| `allocFile`            | `alloc_file`             | 返回 `Option<String>`                |

#### 扫描函数 (C → Rust)

| C 函数                                | Rust 函数                 | 行数      |
|-------------------------------------|-------------------------|---------|
| `hasEmu`                            | `has_emu`               | scan.rs |
| `hasCue`                            | `find_cue`              | scan.rs |
| `hasM3u`                            | `find_m3u`              | scan.rs |
| `hasRoms`                           | `has_roms`              | scan.rs |
| `addEntries`                        | `scan_dir`              | scan.rs |
| `getEntries`                        | `get_entries`           | scan.rs |
| `getRoot`                           | `get_root`              | scan.rs |
| `getRecents`                        | `get_recents_from_list` | scan.rs |
| `getCollection`                     | `get_collection`        | scan.rs |
| `getDiscs`                          | `get_discs`             | scan.rs |
| `getFirstDisc`                      | `get_first_disc`        | scan.rs |
| `isConsoleDir`                      | `is_console_dir`        | scan.rs |
| `Directory_new` + `Directory_index` | `make_directory`        | scan.rs |
| `hasRecents`                        | `load_recents`          | scan.rs |

#### 启动函数 (C → Rust)

| C 函数                                   | Rust 函数                | 行数        |
|----------------------------------------|------------------------|-----------|
| `queueNext`                            | `queue_next`           | launch.rs |
| `escapeSingleQuotes` + `replaceString` | `escape_single_quotes` | launch.rs |
| `autoResume`                           | `auto_resume`          | launch.rs |
| `readyResumePath`                      | `ready_resume_path`    | launch.rs |
| `readyResume`                          | `ready_resume`         | launch.rs |
| `openPak`                              | `open_pak`             | launch.rs |
| `openRom`                              | `open_rom`             | launch.rs |
| `Entry_open`                           | `entry_open`           | launch.rs |

#### 导航/状态 (C → Rust)

| C 函数/变量          | Rust 方法             | 所属       |
|------------------|---------------------|----------|
| `top` (全局指针)     | `stack.last()`      | state.rs |
| `saveLast`       | `save_last`         | state.rs |
| `loadLast`       | `load_last`         | state.rs |
| `saveRecents`    | `save_recents`      | state.rs |
| `addRecent`      | `add_recent_direct` | state.rs |
| `Menu_init`      | `init_menu`         | state.rs |
| `Menu_quit`      | `quit_menu`         | state.rs |
| `openDirectory`  | `open_directory`    | state.rs |
| `closeDirectory` | `close_directory`   | state.rs |
| `main()`         | `run()`             | state.rs |

#### 渲染/电源 (C → Rust)

| C 函数                                   | Rust 函数                                   | 所属                      |
|----------------------------------------|-------------------------------------------|-------------------------|
| `GFX_blitPill`                         | `fb_draw_pill`                            | render.rs               |
| `GFX_blitBattery`                      | `fb_draw_battery`                         | render.rs               |
| `GFX_blitButton`                       | `fb_draw_button_hint`                     | render.rs               |
| `GFX_blitText`                         | `fb_draw_text`                            | render.rs               |
| `GFX_blitMessage`                      | `draw_message`                            | render.rs (UiRenderer)  |
| `GFX_truncateText`                     | `truncate_text`                           | render.rs (FontManager) |
| `GFX_blitHardwareGroup`                | `draw_hardware_status`                    | render.rs (UiRenderer)  |
| `GFX_blitButtonGroup`                  | `draw_button_group`                       | render.rs (UiRenderer)  |
| `PWR_init` / `PWR_update`              | `PowerManager::new()` / `update()`        | power.rs                |
| `PWR_disableSleep` / `PWR_enableSleep` | `disable_sleep()` / `enable_sleep()`      | power.rs                |
| `PWR_preventAutosleep`                 | `prevent_autosleep`                       | power.rs                |
| 休眠/唤醒逻辑                                | `enter_sleep()` / `wake()`                | power.rs                |
| 亮度/音量调节                                | `adjust_brightness()` / `adjust_volume()` | power.rs                |


## 8. Rust 重写的优化与改进

### 8.1 内存安全

| 原 C 问题                                     | Rust 解决                         |
|--------------------------------------------|---------------------------------|
| `malloc`/`free` 配对错误 → 内存泄漏/use-after-free | 所有权系统自动管理，`Drop` 保证释放           |
| `char*` 缓冲区溢出 (`strcpy`, `sprintf`)        | `String` 自动扩容，`format!()` 编译时检查 |
| NULL 指针解引用                                 | `Option<T>` 强制处理 None 情况        |
| 悬垂指针（`top` 指向 `stack` 中的元素）                | 借用检查器保证引用有效性                    |
| 全局可变状态竞态                                   | `&mut self` 保证单线程独占访问           |

### 8.2 类型安全

| 原 C                       | Rust                 | 改进        |
|---------------------------|----------------------|-----------|
| `int type` (magic number) | `EntryType` enum     | 编译时穷举检查   |
| `int mode` (0/1)          | `RenderMode` enum    | 语义化       |
| `#define BUTTON_NA -1`    | `const NA: i32 = -1` | 类型化常量     |
| 位掩码 `uint32_t`            | `Button(u32)` 含方法    | 封装 + 类型区分 |
| `void*` 泛型容器              | `Vec<T>` 带单态化        | 编译时类型检查   |

### 8.3 可测试性

| 原 C           | Rust                                |
|---------------|-------------------------------------|
| 需要实际硬件或完整模拟环境 | `TestPlatform` 在普通电脑上运行             |
| 全局变量使测试相互干扰   | `MinUi::new()` 每次创建独立状态             |
| 测试依赖 SD 卡文件系统 | `setup_test_dirs()` 在 `/tmp` 创建隔离环境 |
| 无单元测试框架       | `#[test]` + `cargo test`，57 个测试     |

### 8.4 代码质量

| 指标      | 原 C                            | Rust                   |
|---------|--------------------------------|------------------------|
| 手写容器代码  | ~100 行 (Array, Hash, IntArray) | 0 行 (Vec, HashMap)     |
| 内存管理代码  | 分散在 20+ 个 `Free` 函数            | 0 行 (自动 Drop)          |
| NULL 检查 | 7 处隐式（依赖约定）                    | 全部由 `Option` 显式处理      |
| 未定义行为风险 | 指针算术、buffer overflow           | unsafe 块封装在 platform 层 |
| 编译时保证   | 无                              | 借用检查、穷举匹配、trait bound  |

### 8.5 架构改进

- **全局状态封装**：原 C 有 12 个 `static` 变量分散在 minui.c 中，Rust 统一在 `MinUi` 结构体
- **纯函数优先**：扫描函数不再依赖全局状态，接收参数返回结果，可独立测试
- **扩展 trait 模式**：渲染函数通过自由函数操作 `Framebuffer`，不侵入 platform 定义
- **direct 版本路径函数**：`slot_path_direct()` 等非泛型版本便于测试，避免不必要的 trait bound 传染

### 8.6 渲染层的差异

| 方面       | 原 C (SDL)                              | Rust (fontdue)            |
|----------|----------------------------------------|---------------------------|
| 依赖       | SDL 1.2 + SDL_ttf + SDL_image (3 个动态库) | fontdue (1 个纯 Rust crate) |
| 字体       | FreeType 通过 SDL_ttf                    | 直接光栅化                     |
| UI 精灵    | `assets@2x.png` 图片文件                   | 纯代码绘制（圆角矩形、电池等）           |
| 像素格式     | SDL 自动转换                               | 直接写入 RGB565               |
| Alpha 混合 | SDL 内置                                 | 自定义实现                     |
| 平台依赖     | 需要目标平台有 SDL                            | 零外部 C 依赖                  |

---

## 9. 如何新增一个平台

以 Anbernic RG35XX 为例，新增平台的步骤：

### 9.1 添加 feature flag

```toml
# Cargo.toml
[features]
platform-rg35xx = []
```

### 9.2 实现 Platform trait

```rust
// src/platform/rg35xx.rs (或直接在 platform.rs 中)

#[cfg(feature = "platform-rg35xx")]
pub struct Rg35xxPlatform {
    fb_fd: i32,
    ion_fd: i32,
    de_mem: *mut u32,
    // ... 平台私有状态
}

#[cfg(feature = "platform-rg35xx")]
impl Platform for Rg35xxPlatform {
    // ── 屏幕参数 ──
    const FIXED_WIDTH: u32 = 640;
    const FIXED_HEIGHT: u32 = 480;
    const FIXED_BPP: u8 = 2;
    const FIXED_SCALE: u32 = 2;

    // ── 路径 ──
    const SDCARD_PATH: &'static str = "/mnt/sdcard";
    const PLATFORM_TAG: &'static str = "rg35xx";

    // ── 按键映射 (SDL keycodes) ──
    const KEY_UP: i32 = SDLK_KATAKANA;
    const KEY_DOWN: i32 = SDLK_HIRAGANA;
    const KEY_A: i32 = SDLK_MUHENKAN;
    // ... 其他按键

    // ── 运行时方法 ──
    fn init_video(&mut self) -> Result<Framebuffer, String> {
        // 1. open("/dev/fb0")
        // 2. ioctl(FBIOGET_VSCREENINFO)
        // 3. open("/dev/ion") → ion_alloc()
        // 4. mmap() → Framebuffer
    }

    fn poll_input(&mut self, pad: &mut PadContext) {
        // 读取 /dev/input/event* → 更新 pad 状态
    }

    fn get_battery_status(&self) -> (bool, u8) {
        // 读取 /sys/class/power_supply/battery/voltage_now
        // 读取 /sys/class/power_supply/battery/charger_online
    }

    // ... 其他方法
}
```

### 9.3 平台特有的硬件交互

每个平台需要处理的底层细节：

| 子系统    | 接口                                                      | 典型操作                            |
|--------|---------------------------------------------------------|---------------------------------|
| 视频     | `/dev/fb0` + ION + DE 寄存器                               | mmap, ioctl, 双缓冲翻页              |
| 输入     | `/dev/input/event*`                                     | evdev read, keycode → Button 映射 |
| 电池     | `/sys/class/power_supply/battery/*`                     | 读取电压/电流/充电状态                    |
| 背光     | `/sys/class/backlight/*/bl_power`                       | 写 0/4 控制开关                      |
| 音频     | ALSA (通过 libmsettings)                                  | SetVolume/GetVolume             |
| CPU 频率 | `/sys/devices/system/cpu/cpu0/cpufreq/scaling_setspeed` | 写频率值                            |
| 关机     | `system("shutdown")` 或 GPIO                             | 执行系统关机命令                        |

### 9.4 验证清单

- [ ] 屏幕能正常显示（framebuffer 地址和 pitch 正确）
- [ ] 所有按键能正确映射和响应
- [ ] 电池电量正确读取和显示
- [ ] 休眠/唤醒正常工作
- [ ] 亮度/音量调节生效
- [ ] SD 卡路径正确
- [ ] 字体能正常渲染
- [ ] HDMI 输出切换正常（如果支持）

---

## 10. 测试策略与覆盖

### 10.1 测试分类

```
57 单元测试 + 2 doc tests = 59 total

类型              数量    模块              测试内容
────────────────────────────────────────────────────
数据结构            5     types              Button 位运算, Directory 属性
路径                1     paths              SD 卡路径拼接正确性
字符串工具         16     utils              匹配函数, 显示名提取, 文件 I/O
────────────────────────────────────────────────────
文件系统扫描       10     scan               目录扫描, 根目录, M3U, 多碟,
                                            名称映射, 字母索引
────────────────────────────────────────────────────
游戏启动            2     launch             Shell 转义
菜单导航            7     state              最近游戏, 目录导航往返,
                                            输入导航, 位置恢复
────────────────────────────────────────────────────
渲染                6     render             RGB565 转换, 填充/圆角,
                                            Alpha 混合, 清屏
电源管理           10     power              休眠计时器, 自动关机,
                                            亮度/音量调节, 边界值
────────────────────────────────────────────────────
总计               57
```

### 10.2 测试基础设施

```rust
// TestPlatform — 零硬件依赖的模拟平台
let mut platform = TestPlatform::new();  // 640x480 RGB565, 内存 framebuffer
let mut minui = MinUi::new();
let renderer = UiRenderer::with_default_font(2, 640, 480);
let mut power = PowerManager::new();

// 可独立运行
minui.init_menu("/tmp/test_sdcard", "test", "/tmp/test_sdcard/.system/test/paks");
assert!(minui.total_entries() > 0);
```

每个测试函数创建独立的临时目录（`/tmp/minui_scan_test_<name>_<id>`），使用原子计数器确保并行测试互不干扰。

---

## 11. 当前模块架构

```
minui-rs/
├── Cargo.toml              ← 12 个平台 feature flags + fontdue 依赖
├── README.md               ← 完整报告（本文档）
├── resources/
│   └── BPreplayBold-unhinted.otf  ← MinUI 官方字体 (169KB)
└── src/
    ├── lib.rs              ← crate 根，10 个模块 + 重导出
    ├── types.rs            ← 核心数据类型 (330 行, 5 tests)
    │   ├── EntryType, Entry, Directory, Recent
    │   ├── Button, ButtonId, PadContext, Axis
    │   ├── Color, RenderMode, CpuSpeed, ScaleMode
    │   └── Sharpness, ScreenEffect, VsyncMode
    ├── platform.rs         ← Platform trait (370 行)
    │   ├── Framebuffer (mmap 抽象)
    │   ├── Platform trait (13 组常量 + 30+ 方法)
    │   └── TestPlatform (测试用)
    ├── paths.rs            ← SD 卡路径派生 (190 行, 1 test)
    │   ├── roms_path, system_path, userdata_path
    │   ├── recent_path, auto_resume_path, slot_path
    │   └── _direct 非泛型版本
    ├── utils.rs            ← 字符串 + 文件 I/O (380 行, 25 tests)
    │   ├── 匹配: prefix_match, suffix_match, hide 等
    │   ├── 显示名: get_display_name, get_emu_name, get_emu_path
    │   ├── 清理: normalize_newline, skip_sorting_meta
    │   └── I/O: put_file, get_file, put_int, get_int
    ├── scan.rs             ← 文件系统扫描 (1010 行, 10 tests)
    │   ├── 模拟器: has_emu, find_cue, find_m3u
    │   ├── 扫描: scan_dir, get_entries, get_root
    │   ├── 特殊: get_collection, get_discs, get_first_disc
    │   ├── 构造: make_directory (含字母索引和同名处理)
    │   └── 持久化: load_recents
    ├── launch.rs           ← 游戏启动 (310 行, 2 tests)
    │   ├── 转义: escape_single_quotes
    │   ├── 启动: open_rom, open_pak, entry_open
    │   └── 恢复: auto_resume, ready_resume
    ├── state.rs            ← 状态机 + 主循环 (820 行, 8 tests)
    │   ├── 导航: open_directory, close_directory
    │   ├── 持久化: save_last, load_last, save_recents
    │   ├── 输入: handle_launcher_input
    │   ├── 渲染: render_frame
    │   └── 主循环: run()
    ├── render.rs           ← 软件渲染器 (860 行, 6 tests)
    │   ├── 类型: Rgb565, Rect, FontSize
    │   ├── 字体: FontManager (fontdue 封装)
    │   ├── 原语: fb_clear, fb_fill_rect, fb_draw_pill, fb_draw_text
    │   ├── UI: UiRenderer (列表, 版本, 电池, 按钮, 状态栏)
    │   └── 混合: blend_rgb565 (Alpha compositing)
    └── power.rs            ← 电源管理 (260 行, 10 tests)
        ├── 状态机: update, check_autosleep, prevent_autosleep
        ├── 控制: enter_sleep, wake, notify_activity
        └── 调节: adjust_brightness, adjust_volume
```

---

## 12. 分步重写记录

### 第一步完成 —— 数据结构和平台 trait 的定义

#### 新增文件

```
minui-rs/
├── Cargo.toml          ← 12 个平台 feature flags，编译时选择
└── src/
    ├── lib.rs          ← crate 根，模块声明 + 重导出
    ├── types.rs        ← 核心数据结构 (320 行)
    ├── platform.rs     ← Platform trait + TestPlatform (340 行)
    ├── state.rs        ← MinUi 状态机 (290 行)
    └── paths.rs        ← SD 卡路径常量 (160 行)
```

#### 各文件与原 C 代码的映射

| Rust 文件       | 对应原 C                                 | 核心内容                                                                    |
|---------------|---------------------------------------|-------------------------------------------------------------------------|
| `types.rs`    | `minui.c:16-208` + `defines.h:36-206` | `Entry`, `Directory`, `Recent`, `Button`, `PadContext`, `Color` 等所有数据结构 |
| `platform.rs` | `platform.h` + `api.h` 全部 `PLAT_*` 声明 | `Platform` trait：13 组关联常量 + 30+ 个方法，覆盖视频、输入、音频、电源、覆盖层                   |
| `state.rs`    | `minui.c:422-436`（全部 static 变量）       | `MinUi` 结构体：导航栈、最近游戏、恢复状态、UI 标志                                         |
| `paths.rs`    | `defines.h:13-33`（全部 `#define` 路径）    | `roms_path()`、`recent_path()`、`slot_path()` 等派生函数                       |

#### Rust 版相对于 C 版的关键改进

1. **所有权替代手动内存管理** —— 不再有 `malloc`/`free`，`Entry_free()` 直接变成 `drop()`
2. **`Option<T>` 替代 NULL** —— `unique: Option<String>` 而非 `char* unique` 的 NULL 检查
3. **`Vec<T>` 替代手写 Array** —— 不再需要 `Array_new`/`Array_push`/`Array_pop`
4. **trait 替代 `#include` 平台头文件** —— 编译时选择平台，运行时多态
5. **所有全局状态在 `MinUi` 结构体中** —— 可构造、可测试、可序列化
6. **`TestPlatform`** —— 能在电脑上运行和测试，不需要实际硬件

准备好进入第二步了吗？第二步是实现**文件系统扫描** —— `add_entries`、`get_root`、`get_recents`、`get_collections`、`get_discs`，以及字符串工具函数（`hide`、`getDisplayName`、`getEmuName` 等）。

---

### 第二步完成 —— 文件系统扫描

#### 新增文件

```
src/
├── utils.rs    (360 行) — 字符串工具 + 文件 I/O
└── scan.rs     (950 行) — 目录扫描 + Directory 构造
```

#### `utils.rs` 包含的函数

| 函数                                           | 对应 C                      | 功能                                       |
|----------------------------------------------|---------------------------|------------------------------------------|
| `prefix_match`                               | `prefixMatch()`           | 大小写不敏感前缀匹配                               |
| `suffix_match`                               | `suffixMatch()`           | 大小写不敏感后缀匹配                               |
| `exact_match`                                | `exactMatch()`            | 大小写敏感精确匹配                                |
| `contains_string`                            | `containsString()`        | 子串搜索                                     |
| `hide`                                       | `hide()`                  | 隐藏文件判断（`.` 开头 / `.disabled` / `map.txt`） |
| `get_display_name`                           | `getDisplayName()`        | 提取显示名（去扩展名、括号、空白）                        |
| `get_display_name_with_platform`             | `getDisplayName` + 平台后缀处理 | 带平台标签的显示名                                |
| `get_emu_name`                               | `getEmuName()`            | 从 ROM 路径提取模拟器标签                          |
| `get_emu_path`                               | `getEmuPath()`            | 构建模拟器启动路径                                |
| `normalize_newline`                          | `normalizeNewline()`      | `\r\n` → `\n`                            |
| `trim_trailing_newlines`                     | `trimTrailingNewlines()`  | 去除行尾换行                                   |
| `skip_sorting_meta`                          | `trimSortingMeta()`       | 跳过 `001) ` 排序前缀                          |
| `path_exists` / `file_exists` / `dir_exists` | `exists()`                | 路径存在性检查                                  |
| `put_file` / `get_file` / `get_file_limited` | `putFile()` / `getFile()` | 文件读写                                     |
| `put_int` / `get_int`                        | `putInt()` / `getInt()`   | 整数的文件读写                                  |
| `alloc_file`                                 | `allocFile()`             | 读取整个文件                                   |
| `file_name` / `parent_dir`                   | —                         | 路径解析辅助                                   |
| `is_pak` / `is_m3u` / `is_cue`               | —                         | 文件类型判断                                   |

#### `scan.rs` 包含的函数

| 函数                      | 对应 C                                    | 功能                  |
|-------------------------|-----------------------------------------|---------------------|
| `has_emu`               | `hasEmu()`                              | 检查模拟器是否可用           |
| `find_cue`              | `hasCue()`                              | 查找 PS1 CUE 文件       |
| `find_m3u`              | `hasM3u()`                              | 查找多碟 M3U 文件         |
| `has_roms`              | `hasRoms()`                             | 检查目录下是否有 ROM + 模拟器  |
| `scan_dir`              | `addEntries()`                          | 扫描目录，构建 Entry 列表    |
| `get_entries`           | `getEntries()`                          | 获取条目（含归类逻辑）         |
| `get_root`              | `getRoot()`                             | 构建根目录               |
| `get_recents_from_list` | `getRecents()`                          | 最近游戏 → Entry 列表     |
| `get_collection`        | `getCollection()`                       | 读取收藏列表              |
| `get_discs`             | `getDiscs()`                            | 读取多碟列表              |
| `get_first_disc`        | `getFirstDisc()`                        | 获取第一张碟              |
| `make_directory`        | `Directory_new()` + `Directory_index()` | 创建 Directory + 字母索引 |
| `load_recents`          | `hasRecents()`                          | 加载最近游戏文件            |

#### 测试覆盖

- **utils**: 25 个测试 — 覆盖所有匹配函数、显示名提取、文件 I/O
- **scan**: 10 个测试 — 覆盖目录扫描、根目录构建、M3U 解析、多碟、名称映射、字母索引
- **总计**: 36 个测试全部通过，零 warning，clippy 干净

#### Rust vs C 的关键差异

| 原 C                                    | Rust                                         |
|----------------------------------------|----------------------------------------------|
| `char*` + `strcpy`/`strrchr`/`sprintf` | `String` + `Path` + `format!()`              |
| `strncasecmp`                          | `.eq_ignore_ascii_case()`                    |
| `opendir`/`readdir`/`closedir`         | `std::fs::read_dir` + Iterator               |
| `FILE*` + `fopen`/`fread`/`fclose`     | `std::fs::read_to_string` / `std::fs::write` |
| `malloc`/`free` 手动管理                   | 所有权自动管理                                      |
| 全局 `static` 变量                         | 纯函数 + 参数传递                                   |

---

### 第三步完成 —— 游戏启动逻辑 & 菜单导航

#### 新增/修改文件

```
src/
├── launch.rs    (新, 280 行) — 游戏启动逻辑
├── state.rs     (重写, 520 行) — 导航方法 + 输入处理
├── scan.rs      (新增 pub 导出 + get_entries_for_path)
└── paths.rs     (新增便捷函数)
```

#### `launch.rs` —— 游戏启动（对应 minui.c 行 946-1208）

| 函数                       | 对应 C                                       | 功能                           |
|--------------------------|--------------------------------------------|------------------------------|
| `escape_single_quotes()` | `escapeSingleQuotes()` + `replaceString()` | Shell 单引号转义                  |
| `queue_next()`           | `queueNext()`                              | 写入 `/tmp/next` 并标记退出         |
| `ready_resume_path()`    | `readyResumePath()`                        | 检测 ROM 是否有存档可恢复              |
| `ready_resume()`         | `readyResume()`                            | 对 Entry 调用的便捷包装              |
| `auto_resume()`          | `autoResume()`                             | 非正常关机后自动恢复游戏                 |
| `open_pak()`             | `openPak()`                                | 启动 Pak（执行 launch.sh）         |
| `open_rom()`             | `openRom()`                                | 启动 ROM（完整流程：模拟器查找、存档恢复、命令构造） |
| `entry_open()`           | `Entry_open()`                             | 按条目类型分发到不同处理逻辑               |

#### `state.rs` 新增方法 —— 导航和输入

| 方法                        | 对应 C                                 | 功能                        |
|---------------------------|--------------------------------------|---------------------------|
| `add_recent_direct()`     | `addRecent()`                        | 非泛型版最近游戏添加                |
| `save_recents()`          | `saveRecents()`                      | 持久化最近游戏到文件                |
| `save_last()`             | `saveLast()`                         | 保存最后位置到 `/tmp/last.txt`   |
| `load_last()`             | `loadLast()`                         | 从 `/tmp/last.txt` 恢复导航位置  |
| `is_in_collection()`      | `prefixMatch(COLLECTIONS_PATH, ...)` | 判断是否在收藏中浏览                |
| `open_directory()`        | `openDirectory()`                    | 打开目录（含 CUE/M3U 自动启动）      |
| `close_directory()`       | `closeDirectory()`                   | 返回上级目录（保存恢复状态）            |
| `init_menu()`             | `Menu_init()`                        | 初始化菜单系统                   |
| `quit_menu()`             | `Menu_quit()`                        | 清理菜单系统                    |
| `handle_launcher_input()` | `main()` 的输入处理段                      | 方向键导航、L1/R1 字母跳转、A/B/X 按钮 |

#### 关键的 Rust 设计决策

**借用检查器的处理** —— 这是本步最大的挑战。原 C 代码通过全局变量随意访问状态，Rust 中必须显式管理借用：

- `handle_launcher_input()` 中复制索引值（`selected`, `start`, `end`）到局部变量，操作完后再写回 `current_dir_mut()`
- `load_last()` 中使用 `let...else` 模式解包并在限定的作用域内完成目录遍历
- `save_restore_state()` 中先在一个 block 中借 `current_dir()` 取出值，再赋值给 `restore_*` 字段

**`selected_entry_cloned()` 而非 `selected_entry() -> &Entry`** —— 因为调用 `selected_entry()` 取得引用后往往需要立即调用 `self` 的可变方法，为避免借用冲突改为返回克隆。

#### 测试覆盖（41 个，全部通过）

新增 5 个测试：
- `test_escape_single_quotes_basic/path` — 命令转义
- `test_init_menu` — 菜单初始化完整流程
- `test_open_and_close_directory` — 目录导航往返
- `test_handle_launcher_input_navigate_down/empty` — 输入导航

#### 当前完整的模块架构

```
src/
├── lib.rs        ← crate 根，8 个模块声明
├── types.rs      ← 核心数据结构（Entry, Directory, Recent, Button, PadContext, Color 等）
├── platform.rs   ← Platform trait（13 组关联常量 + 30+ 方法）+ TestPlatform
├── paths.rs      ← SD 卡路径常量（ROMS_PATH, SYSTEM_PATH, RECENT_PATH 等）
├── utils.rs      ← 字符串工具（匹配、显示名提取、文件 I/O，25 个测试）
├── scan.rs       ← 文件系统扫描（目录遍历、根目录构建、字母索引，10 个测试）
├── launch.rs     ← 游戏启动（存档恢复、Shell 命令、ROM/Pak 打开，2 个测试）
└── state.rs      ← MinUi 状态机 + 导航 + 输入处理（5 个测试）
```

---

### 第四步完成 —— 主事件循环 + 渲染层 + 电源管理

#### 新增文件

```
src/
├── render.rs    (860 行) — 软件 UI 渲染器
├── power.rs     (260 行) — 电源管理状态机
resources/
└── BPreplayBold-unhinted.otf — MinUI 官方字体 (169KB)
```

#### 模块总览

| 模块              | 行数        | 测试数    | 对应原 C                           |
|-----------------|-----------|--------|---------------------------------|
| `types.rs`      | 330       | 5      | `minui.c` 数据结构 + `defines.h` 枚举 |
| `platform.rs`   | 370       | —      | `platform.h` + `api.h` 声明       |
| `paths.rs`      | 190       | 1      | `defines.h` 路径宏                 |
| `utils.rs`      | 380       | 25     | `utils.c`                       |
| `scan.rs`       | 1010      | 10     | `minui.c` 扫描函数                  |
| `launch.rs`     | 310       | 2      | `minui.c` 启动逻辑                  |
| **`render.rs`** | **860**   | **6**  | `api.c` GFX 函数                  |
| **`power.rs`**  | **260**   | **10** | `api.c` PWR 函数                  |
| `state.rs`      | 820       | 8      | `minui.c` 主循环 + 导航              |
| **总计**          | **~4700** | **57** | —                               |

#### `render.rs` 能力矩阵

| 函数                                | 对应 C                                      | 功能                       |
|-----------------------------------|-------------------------------------------|--------------------------|
| `fb_clear`                        | `PLAT_clearVideo`                         | 清空帧缓冲区                   |
| `fb_fill_rect`                    | —                                         | 填充纯色矩形                   |
| `fb_draw_pill`                    | `GFX_blitPill`                            | 圆角矩形（选中项背景）              |
| `fb_draw_text`                    | `GFX_blitText` + `TTF_RenderUTF8_Blended` | 文字渲染（Alpha 混合）           |
| `fb_draw_button_hint`             | `GFX_blitButton`                          | 按钮提示 [A] OPEN            |
| `fb_draw_battery`                 | `GFX_blitBattery`                         | 电池图标 + 填充条 + 充电          |
| `FontManager`                     | SDL_ttf                                   | 字体加载 + 字形光栅化 (`fontdue`) |
| `UiRenderer::render_frame`        | `main()` 渲染段                              | 清屏→状态栏→列表→按钮→翻页          |
| `UiRenderer::draw_list`           | `main()` 列表段                              | 游戏列表（Pill + 文字 + 同名处理）   |
| `UiRenderer::draw_version_screen` | `main()` 版本段                              | 版本/Commit/型号信息           |
| `UiRenderer::draw_message`        | `GFX_blitMessage`                         | 居中消息（如 "Empty folder"）   |

#### `MinUi::run()` 主循环流程图

```
run(platform, renderer, power, sdcard, tag, paks):
  │
  ├─ auto_resume() → 是 → return Ok(false) [直接启动游戏]
  │
  ├─ [init] 视频 → 输入 → 电源 → 菜单 → CPU降频 → VSync
  │
  └─ while !quit && !poweroff:
       │
       ├─ poll_input()
       ├─ power.update(dt) — 休眠/关机计时器
       ├─ power.check_autosleep() — 30s无操作→休眠
       ├─ notify_activity() — 有按键→唤醒
       ├─ 网络状态检测
       ├─ 亮度/音量调节 (MENU+PLUS/MINUS)
       ├─ MENU键 → 版本界面切换
       ├─ handle_launcher_input() — 导航 + A/B/X
       ├─ [if dirty] render_frame() → flip()
       └─ [else] vsync_wait()
```

#### 测试覆盖：57 个，100% 通过

| 类型     | 数量     | 模块     |
|--------|--------|--------|
| 数据结构   | 5      | types  |
| 路径     | 1      | paths  |
| 字符串    | 16     | utils  |
| 扫描     | 10     | scan   |
| 启动     | 2      | launch |
| 渲染     | 6      | render |
| 电源     | 10     | power  |
| 状态/导航  | 7      | state  |
| **总计** | **57** |        |

#### minui 功能完整性检查表

- [x] 数据结构：Entry, Directory, Recent, Button, PadContext
- [x] 平台抽象：Platform trait (视频、输入、音频、电源、覆盖层)
- [x] 路径派生：ROMS_PATH, USERDATA_PATH 等全部路径常量
- [x] 字符串工具：匹配、显示名、文件 I/O
- [x] 文件扫描：getRoot, getEntries, getRecents, getDiscs, Collections
- [x] 目录索引：字母快速跳转、同名条目处理、map.txt 映射
- [x] 游戏启动：openRom, openPak, autoResume, archive recovery
- [x] 导航：openDirectory, closeDirectory, loadLast, saveLast
- [x] 输入：方向键、翻页、L1/R1字母跳转、A/B/X、MENU
- [x] 最近游戏：addRecent, saveRecents, loadRecents
- [x] 主循环：60fps, VSync, HDMI检测
- [x] 渲染：RGB565, 圆角Pill, 字体, 电池, 按钮提示
- [x] 电源：30s休眠, 2min关机, 亮度/音量调节
- [ ] **平台实现**（rg35xx等）—— 后续按需开发
- [ ] **minarch**（libretro前端）—— 独立项目