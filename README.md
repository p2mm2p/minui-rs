# MinUI-rs — Rust 重写完整报告

> 原项目：[shauninman/MinUI](https://github.com/shauninman/MinUI) — 复古掌机上的极简游戏系统

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
6. [minui：启动器](#6-minui启动器)
   - 6.1 [完整生命周期](#61-完整生命周期从启动到退出再到启动)
   - 6.2 [三种 VSync 模式](#62-三种-vsync-模式)
   - 6.3 [ROM 到模拟器的映射](#63-rom-到模拟器的映射)
   - 6.4 [主机目录归类 (Collation)](#64-主机目录归类-collation)
   - 6.5 [滚动窗口算法](#65-滚动窗口算法)
   - 6.6 [双缓冲翻页机制](#66-双缓冲翻页机制)
   - 6.7 [多碟游戏处理 (M3U)](#67-多碟游戏处理-m3u)
   - 6.8 [存档槽位系统](#68-存档槽位系统)
   - 6.9 [名称映射 (map.txt)](#69-名称映射-maptxt)
   - 6.10 [get_root()：构建主屏幕](#610-get_root--构建主屏幕)
   - 6.11 [open_rom()：启动游戏](#611-open_rom--启动游戏)
   - 6.12 [导航和主循环](#612-导航和主循环)
7. [minarch：模拟器前端](#7-minarch模拟器前端)
   - 7.1 [完整生命周期](#71-minarch-的完整生命周期)
   - 7.2 [EmuCore trait](#72-emucore-trait)
   - 7.3 [核心加载：dlopen + 回调注入](#73-核心加载dlopen--回调注入)
   - 7.4 [游戏加载：ZIP 解压](#74-游戏加载zip-解压)
   - 7.5 [三级配置系统](#75-三级配置系统)
   - 7.6 [输入映射：两层转换 + 快捷键](#76-输入映射两层转换--快捷键)
   - 7.7 [视频缩放器：四种模式 × 两种设备类型](#77-视频缩放器四种模式--两种设备类型)
   - 7.8 [音频系统：环形缓冲区 + Nearest 重采样](#78-音频系统环形缓冲区--nearest-重采样)
   - 7.9 [存档系统：三种类型](#79-存档系统三种类型)
   - 7.10 [游戏内菜单](#710-游戏内菜单)
   - 7.11 [线程模式](#711-线程模式prioritize-audio)
   - 7.12 [FPS 统计和 Debug HUD](#712-fps-统计和-debug-hud)
8. [minui-platform：平台抽象层](#8-minui-platform平台抽象层)
   - 8.1 [Platform trait 详解](#81-platform-trait--支持-20-设备的秘诀)
   - 8.2 [Framebuffer 详解](#82-framebuffer--原始帧缓冲的抽象)
9. [minui-render：软件渲染器](#9-minui-render软件渲染器)
   - 9.1 [渲染原语](#91-渲染原语)
   - 9.2 [fb_draw_text() 详解](#92-fb_draw_text--软件文字光栅化)
   - 9.3 [RGB565 像素混合](#93-rgb565-像素混合)
   - 9.4 [UiRenderer：高层组合](#94-uirenderer高层组合)
10. [minui-power：电源管理](#10-minui-power电源管理)
   - 10.1 [PowerManager 详解](#101-powermanager--电源管理状态机)
   - 10.2 [休眠状态机](#102-休眠状态机)
11. [平台实现 (platforms/)](#11-平台实现-platforms)
   - 11.1 [结构](#111-结构)
   - 11.2 [keymon：按键监控守护进程](#112-keymon--按键监控守护进程)
   - 11.3 [libmsettings：文件通信替代共享内存](#113-libmsettings--文件通信替代共享内存)
   - 11.4 [平台列表](#114-平台列表)
12. [构建系统：xtask + cores + skeleton](#12-构建系统xtask--cores--skeleton)
   - 12.1 [xtask：Rust 构建工具](#121-xtask--rust-构建工具)
   - 12.2 [cores：libretro 核心编译](#122-cores--libretro-核心编译)
   - 12.3 [skeleton：SD 卡模板](#123-skeleton--sd-卡模板)
13. [原 C 代码到 Rust 的完整映射](#13-原-c-代码到-rust-的完整映射)
   - 13.1 [源文件映射](#131-源文件映射)
   - 13.2 [完整函数映射表](#132-完整函数映射表)
   - 13.3 [数据结构映射](#133-数据结构映射)
14. [Rust 重写的优化与改进](#14-rust-重写的优化与改进)
15. [如何新增一个平台](#15-如何新增一个平台)
16. [测试策略与覆盖](#16-测试策略与覆盖)
17. [当前模块架构](#17-当前模块架构)
18. [实现状态与路线图](#18-实现状态与路线图)

---

## 1. 系统概述：MinUI 是什么

### 1.1 一句话定义

MinUI 是一个运行在**国产 ARM Linux 复古掌机**上的**极简游戏系统**。它替换原厂那个臃肿花哨的界面，只做一件事：**列出游戏 → 选游戏 → 玩 → 下次开机自动回到上次位置**。

### 1.2 运行环境

| 项目 | 详情 |
|------|------|
| 硬件 | 全志/瑞芯微/君正 ARM SoC，64–256MB RAM，480p/720p LCD |
| 操作系统 | 裁剪过的 Linux 3.x/4.x，rootfs 约 20-80MB |
| 图形 | Linux framebuffer (`/dev/fb0`)，无 X11/Wayland |
| 输入 | `/dev/input/event*` 设备节点，直接读取 evdev 事件 |
| 显示 | RGB565 (16-bit) 像素格式，双缓冲 page flipping |
| 字体 | 单个 .otf 文件，通过 fontdue (纯 Rust) 渲染 |
| 模拟器 | libretro 核心 (.so)，由 minarch 加载和驱动 |

> **背景：为什么是 RGB565 和 framebuffer？** 这些掌机的 LCD 控制器原生支持 16-bit RGB565 格式，不需要像桌面 GPU 那样做颜色空间转换。使用 framebuffer 直接写入硬件显示缓冲区，避免了 X11/Wayland 等显示服务器的开销——在 64MB RAM 的设备上，每一 KB 都很珍贵。MinUI 不需要窗口管理器：整个屏幕就是一块可写的像素数组。

### 1.3 支持的设备

20+ 种掌机：Anbernic RG35XX 系列、Miyoo Mini/Plus、Trimui Smart/Brick/Pro、Powkiddy RGB30、MagicX 系列、GKD Pixel 等。所有设备共享**同一张 SD 卡和同一套代码**，仅编译时切换平台配置文件。

### 1.4 核心设计哲学

- **零配置**：没有设置菜单，插入 SD 卡即用
- **极简 UI**：无封面图、无主题、无动画、无多余元素
- **自动恢复**：关机再开直接回到刚才玩的游戏，用户感觉不到中断
- **单 SD 卡跨设备**：一张卡可以在不同厂商的多个设备间共用
- **Pak 扩展系统**：第三方模拟器以 `.pak` 文件夹形式安装，无需重新编译

### 1.5 两个独立进程

MinUI 由**两个完全独立的程序**组成，它们不直接调用对方，而是通过文件系统接力：

| 程序 | 什么时候运行 | 做什么 |
|------|------------|--------|
| **minui** | 用户在菜单里选游戏时 | 扫描 SD 卡，显示游戏列表，处理导航 |
| **minarch** | 用户选中游戏后 | 加载 libretro 模拟器核心，跑游戏 |

minui 选中游戏后，把启动命令写入 `/tmp/next`，然后自己退出。外层 shell 脚本读到命令后启动 minarch。minarch 退出后，shell 再次启动 minui。**两个进程永远不会同时存在。**

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
  ├─ [3] 原厂 init 脚本被 MinUI 劫持（skeleton/BOOT/ 下的启动脚本）
  │
  ├─ [4] 挂载 SD 卡 → 设置环境变量 → 启动 keymon 守护进程
  │
  └─ [5] 主循环脚本 (MinUI.pak/launch.sh) 启动 minui → 显示游戏列表
```

关键设计：**MinUI 不修改原厂 kernel 和 rootfs**。它利用原厂固件已有的 SD 卡启动机制，只需在 SD 卡上放置正确的文件即可。

### 2.2 minui 的生命周期

```
minui 启动
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
     └─ 外层 shell 读取 /tmp/next → 执行 minarch <core> <rom>
```

### 2.3 进程接力机制

minui **不直接启动游戏**，而是通过一个临时文件接力：

```
minui:
  queueNext("'/path/to/GB.pak/launch.sh' '/path/to/Zelda.gb'")
  → putFile("/tmp/next", cmd)
  → quit = 1
  → 退出

外层 shell:
  cmd=$(cat /tmp/next)
  eval $cmd

launch.sh:
  minarch gambatte_libretro.so "Zelda.gb"
```

> **背景：进程交接模式。** 在资源受限的嵌入式 Linux 上，`fork()+exec()` 会复制整个进程地址空间（包括 mmap 的 framebuffer），开销很大。通过文件接力，minui 干净地退出并释放所有资源（关闭 fd、unmap 内存），然后由轻量的 shell 脚本启动下一阶段。这避免了进程树中的僵尸进程，也简化了信号处理。

### 2.4 存档恢复机制

MinUI 的无感恢复依赖两个文件：

| 文件 | 写入时机 | 读取时机 | 内容 |
|------|---------|---------|------|
| `auto_resume.txt` | minarch 异常退出时（没电） | minui 启动时 | ROM 相对路径 |
| `recent.txt` | 每次启动游戏 | minui 启动时 | 最近游戏列表（含别名） |
| `/tmp/last.txt` | 每次游戏启动/退出 | minui 启动时（`loadLast`） | 上次浏览的目录路径 |

自动恢复流程：
```
autoResume():
  1. 检查 auto_resume.txt 是否存在
  2. 读取 ROM 相对路径 → 拼接 SD 卡完整路径
  3. 验证 ROM 文件仍然存在
  4. 验证对应模拟器仍然存在
  5. 写入存档槽位 9（自动恢复专用）
  6. 构造命令 → 写入 /tmp/next → 退出
```

正常启动（A 键）使用默认槽位 8（隐藏存档），X 键从上次手动存档恢复。

---

## 3. SD 卡文件系统布局

```
<SDCARD>/                              ← 例如 /mnt/sdcard
│
├── Bios/                              ← 主机 BIOS 文件
├── Roms/                              ← 游戏 ROM（MinUI 扫描此目录）
│   ├── Game Boy (GB)/                 ← 括号中的标签映射到模拟器
│   ├── Sony PlayStation (PS)/
│   │   └── Final Fantasy VII/
│   │       ├── Disc 1.bin
│   │       └── Final Fantasy VII.m3u  ← 多碟播放列表
│   └── map.txt                        ← 可选的名称映射文件
│
├── Saves/                             ← 游戏存档
│
├── .system/                           ← MinUI 系统文件（⚠ 更新时整体替换）
│   ├── <PLATFORM>/                    ← 如 rg35xxplus
│   │   ├── bin/                       ← minui, minarch, keymon, shutdown...
│   │   ├── cores/                     ← libretro 核心 (.so)
│   │   ├── lib/                       ← .keep (Rust 版不再需要 .so 库)
│   │   ├── system.cfg                 ← 平台强制配置
│   │   └── paks/
│   │       ├── MinUI.pak/             ← 主 Pak（环境变量 + 启动循环）
│   │       └── Emus/
│   │           ├── GB.pak/            ← launch.sh + default.cfg
│   │           └── ...
│   └── res/
│       └── BPreplayBold-unhinted.otf ← 字体文件
│
├── .userdata/                         ← 用户数据（⚠ 更新时保留）
│   ├── <PLATFORM>/
│   └── shared/
│       ├── enable-simple-mode         ← 空文件，存在则启用简化模式
│       └── .minui/
│           ├── recent.txt             ← 最近游戏列表（相对路径）
│           ├── auto_resume.txt        ← 自动恢复标记
│           └── <EMU>/
│               └── <romname>.txt      ← 存档槽位状态文件
│
├── Emus/                              ← 额外模拟器 Pak
├── Tools/                             ← 工具 Pak
└── Collections/                       ← 收藏列表（.txt 文件作为伪目录）
```

### 关键设计原则

- **`.system/` vs `.userdata/`**：前者存放可执行代码，升级时整体替换；后者存放用户数据，升级时保留。这是 MinUI OTA 更新的核心。
- **路径相对化**：`recent.txt` 中存储的是去掉 SDCARD_PATH 前缀的相对路径，使同一张 SD 卡可在不同设备间共享。
- **伪目录**：`Recently Played` 和 `Collections/*.txt` 不是真实目录，但在 UI 中表现为可浏览的目录。

---

## 4. 架构分层设计

### 4.1 整体结构

```
minui-rs/
├── Cargo.toml              ← workspace（18 个 member）
│
├── crates/                 ← Rust 库和二进制
│   ├── common/             ← 共享基础（types, utils, paths）
│   ├── minui-platform/     ← Platform trait 定义
│   ├── minui-render/       ← 软件渲染器
│   ├── minui-power/        ← 电源管理
│   ├── minui/              ← 启动器二进制
│   └── minarch/            ← 模拟器前端二进制
│
├── platforms/              ← 每个设备一个 crate
│   ├── rg35xxplus/         ← 完整参考实现
│   └── ...（其余 11 个平台）
│
├── cores/                  ← libretro 构建模板 + 共享补丁
├── skeleton/               ← SD 卡文件系统模板
├── xtask/                  ← 构建工具（cargo xtask build）
└── resources/              ← 编译时嵌入的字体
```

### 4.2 crate 依赖关系图

```
                    ┌──────────┐
                    │  common   │  (types, utils, paths)
                    └──┬───┬───┘
                       │   │
    ┌──────────────────┼───┼──────────────────┐
    │                  │   │                  │
┌───▼────────┐  ┌──────▼───▼───┐  ┌──────────▼──┐
│minui-      │  │ minui-render │  │ minui-power  │
│platform    │  │  (fontdue)   │  │              │
└───┬────────┘  └──────┬──────┘  └──────┬───────┘
    │                  │                │
    └────────┬─────────┼────────┬───────┘
             │         │        │
        ┌────▼──┐  ┌───▼──┐  ┌─▼──────┐
        │ minui │  │minarch│  (独立)   │
        │(启动器)│  │(前端) │  ← 通过   │
        └───────┘  └──────┘  /tmp/next │
                             文件接力   │
```

- `common` 是最底层共享 crate，被所有其他 crate 依赖
- `minui-platform` 定义 Platform trait，被 minui、minarch 和各平台实现依赖
- `minui-render` 和 `minui-power` 被两个二进制共同需要
- `minui` 和 `minarch` 是独立的二进制，**不互相引用**，仅通过 `/tmp/next` 文件通信

### 4.3 与原版 C 项目的对照

| 原版 C | Rust 对应 | 说明 |
|--------|----------|------|
| `workspace/all/common/` | `crates/common/` | types, utils, paths |
| `workspace/all/minui/` | `crates/minui/` | 启动器 |
| `workspace/all/minarch/` | `crates/minarch/` | 模拟器前端 |
| `workspace/<平台>/platform/` | `platforms/<平台>/src/lib.rs` | 平台实现 |
| `workspace/<平台>/keymon/` | `platforms/<平台>/src/keymon.rs` | 按键监控 |
| `workspace/<平台>/libmsettings/` | `platforms/<平台>/src/libmsettings.rs` | 设置管理 |
| `workspace/<平台>/cores/` | `platforms/<平台>/cores/` | 核心列表和补丁 |
| `workspace/all/cores/makefile` | `cores/makefile` | 核心构建模板 |
| `skeleton/` | `skeleton/` | SD 卡模板（不变） |

---

## 5. 核心数据结构全景

> 以下类型定义在 `crates/common/src/types.rs` 中，对应原 C 代码 `minui.c` 的数据结构段和 `defines.h` 的枚举定义。

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

### 5.4 Entry — 文件系统条目的表示

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

### 5.5 Directory — 一个屏幕的可浏览内容

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

### 5.6 Recent — 最近游戏（跨设备共享）

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
每行一个条目，`\t` 后是可选的别名。读取时反向解析。

### 5.7 Button 和 PadContext — 输入抽象

```rust
// C: 分散的 #define BTN_* 宏 + PAD_Context 结构体
struct Button(pub u32);  // 新类型模式包装 u32 位掩码
```

**PadContext 的时序检测**：
```rust
struct PadContext {
    is_pressed: Button,      // 当前帧被按下的所有按钮
    just_pressed: Button,    // 当前帧刚按下的按钮（上升沿）
    just_released: Button,   // 当前帧刚释放的按钮（下降沿）
    just_repeated: Button,   // 长按自动重复（首次300ms延迟，之后每100ms）
    repeat_at: [u32; COUNT], // 每个按钮下次触发 repeat 的时间戳
}
```

**just_pressed 的计算**（在 `poll_input` 中）：`just_pressed = is_pressed & !was_pressed`。即：当前帧按下的按钮中，去掉上一帧也在按下的。这是纯硬件去抖 + 边沿检测。

**just_repeated 的计算**：当 `is_pressed` 持续为真且当前时间 `>= repeat_at[btn_id]` 时，将该按钮加入 `just_repeated`。首次延迟 300ms，之后每次间隔 100ms。

**为什么需要组合按钮**：嵌入式掌机的 D-pad 和摇杆在 evdev 层面是不同的按键码，但对用户来说都是"方向"操作。`Button::UP = DPAD_UP | ANALOG_UP` 统一了这两种输入源。

---

## 6. minui：启动器

> 对应原 C 项目 `workspace/all/minui/minui.c`（约 700 行）。Rust 实现在 `crates/minui/src/` 下，分为 `state.rs`（主循环和导航）、`scan.rs`（文件系统扫描）、`launch.rs`（游戏启动）三个模块。

minui 是用户看到的"游戏列表界面"。它负责扫描 SD 卡上的 ROM 目录、显示游戏列表、处理用户导航、并在选中游戏后启动 minarch。minui 自己不运行任何模拟器——它只是一个"文件浏览器"和"进程启动器"。

### 6.1 完整生命周期：从启动到退出再到启动

minui 的每次运行都经历相同的三个阶段：

```
[启动] 检查是否从异常关机恢复
  │
  ├─ autoResume() 发现 auto_resume.txt 存在
  │    → 直接构造启动命令 → 写入 /tmp/next → exit(0)
  │    (整个过程不到 100ms，用户看不到任何界面)
  │
  └─ 没有 auto_resume.txt → 进入正常初始化

[初始化]
  ├─ GFX_init()     → open /dev/fb0 → mmap → 双缓冲
  ├─ PAD_init()     → open /dev/input/event*
  ├─ PWR_init()     → 启动电池监控线程
  ├─ Menu_init()    → 扫描 SD 卡 → 构建目录树 → 加载最近游戏
  │                   → loadLast() 恢复到上次浏览位置
  └─ set_cpu_speed(MENU) + set_vsync(STRICT)

[主循环] 每帧约 16ms (60fps)
  while (!quit && !poweroff_requested):
    ├─ platform.poll_input(&mut pad)
    ├─ power.update(dt_ms)           ← 推进设置提示/关机计时器
    ├─ if !pad.any_pressed():        ← 空闲检测
    │    if prevent_autosleep():      ← 充电/禁用/HDMI 时不休眠
    │      notify_activity()          ← 重置空闲计时器
    │    elif check_autosleep():      ← 累计空闲时间, 30s 后休眠
    │      dirty = true
    ├─ if pad.any_just_pressed():    ← 有按键→唤醒+重置
    │    notify_activity()
    ├─ 网络状态检测 (WiFi 变化→dirty)
    ├─ 亮度/音量输入: MENU+PLUS/MINUS → power.handle_setting_input()
    ├─ MENU 快按: toggle show_version
    ├─ 导航/确认处理: handle_launcher_input()
    ├─ if dirty && !asleep:
    │    render_frame() → flip()      ← 只有变化时才渲染
    │    dirty = false
    └─ elif !asleep: vsync_wait()     ← 空闲时维持 60fps 节奏

[退出]
  if poweroff_requested: platform.power_off()  ← 不返回
  quit_menu() → 释放所有资源 → return Ok(true)
```

> **关键时序：** `dt_ms` 被 clamp 到 100ms 以下。如果因为某种原因（如 storage I/O 阻塞）导致帧间隔超过 100ms，计时器不会因此跳进——防止一帧卡顿就让设备进入休眠。

### 6.2 三种 VSync 模式

原 C 代码通过 `GFX_setVsync()` 设置，对应 Rust `Platform::set_vsync()`：

| 模式 | 值 | 行为 | 使用场景 |
|------|---|------|---------|
| VSYNC_OFF | 0 | 永远不等待，`SDL_Delay` 补足帧时间 | 需要最低延迟时 |
| VSYNC_LENIENT | 1 | 只在帧预算内时等待 VSync | minarch 默认值 |
| VSYNC_STRICT | 2 | 总是等待 VSync | minui 菜单（稳定画面） |

**Lenient 模式的精妙之处：** 帧预算是 17ms（60fps）。如果模拟器这帧花了 19ms 才跑完（已经超了），就不等待了——直接把画面丢出去。下一帧如果只花了 10ms，就等 7ms 再翻页。这样画面不会撕裂，也不会因为偶尔的性能尖刺导致整体帧率下降。

> **背景：为什么菜单用 Strict 而游戏用 Lenient？** 菜单界面很少变化（用户不操作时可能几秒钟都无需重绘），Strict 模式下硬等待 VSync 能最大化省电。游戏画面每帧都在变，Lenient 模式在保证画面完整性的前提下允许偶尔的帧率波动。

### 6.3 ROM 到模拟器的映射

MinUI 通过**目录命名约定**自动发现模拟器，无需任何配置文件：

```
路径: /mnt/sdcard/Roms/Game Boy (GB)/Zelda.gb
                                    ^^
                              这就是模拟器标签

解析流程 getEmuName():
  1. 路径在 ROMS_PATH 下 → 提取 Roms 子目录名 "Game Boy (GB)"
  2. 取末尾括号内容 → "GB"
  3. 查找 "GB.pak" → 找到 launch.sh
  4. launch.sh 调用 minarch gambatte_libretro.so "Zelda.gb"
```

模拟器查找优先级：先查 SD 卡上的 `<SDCARD>/Emus/<PLATFORM>/<emu>.pak/launch.sh`（用户自己装的），再查系统目录 `<PAKS_PATH>/Emus/<emu>.pak/launch.sh`（内置的）。这种两级查找允许用户用第三方模拟器覆盖内置的。

### 6.4 主机目录归类 (Collation)

同名主机的不同变体自动合并显示。这是为处理"Game Boy (GB)"和"Game Boy Color (GBC)"这种情况设计的：两个目录共享"Game Boy"前缀，用户看到的是一个合并后的列表。

```
Roms/
├── Game Boy (GB)/       ← 归类前缀: "Game Boy ("
├── Game Boy (GBC)/      ← 匹配前缀 → 合并
├── Game Boy Color (GBC)/← 不匹配 ( "(" ≠ "C" ) → 单独列表
└── Game Boy Advance (GBA)/ ← 不匹配 → 单独列表
```

算法：对当前目录取到最后一个 `(` 为止（包含 `(`），然后用这个前缀去匹配 ROMS_PATH 下的其他目录。保留 `(` 是为了避免 "Game Boy (" 同时匹配 "Game Boy Advance"——因为 "Game Boy (" 和 "Game Boy A" 的首个不同字符就是 `(` vs `A`。

### 6.5 滚动窗口算法

屏幕只能显示有限行数（由 `MAIN_ROW_COUNT` 定义，通常为 6）。维护三个索引实现虚拟滚动：

```
条目总数 = T, 屏幕行数 = N

不变量:
  start ≤ selected < end
  end - start ≤ N
  end ≤ T

按键行为:
  UP:    selected--; if selected < start: start--, end--
         到顶时：循环到末尾 (selected=total-1, start=total-N, end=total)
  DOWN:  selected++; if selected ≥ end: start++, end++
         到底时：循环到开头
  LEFT:  selected -= N (翻页上)，不循环
  RIGHT: selected += N (翻页下)，不循环
  L1:    跳转到上一个字母组的第一项
  R1:    跳转到下一个字母组的第一项
```

UP/DOWN 到边界时**循环到另一端**（wrap around），但 LEFT/RIGHT 翻页**不循环**（停在边界）。这是为了和 L1/R1 的字母跳转配合：如果用户按 LEFT 翻页到了边界，他可以通过 L1/R1 继续导航。

### 6.6 双缓冲翻页机制

使用 ION 内存分配器（Linux 内核的 DMA 内存管理接口）分配连续的物理内存：

```
ION 分配:  [ PAGE_0 (640x480x2 bytes) ][ PAGE_1 (640x480x2 bytes) ]
              ↑ 当前显示                    ↑ 后台绘制

flip():
  1. 写硬件寄存器 DE_OVL_BA0 = fb_paddr + page * PAGE_SIZE
  2. vsync_wait()
  3. page ^= 1
```

> **背景：为什么需要 ION？** 普通的 `malloc` 分配的是虚拟内存，物理地址可能跨越多个不连续的页框。显示引擎的 DMA 控制器需要连续的物理地址才能正确传输一行像素数据。ION 从内核预留的连续内存池（如 CMA, Contiguous Memory Allocator）中分配。这避免了 CPU 做昂贵的 `memcpy` 到"真正"的显示缓冲区——CPU 直接写 ION 内存，DMA 直接从 ION 内存读。

### 6.7 多碟游戏处理 (M3U)

PlayStation 等多碟游戏通过 `.m3u` 文件管理：

```
Roms/PS/
├── Final Fantasy VII/
│   ├── Final Fantasy VII (Disc 1).cue
│   ├── Final Fantasy VII (Disc 2).cue
│   └── Final Fantasy VII (Disc 3).cue
└── Final Fantasy VII.m3u       ← 内容：三行相对路径
```

用户选中 `.m3u` 时，minui 展示 Disc 1/2/3 的子列表。选中某碟后启动对应的 `.cue` 文件。

**换碟与存档的联动**：每个存档槽位文件记录了存档时的碟号。恢复存档时自动切换到正确的碟片——用户按 X 键恢复时不必记住自己当时用哪张碟存的。实现方式是在 `.stN` 存档文件旁边保存一个 `.N.txt` 记录当时的碟路径。

### 6.8 存档槽位系统

每个 ROM 有 10 个槽位，通过两个文件协同工作：

| 槽位 | 用途 | 触发方式 |
|------|------|---------|
| 0-7 | 手动存档 | 游戏内菜单选择（带截图预览） |
| 8 | 隐藏默认存档 | 按 A 键启动游戏（普通启动） |
| 9 | 自动恢复存档 | 设备没电/非正常关机后自动恢复 |

| 文件 | 格式 | 内容 |
|------|------|------|
| 槽位选择 | `.minui/<EMU>/<rom>.txt` | 当前选中槽位号（如 "3"） |
| 碟号记录 | `.minui/<EMU>/<rom>.3.txt` | 多碟游戏下槽位 3 对应的碟路径 |

`ready_resume_path()` 检查槽位选择文件是否存在来决定是否显示 "X RESUME" 按钮。

### 6.9 名称映射 (map.txt)

任何目录下可以放置 `map.txt`：

```
格式: <原始文件名><TAB><新显示名>

Game Boy (GB)	Nintendo Game Boy
.some_rom	.Hidden Game        ← 以 . 开头 → 条目被 hide() 过滤
```

> **背景：为什么需要 map.txt？** 不同地区的 ROM 发行商对同一游戏主机有不同命名（如 "Famicom" vs "NES"），ROM 文件名可能包含版本号和 dump 信息。`map.txt` 让用户自定义显示名而不改变文件系统结构。

映射在 `make_directory()` 的索引建立阶段应用。映射后的条目如果 `hide()` 返回 true，从列表移除。如果任何条目被重命名，整个列表重新排序。

### 6.10 get_root() — 构建主屏幕

启动时最复杂的函数：

```
get_root(sdcard, platform_tag, paks, has_recents, has_collections, simple_mode):
  root = []
  // 1. "Recently Played" 伪目录
  // 2. 扫描 Roms/ 下子目录 → 过滤无模拟器/无ROM的
  // 3. 排序 + 同名去重
  // 4. 应用 Roms/map.txt 名称映射
  // 5. Collections: 有系统时作为子目录，无系统时提升到根
  // 6. Tools/<PLATFORM>: 非简化模式下添加到根
  return root
```

### 6.11 open_rom() — 启动游戏

```
open_rom(path):
  1. M3U 处理: 检测多碟 → 取第一张碟
  2. 存档恢复 (X键): 读槽位号 → 读碟号记录 → 切换到正确碟
     普通启动 (A键): 使用槽位 8
  3. get_emu_path() 找到模拟器 launch.sh
  4. add_recent() + save_last() 持久化状态
  5. queue_next(cmd) → 写 /tmp/next → quit=1
```

`open_directory()` 处理自动启动：如果目录下有 `.cue` 或上级有同名 `.m3u`，跳过目录浏览直接启动游戏——这样 PS1 游戏的文件夹就不会把 `.bin`/`.cue` 内部文件暴露给用户。

### 6.12 导航和主循环

`handle_launcher_input()` 处理所有导航输入（UP/DOWN/LEFT/RIGHT/L1/R1/A/B/X），每次操作后更新滚动窗口并触发 `ready_resume()` 检查（决定是否显示 X RESUME）。

`run()` 是 minui 的入口：autoResume 检查 → 初始化所有子系统 → 进入主循环 → 每帧 poll 输入 → power 更新 → 自动休眠检查 → 导航处理 → 条件渲染 → 帧率控制 → HDMI 检测。退出时，如果用户选了游戏，外层脚本读取 `/tmp/next` 启动 minarch；如果是关机请求，调用 `platform.power_off()`。

---

## 7. minarch：模拟器前端

> 对应原 C 项目 `workspace/all/minarch/minarch.c`（约 4800 行）。Rust 实现在 `crates/minarch/src/` 下，分为 9 个子模块。

minarch 本身**不模拟任何游戏机**——它不包含 GB 的 CPU 模拟、不包含 PS 的 GPU 模拟。它是 libretro 核心的**宿主程序**（host），负责"把核心跑起来"所需的一切基础设施：加载核心、喂输入、收画面、放声音、管存档。

```
你的手指 → evdev → platform::poll_input → input mapper → 核心
                                                              ↓
                                                       核心跑一帧
                                                       ↙        ↘
                                              画面 (video)    声音 (audio)
                                                 ↓               ↓
                                          selectScaler()  环形缓冲区 → SDL
                                                 ↓
                                          GFX_blitRenderer → flip
```

> **背景：libretro 是什么？** libretro 是一个模拟器 API 标准。每个"核心"（core）是实现了 `retro_run()`, `retro_serialize()` 等函数的 .so 文件。前端程序（如 minarch）加载核心、调用这些函数、并注入回调（"画面来了画到哪？""按键在哪？""存档存哪？"）。这种分离让同一份核心代码可以被数十种前端使用。

### 7.1 minarch 的完整生命周期

```
[启动] minarch gambatte_libretro.so "Zelda.gb"
  │
  ├─ GFX_init(MODE_MENU)         ← 初始化屏幕
  ├─ PAD_init()                   ← 打开输入设备
  ├─ VIB_init()                   ← 震动初始化
  ├─ PWR_init()                   ← 电池监控
  │
  ├─ Core_open(core_path)        ← dlopen → dlsym 所有 retro_* 函数
  │   └─ 读取系统信息 (name, version, extensions)
  │   └─ 创建目录: config_dir, states_dir, saves_dir, bios_dir
  │   └─ 注入回调: environment, video_refresh, audio_sample, input_poll, input_state
  │
  ├─ Game_open(rom_path)         ← 加载 ROM（普通/ZIP/M3U）
  ├─ Config_load()               ← 三级配置加载
  ├─ Config_init()               ← 解析 default.cfg 按键绑定
  ├─ Core_init()                  ← retro_init()
  ├─ Core_load()                  ← retro_load_game() + SRAM_read + RTC_read
  │   └─ get_system_av_info() → 获取 fps, sample_rate, aspect_ratio
  │   └─ set_controller_port_device(RETRO_DEVICE_JOYPAD)
  │
  ├─ Input_init()                ← 设置默认按键映射
  ├─ SND_init(sample_rate, fps)  ← 打开音频设备
  ├─ Menu_init()                 ← 初始化游戏内菜单（截图背景、存档预览）
  ├─ State_resume()              ← 自动恢复存档（槽位 9）
  │
  └─ [主循环] while (!quit)
       ├─ if !thread_video: core.run()  ← 跑一帧游戏
       │    └─ 核心回调: video_refresh → 缩放 → blit → flip
       │    └─ 核心回调: audio_sample  → 重采样 → 环形缓冲
       │    └─ 核心回调: input_poll    → 读取按键 → 更新 retropad
       ├─ if show_menu: Menu_loop()     ← MENU 键弹出菜单
       ├─ limitFF()                     ← 快进限速
       ├─ trackFPS()                    ← 帧率统计
       ├─ toggle_thread()               ← 线程模式切换
       └─ hdmimon()                     ← HDMI 检测
```

### 7.2 EmuCore trait：核心抽象

```rust
trait EmuCore {
    fn init(&mut self) -> Result<()>;
    fn run(&mut self, callbacks: &mut CoreCallbacks) -> Result<()>;
    fn load_game(&mut self, game: &GameData) -> Result<()>;
    fn serialize(&self, data: &mut [u8]) -> Result<()>;
    fn unserialize(&mut self, data: &[u8]) -> Result<()>;
    fn get_memory_data(&self, id: MemoryType) -> Option<&[u8]>;
    fn get_memory_size(&self, id: MemoryType) -> usize;
}
```

两种潜在实现：
1. **LibretroCore**：通过 `libloading` crate 动态加载 .so 文件，FFI 调用 `retro_*` 函数——兼容所有现有 libretro 核心
2. **RustCore**（未来）：纯 Rust 模拟器直接编译进 minarch，无需 dlopen，编译时就知道核心能做什么

### 7.3 核心加载：dlopen + 回调注入

`LibretroCore::open()` 做了和原版 `Core_open()` 一样的事：

```rust
let lib = Library::new(core_path)?;                    // dlopen
let init: Symbol<extern "C" fn()> = lib.get(b"retro_init")?; // dlsym
let run: Symbol<extern "C" fn()> = lib.get(b"retro_run")?;
// ... 共提取 15+ 个函数指针

// 注入回调：告诉核心"有需求时调用我"
set_environment(environment_callback);      // 我要 BIOS、我要存档目录、我产出了什么格式...
set_video_refresh(video_refresh_callback);  // 这帧画面来了，你处理
set_audio_sample(audio_sample_callback);    // 这帧声音来了，你处理
set_input_poll(input_poll_callback);        // 帮我读一下按键
set_input_state(input_state_callback);      // 当前哪个键按着？
```

> **environment_callback 是核心与前端通信的总线。** 核心通过它问了不下 50 种问题：`RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY`（BIOS 在哪？）、`RETRO_ENVIRONMENT_SET_PIXEL_FORMAT`（我能输出 565 你收吗？）、`RETRO_ENVIRONMENT_SET_CORE_OPTIONS`（我有哪些配置选项？）、`RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE`（我能热换碟，你要用吗？）。minarch 的 `environment_callback()` 是一个巨大的 match 语句，只处理自己关心的那部分。

### 7.4 游戏加载：ZIP 解压

很多 ROM 以 `.zip` 格式存储。分两种情况：

- **核心自己支持 ZIP**：minarch 直接把 zip 路径传给核心
- **核心不支持 ZIP**：minarch 手动解析 ZIP 文件结构（约 80 行手写代码），找到第一个匹配的 ROM 文件，解压到 `/tmp/minarch-XXXXXX/` 临时目录

ZIP 解析只支持 Store（无压缩）和 Deflate（zlib）两种方式。这是为了最小化依赖：不需要引入完整的 `libzip` 或 `zlib` 库。

### 7.5 三级配置系统

配置的优先级从高到低，每一层可以覆盖下一层：

```
system.cfg     ← 平台强制限制。如弱 CPU 必须降分辨率。
                  └─ 锁定: -minarch_screen_scaling = Native
                     (前面有 - 前缀 → 用户界面中不可见、不可改)
  ↓ 覆盖
default.cfg    ← PAK 默认值。开发者设的最佳配置。
  ↓ 覆盖
minarch.cfg    ← 用户全局设置。所有游戏生效。
<game>.cfg     ← 单游戏覆盖。
```

```rust
struct Config {
    system: Option<HashMap<String, String>>,    // 含锁定标记
    defaults: Option<HashMap<String, String>>,
    user_global: Option<HashMap<String, String>>,
    user_game: Option<HashMap<String, String>>,
    locks: HashSet<String>,
}
```

`-` 前缀实现的机制：解析 `system.cfg` 时 `Config_getValue()` 检查 key 的前一个字符是否是 `-`——如果是，跳过前面减号取 key，并把选项标记为 `lock=1`。被锁定的选项在 UI 菜单中直接被过滤掉。

> **典型场景：** 某廉价掌机跑 PS 模拟器必须降分辨率。开发者在 `system.cfg` 写 `-minarch_screen_scaling = Native`。用户打开 Options → Frontend 时看不到 "Screen Scaling" 这个选项——它从选项列表中消失了。防止用户误改导致游戏跑不动。

### 7.6 输入映射：两层转换 + 快捷键

输入经过两层映射才到达核心：

```
物理按键扫描码   →  BTN_ID  →  controls 配置  →  retro_id  →  核心
(platform.h)       (逻辑层)    (用户可重绑定)    (libretro)
```

默认映射是直通的（D-pad 就是 D-pad），但用户可以在 Controls 菜单中重映射。例如把 R2 映射到 RetroPad 的 A——某些游戏需要快速连按时的替代方案。

8 种快捷键在自己的循环中处理，**优先级高于普通按键**——如果用户绑了 MENU+A 为 Save State，那按下 MENU+A 时核心收不到 A 键：

| 快捷键 | 作用 |
|--------|------|
| Save State / Load State | 即时存档/读档 |
| Reset Game | 重置游戏 |
| Save & Quit | 存档后退出 |
| Cycle Scaling / Cycle Effect | 循环缩放模式/画面效果 |
| Toggle FF / Hold FF | 切换快进 / 按住快进 |

快进的实现：`core.run()` 照样跑，但音频回调中 `if (fast_forward) return`——不写音频数据。`limitFF()` 限制最大帧率：默认 4x（即模拟器每秒最多渲染 `fps × 4` 帧）。用一个微秒级的时间戳（`gettimeofday`）做精确限速。

### 7.7 视频缩放器：四种模式 × 两种设备类型

`selectScaler()` 是 minarch 中最数学的部分。核心输出一张原始像素图（GB 是 160×144，SFC 是 256×224），需要缩放到设备屏幕（如 640×480）。结果是一个 `GFX_Renderer`，包含了 src_rect / dst_rect / scale / blit 函数。

| 模式 | 算法 | 效果 |
|------|------|------|
| **Native** | `scale = min(dev_w/src_w, dev_h/src_h)` 整数倍 | 像素完美，可能留黑边 |
| **Aspect** | 保持宽高比，尽可能填满 | 可能有 letterbox/pillarbox |
| **Fullscreen** | 拉伸到全屏 | 像素可能非正方形 |
| **Cropped** | 整数倍缩放后裁切 | 填满 + 像素方正（部分设备支持） |

设备分两类：
- **fit 设备**（画面小于屏幕）：使用 SDL2 硬件缩放
- **oversized 设备**（如 320×240 屏跑 256×224 游戏）：需要软件缩放到目标尺寸再裁切

缩放后的画面可以叠加效果：Sharp（最近邻）/ Crisp（先整数放大再线性）/ Soft（线性插值）。以及 Grid（LCD 网格）和 Line（CRT 扫描线）效果。

> **Aspect 模式的实际例子：** 设备 640×480，核心输出 256×224，核心报告宽高比 1.306:1（SNES）。
> - 先计算核心宽高比适配的尺寸：`aspect_w = 480 * 1.306 = 627`, `aspect_h = 480`
> - 整数缩放：`scale = max(640/256, 480/224) = max(2.5, 2.14) = 3`
> - 缩放后尺寸：`256*3=768, 224*3=672`
> - 因为核心宽高比和屏幕不一致（1.306 vs 1.333），做 pillarbox 调整
> - 最终 dst_rect: 宽度 768 然后 pillarbox 压缩，高度 672 居中

### 7.8 音频系统：环形缓冲区 + Nearest 重采样

```
核心产采样 → audio_sample_batch_callback(16bit stereo)
  → SND_batchSamples()
    → if 采样率不同: Nearest 重采样
    → 写入环形缓冲区 (5秒容量)
    → if 缓冲区满: 最多等 10ms, 然后强制覆盖

SDL 需要数据 → SND_audioCallback()
  → 从环形缓冲区读取
  → if 缓冲区空: 重复最后一个采样值 (而非爆音)
  → 写入 SDL 音频流
```

**为什么 5 秒？** 模拟器不是每帧都恰好产生正确数量的采样。有时跑得快（多产了），有时跑得慢（少产了）。5 秒缓冲给足够弹性。

**Nearest 重采样**用 Bresenham 式误差累积：
```rust
fn resample_near(frame):
    if diff < sample_rate_out:
        buffer[write++] = frame
        diff += sample_rate_in
    if diff >= sample_rate_out:
        diff -= sample_rate_out
        return 1 // consumed
    return 0     // dropped
```

**生产端反压**是精妙设计：如果写入速度超过读取速度（模拟器跑太快或音频设备暂时卡住），`SND_batchSamples()` 会 `SDL_UnlockAudio()` 然后 `SDL_Delay(1)` 再 `SDL_LockAudio()`——让 SDL 有机会从缓冲区消费。最多试 10 次（10ms），超过后强制覆盖。

### 7.9 存档系统：三种类型

| 类型 | 读取时机 | 写入时机 | 数据来源 |
|------|---------|---------|---------|
| **SRAM** | `Core_load()` 后立即 | 退出/休眠前 + 进菜单时 | `core.get_memory_data(RETRO_MEMORY_SAVE_RAM)` |
| **RTC** | 同上 | 同上 | `core.get_memory_data(RETRO_MEMORY_RTC)` |
| **State** | 用户选择 Load | 用户选择 Save / 休眠前 / 开机自动恢复 | `core.serialize()` 二进制 |

State 最有趣：它把整个模拟器状态（CPU 寄存器、内存、各种芯片状态）序列化成二进制。这不是"存档文件"——是模拟器的完整快照。10 个槽位（0-7 手动, 8 默认, 9 自动恢复）。存档时截图保存为 BMP 作为预览。

休眠/退出前 `State_autosave()` 使用槽位 9。下次开机时如果在 `/tmp/resume_slot.txt` 发现存档槽位号，`State_resume()` 恢复。

### 7.10 游戏内菜单

按 MENU 键时，minarch 不暂停核心——而是截取当前画面，做半透明遮罩，在上面画菜单：

```
Continue    ← 继续 / 多碟时左右切换碟片
Save        ← 即时存档 (8 槽位, 右侧截图预览, 分页点)
Load        ← 即时读档
Options →   Frontend (缩放/效果/CPU速度/防撕裂/线程/调试/快进上限)
            Emulator (核心动态提供的选项, 如 GB 调色板)
            Controls (按键重映射, 支持 MENU+组合)
            Shortcuts (快捷键绑定)
            Save Changes (保存为 console/game 级别, 或恢复默认)
Quit        ← 退出游戏
```

菜单是一个嵌套的 `MenuList` 系统，有四种模式：`MENU_LIST`（纯文字列表）、`MENU_VAR`（左名右值，左右键切换）、`MENU_FIXED`（类似 VAR 但值不扩到全宽）、`MENU_INPUT`（按 A 进入"等待按键"重映射模式）。按键重映射的实现特别有趣：用户选中一行按 A 后，代码进入一个 `while (!bound)` 的小循环，等待下一个按键事件——按什么就绑什么。如果同时按着 MENU，就绑定 MENU+键组合。按 X 清除绑定（设为 NONE）。

### 7.11 线程模式（Prioritize Audio）

某些核心（尤其是 SNES 的 SuperFX 芯片游戏）在高负载时音频出现爆音。原因是视频渲染（`GFX_flip + vsync_wait`）阻塞了音频处理。解决方案：把 `core.run()` 放到独立线程：

```
[线程1: 核心]          [线程2: 渲染]
core.run()  ──→  backbuffer (mutex)
  ↓                    ↓
cond.signal()  ──→  cond.wait() 被唤醒
                     ↓
                  video_refresh(backbuffer)
                  GFX_flip()
```

两个线程通过 Mutex + Condvar 同步。核心线程跑完一帧后，把画面复制到 `backbuffer`，发信号给渲染线程。渲染线程等信号到了才渲染。这样核心不会在 vsync 上阻塞。

> **开关线程模式需要小心。** `toggle_thread()` 处理了两种情况：从单线程切到多线程（创建线程），和从多线程切回单线程（cancel + join + 强制 vsync 清屏）。快进时也要考虑线程切换——`was_threaded` 标记记录了"进入快进前的模式"，退出快进后恢复。

### 7.12 FPS 统计和 Debug HUD

FPS 通过计数每秒 `core.run()` 的调用次数得到。CPU 使用率通过读 `/proc/self/stat` 获取进程的 utime+stime 然后除以 `sysconf(_SC_CLK_TCK)` 得到秒数。Debug HUD 用自制的 5×9 位图字体渲染，避免依赖 TTF 渲染（在游戏画面上直接写像素，零性能开销）：

```
160x144 2x          640,480 320x288
60.1/60.0 15%
640x480
```

左上角是源分辨率和缩放倍数，右上角是目标坐标和缩放后尺寸，第三行是 FPS/CPU 使用率，第四行是输出分辨率。

---

## 8. minui-platform：平台抽象层

**对应原版**：`platform.h`（常量 #define）+ `api.h`（方法签名）。Rust 实现在 `crates/minui-platform/src/platform.rs`。

### 7.1 Platform trait — 支持 20+ 设备的秘诀

```rust
pub trait Platform: Send + Sized {
    // ── 屏幕参数（编译时常量）──
    const FIXED_WIDTH: u32;       // 如 640
    const FIXED_HEIGHT: u32;      // 如 480
    const FIXED_BPP: u8;          // 2 (RGB565)
    const FIXED_SCALE: u32;       // 2

    // ── 按键映射 ──
    const KEY_UP: i32;            // evdev 扫描码
    const KEY_A: i32;
    // ... 22 个按键映射

    // ── 运行时方法 ──
    fn init_video(&mut self) -> Result<Framebuffer, String>;
    fn poll_input(&mut self, pad: &mut PadContext);
    fn flip(&mut self, fb: &Framebuffer, sync: bool);
    fn get_battery_status(&self) -> (bool, u8);
    fn power_off(&self) -> !;
    fn init_audio(&mut self, ...) -> Result<(), String>;
    // ... 30+ 方法
}
```

**为什么用关联常量而非字段**：`FIXED_WIDTH` 等值在编译时就已确定，用关联常量让编译器做常量折叠和内联优化，零运行时开销。运行时方法如 `poll_input` 需要访问平台的状态（fd、mmap），所以是 `&mut self` 方法。

**TestPlatform**：测试中使用的平台实现，所有操作在堆分配的 `Vec<u8>` 上完成，不依赖任何硬件：
```rust
struct TestPlatform { fb: Vec<u8>, battery_charging: bool, battery_level: u8, ... }
impl Platform for TestPlatform {
    fn init_video(&mut self) -> Result<Framebuffer, String> {
        Ok(Framebuffer { pixels: self.fb.as_mut_ptr(), ... })
    }
}
```

### 7.2 Framebuffer — 原始帧缓冲的抽象

```rust
// C: SDL_Surface { pixels, w, h, pitch, format->BytesPerPixel }
struct Framebuffer {
    pixels: *mut u8,   // mmap 的 ION 内存或测试用堆内存
    width: u32, height: u32,
    pitch: u32,        // 每行字节数（可能 > width * bpp）
    bpp: u8,           // 2 = RGB565
}
```

**pitch vs width * bpp**：硬件 framebuffer 的每行可能有额外的填充字节以满足对齐要求。所有像素寻址必须用 `y * pitch + x * bpp`。

---

## 9. minui-render：软件渲染器

**对应原版**：`api.c` 中 GFX 部分。Rust 实现在 `crates/minui-render/src/lib.rs`。使用 [fontdue](https://crates.io/crates/fontdue) 进行字体光栅化——纯 Rust 实现，无需 FreeType/SDL_ttf。

> **为什么选择 fontdue？** 原 C 版依赖 SDL_ttf → FreeType，这是一个 C 库依赖链。fontdue 是纯 Rust，`no_std` 兼容，字形质量与 FreeType 相当。

### 8.1 渲染原语

| 函数 | 对应 C | 功能 |
|------|--------|------|
| `fb_clear` | `PLAT_clearVideo` | 填充全屏 |
| `fb_fill_rect` | — | 填充矩形 |
| `fb_draw_pill` | `GFX_blitPill` | 圆角矩形（左右半圆 + 中间矩形） |
| `fb_draw_text` | `GFX_blitText` | Alpha 混合文字 |
| `fb_draw_battery` | `GFX_blitBattery` | 电池图标（含电量填充和充电指示） |

### 8.2 fb_draw_text() — 软件文字光栅化

```rust
fn fb_draw_text(fb, text, rect, color, font, px):
  for each char in text:
    (metrics, bitmap) = font.rasterize(char, px)
    for each pixel in bitmap:
      alpha = bitmap[y * w + x]
      if alpha > 0:
        existing = fb.pixels[screen_y * pitch + screen_x * 2]
        fb.pixels[...] = blend_rgb565(existing, color.0, alpha)
```

### 8.3 RGB565 像素混合

```rust
fn blend_rgb565(bg: u16, fg: u16, alpha: u8) -> u16:
  r = ((fg_r * a + bg_r * (255 - a)) / 255) & 0x1F;
  g = ((fg_g * a + bg_g * (255 - a)) / 255) & 0x3F;
  b = ((fg_b * a + bg_b * (255 - a)) / 255) & 0x1F;
  (r << 11) | (g << 5) | b
```

### 8.4 UiRenderer：高层组合

`UiRenderer` 组合上述原语，提供单帧的完整渲染：

```rust
renderer.render_frame(fb, list_input, &status, &left_buttons, &right_buttons,
                       show_version, version_info);
```

渲染顺序：清屏 → 缩略图 → 硬件状态栏 → 列表/版本界面 → 按钮提示。

---

## 10. minui-power：电源管理

**对应原版**：`api.c` 中 PWR 部分。Rust 实现在 `crates/minui-power/src/lib.rs`。

### 9.1 PowerManager — 电源管理状态机

```rust
struct PowerManager {
    is_asleep: bool,
    idle_time_ms: u32,            // 距上次输入时间
    sleep_time_ms: u32,           // 休眠持续时间
    autosleep_timeout_ms: u32,    // 30_000ms
    autopoweroff_timeout_ms: u32, // 120_000ms
    battery_charge: u8,           // 0/10/20/40/60/80/100
    brightness: u8,               // 0-10
    volume: u8,                   // 0-20
    show_setting: u8,             // 0=无, 1=亮度, 2=音量
    poweroff_requested: bool,
}

fn update(&mut self, dt_ms: u32) -> bool;   // 每帧调用
fn prevent_autosleep(&self, has_hdmi: bool) -> bool;
fn check_autosleep(&mut self, dt_ms: u32) -> bool;
fn handle_setting_input(&mut self, pad, ...) -> bool;
```

### 9.2 休眠状态机

```
活跃 ──(30s 无操作)──→ 休眠 ──(2 分钟)──→ 自动关机
  ↑                      │
  └───(按电源键唤醒)──────┘
```

---

## 11. 平台实现 (platforms/)

每个平台是一个独立的 crate，位于 `platforms/<name>/`。

### 11.1 结构

```
platforms/rg35xxplus/
├── Cargo.toml              ← [lib] + [[bin]] keymon
├── src/
│   ├── lib.rs              ← impl Platform for Rg35xxPlus
│   ├── keymon.rs           ← 按键监控守护进程
│   └── libmsettings.rs     ← 文件通信设置
├── keymon/credits.txt      ← 开发者致谢
├── cores/
│   ├── makefile            ← 本平台的核心列表
│   └── patches/            ← 平台专属补丁
├── install/                ← 安装脚本
├── show/                   ← 安装画面程序
└── ...                     ← 其他平台专属组件
```

### 11.2 keymon — 按键监控守护进程

平台 crate 中的独立二进制（`[[bin]]`），单独编译。独立于 minui/minarch 运行，直接读取 `/dev/input/event1` 的 evdev 事件，处理 MENU、VOL+、VOL- 三个按键。

- MENU + VOL+/VOL- = 亮度调节
- 单按 VOL+/VOL- = 音量调节（应用 `amixer` 命令到硬件）

后台线程每 1 秒检查 HDMI 状态。必须在开机最早期启动，仅有 `std` 依赖，不链接任何 MinUI 库。

### 11.3 libmsettings — 文件通信替代共享内存

取代原版基于 POSIX 共享内存的 `libmsettings.so`。改用 `/tmp/settings/` 目录下的纯文件：

```
/tmp/settings/
├── brightness       ← 0-10
├── volume           ← 0-20
├── jack             ← 0/1
├── hdmi             ← 0/1
└── mute             ← 0/1
```

keymon 是 writer，minui/minarch 是 reader。`/tmp` 是 tmpfs（内存文件系统）。优势：无 unsafe、崩溃安全、可调试。

### 11.4 平台列表

| 平台 | 设备 | SoC |
|------|------|-----|
| rg35xxplus | RG35XX Plus/H/SP/40XX/CubeXX/34XX | Allwinner H700 |
| rg35xx | RG35XX (original) | Allwinner H700 |
| miyoomini | Miyoo Mini / Mini Plus | SigmaStar SSD202D |
| trimuismart | Trimui Smart | Allwinner S3 |
| tg5040 | Trimui Smart Pro / Brick | Allwinner TG5040 |
| rgb30 | Powkiddy RGB30 | Rockchip RK3566 |
| m17 | M17 | Rockchip RK3326 |
| my282 | Miyoo A30 | Allwinner A33 |
| my355 | Miyoo Flip | Rockchip RK3566 |
| magicmini | MagicX XU Mini M | Rockchip RK3326 |
| zero28 | MagicX Mini Zero 28 | Allwinner A133 |
| gkdpixel | GKD Pixel | Ingenic X1000 |

---

## 12. 构建系统：xtask + cores + skeleton

### 12.1 xtask — Rust 构建工具

Rust 生态中的构建工具模式。`cargo xtask build --platform rg35xxplus` 一条命令：

1. 交叉编译 `minui` → target 目录
2. 交叉编译 `minarch` → target 目录
3. 交叉编译 `keymon`（来自 `platform-rg35xxplus` crate 的 `[[bin]]`）
4. 复制 `skeleton/` 到临时 staging 目录（`target/minui-package/rg35xxplus/`）
5. 放入三个编译产物到 `staging/SYSTEM/rg35xxplus/bin/`
6. 打包为 `MinUI-rg35xxplus.zip`

**skeleton 永不被污染**。构建产物均在 `target/` 下，打包时才从 skeleton 复制到临时目录组装。

```
cargo xtask build --platform rg35xxplus       # 编译 + 打包
cargo xtask build-debug --platform rg35xxplus  # debug 版本
cargo xtask list-platforms                     # 列出支持的设备
```

### 12.2 cores — libretro 核心编译

libretro 核心用 C/C++ 编写，需用交叉编译器单独编译。沿用原版 makefile：

```
cores/
├── makefile              ← 模板：git clone → patch → make → 收集产物
└── patches/              ← 共享补丁（所有平台都需要）

platforms/rg35xxplus/cores/
├── makefile              ← 平台职责：CORES 列表 + 覆盖变量
└── patches/              ← 平台专属补丁（仅此平台需要）
```

两级补丁：`cores/patches/`（模拟器本身有 bug）和 `platforms/<名>/cores/patches/`（特定 SoC 兼容性）。

```bash
make -C platforms/rg35xxplus/cores
cp cores/build/rg35xxplus/output/*.so skeleton/SYSTEM/rg35xxplus/cores/
```

**为什么不用 xtask 编译核心？** 核心编译涉及 git clone、C 交叉编译器、平台特定的 makefile 参数——这是 C 世界的构建流程，xtask 只负责 Rust 编译和最终打包。

### 12.3 skeleton — SD 卡模板

发布到用户 SD 卡上的完整目录结构，包括空文件夹、启动劫持脚本（`BOOT/`）、PAK 定义（`SYSTEM/<平台>/paks/`）、字体资源等。

---

## 13. 原 C 代码到 Rust 的完整映射

### 13.1 源文件映射

| 原 C 文件 | Rust 文件 |
|-----------|----------|
| `defines.h` | `common/src/types.rs` + `common/src/paths.rs` |
| `utils.h/c` | `common/src/utils.rs` |
| `api.h/c`（GFX） | `crates/minui-render/` |
| `api.h/c`（PWR） | `crates/minui-power/` |
| `minui.c`（数据结构） | `common/src/types.rs` |
| `minui.c`（扫描） | `crates/minui/src/scan.rs` |
| `minui.c`（启动） | `crates/minui/src/launch.rs` |
| `minui.c`（主循环） | `crates/minui/src/state.rs` |
| `minarch.c` | `crates/minarch/src/*.rs`（core, game, config, input, video, audio, save, menu, main_loop） |
| `platform.h` | `crates/minui-platform/` |
| `platform.c`（各平台） | `platforms/<name>/src/lib.rs` |
| `keymon/keymon.c` | `platforms/<name>/src/keymon.rs` |
| `libmsettings/` | `platforms/<name>/src/libmsettings.rs` |
| `all/cores/makefile` | `cores/makefile` |
| `<平台>/cores/makefile` | `platforms/<name>/cores/makefile` |
| `skeleton/` | `skeleton/`（不变） |

### 13.2 完整函数映射表

#### 数据结构 (C → Rust)

| C 类型/函数 | Rust 等价 | 备注 |
|------------|----------|------|
| `Array` + `Array_*` 函数族 | `Vec<T>` | 标准库提供 |
| `Hash`（线性查找 KV） | `HashMap<String, String>` | O(1) |
| `IntArray`（定长 27） | `Vec<usize>` | 不限长 |
| `Entry` + `Entry_new/Free` | `Entry` struct | 所有权管理 |
| `Directory` + `Directory_new/Free` | `Directory` struct | 同上 |
| `Recent` + `Recent_new/Free` | `Recent` struct | 同上 |

#### 工具函数 (C → Rust)

| C 函数 | Rust 函数 | 差异 |
|--------|----------|------|
| `prefixMatch` | `prefix_match` | `.eq_ignore_ascii_case()` |
| `suffixMatch` | `suffix_match` | 同上 |
| `exactMatch` | `exact_match` | `==` |
| `hide` | `hide` | 完全等价 |
| `getDisplayName` | `get_display_name` | 完全等价 |
| `getEmuName` | `get_emu_name` | 完全等价 |
| `getEmuPath` | `get_emu_path` | 完全等价 |
| `exists` | `path_exists` | `Path::exists()` |
| `putFile` / `getFile` | `put_file` / `get_file` | `fs::write` / `fs::read_to_string` |

#### 扫描函数 (C → minui/src/scan.rs)

| C 函数 | Rust 函数 |
|--------|----------|
| `hasEmu` | `has_emu` |
| `hasCue` | `find_cue` |
| `hasM3u` | `find_m3u` |
| `hasRoms` | `has_roms` |
| `addEntries` | `scan_dir` |
| `getEntries` | `get_entries` |
| `getRoot` | `get_root` |
| `getRecents` | `get_recents_from_list` |
| `getCollection` | `get_collection` |
| `getDiscs` | `get_discs` |
| `getFirstDisc` | `get_first_disc` |
| `Directory_new` + `Directory_index` | `make_directory` |
| `hasRecents` | `load_recents` |

#### 启动函数 (C → minui/src/launch.rs)

| C 函数 | Rust 函数 |
|--------|----------|
| `queueNext` | `queue_next` |
| `escapeSingleQuotes` | `escape_single_quotes` |
| `autoResume` | `auto_resume` |
| `readyResumePath` | `ready_resume_path` |
| `openPak` | `open_pak` |
| `openRom` | `open_rom` |
| `Entry_open` | `entry_open` |

#### 导航/状态 (C → minui/src/state.rs)

| C 函数/变量 | Rust 方法 |
|------------|----------|
| `top`（全局） | `stack.last()` |
| `saveLast` | `save_last` |
| `loadLast` | `load_last` |
| `saveRecents` | `save_recents` |
| `addRecent` | `add_recent_direct` |
| `Menu_init` | `init_menu` |
| `openDirectory` | `open_directory` |
| `closeDirectory` | `close_directory` |
| `main()` | `run()` |

#### 渲染/电源 (C → Rust)

| C 函数 | Rust 函数 | 所属 |
|--------|----------|------|
| `GFX_blitPill` | `fb_draw_pill` | minui-render |
| `GFX_blitBattery` | `fb_draw_battery` | minui-render |
| `GFX_blitText` | `fb_draw_text` | minui-render |
| `GFX_blitMessage` | `draw_message` | minui-render |
| `GFX_truncateText` | `truncate_text` | minui-render |
| `PWR_init` / `PWR_update` | `PowerManager::new()` / `update()` | minui-power |
| `PWR_preventAutosleep` | `prevent_autosleep` | minui-power |

### 13.3 数据结构映射

| C | Rust | 改进 |
|---|------|------|
| `int type` | `EntryType` enum | 编译时穷举检查 |
| `int mode` | `RenderMode` enum | 语义化 |
| `#define BUTTON_NA -1` | `const NA: i32 = -1` | 类型化常量 |
| `void*` 泛型容器 | `Vec<T>` 单态化 | 编译时类型检查 |
| `static` 全局变量 | `MinUi` 结构体字段 | 可测试、可序列化 |

---

## 14. Rust 重写的优化与改进

### 14.1 内存安全

| 原 C 问题 | Rust 解决 |
|----------|----------|
| `malloc`/`free` 配对错误 → 泄漏/use-after-free | 所有权系统，`Drop` 保证释放 |
| `char*` 缓冲区溢出 (`strcpy`, `sprintf`) | `String` 自动扩容，`format!()` 编译时检查 |
| NULL 指针解引用 | `Option<T>` 强制处理 |
| 全局可变状态竞态 | `&mut self` 保证单线程独占 |

### 14.2 可测试性

| 原 C | Rust |
|------|------|
| 需要实际硬件 | `TestPlatform` 在普通电脑上运行 |
| 全局变量使测试互干扰 | `MinUi::new()` 每次独立状态 |
| 无单元测试框架 | `#[test]` + `cargo test` |

### 14.3 代码质量

| 指标 | 原 C | Rust |
|------|------|------|
| 手写容器代码 | ~100 行 (Array, Hash) | 0 (Vec, HashMap) |
| 内存管理代码 | 分散在 20+ 个 `Free` 函数 | 0 (自动 Drop) |
| NULL 检查 | 7 处隐式 | 全部 `Option` 显式处理 |
| 编译时保证 | 无 | 借用检查、穷举匹配、trait bound |

### 14.4 架构改进

- **全局状态封装**：原 C 有 12 个 `static` 变量，Rust 统一在 `MinUi` 结构体
- **纯函数优先**：扫描函数不依赖全局状态，可独立测试
- **分离进程**：minui 和 minarch 是两个独立二进制，通过文件通信，互不引用
- **libmsettings 文件化**：用 `/tmp/` 文件替代共享内存，无 unsafe、可调试

### 14.5 渲染层的差异

| 方面 | 原 C (SDL) | Rust (fontdue) |
|------|-----------|----------------|
| 依赖 | SDL 1.2/2 + SDL_ttf + SDL_image | fontdue（纯 Rust） |
| 字体 | FreeType 通过 SDL_ttf | 直接光栅化 |
| UI 精灵 | `assets@2x.png` 图片文件 | 纯代码绘制 |
| Alpha 混合 | SDL 内置 | 自定义实现 |
| 平台依赖 | 需要目标平台有 SDL | 零外部 C 依赖 |

---

## 15. 如何新增一个平台

### 步骤 1：创建平台 crate

```
platforms/<name>/
├── Cargo.toml          ← 依赖 minui-platform + minui-core
├── src/
│   ├── lib.rs          ← impl Platform
│   ├── keymon.rs       ← 按键监控
│   └── libmsettings.rs ← 文件通信设置
└── cores/
    ├── makefile        ← CORES 列表
    └── patches/        ← 平台专属补丁
```

### 步骤 2：实现 Platform trait

```rust
pub struct MyPlatform { ... }

impl Platform for MyPlatform {
    const FIXED_WIDTH: u32 = 640;
    const FIXED_HEIGHT: u32 = 480;
    const SDCARD_PATH: &'static str = "/mnt/sdcard";
    const KEY_UP: i32 = 103;  // evdev 扫描码
    // ...

    fn init_video(&mut self) -> Result<Framebuffer, String> {
        // open /dev/fb0 → mmap
    }
    fn poll_input(&mut self, pad: &mut PadContext) {
        // read /dev/input/event*
    }
    // ...
}
```

### 步骤 3：添加到 workspace 和 xtask

```toml
# Cargo.toml
[workspace]
members = ["platforms/<name>"]
```

```rust
// xtask/src/main.rs
const PLATFORMS: &[Platform] = &[
    Platform { tag: "<name>", pkg: "platform-<name>", ... },
];
```

### 步骤 4：在 skeleton 中创建平台系统目录

```
skeleton/SYSTEM/<name>/
├── bin/.keep
├── cores/.keep
├── lib/.keep
├── system.cfg
└── paks/MinUI.pak/launch.sh
```

### 验证清单

- [ ] 屏幕能正常显示（framebuffer 地址和 pitch 正确）
- [ ] 所有按键能正确映射
- [ ] 电池电量正确显示
- [ ] 休眠/唤醒正常
- [ ] 亮度/音量调节生效
- [ ] SD 卡路径正确
- [ ] HDMI 输出切换正常（如支持）

---

## 16. 测试策略与覆盖

### 16.1 测试分类

```
类型        数量    模块              内容
───────────────────────────────────────────
数据结构      5     types              Button 位运算, Directory 属性
路径          1     paths              SD 卡路径拼接
字符串工具   16     utils              匹配, 显示名, 文件 I/O
文件扫描     10     scan               扫描, 根目录, M3U, 映射, 索引
游戏启动      2     launch             Shell 转义
菜单导航      7     state              最近游戏, 导航, 输入, 恢复
渲染          6     render             RGB565, 填充, 圆角, Alpha
电源         10     power              休眠, 关机, 亮度, 音量
───────────────────────────────────────────
总计         57
```

### 16.2 测试基础设施

```rust
// TestPlatform — 零硬件依赖
let mut platform = TestPlatform::new();  // 640x480 RGB565 内存 framebuffer
let mut minui = MinUi::new();
let renderer = UiRenderer::with_default_font(2, 640, 480);
let mut power = PowerManager::new();

minui.init_menu("/tmp/test_sdcard", "test", "/tmp/test_sdcard/.system/test/paks");
assert!(minui.total_entries() > 0);
```

每个测试创建独立临时目录（`/tmp/minui_scan_test_<name>_<id>`），原子计数器确保并行测试互不干扰。

---

## 17. 当前模块架构

```
minui-rs/
├── Cargo.toml              ← workspace（18 members）
├── README.md               ← 完整报告（本文档）
├── resources/              ← BPreplayBold-unhinted.otf
│
├── crates/
│   ├── common/src/         ← types.rs (330行), utils.rs (380行), paths.rs (80行)
│   ├── minui-platform/src/ ← platform.rs (370行), paths.rs (90行)
│   ├── minui-render/src/   ← lib.rs (860行, UiRenderer + 字体 + 原语)
│   ├── minui-power/src/    ← lib.rs (260行, PowerManager)
│   ├── minui/src/          ← state.rs (820行), scan.rs (1010行), launch.rs (310行)
│   └── minarch/src/        ← core/game/config/input/video/audio/save/menu/main_loop.rs
│
├── platforms/
│   ├── tg5040/src/         ← 🟡 lib.rs (常量就绪, IO待实现), keymon.rs, libmsettings.rs
│   └── */src/              ← (其余 11 个平台：存根，待适配)
│
├── cores/                  ← makefile + patches/
├── skeleton/               ← SD 卡模板
└── xtask/                  ← cargo xtask build
```

---

## 18. 实现状态与路线图

### 18.1 当前状态

| 模块 | 状态 | 说明 |
|------|------|------|
| `common` | ✅ 完成 | types, utils, paths 全部就绪 |
| `minui-platform` | ✅ 完成 | Platform trait + Framebuffer + TestPlatform |
| `minui-render` | ✅ 完成 | RGB565 软件渲染器 + fontdue 字体 |
| `minui-power` | ✅ 完成 | PowerManager 状态机 |
| `minui` (启动器) | ✅ 完成 | 扫描、导航、启动逻辑全部就绪 |
| `minarch` (模拟器前端) | 🟡 骨架就绪 | trait 和模块结构完成，核心逻辑待实现 |
| **tg5040 平台** | 🟡 **开发中** | 常量已定义，IO 方法为存根 |
| 其余 11 个平台 | ⬜ 存根 | 编译占位，待适配 |
| `cores/` (libretro 构建) | ✅ 完成 | makefile 模板 + 补丁全部就位 |
| `skeleton/` (SD 卡模板) | ✅ 完成 | 目录结构和启动脚本就位 |
| `xtask` (构建工具) | ✅ 完成 | 编译 + 打包一条命令 |

### 18.2 下一步计划

- [ ] `crates/minarch` — 模拟器前端核心逻辑
- [ ] `platforms/tg5040` — 平台 IO 实现（视频/输入/音频/电源）
- [ ] TG5040 libretro 核心编译（`make -C platforms/tg5040/cores`）
- [ ] 真机打包测试（`cargo xtask build --platform tg5040`）
- [ ] Brick 变体适配（1024×768, scale=3）
- [ ] `platforms/*` — 其余 11 个平台适配
