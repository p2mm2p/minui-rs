//! 测试共享基础设施
//!
//! 本模块提供 minui crate 测试共用的全局资源：
//!
//! `TMP_LOCK`——`/tmp` 协议文件（`/tmp/next`、`/tmp/last.txt`、
//! `/tmp/change_disc.txt`、`/tmp/resume_slot.txt` 等）的**跨模块互斥锁**。
//!
//! ## 为什么需要全局共享锁
//!
//! `launch`/`recents`/`browser`/`menu` 的测试都读写 `/tmp` 协议文件
//! （进程通信文件，crate 内共享路径）。各模块若各自定义私有静态锁，
//! 并行测试时跨模块互相踩踏（如 menu 测试删除 `/tmp/next` 会破坏
//! launch 测试对 `/tmp/next` 内容的断言）。凡涉及 `/tmp` 文件读写的
//! 测试 SHALL 获取本锁（而不是各模块私有锁）。
//!
//! `lock()` 使用 `unwrap_or_else(|e| e.into_inner())`——单个测试失败
//! （panic）不 poison 锁拖垮其余测试。
#![cfg(test)]

/// crate 级 `/tmp` 文件测试互斥锁
pub(crate) static TMP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
