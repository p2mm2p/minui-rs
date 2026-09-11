# minarch — MinUI 游戏内前端

`minarch` 是 MinUI 的"游戏内界面"。当你从启动器（minui）选中一个 ROM 后，minarch 被 shell 脚本拉起，负责：加载 libretro 模拟核心（.so 动态库）、运行游戏循环、提供游戏内菜单（即时存档、画面设置、换碟）、以及退出后回到 minui。

## 模块定位与职责

```
开机 ──→ minui（启动器，浏览 SD 卡选 ROM）
              │
              │ 写入 /tmp/next → shell 脚本 → 启动 minarch
              ▼
          minarch（本 crate）  ←────┐
              │                    │
              ├── 加载 libretro .so │
              ├── 游戏循环          │  用户按 MENU
              │   ├── 输入 → 核心   │  弹出游戏内菜单
              │   ├── 核心渲染 → 画面缩放 → flip
              │   └── 核心音频 → 重采样 → push_audio
              │                    │
              └── 退出 → shell 脚本重新拉起 minui ─┘
```

**核心职责**：作为 libretro 核心和掌机硬件之间的桥梁。
- 通过 `libloading` 动态加载 `.so` 文件
- 实现 libretro 的 `retro_environment` / `retro_video_refresh` / `retro_audio_sample` 等回调
- 管理游戏存档（savestate 快照 + SRAM 电池存档）
- 提供游戏内菜单（即时存档/读档、画面比例切换、音量、换碟）
- 处理 HDMI 输出时的分辨率/缩放适配

## 内部结构（子组件）

```
crates/minarch/src/
├── lib.rs           ← crate 根：pub mod 声明（lib 目标，供 tests/ 集成测试链接）
├── main.rs          ← 装配层入口（bin）：启动序列、主循环、菜单子循环、睡眠、退出
├── assembly.rs      ← 装配层可测纯逻辑（lib 目标）：存档编排、resume 解析、线程状态机
├── libretro.rs      ← libretro ABI 地基：类型/常量/Core 加载器/回调桥
├── game.rs          ← 游戏文件组织（zip 解压/m3u 探测/换碟）
├── environment.rs   ← retro_environment() 回调分发实现：30 case 纯分发 + 薄静态
├── savestate.rs     ← 存档快照持久化：快照路径命名与读写
├── sram.rs          ← 电池存档持久化：SRAM/RTC 路径与读写
├── config.rs        ← 前端/核心配置纯逻辑层：OptionList 注册表、前端选项表、cfg 解析
├── audio.rs         ← 音频引擎：核心回调 → 重采样 → 队列引擎 + 薄静态层
├── vibration.rs     ← 振动引擎：排队 + 帧去抖状态机 + 薄静态层
├── menu.rs          ← 游戏内菜单 UI：主菜单状态机、选项子菜单框架、菜单绘制、存档交互编排
├── controls.rs      ← 输入映射纯逻辑层：ButtonMapping 模型与表、核心映射初始化、按键映射
└── hdmi.rs          ← HDMI 热插拔检测（纯逻辑状态机）
```

**当前状态**：全部 15 个模块已完整实现并有测试覆盖；装配层（`main.rs` + `assembly.rs`）已闭环接线——启动序列、主循环、菜单子循环（含 Sleep/PowerOff 的 before/after 序列）、thread_video 线程模式、菜单选项写回 user cfg 均已完成（详见「main.rs + assembly.rs」章节的边界清单）。

## 运行逻辑（模块内部流程）

### 主循环（简化）

```
1. main() 解析命令行参数（核心路径、ROM 路径——参数缺失打印用法并非零退出）
2. 初始化 Platform（视频 + 输入 + 音频）
3. 加载 libretro 核心（libloading::Library::new）
4. 调用 retro_init() → retro_load_game(rom_path)
5. 进入游戏循环:
   while running:
     poll_input()                ← 读取掌机按键
     retro_run()                 ← 执行一帧模拟
       → retro_video_refresh()   ← 核心回调：输出一帧画面
       → retro_audio_sample()    ← 核心回调：输出音频采样
     scaler.scale()              ← 画面缩放到 640×480
     blit_hardware_group()       ← 渲染状态栏（电池、WiFi）
     platform.flip()             ← 提交到屏幕
     if MENU pressed:
       show_menu()               ← 游戏内菜单
6. retro_unload_game() → retro_deinit()
7. 退出 → shell 脚本重新拉起 minui
```

### `<P: Platform>` 泛型参数的作用

你会看到 minarch 中大量出现 `<P: Platform>`。这个 `P` 是 Rust 的**泛型参数**（generic parameter）——可以理解为"占位符"，在编译时会被替换为具体的平台类型（如 `Tg5040`）。

**通俗解释**：类比 USB 接口。USB 规范定义了"供电+数据传输"的标准，任何符合规范的设备（U 盘、键盘、风扇）都能插上去工作。`Platform` trait 就是 MinUI 的"USB 规范"，`Tg5040` 是具体的"设备"。minarch 的代码只对着"规范"写（`P: Platform`），不关心插上来的是什么设备。

**为什么不用运行时多态（dyn trait）？** Rust 的泛型在编译时展开（单态化），编译器为每个平台生成一份专用的 minarch 代码。这避免了运行时的虚函数调用开销——在掌机这种性能受限的 ARM 设备上，每帧 17ms 的预算非常紧张，不需要的抽象一层都不能多。

### 与 minui 的通信

minui 和 minarch 是两个独立进程，通过 `/tmp` 文件通信：

```
minui                                minarch
  │                                     │
  ├── 写入 /tmp/next                    │
  │   "trimui_tg5040/Roms/GB/game.gb"   │
  │                                     │
  ├── exec shell 脚本 ──────────────────→ 读取 /tmp/next
  │                                       加载核心 + ROM
  │
  │                                     │ 用户退出
  │  读取 /tmp/next                      │
  │←──── shell 脚本重新拉起 minui ─────── 写入 /tmp/next
  │   (可能包含"回到最近游戏"标记)         │   (可能为空 = 回到主菜单)
```

这个设计沿用了原 C 版本，没有改动。它不是最优雅的方案，但它在 20+ 个平台上被验证了 5 年以上，稳定可靠。

### 与 minui 的 /tmp 协议全家（除 /tmp/next 外的三个文件）

minui ↔ minarch 之间不止 `/tmp/next` 一个文件，还有三个跨进程协议文件（常量定义在 `common::paths`，读写方如下）：

| 文件 | 谁写 | 谁读 | 语义 |
|------|------|------|------|
| `/tmp/resume_slot.txt` | minui（`launch::open_rom`：续玩写槽位号、非续玩写 `8` 隐藏默认槽、`auto_resume` 写 `9`） | minarch（启动时 `assembly::read_resume_slot`：读取后**删除**） | 续玩槽位传递——minarch 按槽位自动读档 |
| `/tmp/change_disc.txt` | minarch（`game::change_disc` 换碟时；即使核心未注册换碟回调也写） | minui（`recents::find_recents` 读后**删除**，条目注入列表顶部） | 换碟请求回传——minui 更新最近游玩列表 |
| `.userdata/shared/.minui/auto_resume.txt` | minarch（睡眠前/HDMI 重启前写 ROM **相对路径**） | minui（`launch::auto_resume` 读后删除，校验仍存在则直接续玩启动） | 自动续玩标记——"睡死/重启后回到游戏" |

槽位语义约定：`AUTO_RESUME_SLOT = 9` 是自动档；minarch 读到缺失/`8`/解析失败时按 `9`（自动档）处理。这套文件协议与 `/tmp/next` 一样原样沿自 C，两个进程各自按约定读写，无共享内存。

## 与原 C minarch 的对比

| 维度 | 原 C（~4,800 行） | Rust |
|------|-------------------|------|
| 核心加载 | `dlopen` + 手动函数指针类型转换 | `libloading` crate，类型安全的符号查找 |
| 音频 | 全局 `snd` 数组 + pthread 回调 | `AudioRingBuffer` + `Resampler`（common::audio） |
| 存档 | 全局变量 + memcpy | 所有权系统保证存档数据不意外共享 |
| 画面缩放 | ~3,000 行 scaler（含 NEON 汇编） | `render::scaler`（IntegerScaler + AaScaler，~560 行） |
| 配置 | 全局变量 + 手动 ini 解析 | `config.rs` 模块封装 |
| 状态栏 | 渲染函数内部调用 `GetHDMI()` | `HardwareStatus` struct 传入，不回调平台 |

## 关键技术拓展

### 什么是 libretro？

libretro 是一个模拟器前端 API 规范。它把"模拟核心"和"前端界面"分离——核心负责模拟游戏机（FC、GBA、PS1……），前端负责画面显示、音频播放、输入处理、存档管理。

```
┌────────────────────────────────────┐
│            minarch（前端）          │  ← 本 crate
│  画面缩放 | 音频重采样 | 存档管理   │
│  菜单 UI | 按键映射 | 状态栏       │
└──────────────┬─────────────────────┘
               │ 回调函数指针
┌──────────────▼─────────────────────┐
│    libretro 核心（.so 动态库）     │
│  gambatte.so / snes9x.so / ...    │  ← cmake 编译，保持原样
│  模拟 CPU + PPU + APU + Cartridge  │
└────────────────────────────────────┘
```

MinUI 使用 libretro 而非自己写模拟器。这让你可以在同一个启动器里玩十几种游戏机——Gambatte 模拟 GB/GBC、Snes9x 模拟 SFC、PCSX-ReARMed 模拟 PS1……

### 即时存档 vs 电池存档

- **即时存档（Savestate）**：把模拟器的完整状态（CPU 寄存器、内存、PPU 状态）序列化成一个文件。任何时候都能存/读，就像游戏机的"时间暂停"。
- **电池存档（SRAM）**：模拟游戏卡带里的纽扣电池供电的存档芯片。只在游戏内"存档"操作时写入，对应 `.sav` 文件（路径拼接见 minarch.c:386/435）。

---

## 公开 API 说明

minarch 是 **lib + bin** 双目标 crate：

- **lib 目标**（`src/lib.rs`）：对外暴露 `minarch::libretro` 模块（ABI 类型、`Core` 加载器、`register_callbacks`），供 `tests/` 集成测试直接链接——这是与 minui（bin-only）不同的结构，引入理由见「测试」章节
- **bin 目标**（`src/main.rs`）：装配层入口（薄胶水 + 平台门控），完整实现见「main.rs + assembly.rs」章节

命令行接口：

```sh
minarch <核心.so 路径> <ROM 路径>
```

参数说明：
- `核心.so 路径`：libretro 核心动态库路径（如 `.system/tg5040/paks/GBC.pak/gambatte_libretro.so`）
- `ROM 路径`：ROM 文件路径（支持 `.zip` 压缩包，通过 `flate2` 解压）；目录或 cue/m3u 形态由核心与装配层共同处理
- 两个参数任一缺失：打印用法并退出码 1（对应 C :4676-4684）

## 关键代码解析

### libretro 模块（地基）：22 个符号的加载与回调桥

**概念背景**：minarch 要跑游戏，第一步是把模拟核心（`.so` 动态库）"请进来"。libretro ABI 就是请柬的格式——核心按固定符号名导出函数（`retro_init`/`retro_run`/…），前端按固定签名绑定它们。C 版用两样东西完成：构建时 `git clone` 的官方头文件 `libretro.h` + `Core_open` 里的 `dlopen`/`dlsym`（minarch.c:2891-2955）。Rust 版把它们对应为 `libretro.rs` 一个模块：

```
dlopen(.so)
  ├─▶ 22 次 dlsym ──▶ Core 结构体（函数指针字段，与 C struct Core 对应）
  ├─▶ retro_get_system_info ──▶ 信息缓存 name/version/extensions/need_fullpath
  └─▶ 6 个 trampoline 经 set_* 交给核心 ──▶ 核心每帧"回头"调用前端
```

| C | Rust |
|---|------|
| `libretro.h` 头文件 | 手写 `#[repr(C)]` 子集（17 结构体 + 3 枚举 + 常量），布局由 `tests/libretro_abi.rs` 的 `size_of`/`offset_of` 断言校验（期望值派生自 vendored `tests/fixtures/libretro.h`） |
| `dlopen`/`dlsym` | `libloading::Library` + 私有 `load_symbol` 辅助（解引用复制出裸 fn 指针，与句柄生命周期解耦） |
| 全局变量承接核心回调 | `OnceLock<FrontendState>` 静态桥 + 6 个 `extern "C"` trampoline |
| dlsym 失败 → NULL → 运行时崩溃 | 20 个必需符号缺失返回 `CoreError::SymbolMissing { name }`，启动即报错、可诊断 |

**C 缺陷对照**：C 版 dlsym 拿到 NULL 后直接调用 = 段错误；Rust 版每次绑定都过 `load_symbol`，失败返回 `SymbolMissing`——把"跑起来才崩"提前成"打不开就报错"。另外 `retro_load_game_special`/`retro_get_region` 两个符号 C 版绑而不用，Rust 以 `Option<fn>` 预留，缺失不报错。

### 核心加载（真实代码）

```rust
use minarch::libretro::Core;

fn example() -> Result<(), minarch::libretro::CoreError> {
    // dlopen + 22 符号 dlsym + retro_get_system_info 填充信息缓存
    let core = Core::open(
        "/mnt/SDCARD/.system/tg5040/paks/GBC.pak/gambatte_libretro.so",
        "GBC",                       // 模拟器标签
        "/mnt/SDCARD/.userdata/arm-480/GBC-gambatte",  // config_dir
        "/mnt/SDCARD/.userdata/arm-480/GBC-gambatte",  // states_dir
        "/mnt/SDCARD/Saves/GBC",     // saves_dir
        "/mnt/SDCARD/Bios/GBC",      // bios_dir
    )?;

    assert_eq!(core.name, "gambatte");  // basename 截去末尾 _libretro
    // 预留符号：retro_load_game_special / retro_get_region
    // （C 版绑而不用，缺失时对应字段为 None）
    assert!(core.load_game_special.is_some());
    Ok(())
}
```

**为什么这段代码有 unsafe？** `libloading` 加载动态库时，编译器无法验证 `.so` 文件中的函数签名是否正确——这份信任从编译器转移到了开发者身上。`Core::open` 内部完成全部 `unsafe` 操作并暴露安全的 `Result` 接口：dlopen 失败返回 `LibraryOpenFailed`，必需符号缺失返回 `SymbolMissing`，加载成功则 22 个符号保证全部非空（调用时仍需 `unsafe` 块，因为核心是外部 C 代码）。

### 回调桥：核心如何"回头"调用前端

核心不拥有画面/音频/输入，它通过函数指针回调前端。Rust 的 `extern "C" fn` 不能捕获环境（闭包），所以 `libretro.rs` 提供 6 个 trampoline，经一次性注册表转发到处理器：

```rust
use minarch::libretro::{register_callbacks, FrontendState, video_refresh_trampoline};

fn example() -> Result<(), FrontendState> {
    // 装配层启动时注册一次（核心加载之前），此后只读——无锁设计
    register_callbacks(FrontendState {
        video_refresh: Some(|data, width, height, pitch| {
            // 帧数据在这里处理（缩放、渲染）
        }),
        ..FrontendState::default()
    })
}
```

注册完成后，把 trampoline 交给核心即可（`unsafe { (core.set_video_refresh)(video_refresh_trampoline) }`），核心渲染每帧时就会回头调用它。`FrontendState` 的 6 个处理器字段（environment/video_refresh/audio_sample/audio_sample_batch/input_poll/input_state）全部为 `Option`：

- 未注册的处理器由 trampoline 返回安全默认值（环境回调 `false`、音频批量返回 `frames`、输入返回 `0`），不 panic
- 重复注册返回错误并携带第二次传入的状态（`OnceLock::set` 语义）
- 字符串字段（`library_name` 等）所有权归核心（静态字符串）：前端只读、**不释放**

**边界与半环**：`register_callbacks` 的注册点（装配层 main.rs，核心加载前一次）与 `Core` 生命周期消费方（init/load/run 编排）在装配层实现——见装配章节边界清单Requirement 与「minarch 半环闭环清单」。

### config.rs：OptionList 核心选项注册表

**概念背景**：每个核心都有一堆"调校旋钮"——画面缩放、滤镜、超频档位、防撕裂……核心通过环境回调 `SET_CORE_OPTIONS`/`SET_VARIABLES` 把自己的选项表上报给前端；前端要把它们装进游戏内菜单、写进 cfg 文件、并在核心查询时（`GET_VARIABLE`）给出当前值。`config.rs` 的 `OptionList` 就是这份"选项注册表"——核心上报与前端消费之间的中间层，**纯逻辑**（不碰文件系统与平台）。

```
核心 ── SET_CORE_OPTIONS(定义数组) ──▶ from_core_options ──▶ OptionList
核心 ── SET_VARIABLES(vars 数组) ────▶ from_variables   ──▶ 同上
菜单 ── set_option_value(key, val) ──▶ changed = true
核心 ── GET_VARIABLE(key) ────────────▶ 返回 values[value] 指针（environment.rs 消费）
cfg 落盘 ── serialize_cfg ──▶ "key = value" 文本   ← 半环：文件 IO 归装配层
```

```rust
use minarch::config::OptionList;

// 环境回调（environment.rs）把核心给的指针原样传入——
// unsafe 解码收敛在构造函数内，深拷贝全部字符串
fn on_core_options(defs: *const minarch::libretro::RetroCoreOptionDefinition) {
    let mut list = unsafe { OptionList::from_core_options(defs) };
    list.set_option_value("gambatte_colorization", "GBC"); // 改值
    assert_eq!(list.get_option_value("gambatte_colorization"), Some("GBC"));
    assert!(list.changed); // GET_VARIABLE_UPDATE 据此通知核心
}
```

**C 语义表**（核心选项生命周期）：

| 时刻 | C | Rust |
|------|---|------|
| 核心上报定义 | `calloc` + `strcpy` 深拷贝进 `config.core` | `from_core_options` 深拷贝为 `String`，重建即释放 |
| 查询当前值 | 返回选项表内字符串指针（零拷贝） | 同（environment.rs 写入 `var->value`） |
| 修改值 | `Option_setValue` 查表 | `set_option_value` 匹配 `values` 列表 |
| 列表替换 | `OptionList_reset` 手工 free 迷宫 | 重建 `Vec`，旧内存自动释放，reset 函数消失 |

**C 缺陷对照**（4 条，不继承）：

1. **无分号 vars → 对 NULL 调 `strchr` 崩溃**：`OptionList_vars` 中 `char* opt = tmp` 后 `strchr(tmp, '|')`（minarch.c:1526-1527）——value 不含 `;` 时 `tmp` 从未赋值即解引用。Rust 定义三态解析规则（有分号/无分号/分号后非空格），不崩溃
2. **静默改值**：`Option_getValueIndex` 未命中或 `value==NULL` 时 `return 0`（minarch.c:1396-1402），`Option_setValue`（1403-1406）就把值静默设为第一项。Rust `set_option_value` 不匹配任何值时**忽略**
3. **`values[128]` 越界读（UB）**：`OptionList_init` 的 values 扫描循环（minarch.c:1422-1496）以 NULL 为哨兵，128 项全非空时越界。Rust 安全截断为 128 项
4. **内存管理**：`OptionList_reset`（minarch.c:1554+）是一段 calloc/free 迷宫。Rust 的 `String`/`Vec` 自持所有权

另外两块拼图：

- **前端选项表** `frontend_options(supports_overscan) -> Vec<ConfigOption>`：构造 8 个前端自有选项，键名 `minarch_*` 与 C 逐字一致——`screen_scaling`（缩放，overscan 支持时多两档）/`screen_effect`（滤镜）/`screen_sharpness`（锐度）/`prevent_tearing`/`cpu_speed`（超频）/`thread_video`（Prioritize Audio）/`debug_hud`/`max_ff_speed`（快进上限）
- **cfg 解析/序列化** `get_cfg_value(cfg, key) -> Option<CfgValue>` / `serialize_cfg(entries) -> String`：`key = value` 文本格式，`-key` 前缀表示锁定项，round-trip 可逆。cfg 文件 IO 与平台联动（改画面效果立即生效）由装配层实现（见装配章节边界清单）

**边界与半环**：cfg 文件 IO、`Config_syncFrontend` 平台联动、`Option.lock` 消费——接线归装配层与 menu（终态见装配章节边界清单）。

### controls.rs：按键映射纯逻辑层

**概念背景**：掌机按键到模拟器按键之间隔着一层"翻译"——掌机按的是 A/B/X/Y 物理键，核心要的是 `RETRO_DEVICE_ID_JOYPAD_*` 位掩码，而且每个核心的键名可能不同（Virtual Boy 有右方向键，PS1 有双摇杆）。`controls.rs` 负责这个翻译层：**映射表 + 位掩码计算**，纯逻辑（轮询与动作执行都不在这里）。

C 的 `input_poll_callback`（minarch.c:1654-1776）把三层职责搅在一起，Rust 版拆开：

```
物理轮询   Platform::poll_input() → InputState     → main.rs 装配层
快捷指令动作 存档/重置/缩放循环/快进切换             → menu.rs / 装配层
按键映射   mapping 遍历 → retro 按键位掩码           → controls.rs（本模块）
```

核心用法（映射表 + 位掩码计算）：

```rust
use common::input::BTN_A;
use minarch::controls::{build_buttons, default_button_mapping};

fn example() {
    let mappings = default_button_mapping(); // 16 项，与 C 逐项一致
    let pressed = BTN_A;                     // 本帧按下掩码（装配层从 InputState 取）
    let buttons = build_buttons(&mappings, pressed, false);
    // 应答核心查询：input_state 时返回第 buttons 位
    assert_eq!(buttons, 1 << minarch::libretro::RETRO_DEVICE_ID_JOYPAD_A);
}
```

**C 语义表**：

| C（`input_poll_callback` 一个函数） | Rust |
|---|---|
| 轮询物理按键 + 读映射 + 算位掩码，三层搅在一起 | 只留映射层；轮询归装配层、动作归 menu |
| `BTN_ID_NONE == -1` 哨兵表示"未绑定" | `Option<usize>` |
| 逻辑方向位（十字键 ∪ 摇杆合并成一位） | common 无此位，折叠移到查询处 |
| `SET_INPUT_DESCRIPTORS` 上报核心键名后重映射 | `init_core_mapping`（unsafe，核心名覆盖默认映射，不支持的按键标 `ignore`） |

**三个值得注意的设计**：

- **`Option<usize>` 取代 C 的 -1 哨兵**：C 用 `BTN_ID_NONE == -1` 表示"未绑定"，Rust 的 `usize` 没有负数，且 common 把 `BTN_ID_NONE` 定义为 `0`（与 `BTN_ID_DPAD_UP` 撞车）——`Option` 是这里语义最诚实的表达
- **摇杆方向折叠**：十字键绑定可被摇杆方向触发（`BTN_DPAD_UP | BTN_ANALOG_UP` 并集查询）——C 用"逻辑方向位"（dpad 和摇杆合并成一个位），Rust 的 common 没有这个位，折叠移到了查询处
- **核心按键重映射**：核心经 `SET_INPUT_DESCRIPTORS` 上报自己的按键名（如 Virtual Boy 的右方向键），`init_core_mapping` 把核心名覆盖到默认映射上，并把核心不支持的按键标记 `ignore`（映射时跳过）

**边界与半环**：handler 实现、buttons 静态状态、快捷指令动作等——接线归装配层，由装配层一次收拢。

### environment.rs：30 case 分发——纯分发 + 薄静态

**概念背景**：核心通过 `retro_environment` 回调向前端提问/上报（“支持画面复制吗？”“我的选项表如下”“我支持双摇杆”……）。C 版 `environment_callback`（minarch.c:1860-2158）是一个摸遍全局变量的 30 case switch。Rust 版按「纯分发 + 薄静态」拆成两层：

```
┌─────────────────────────────────────────────────┐
│ dispatch(cmd, data, state, runtime)  ← 纯分发层 │
│ 30 个 case 全部在此，只操作本地实例、不碰全局     │
│ → 测试用本地实例构造，没有跨测试污染             │
└──────────────┬──────────────────────────────────┘
┌──────────────▼──────────────────────────────────┐
│ STATE / RUNTIME / RUMBLE_HOOK     ← 薄静态层     │
│ OnceLock（注册后只读）+ Mutex（共享可变数据）    │
│ handle() 锁 RUNTIME → 转发 dispatch             │
└─────────────────────────────────────────────────┘
```

**为什么 dispatch 必须是纯函数？** 全局状态是测试的天敌——一个测试改了 `config.core`，另一个测试的断言就作废。dispatch 只吃本地 `state`/`runtime` 参数，测试每个 case 各自构造实例，互不干扰（config/controls 测试已证明这个模式）。全局只留一层薄到不能再薄的静态壳：`handle` 锁一下 `RUNTIME` 就转发。


```rust
use std::ffi::CString;
use minarch::environment;

// 装配层在核心加载前注册一次：目录状态 + 运行时
let bios = CString::new("/mnt/SDCARD/Bios/GBC").unwrap();
let saves = CString::new("/mnt/SDCARD/Saves/GBC").unwrap();
environment::init(bios, saves).expect("环境只注册一次");

// 核心经 trampoline 提问时：handle 锁 RUNTIME → 转发 dispatch（30 case）
let mut can_dupe = false;
let answered = environment::handle(
    minarch::libretro::RETRO_ENVIRONMENT_GET_CAN_DUPE,
    (&mut can_dupe as *mut bool).cast(),
);
assert!(answered && can_dupe);
```

**30 个 case 的 6 组**：

| 组 | case 数 | 内容 | 例子 |
|----|---------|------|------|
| A 常量/简单查询 | 19 | 出参写死的查询 + 无操作应答 | `GET_OVERSCAN`→true、`GET_CORE_OPTIONS_VERSION`→1 |
| B 选项体系 | 6 | 核心选项表的注入/查询/修改 | `SET_CORE_OPTIONS`/`GET_VARIABLE` |
| C 输入描述符 | 1 | 按键映射替换 | `SET_INPUT_DESCRIPTORS` |
| D 换碟 | 2 | 换碟回调表存储 | `SET_DISK_CONTROL_INTERFACE`/`_EXT_INTERFACE` |
| E 振动 | 1 | 填 trampoline | `GET_RUMBLE_INTERFACE` |
| F 控制器信息 | 1 | dualshock 扫描 | `SET_CONTROLLER_INFO` |

**GET_VARIABLE 的指针契约（本模块最容易踩的坑）**：核心查询选项值时，`var->value` 写的是**指向 `OptionList` 内部 String 数据的指针**，而不是新分配一份字符串。这意味着：

- **零拷贝**：核心拿到的就是选项表里躺着的那份数据
- **有效期**：指针在**下一次列表替换**（`SET_CORE_OPTIONS`/`SET_CORE_OPTIONS_INTL`/`SET_VARIABLES`）前有效——替换会重建列表、释放旧 String，旧指针就悬垂了
- **为什么安全**：`set_option_value`/`set_option_raw_value` 只改索引（选 `values[value]` 的哪一项），从不改动字符串内容——列表被替换之前，字符串数据一块内存都不会挪
- **与 C 完全一致**：C 的指针同样在 `OptionList_reset` 后失效，这是 libretro 规范本身的规定（核心须同步读取，不得长期持有）

换个角度看：Rust 的所有权在这里表现为「列表替换即失效」的显式契约，而不是魔法——不安全的地方只有从 String 取出裸指针那一刻，且由 `OptionList` 的替换语义背书。

**与 C 的偏离点**（有意为之的偏离，明细见「设计决策记录」章节）：

1. **`SET_PERFORMANCE_LEVEL` 不落穿**：C 的这个 case 忘了写 `break`，落穿进 `GET_SYSTEM_DIRECTORY` 把 data 当 `char**` 野写 bios_dir（minarch.c:1885-1889）——Rust 显式区分，不再继承
2. **`dualshock` 扫描大小写不敏感**：C 的 `exactMatch` 用 `strncmp`（区分大小写），Rust 用 `eq_ignore_ascii_case`
3. **恒 false 的三处**（与 C 原样一致，非偏离）：`SET_INPUT_DESCRIPTORS`（C :1909）、`GET_LOG_INTERFACE`（Rust 无法稳定定义 C 可变参函数）、`SET_CONTROLLER_INFO`（C :2007 TODO）
4. **rumble 强度截断**：`u16 → u8` 是平台能力上限（`Platform::set_rumble(u8)`）；port/effect 忽略（C TODO 同款）

**边界与半环**：handler 注册进 `FrontendState`、rumble hook 接线（经 `vibration` 引擎）、`controls_mapping`/`disc_control`/`has_custom_controllers` 的消费方——接线归装配层（终态见装配章节边界清单）。

---

### vibration.rs：排队 + 帧去抖状态机

核心（游戏）通过 libretro 的 rumble 接口请求振动时，minarch 并不
立刻驱动马达，而是先"排队"，再由主循环每帧推进——两步各有原因：

- **排队**：核心在 thread_video 模式下从独立线程回调，强度先写进
  `queued`，setter 非阻塞；真正 apply 发生在主循环，天然不在核心线程
- **去抖**：C 原版注释（api.c:1411）说"马达不喜欢 0↔非 0 抖动"。
  游戏每帧"震一下、停一下"会让马达快速抖振。所以关断延迟 3 帧
  （`DEFER_FRAMES 3`）——期间新非零请求到来则立即恢复，否则第 4 帧
  才真正关断。**开启永远立即生效**

状态转移图：

```
             ┌──────────────┐
             │ queued==cur  │──▶ tick() → None（空闲）
             └──────────────┘
             queued ≠ 0 ──▶ Some(queued)（立即应用，defer 清零）
             queued = 0 且 defer < 3 ──▶ None（延迟关断，defer+1）
             queued = 0 且 defer ≥ 3 ──▶ Some(0)（关断）
```

**C 的做法**：独立 pthread（`VIB_thread`）每 17ms `SDL_Delay` 轮询
一次，摸全局 `vib` 结构体，强度变化时调 `PLAT_setRumble`。

**Rust 的做法**：无线程。引擎是纯状态机 `Vibration`（不碰平台、无
IO），主循环每帧调 `vibration::tick()`，拿到 `Some(strength)` 才调
`platform.set_rumble(strength)`：

```rust
// 装配层主循环（每帧）：
if let Some(strength) = vibration::tick() {
    platform.set_rumble(strength);
}
```

**为什么弃用线程**（三个理由）：

1. **先例**：`common::power` 已把 C 的电池监控 pthread 改为帧内轮询
   （power.rs 模块文档："无需独立线程"）——两个模块形态同构
2. **所有权代价**：`Platform::set_rumble(&mut self)` 是 &mut 方法，
   线程方案要把平台静态化 + 全局锁，音频热路径（`push_audio` 每帧）
   也得走这把锁——为 3 帧计数器付全局锁
3. **帧语义**：`DEFER_FRAMES` 本就是帧，C 的 17ms 只是用时间模拟帧；
   60fps 主循环 ≈ 16.7ms/帧，时序等价，Rust 直接有真帧

**睡眠**：`suspend()` 保存实际强度并确定性清零（对应 C main:4246-4247），
唤醒后 `resume(saved)` 恢复（C main:4554-4555）。C 睡眠时只排队清零、
依赖线程唤醒后的执行顺序巧合恢复——Rust 显式清零，消除时序依赖。

**调用链全貌**：

```
核心 ──▶ rumble_trampoline ──▶ RUMBLE_HOOK ──▶ vibration::set_strength（排队）
        (environment.rs)                        vibration::tick()（主循环每帧）
                                                       │ Some(strength)
                                                       ▼
                                                Platform::set_rumble
                                                       ▼
                                                tg5040: gpio227（静音时抑制）
```

hook 接线、tick 驱动、睡眠调用点由装配层接线（见装配章节）。

---

### audio.rs：音频队列引擎

**概念背景**：核心每渲染一帧视频，就把这一帧期间攒下的声音一次性
"哗啦"倒出来（GB 核心每帧约 735 帧音频）；而扬声器那头以恒定速率
匀速流出（48kHz 下每 20.8μs 一帧）。用**水桶**打比方：

```
游戏核心（水龙头，一阵猛倒）            扬声器（排水口，匀速慢流）
  retro_run     retro_run                    ······恒速······
  │735帧│735帧│735帧│              vs
  └─────┴─────┴─────┘                    每帧 20.8μs 不眠不休
```

两种节奏之间必须有个"桶"接着。此外还有两个小麻烦：**单位不同**
（核心采样率随主机种变化：NES≈55930、GB≈43690、SNES≈32040、
PS1=44100；设备固定 48kHz——不换算直接播会变调）；**快进时水龙头
开大**（核心 3~10 倍速产音频，桶瞬间爆满——所以快进时声音直接丢弃）。

数据流（两级桶，各管一段路）：

```
核心线程（thread_video 时独立）               主线程（每帧 drain）
  retro_audio_sample(_batch)                      │
        │ trampoline（libretro.rs）               │
        ▼                                         ▼
  batch_handler ──▶ AudioEngine ──▶ platform.push_audio ──▶ 平台环形缓冲
                  （重采样+队列+快进门控）                     ──▶ SDL 音频线程 ──▶ 扬声器
```

**为什么两级桶**：`Platform::push_audio(&mut self)` 平台归主线程独占，
而 thread_video 模式下回调跑在核心线程——引擎队列让"核心线程只碰
引擎、主线程只碰平台"，两线程各碰各的。平台那级
4000 帧缓冲是给 SDL 回调线程的（突发 vs 匀速），引擎这级是给线程
边界的——两者职责不同，不是冗余。

**C 语义表**：

| C 符号 | 位置 | 职责 | Rust 对应 |
|--------|------|------|-----------|
| `SND_Context` | api.c:928-944 | 全局环形缓冲 + 读写指针 + 重采样器 | `AudioEngine` 字段 |
| `SND_batchSamples` | api.c:1030-1066 | 批量入口（满时 `SDL_Delay(1)`×10 等待） | `process_batch`（丢帧不阻塞） |
| `SND_resampleNone/Near` | api.c:1000-1021 | 直通 / 最近邻重采样 | `common::audio::Resampler` |
| `SND_audioCallback` | api.c:945-979 | SDL 音频线程消费环形缓冲 | tg5040 `AudioCallbackImpl` |
| `audio_sample(_batch)_callback` | minarch.c:2875-2881 | 核心回调入口 + 快进门控 | `sample_handler`/`batch_handler` |

**C 缺陷对照**（不继承的点）：

1. **裸 `static int fast_forward` 数据竞争**：C 主线程写、核心线程读
   （minarch.c:53）——Rust 把快进标志放进引擎字段，由 Mutex 保护
2. **满时等待 10ms 重试**：C 缓冲满时解锁 + `SDL_Delay(1)` 重试 ×10，
   核心线程被拖慢——Rust 满则丢帧、立即返回（音频尽力而为），
   丢帧只发生在主循环停顿时，平台 83ms 缓冲兜底
3. **"倒序回放"伪回声**：C `SND_audioCallback` 缓冲不足时回放已输出
   采样（`*--in`，api.c:975）——已在平台侧否决（静音填充，见 platform-tg5040 文档）
4. **容量算式残留**：C 容量 = `buffer_seconds×rate/fps`，除 `fps`
   是游戏帧计量残留——Rust 固定 4000 帧（与平台缓冲同值）

**Rust 的做法**（真实 API）：

```rust
// 装配层启动：核心加载后初始化引擎（core_rate 来自 av_info.timing.sample_rate）
let requested = platform.pick_sample_rate(core_rate, audio::MAX_SAMPLE_RATE);
let actual_rate = platform.init_audio(requested); // 返回设备实际采样率
audio::init(core_rate, actual_rate).unwrap();

// 回调注册：handler 是安全 fn 指针，直接填入 FrontendState
FrontendState {
    audio_sample: Some(audio::sample_handler),
    audio_sample_batch: Some(audio::batch_handler),
    ..FrontendState::default()
};

// 主循环每帧：舀出引擎队列，倒进平台缓冲
let mut buf = [common::audio::AudioFrame { left: 0, right: 0 }; 2048];
let n = audio::drain(&mut buf);
if n > 0 {
    platform.push_audio(&buf[..n]);
}
```

引擎的批量入口恒返回全部输入帧数（永不提前中止，满则丢帧）；重采样
在入队前完成（生产侧，与 C 一致），上采样复制帧、下采样跳帧，比率
由 `sample_rate_in`/`sample_rate_out` 决定（相等时直通）。

`audio::init`、`drain`、handler 注册、快捷键 → `set_fast_forward` 的
接线均由装配层完成（见装配章节边界清单）。

---




### sram.rs：电池存档的持久化桥梁

**概念背景**：游戏卡带里的"存档"靠一块纽扣电池供电的 RAM 芯片
（SRAM），《宝可梦》的进度、《牧场物语》的时间（RTC 实时时钟）都
存在这上面。模拟核心在**自己的内存**里模拟这块芯片——游戏内"存档"
只是改了核心内存；把它变成关机后依然存在的 SD 卡文件，是前端的
责任。`sram.rs` 就是这条搬运链路的纯逻辑层（与即时存档的区别见
「关键技术拓展」）。

```
启动      Core_init   读：  Saves/{tag}/{name}.sav ──▶ 核心 SRAM 缓冲区
游戏运行  核心在自己内存里改数据（游戏内"存档"= 改这块内存）
  ├─ 换游戏/重置 Core_reset ─┐
  ├─ 菜单退出游戏            ├─▶ 写：核心缓冲区 ──▶ .sav + sync_all 刷盘
  └─ 睡眠前                ──┘
```

**C 语义表**（`SRAM_read`/`SRAM_write`，minarch.c:388-432；RTC 同构）：

| 情形 | C 行为 | Rust |
|------|--------|------|
| `get_memory_size` 返回 0 | 静默返回 | 空切片 → `Ok(Skipped)`，不打开文件 |
| 读：文件不存在 | 静默返回（首次运行） | `Ok(ReadOutcome::NotFound)` |
| 读：文件比 size 短 | 部分填充，不报错 | 同（至多 `memory.len()` 字节） |
| 读：文件比 size 长 | 多余截断 | 同 |
| 写：打开失败/短写 | `LOG_error` | `Err`（日志缺失期静默，由调用方决定） |
| 写完成 | 系统级 `sync()` 全盘刷 | `File::sync_all()` 仅本文件 |

**C 缺陷对照与偏离**：

1. **`sync()` → `sync_all()`**：C 同步整个系统脏页（掌机代价高），
   Rust 只刷本文件——意图（断电保护刚写的存档）忠实、范围更窄
2. **空文件读 0 字节**：C 走 `LOG_error`（minarch.c:399），Rust 静默
   `Loaded`——日志缺失期的统一约定
3. **无原子写**（否决记录）：C 直接截断写，断电瞬间可能损坏存档；
   tmp+rename 能消除但引入新复杂度，保持 C 语义

**真实代码示例**：

```rust
use std::path::Path;
use minarch::sram::{read_into, save_path, write_from, ReadOutcome};

// 路径：name 是完整文件名含扩展名（C `strrchr(path,'/')+1`）
let path = save_path("/mnt/SDCARD/Saves/GBC", "Pokemon Red.gb");
assert_eq!(path, "/mnt/SDCARD/Saves/GBC/Pokemon Red.gb.sav");

// 启动读（memory 由装配层从核心内存构造，长度即 size）
let mut sram = vec![0u8; 32 * 1024];
match read_into(&mut sram, Path::new(&path)).unwrap() {
    ReadOutcome::Loaded => {}        // 有存档，已灌入
    ReadOutcome::NotFound => {}      // 首次运行，用核心初值
    ReadOutcome::Skipped => {}       // 核心无存档功能
}

// 退出/睡眠写：截断写 + sync_all 刷盘
write_from(&sram, Path::new(&path)).unwrap();
```

**边界与半环**：FFI 取指针（`get_memory_data`/`get_memory_size` →
切片构造）与四个调用点（Core_init 读、Core_reset 写、菜单退出写、
睡眠前写，C 2971-72/2995-96/3114-15/4238-39）——由 core/menu/主循环装配层收拢（见装配章节）。

### game.rs：游戏文件组织

**概念背景**：minui 递给 minarch 的 ROM 路径可能是个"包裹"——为了省
SD 卡空间，ROM 常被打成 zip；多碟游戏（PS1 的 FF7、SS 的月下）则用
m3u 播放列表把几张碟串成一个游戏。`game.rs` 在"路径"与"核心能加载
的文件"之间做翻译：该解压的解压、该改名的改名、该换碟的换碟。

**数据流**：

```
Game::open(path, core_extensions, need_fullpath)
│
├─ .zip 且核心不支持 zip ──► 扫描本地文件头 ──► 首个扩展名匹配条目
│                              │ 方法 0：拷贝 │ 方法 8：raw deflate
│                              ▼
│                         /tmp/minarch-<pid>-<n>/<条目名>   ← tmp_path
├─ need_fullpath=false ──► 整读（tmp_path 或原路径）入内存   ← data
└─ 同目录存在 <目录名>.m3u ──► name 改为 m3u 文件名（多碟共享存档名）

change_disc(new, …)
  close() → open(new) → disc_control().replace_image_index(0, &info)
  → 写 /tmp/change_disc.txt（minui 读它更新 recents）
```

**C 语义对照表**：

| C（minarch.c） | Rust（game.rs） |
|----------------|-----------------|
| `Game_open`（:196-358）扫描 30 字节本地文件头，LE 位域读取，取首个匹配条目 | `open` 同语义；签名/central directory 不读（与 C 一致的最小解析） |
| `Zip_copy`（:118-127）store 方法分块拷贝 | `copy_stream` |
| `Zip_inflate`（:128-183）zlib `inflateInit2(-MAX_WBITS)` raw deflate | `inflate_stream` + `flate2::Decompress::new(false)` |
| `Game_close`（:359-364）free + 删文件 + `VIB_setStrength(0)` | `close()` 释放 data + 删整个目录 + `vibration::set_strength(0)` |
| `Game_changeDisc`（:366-381）关旧开新 + replace + `putFile` | `change_disc()`，`replace_image_index` 经 `Option` 承载 |

**C 缺陷对照**（Rust 的修正）：

1. **全局 `game` 变量**——任何函数都能摸到；Rust 的 `Game` 是装配层
   持有的实例，方法显式收 `&mut self`
2. **空指针 replace 调用**——核心未注册换碟回调时 `disk_control_ext`
   是 memset 零值，C 无条件调用 → 崩溃；Rust 的 `Option` 跳过调用、
   仍写通知文件（minui 侧 recents 不受影响）
3. **tmpfs 目录泄漏**——C 只删解压文件、空目录留给重启回收；Rust
   `remove_dir_all` 目录级清理（主机测试环境不累积垃圾）
4. **errno 残留值**——不支持的压缩方法走 `LOG_error(strerror(errno))`，
   打印的是上次系统调用的残留；Rust 用 `ExtractMethod { method }` 携带
   方法号精确诊断
5. **截断 zip 死循环**——`fread` 返回 0 且无 ferror 时外层 while 条件
   不变；Rust 显式报 `Extract`（`UnexpectedEof`）

**真实代码示例**：

```rust
use minarch::game::{Game, GameError};

// core_extensions/need_fullpath 来自 libretro::Core::open 的缓存
let mut game = Game::open(
    "/mnt/SDCARD/Roms/GB/game.zip",
    "gb|gbc",   // 核心扩展名表（'|' 分隔）
    false,      // 前端整读入内存
)?;

// 多碟换碟：关旧开新 + 通知核心 + 通知 minui
game.change_disc("/mnt/SDCARD/Roms/GB/disc2.gb", "gb|gbc", false)?;

// 退出：清理解压产物 + 振动清零
game.close();
# Ok::<(), GameError>(())
```

**边界与半环**：本模块闭环清单中两条半环——「换游戏振动清零」（close
内的 `vibration::set_strength(0)`）与「disc_control 换碟消费」。`Game`
实例的持有与调用点（Core_open 之后、菜单/快捷指令消费方）属装配层
归装配层主循环；m3u 碟片列表解析归 menu 模块。

---

### core.rs：会话与视频管线

**概念背景**：模拟器前端最核心的问题——"怎么让一个 `.so` 核心跑起来，
并把它吐出来的画面放到屏幕上"。`core.rs` 就是这道工序的车间：它管理
核心的**生命周期**（加载游戏、读存档、退出写档），并把核心每帧回调的
**原始画面**转换成屏幕上的**最终画面**（缩放、裁切、宽高比）。

用打比方的方式理解：核心是一台"游戏机主机"，它只认自己的输出格式
（RGB565 像素流）；屏幕是另一台设备，有自己的物理分辨率（如 1280×720）。
`core.rs` 是中间的"转接线"——它既要负责"开机/关机"（生命周期），
也要负责"信号转换"（画面缩放）。

**为什么 C 版和 Rust 版的结构不同？** 原 C 版 tg5040 平台把画面缩放
全部丢给 GPU（`SDL_RenderCopy`，platform.c:373-446）——核心回调只记录
"这帧从哪来到哪去"，真正缩放发生在硬件。Rust 版 tg5040 的 `flip` 是
物理尺寸 1:1 上传（implement-tg5040-video 决策），**画面缩放必须在
软件层完成**——这就是本模块存在的根因。

**数据流图**（pending 帧模型）：

```
核心线程（retro_run 回调）                    主循环（装配层）
┌──────────────────────────────┐              ┌──────────────────────┐
│ video_handler(data,w,h,pitch)│              │ present_frame(闭包)  │
│  ① 预渲染钩子（Special 预留） │              │   ┌─ pending? ─┐     │
│  ② 快进 10ms 节流            │              │   │ 是：flip    │     │
│  ③ 空帧返回                  │              │   │ 否：跳过    │     │
│  ④ 尺寸变化 → 重算布局+清黑   │              │   └────────────┘     │
│  ⑤ 裁剪 → 缩放 → 写 pending  │──pending──▶ │  Platform::flip(buf) │
└──────────────────────────────┘              └──────────────────────┘
```

**C 语义表**（对应 C minarch.c 的四块）：

| C 函数 | Rust 对应 | 语义要点 |
|--------|-----------|---------|
| `Core_load`（:2961-2986） | `CoreSession::load` | 构造 `RetroGameInfo` → `load_game` → SRAM/RTC 读 → av_info → 设手柄 |
| `Core_quit`（:2993-3001） | `CoreSession::quit` | SRAM/RTC 写 → `unload_game` → `deinit`（`initialized` 门控） |
| `selectScaler`（:2536-2772） | `compute_layout`（纯函数） | 只移植 tg5040 生效子集（native/cropped/forced-crop/fullscreen）+ `PLAT_flip` 的 dst 数学 |
| `video_refresh_callback_main`（:2773-2847） | `video_handler` | 钩子 → 节流 → 空帧 → 尺寸变化 → 缩放 → pending |
| `limitFF`（:4620-4645） | `ff_frame_budget`/`limit_ff`（纯函数） | 快进限速；时钟与 sleep 由装配层注入 |

**C 缺陷对照**（本模块修正/不继承的点）：

1. **forced-crop 黑屏 bug（修复）**：C tg5040 的 `PLAT_flip` 对
   `scale==0`（源大于屏）算 `dst_w = src_w × 0 = 0`——空 dst rect，
   GPU 渲染黑屏（platform.c:408）。Rust 用双线性下采样把 forced-crop
   渲染出来，顺手修复
2. **crisp+aspect 实为 GPU 双线性**：C 的 CRISP 锐度只在整数模式走
   hard_scale 最近邻；aspect 模式经过 4x target 后仍是 GPU 双线性。
   Rust 忠实映射：crisp+整数 → `IntegerScaler`，soft 或 aspect →
   `BilinearScaler`
3. **死代码不移植清单**：`%8` 对齐（:2665）、`core_aspect×1000` 整数
   比较（:2686-2687）、`fit=1` 分支、`GFX_resize`、`downsample`——
   全部是 tg5040 上的死代码（逐条记录见「设计决策记录」章节）
4. **`load_game` 返回值 Result 化**：C 静默忽略 bool 返回（:2969），
   Rust 以 `SessionError::GameLoadFailed` 显式传播
5. **读错误吞掉/写错误传播**：load 阶段 SRAM/RTC 读错误吞掉（C
   `LOG_error + 继续`，游戏仍可启动）；quit 写错误传播
   `SramWrite`/`RtcWrite`——存档丢失不可接受

**真实代码示例**（与 `src/` API 签名一致）：

```rust
use minarch::core::{
    CoreSession, Scaling, Sharpness, compute_layout, limit_ff, set_core_aspect,
    set_frontend, video_handler,
};

// ① 装配层：启动时初始化渲染态（屏幕物理尺寸）
let _ = minarch::core::init(1280, 720);

// ② 前端选项同步（screen_scaling/screen_sharpness 来自 config 前端选项表）
set_frontend(Scaling::Aspect, Sharpness::Soft);
set_core_aspect(4.0 / 3.0);

// ③ 布局纯函数（核心帧 256×224 → 1280×720 屏的 aspect 布局）
let layout = compute_layout(256, 224, 4.0 / 3.0, 1280, 720, Scaling::Aspect);
assert_eq!(layout.dst.w, 960); // 信箱：dst_h=720、dst_w=960、居中

// ④ 会话生命周期（实例由装配层持有；core/game 以参数传入）
// let mut session = CoreSession::new();
// session.init(&core);
// session.load(&mut core, &game)?;   // 填充 core.fps/sample_rate/aspect_ratio
// session.quit(&core, &game)?;       // 写档 + unload + deinit

// ⑤ 快进限速（时钟注入，纯函数）
let (delay_ms, next_last) = limit_ff(1_001_000, 1_000_000, 4166, true, 3);
assert_eq!(delay_ms, 3);

// ⑥ 视频处理器注册进 FrontendState（装配层，核心加载前）
// FrontendState { video_refresh: Some(video_handler), ..Default::default() };
```

**边界与半环清单引用**：handler 注册、pending 帧 flip 与时钟注入、
Special 钩子注册、前端选项同步、core_aspect 同步——登记在主 spec
「minarch 半环闭环清单」，由装配层逐项收拢（`limit_ff` 的
sleep 执行归主循环，C `SDL_Delay` 语义等价 `std::thread::sleep`）。

---

### savestate.rs：存档快照的持久化桥梁

**概念背景**：玩游戏时想"暂停一下、下次接着玩"，光靠游戏内的存档
往往不够——不是每个游戏都有随时可存档的机制。**即时存档**（状态
快照，save state）就像给游戏瞬间拍一张**拍立得**：把核心此刻的全部
状态（CPU 寄存器、内存、外设）序列化成一段字节存成文件，读档时反
序列化恢复现场，游戏从拍照片的那一刻继续。它与电池存档（SRAM）的
区别：SRAM 存的是"游戏自己认为的进度"，快照存的是"整个模拟器的
状态"——前者持久但受游戏机制约束，后者任意时刻可用（区别详见
「关键技术拓展」）。

`state_path` 的产物形如 `Saves/{tag}/{name}.st{slot}`——`slot` 0-8
是手动槽位（菜单选），`slot` 9 是自动存档槽位（睡眠前自动写、
开机自动续玩）。本模块**不感知槽位语义**，只做命名与文件搬运。

**数据流/结构图**（模块边界：物 vs 事）：

```
┌─ savestate.rs（纯逻辑层，零 FFI、零模块依赖）───────────────────┐
│  state_path(states_dir, name, slot) → String                     │
│  write_from(&[u8], &Path) → io::Result<()>     截断写 + fsync    │
│  read_into(&mut [u8], &Path) → io::Result<ReadOutcome>  三态读   │
└──────────────────────────────────────────────────────────────────┘
        ▲ 字节缓冲由装配层传入（切片 = 数据收集的边界）            ▲
        │                                                        │
┌─ main.rs（装配层）─────────────────────────────────┐
│  size = core.serialize_size() → buf = vec![0; size]（零填充）   │
│  写档：ff 门控关 → core.serialize(buf) → write_from(buf, …)     │
│  读档：read_into(buf, …) → core.unserialize(buf) → ff 门控恢复  │
└──────────────────────────────────────────────────────────────────┘
```

**C 语义表**（`State_getPath`/`State_read`/`State_write`，minarch.c:484-568）：

| 情形 | C 行为 | Rust |
|------|--------|------|
| 路径命名 | `{states_dir}/{name}.st{slot}`（`sprintf`） | 同（`state_path` 纯拼接） |
| 写：size 为 0 | 静默返回 | 空切片 → `Ok(())`，不创建文件 |
| 写：打开失败/短写 | `LOG_error` | `Err`（日志缺失期静默，由调用方决定） |
| 写完成 | 系统级 `sync()` 全盘刷 | `File::sync_all()` 仅本文件 |
| 读：文件不存在 | 静默返回（slot 8 特例豁免日志） | `Ok(ReadOutcome::NotFound)`（统一静默） |
| 读：文件比缓冲短 | 部分填充 + `calloc` 零填充尾部 | 同（至多 `buffer.len()` 字节，后段保持调用方初值） |
| 读：`fread` 超 size 判断 | `state_size < fread(...)` **恒假死分支** | 不移植，直接实现真实语义 |

**C 缺陷对照与偏离**：

1. **恒假死分支不移植**：C `State_read` 的 `if (state_size < fread(...))`
   （minarch.c:513）永不可能为真——`fread` 上限就是 `state_size`。
   Rust 直接实现真实语义：读入至多缓冲大小字节，短读只覆盖前段；
   unserialize 传缓冲大小（而非实际读入量）由装配层零填充保证等价
2. **`sync()` → `sync_all()`**：C 同步整个系统脏页（掌机代价高），
   Rust 只刷本文件——意图（断电保护刚写的快照）忠实、范围更窄
   （与 sram.rs 同偏离）
3. **slot 8 日志豁免无移植**：C 的「slot 8 不存在时不报错」
   （minarch.c:505-506）是日志豁免；日志缺失期所有 slot 统一静默，
   本模块不感知槽位语义，无行为差异
4. **无原子写**（否决记录）：C 直接截断写（`fopen(path, "w")`），
   断电瞬间可能损坏快照；tmp+rename 能消除但快照可达 MB 级、翻倍
   写入量，保持 C 语义

**真实代码示例**（与 `src/` API 签名一致）：

```rust
use std::path::Path;
use minarch::savestate::{read_into, state_path, write_from, ReadOutcome};

// 路径：name 是完整文件名含扩展名（C `strrchr(path,'/')+1`）
let path = state_path("/mnt/SDCARD/Saves/GBC", "Pokemon Red.gb", 3);
assert_eq!(path, "/mnt/SDCARD/Saves/GBC/Pokemon Red.gb.st3");

// 写档：缓冲由装配层从核心 serialize 填好（本模块只管搬字节）
let snapshot = vec![0xABu8; 1024];
write_from(&snapshot, Path::new(&path)).unwrap();

// 读档：装配层用 vec![0; size] 零填充（对应 C calloc），短读只覆盖前段
let mut buf = vec![0u8; 1024];
match read_into(&mut buf, Path::new(&path)).unwrap() {
    ReadOutcome::Loaded => {}    // 有快照，交给 unserialize
    ReadOutcome::NotFound => {}  // 没存过，从游戏开头玩
    ReadOutcome::Skipped => {}   // 空缓冲（serialize_size 为 0）
}
```

**边界与半环清单引用**：序列化 FFI 数据收集与时序编排（
`serialize_size` 查询、缓冲分配、serialize/unserialize 调用、快进门控
关→序列化→恢复）、autosave/resume 槽位编排（`AUTO_RESUME_SLOT` 切换、
`RESUME_SLOT_PATH` 读+删）、菜单存档交互（`.bmp` 预览、多碟 `.txt`
记忆、slot 记忆文件）——接线归装配层与 menu（见装配章节）。

---

### hdmi.rs：HDMI 热插拔检测状态机

#### 概念背景

**HDMI 热插拔检测解决什么问题？** 当掌机通过 HDMI 线连接电视时，系统的显示输出会切到电视；拔掉时切回掌机屏幕。这个切换通常伴随分辨率、音频输出路径的变化——对游戏前端来说，最稳妥的处理方式是**检测到变化后自动重启**，让系统干净地重新初始化（C 原版就是这么做的：`hdmimon()` 变化时保存进度 → 睡 4 秒 → 退出，由 shell 脚本重新拉起）。

通俗类比：就像电视的"信号源自动检测"——你插上 HDMI 线，电视自动切到对应输入；拔掉又切回。本模块就是掌机端的"信号源检测器"，但它只负责**发现变化并报告**，真正"切换信号源"（重启）的动作由装配层执行。

#### 数据流/结构图

```
┌─ 装配层 main.rs（主循环）────────────────────┐
│  每帧:                                                     │
│    let change = monitor.update(platform.is_hdmi_active()); │
│    if let Some(c) = change {                               │
│        // C 语义: Menu_beforeSleep → sleep(4) → quit       │
│        // （minarch.c:2170-2174）                          │
│    }                                                       │
└──────────────┬─────────────────────────────────────────────┘
               │ is_active: bool（变量传入，模块不接触平台）
               ▼
┌─ hdmi.rs（本模块，纯逻辑状态机）──────────────────────────┐
│  HdmiMonitor { had_hdmi: Option<bool> }                    │
│  update(is_active) → Option<HdmiChange>                    │
│    首次采样 → None（只记录）                                │
│    无变化   → None                                          │
│    变化     → Some(Connected / Disconnected)                │
└─────────────────────────────────────────────────────────────┘
```

#### C 语义表

| C `hdmimon()`（minarch.c:2162-2176） | Rust `HdmiMonitor` |
|--------------------------------------|-------------------|
| `static int had_hdmi = -1`（函数内静态，-1 = 尚未采样） | `had_hdmi: Option<bool>`（`None` = 尚未采样） |
| `if (had_hdmi==-1) had_hdmi = has_hdmi;`（首次采样只记录） | `match had_hdmi { None => 记录并返回 None }` |
| `if (has_hdmi!=had_hdmi)`（变化判断） | `Some(had) if had != is_active` |
| 变化 → `Menu_beforeSleep()` + `sleep(4)` + `quit` | 返回 `Some(HdmiChange)`，动作由装配层执行 |
| 变化时不区分方向（只重启） | `Connected`/`Disconnected` 携带方向（装配层可忽略） |

#### C 缺陷对照

| C 行为 | Rust 处理 | 理由 |
|--------|----------|------|
| 函数内 `static int had_hdmi` 跨帧记忆 | 结构体字段 `had_hdmi: Option<bool>` | Rust 无函数内可变 static（`static mut` 有 UB 风险）；状态显式归属实例 |
| `-1` 哨兵表达"尚未采样" | `Option::None` | 消除魔法值，类型系统表达可选状态 |
| 变化时直接执行重启（副作用焊死在检测函数里） | 只返回 `Option<HdmiChange>` | 纯逻辑可单测；副作用归装配层（config 规则「纯逻辑为主」） |
| 变化时打日志 `LOG_info`（minarch.c:2170） | 静默 | 日志缺失期约定（与 sram/environment 一致）；信息经返回值传递 |

#### 真实代码示例

```rust
use minarch::hdmi::{HdmiChange, HdmiMonitor};

// 装配层持有实例（单线程主循环，无需锁）
let mut monitor = HdmiMonitor::new();

// 每帧采样推进（is_active 来自 Platform::is_hdmi_active()）
assert_eq!(monitor.update(false), None);                    // 首次采样只记录
assert_eq!(monitor.update(false), None);                    // 无变化
assert_eq!(monitor.update(true), Some(HdmiChange::Connected));   // 插上 HDMI
assert_eq!(monitor.update(true), None);                     // 状态已同步
assert_eq!(monitor.update(false), Some(HdmiChange::Disconnected)); // 拔掉 HDMI
```

#### 边界与半环清单引用

本模块**不实现**装配层接线：每帧驱动（`HdmiMonitor::update` ← `Platform::is_hdmi_active()`）与变化时的重启动作（`Menu_beforeSleep` 等价序列 → sleep(4) → quit，C 2170-2174）——接线归装配层hdmi 行，由装配层主循环收拢（终态见装配章节）。tg5040 平台 `is_hdmi_active()` 恒 false（与原 C `GetHDMI()` 恒 0 一致），当前平台永不触发重启——本模块为未来支持 HDMI 检测的平台预留结构。

---

### menu.rs：游戏内菜单——暂停菜单的状态机与绘制

#### 概念背景

**游戏内菜单解决什么问题？** 游戏运行中按 MENU 键弹出的暂停菜单：Continue（继续玩）、Save/Load（手动存/读档）、Options（前端/模拟器设置）、Quit（退出游戏）。它是玩家与模拟器交互的核心入口——C 原版约 1500 行（minarch.c:3006-4572）承载了主菜单循环、选项子菜单框架、存档交互与绘制。

通俗类比：就像游戏机的「开始键菜单」——按一下暂停，弹出功能列表；选项里还能调音量、换按键、改画面设置。C 版是**两个嵌套的阻塞循环**（主菜单循环里嵌选项菜单循环，选项里还能嵌消息框循环）；Rust 版改成了**纯逻辑状态机 + 注入式 IO**——装配层逐帧把按键状态传进来，菜单推进自己的状态并返回「该干什么」（存档？读档？退出？），绘制也是纯函数把菜单画进帧缓冲。

#### 数据流/结构图

```
┌─ 装配层 main.rs（主循环）─────────────────────────────┐
│  PAD 轮询 → MenuInput{up,down,left,right,a,b,x,menu}               │
│      │                                                             │
│      ▼                                                             │
│  MainMenu::update(&input) → Option<MainAction>                     │
│      │                 Continue/Save/Load/Options/Reset/DiscChange │
│      ▼                                                             │
│  执行动作：换碟 / 序列化存档 / 打开子菜单（OptionMenu::update）      │
│      │                                                             │
│      ▼                                                             │
│  draw_main_menu(theme, screen, overlay, name, &menu, …)            │
│  draw_option_menu(theme, screen, &OptionDrawState{&menu, …})       │
└────────────────────────────────────────────────────────────────────┘
```

菜单模块的三个组成部分：

| 部分 | 职责 | 对应 C |
|------|------|--------|
| `MainMenu` | 主菜单状态机：5 项导航、换碟/换槽位、A 键分派 | `Menu_loop`（minarch.c:4289-4391） |
| `OptionMenu` + `MenuItem` | 选项子菜单框架：4 种类型、滚动窗口、回调分发、await_input | `Menu_options`（minarch.c:3586-3970） |
| `SaveStateIo` | 存档交互文件层：slot 记忆、多碟记忆、存在性查询 | `Menu_saveState`/`Menu_loadState`（minarch.c:4135-4182） |

#### C 语义表

| C 行为（minarch.c） | Rust 对应 |
|---------------------|-----------|
| 5 个固定项 Continue/Save/Load/Options/Quit（:3008-3017） | `MAIN_ITEMS` 常量 + `MainMenu.selected` |
| `simple_mode` 下 Options 变 Reset（:3073） | `MainMenu::new(simple_mode, …)` 参数化 |
| LEFT/RIGHT 在 Continue 项换碟、Save/Load 项换槽位（:4299-4324） | `MainMenu::update` 的 `step()` |
| A 键分派 STATUS_*（:3020-3027/:4334-4389） | `MainAction` 枚举 |
| `MenuItem`/`MenuList` 模型 + 4 类型（:3126-3159） | `MenuItem`/`ItemKind`/`OptionMenu` |
| `PAD_justRepeated(UP/DOWN)` 滚动 + 回绕（:3630-3654） | `OptionMenu::move_up/move_down` |
| `await_input`/`defer_menu` 绑定流程（:3609-3625） | `OptionMenu.awaiting` + `finish_await()` |
| `menu.slot_path`/`menu.txt_path` 记忆文件（:3071/:4127） | `slot_memory_path`/`disc_memory_path` |
| `Menu_updateState` 的 save_exists/preview_exists（:4115-4134） | `SaveStateIo::save_exists/preview_exists` |
| 主菜单绘制：遮罩/名称条/药丸列表/预览窗（:4395-4534） | `draw_main_menu` |
| 选项菜单四种布局绘制（:3744-3960） | `draw_option_menu` |

#### C 缺陷对照

| C 行为 | Rust 处理 | 理由 |
|--------|----------|------|
| 嵌套阻塞循环直接 `PAD_poll()`/`GFX_*`（菜单焊死 IO） | 纯状态机 + `MenuInput` 注入 | 与 config 规则「装配层收集数据」一致；菜单可单测；装配层一个主循环驱动所有层级 |
| 全局 `menu`/`selected`/`simple_mode` 状态 | 实例字段（`MainMenu`/`OptionMenu`） | 状态显式归属，无全局可变（与 vibration/hdmi 同模式） |
| `Option.lock` 的消费散落在 openMenu 里 | `OptionMenu::from_options` 统一过滤 | 半环清单 config「Option.lock 消费」闭环 |
| 存档时直接在菜单内调 `State_write()`（serialize FFI） | `SaveStateIo` 只做文件层，序列化归装配层 | 「物 vs 事」边界（同 savestate 决策）；菜单不依赖 core |
| 菜单绘制行距用 `BUTTON_SIZE`（40px @scale2） | 行距用 `PILL_SIZE × scale`（60px） | render 的 `blit_pill` 高度固定为精灵高（30×scale），不按传入 h 裁切——行距取精灵高避免重叠与越界（render 既有行为的适配） |

#### 真实代码示例

```rust
use minarch::menu::{MainAction, MainMenu, MenuInput, OptionMenu, OptionMenuEvent};

// 装配层持有主菜单实例（单线程，无需锁）
let mut menu = MainMenu::new(false, 3); // 非 simple_mode、3 碟

// 逐帧注入按键状态（装配层从 PAD 收集）
let input = MenuInput { down: true, ..Default::default() };
menu.update(&input); // 选中项移到 Save

// A 键分派
let action = menu.update(&MenuInput { a: true, ..Default::default() });
assert_eq!(action, Some(MainAction::Save)); // 装配层据此执行存档序列化

// 选项子菜单：从配置选项构建（锁定项自动过滤）
let items = OptionMenu::from_options(&config_options, ItemKind::Var);
let mut opts = OptionMenu::new(items, 7, None, None);
let event = opts.update(&MenuInput { right: true, ..Default::default() });
assert_eq!(event, OptionMenuEvent::Change(0)); // 值已修改，装配层执行 on_change
```

#### 边界与半环清单引用

本模块**不实现**装配层接线：PAD 轮询与 `MenuInput` 收集、菜单打开/关闭触发（`PAD_justReleased(BTN_MENU)`）、睡眠/唤醒序列（SRAM/RTC 写、autosave、CPU 调速）、`OptionFrontend_*`/`OptionEmulator_*` 的平台联动（`Config_syncFrontend`/`selectScaler`/`setOverclock`）、按键绑定的实际 PAD 扫描循环、`.bmp` 预览图解码与缩放——接线归装配层的 config/controls/environment/sram/savestate 行，由装配层与主循环实现收拢。menu 模块已闭环：config「Option.lock 消费」、savestate「菜单存档交互」文件层（`.txt`/slot 记忆/`save_exists`/`preview_exists`）。

---

### main.rs + assembly.rs：装配层——把 12 个模块接成一台掌机

#### 概念背景

前面每个模块都是"纯逻辑零件"：`config` 会解析 cfg 文本但不知道文件在哪，`core` 会算布局但不知道屏幕多大，`audio` 会重采样但不知道采样率多少。装配层就是把这些零件**接起来并驱动**的"总装车间"——对应原 C 版 4800 行单文件里 `main()`（minarch.c:4668-4829）干的所有事：启动、主循环、菜单、睡眠、退出。

为什么拆成 `main.rs` + `assembly.rs` 两个文件？因为 `main.rs` 是 bin 目标且 `#[cfg(feature = "tg5040")]` 门控（需要平台 crate），**集成测试（无 feature 编译）链接不到它**。于是把可脱离平台测试的纯逻辑（存档编排、resume 解析）放进 lib 目标的 `assembly.rs`，`main.rs` 只留薄胶水——这延续了 minui 的"装配层不直接测试，可测逻辑下沉"模式。

#### 数据流/结构图

```
启动序列（main.rs run<P: Platform>）
─────────────────────────────────────────────
argv[1]=核心.so  argv[2]=ROM
   │
   ├─▶ 平台初始化（video/input/power）
   ├─▶ Core::open（dlopen + 目录派生）
   ├─▶ Game::open（zip/m3u 探测）
   ├─▶ 三级 cfg 加载（system→default→user）
   │      └─ config::apply_cfg → 选项表/映射/快捷指令
   ├─▶ 静态注册（environment/vibration/buttons/core）
   ├─▶ register_callbacks（6 个 handler 一次注册）
   ├─▶ CoreSession::init + load → set_core_aspect
   ├─▶ audio::init（core_rate ← av_info、actual ← init_audio）
   ├─▶ 菜单/资源初始化 + resume 槽位恢复
   │
主循环（每帧）                             菜单子循环（show_menu 时）
─────────────────────                    ─────────────────────────
帧计时 → poll_input                       进入：写 SRAM/RTC、vibration
→ detect_shortcuts → 消费动作                suspend、降频、关快进
→ update_buttons（注入 BUTTONS）          帧循环：MenuInput 构造 →
→ core.run()（同步）                        MainMenu/OptionMenu update
→ limit_ff 限速 → present_frame/flip        → 消费 MainAction（Save/Load/
→ audio::drain → push_audio                 DiscChange/Quit/Reset）
→ vibration::tick → set_rumble            退出：恢复高频/vibration
→ power::update（睡眠/关机）                resume/快进恢复
→ hdmi::update（变化→重启）
→ 帧预算补偿
```

#### C 语义表

| C 段（minarch.c） | Rust 装配层对应 | 说明 |
|------|------|------|
| `main` 启动 :4668-4748 | `run<P: Platform>` 启动序列 | 参数/平台/核心/游戏/cfg/回调/音频/菜单 |
| `input_poll_callback` 快捷段 :1681-1728 | `detect_shortcuts` + `handle_shortcut` | 检测（controls.rs 纯函数）+ 动作消费（装配层） |
| `input_poll_callback` 映射段 :1755-1773 | `update_buttons` | 装配层在 `core.run()` 前注入 |
| `input_state_callback` :1777-1793 | `input_state_handler` | 从 `BUTTONS` 静态应答 |
| `Menu_beforeSleep` :3112-3119 | `before_sleep` | SRAM/RTC + autosave + auto_resume + 降频 + suspend |
| `Menu_afterSleep` :3120-3124 | `after_sleep` | 删标记 + 恢复高频 + resume |
| `Menu_loop` :4224-4572 | `menu_loop` | 进入准备/帧循环/退出恢复 |
| `hdmimon` :2162-2176 | 主循环 `hdmi.update` | 变化 → before_sleep → sleep(4s) → quit |
| `main` 退出 :4805-4829 | 退出序列 | Game::close → CoreSession::quit → 平台收尾 |

#### C 缺陷对照

- **`input_poll` 回调不轮询**：C 版 `input_poll_callback` 内部直接调 `PAD_poll()`（摸全局平台）；Rust 版 `fn` 指针无法捕获 `Platform`，改为装配层在 `core.run()` **前**注入按键数据（`update_buttons`），回调只读静态——单线程下时序等价，且消除 C 版回调内摸全局的耦合
- **`Game_open` 失败直通**：C 版 `goto finish` 跳过核心加载直接清理；Rust 版错误路径同样跳过核心加载、非零退出（`Core::open`/`Game::open`/`load` 错误均显式传播，C 版静默）
- **`serialize` 返回 bool**：C 版 `State_write` 按 `state_size` 写满缓冲（`serialize` 成功即填满）；Rust 版同样全量写 `size` 字节，不把 bool 当长度
- **`thread_video` 优雅退出**：C 版 `coreThread`/`toggle_thread` 用 `pthread_cancel` 粗暴中断核心线程（可能在 `core.run()` 执行中途杀线程，未定义行为）；Rust 版（implement-minarch-thread-video）采用**方案 A 优雅退出**——`should_run_core` 原子门控，核心线程在帧边界自退出、装配层 `join`，`unload_game`/`deinit` 严格在 join 之后（修复 C 退出不 join 的竞态）
- **`write_saves` 公开**：C 版 `SRAM_write`/`RTC_write` 是自由函数；Rust 版经 `CoreSession::write_saves`（只写档不卸载核心），菜单进入/睡眠前调用

#### 真实代码示例

```rust
// assembly.rs：存档编排（可测纯逻辑）——mock 核心下真实 dlopen 往返
use minarch::assembly::{save_state, load_state};
use minarch::libretro::Core;
use minarch::menu::SaveStateIo;

let core = Core::open(so_path, "TST", "/cfg", &states_dir, "/saves", "/bios")?;
let save_io = SaveStateIo::new(&minui_dir, &states_dir, "Game.mc", vec![], "");
save_state(&core, &game, 3, &save_io, false)?;   // 写 {states}/Game.mc.st3
load_state(&core, &game, 3, &save_io, false)?;   // 读回 + unserialize

// main.rs：快进同步（core 静态为唯一事实源）
let new = !run_state.fast_forward;
core::set_fast_forward(new);
audio::set_fast_forward(new);  // 装配层同步调用 audio setter
```

#### 装配细节：三级 cfg 契约

启动时按 **system → default → user** 顺序加载三个 cfg 文件，后者覆盖前者；文件缺失静默跳过（用默认值）。路径规则（`main.rs` 装配处定义，有意不进 common）：

| 级 | 默认路径 | DEVICE 环境变量优先 |
|----|----------|---------------------|
| system | `{.system}/{platform_code}/system.cfg` | `DEVICE=<tag>` 且 `system-<tag>.cfg` 存在 → 用之 |
| default | `{emu_dir}/default.cfg`（emu_dir = 模拟器 `.pak` 目录） | `DEVICE=<tag>` 且 `default-<tag>.cfg` 存在 → 用之 |
| user | `{config_dir}/minarch.cfg` | **游戏级覆盖**：`{config_dir}/{game.name}.cfg` 存在 → 用之（user 级=游戏级 cfg） |

`apply_cfg` 一次性更新五样东西：前端选项表（`frontend_options`）、核心选项表（`core_options`）、按键映射（`mapping`）、快捷指令（`shortcuts`）、手柄类型（`gamepad_type`）。cfg 文本里 `-` 前缀的键是锁定项（`Option.lock` 语义，菜单侧统一过滤）。

#### 装配细节：Config_syncFrontend 联动矩阵与菜单写回

cfg 应用后执行一次"前端选项 → 平台/核心"联动，菜单里改值也实时走同一路径：

| cfg 键（minarch_*） | 值 → 联动目标 | 执行 |
|--------------------|--------------|------|
| `screen_scaling` | Native/Fullscreen/Cropped/Aspect → `Scaling` | `core::set_frontend(scaling, sharpness)` |
| `screen_sharpness` | Crisp/Soft → `Sharpness` | 同上（一次调用带两个参数） |
| `prevent_tearing` | Strict/Lenient/Off → `VsyncMode` | `platform.set_vsync(mode)` |
| `cpu_speed` | Powersave/Performance/Normal → `CpuSpeed` | `platform.set_cpu_speed(speed)` |
| `max_ff_speed` | 数字（默认 3）→ 快进倍率上限 | 存入运行状态，快进预算据此换算 |

关闭菜单时把 8 个前端键的当前值序列化写回 user cfg（`save_cfg` 截断创建 + `sync_all`）：写回目标 = user 级路径（有游戏级 cfg 时写游戏级，否则写 `minarch.cfg`）；写失败静默（与 C `putFile` 忽略一致）。

#### 装配细节：thread_video 线程模式（A/B/C 状态机）

`minarch_thread_video` 选项为 On 时，核心的 `retro_run` 被挪进独立线程（方案 A 优雅退出，见「C 缺陷对照」），装配层与核心线程解耦。切换状态机 `ThreadToggleState`（assembly.rs）三态 A/B/C 对应三类触发：**进/退快进**、**电源键按下/释放**、**菜单选项 OFF/ON**——每类触发在帧边界消费（`should_run_core` 原子门控：核心线程在帧边界自检并退出，装配层 `join` 后才 `unload_game`/`deinit`；核心经 `Arc<Core>` 与线程共享保活）。时钟单归属：`limit_ff_now` 只在装配层（VideoState）计算，线程不碰帧计时；快进预算由装配层算好传下。菜单期间线程按需常驻空转或销毁（菜单选项 OFF 时彻底退出线程）；thread_video Off 时核心回退同步执行路径，同一套主循环代码。

#### 边界与半环清单引用

装配层已闭环半环清单的全部装配/主循环接线行：cfg 三级 IO、`Config_syncFrontend`、controls handler/buttons/快捷指令/bind 应用、environment 注册、rumble 接线、vibration tick/suspend、sram 调用点（经 `CoreSession::write_saves`）、audio init/drain/ff、core handler/flip/选项同步、序列化编排（`assembly::save_state`/`load_state`）、autosave/resume（`assembly::read_resume_slot`）、hdmi 每帧驱动；thread-video 已闭环 **thread_video 线程模式**（核心线程 `spawn_core_thread` 方案 A、`ThreadToggleState` 状态机、`minarch_thread_video` 运行时开关）与**菜单选项写回 user cfg**；analog-sticks 已闭环**摇杆原始轴值闭环**（`InputState.laxis/raxis` → `update_sticks` → `input_state_handler` 的 `RETRO_DEVICE_ANALOG` 应答）。仍保持未闭环：Special 预渲染钩子、gamepad 选项菜单消费（menu 侧）、`.bmp` 预览解码。

---

## 设计决策记录

模块级决策（"为什么"与 C 的差异、被否决方案）已随各模块章节讲解，本节收拢**跨模块与装配级**的决策及演进史。每条含：结论 → 为什么 → 被否决方案 → 产生的问题。

### 决策：装配层三拆（lib / main 薄胶水 / assembly 可测逻辑）

- **结论**：纯逻辑全部进 lib 目标模块；`main.rs`（bin，`#[cfg(feature)]` 门控平台）只留启动序列/主循环/睡眠退出等装配；`assembly.rs`（lib）承载可脱离平台测试的装配逻辑（存档编排 `save_state`/`load_state`、resume 槽位解析、`ThreadToggleState`）。`run<P: Platform>` 内全部局部变量，无全局 static。
- **为什么**：集成测试只能链接 lib 目标（cargo 规则），纯逻辑进 lib 才可测；装配层本身不做自动化测试，其可测部分单独下沉避免 bin 门控拖累。
- **被否决**：全塞 `main.rs`（约 600 行、渲染/导航/循环混杂、不可测）；只拆 `ui` 或只拆 `menu`（菜单逻辑仍不可测）。C 版全局 `pad`/`snd`/`menu` 等 20+ 全局变量的 Rust 化 = `run<P>` 局部变量显式传递。
- **产生的问题**：bin 目标的装配契约对 lib 读者不可见（`cargo doc` 不覆盖 bin），装配语义主要靠 main.rs 注释与集成测试（`main_loop.rs`）承载。

### 决策：三级 cfg 契约（system → default → user，装配处自持）

- **结论**：按 system → default → user 顺序加载、后者覆盖前者、缺失静默；DEVICE 环境变量用"存在性优先"选择带 tag 的变体（`system-<tag>.cfg`/`default-<tag>.cfg`）；user 级 = 游戏级 cfg 存在时整份覆盖 `minarch.cfg`（逐游戏设置）。
- **为什么**：忠实 C 的 cfg 加载链（`InitSettings` + per-game override），装配处实现且不进 common——cfg 路径依赖 `.pak` 目录与 emu 名，属 minarch 装配语义而非通用路径。
- **被否决**：cfg 路径族下沉 `common::paths`（跨 crate 无第二个消费方）；合并加载 system 与 default（C 语义是分层覆盖，合并会破坏"default 覆盖 system"的优先级）。
- **产生的问题**：锁定项（`-key`）与游戏级 cfg 并存时菜单只写 8 个前端键，核心选项不回写（C 同款，写回 user cfg 仅 frontend 8 键）。

### 决策：Config_syncFrontend 联动集中装配层

- **结论**：cfg 应用与菜单改值后，由装配层统一把前端选项值翻译为平台/核心调用（scaling/sharpness → `core::set_frontend`；prevent_tearing → `platform.set_vsync`；cpu_speed → `platform.set_cpu_speed`；max_ff → 快进预算），映射表见「装配细节」。
- **为什么**：C 的选项改值联动散在 `Option_setValue` 各处（菜单与启动两套路径）；Rust 收敛为单一翻译函数，启动与菜单复用同一路径，杜绝两套路径漂移。
- **被否决**：联动逻辑放 config 模块（config 是纯逻辑层，不碰平台 trait——"纯逻辑 + 薄静态，数据收集与装配归装配层"项目规则）。
- **产生的问题**：新增前端选项时必须同步扩展装配层翻译函数（编译期枚举保证漏配即编译错误）。

### 决策：thread_video 方案 A 优雅退出（A/B/C 状态机）

- **结论**：核心线程在帧边界自检 `should_run_core` 门控退出，装配层 `join` 后才 `unload_game`/`deinit`；切换由 `ThreadToggleState`（assembly.rs）A/B/C 三态统一管理（快进进出/电源键/菜单选项三类触发）。
- **为什么**：C 用 `pthread_cancel` 在 `core.run()` 中途杀线程（未定义行为），退出不 join 还有竞态；方案 A 让线程"跑完当前帧再走"，`join` 保证析构顺序（详见装配章节「C 缺陷对照」）。
- **被否决**：双缓冲 video backbuffer（pending 帧模型下单缓冲已够）；线程内自管时钟（时钟单归属装配层，避免双源计时漂移）。
- **产生的问题**：核心经 `Arc<Core>` 与线程共享（保活 `.so`）；菜单期间线程按需空转或销毁的策略差异需在真机验证帧时序。

### 决策：回调桥无锁设计（OnceLock + trampoline）

- **结论**：6 个 `extern "C"` trampoline + `OnceLock<FrontendState>` 一次性注册、此后只读；未注册字段由 trampoline 返回安全默认值。
- **为什么**：注册发生在核心加载前（单线程装配段），运行期只读 → 无锁零开销；C 版靠全局函数指针 + 调用顺序约定，Rust 用类型（`OnceLock`）固化"只注册一次"约束。
- **被否决**：`Mutex<FrontendState>`（音频热路径每帧加锁）；每次回调查 `Option`（与 trampoline 语义重复）。
- **产生的问题**：进程级 `OnceLock` 迫使桥测试单测试串行（见「测试」章节的解决方式）。

### 决策：阻塞循环状态机化（vibration/menu/audio 的共同模式）

- **结论**：C 的线程轮询（vibration pthread）、嵌套阻塞循环（menu 两层 while + 直接 PAD/GFX）、满则等待（audio `SDL_Delay` 重试）统一改为"纯状态机/引擎 + 装配层单主循环驱动"。
- **为什么**：帧语义（`DEFER_FRAMES` 本就按帧计）、所有权（`&mut Platform` 单线程独占）、可测性（状态机纯逻辑）三个理由在 vibration 章节完整展开；menu 章节对照 C 嵌套循环的逐项改造。
- **被否决**：保留 C 的线程/阻塞结构（pthread 需全局锁、嵌套循环不可测）；部分状态机化（两套路径并存）。
- **产生的问题**：状态机的"动作执行"与"检测"分离，装配层需为每个动作提供执行点（menu_loop/主循环 switch），动作遗漏是装配期主要 bug 源——`MainAction` 枚举 + 编译期 match 穷尽缓解。

### 决策：存档编排的"物 vs 事"边界

- **结论**：savestate/sram 模块只做"文件搬运"（路径命名、截断写、`sync_all`、三态读），序列化 FFI（`serialize_size`→分配→`serialize`→`unserialize`）与槽位编排归装配层（`assembly::save_state`/`load_state`）；菜单只做文件层交互（`SaveStateIo`）。
- **为什么**：核心的 serialize 符号经 FFI 调用需要 `&Core` 与快进门控，若下沉到 savestate 模块会让纯文件模块依赖 core；"物（文件）vs 事（编排）"分层让三层各自可测（测试章节）。
- **被否决**：savestate 模块直接调 core 符号（模块依赖反转）；菜单内嵌序列化（C 的做法，菜单与 core 耦合）。
- **产生的问题**：编排时序（快进门控关→序列化→恢复）散在装配层，需集成测试（`assembly.rs` 测试）锁定顺序。

---

## 测试

minarch 的测试基础设施是本 crate 设计决策最密集的部分，这里展开讲透。

### 测试的难题：没有掌机，怎么测一个"掌机程序"？

minarch 的全部意义就是和 libretro 核心 `.so` 对话。但真实核心（gambatte、snes9x 等）是 ARM 交叉编译产物——在 macOS 开发机上根本编不出来，更别说运行。于是摆在面前的三个问题各有对应设计：

```
问题                              答案（三个设计）
──────────────────────────────────┬─────────────────────────────────
① 测试文件放哪？                   │ lib+bin 拆分：集成测试需要 lib 目标
   bin-only 的 crate 无法被        │
   集成测试引用                    │
                                  │
② 没有真核心，拿什么测？           │ mock 核心：一个 60 行的假核心，
                                  │ 编译成动态库充当"替身"
                                  │
③ 怎么保证手写 FFI 的布局          │ 编译期 const 断言：错即编译失败，
   永远不漂移？                    │ 机器漏不掉
```

后文依次展开这三个设计。

### lib+bin 拆分：给测试开一扇门

Rust 的测试分两种，差别在于"站得多远"看代码：

```
单元测试（模块内部）                    集成测试（tests/*.rs）
┌─────────────────────────┐           ┌─────────────────────────┐
│ src/                    │           │ tests/                  │
│   libretro.rs           │           │   libretro_abi.rs      │
│   └ #[cfg(test)]        │           │   core_loading.rs      │
│     mod tests { ... }   │           └────────────┬────────────┘
└─────────────────────────┘                        │ 只能调用 pub API
        能访问私有函数/字段                          ▼
                                                   lib 目标
```

关键规则：**集成测试只能链接 crate 的 lib 目标**。这就是 minarch 有 `src/lib.rs` 而 minui 没有的原因：

- **minui**：bin-only（只有 `main.rs`），测试写在模块内部（`#[cfg(test)] mod tests`）——它的模块都是纯逻辑（文件浏览、路径匹配），内嵌测试足够
- **minarch**：FFI 测试需要从"外部消费者"视角调用 pub API（`Core::open`、`register_callbacks`），而且多个测试文件要共享 mock 核心编译辅助——这些内嵌测试做不了。于是拆成 lib（12 个 `pub mod`）+ bin（`main.rs` 薄入口）

顺带的收益：bin 目标里未被调用的 pub 项会触发 dead_code 警告；lib 目标的 pub 项就是公共 API，天然没有这份噪音。

crates/minarch/tests/            ← cargo 约定目录：顶层 .rs 自动成为测试目标
│
├── libretro_abi.rs              ← ABI 常量数值 + 结构体布局（静态契约，编译期断言）
├── core_loading.rs              ← 加载器 + 回调桥（真实 dlopen）
├── assembly.rs                  ← 存档编排（save_state/load_state）+ resume 槽位解析
├── main_loop.rs                 ← 装配层主循环集成（真实 mock 核心往返）
├── menu.rs                      ← 主菜单/选项子菜单状态机 + 存档交互文件层
├── config.rs                    ← 核心选项注册表 + 前端选项表 + cfg 解析
├── controls.rs                  ← 输入映射模型与表、bind 解析
├── environment.rs               ← 30 case 环境回调分发
├── sram.rs                      ← SRAM/RTC 路径与读写
├── savestate.rs                 ← 快照路径命名与读写
├── vibration.rs                 ← 振动状态机 + 薄静态层
├── audio.rs                     ← 音频队列引擎 + 薄静态层
├── core.rs                      ← 会话生命周期、布局计算、视频处理器、快进帧控
├── game.rs                      ← zip 解压/m3u 探测/换碟行为测试
├── hdmi.rs                      ← HDMI 热插拔检测状态机
│
├── common/
│   └── mod.rs                   ← 共享辅助（不是测试！）
├── test_common/
│   └── mod.rs                   ← `common/mod.rs` 的转发层：`mod common;`
│                                   会遮蔽外部 crate `common` 导致
│                                   `use common::video::…` 编译失败，
│                                   故以 `mod test_common;` 引用
│
└── fixtures/                    ← 测试夹具（材料）
    ├── libretro.h               ← vendored 官方 ABI 头（钉住 commit）
    └── mock_core.c              ← 假核心，编译成动态库供测试加载
```

**cargo 的两条隐式约定**（新手容易困惑的点）：

1. `tests/` 下**每个顶层 `.rs`** 都变成一个独立测试程序（有自己的入口和测试 harness）
2. `tests/` 下的**子目录**（如 `common/`、`test_common/`、`fixtures/`）不会被当作测试入口——所以共享代码放子目录，测试文件里 `mod …;` 引用即可

### 假核心 mock_core.c：测试材料

为什么要造假核心？`libretro.rs` 的全部意义就是和核心 `.so` 对话，测试它就必须有一个能加载的 `.so`。真核心编不出来，就写一个 60 行的假核心：

```
测试编译：cc -shared -fPIC mock_core.c → mock_core_bridge.dylib
                                                      │
libretro.rs Core::open(path) ── dlopen ──────────────┘
                                                      │
           ┌──────────────────────────────────────────┤
           │  核心导出 22 个 retro_* 符号（与真核心同名）│
           │  retro_run() 主动触发 video/audio 回调      │
           │  帧数据首 2 字节 = 0x1234（断言锚点）        │
           │  编译宏开关：MOCK_NO_RETRO_RUN 等           │
           │    → 故意"缺少符号"，测试错误路径             │
           └──────────────────────────────────────────┘
```

编译宏让同一个假核心能扮演"坏核心"：

| 编译宏 | 效果 | 对应测试场景 |
|--------|------|--------------|
| `MOCK_NO_RETRO_RUN` | 不导出 `retro_run` | `Core::open` 返回 `SymbolMissing` |
| `MOCK_NO_LOAD_GAME_SPECIAL` | 不导出预留符号 | 加载仍成功、字段为 `None` |
| `MOCK_NO_GET_REGION` | 同上 | 同上 |
| `MOCK_LOAD_GAME_FAIL` | `retro_load_game` 返回 0 | `CoreSession::load` 返回 `GameLoadFailed` |
| `MOCK_ZERO_ASPECT` | `av_info.aspect_ratio` 填 0.0 | aspect 回落 `base_width/base_height` |
| `MOCK_TRACK_CALLS` | `unload_game`/`deinit`/`set_controller_port_device` 计数 | 经 `mock_call_count(name)` 查询调用序 |

### vendored libretro.h：断言期望值的唯一权威来源

`tests/fixtures/libretro.h` 是从 libretro-common 仓库钉住 commit 的官方头（文件头部注明来源 `b894fa4`），双重身份：

- mock_core.c 的编译依据
- 测试断言期望值的**唯一权威来源**——常量数值、结构体大小全部从它推导。手写 FFI 最大的风险是"我以为 ABI 长这样，其实不是"，钉住官方头就消灭了"我以为"

### libretro_abi.rs：静态契约（不需要运行核心）

| 断言组 | 验证内容 |
|--------|----------|
| 30 个 ENVIRONMENT 常量 | `SET_VARIABLE == 70`、`MASK == 256`、`RGB565 == 2`…与头文件逐值一致 |
| 设备/内存常量 | 同上 |
| 类型精确宽度 | 常量是 `u32`、签名用 `u32` 对应 C `unsigned`（编译期证据） |
| 17 结构体 + 3 枚举布局 | `size_of`/`offset_of`，编译期 const 断言 |

### core_loading.rs：动态行为（真实 dlopen）

```
加载成功    loads_all_symbols_and_fills_info_cache   22 符号逐一调用 + 信息缓存断言
错误路径    missing_so_returns_library_open_failed   CoreError 分支
           missing_required_symbol_returns_symbol_missing
           missing_reserved_symbols_load_with_none
线程安全    core_is_send_and_sync                     编译期 Send + Sync 断言
回调桥      callback_bridge_lifecycle                 单个测试串行覆盖整个生命周期
```

### 编译期 const 断言的原理

布局与常量断言（`tests/libretro_abi.rs`）没有用运行时 `assert_eq!`，而是编译期断言：

```rust
const _: () = {
    assert!(size_of::<RetroCoreOptionDefinition>() == 2080);
    assert!(offset_of!(RetroCoreOptionDefinition, default_value) == 2072);
};
```

打个比方：`assert_eq!` 是"产品出厂后的抽检"——谁忘了跑测试，谁就带着错误布局发布；`const _: () = assert!(...)` 是"图纸审查"——尺寸不对，**编译阶段就直接拒收**（`error[E0080]`），连 `cargo build` 都过不去。FFI 布局错误是最危险的（不是崩溃就是静默内存破坏），所以选图纸审查。

这个选择还带来一个红利：32 位 arm 设备（rg35xx 等）的布局期望值与 64 位不同，在 macOS 主机上**永远跑不到** 32 位分支——但 `cfg(target_pointer_width = "32")` 分组下的 const 断言会在交叉编译时被求值。`cargo check` 不产出可执行文件也不需要真机，它只做类型检查和编译期求值——恰好足够验证 32 位布局断言：

```sh
rustup target add armv7-unknown-linux-gnueabihf
cargo check --tests --target armv7-unknown-linux-gnueabihf -p minarch
```

### 回调桥生命周期测试的设计

`tests/core_loading.rs` 的 `callback_bridge_lifecycle` 把整个回调桥的行为塞进**单个测试**，且断言全部写在测试主体、处理器只捕获参数。这是两个踩坑后的选择：

**为什么合并成单测试？** `FRONTEND_STATE` 是进程级 `OnceLock`，注册一次后无法撤销。如果拆成多个测试，cargo 并行运行时会互相污染（一个测试注册了处理器，另一个测试的"未注册默认值"断言就废了）。单测试内按「未注册默认值 → 注册转发 → 重复注册报错 → 首次处理器仍生效」串行走完生命周期，天然规避并行冲突。

**为什么断言不写在回调处理器里？** 处理器是由 C 核心经 `extern "C"` 函数指针调用的。Rust 的 panic 不能穿过 FFI 边界——在处理器里写 `assert_eq!`，断言失败时进程直接 SIGABRT（abort），拿不到正常的测试失败报告。所以处理器只把参数捕获进静态量：

```rust
fn video_handler(data: *const c_void, width: u32, height: u32, pitch: usize) {
    VIDEO_WIDTH.store(width as usize, Ordering::SeqCst);   // 只捕获，不断言
}
// …retro_run 返回后，在测试主体里断言：
assert_eq!(VIDEO_WIDTH.load(Ordering::SeqCst), 160);
```

这样断言失败会以正常测试失败呈现（含文件名与行号），而不是整个测试进程消失。

### 运行方式与边界

```sh
cargo test -p minarch                    # 全部（14 个集成测试目标 + lib 单测，213 个运行时测试 + 编译期断言）
cargo check --tests --target armv7-unknown-linux-gnueabihf -p minarch   # 32 位布局验证
```

测试覆盖之外的部分（需要真机或未实现功能）：

- 真实核心兼容性（需真机 + 交叉编译核心）
- Special 预渲染钩子、`.bmp` 预览解码（未闭环功能，见装配章节边界清单）

