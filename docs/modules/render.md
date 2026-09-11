# render — MinUI 像素渲染层

`render` 是 MinUI 的"画布层"。它汇集了所有"画东西"的函数——从圆角药丸按钮到游戏画面的缩放，从电池图标到文字排版——全部直接操作 RGB565 像素缓冲区（`common::video::VideoBuffer`），不依赖任何 SDL 类型。

```
load_atlas("assets@2x.png")
        │
        ▼
    Atlas (精灵图集，一次性加载到内存)
        │
        ├──→ blit_asset()  ──→ 裁切 sprite
        ├──→ blit_pill()   ──→ 圆角药丸（精灵帽 + 纯色填充）
        ├──→ blit_rect()   ──→ 圆角矩形（四角 + 填充条）
        ├──→ blit_battery()──→ 电池图标（外壳 + 填充条裁切）
        └──→ blit_button() ──→ 按钮（sprite + text）
                    │
    blit_hardware_group() ← 组合以上全部，输出状态栏
                    │
                    ▼
            VideoBuffer (RGB565 像素数组)
                    │
    调用方通过 Platform::flip() 提交到物理屏幕
```

---

## 内容地图

| 章节 | 回答的问题 |
|------|-----------|
| [为什么需要 render crate](#为什么需要-render-crate) | 为什么渲染要独立成 crate？ |
| [依赖关系](#依赖关系) | render 依赖谁？为什么只有 common/fontdue/png？ |
| [核心概念](#核心概念) | VideoBuffer / RGB565 / Rect / Asset |
| [模块总览](#模块总览) | 八个模块各管什么 |
| [asset — 精灵图集](#asset--精灵图集) | 图集加载与裁切（ASSET_RECTS 未缩放坐标） |
| [pill — 圆角药丸与矩形](#pill--圆角药丸与矩形) | 三段式/七段式绘制、坐标体系 |
| [text — 文字渲染与排版](#text--文字渲染与排版) | fontdue 栅格化、混合、换行截断 |
| [button — 按钮栏](#button--按钮栏) | 按钮测量与组布局 |
| [battery — 电池图标](#battery--电池图标) | 充电分支、src_rect 百分比裁切 |
| [hardware — 状态栏](#hardware--状态栏) | 状态栏双分支、设置调节渲染 |
| [thumbnail — 缩略图加载](#thumbnail--缩略图加载) | PNG 缩略图与 C 的差异 |
| [scaler — 像素缩放器](#scaler--像素缩放器) | IntegerScaler vs AaScaler、SIMD |
| [布局常量与颜色体系](#布局常量与颜色体系) | 单一来源的布局/颜色常量 |
| [C → Rust 迁移对照表](#c--rust-迁移对照表) | 原 C 渲染体系 vs Rust 版 |
| [测试策略](#测试策略) | 纯逻辑渲染如何 headless 测试 |
| [性能设计说明](#性能设计说明) | 热路径优化手段 |
| [否决方案清单](#否决方案清单) | 被否决的设计（防 AI 误引入） |
| [常见问题 FAQ](#常见问题-faq) | 高频疑问答疑 |

## 为什么需要 render crate

原版 MinUI 的渲染代码集中在 `api.c`（约 800 行）和 `scaler.c`（约 2800 行）中。它们直接操作 `SDL_Surface*`、调用 `TTF_RenderUTF8_Blended()`、在渲染函数内部访问全局变量 `gfx.mode` 和平台函数 `GetHDMI()`。这带来四个问题：

1. **SDL 耦合**：渲染测试需要初始化 SDL 上下文——无法在 CI 中运行，也无法跨平台验证
2. **平台耦合**：`GFX_blitHardwareGroup()` 内部调用 `GetHDMI()`、`PLAT_isOnline()`——修改渲染逻辑可能影响硬件层
3. **全局可变状态**：`gfx`、`pad`、`blend_args` 等全局变量——多线程不安全，难以追踪数据流
4. **无模块边界**：`api.c` 是单文件——药丸渲染、文字渲染、电池渲染全部混在同一文件中

Rust 版的设计回应是：

| 原 C 问题 | Rust 解决方案 |
|----------|-------------|
| SDL 耦合 | 所有渲染函数操作纯 Rust 类型（`&[u16]`、`&Atlas`、`&mut VideoBuffer`） |
| 平台耦合 | 硬件状态通过 `HardwareStatus` struct 传入——渲染函数不调用平台 API |
| 全局变量 | `AaScaler` 的混合参数存为私有字段、`load_atlas` 返回 `Atlas` 而非存入全局 |
| 单文件 | 8 个子模块按语义拆分——pill、asset、text、button、battery、hardware、thumbnail、scaler |

```rust
// C 方式：渲染函数内部访问全局
// GFX_blitPill(gfx.mode == MODE_MAIN ? ASSET_DARK_GRAY_PILL : ASSET_BLACK_PILL, ...);

// Rust 方式：调用方传入状态
let pill_asset = if status.mode_main { Asset::DarkGrayPill } else { Asset::BlackPill };
blit_pill(&atlas, pill_asset, scale, &mut screen, rect);
```

## 依赖关系

```
                    common（零依赖）
                        ↑
                    render（png + fontdue）
             ↙       ↓        ↓       ↘
         minui    minarch    clock    minput
```

- **common**：提供 `VideoBuffer`、`Rect`、`Asset` 枚举、布局常量——全部是纯数据结构，零第三方依赖
- **render**：依赖 `common` + `png` crate（精灵图集）+ `fontdue`（文字栅格化）。**不依赖任何平台 crate**
- **minui / minarch / clock / minput**：同时依赖 render（渲染 UI）和 platforms（硬件访问），在依赖边界上桥接两者

render 不依赖平台——这是架构的核心约束。它保证了渲染逻辑可以在开发机上独立测试，不需要掌机硬件或 SDL 环境。

## 核心概念

在深入各模块之前，需要理解 render crate 的三个基础概念。它们是所有渲染函数的"通用语言"。

### VideoBuffer — 画布

**什么是像素缓冲（pixel buffer）**：一块连续内存区域，每个像素用固定格式存储颜色。类比——一张方格纸，每格一种颜色。

```rust
pub struct VideoBuffer {
    pub pixels: Vec<u16>,   // RGB565 像素数据
    pub width: u32,          // 可见宽度（像素）
    pub height: u32,         // 可见高度（像素）
    pub pitch: u32,          // 每行像素数（≥ width）
}
```

**pitch（行距）是什么**：pitch 是内存中每行像素的数量。在绝大多数情况下 `pitch == width`——此时像素 `(x, y)` 的索引就是 `y * width + x`。但某些硬件要求像素行按 2/4/8 个像素对齐到内存边界——这时 `pitch > width`，每行末尾有不可见的"填充像素"。类比：方格纸上你只能在前 80 列写字，但纸张实际有 82 列——第 81、82 列是空白的。

```
pitch == width (最常见):         pitch > width (硬件对齐):
┌──┬──┬──┬──┐                    ┌──┬──┬──┬──┬──┬──┐
│A │B │C │D │                    │A │B │C │D │  │  │
├──┼──┼──┼──┤                    ├──┼──┼──┼──┼──┼──┤
│E │F │G │H │                    │E │F │G │H │  │  │
└──┴──┴──┴──┘                    └──┴──┴──┴──┴──┴──┘
pixels[1*4+2]=G                  pixels[1*6+2]=G
```

MinUI 在大多数平台使用 `pitch == width`。`VideoBuffer::new(w, h)` 创建的缓冲区默认 `pitch = width`。

**fill_rect**：最基础的渲染原语。用指定 RGB565 颜色填充矩形区域内的所有像素。**不做边界裁剪**——调用方负责确保 rect 完全位于 `(width, height)` 范围内。为什么？渲染循环中 `fill_rect` 被高频调用——每个像素增加一条 `if` 检查累积极大。一次性验证 rect 合法性，比逐像素检查高效得多。

### RGB565 色彩格式

**为什么用 16 位色？** 这是掌机硬件的物理约束：

- 大多数复古掌机的 LCD 控制器使用 RGB565 接口
- 内存考量：640×480×2 字节 = 614KB vs 640×480×4 字节 = 1.2MB——帧缓冲占用量差一倍
- 色觉考量：人眼对绿色最敏感——绿色通道分配 6 位（64 级），红蓝各 5 位（32 级）

**位布局图**：

```
RGB565 16 位：
┌────────────┬──────────┬──────────┐
│ R (5 bits) │ G (6)    │ B (5)    │
│ bits 15-11 │ bits 10-5│ bits 4-0 │
└────────────┴──────────┴──────────┘

纯红: 0xF800 = 11111 000000 00000
纯绿: 0x07E0 = 00000 111111 00000
纯蓝: 0x001F = 00000 000000 11111
纯白: 0xFFFF = 11111 111111 11111
纯黑: 0x0000 = 00000 000000 00000
```

**8-bit RGBA → RGB565 的转换**（加载 PNG 素材时的核心操作）：

```
RGBA(255,0,0,255) → RGB565:
  R = 255 >> 3 = 31 (5bit)   →  bits[15:11] = 11111
  G = 0   >> 2 = 0  (6bit)   →  bits[10:5]  = 000000
  B = 0   >> 3 = 0  (5bit)   →  bits[4:0]   = 00000
  结果: 0xF800
```

```rust
// RGB565 转换公式（出现在 load_atlas 和 load_thumbnail 中）
let rgb565 = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
```

颜色常量（编译期 RGB565 值）：

| 常量 | 值 | 颜色 | 对应原 C |
|------|-----|------|---------|
| `RGB_WHITE` | `0xFFFF` | 白 | `SDL_MapRGB(..., 0xFF,0xFF,0xFF)` |
| `RGB_BLACK` | `0x0000` | 黑 | `SDL_MapRGB(..., 0,0,0)` |
| `RGB_LIGHT_GRAY` | `0xCE79` | 浅灰 | `SDL_MapRGB(..., 0xCE,0xCF,0xCE)` |
| `RGB_GRAY` | `0x9CD3` | 灰 | `SDL_MapRGB(..., 0x9C,0xDF,0x9C)` |
| `RGB_DARK_GRAY` | `0x4208` | 深灰 | `SDL_MapRGB(..., 0x42,0x10,0x42)` |

```rust
// 创建画布并在左上角画一个白色矩形
let mut buf = VideoBuffer::new(320, 240);
buf.fill_rect(Rect { x: 0, y: 0, w: 100, h: 50 }, RGB_WHITE);
// pitch == width，所以像素 (10, 10) 的索引 = 10 * 320 + 10 = 3210
assert_eq!(buf.pixels[3210], 0xFFFF); // 白色
assert_eq!(buf.pixels[60 * 320 + 10], 0x0000); // 黑色（矩形外）
```

### Rect 坐标系统

```rust
pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
```

`(x, y)` = 左上角坐标，`w` = 宽度，`h` = 高度。所有坐标以像素为单位，且已按平台 `SCALE` 缩放（在调用 render 函数之前，由调用方完成所有的 `scale` 乘法）。

**为什么所有渲染函数不做边界裁剪？** 这是设计与性能的权衡。热路径（hot path）中的渲染函数被每帧调用数十次——在每个像素的循环中加入 `if col < dst.width && row < dst.height` 检查，累计开销超过一次性的 rect 验证。代价是调用方需要确保坐标合法——这是合理的分工：布局逻辑负责合法的坐标，渲染逻辑负责快速绘制。

### Asset 精灵图集系统

精灵图集是一张 100×64 像素（未缩放）的 PNG 图片，包含 MinUI 全部 UI 素材的像素数据。渲染时不是每次打开文件，而是：

1. **启动时**：`load_atlas()` 一次性加载整张 PNG → `Atlas`（RGB565 像素数组）
2. **渲染时**：`blit_asset()` 从 `Atlas` 中裁切指定素材的矩形区域并复制到目标 `VideoBuffer`

**ASSET_RECTS 坐标表**：定义了 26 个素材（`Asset` 枚举）在图集中的源矩形坐标。单色素材（索引 0-13，如 `WhitePill`）的精灵只是形状——颜色由 `ASSET_RGBS[asset]` 的纯色填充决定。彩色素材（索引 15-25，如 `BatteryIcon`、`WiFi`）用图集原像素颜色。

## 模块总览

| 模块 | 公开函数/类型 | 职责 | 依赖 | 对应原 C |
|------|------------|------|------|---------|
| `pill` | `blit_pill`, `blit_rect` | 圆角药丸与矩形 | 无 | `GFX_blitPill` `GFX_blitRect` |
| `asset` | `Atlas`, `load_atlas`, `blit_asset` | 精灵图集加载与裁切 | `png` | `GFX_init` `GFX_blitAsset` |
| `text` | `render_text`, `size_text`, `truncate_text`, `wrap_text`, `blit_message`, `blit_text` | 文字渲染与排版 | `fontdue` | `TTF_RenderUTF8_Blended` `GFX_sizeText` 等 |
| `button` | `blit_button`, `blit_button_group`, `blit_hardware_hints`, `get_button_width` | 按钮栏 | `fontdue` | `GFX_blitButton` `GFX_blitButtonGroup` |
| `battery` | `blit_battery` | 电池图标 | 无 | `GFX_blitBattery` |
| `hardware` | `HardwareStatus`, `blit_hardware_group` | 状态栏 | 无 | `GFX_blitHardwareGroup` |
| `thumbnail` | `load_thumbnail` | ROM 缩略图 | `png` | `IMG_Load` (缩略图) |
| `scaler` | `IntegerScaler`, `AaScaler`, `BilinearScaler` | 画面缩放 | 无 | `scale{xmul}x{ymul}_c16` `scaleAA`（双线性对应 tg5040 GPU 过滤） |

---

## asset — 精灵图集

### Atlas 结构体

```rust
pub struct Atlas {
    pub pixels: Vec<u16>,  // RGB565 像素数据
    pub width: u32,         // 图集宽度
    pub height: u32,        // 图集高度
}
```

一次加载，无限次使用。所有 UI blit 都从同一块内存读取，无需重复 I/O。精灵图集原始画布为 100×64 像素（未缩放），实际加载的 PNG 可能已按平台 `SCALE` 缩放（如 `assets@2x.png` 为 200×128）。

### load_atlas

使用 `png` crate 解码 PNG 文件——替代 C 的 `SDL_image` / `IMG_Load`。支持四种 PNG 色彩格式（Rgba、Rgb、Grayscale、GrayscaleAlpha），逐行解码 → RGBA→RGB565 转换 → 存入 `Vec<u16>`。

**返回值**：`Result<Atlas, String>`。图集缺失或格式错误是硬错误——图集是 MinUI 渲染的基础依赖，缺少它意味着整个 UI 无法渲染。

```rust
let atlas = load_atlas(".system/res/assets@2x.png")
    .expect("图集加载失败——UI 无法渲染");
```

### blit_asset — 裁切与复制

从图集中裁切指定 Asset 的源矩形、按 `scale` 缩放坐标、逐行 `copy_from_slice` 复制到目标缓冲区。等价于 C 的 `SDL_BlitSurface`，但操作的是裸 `&[u16]` 切片。

```
blit_asset(atlas, WhitePill, scale=2, dst, pos=(10,10), src_rect=None)
                                     │
  ASSET_RECTS[WhitePill] = {1,1,30,30}
  × scale = {2,2,60,60}              │
                                     ▼
  Atlas                                   VideoBuffer
  ┌────────────────────────┐              ┌────────────────────────┐
  │  ░░░░░░░░░░            │              │                        │
  │  ░WhitePill░  (2,2)    │   copy      │     (10,10) WhitePill  │
  │  ░░░░░░░░░░            │  ──────→    │     60×60 像素        │
  │                        │              │                        │
  └────────────────────────┘              └────────────────────────┘
```

**src_rect 裁切机制**：`None` = 使用整个源矩形。`Some(clip)` = 仅裁切 clip 矩形（相对于 Asset 源矩形左上角）。clip 坐标也是未缩放的——由 `blit_asset` 内部乘以 `scale`。这个机制在电池模块中被巧妙利用——通过动态调整 `src_rect` 的宽度实现电量百分比显示。

```rust
// 完整 sprite
blit_asset(&atlas, Asset::WhitePill, 2, &mut screen, (10, 10), None);

// 仅左半部分（裁切 w=15, h=30）
blit_asset(&atlas, Asset::WhitePill, 2, &mut screen, (10, 10),
    Some(Rect { x: 0, y: 0, w: 15, h: 30 }));
```

**与 C 的对比**：

| 维度 | 原 C | Rust |
|------|------|------|
| API | `SDL_BlitSurface(sprite_sheet, &srcrect, dst, &dstrect)` | `blit_asset(atlas, asset, scale, dst, pos, src_rect)` |
| 坐标 | 预缩放（`SCALE4` 在 init 时已乘） | 未缩放 + 运行时 `× scale` |
| 像素格式 | 依赖 SDL surface 的 format | 固定 RGB565 |

---

## pill — 圆角药丸与矩形

### blit_pill — 三段式结构

圆角药丸（Pill）由三段组成：

```
┌──────────────┬────────────────────────┬──────────────┐
│   左半圆帽    │     中间纯色填充         │   右半圆帽    │
│ blit_asset() │   fill_rect()          │ blit_asset() │
│ (精灵抗锯齿边) │   (颜色来自ASSET_RGBS)   │ (精灵抗锯齿边) │
└──────────────┴────────────────────────┴──────────────┘
         ← r×scale →          ← center_w →        ← r×scale →
         ├──────────────── w ──────────────────────────────┤
```

- `r = ASSET_RECTS[asset].h / 2`（精灵高度的一半），未缩放
- 左帽从精灵图集左侧裁切 `r` 像素宽 → blit → 中间 `fill_rect` 纯色 → 右帽裁切精灵右侧 `r` 像素
- 如果 `w < h`：宽度自动扩展到高度——防止药丸变成椭圆
- 如果 `h == 0`：使用 `ASSET_RECTS[asset].h × scale` 作为默认高度

**为什么精灵 + 填充**而不是纯代码绘制圆角？精灵图集提供的半圆帽已经包含抗锯齿边缘（设计师在 Photoshop 中手动绘制的），`fill_rect` 只负责中间平坦区域。这比纯代码绘制贝塞尔曲线更快（内存读取 vs 数学计算）。

### blit_rect — 七段式结构

圆角矩形（Rect）由七段组成——4 个四分之一圆角（精灵 blit）+ 3 条水平纯色填充：

```
┌──────────────┬──────────────────────────┬──────────────┐
│   左上角 (TL) │     上边填充 (fill_rect)   │   右上角 (TR) │
│ blit_asset() │                          │ blit_asset() │
├──────────────┼──────────────────────────┼──────────────┤
│                                              │         │
│          中间全宽填充 (fill_rect)               │   ← 三条水平填充条
│                                              │         │
├──────────────┼──────────────────────────┼──────────────┤
│   左下角 (BL) │     下边填充 (fill_rect)   │   右下角 (BR) │
│ blit_asset() │                          │ blit_asset() │
└──────────────┴──────────────────────────┴──────────────┘
```

- 圆角半径 `rs = (ASSET_RECTS[asset].w / 2) × scale`
- 上/下边填充覆盖两角之间的水平区域（宽度 = `w - ds`）
- 中间填充覆盖全宽（宽度 = `w`）

### 坐标体系设计

**关键决策**：`ASSET_RECTS` 存储**未缩放**坐标（原始像素值）。C 版本在初始化时用 `SCALE4()` 宏预乘 scale 的值存入 `asset_rects`。

| 方式 | C | Rust |
|------|---|------|
| 存储 | `SCALE4(1, 1, 30, 30)` → `{4,4,120,120}`（scale=4） | `Rect{x:1,y:1,w:30,h:30}`（始终未缩放） |
| 使用时 | 直接拿坐标 | `base.x × scale` |
| 多平台 | 每个平台需重新编译（scale 不同） | 同一份坐标表，scale 由调用方决定 |

```rust
// 状态栏药丸：scale=2 时宽 120、高 60（对应未缩放的 60×30）
blit_pill(&atlas, Asset::DarkGrayPill, 2, &mut screen, Rect {
    x: 320, y: 10, w: 120, h: 60,
});

// 存档槽圆角矩形面板
blit_rect(&atlas, Asset::Option, 2, &mut screen, Rect {
    x: 50, y: 100, w: 400, h: 200,
});
```

## text — 文字渲染与排版

text 模块使用 `fontdue` crate 作为字体引擎。fontdue 是一个纯 Rust 的 TTF/OTF 栅格化库，替代 C 代码中的 SDL_ttf。

### fontdue 与 SDL_ttf 的关键差异

| 维度 | 原 C (SDL_ttf) | Rust (fontdue) |
|------|---------------|---------------|
| 栅格化输出 | `SDL_Surface*`（ARGB 像素数据） | `(&[u8], Metrics)`（alpha 位图 + 字形信息） |
| 混合方式 | SDL 内部 alpha 混合到目标 surface | 手动 `blend_pixel`——逐像素通道分离加权 |
| 字号管理 | 为每个字号打开一个 `TTF_Font*`（C 代码有 4 个） | 一个 `Font` 实例支持所有字号 |
| 测量 | `TTF_SizeUTF8()` | `size_text()`——由 rasterize 的 advance_width 累加 |

### render_text — Alpha 混合

逐字符栅格化，对返回的 alpha 位图（8-bit 透明度信息）逐像素混合到目标 RGB565 背景上。

**核心算法——通道分离的加权平均**：

```rust
// 不能对整个 u16 做算术平均——RGB565 的 R/G/B 跨位边界
// 例如: 0xF800 (纯红, R=31 G=0 B=0) + 0x07E0 (纯绿, R=0 G=63 B=0)
// 直接的 u16 平均 = (0xF800 + 0x07E0) / 2 = 0x7FF0
// 这会污染红色通道的进位到绿色通道

// 正确做法：拆开 R(5bit)/G(6bit)/B(5bit) 各自计算
let fr = ((fg >> 11) & 0x1F) as u32;  // 前景 R
let fg_ = ((fg >> 5) & 0x3F) as u32;  // 前景 G
let fb = (fg & 0x1F) as u32;           // 前景 B

let br = ((bg >> 11) & 0x1F) as u32;  // 背景 R
let bg_ = ((bg >> 5) & 0x3F) as u32;  // 背景 G
let bb = (bg & 0x1F) as u32;           // 背景 B

// 各自独立做线性插值
let r = (fr * alpha + br * (255 - alpha)) / 255;
let g = (fg_ * alpha + bg_ * (255 - alpha)) / 255;
let b = (fb * alpha + bb * (255 - alpha)) / 255;

// 重新打包为 RGB565
((r as u16) << 11) | ((g as u16) << 5) | (b as u16)
```

alpha = 0（全透明）和 alpha = 255（全不透明）走快速路径——无需计算，直接返回背景或前景色。

### size_text — 多行尺寸测量

按 `\n` 分割文字 → 逐字符调用 `font.rasterize(c, px)` 累加 `advance_width` → 宽度取所有行最大值 → 高度 = `行数 × px + (行数-1) × leading`。空字符串返回 `(0, 0)`。

### truncate_text — 贪心截断

循环：移除末尾 3 个 Unicode 字符 → 追加 `"..."` → 重新测量宽度 → 若仍超宽则继续。最小返回值是 `"..."`。

**为什么返回新 `String` 而非原地修改 `&mut str`？** 原 C 版 `GFX_truncateText` 原地修改 `char*`——C 可以用 `\0` 截断字符串。Rust 的 `String` 是 UTF-8 编码——`"…"` 是 3 字节（`\xE2\x80\xA6`）、`truncate()` 以字节偏移操作——原地截断容易在错误边界切断多字节字符导致 panic。返回新 String 更安全。

### wrap_text — 空格分词自动换行

按空格分词 → 贪心填满当前行 → 溢出则新起行；单个单词超过 `max_width` 时允许溢出（不截断单词，保留可读性）。

### blit_message / blit_text

`blit_message`：多行居中渲染，各行独立水平居中，填充色固定为 `RGB_WHITE`。`blit_text`：多行左对齐渲染，支持行距参数（`px + leading` 像素/行）。

```rust
// 加载字体（一个 Font 支持所有字号——比 C 版简单）
let font_data = std::fs::read(".system/res/BPreplayBold-unhinted.otf")?;
let font = Font::from_bytes(font_data, FontSettings::default())?;

// 渲染单行文字
render_text(&mut screen, &font, "你好，MinUI！", 32, RGB_WHITE, (20, 50));

// 测量 → 截断
let (w, _) = size_text(&font, "超长文件名.gba", 24, 4);
if w > 300 {
    let short = truncate_text(&font, "超长文件名超长文件名.gba", 300, 24);
    // → "超长文件名超长文..."
    render_text(&mut screen, &font, &short, 24, RGB_WHITE, (20, 80));
}

// 多行居中消息
blit_message(&mut screen, &font, "存档已保存\n3 个存档槽", 16,
    Rect { x: 0, y: 0, w: 320, h: 240 });
```

### 字形度量边界——descender（y/g/p）的负 ymin

fontdue 的 `xmin`/`ymin` 度量是**有符号** `i16`：含下行笔画（descender）的字形（`y`/`g`/`p` 等）`ymin` 为负（基准线以下部分）。`render_text` 把字形放进目标坐标时若直接做 `pos + ymin as u32`，负数转 `u32` 会变成巨大数、减法下溢——**这是真机必炸的 bug**（任何含 y/g/p 的 ROM 名都会触发），曾作为 minarch ui 测试的伴随修复被发现。

处理方式：目标坐标改用**有符号运算并夹紧到 0**：

```rust
let dst_x = (pos.0 as i64 + x_offset as i64 + metrics.xmin as i64).max(0) as u32;
let dst_y = (pos.1 as i64 + px as i64 - metrics.height as i64 - metrics.ymin as i64).max(0) as u32;
```

正常字形（`ymin ≥ 0`）下公式与原实现等价；descender 字形经有符号运算得到正确下移位置。回归测试：`render_text_descender_no_underflow`（渲染含 `y` 的 "My Game"）。被否决的偷懒方案：测试数据回避 descender 字形（掩盖真机必炸 bug）；在 ui 调用层 clamp（bug 属 render 层，治标不治本）。

## button — 按钮栏

### 单字符 vs 多字符按钮

单字符按钮（`"A"`）和多字符标签（`"+ -"`）有不同的渲染路径：

- **单字符**：blit `ASSET_BUTTON` sprite（20×20 未缩放）+ hint 文字在右侧。设计假设——单个字母放在 sprite 圆角方块中更美观
- **多字符**：直接渲染标签文字（使用 `px` 字号，无 sprite）+ hint 文字在更右侧。3 个字符的宽度已经超过单个 sprite——blit sprite 没有意义

### blit_button_group — 按钮组布局

在 pill 背景上排列 1-2 个按钮。支持居中或右对齐：

```
计算各按钮宽度 → pill_w = sum(widths) + padding × 2 + BUTTON_PADDING × scale
            → pill_h = BUTTON_SIZE × scale + BUTTON_PADDING × scale
            → 绘制 WhitePill 背景
            → 从 pill 左边缘 + padding 开始逐个 blit_button
```

```rust
// "打开 (A)" + "返回 (B)" 按钮组，居中显示
blit_button_group(
    &[("打开", "A"), ("返回", "B")],
    &atlas, &font, &mut screen,
    Rect { x: 0, y: 400, w: 640, h: 60 },
    false,  // 居中而非右对齐
    16, 2,  // px=16, scale=2
);
```

### get_button_width — 宽度测量

```
单字符："BUTTON_SIZE × scale + margin + hint_width"
多字符："BUTTON_SIZE × scale / 2 + label_width + margin + hint_width"
```

### blit_hardware_hints — 按键提示文字（状态栏/版本页底部）

`blit_hardware_hints` 渲染状态栏下方的按键提示条（如 `+ -` 亮度/音量调节提示）。行为契约：

- `show_setting == 0`（状态栏模式）时**无操作**——提示只在设置调节（亮度/音量面板弹出）时显示
- 提示标签为 `BRIGHTNESS_BUTTON_LABEL`（`"+ -"`），用 **`font.large` 字号**渲染（原 C 用 `font.large` 而非 `font.tiny`——提示需在设置面板下可读）
- 纯文字渲染，不涉及 sprite/药丸素材
- 绘制位置：底部右对齐（`blit_button_group` 同款对齐语义）

---

## battery — 电池图标

### 充电 vs 非充电两个分支

```
                 充电中                          非充电
                 ──────                          ──────
外壳              BatteryIcon                     ≤10% → BatteryLow
                                                  >10% → BatteryIcon
闪电              BatteryBolt                     (无)
填充条            (不绘制)                        ≤20% → BatteryFillLow
                                                  >20% → BatteryFill
填充条宽度        —                               按百分比 src_rect 裁切
```

### src_rect 按百分比裁切——最精巧的用例

填充条的动态裁切通过 `blit_asset` 的 `src_rect` 参数实现。这是 `src_rect` 在项目中最高级的用法：

```
fill_base = ASSET_RECTS[BatteryFill] = { x:81, y:33, w:12, h:6 }

电量 80%:
  clip_w = 12 × 80 / 100 = 9
  clip_x = 12 - 9 = 3           ← 右对齐：从精灵右侧开始取样
  src_rect = Rect { x: 3, y: 0, w: 9, h: 6 }

电量 0%:
  clip_w = 0 → 提前返回，不绘制填充条（仅保留外壳）
```

**为什么是右对齐？** 电池填充条在精灵图集中是满电状态的全长条。电量越少，裁切后的条越短——但条形始终贴在电池的右侧边缘（电量从右侧"退去"）。

### PILL_SIZE 居中

电池图标在 `PILL_SIZE × PILL_SIZE`（缩放后）区域内居中：

```
x = pos.0 + (PILL_SIZE × scale - (base.w × scale + scale)) / 2
y = pos.1 + (PILL_SIZE × scale - base.h × scale) / 2
```

`+ scale` 项补偿精灵图集右侧的像素间隙——与原 C 的 `+ FIXED_SCALE` 完全一致。

```rust
// 80% 电量，非充电
blit_battery(&mut screen, &atlas, 80, false, (100, 10), 2);

// 5% 低电量，正在充电——将自动使用红色外壳和闪电图标
blit_battery(&mut screen, &atlas, 5, true, (100, 10), 2);
```

## hardware — 状态栏

`blit_hardware_group` 是 render crate 中依赖面最宽的渲染函数——组合 `blit_pill`、`blit_asset`、`blit_battery` 三者，输出设备状态栏或亮度/音量调节 UI。

### HardwareStatus — 从全局变量到 struct

原 C 的 `GFX_blitHardwareGroup()` 在函数内部调用 6 个平台函数和 1 个全局变量来获取状态。这是 C 架构中「渲染 ↔ 平台」耦合的集中体现。Rust 版将所有这些打包为一个参数：

| 字段 | 类型 | 说明 | 原 C 来源 |
|------|------|------|---------|
| `show_setting` | `u8` | 0=状态栏, 1=亮度, 2=音量 | `PWR_update()` 返回值 |
| `setting_value` | `u8` | 当前亮度(0-10)或音量(0-20) | `GetBrightness()` / `GetVolume()` |
| `battery_percentage` | `u8` | 电量百分比 (0-100) | `PLAT_getBatteryStatus()` |
| `battery_charging` | `bool` | 是否充电中 | `PLAT_getBatteryStatus()` |
| `wifi_online` | `bool` | WiFi 是否已连接 | `PLAT_isOnline()` |
| `mode_main` | `bool` | true=主界面, false=菜单界面 | `gfx.mode == MODE_MAIN` |
| `has_hdmi` | `bool` | HDMI 是否连接 | `GetHDMI()` |

**为什么用 struct 而不用 10 个扁平参数？** 6 个 `bool`/`u8` 连续排列——编译器无法区分 `(wifi_online, mode_main, has_hdmi)` 和 `(mode_main, has_hdmi, wifi_online)`——参数顺序错误不会触发编译错误，但会生成微妙的渲染 bug。struct 的命名字段在编译期消除这类错误。

### 双分支逻辑

```
       show_setting == 0                               show_setting != 0
       (状态栏)                                         ∧ !has_hdmi (调节 UI)
       ──────────                                      ──────────────────
       DarkGrayPill / BlackPill                        DarkGrayPill / BlackPill
       ├── WiFi 图标 (wifi_online)                     ├── Brightness / Volume / Mute
       │   居中于 PILL_SIZE×scale 内                    │   (6,5)×scale 或 (8,7)×scale 偏移
       └── Battery (PILL_SIZE×scale 内居中)            ├── BarBg / BarBgMenu (滑条底)
                                                       └── Bar (填充，按百分比 width)
       has_hdmi ∧ show_setting≠0:
       → 退回到状态栏（降级展示——不显示调节 UI，但保留电池和 WiFi）
```

### 设置调节分支详解

**滑条总宽度**：`ow = (PILL_SIZE + SETTINGS_WIDTH + 10 + 4) × scale = 124 × scale`

**图标选择逻辑**：
- `show_setting == 1` → `Asset::Brightness`（亮度图标）
- `show_setting == 2 ∧ setting_value > 0` → `Asset::Volume`（正常音量图标）
- 否则 → `Asset::VolumeMute`（静音图标）

**滑条百分比计算**：
```rust
let (min, max) = if show_setting == 1 {
    (BRIGHTNESS_MIN, BRIGHTNESS_MAX)  // 0, 10
} else {
    (VOLUME_MIN, VOLUME_MAX)          // 0, 20
};
let fill_w = SETTINGS_WIDTH * scale * (setting_value - min) as u32 / (max - min) as u32;
```

**Bar pill 的渲染条件**：仅当 `show_setting == 1 || setting_value > 0`。亮度模式下即使值为 0 也显示（让用户看到空滑条）；音量模式值为 0 时不显示——因为静音图标已经说明了状态。这个逻辑完全遵循原 C 的 `if (show_setting==1 || setting_value>0)` 条件。

### HDMI 降级设计的推导

原 C 的条件是 `if (show_setting && !GetHDMI()) ... else ...`。当 HDMI 已连接且用户正在调节亮度/音量时，条件为 `false`，落入 else 分支——**退回到状态栏，而非完全跳过渲染**。

```
show_setting   GetHDMI()   走哪个分支
═══════════   ════════   ════════════
     0           0       → 状态栏
     0           1       → 状态栏
     1           0       → 亮度/音量 UI
     1           1       → 状态栏（降级——关键！）
```

**物理场景推演**：掌机插入 HDMI → 画面输出到电视 → 亮度调节失去意义（画面显示在电视上）→ 音量调节失去意义（音频通过 HDMI 输出）→ 但电池和 WiFi 状态不变。如果"完全跳过渲染、返回 0"，右上角将出现一块空白——用户会认为渲染出错了。原 C 的设计是**降级展示**——抑制无意义的调节 UI，但基础状态信息照常渲染。

```rust
// 状态栏模式
let hw = HardwareStatus {
    show_setting: 0, setting_value: 0,
    battery_percentage: 80, battery_charging: false,
    wifi_online: true, mode_main: true, has_hdmi: false,
};
let ow = blit_hardware_group(&atlas, &hw, 2, &mut screen);
// → 右上角出现 pill + WiFi 图标 + 电池图标

// 亮度调节模式（亮度 = 5/10）
let hw_bright = HardwareStatus { show_setting: 1, setting_value: 5, ..hw };
blit_hardware_group(&atlas, &hw_bright, 2, &mut screen);
// → 右上角出现 pill + Brightness 图标 + 滑条（填充 50%）
```

**返回值 `ow` 语义**：`blit_hardware_group` 返回状态栏占用的**水平宽度**（供调用方计算剩余布局空间——minui 的缩略图列、版本信息等按 `ow` 让位）。状态栏 pill 宽度 = `PILL_SIZE × scale`；WiFi 在线时再加 `(PILL_SIZE − 3) × scale`（WiFi 图标区），垂直/水平居中公式见「双分支逻辑」。该约定与原 C 一致（`GFX_blitHardwareGroup` 的返回语义）。

## thumbnail — 缩略图加载

MinUI 支持为 ROM 文件提供缩略图预览：`.res/<rom_file>.png`。`load_thumbnail` 加载 PNG 并返回独立的 `VideoBuffer`。

### 与 C 的差异

C 代码在 `minui.c` 中直接 `IMG_Load(res_path)` → `SDL_BlitSurface(screen)` → `SDL_FreeSurface`——加载和渲染在同一处完成。Rust 版返回 `Option<VideoBuffer>`——由调用方决定何时、何处 blit。这个解耦意味着调用方可以缓存缩略图、在不同位置多次渲染、或在渲染前缩放。

### 三个设计决策

**1. 为什么不复用 `load_atlas` 的代码？** `load_atlas` 返回 `Atlas`（需要 width/height 给坐标系统），`load_thumbnail` 返回 `VideoBuffer`。两者约 25 行的 PNG 解码循环结构相同但返回类型不同——提取共享函数需要引入新抽象类型，而两个独立函数的总代码量几乎不变。按项目规范"非必要不要增加复杂度"——独立实现比一个过度泛化的共享函数更简单。

**2. 为什么返回 `Option` 而非 `Result`？** 缩略图缺失是正常情况（绝大多数 ROM 没有配套缩略图），不是错误。`None` 直接对应 C 代码的"跳过渲染"——调用方无需 `match` 错误类型。

**3. `Vec::with_capacity` + `push` 代替 `vec![0; N]`？** `VideoBuffer::new()` 先零初始化全部像素，然后再逐像素覆盖。对于 640×640 的缩略图——浪费了约 800KB 的零初始化 + 覆盖写入。`with_capacity` + `push` 只分配不初始化，一次写入到位。

```rust
match load_thumbnail(".res/MyGame.gba.png") {
    Some(thumb) => {
        let x = screen.width.saturating_sub(thumb.width);
        // 缩略图放在右上角（需自己实现 blit）
    }
    None => { /* 无缩略图——正常情况，优雅跳过 */ }
}
```

## scaler — 像素缩放器

`scaler` 是 render crate 中**唯一不操作 `VideoBuffer` 的模块**——它直接操作裸的 RGB565 像素切片（`&[u16]`）。它的职责是将模拟器输出画面（如 GBA 的 240×160）缩放至设备物理分辨率（如 640×480）。这是性能最敏感的代码路径——在 400MHz ARM 芯片上，每帧缩放必须在 16ms 内完成以保持 60fps。

### IntegerScaler — 最近邻像素复制

当设备分辨率恰好是游戏分辨率的整数倍时使用。例如 GBA（240×160）在 480×320 屏幕——恰好 2 倍。算法极其简单——每个源像素在水平方向复制 `xmul` 次、垂直方向复制 `ymul` 次。

```
源图 4×4 像素：                    目标 8×8（2×2 倍）：
┌──┬──┬──┬──┐                     ┌──┬──┬──┬──┬──┬──┬──┬──┐
│A │B │C │D │                     │A │A │B │B │C │C │D │D │
├──┼──┼──┼──┤                     ├──┼──┼──┼──┼──┼──┼──┼──┤
│E │F │G │H │    每个源像素        │A │A │B │B │C │C │D │D │
├──┼──┼──┼──┤    →                ├──┼──┼──┼──┼──┼──┼──┼──┤
│I │J │K │L │    水平×2           │E │E │F │F │G │G │H │H │
├──┼──┼──┼──┤    垂直×2           ├──┼──┼──┼──┼──┼──┼──┼──┤
│M │N │O │P │                     │E │E │F │F │G │G │H │H │
└──┴──┴──┴──┘                     └──┴──┴──┴──┴──┴──┴──┴──┘
```

**效果**：锐利、像素感强——保持老式游戏的原始美感。许多复古游戏爱好者偏爱这种风格。

**实现**：两层循环——水平展开（逐像素 × xmul）+ 垂直复制（逐行 × ymul）。特殊快路径：当 xmul=1 且 src/dst 的 pitch 一致时，`copy_from_slice` 全局复制——等价于 `memcpy`，零加工。

```rust
let scaler = IntegerScaler::new(2, 2);
scaler.scale(&src_gba_frame, 240, 160, 240, &mut dst_frame, 480);
// 源 240×160 GBA 画面 → 480×320（2 倍锐利）
```

### AaScaler — 双线性插值

当源和目标是**非整数倍**关系时使用——例如 320×240 → 480×320（约 1.5×1.33 倍）。IntegerScaler 会产生"块状锯齿"——某些像素被复制 2 次而相邻像素只复制 1 次。AA 混合缩放用加权平均消除这种不平滑。

**核心思想**：每个输出像素不是直接复制某个源像素，而是按空间位置取周围 4 个源像素做加权平均。

```
源图像素：        A       B        C        D
                  │       │        │        │
                  0       1        2        3    (源坐标)

输出像素映射回源坐标（以 1.5 倍水平缩放为例）：
  ox=0  →  sx=0.0  →  权重全在 A         →  100% A
  ox=1  →  sx=0.6  →  60% 靠近 A, 40% 靠近 B →  混合值
  ox=2  →  sx=1.2  →  80% 靠近 B, 20% 靠近 C →  混合值
  ox=3  →  sx=1.8  →  20% 靠近 B, 80% 靠近 C →  混合值
  ox=4  →  sx=2.4  →  60% 靠近 C, 40% 靠近 D →  混合值
```

**算法流程**：

1. **构造时**：`gcd(src_w, dst_w)` → 宽高比的最简分数 `ratio_w_in / ratio_w_out`
2. **缩放时（逐输出像素）**：映射回源坐标 → 取分数坐标 → 上下左右 4 个邻近源像素加权→结果

**混合公式**：（固定分母 256，整数运算，零浮点）

```rust
fn blend_pixels(a: u16, b: u16, weight: u32) -> u16 {
    // 通道分离——RGB565 的 R/G/B 跨位边界，不能做 u16 整体混合
    let wa = weight;
    let wb = 256 - weight;
    let r = ((a>>11 as u32 * wa + b>>11 as u32 * wb) >> 8) as u16;
    let g = (((a>>5 & 0x3F) as u32 * wa + (b>>5 & 0x3F) as u32 * wb) >> 8) as u16;
    let b_ = ((a & 0x1F) as u32 * wa + (b & 0x1F) as u32 * wb) >> 8;
    (r << 11) | (g << 5) | b_ as u16
}
```

**为什么分母是 256 不是 255？** ARM 上 `>> 8` 是 `lsr #8`——1 周期。`/ 255` 需要乘法 + 调整——约 5 周期。±1 色阶差异在 3.5 寸掌机屏幕上人眼不可辨识（每个 RGB565 通道仅 32-64 级，±1 小于人眼的最小辨别差）。

**效果**：平滑过渡、无锯齿、无块状感。

```rust
// PS1 画面 320×240 全屏拉伸到 640×480
let scaler = AaScaler::new(320, 240, 640, 480);
scaler.scale(&src_ps1_frame, 320, 240, 320, &mut dst_frame, 640);
```

### 整数缩放 vs AA 混合 vs 双线性——对比

```
同一 GBA 游戏画面，缩放至同一尺寸：

IntegerScaler (3×):                 AaScaler (全屏拉伸):
████  ████  ████                    ███▓ ▓███  ▒███
████  ████  ████  锐利、            ▓███ ██▓  ▓█▓█  平滑、
████  ████  ████  马赛克感          ██▓▓ ▓▓██  █▓██  柔和
████  ████  ████                    ████ ██▓▓  ▓██▒

BilinearScaler (2×):
████  ████  ████
████  ████  ████  上采样平滑（四邻域插值）——与 AA 的下采样抗锯齿不同
████  ████  ████
████  ████  ████
```

**选用原则**：

| 场景 | 缩放器 |
|------|--------|
| 整数倍关系 + 想要锐利像素（crisp） | `IntegerScaler`（像素完美、零开销） |
| 非整数倍 + 想要平滑（soft / aspect 模式） | `BilinearScaler`（四邻域插值） |
| 下采样抗锯齿 | `AaScaler`（面积平均，minarch 目前不经过它） |

### BilinearScaler — 双线性插值缩放

**什么时候用？** 任意（分数）倍率的画面缩放——尤其模拟器帧在运行时
才知道分辨率、屏幕比例又对不上的场景（如 GBA 240×160 拉伸到
1280×720 的 aspect 模式）。它对应原 C tg5040 平台的 SOFT 锐度 =
SDL 纹理默认双线性过滤（`SDL_RenderCopy` 路径，platform.c:436）——
C 把双线性交给 GPU，Rust 在软件层重建。

**和 AaScaler 有什么区别？** 这是最容易混淆的一对：

- **AaScaler 是面积平均**（`scaleAA`，api.c:375-466）——为**下采样
  抗锯齿**设计，输出像素是源区域的加权平均，上采样时明显发糊
- **BilinearScaler 是四邻域插值**——为**上采样平滑**设计，每个目标
  像素反查源坐标、取周围 4 个像素按距离加权

两者不可互相替代（spec 明言）。

**算法**（中心对齐反查 + 固定分母 256）：

```rust
let scaler = BilinearScaler::new(160, 144, 1280, 720);
scaler.scale(&src_frame, 160, 144, 160, &mut dst_frame, 1280);
// 每个目标像素 (ox, oy)：
//   sx = (ox + 0.5) × 160 / 1280 - 0.5   ← 中心对齐
//   取 sx 左右两个源像素，按分数距离加权（水平）
//   再对上下两行结果按 sy 加权（垂直）
//   边缘钳制：反查落点超出源边缘时退化为边缘像素
```

- **中心对齐**：`(x+0.5)×sw/dw−0.5`，避免传统 `x×sw/dw` 的左上角
  偏移（放大时画面整体往左上偏半个像素）
- **固定分母 256**：`>> 8` 快除，与 `AaScaler` 同策略
- **1:1 恒等快路径**：权重退化为 100%，等价 memcpy
- **pitch 感知**：`sp`/`dp` 参数与 `IntegerScaler`/`AaScaler` 一致

```rust
// minarch 视频管线里的用法（锐度映射）：
// crisp + 整数模式 → IntegerScaler
// soft 或 aspect 模式 → BilinearScaler
```

### SIMD 科普——为什么手写 900 行汇编

NEON 是 ARM 处理器的 **SIMD**（Single Instruction Multiple Data，单指令多数据）指令集。SIMD 的核心思想是一条指令同时对多个数据执行相同操作：

```
普通循环（一次 1 个像素，8 条指令）：     NEON 指令（一次 8 个像素，1 条指令）：
p[0] += c;                              vadd.u16 q0, q0, q1
p[1] += c;
p[2] += c;                              8 个 u16 像素同时完成加法
p[3] += c;
p[4] += c;
p[5] += c;
p[6] += c;
p[7] += c;
```

**原 C 为什么需要手写 900 行 NEON 汇编？** 2010 年代的 ARM GCC 编译器没有自动向量化能力（auto-vectorization）。开发者只能手写 `vldmia`（加载多寄存器）、`vext.16`（16 位元素抽取重排）、`vstmia`（存储多寄存器）等内联汇编指令来让 400MHz 芯片跑满 60fps。

**Rust 为什么不需要？** 现代 LLVM 编译器（Rust 使用的后端）在 `--release` 下自动识别可向量化的循环模式：

```rust
// 你写标准 Rust 循环：
for chunk in src.chunks(8) {
    for (d, s) in dst.iter_mut().zip(chunk) {
        *d = *s;
    }
}

// LLVM 在 --release 下自动生成等效的 NEON：
// vldmia r0!, {q8}      (128-bit load, 8 pixels)
// vstmia r1!, {q8}      (128-bit store, 8 pixels)
```

这对所有 ARM 目标一视同仁——设备的 NEON 可用时 LLVM 自动启用，没有 NEON 时自动降级为标量路径。C 代码的 `#ifdef HAS_NEON` 条件编译变成了编译器自动决策。

### 常量传播——为什么 1 个 Rust 函数 = 60 个 C 函数

原 C 代码有 60+ 个如下形式的包装函数：

```c
void scale2x1_c16(...) { scale2x_c16(..., 1); }  // 仅传递 ymul=1
void scale2x2_c16(...) { scale2x_c16(..., 2); }  // 仅传递 ymul=2
void scale2x3_c16(...) { scale2x_c16(..., 3); }
void scale2x4_c16(...) { scale2x_c16(..., 4); }
// ... 60 个
```

C 需要这些包装是因为老编译器无法对 `ymul` 这个变量参数做常量折叠。把 `ymul` 硬编码为字面量常量写入函数签名——编译器才知道"这个值是 3"并据此展开循环。

**Rust/LLVM 的做法——常量传播（constant propagation）**。编译器追踪值的流向：

```
IntegerScaler::new(2, 3)    → self.xmul=2, self.ymul=3（常量）
    .scale(...)             → 在 scale() 体内：
        for _ in 0..self.ymul { ... }  // LLVM 看到 self.ymul=3 是常量
        // → 展开为 3 次 copy，无循环开销
```

这就是为什么 Rust 版用 1 个泛型循环替代 60 个手写包装函数——结果是相同的机器码。

### 原 C 与 Rust 对照

| 维度 | 原 C | Rust |
|------|------|------|
| 总代码量 | ~3000 行（`scaler.c` 2800 + `api.c` 200）| ~200 行 |
| 函数/结构体数量 | ~180 个 per-factor 函数 + 2 个 dispatch 函数 | 2 个结构体 |
| NEON/SIMD | ~900 行 ARM 内联汇编（`vldmia`/`vext.16`/`vstmia`） | 0 行（LLVM `--release` 自动向量化） |
| 格式转换 | 2 个 16→32bpp 专用函数 | 跳过（MinUI 只用 RGB565） |
| 混合算法 | 5 级离散（0%/25%/50%/75%/100%） | 256 级连续（固定分母 `>> 8`） |
| 混合状态 | `blend_args` + `blend_line` 全局变量 + `calloc`/`free` | `AaScaler` 私有字段 + 栈临时变量 |
| per-factor | 60+ 个函数（手工常量折叠） | 0 个（LLVM 自动常量传播） |
| grid/line | `scale*x*_grid` / `scale*x*_line` 独立函数 | `IntegerScaler` 特定参数（如 `xmul=2, ymul=2`） |
| 非 NEON 设备 | `#ifdef HAS_NEON` 编译时二选一 | 无特殊处理（LLVM 自动降级标量） |

## 布局常量与颜色体系

### 布局常量

所有 UI 布局常量定义在 `common::video` 模块——这是项目强制执行的**单一来源（single source of truth）原则**。

| 常量 | 值 | 原 C | 含义 |
|------|-----|------|------|
| `PILL_SIZE` | 30 | `defines.h:51` | 药丸 UI 的基准尺寸 |
| `BUTTON_SIZE` | 20 | `defines.h:52` | 按钮 sprite 尺寸 |
| `BUTTON_MARGIN` | 5 | `defines.h:53` | 按钮与文字间距 = (PILL_SIZE-BUTTON_SIZE)/2 |
| `BUTTON_PADDING` | 12 | `defines.h:54` | 按钮组内部填充 |
| `PADDING` | 10 | `defines.h:63` | 页面边缘留白 = PILL_SIZE/3 |
| `SETTINGS_SIZE` | 4 | `defines.h:55` | 亮度滑条高度 |
| `SETTINGS_WIDTH` | 80 | `defines.h:56` | 亮度滑条宽度 |
| `BRIGHTNESS_MIN` | 0 | `defines.h:8` | 亮度最小值 |
| `BRIGHTNESS_MAX` | 10 | `defines.h:9` | 亮度最大值 |
| `VOLUME_MIN` | 0 | `defines.h:6` | 音量最小值 |
| `VOLUME_MAX` | 20 | `defines.h:7` | 音量最大值 |

**为什么不能在业务 crate 中本地定义相同的常量？**

```rust
// ❌ 错误做法——存在两个 BUTTON_SIZE
// crates/render/src/button.rs:  const BUTTON_SIZE: u32 = 20;
// crates/minarch/src/menu.rs:   const BUTTON_SIZE: u32 = 20;

// 新增第三个使用点时，可能随意取一个值——如果后续修改了 render 中的值
// 而未同步 minarch 中的，两个 crate 有了不一致的布局，产生细微的 UI 偏移

// ✅ 正确做法——唯一来源
// crates/render/src/button.rs:  use common::video::BUTTON_SIZE;
// crates/minarch/src/menu.rs:   use common::video::BUTTON_SIZE;
```

**为什么 C 代码没有这个问题？** C 的 `#define` 在头文件中——`#include "defines.h"` 解决了单一来源。Rust 没有预处理器——常量通过模块系统的 `use` 导入来共享。如果每个 crate 各自定义 `const`，它们就是独立的值——编译器不保证它们相等。

### 颜色常量

| 常量 | RGB565 值 | 色彩 |
|------|----------|------|
| `RGB_WHITE` | `0xFFFF` | 纯白 (R=31, G=63, B=31) |
| `RGB_BLACK` | `0x0000` | 纯黑——`fill_rect` 的默认背景 |
| `RGB_LIGHT_GRAY` | `0xCE79` | 浅灰 |
| `RGB_GRAY` | `0x9CD3` | 中灰 |
| `RGB_DARK_GRAY` | `0x4208` | 深灰——状态栏 pill 背景 |

### ASSET_RGBS —— 单色素材的颜色映射

精灵图集中的单色素材（WhitePill、BlackPill…）只存储形状——颜色由 `ASSET_RGBS[asset as usize]` 在运行时用纯色填充覆盖。这比在精灵图集中存储 14 个不同颜色的单独精灵节省空间——一个形状（30×30 像素），多种颜色（白/黑/深灰/…），合并在一个像素上。

```rust
use common::video::{PILL_SIZE, BUTTON_SIZE, PADDING, RGB_WHITE};

// 右下角定位
let x = screen.width - PADDING * scale - PILL_SIZE * scale;
let y = screen.height - PADDING * scale - PILL_SIZE * scale;
blit_pill(&atlas, Asset::WhitePill, scale, &mut screen, Rect { x, y, w: 60, h: 60 });
```

## C → Rust 迁移对照表

| 原 C 函数 / 位置 | Rust 函数 | 文件 |
|-----------------|-----------|------|
| `GFX_init()` → `IMG_Load(asset_path)` `api.c:152` | `load_atlas(path)` | `asset.rs` |
| `SDL_BlitSurface(sprite_sheet, &srcrect, dst, &dstrect)` `api.c:518` | `blit_asset(atlas, asset, scale, dst, pos, src_rect)` | `asset.rs` |
| `GFX_blitPill(asset, dst, dst_rect)` `api.c:520-539` | `blit_pill(atlas, asset, scale, dst, dst_rect)` | `pill.rs` |
| `GFX_blitRect(asset, dst, dst_rect)` `api.c:540-558` | `blit_rect(atlas, asset, scale, dst, dst_rect)` | `pill.rs` |
| `SDL_FillRect(dst, rect, color)` | `VideoBuffer::fill_rect(rect, color)` | `video.rs` |
| `TTF_RenderUTF8_Blended(font, text, color)` | `render_text(dst, font, text, px, color, pos)` | `text.rs` |
| `TTF_SizeUTF8(font, text, &w, &h)` | `size_text(font, text, px, leading)` | `text.rs` |
| `GFX_truncateText(font, str, max_width)` | `truncate_text(font, text, max_width, px)` | `text.rs` |
| `GFX_wrapText(font, str, max_width)` | `wrap_text(font, text, max_width, px)` | `text.rs` |
| `GFX_blitMessage(font, text, dst, dst_rect)` | `blit_message(dst, font, text, px, dst_rect)` | `text.rs` |
| `GFX_blitText(font, text, dst, dst_rect)` | `blit_text(dst, font, text, px, color, dst_rect, leading)` | `text.rs` |
| `GFX_blitButton(hint, button, dst, pos)` `api.c:610` | `blit_button(hint, button, atlas, font, dst, x, y, px, scale)` | `button.rs` |
| `GFX_blitButtonGroup(pairs, dst, rect)` `api.c:634` | `blit_button_group(pairs, atlas, font, dst, pill_rect, align_right, px, scale)` | `button.rs` |
| `GFX_blitHardwareHints(dst, show_setting)` `api.c:778` | `blit_hardware_hints(show_setting, font, dst, x, y, px, scale)` | `button.rs` |
| `GFX_getButtonWidth(hint, button, font)` `api.c:590` | `get_button_width(hint, button, font, px, scale)` | `button.rs` |
| `GFX_blitBattery(dst, dst_rect)` `api.c:559-589` | `blit_battery(dst, atlas, pct, charging, pos, scale)` | `battery.rs` |
| `GFX_blitHardwareGroup(dst, show_setting)` `api.c:692-777` | `blit_hardware_group(atlas, status, scale, dst)` | `hardware.rs` |
| `IMG_Load(res_path)` (缩略图) `minui.c:1513` | `load_thumbnail(path)` | `thumbnail.rs` |
| `scale{xmul}x{ymul}_c16` `scaler.c:121-210` | `IntegerScaler::new(xmul, ymul).scale(...)` | `scaler.rs` |
| `scaleAA()` + `blend_args` `api.c:375-500` | `AaScaler::new(sw,sh,dw,dh).scale(...)` | `scaler.rs` |

## 测试策略

render crate 共有 60+ 个测试，覆盖全部 8 个模块。测试分为两类：

**像素精确测试（CI 可运行）**：asset、pill、battery、hardware、thumbnail、scaler。这些模块操作的是纯像素数据——不依赖字体文件，可在任何平台/CI 环境运行。

**视觉解析测试（需字体）**：text、button。需要系统安装的 TTF/OTF 字体文件。当前 text 测试查找 segoeui.ttf（Windows）、DejaVuSans.ttf（Linux）、或 Helvetica.ttc（macOS）——若均不存在则 panic。

模块间的测试独立性——修改 `pill.rs` 不需要运行 text 测试。这使得 CI 中的反馈粒度更细、编译更快。

TDD 流程遵循项目规范（config.yaml tasks 规则）：

```
红阶段 →  先写测试，定义预期行为（测试失败）
绿阶段 →  实现最少代码使测试通过
重构  →  优化代码，确保测试仍然通过
```

## 性能设计说明

### 热路径与边界裁剪

渲染函数均不做边界裁剪。在"热路径"（hot path）——每帧 16ms 预算内被调用数十次的代码路径——中，每个像素添加 `if col < dst.width && row < dst.height` 检查的累计开销远比一次性 rect 验证昂贵。调用方负责确保坐标合法。

### 避免双重写入

`load_thumbnail` 使用 `Vec::with_capacity(pixel_count)` + `push` 而非 `vec![0u16; pixel_count]`。后者先零初始化整个缓冲区（~800KB），然后再覆盖写入——两次内存写。`with_capacity` + `push` 只写一次。

### copy_from_slice 快路径

`IntegerScaler` 的 `scale()` 在 `xmul==1 && ymul==1 && sp==dp` 时走快路径——零加工的 `slice::copy_from_slice`，等同于 C 的 `memcpy`。无缩放 = 零开销。

### blend_pixels 固定分母 256

`AaScaler` 的加权混合使用 `>> 8` 右移而非 `/= 255` 除法。ARM 上 `lsr #8` 是 1 周期指令——`/= 255` 需要 5+ 周期（乘法 + 高位调整）。±1 色阶差异在 RGB565 的人眼不可辨识范围内。

### 通道分离混合的必要性

RGB565 的 R/G/B 跨位边界——对 u16 做整体算术会污染邻位通道。`0xF800`（纯红）+ `0x07E0`（纯绿）直接相加 = `0xFFE0`——绿色通道被红色进位污染。通道分离消除了这种跨位污染。

---

## 否决方案清单

以下是在 render 开发过程中**被否决的方案**及否决理由——遇到相似想法时先查此清单（防止 AI 后续生成作出错误决策）。本清单连同各章节的「为什么」小节共同构成 render 的设计决策记录（条目要素：结论/为什么/被否决方案/产生的问题）：正文散落处不再重复。

### 图集（define-asset-atlas / implement-render-asset）

| 方案 | 否决理由 |
|------|---------|
| `ASSET_RECTS` 存预缩放坐标（每平台编译时计算） | 需要为每个平台单独生成坐标数组，增加维护负担 |
| `scale` 存储到 `Atlas` 中（load_atlas 时就确定） | 同一个 Atlas 在不同平台可能需要不同 scale，且 `ASSET_RECTS` 不依赖平台——scale 由调用方传入 |
| 逐像素 alpha 混合（blit 默认开启） | 原 C 的 `SDL_BlitSurface` 也不做 alpha 混合（`api.c:1450` 先 `SDLX_SetAlpha(gfx.assets, 0, 0)` 关闭）——只有低电量覆盖层等特定场景才手动开启。Rust 保持同样策略 |

### 字体（fix-fontdue-dependency / implement-render-text）

| 方案 | 否决理由 |
|------|---------|
| `rusttype` | 已停止维护且 API 较旧 |
| `ab_glyph` | 与 fontdue 相比功能较少 |
| `ttf-parser` + `cosmic-text` | 过于重量级——MinUI 只需要简单的栅格化 |
| text.rs 内部 `lazy_static` 持有 `Font` | 字体路径和字号由调用方决定，不应硬编码在 render crate 中 |
| 预乘 alpha 查找表（256×32×64×32） | 表太庞大——每帧渲染几十个字的场景，逐像素计算完全可以接受 |
| 探索 fontdue `Layout` API 的正确用法 | 当前方案工作正常且更简单——若未来出现性能问题（长文本列表测量）再优化为 Layout（`size_text` 签名不变，属透明优化） |

### 其他（implement-render-pill）

| 方案 | 否决理由 |
|------|---------|
| `fill_rect` 用自由函数 | `VideoBuffer` 是 common 的公共类型，在其上提供方法是 Rust 惯用做法（参考 `Vec::fill`、`[T]::fill`） |
| `ASSET_RGBS` 放在 `render/pill.rs` | 颜色映射是数据定义，不是渲染逻辑——按原 C 结构（`asset_rgbs[]` 紧邻 `asset_rects[]`），两者应放同一位置（common::video） |

### render::image 模块增删史（平台自治边界教训）

render 曾短暂存在过 `image.rs`（`load_png`/`blit_buffer`）：当时 show 启动画面工具还在 render 内共享 PNG 解码，minui 与 show 两个消费方。随后 show 迁往平台自治 bin（`platforms/tg5040/show` 独立 crate，自实现 PNG 解码），render 的 image 消费方只剩 minui 一个——模块随即**删除**，`blit_buffer` 回滚为私有。

教训（记录防回归）：**render 是上层 UI 渲染库，不承载平台自治工具的原语**——某个函数只被"上层 UI + 平台自治工具"共用时不构成进 render 的理由；平台自治 bin 应自包含（show 自实现解码）。配套原则：**不用就删**——消费方只剩一个时，函数回归调用方私有，不为"可能复用"保留公共 API。

### 缩放器（implement-render-scaler / implement-minarch-core）

| 方案 | 否决理由 |
|------|---------|
| 双线性用 `AaScaler` 凑合 | 语义错误：AA 是面积平均（下采样抗锯齿设计），上采样时输出明显糊；spec 明言两者不可互替 |
| 引入 `image` 等图像库实现双线性 | 为一个纯 Rust 可手写的 60 行算法引入依赖，违背「非必要不增加复杂度」 |

---

## 常见问题 FAQ

### fontdue 为什么替换 rusttype？

rusttype 已停止维护且 API 较旧（详见「否决方案清单」字体组）。fontdue 功能齐全、维护活跃，且输出 alpha 位图 + 字形信息（`(&[u8], Metrics)`）——配合手动通道分离混合，比 SDL_ttf 的 surface 混合更可控（详见 [text 章节的差异表](#fontdue-与-sdl_ttf-的关键差异)）。

### 为什么默认不做 alpha 混合？

原 C 的 `SDL_BlitSurface` 默认也不做 alpha 混合（`api.c:1450` 先关闭 alpha）——精灵图集自带准确边缘，只有低电量覆盖层等特定场景需要混合（由上层按需开启）。Rust 保持同样策略，避免每帧全图混合的开销。

### IntegerScaler、AaScaler 和 BilinearScaler 怎么选？

**整数倍关系 + 想要锐利像素 → IntegerScaler**（像素完美、零开销）；
**非整数倍 + 想要平滑（soft / aspect 模式）→ BilinearScaler**（四邻域
插值，对应 tg5040 的 GPU 双线性）；**下采样抗锯齿 → AaScaler**（面积
平均）。GBA（240×160）在 480×320 屏幕是恰好 2 倍——IntegerScaler；
拉伸到非整数倍尺寸——BilinearScaler（详见 [scaler 章节的对比](#整数缩放-vs-aa-混合-vs-双线性对比)）。

### 为什么 minui 不需要双线性，而 minarch 需要？

minui（启动器）的素材（药丸、图标、文字）在**打包期**就已经按平台
`SCALE` 预缩放好了——`assets@2x.png` 是设计尺寸的整数倍，渲染时只需
`blit_asset` 按整数 `scale` 裁切复制，**没有运行时插值的需求**。
minarch（游戏前端）的"素材"是模拟器每帧输出的画面——分辨率在运行时
才可知（GBA 240×160、PS1 320×239……），屏幕比例又对不上，必须做
分数倍缩放。这就是双线性缩放器进 render 但只有 minarch 消费的原因。

### 为什么 ASSET_RECTS 存未缩放坐标？

C 版本用 `SCALE4()` 宏预乘 scale 存 `asset_rects[]`（每平台一份坐标表）；Rust 存原始 100×64 画布上的坐标，由 `blit_asset` 运行时 × scale——同一个坐标表可用于不同 scale 的平台，源数据统一、缩放由渲染层负责（`+ scale` 项补偿图集右侧像素间隙，与原 C 的 `+ FIXED_SCALE` 一致）。

### 为什么手写 900 行汇编（SIMD）？

`AaScaler` 的混合是像素级加权运算——ARM 的 NEON SIMD 指令（`vld1`/`vmlal`/`vst1` 等）能并行处理 8 个像素。900 行是"手写 + 逐指令注释"的教学式写法（本项目文档服务于 Rust 新手科普——见「性能设计说明」）。

**编译器为什么不自动向量化？** 两个原因叠加：一是时代背景——2010 年代的 ARM GCC 没有自动向量化能力（见「SIMD 科普」章节）；二是即便现代 LLVM，RGB565 位拆装（把每像素 16 位拆成 R/G/B 通道）也不是典型的标准循环形态，直接向量化困难。Rust 版的解法是让 LLVM 处理**通道分离后的逐通道简单循环**（`--release` 自动向量化 + 常量传播），源码零汇编、性能等价。

### 为什么 1 个 Rust 函数 = 60 个 C 函数？

C 版 `video_scale_bilinear` 按分辨率/颜色位深/混合选项写了几十个 `#ifdef` 变体（每个组合一个函数）；Rust 用泛型 + 常量参数在编译期特化——一个泛型函数生成全部变体，源码量大幅缩减（详见 [scaler 章节的常量传播](#常量传播为什么-1-个-rust-函数--60-个-c-函数)）。

### 单字符 vs 多字符按钮？

`blit_button` 对单字符（如 "A"）与多字符（如 "START"）分别处理——单字符按钮用 sprite 裁切（`ASSET_RECTS` 的字母图），多字符按钮用文字渲染 + pill 背景。测量（`get_button_width`）与绘制按此分支（详见 [button 章节](#单字符-vs-多字符按钮)）。

### 电池图标 src_rect 为什么按百分比裁切？

电池填充条的动态电量不是固定尺寸——`blit_asset` 的 `src_rect` 参数从 `BatteryFill` 精灵（12×6）按电量百分比裁切宽度（如 50% → 6×6），外壳精灵固定、填充条动态——最精巧的 `src_rect` 用例（详见 [battery 章节](#src_rect-按百分比裁切最精巧的用例)）。
