//! # MinUI — 统一启动入口
//!
//! 本文件是 MinUI 启动器的 `main()` 函数。通过编译时 Cargo feature 选择目标平台，
//! 所有平台的启动路径完全一致——与 C 原版 `minui.c` 的 `main()` 一样，主函数不
//! 感知平台差异。
//!
//! ## 对应原 C 代码
//!
//! ```makefile
//! # workspace/all/minui/makefile
//! SOURCE = minui.c ../common/scaler.c ../common/utils.c ../common/api.c \
//!          ../../$(PLATFORM)/platform/platform.c
//! CFLAGS += -DPLATFORM=\"$(PLATFORM)\"
//! ```
//!
//! Rust 等价物：
//! ```bash
//! cargo run -p minui --features platform-pc      # PC 桌面测试
//! cargo build --features platform-tg5040 --target armv7-... # 真机
//! ```

use platform::Platform;
use crate::MinUi;
use render::{UiRenderer, FontManager, Mode};
use power::PowerManager;

// ── 编译时平台选择：类型别名 ─────────────────────────────────────
//
// 对应 C 的 `make PLATFORM=tg5040`，在 Rust 中用 `--features platform-tg5040`
// 激活。`CurrentPlatform` 类型别名使得所有平台能零差异地从关联常量读取
// SDCARD_PATH、PLATFORM_TAG 等信息，同时保证构造函数签名统一（均无参）。

#[cfg(feature = "platform-pc")]
type CurrentPlatform = platform_pc::PcPlatform;

#[cfg(feature = "platform-tg5040")]
type CurrentPlatform = platform_tg5040::Tg5040;

#[cfg(not(any(
    feature = "platform-pc",
    feature = "platform-tg5040",
)))]
compile_error!("No platform feature selected. Use --features platform-<name>");

fn main() {
    // 初始化日志
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("MinUI starting...");

    // 1. 创建平台实例（编译时选择，所有平台 new() 均无参）
    let mut platform = CurrentPlatform::new();
    let sdcard = CurrentPlatform::SDCARD_PATH;
    let platform_tag = CurrentPlatform::PLATFORM_TAG;

    // 2. 加载字体
    //
    // include_bytes! 在编译时将整个字体文件嵌入二进制（~200KB）。
    // 这与 C 原版不同——C 原版通过 SDL_ttf 从 SD 卡文件加载字体。
    // 此处简化实现，所有平台均嵌入字体；后续可改为从 SD 卡路径读取。
    let font_data = include_bytes!("../../resources/BPreplayBold-unhinted.otf");
    let font_manager = FontManager::new(font_data, 2.0);
    let renderer = UiRenderer {
        font_manager,
        mode: Mode::Main,
        main_row_count: 6,
        scale: 2,
        screen_w: 640,
        screen_h: 480,
    };

    // 3. 创建电源管理器
    let mut power = PowerManager::new();

    // 4. 创建 MinUI 状态机
    let mut minui = MinUi::new();

    // 5. 启动主循环
    //
    // 对应 C 原版 `workspace/all/minui/minui.c` 的 `main()`：
    //
    //     screen = GFX_init(MODE_MAIN);   // → PLAT_initVideo()
    //     PAD_init();                     // → PLAT_initInput()
    //     // ... 进入主循环 ...
    //
    let paks = format!("{}/.system/{}/paks", sdcard, platform_tag);

    log::info!("Platform: {}  SD: {}  Paks: {}", platform_tag, sdcard, paks);

    match minui.run(&mut platform, &renderer, &mut power, sdcard, platform_tag, &paks) {
        Ok(true) => log::info!("MinUI exited normally (game launched)"),
        Ok(false) => log::info!("MinUI auto-resumed — no UI needed"),
        Err(e) => log::error!("MinUI error: {}", e),
    }
}
