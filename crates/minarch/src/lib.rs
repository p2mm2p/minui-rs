//! # MinArch — 模拟器前端
//!
//! MinUI 的 libretro 宿主程序。
//! 通过 `/tmp/next` 文件与 minui 启动器通信。
//!
//! ## 架构
//!
//! ```text
//! main.rs         ← 入口，解析命令行参数
//! lib.rs          ← run() 主函数
//! core.rs         ← EmuCore trait + LibretroCore 实现（dlopen）
//! game.rs         ← ROM 加载 / ZIP 解压 / M3U 解析
//! config.rs       ← 三级配置系统 (system/default/user)
//! input.rs        ← 输入映射 + 快捷键
//! video.rs        ← 缩放器 + 画面效果
//! audio.rs        ← 环形缓冲区 + 重采样
//! save.rs         ← SRAM / RTC / State 管理
//! menu.rs         ← 游戏内菜单 (Continue/Save/Load/Options/Quit)
//! main_loop.rs    ← 主循环 + 线程模式 + FPS 统计
//! ```

pub mod core;
pub mod game;
pub mod config;
pub mod input;
pub mod video;
pub mod audio;
pub mod save;
pub mod menu;
pub mod main_loop;

use platform::Platform;
use power::PowerManager;

/// minarch 的主入口
///
/// - `platform`: 平台实现（视频、输入、音频、电源等硬件抽象）
/// - `core_path`: libretro 核心 .so 文件路径
/// - `rom_path`: ROM 文件路径
/// - `font_data`: 字体文件的字节数据
/// - `power`: 电源管理器
pub fn run(
    platform: &mut impl Platform,
    core_path: &str,
    rom_path: &str,
    font_data: &[u8],
    power: &mut PowerManager,
) -> Result<(), String> {
    log::info!("MinArch starting...");
    log::info!("  core: {}", core_path);
    log::info!("  rom:  {}", rom_path);

    // TODO: 实际的模拟器运行逻辑
    // 1. Core::open(core_path)
    // 2. Game::open(rom_path)
    // 3. Config::load()
    // 4. Core::init() + Core::load_game()
    // 5. SND::init()
    // 6. main_loop::run()

    let _ = platform;
    let _ = font_data;
    let _ = power;
    Ok(())
}
