//! hdmi.rs HDMI 热插拔检测状态机测试
//!
//! 覆盖 spec「HDMI 热插拔检测」Requirement 的全部 Scenario。
//!
//! 分层测试策略：纯逻辑状态机——每个测试构造本地 `HdmiMonitor`
//! 实例，不依赖任何静态/平台/mock，各用例可并行执行。

use minarch::hdmi::{HdmiChange, HdmiMonitor};

// ── 首次采样不触发 ─────────────────────────────────────────────

#[test]
fn first_sample_does_not_trigger() {
    // 首次采样只记录状态（对应 C `had_hdmi==-1` 初始化分支），
    // 不视为变化——进程启动时 HDMI 可能已连接，不应误判为热插拔
    let mut monitor = HdmiMonitor::new();
    assert_eq!(monitor.update(false), None);
}

#[test]
fn first_sample_records_true_without_triggering() {
    // 首次采样为 true（开机即插着 HDMI）同样不触发
    let mut monitor = HdmiMonitor::new();
    assert_eq!(monitor.update(true), None);
}

// ── 无变化不触发 ───────────────────────────────────────────────

#[test]
fn no_change_returns_none() {
    let mut monitor = HdmiMonitor::new();
    monitor.update(false);
    // 状态不变：连续两次 false 均不触发
    assert_eq!(monitor.update(false), None);
    assert_eq!(monitor.update(false), None);
}

#[test]
fn steady_connected_returns_none() {
    let mut monitor = HdmiMonitor::new();
    monitor.update(true);
    // 持续连接：连续 true 不触发（对应 C `has_hdmi != had_hdmi` 为假）
    assert_eq!(monitor.update(true), None);
}

// ── 状态变化触发并携带方向 ─────────────────────────────────────

#[test]
fn change_to_connected_triggers_once() {
    let mut monitor = HdmiMonitor::new();
    monitor.update(false);
    // 断开 → 连接：触发 Connected 且内部状态更新
    assert_eq!(monitor.update(true), Some(HdmiChange::Connected));
    // 一次变化只触发一次：状态已同步，再次 update(true) 不触发
    assert_eq!(monitor.update(true), None);
}

#[test]
fn change_to_disconnected_triggers_once() {
    let mut monitor = HdmiMonitor::new();
    monitor.update(true);
    // 连接 → 断开：触发 Disconnected
    assert_eq!(monitor.update(false), Some(HdmiChange::Disconnected));
    assert_eq!(monitor.update(false), None);
}

// ── 多帧连续变化逐次触发 ───────────────────────────────────────

#[test]
fn successive_changes_trigger_independently() {
    let mut monitor = HdmiMonitor::new();
    monitor.update(false);
    // 连续三次状态翻转，每次变化独立触发一次
    assert_eq!(monitor.update(true), Some(HdmiChange::Connected));
    assert_eq!(monitor.update(false), Some(HdmiChange::Disconnected));
    assert_eq!(monitor.update(true), Some(HdmiChange::Connected));
}
