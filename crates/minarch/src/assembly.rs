//! 装配层纯逻辑（可测部分，implement-minarch-main）
//!
//! 本模块承载装配层中**可脱离平台与主循环测试**的纯逻辑：存档序列化
//! 编排、resume 槽位解析、快捷指令动作消费的纯函数部分。对应原 C
//! `minarch.c` 的 `State_save`/`State_load`/`State_resume`（:569-583）
//! 与 `input_poll_callback` 的快捷动作 switch（:1683-1734）。
//!
//! 边界：本模块不接触 `Platform`/`PAD`/SDL——平台值（`poll_input`/
//! `now_ms` 等）由 `main.rs` 装配层收集后传入。`main.rs` 是薄胶水，
//! 本模块是可测逻辑。
//!

use std::path::Path;

use crate::audio;
use crate::core;
use crate::game::Game;
use crate::libretro::Core;
use crate::menu::SaveStateIo;
use crate::savestate;

/// 快进标志当前状态（core 静态为唯一事实源；装配层维护的同步快照）
///
/// 供 `save_state`/`load_state` 的快进门控使用——调用方传入
/// `core::fast_forward()` 的当前值即可。
type FastForwardFlag = bool;

/// 存档序列化编排（对应 C `Menu_saveState` 的序列化段，minarch.c:4135-4155）
///
/// `serialize_size` → 分配 → `serialize` → `savestate::write_from`；
/// 快进门控（关→执行→恢复，core 静态为唯一事实源）。
///
/// # 参数
///
/// - `core`: 核心（`serialize_size`/`serialize` 符号）
/// - `game`: 游戏（`name` 用于快照路径）
/// - `slot`: 槽位号（0-7 手动 / 9 自动存档，模块不感知语义）
/// - `save_io`: 菜单存档交互（slot 记忆 + 多碟 `.txt` 写入）
/// - `was_ff`: 序列化前的快进状态（调用方从 `core::fast_forward()` 读取）
///
/// # 返回值
///
/// - `Ok(())`: 序列化成功或核心 `serialize` 返回 false（视为成功——C
///   静默，Rust 不中断）；slot 记忆写入失败也静默（与 C `putFile`
///   忽略一致）
/// - `Err(io::Error)`: 快照文件写入失败
pub fn save_state(
    core: &Core,
    game: &Game,
    slot: usize,
    save_io: &SaveStateIo,
    was_ff: FastForwardFlag,
) -> std::io::Result<()> {
    let _ = save_io.store_slot(slot);
    let _ = save_io.store_disc(slot, 0);
    if was_ff {
        core::set_fast_forward(false);
        audio::set_fast_forward(false);
    }
    let size = unsafe { (core.serialize_size)() };
    let mut buf = vec![0u8; size];
    let ok = unsafe { (core.serialize)(buf.as_mut_ptr() as *mut _, size) };
    if !ok {
        if was_ff {
            core::set_fast_forward(true);
            audio::set_fast_forward(true);
        }
        return Ok(());
    }
    let path = savestate::state_path(&save_io.states_dir, &game.name, slot as i32);
    // C 版写 `state_size` 字节（serialize 成功即填满缓冲；`serialize`
    // 返回 bool 而非长度——全量写 size 字节，对应 C `fwrite(state, 1,
    // state_size, ...)`，minarch.c:556）
    let result = savestate::write_from(&buf, Path::new(&path));
    if was_ff {
        core::set_fast_forward(true);
        audio::set_fast_forward(true);
    }
    result
}

/// 读档反序列化编排（对应 C `Menu_loadState`，minarch.c:4157-4176）
///
/// `save_exists` 门控 → 换碟请求检测 → 快进门控 → `read_into` →
/// `unserialize` → 恢复快进。
///
/// # 参数
///
/// - `core`: 核心（`serialize_size`/`unserialize` 符号）
/// - `game`: 游戏（`name` 用于快照路径）
/// - `slot`: 槽位号
/// - `save_io`: 菜单存档交互（存在性查询 + 换碟检测）
/// - `was_ff`: 反序列化前的快进状态
///
/// # 返回值
///
/// - `Ok(())`: 读档成功、无存档（空操作）或 `unserialize` 返回 false
///   （C 静默，Rust 不中断）
/// - `Err(io::Error)`: 快照文件读取失败
pub fn load_state(
    core: &Core,
    game: &Game,
    slot: usize,
    save_io: &SaveStateIo,
    was_ff: FastForwardFlag,
) -> std::io::Result<()> {
    if !save_io.save_exists(slot) {
        return Ok(());
    }
    // 换碟请求由装配层（menu_loop）在调用本函数前处理（C :4164-4176）
    if was_ff {
        core::set_fast_forward(false);
        audio::set_fast_forward(false);
    }
    let size = unsafe { (core.serialize_size)() };
    let path = savestate::state_path(&save_io.states_dir, &game.name, slot as i32);
    let mut buf = vec![0u8; size];
    let outcome = savestate::read_into(&mut buf, Path::new(&path))?;
    let loaded = match outcome {
        savestate::ReadOutcome::Loaded => unsafe {
            (core.unserialize)(buf.as_ptr() as *const _, size)
        },
        _ => false,
    };
    if was_ff {
        core::set_fast_forward(true);
        audio::set_fast_forward(true);
    }
    let _ = loaded;
    Ok(())
}

/// 读取并删除 resume 槽位标记（对应 C `State_resume`，minarch.c:575-583）
///
/// # 参数
///
/// - `resume_path`: `RESUME_SLOT_PATH`（参数化便于测试）
///
/// # 返回值
///
/// 槽位号：无文件/内容 `8`/解析失败 → `AUTO_RESUME_SLOT`（9）
pub fn read_resume_slot(resume_path: &str) -> i32 {
    let content = std::fs::read_to_string(resume_path).ok();
    let _ = std::fs::remove_file(resume_path);
    match content.and_then(|c| c.trim().parse::<i32>().ok()) {
        Some(8) => common::paths::AUTO_RESUME_SLOT,
        Some(v) if v > 0 => v,
        _ => common::paths::AUTO_RESUME_SLOT,
    }
}

// ═══════════════════════════════════════════════════════════════
// 线程模式切换状态机（implement-minarch-thread-video）
//
// 对应 C 的 `thread_video`/`was_threaded`/`toggle_thread` 三个全局
// （minarch.c:28-29/971）与 `setFastForward`/电源键分支/`Config_syncFrontend`
// FE_OPT_THREAD 的置位逻辑（minarch.c:1637-1681/:1002-1006）。
// 纯逻辑层：装配层（main.rs）持实例并调用，切换执行（自退出 → join →
// 重建）归装配层主循环。
// ═══════════════════════════════════════════════════════════════

/// 线程模式切换状态（对应 C `thread_video`/`was_threaded`/`toggle_thread`）
///
/// 三个状态字段的可达组合（design 决策 3）：
///
/// - A 态 `(thread_mode=false, was_threaded=false)`：单线程，无记忆
/// - B 态 `(thread_mode=false, was_threaded=true)`：单线程 + 记忆
///   （快进/电源键打断中）——瞬时，至多跨一次 toggle 处理
/// - C 态 `(thread_mode=true, was_threaded=false)`：线程化
///
/// `(thread_mode=true, was_threaded=true)` 不可达——toggle 处理在主循环
/// 帧内原子完成，B 态在被处理前不会叠加新事件。
pub struct ThreadToggleState {
    /// 当前是否线程化（对应 C `thread_video`）
    pub thread_mode: bool,
    /// 快进/电源键打断前的线程模式记忆（对应 C `was_threaded`）
    pub was_threaded: bool,
    /// 帧边界切换请求（对应 C `toggle_thread`；装配层消费后清零）
    pub toggle_thread: bool,
}

impl ThreadToggleState {
    /// 构造初始状态
    ///
    /// # 参数
    ///
    /// - `thread_mode`: 启动时的线程模式（cfg `minarch_thread_video` 决定）
    pub fn new(thread_mode: bool) -> Self {
        Self {
            thread_mode,
            was_threaded: false,
            toggle_thread: false,
        }
    }

    /// 快进状态变更（对应 C `setFastForward`，minarch.c:1637-1650）
    ///
    /// - 进入快进且当前线程化 → `was_threaded=true, toggle=true`
    ///   （快进时切回单线程，避免核心线程空转浪费）
    /// - 退出快进且当前单线程且 `was_threaded` → `was_threaded=false,
    ///   toggle=true`（恢复快进前的线程模式）
    /// - 其余情况不触发切换（A 态进出快进、B 态再进快进等）
    ///
    /// 快进标志本身不归本状态机持有（core 静态为唯一事实源，装配层
    /// 同步 core/audio）——本函数只负责线程切换触发。
    ///
    /// # 参数
    ///
    /// - `was_ff`: 变更前的快进状态（调用方从 `core::fast_forward()` 读）
    /// - `enable`: 快进目标状态
    pub fn on_fast_forward_change(&mut self, was_ff: bool, enable: bool) {
        if !was_ff && enable && self.thread_mode {
            // C :1638-1641：线程化中进入快进 → 记忆并请求切回单线程
            self.was_threaded = true;
            self.toggle_thread = true;
        } else if was_ff && !enable && !self.thread_mode && self.was_threaded {
            // C :1643-1647：单线程中退出快进且之前线程化 → 恢复线程
            self.was_threaded = false;
            self.toggle_thread = true;
        }
    }

    /// 电源键按下/释放（对应 C :1668-1681）
    ///
    /// - 按下且当前线程化 → `was_threaded=true, toggle=true`（睡眠需要
    ///   确定时序，先切回单线程）
    /// - 释放且当前单线程且 `was_threaded` → `was_threaded=false,
    ///   toggle=true`（恢复线程化）
    ///
    /// # 参数
    ///
    /// - `pressed`: 本帧电源键是否刚按下（`input.just_pressed`）；释放
    ///   判定由调用方传 false 且本函数只处理 `was_threaded` 恢复
    pub fn on_power_button(&mut self, pressed: bool) {
        if pressed {
            // C :1669-1672：线程化中按下电源键
            if self.thread_mode {
                self.was_threaded = true;
                self.toggle_thread = true;
            }
        } else if !self.thread_mode && self.was_threaded {
            // C :1676-1679：单线程中释放电源键且之前线程化 → 恢复
            self.was_threaded = false;
            self.toggle_thread = true;
        }
    }

    /// 菜单选项 `minarch_thread_video` 变更（对应 C `Config_syncFrontend`
    /// FE_OPT_THREAD，minarch.c:1002-1006）
    ///
    /// `old = thread_mode || was_threaded`、`toggle = old != value`。
    /// B 态设 OFF 时 `old == true`、`value == false` → 请求 toggle——
    /// 但装配层切换块的特判（`was_threaded && !thread_mode` 清标志、
    /// 不翻转模式）使净效果为"清 was_threaded、保持单线程"，即 C
    /// :4775-4780 的"双翻转抵消"语义（design 决策 3 简化）。
    ///
    /// # 参数
    ///
    /// - `value`: 选项目标值（`true` = ON / 线程化）
    pub fn on_option_change(&mut self, value: bool) {
        let old = self.thread_mode || self.was_threaded;
        self.toggle_thread = old != value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_slot_parsing_states() {
        // 无文件 → AUTO_RESUME_SLOT
        let missing = "/nonexistent/resume_slot.txt";
        assert_eq!(read_resume_slot(missing), common::paths::AUTO_RESUME_SLOT);

        // 内容 3 → 3（文件被删除）
        let dir =
            std::env::temp_dir().join(format!("minarch_assembly_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("resume_slot.txt");
        std::fs::write(&path, "3\n").unwrap();
        assert_eq!(read_resume_slot(path.to_str().unwrap()), 3);
        assert!(!path.exists(), "读后应删除");

        // 内容 8 → AUTO_RESUME_SLOT（slot 8 归一）
        std::fs::write(&path, "8\n").unwrap();
        assert_eq!(
            read_resume_slot(path.to_str().unwrap()),
            common::paths::AUTO_RESUME_SLOT
        );

        // 解析失败 → AUTO_RESUME_SLOT
        std::fs::write(&path, "garbage\n").unwrap();
        assert_eq!(
            read_resume_slot(path.to_str().unwrap()),
            common::paths::AUTO_RESUME_SLOT
        );
    }

    // ── ThreadToggleState（线程模式切换状态机）──

    #[test]
    fn toggle_state_initial() {
        // 单线程启动：三字段默认
        let st = ThreadToggleState::new(false);
        assert!(!st.thread_mode);
        assert!(!st.was_threaded);
        assert!(!st.toggle_thread, "toggle 标志默认 false");

        // 线程化启动
        let st = ThreadToggleState::new(true);
        assert!(st.thread_mode);
        assert!(!st.was_threaded);
        assert!(!st.toggle_thread);
    }

    #[test]
    fn toggle_fast_forward_c_to_b_to_c() {
        // C 态（线程化）进快进 → B 态
        let mut st = ThreadToggleState::new(true);
        st.on_fast_forward_change(false, true); // 未快进 → 进快进
        assert!(st.was_threaded, "线程化中进快进应记忆");
        assert!(st.toggle_thread, "应请求切回单线程");
        assert!(st.thread_mode, "模式翻转由切换块执行，本函数不翻转");

        // 模拟切换块执行：B 态（单线程 + 记忆）
        st.thread_mode = false;
        st.toggle_thread = false;

        // B 态退快进 → C 态（恢复线程化）
        st.on_fast_forward_change(true, false); // 快进中 → 退快进
        assert!(!st.was_threaded, "恢复线程化后清记忆");
        assert!(st.toggle_thread, "应请求恢复线程化");
    }

    #[test]
    fn toggle_fast_forward_a_state_no_toggle() {
        // A 态（单线程无记忆）进出快进均不触发切换
        let mut st = ThreadToggleState::new(false);
        st.on_fast_forward_change(false, true); // 进快进
        assert!(!st.was_threaded);
        assert!(!st.toggle_thread, "A 态进快进不应触发切换");

        st.on_fast_forward_change(true, false); // 退快进
        assert!(!st.was_threaded);
        assert!(!st.toggle_thread, "A 态退快进不应触发切换");
    }

    #[test]
    fn toggle_power_button_states() {
        // C 态按下 → B 态（记忆 + 请求切换）
        let mut st = ThreadToggleState::new(true);
        st.on_power_button(true);
        assert!(st.was_threaded);
        assert!(st.toggle_thread);

        // 模拟切换块执行
        st.thread_mode = false;
        st.toggle_thread = false;

        // B 态释放 → C 态（恢复）
        st.on_power_button(false);
        assert!(!st.was_threaded);
        assert!(st.toggle_thread);

        // A 态按下不触发（单线程无需切换）
        let mut st = ThreadToggleState::new(false);
        st.on_power_button(true);
        assert!(!st.was_threaded);
        assert!(!st.toggle_thread, "A 态按下电源键不应触发切换");
    }

    #[test]
    fn toggle_option_change() {
        // A 态设 ON → toggle（old=false != true）
        let mut st = ThreadToggleState::new(false);
        st.on_option_change(true);
        assert!(st.toggle_thread, "A 态设 ON 应请求切换");

        // C 态设 OFF → toggle（old=true != false）
        let mut st = ThreadToggleState::new(true);
        st.on_option_change(false);
        assert!(st.toggle_thread, "C 态设 OFF 应请求切换");

        // C 态设 ON（值回切）→ toggle（old=true != true 不触发；但 old
        // 计算含 was_threaded，C 态 was=false 时 old=true——设 ON 不触发）
        st.toggle_thread = false;
        st.on_option_change(true);
        assert!(!st.toggle_thread, "C 态设 ON（保持线程化）不应触发");

        // B 态设 OFF → 请求 toggle（切换块特判清 was、不翻转——净效果
        // 为清标志保持单线程，对应 C :4775-4780 双翻转抵消）
        let mut st = ThreadToggleState::new(false);
        st.was_threaded = true;
        st.on_option_change(false);
        assert!(
            st.toggle_thread,
            "B 态设 OFF 应请求 toggle（由切换块清标志）"
        );
    }
}
