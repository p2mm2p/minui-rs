//! # Main Loop — 主循环 + 线程模式 + FPS 统计
//!
//! 对应原 C 代码 `main()` 中的 while 循环。

use common::types::PadContext;
use platform::Platform;
use power::PowerManager;

use crate::core::EmuCore;
use crate::menu::GameMenu;

/// 主循环运行结果
pub enum LoopResult {
    /// 正常退出（回到启动器）
    Quit,
    /// 请求关机
    PowerOff,
    /// 错误
    Error(String),
}

/// 运行模拟器主循环
///
/// 对应原 C 中 `main()` 的 `while (!quit)` 循环。
pub fn run_loop(
    _platform: &mut impl Platform,
    _core: &mut impl EmuCore,
    _power: &mut PowerManager,
    _menu: &mut GameMenu,
    _pad: &mut PadContext,
) -> LoopResult {
    // TODO: 实现完整的模拟器主循环
    //
    // 伪代码：
    //
    // while !quit && !power.poweroff_requested {
    //     // 1. 轮询输入
    //     platform.poll_input(pad);
    //
    //     // 2. 核心运行一帧
    //     core.run(&mut callbacks);
    //
    //     // 3. 电源更新
    //     power.update(dt_ms);
    //
    //     // 4. 快捷键处理
    //     if let Some(shortcut) = input_mapper.poll(pad) {
    //         match shortcut {
    //             Shortcut::SaveState => save_state(),
    //             Shortcut::LoadState => load_state(),
    //             Shortcut::ToggleFF => toggle_fast_forward(),
    //             ...
    //         }
    //     }
    //
    //     // 5. MENU 键 → 打开游戏内菜单
    //     if pad.just_released.contains(Button::MENU) {
    //         menu.run();
    //     }
    //
    //     // 6. HDMI 检测
    //     if platform.hdmi_changed() { ... }
    //
    //     // 7. FPS 统计
    //     track_fps();
    //     limit_fast_forward();
    // }
    //
    // // 退出时保存 SRAM + RTC
    // save_manager.write_sram(...);
    // save_manager.write_rtc(...);

    LoopResult::Quit
}
