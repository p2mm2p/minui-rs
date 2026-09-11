//! vibration.rs 振动引擎测试
//!
//! 覆盖 spec「Vibration 引擎状态机」「vibration 薄静态层」两个
//! Requirement 的全部 Scenario。
//!
//! 分层测试策略：
//! - `Vibration` 状态机用本地实例构造——纯逻辑、无静态，各用例互不
//!   干扰，可并行执行
//! - 薄静态层（`VIBRATION`）与 rumble hook（`RUMBLE_HOOK`）是进程级
//!   OnceLock，注册后无法撤销——生命周期用例合并为**单个测试串行
//!   执行**（同 tests/environment.rs 的静态薄层决策）

use minarch::vibration::Vibration;

// ── 状态机：排队与去抖全转移 ─────────────────────────────────────

#[test]
fn initial_state_and_duplicate_strength_ignored() {
    let mut vib = Vibration::new();
    // 初始：空闲，无输出
    assert_eq!(vib.get_strength(), 0);
    assert_eq!(vib.tick(), None);

    vib.set_strength(128);
    assert_eq!(vib.tick(), Some(128));
    // 重复同值：忽略（C `if (queued_strength==strength) return`）
    vib.set_strength(128);
    assert_eq!(vib.tick(), None);
}

#[test]
fn nonzero_applies_immediately() {
    let mut vib = Vibration::new();
    vib.set_strength(200);
    assert_eq!(vib.tick(), Some(200));
    assert_eq!(vib.get_strength(), 200);
}

#[test]
fn zero_defers_three_frames() {
    let mut vib = Vibration::new();
    vib.set_strength(100);
    assert_eq!(vib.tick(), Some(100));

    vib.set_strength(0);
    // 前 3 帧延迟关断（defer 1→2→3），马达仍震
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.get_strength(), 100);
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.get_strength(), 100);
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.get_strength(), 100);
    // 第 4 帧关断
    assert_eq!(vib.tick(), Some(0));
    assert_eq!(vib.get_strength(), 0);
}

#[test]
fn zero_defer_interrupted_by_nonzero() {
    let mut vib = Vibration::new();
    vib.set_strength(100);
    assert_eq!(vib.tick(), Some(100));

    vib.set_strength(0);
    assert_eq!(vib.tick(), None); // defer == 1

    vib.set_strength(50); // 去抖期间新非零到来
    assert_eq!(vib.tick(), Some(50)); // 立即应用，defer 清零
    assert_eq!(vib.get_strength(), 50);
}

#[test]
fn idle_returns_none() {
    let mut vib = Vibration::new();
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.get_strength(), 0);
}

#[test]
fn strength_domain_boundaries() {
    let mut vib = Vibration::new();
    vib.set_strength(255);
    assert_eq!(vib.tick(), Some(255));
    assert_eq!(vib.get_strength(), 255);

    vib.set_strength(0);
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.tick(), None);
    assert_eq!(vib.tick(), Some(0));
    assert_eq!(vib.get_strength(), 0);
}

#[test]
fn suspend_saves_current_and_clears_all() {
    let mut vib = Vibration::new();
    vib.set_strength(120);
    assert_eq!(vib.tick(), Some(120));

    // 睡眠前：保存 current 并确定性清零三者
    assert_eq!(vib.suspend(), 120);
    assert_eq!(vib.get_strength(), 0);
    assert_eq!(vib.tick(), None);
}

#[test]
fn resume_restores_nonzero_only() {
    let mut vib = Vibration::new();
    vib.set_strength(120);
    assert_eq!(vib.tick(), Some(120));
    assert_eq!(vib.suspend(), 120);

    // 唤醒后：非零恢复排队
    vib.resume(120);
    assert_eq!(vib.tick(), Some(120));

    // resume(0)：无操作（引擎保持全零）
    let mut idle = Vibration::new();
    idle.resume(0);
    assert_eq!(idle.tick(), None);
    assert_eq!(idle.get_strength(), 0);
}

// ── 薄静态层：生命周期（单测试串行）────────────────────────────
//
// VIBRATION/RUMBLE_HOOK 是进程级 OnceLock，注册后无法撤销——
// 全部静态层用例（含全链路串联）SHALL 在同一测试函数内串联执行
// （同 tests/environment.rs 的静态薄层决策）。

#[test]
fn static_layer_lifecycle() {
    use minarch::vibration;

    // ── 未 init：安全默认，SHALL NOT panic ──
    vibration::set_strength(100); // 空操作
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::suspend(), 0);
    vibration::resume(100); // 空操作
    assert_eq!(vibration::tick(), None);

    // ── init 生命周期：首次 Ok，重复 Err 携带原值 ──
    assert!(vibration::init().is_ok());
    let second = vibration::init().expect_err("重复注册应返回 Err");
    assert_eq!(second.get_strength(), 0); // 携带的是第二次传入的引擎

    // ── init 后：全局转发与实例等价 ──
    vibration::set_strength(30);
    assert_eq!(vibration::tick(), Some(30));
    assert_eq!(vibration::tick(), None);

    // ── 全局 suspend/resume 往返 ──
    vibration::set_strength(90);
    assert_eq!(vibration::tick(), Some(90));
    assert_eq!(vibration::suspend(), 90);
    assert_eq!(vibration::tick(), None);
    vibration::resume(90);
    assert_eq!(vibration::tick(), Some(90));
    // 收敛归零：4 帧关断
    vibration::set_strength(0);
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::tick(), Some(0));

    // ── 全链路串联：trampoline → hook → 引擎 → tick ──
    // 本 change 内三环齐备，这是「核心回调 → 马达」的唯一可测通路
    // （无需 mock 平台：apply 发生在主循环，本测试止于 tick 输出）。
    use minarch::environment::{rumble_trampoline, set_rumble_hook};
    use minarch::libretro::RetroRumbleEffect;

    assert!(set_rumble_hook(vibration::set_strength).is_ok());

    // 核心请求强振动 65535 → u16→u8 截断为 255 → 引擎排队 → tick 输出
    unsafe {
        assert!(rumble_trampoline(0, RetroRumbleEffect::Strong, 65535));
    }
    assert_eq!(vibration::tick(), Some(255));

    // 核心请求关断 → 4 帧延迟后输出 0
    unsafe {
        assert!(rumble_trampoline(0, RetroRumbleEffect::Strong, 0));
    }
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::tick(), None);
    assert_eq!(vibration::tick(), Some(0));
}
