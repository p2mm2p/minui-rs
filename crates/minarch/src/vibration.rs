//! 振动管理（对应 C `api.c` VIB_Context/VIB_thread，minarch.c:1400-1456）
//!
//! 负责把核心的振动请求（经 `environment::rumble_trampoline` 转来的
//! 强度值）以「排队 + 帧去抖」的语义应用到平台 `Platform::set_rumble`。
//!
//! ## 为什么排队 + 去抖？
//!
//! 原版 C 注释（minarch.c:1405）：「minimize vacillation between 0 and
//! some number (which this motor doesn't like)」——真实马达讨厌
//! 0↔非 0 的快速抖动，所以关断要延迟 3 帧（`DEFER_FRAMES 3`），期间
//! 若新非零请求到来则立即恢复。开启则永远立即生效。
//!
//! ## 与 C 的架构差异
//!
//! C 用独立 pthread 每 17ms 轮询（`VIB_thread` + `SDL_Delay(17)`），
//! Rust 改为**主循环每帧 tick**：
//!
//! - 60fps 主循环 ≈ 16.7ms/帧，与 C 的 17ms 轮询时序等价
//! - `DEFER_FRAMES` 本就是帧语义，17ms 只是 C 用时间模拟帧
//! - 先例：`common::power` 已将 C 的电池监控 pthread 收敛为帧内轮询
//! - `Platform::set_rumble(&mut self)` 迫使线程方案把平台静态化 +
//!   全局锁——为 3 帧计数器付全局锁不值得
//!

use std::sync::{Mutex, OnceLock};

/// 振动强度排队与帧去抖状态机（对应 C `static struct VIB_Context`，
/// minarch.c:1400-1403）
///
/// 纯逻辑状态机：不碰平台、无线程、无 IO。字段语义：
///
/// - `queued`: 排队强度——`set_strength` 写入，等待 tick 推进
/// - `current`: 实际强度——已应用到马达的值
/// - `defer`: 关断去抖计数（0..=3）——排队关断时逐帧累加
///
/// ## 与 C 的偏离
///
/// - C 的 `defer` 是线程内 static 局部变量 → Rust 归入结构体字段
///   （无线程，状态必须显式归属）
/// - C 的 `queued`/`current` 为 `int` → Rust 为 `u8`
///   （`rumble_trampoline` 已截断为 u8，平台能力上限）
pub struct Vibration {
    queued: u8,
    current: u8,
    defer: u8,
}

impl Default for Vibration {
    /// 与 [`Vibration::new`] 等价（零强度、零去抖）
    fn default() -> Self {
        Self::new()
    }
}

impl Vibration {
    /// 构造全新引擎（零强度、零去抖）
    pub fn new() -> Self {
        Self {
            queued: 0,
            current: 0,
            defer: 0,
        }
    }

    /// 排队强度（对应 C `VIB_setStrength`，minarch.c:1435-1438）
    ///
    /// 只写入 `queued`，不立即应用——真正应用到平台发生在
    /// [`tick`](Self::tick) 返回 `Some` 时。
    ///
    /// # 参数
    ///
    /// - `strength`: 目标强度（`0` = 关断）
    ///
    /// # 语义
    ///
    /// 与当前排队值相同时忽略（C `if (queued_strength==strength) return`）。
    pub fn set_strength(&mut self, strength: u8) {
        if self.queued == strength {
            return;
        }
        self.queued = strength;
    }

    /// 查询实际强度（对应 C `VIB_getStrength`，minarch.c:1439-1441）
    ///
    /// 返回 `current`（已应用值）而非 `queued`（排队值）——睡眠前
    /// 保存强度（C main:4246）依赖此语义：排队中的值还没震过马达，
    /// 唤醒后恢复它没有意义。
    pub fn get_strength(&self) -> u8 {
        self.current
    }

    /// 推进一帧去抖（对应 C `VIB_thread` 一轮 17ms 轮询）
    ///
    /// 返回待应用强度（`Some`）或无动作（`None`）。状态转移：
    ///
    /// - `queued == current` → `None`（空闲）
    /// - `queued != 0` → `Some(queued)`，`defer` 清零（**立即应用**）
    /// - `queued == 0` 且 `defer < 3` → `None`，`defer += 1`（**延迟关断**）
    /// - `queued == 0` 且 `defer >= 3` → `Some(0)`，`defer` 清零（关断）
    ///
    /// 调用方（装配层主循环）在拿到 `Some` 时应执行
    /// `platform.set_rumble(strength)`——本引擎不直接触碰平台。
    pub fn tick(&mut self) -> Option<u8> {
        if self.queued == self.current {
            return None;
        }
        if self.queued != 0 {
            self.current = self.queued;
            self.defer = 0;
            return Some(self.current);
        }
        if self.defer < 3 {
            self.defer += 1;
            return None;
        }
        self.current = 0;
        self.defer = 0;
        Some(0)
    }

    /// 睡眠前保存实际强度并确定性清零（对应 C main:4246-4247）
    ///
    /// # 返回值
    ///
    /// 保存的 `current`——调用方应在唤醒后把它传给
    /// [`resume`](Self::resume)。
    ///
    /// ## 与 C 的偏离
    ///
    /// C 只排队清零（`VIB_setStrength(0)`），依赖线程唤醒后的执行
    /// 顺序巧合恢复正确值；Rust 确定性清零 `queued`/`current`/`defer`
    /// 三者，结果等价、消除时序依赖。
    pub fn suspend(&mut self) -> u8 {
        let saved = self.current;
        self.queued = 0;
        self.current = 0;
        self.defer = 0;
        saved
    }

    /// 唤醒后恢复强度（对应 C main:4554-4555）
    ///
    /// # 参数
    ///
    /// - `strength`: 睡眠前由 [`suspend`](Self::suspend) 保存的值；
    ///   `0` 时无操作（睡眠前马达本就没震）
    pub fn resume(&mut self, strength: u8) {
        if strength != 0 {
            self.set_strength(strength);
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 薄静态层
// ═══════════════════════════════════════════════════════════════

/// 进程级振动引擎静态（OnceLock 注册后只读；Mutex 承载可变引擎状态）
///
/// 与 `environment::RUNTIME` 同构的薄静态层。`fn(u8)` hook 类型
/// （`environment::set_rumble_hook`）不可捕获状态，引擎只能以静态
/// 承载；Mutex 是跨线程（核心回调线程写 `queued` / 主循环 tick）共享
/// 可变状态的唯一诚实载体。
static VIBRATION: OnceLock<Mutex<Vibration>> = OnceLock::new();

/// 注册进程级引擎（装配层在核心加载前调用一次）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(vibration)`: 已注册过（重复注册是误用），携带本次传入的
///   引擎原值（`OnceLock::set` 语义）
pub fn init() -> Result<(), Vibration> {
    VIBRATION
        .set(Mutex::new(Vibration::new()))
        .map_err(|m| m.into_inner().unwrap_or_else(|p| p.into_inner()))
}

/// 全局排队强度：锁内转发 [`Vibration::set_strength`]，未 `init`
/// 时空操作（与 `environment::handle` 未 init 语义一致）
///
/// # 参数
///
/// - `strength`: 目标强度（`0` = 关断）
pub fn set_strength(strength: u8) {
    if let Some(engine) = VIBRATION.get() {
        engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_strength(strength);
    }
}

/// 全局推进一帧去抖：锁内转发 [`Vibration::tick`]，未 `init` 时
/// 返回 `None`
///
/// 调用方（装配层主循环）在拿到 `Some` 时应执行
/// `platform.set_rumble(strength)`。
pub fn tick() -> Option<u8> {
    VIBRATION.get().and_then(|engine| {
        engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tick()
    })
}

/// 全局睡眠前保存并清零：锁内转发 [`Vibration::suspend`]，未
/// `init` 时返回 `0`
pub fn suspend() -> u8 {
    VIBRATION
        .get()
        .map(|engine| {
            engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .suspend()
        })
        .unwrap_or(0)
}

/// 全局唤醒后恢复：锁内转发 [`Vibration::resume`]，未 `init` 时
/// 空操作
///
/// # 参数
///
/// - `strength`: 睡眠前由 [`suspend`] 保存的值
pub fn resume(strength: u8) {
    if let Some(engine) = VIBRATION.get() {
        engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .resume(strength);
    }
}
