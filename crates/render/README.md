# render — MinUI 像素渲染层

`render` 是 MinUI 的渲染库：所有"往屏幕上画东西"的函数集中于此（文字、药丸按钮、电池图标、状态栏、缩略图、缩放器），直接操作 `common::video::VideoBuffer`（RGB565 纯像素缓冲）——不依赖 SDL，可在宿主环境测试。

## 模块定位与职责

- **为什么独立成 crate**：C 原版把渲染散在各模块直接调 SDL/GFX 全局函数（无边界、难测试）；Rust 版渲染与业务分离——调用方只依赖 `VideoBuffer` 纯像素语义
- **依赖**：`common`（类型/常量）+ `fontdue`（字体栅格化）+ `png`（图集解码）——不依赖平台 crate
- **八个模块**：`asset`（图集裁切）/`pill`（圆角药丸）/`text`（文字排版）/`button`（按钮栏）/`battery`（电池图标）/`hardware`（状态栏）/`thumbnail`（缩略图）/`scaler`（三种像素缩放器）

## 内部结构（子组件）

```
crates/render/src/
├── lib.rs            ← crate 根
├── asset.rs          ← Atlas 加载、blit_asset 裁切复制（ASSET_RECTS 未缩放坐标）
├── pill.rs           ← blit_pill（三段式）/ blit_rect（七段式）
├── text.rs           ← fontdue 栅格化、render_text/size_text/truncate/wrap/blit_message
├── button.rs         ← blit_button / blit_button_group / blit_hardware_hints
├── battery.rs        ← 充电/非充电双分支、src_rect 百分比裁切
├── hardware.rs       ← HardwareStatus、blit_hardware_group（返回 ow 宽度）
├── thumbnail.rs      ← load_thumbnail（PNG → Option<VideoBuffer>）
└── scaler.rs         ← IntegerScaler（最近邻）/ AaScaler（面积平均）/ BilinearScaler（双线性）
```

## 快速了解

- **纯像素、可测试**：全部渲染函数操作 `VideoBuffer`，headless 测试（无 SDL/窗口）
- **文字**：fontdue 栅格化 + 自实现混合/排版（替换 C 的 SDL_ttf）
- **缩放三选一**：整数倍最近邻（minui 素材）/ 面积平均（下采样抗锯齿）/ 双线性（minarch 模拟器画面分数倍缩放）
- **否决清单**：图集/字体/缩放器共 13+ 项被否决方案的记录在详页（防 AI 误引入）

## 详细文档

完整文档（核心概念、逐模块解析、性能设计、否决方案清单、FAQ、C → Rust 对照）见 [docs/modules/render.md](../../docs/modules/render.md)。
