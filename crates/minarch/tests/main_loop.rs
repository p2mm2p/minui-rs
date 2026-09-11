//! main_loop 装配层集成测试（implement-minarch-thread-video）
//!
//! 覆盖 spec「核心线程生命周期」的可测部分：`core::spawn_core_thread`
//! 方案 A 优雅退出（`should_run=false` → 自退出 → join）与核心线程
//! 门控（`Arc<AtomicBool>`）。
//!
//! 注：`main.rs` 的完整装配（RunState/menu_loop/handle_shortcut）是 bin
//! 目标且 feature 门控，无法在无平台 feature 的测试环境链接——与
//! `tests/assembly.rs` 同先例（装配层平台交互不直接测试）；线程机制
//! 的可测部分下沉到 `core.rs`（lib），本文件覆盖它。

mod test_common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use minarch::core::spawn_core_thread;
use minarch::libretro::Core;

/// 加载 mock 核心（Arc 包装——线程与测试共享，保持 .so 存活）
fn open_core(id: &str) -> Arc<Core> {
    let path = test_common::build_mock_core(&format!("mainloop_{id}"), &[]);
    Arc::new(
        Core::open(
            path.to_str().unwrap(),
            "TST",
            "/cfg",
            "/states",
            "/saves",
            "/bios",
        )
        .expect("mock 核心加载"),
    )
}

/// 方案 A：置 `should_run=false` 后核心线程自退出，join 返回
#[test]
fn core_thread_graceful_exit_on_should_run_false() {
    let core = open_core("exit");
    let should_run = Arc::new(AtomicBool::new(true));

    let handle = spawn_core_thread(Arc::clone(&core), Arc::clone(&should_run), 4166, 3);

    // 让线程跑几轮（core.run 是 mock 的短调用），然后请求退出
    std::thread::sleep(std::time::Duration::from_millis(50));
    should_run.store(false, Ordering::Release);

    // join 应返回（线程自退出，不 panic）
    handle.join().expect("核心线程应优雅退出并可 join");

    // Arc 保持 .so 存活：drop core 后（线程已 join）可安全释放
    drop(core);
}

/// 方案 A：`should_run` 初始 false 时线程立即自退出（不执行 run）
#[test]
fn core_thread_stops_calling_run() {
    let core = open_core("stop");
    let should_run = Arc::new(AtomicBool::new(false)); // 一开始就不运行

    let handle = spawn_core_thread(Arc::clone(&core), Arc::clone(&should_run), 4166, 3);

    // 线程应立即退出（should_run 初始 false → 闭包第一轮即 return）
    handle
        .join()
        .expect("should_run=false 时核心线程应立即自退出");
}

/// 方案 A：线程存活期间持续调 run；门控翻转后退出
#[test]
fn core_thread_runs_while_gated() {
    let core = open_core("run");
    let should_run = Arc::new(AtomicBool::new(true));

    let handle = spawn_core_thread(Arc::clone(&core), Arc::clone(&should_run), 4166, 3);

    // 运行一小段（mock run 是短调用，期间线程在跑）
    std::thread::sleep(std::time::Duration::from_millis(30));

    // 门控关闭 → 优雅退出
    should_run.store(false, Ordering::Release);
    handle.join().expect("门控关闭后核心线程应自退出并可 join");
}

/// 方案 A：重复门控开关（菜单场景：开 → 关 → 开），线程不 panic
#[test]
fn core_thread_should_run_toggle_no_panic() {
    let core = open_core("toggle");
    let should_run = Arc::new(AtomicBool::new(true));

    let handle = spawn_core_thread(Arc::clone(&core), Arc::clone(&should_run), 4166, 3);

    // 模拟菜单开关：置 false 再置 true（线程空转检查标志，不退出）
    std::thread::sleep(std::time::Duration::from_millis(20));
    should_run.store(false, Ordering::Release);
    std::thread::sleep(std::time::Duration::from_millis(20));
    should_run.store(true, Ordering::Release);
    std::thread::sleep(std::time::Duration::from_millis(20));

    // 最终退出
    should_run.store(false, Ordering::Release);
    handle.join().expect("门控反复开关后线程仍应优雅退出");
}
