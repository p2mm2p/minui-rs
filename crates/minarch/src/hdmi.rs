//! HDMI 热插拔检测：变化检测状态机（对应 C `hdmimon()`，minarch.c:2162-2176）
//!
//! 本模块回答一个问题：**HDMI 输出状态有没有发生变化？**
//!
//! 原版 C 的 `hdmimon()` 是一个"每帧轮询 + 状态记忆"函数：它记住上一次
//! 采样到的 HDMI 状态（`static int had_hdmi = -1`），每帧重新采样一次，
//! 一旦发现状态变化就触发"重启前端"的连锁动作（因为 HDMI 插拔通常伴随
//! 分辨率/音频输出路径变化，重启是让系统干净地重新初始化的最稳方式）。
//!
//! ## 边界（谁做什么）
//!
//! - **本模块（纯逻辑状态机）**：只做「采样 → 比较 → 报告变化」，
//!   零静态、零平台依赖、零 FFI、零泛型——`is_active` 由调用方
//!   （装配层）每帧从 `Platform::is_hdmi_active()` 收集后作为**变量**
//!   传入（config 规则「调用方收集数据传入」）
//! - **装配层（主循环 change，半环登记）**：收到 `Some(HdmiChange)` 后
//!   执行 C 的重启序列（`Menu_beforeSleep()` → `sleep(4)` → `quit`，
//!   minarch.c:2170-2174）——见 spec「minarch 半环闭环清单」
//!
//! ## 当前平台说明
//!
//! tg5040 平台的 `Platform::is_hdmi_active()` 恒返回 `false`（与原 C
//! `GetHDMI()` 恒返回 0 一致，msettings.c:246-250）——因此本状态机在
//! 当前平台**永不触发**变化。模块是为未来支持 HDMI 检测的平台预留的
//! 忠实结构（C 版 `hdmimon()` 在 tg5040 上同样是死代码）。
//!
//! ## 与 C 的偏离
//!
//! - C 用函数内 `static int had_hdmi = -1`（-1 哨兵表示"尚未采样"）——
//!   Rust 无函数内可变 static，状态显式归属 [`HdmiMonitor`] 结构体字段，
//!   用 `Option<bool>` 的 `None` 表达"尚未采样"，消除 -1 魔法值
//! - C 只判断"变没变"（不区分方向）——Rust 返回 [`HdmiChange`] 枚举
//!   携带方向信息（`Connected`/`Disconnected`），装配层可自行决定是否
//!   忽略方向（当前只重启，与 C 行为一致）
//! - C 变化时打日志（`LOG_info`，minarch.c:2170）——日志缺失期静默，
//!   变化信息经返回值传给装配层（与 sram/environment 同约定）
//!

/// HDMI 状态变化的方向
///
/// 对应 C `hdmimon()` 中 `has_hdmi != had_hdmi` 的两种变化方向
/// （minarch.c:2167）。C 只关心"变了"，不区分方向；Rust 保留完整
/// 信息，供装配层后续（如 HDMI 断开时恢复音量，api.c:198-199）使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdmiChange {
    /// HDMI 从断开变为连接（`is_active` 从 `false` 变为 `true`）
    Connected,
    /// HDMI 从连接变为断开（`is_active` 从 `true` 变为 `false`）
    Disconnected,
}

/// HDMI 热插拔检测状态机
///
/// 对应 C `hdmimon()` 的"上次状态记忆"（minarch.c:2164 的
/// `static int had_hdmi = -1`）。实例由装配层持有，主循环每帧调用
/// [`HdmiMonitor::update`] 推进一次采样。
pub struct HdmiMonitor {
    /// 上次采样的 HDMI 状态；`None` 表示尚未采样（对应 C `had_hdmi == -1`）
    had_hdmi: Option<bool>,
}

impl HdmiMonitor {
    /// 新建监视器，尚未采样任何 HDMI 状态
    ///
    /// # 返回值
    ///
    /// `had_hdmi = None` 的初始状态（首次 [`HdmiMonitor::update`]
    /// 只记录不触发）。
    pub fn new() -> Self {
        HdmiMonitor { had_hdmi: None }
    }

    /// 单帧采样推进：传入当前 HDMI 是否活跃，返回状态变化
    ///
    /// # 参数
    ///
    /// - `is_active`: 当前 HDMI 是否活跃——由装配层每帧从
    ///   `Platform::is_hdmi_active()` 收集传入（本模块不接触平台）
    ///
    /// # 返回值
    ///
    /// - `None`: 首次采样（只记录状态，对应 C `had_hdmi==-1` 分支，
    ///   不视为变化）或状态无变化（`is_active == had_hdmi`）
    /// - `Some(HdmiChange)`: 状态发生变化（`is_active != had_hdmi`），
    ///   同时内部状态更新——同一状态连续采样只触发一次
    pub fn update(&mut self, is_active: bool) -> Option<HdmiChange> {
        match self.had_hdmi {
            // 首次采样：记录状态、不触发（C `if (had_hdmi==-1) had_hdmi = has_hdmi;`）
            None => {
                self.had_hdmi = Some(is_active);
                None
            }
            // 无变化：不触发
            Some(had) if had == is_active => None,
            // 变化：更新状态并返回方向
            Some(_) => {
                self.had_hdmi = Some(is_active);
                Some(if is_active {
                    HdmiChange::Connected
                } else {
                    HdmiChange::Disconnected
                })
            }
        }
    }
}

impl Default for HdmiMonitor {
    /// 与 [`HdmiMonitor::new`] 等价的默认构造
    fn default() -> Self {
        Self::new()
    }
}
