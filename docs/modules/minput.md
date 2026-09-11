# minput — 按键诊断工具

`minput` 是 MinUI 的"按键测试"小工具——一个独立的二进制，把设备声明的所有按键画在屏幕上，你逐个按键，对应按钮就会亮起（实心），松开后变回空心。它用来验证**按键映射是否可用**：每个物理键按下时，事件是否到达了系统。是发布包完整性要求的 6 个二进制之一（minui/minarch/keymon/clock/minput/show）。

## 模块定位与职责

```
用户在设备上运行 minput（独立二进制，如 Tools/Input.pak）
          │
          ├── 画出全部按键面板
          │   ├── L 组    L1（+ L2 若有）
          │   ├── R 组    R1（+ R2 若有）
          │   ├── DPAD    U/D/L/R（十字键）
          │   ├── ABXY    X/B/Y/A（动作键）
          │   ├── 音量组  VOL - / VOL +（若有音量键）
          │   ├── 系统组  MENU / POWER（按能力增减）
          │   ├── META    SELECT / START / QUIT
          │   └── L3/R3   （若设备有摇杆按下键）
          │
          ├── 你按键 → 对应按钮点亮（实心 Button）
          ├── 松开 → 变回空心（Hole）
          └── SELECT+START 同时按下 → 退出
```

**核心职责**：提供一个极简的按键映射验证器。没有菜单、没有状态栏、没有多余功能——只有按键面板和"按下点亮"。这符合 MinUI 的"极简"哲学。

**为什么需要独立进程？** 与原版 C 一致，minput 是独立二进制，被打包为 `Tools/Input.pak`（原版 `makefile:56` 直接拷贝 `minput.elf`）。它和 minui/minarch 一样通过 `Platform` trait 访问硬件，但本身不参与启动器或游戏流程——开发者/用户在需要验证按键映射时运行它。

## 内部结构（子组件）

```
crates/minput/src/
├── main.rs      ← 入口：装配层（启动序列、主循环、绘制编排、平台接线）
└── layout.rs    ← 纯逻辑：设备能力收集 + 面板布局几何 + 按键状态查询（可单测）
```

**当前状态**：两个模块全部实现并带单元测试。`main.rs` 完成装配（启动序列 → 主循环 → 退出序列），依赖 `layout`（能力/几何/状态查询）纯逻辑模块。

## 运行逻辑（模块内部流程）

**启动失败语义**：初始化失败（`init_video`/`init_input` panic、图集/字体 `.expect`）即进程非零退出、不进入主循环——平台初始化在掌机上是"要么成功要么重启"（Platform trait 不返回 Result 的既定契约）。

### 主循环（简化）

```
1. main() 创建平台实例: let mut platform = Tg5040::new();
2. run(&mut platform) 装配:
   platform.set_cpu_speed(Menu)    ← CPU 降频省电
   platform.init_video()           ← SDL 视频初始化
   platform.init_input()           ← 输入初始化
   load_atlas / load_font          ← 资源加载（图集 + 字体）
   Capabilities::from_platform()   ← 收集设备按键能力（编译期常量）
3. 主循环:
   loop:
     let input = platform.poll_input()        ← 读取按键
     if SELECT+START:  break                  ← 组合键退出
     if any_pressed || any_just_released:      ← 按键状态变化
       dirty = true
     if dirty:                                 ← 脏帧渲染
       fill_rect(RGB_BLACK)                    ← 清屏
       layout_background(...) → blit_pill     ← 组药丸 + DPAD 连接条
       layout_buttons(...) → blit 按钮         ← 全部按钮（按下/未按）
       platform.flip(&screen, wait_vsync)
       dirty = false
     else:                                     ← 不脏帧时限幅（60fps）
       sleep(FRAME_BUDGET - elapsed)
4. 退出:
   platform.quit_input() / quit_video()
```

**关键设计**：能力收集（`Capabilities::from_platform`）、面板几何（`layout_background`/`layout_buttons`）、按键状态查询（`button_pressed`）**全部是纯函数**——不依赖平台、不碰 SDL，可在桌面上直接 `cargo test` 单测。装配层（main.rs）只做"收集数据 + 调用纯函数 + 驱动渲染"。

### `<P: Platform>` 泛型参数

minput 与 minui/minarch/clock 一样，函数签名使用 `<P: Platform>`。这个 `P` 是 Rust 的**泛型参数**——编译时的"占位符"，最终被替换为具体平台类型（如 `Tg5040`）。

**通俗理解**：minput 的代码只对着"插座标准"写——我需要读按键（`poll_input`）、需要显示画面（`flip`）、需要降频（`set_cpu_speed`）。至于插在墙上的具体是什么设备，编译时才决定。所以 minput 是**通用工具**：代码本身不绑定任何平台，但编译时必须通过 feature 透传实例化一个具体平台。

### 设备按键能力（Capabilities）

minput 画哪些按钮，取决于设备"声明"有哪些键。这个声明是**编译期**确定的——来自 `Platform` trait 的 8 个能力常量（`HAS_L2`/`HAS_R2`/`HAS_L3`/`HAS_R3`/`HAS_LS`/`HAS_RS`/`HAS_VOLUME`/`HAS_MENU`，加上既有的 `HAS_POWER_BUTTON`）：

```
设备声明（编译期）        minput 渲染          你的操作
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ HAS_L2=true  │ →  │ 画 L2 按钮   │ →  │ 按 L2        │
└──────────────┘    └──────────────┘    └──────┬───────┘
                                               ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ HAS_L3=false │ →  │ 不画 L3      │    │ (无此键)      │
└──────────────┘    └──────────────┘    └──────────────┘
                                               │
          按键事件 → 平台 poll_input 翻译 → InputState → 按钮点亮
```

注意**两层职责**的分离：
- **能力常量**（`HAS_*`）：设备"声明"有哪些键（编译期，minput 据此画按钮）
- **poll_input 翻译**（运行时）：按键事件变成 `InputState` 里的键位（tg5040 已实现 L2/R2 轴触发、LX/LY 摇杆方向键位）

一个键"可用" = ① 声明存在（`HAS_*` 为 true，所以按钮被画出来）∧ ② 你按下时按钮亮起（事件到达 `InputState`）。**minput 不检测硬件是否损坏**——它是给你确认"按键映射对不对"的工具。

### 摇杆为什么没有显示？

`HAS_LS`/`HAS_RS`（有无左右摇杆）被收集了，但**面板不渲染摇杆区域**。这是忠实于原版 C 的行为：原版 `minput.c:57-58` 计算了 `has_LS`/`has_RS` 但从未使用——minput 定位是**按键**测试工具，不是摇杆校准工具。推摇杆不会点亮任何按钮（摇杆方向被翻译成 `BTN_ANALOG_*`，minput 不查询这些位）。

> 如果你在 minput 里推摇杆没反应，不是摇杆坏了——这是与 C 原版一致的预期行为。

## 与原 C minput 的对比

| 维度 | 原 C（274 行） | Rust |
|------|-----------------|------|
| 能力探测 | 平台宏编译期算死（`BUTTON_*!=BUTTON_NA || ...`） | `Platform` trait 8 个 `HAS_*` 关联常量（编译期） |
| 面板几何 | main 里直接写死（`minput.c:87-260`） | `layout::layout_buttons`/`layout_background` 纯函数，能力组合 × 几何可单测 |
| 按键状态 | `PAD_isPressed(btn)` 直接查全局 | `InputState::is_pressed` 包装（`button_pressed`） |
| 按钮素材 | `ASSET_BUTTON`（按下）/`ASSET_HOLE`（未按）SDL blit | `render::asset` 的 `Button`/`Hole`，`render::pill` blit |
| 布局常量 | `SCALE1(PILL_SIZE)` 等宏 | 复用 `common::video` 的 `PILL_SIZE`/`BUTTON_SIZE`/`BUTTON_MARGIN`/`PADDING`/`FRAME_BUDGET_MS` |
| 设置初始化 | `InitSettings()`（从未使用） | 省略（无谓依赖） |
| 状态栏 | `GFX_blitHardwareGroup` 被注释掉 | 不渲染（忠实于 C） |

## 关键技术拓展

### 为什么能力常量放 Platform trait 而不是 minput 内部？

`has_*` 是**设备硬件知识**（编译期确定），不是 minput 的业务逻辑。放 `Platform` trait 与既有的 `HAS_POWER_BUTTON`/`BTN_MOD_*` 同构——每个平台声明自己的按键能力，任何消费方（minput、未来的摇杆诊断工具）都能用。tg5040 的 smart/brick 差异（L3/R3 仅 brick 有）用 `#[cfg(feature)]` 编译期分支表达，与 `SCREEN_WIDTH` 同模式。

### 为什么用脏帧模式？

minput 常驻待测，可能几分钟没有按键操作。脏帧模式（有按键变化才重绘）在无操作时零重绘、只限幅空转——对掌机电池敏感。原版 C 也是这个结构（`minput.c:78` 的 `dirty` 标志），非必要不改变。

### 为什么工具私有宽度放 minput 内部？

音量组/系统组/META 组的药丸宽度（`99`/`130`/`42`/`98` 等）是 **minput 面板专用**的布局值，其他 UI 不用。项目规范要求"新增 UI 布局常量前先检查 `common::video`"——检查结果：`PILL_SIZE`/`BUTTON_SIZE`/`BUTTON_MARGIN`/`PADDING`/`FRAME_BUDGET_MS` 全部已有、直接复用；`99`/`130`/`42`/`98` 是 minput 私有魔法值（原版 C 也是 minput.c 内部魔法值，非 defines.h 通用宏），放 minput 内部保持工具自洽。

## 公开 API 说明

minput 是一个二进制 crate（`main.rs`），不对外暴露库 API。其模块结构仅供内部组织代码使用。

主要入口：

```rust
// main.rs
fn main() {
    #[cfg(feature = "tg5040")]
    {
        let mut platform = tg5040::Tg5040::new();
        run(&mut platform);          // 实际装配（启动序列 → 主循环 → 退出）
    }
}
```

纯逻辑模块（`layout.rs`）的函数是 `pub`，但仅在同一 crate 内（`mod layout`）可见——它们不构成对外 API，而是为了单元测试可达。

### 平台编译选择

与 clock 相同，minput 通过 Cargo feature flag 选择目标平台（设备 feature 必选——`tg5040` 后必须跟 `/smart` 或 `/brick`，见 workspace-structure spec「平台 feature 系统规范」）：

```sh
cargo build -p minput --features tg5040/smart --release
```

每个 feature 对应一个 `platform-*` crate。编译时只有一个平台实现被链接进来。minput 的 `Cargo.toml` 声明 optional 平台依赖 + `tg5040` feature（`["dep:tg5040"]`），与 `crates/clock/Cargo.toml` 完全同模式。

## 关键代码解析

### 布局纯函数（layout_buttons）

按钮面板的几何是 minput 的核心逻辑，对应原版 `minput.c:87-260` 的 7 组绘制。Rust 版提取为纯函数：

```rust
pub fn layout_buttons(
    caps: &Capabilities,       // 设备能力（画哪些按钮）
    input: &InputState,        // 当前帧按键（按下态）
    scale: u32,                // 平台缩放倍率
    screen_w: u32,             // 屏幕宽度
    text_width: &impl Fn(&str) -> u32,  // 文字测量（装配层传 size_text）
) -> Vec<ButtonLayout>
```

每个 `ButtonLayout` 含标签、键位掩码、坐标、宽度、按下态——装配层拿到后直接 blit。垂直起点 `oy` 有个细节：**无 L3/R3 时下移一个 `PILL_SIZE`**（对应 C `minput.c:65-66`）——没有 L3/R3 行的设备，面板整体下移补齐空间。

### 退出组合键（SELECT+START）

```rust
if input.is_pressed(BTN_SELECT) && input.is_pressed(BTN_START) {
    break;
}
```

对应原版 `minput.c:79`。SELECT+START 是 MinUI 的"万能退出"组合——不会误触（两个键相距远，同时按是有意的），且有 QUIT 文本提示。

### 按钮点亮（Button/Hole 素材切换）

```rust
let asset = if btn.pressed { Asset::Button } else { Asset::Hole };
blit_pill(&atlas, asset, scale, &mut screen, Rect { x: btn.x, y: btn.y, w: btn.w, h: 0 });
```

按下 = 实心 `Button` 素材，未按 = 空心 `Hole` 素材。这是原版 C `blitButton` 的 `ASSET_BUTTON`/`ASSET_HOLE` 切换（`minput.c:31`），Rust 版复用 `render::asset` 的既有素材——**不新增任何 render 原语**。

