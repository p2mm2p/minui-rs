//! `Platform` trait — 硬件抽象层
//!
//! 本模块定义了 MinUI 中所有平台实现的统一接口。minui（启动器）和
//! minarch（游戏内前端）仅通过此 trait 访问硬件，不感知底层是 SDL2、
//! SDL1.2 还是 framebuffer + evdev。
//!
//! ## 设计理念
//!
//! - **单一 trait**：不拆分 Video/Input/Audio 子 trait——每个平台都是一个完整的
//!   硬件集合，不存在"只实现 Video 不实现 Input"的情况
//! - **渲染原语而非 Surface**：上层操作 `VideoBuffer`（纯像素数组），不碰
//!   SDL 类型。平台在 `flip` 内部处理像素拷贝和呈现
//! - **关联常量**：屏幕尺寸、缩放倍率等编译期确定的值使用 `const`
//! - **错误处理**：不返回 `Result`——嵌入式设备初始化失败意味着无法运行，
//!   匹配原 C 代码的行为
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.h` 使用 `#define GFX_clear PLAT_clearVideo` 宏转发到平台函数。
//! 这导致跨平台代码和平台实现混杂在同一编译单元中。Rust 版通过 trait 将接口
//! 与实现分离——上层只依赖 `Platform` trait，平台 crate 提供具体实现。
//!
//! 原 C `api.h` 声明了 34 个 `PLAT_*` 函数。Rust `Platform` trait 精减为
//! 27 个方法 + 24 个关联常量（16 设备常量 + 8 按键能力常量 HAS_*；
//! `PLAT_getModel` 由运行时函数改为编译期常量 `DEVICE_MODEL`——见「设备型号」常量声明）：
//!
//! | 去向 | 数量 | 示例 |
//! |------|------|------|
//! | render crate 替代 | 5 | `PLAT_getScaler`、`PLAT_blitRenderer`、`PLAT_setNearestNeighbor`、`PLAT_setSharpness`、`PLAT_setEffect` |
//! | 上层用 VideoBuffer 替代 | 3 | `PLAT_clearVideo`、`PLAT_clearAll`、`PLAT_resizeVideo` |
//! | 空函数 | 2 | `PLAT_setVideoScaleClip`(tg5040:// buh)、`PLAT_setEffectColor` |
//! | 合并到 poll_input | 2 | `PLAT_initLid`、`PLAT_lidChanged` |
//! | Overlay 不需要 | 3 | `PLAT_initOverlay`、`PLAT_quitOverlay`、`PLAT_enableOverlay` |
//! | SDL_Delay → now_ms+sleep | 1 | `PLAT_vsync` |
//! | 新增（C 无直接对应） | 5 | `prepare_sleep`、`complete_wake`、`now_ms`、`is_hdmi_active`、`set_vsync` |
//!

use crate::audio::AudioFrame;
use crate::input::InputState;
use crate::power::{BatteryStatus, CpuSpeed};
use crate::video::{VideoBuffer, VsyncMode};

/// 平台硬件抽象接口
///
/// 每个支持的掌机设备对应一个此 trait 的实现。上层代码（minui/minarch）
/// 通过泛型参数 `<P: Platform>` 使用，编译时单态化，零运行时开销。
///
/// 共 27 个方法 + 24 个关联常量（16 设备常量 + 8 按键能力常量 HAS_*）。
/// 10 个方法提供默认实现——标注 `（默认实现：xxx）` 的方法可选择不覆盖。
pub trait Platform {
    // ══════════════════════════════════════════════
    // 关联常量
    // ══════════════════════════════════════════════

    /// 屏幕物理宽度（像素）
    const SCREEN_WIDTH: u32;
    /// 屏幕物理高度（像素）
    const SCREEN_HEIGHT: u32;
    /// 整数缩放倍率
    const SCALE: u32;
    /// 每像素字节数（2 = RGB565）
    const BYTES_PER_PIXEL: u8;

    /// 设备是否有 HDMI 能力（编译期常量，不同于运行时的 `is_hdmi_active`）
    const HAS_HDMI: bool;
    /// 是否有物理电源键（影响关机文案）
    const HAS_POWER_BUTTON: bool;
    /// 是否有独立关机键
    const HAS_POWEROFF_BUTTON: bool;
    /// 是否支持过扫描区域
    const SUPPORTS_OVERSCAN: bool;
    /// 设备型号名称（如 `"TrimUI Smart Pro"`）
    ///
    /// 编译期确定（设备由 feature 固定）——对应原 C `PLAT_getModel()`
    /// （读环境变量 `TRIMUI_MODEL`），Rust 版改为编译期常量，不读环境变量。
    /// 消费方：minui（版本页显示设备型号）。
    const DEVICE_MODEL: &str;
    /// SD 卡根路径（对应 C 各平台 `platform.h` 的 `SDCARD_PATH` 宏）
    ///
    /// C 原版 13 个平台各不相同（如 `/mnt/SDCARD`、`/mnt/sdcard`、
    /// `/media/roms`）——这是平台知识，SHALL 由各平台实现提供。
    /// 派生路径由 `common::paths` 自由函数按需拼接（见该模块文档）。
    const SDCARD_PATH: &str;
    /// 平台代码（如 `"tg5040"`，对应 C makefile 的 `PLATFORM` 变量）
    ///
    /// 平台代码是**运行期与文件系统的唯一标识**：`.system/{code}`、
    /// `.userdata/{code}`、`Tools/{code}`、`platforms/{code}` 目录与 xtask
    /// `--platform` 参数均用本值——即 `.system/tg5040`、`skeleton/SYSTEM/tg5040`，
    /// **不带 `platform-` 前缀**。cargo 的依赖标识（dep key / feature 名）另用
    /// 包名 `platform-{code}`，只参与编译期解析，**不参与任何路径拼接**。
    const PLATFORM: &str;

    // ── 设备语义键（对应 C 各平台 platform.h 的 BTN_SLEEP/BTN_MOD_* 宏）──

    /// 睡眠键：本设备的"进入睡眠"物理键（tg5040 为 `BTN_POWER`）
    ///
    /// 语义键是前端与 keymon 守护进程的协调常量——keymon 在自己的
    /// 状态机里硬编码同套按键约定（evdev 事件码），前端经本组常量
    /// 识别并忽略其组合键（不误开菜单、不误入核心）。装配层从关联
    /// 常量取值后按值传入纯函数（「调用方收集数据传入」模式）。
    const BTN_SLEEP: u32;
    /// 亮度调节修饰键（tg5040 为 `BTN_MENU`；m17 类平台为 `BTN_START`）
    const BTN_MOD_BRIGHTNESS: u32;
    /// 音量调节修饰键（tg5040 为 `BTN_NONE` 即无此组合）
    const BTN_MOD_VOLUME: u32;
    /// 亮度/音量加键（tg5040 为 `BTN_PLUS`）
    const BTN_MOD_PLUS: u32;
    /// 亮度/音量减键（tg5040 为 `BTN_MINUS`）
    const BTN_MOD_MINUS: u32;

    // ══════════════════════════════════════════════
    // 设备按键能力（8 常量，对应原 C 各平台 platform.h 的
    // BUTTON_*/CODE_*/JOY_*/AXIS_* 宏组合探测式）
    // ══════════════════════════════════════════════

    /// 是否有 L2 键
    ///
    /// 对应原 C `BUTTON_L2!=BUTTON_NA || CODE_L2!=CODE_NA ||
    /// JOY_L2!=JOY_NA || AXIS_L2!=AXIS_NA`（defines.h:53 的
    /// `has_L2` 探测式）。消费方：minput（渲染按键面板）。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_L2: bool = false;
    /// 是否有 R2 键
    ///
    /// 对应原 C `BUTTON_R2!=BUTTON_NA || CODE_R2!=CODE_NA ||
    /// JOY_R2!=JOY_NA || AXIS_R2!=AXIS_NA`。消费方：minput（渲染按键面板）。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_R2: bool = false;
    /// 是否有 L3 键（摇杆按下）
    ///
    /// 对应原 C `BUTTON_L3!=BUTTON_NA || CODE_L3!=CODE_NA ||
    /// JOY_L3!=JOY_NA`。消费方：minput（渲染按键面板）。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_L3: bool = false;
    /// 是否有 R3 键（摇杆按下）
    ///
    /// 对应原 C `BUTTON_R3!=BUTTON_NA || CODE_R3!=CODE_NA ||
    /// JOY_R3!=JOY_NA`。消费方：minput（渲染按键面板）。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_R3: bool = false;
    /// 是否有左摇杆
    ///
    /// 对应原 C `AXIS_LX!=AXIS_NA`。仅表达"设备有左摇杆"这一编译期
    /// 能力，摇杆轴事件翻译由平台 `poll_input` 负责（tg5040 已实现
    /// LX/LY → `BTN_ANALOG_*` 方向键位）。消费方：minput。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_LS: bool = false;
    /// 是否有右摇杆
    ///
    /// 对应原 C `AXIS_RX!=AXIS_NA`。与 `HAS_LS` 同模式。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_RS: bool = false;
    /// 是否有音量键
    ///
    /// 对应原 C `BUTTON_PLUS!=BUTTON_NA || CODE_PLUS!=CODE_NA ||
    /// JOY_PLUS!=JOY_NA`。消费方：minput（渲染音量组）。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_VOLUME: bool = false;
    /// 是否有 MENU 键
    ///
    /// 对应原 C `HAS_MENU_BUTTON` 宏（`BUTTON_MENU!=BUTTON_NA ||
    /// CODE_MENU!=CODE_NA || JOY_MENU!=JOY_NA`，defines.h:89），独立于
    /// `HAS_POWER_BUTTON` 探测。消费方：minput（渲染系统组）。
    ///
    /// （默认实现：`false`——平台未声明即视为无此键）
    const HAS_MENU: bool = false;

    // ══════════════════════════════════════════════
    // 视频（4 方法）
    // ══════════════════════════════════════════════

    /// 初始化视频子系统
    ///
    /// 平台内部创建窗口、渲染器、纹理等 SDL 资源。
    /// **不返回** `VideoBuffer`——上层通过关联常量 `SCREEN_WIDTH`/`SCREEN_HEIGHT`
    /// 自行创建绘制缓冲区，`flip` 时将缓冲区像素提交到平台内部的纹理。
    fn init_video(&mut self);

    /// 销毁视频子系统
    fn quit_video(&mut self);

    /// 设置垂直同步模式
    ///
    /// （默认实现：空操作——部分平台在创建渲染器时已决定 vsync 行为）
    fn set_vsync(&mut self, _mode: VsyncMode) {}

    /// 将 `VideoBuffer` 的像素提交到物理屏幕
    ///
    /// `buffer`：上层绘制的 RGB565 像素缓冲区。平台负责将 `buffer.pixels`
    /// 拷贝到内部 SDL 纹理并呈现。
    /// `wait_vsync`：是否等待垂直同步——由上层帧计时逻辑计算。
    fn flip(&mut self, buffer: &VideoBuffer, wait_vsync: bool);

    // ══════════════════════════════════════════════
    // 输入（4 方法）
    // ══════════════════════════════════════════════

    /// 初始化输入子系统
    fn init_input(&mut self);

    /// 销毁输入子系统
    fn quit_input(&mut self);

    /// 轮询输入设备，返回当前帧的输入状态
    fn poll_input(&mut self) -> InputState;

    /// 重置跨帧输入状态（按键掩码 + 重复计时）
    ///
    /// 对应原 C `PAD_reset()`（api.c:1180-1186）。由 `faux_sleep` 在
    /// 睡眠前/唤醒后各调用一次（对应 C `PWR_fauxSleep` 的两次
    /// `PAD_reset`，api.c:1687-1694）——防止睡眠按键事件残留导致
    /// 唤醒后立即误触发（如睡眠键释放被再次识别为手动睡眠）。
    ///
    /// 注意：`common::input::InputState::reset` 清的是调用方持有的每帧
    /// 快照，对平台的跨帧状态（如 `PadState`）无效——本方法才是平台侧
    /// 跨帧状态的重置入口。
    ///
    /// （默认实现：空操作——无跨帧输入状态的平台无需覆盖）
    fn reset_input(&mut self) {}

    // ══════════════════════════════════════════════
    // 音频（5 方法）
    // ══════════════════════════════════════════════

    /// 协商采样率
    ///
    /// 平台根据硬件能力从请求值和上限中选择实际使用的采样率。
    /// 纯查询（`&self`），在 `init_audio` 之前调用。
    ///
    /// （默认实现：`requested.min(max)`——12 个 C 平台中 11 个使用此逻辑）
    fn pick_sample_rate(&self, requested: u32, max: u32) -> u32 {
        requested.min(max)
    }

    /// 初始化音频设备
    ///
    /// `sample_rate`：`pick_sample_rate` 协商后的采样率。
    /// 返回硬件实际使用的采样率（可能与请求值不同，为重采样器提供基准）。
    fn init_audio(&mut self, sample_rate: u32) -> u32;

    /// 关闭音频设备
    fn quit_audio(&mut self);

    /// 暂停/恢复音频输出（用于睡眠/唤醒）
    fn pause_audio(&mut self, pause: bool);

    /// 推送音频帧到平台内部的播放缓冲
    ///
    /// 返回实际接受的帧数（缓冲满时可能为 0）。
    /// 平台内部负责缓冲管理——可使用 `common::audio::AudioRingBuffer` 或自定义方案。
    fn push_audio(&mut self, frames: &[AudioFrame]) -> usize;

    // ══════════════════════════════════════════════
    // 电源（3 方法）
    // ══════════════════════════════════════════════

    /// 关闭设备电源
    fn power_off(&mut self);

    /// 获取电池状态快照
    fn get_battery_status(&self) -> BatteryStatus;

    /// 设置 CPU 速度档位
    fn set_cpu_speed(&mut self, speed: CpuSpeed);

    // ══════════════════════════════════════════════
    // 硬件（4 方法）
    // ══════════════════════════════════════════════

    /// 开关背光（用于睡眠/唤醒）
    fn enable_backlight(&mut self, enable: bool);

    /// 设置震动强度（0 = 关闭）
    ///
    /// （默认实现：空操作——不是所有设备都有震动功能）
    fn set_rumble(&mut self, _strength: u8) {}

    /// WiFi 是否已连接（对应 C `PLAT_isOnline()`）
    ///
    /// （默认实现：返回 `false`——无 WiFi 的设备）
    fn is_online(&self) -> bool {
        false
    }

    /// HDMI 是否当前连接（运行时检测，不同于 `HAS_HDMI` 编译期常量）
    ///
    /// （默认实现：返回 `false`——无 HDMI 的设备）
    fn is_hdmi_active(&self) -> bool {
        false
    }

    // ══════════════════════════════════════════════
    // 睡眠/唤醒生命周期（3 方法）
    // ══════════════════════════════════════════════

    /// 准备进入睡眠：关背光、暂停音频、停止 keymon、sync 文件系统
    ///
    /// 替代原 C `PWR_enterSleep` 中 `system("killall -STOP keymon.elf")` 等
    /// 平台特定的操作系统命令。
    fn prepare_sleep(&mut self);

    /// 检查是否有唤醒事件（电源键/合盖打开）
    ///
    /// 在 `PWR_fauxSleep` 的唤醒等待循环中每 200ms 调用一次。
    ///
    /// （默认实现：返回 `false`——无唤醒事件）
    fn should_wake(&self) -> bool {
        false
    }

    /// 从睡眠恢复：开背光、恢复音频、启动 keymon、sync 文件系统
    fn complete_wake(&mut self);

    // ══════════════════════════════════════════════
    // 布局（2 方法）
    // ══════════════════════════════════════════════

    /// 每页可显示的行数（列表滚动窗口高度）
    ///
    /// 对应原 C `MAIN_ROW_COUNT` 宏（defines.h:58 通用默认，各平台
    /// platform.h 覆盖）。被 minui 列表渲染用于滚动窗口计算
    /// （UP/DOWN/LEFT/RIGHT 翻页边界）。
    ///
    /// **为什么是方法而非关联常量**：部分平台（my355/rg35xxplus）的
    /// `MAIN_ROW_COUNT` 依赖运行时 HDMI 状态（`on_hdmi`，platform.c:447
    /// 每帧刷新）——编译期常量无法表达。tg5040 等编译期固定的平台
    /// 可在方法体内用 `#[cfg(feature)]` 分支，零运行时开销（单态化）。
    ///
    /// （默认实现：返回 `common::video::MAIN_ROW_COUNT`——通用默认 6）
    fn main_row_count(&self) -> u32 {
        crate::video::MAIN_ROW_COUNT
    }

    /// 页面边缘留白（列表项左右边距，未缩放像素）
    ///
    /// 对应原 C `PADDING` 宏（defines.h:63 通用默认，各平台 platform.h
    /// 覆盖——tg5040 smart=40、brick=5）。被 minui 列表渲染用于
    /// 列表项水平边距与顶部偏移（`SCALE1(PADDING)`）。
    ///
    /// **为什么是方法而非关联常量**：与 `main_row_count` 同理——my355/
    /// rg35xxplus 的 `PADDING` 依赖运行时 HDMI 状态。
    ///
    /// （默认实现：返回 `common::video::PADDING`——通用默认 10）
    fn padding(&self) -> u32 {
        crate::video::PADDING
    }

    // ══════════════════════════════════════════════
    // 时间（2 方法）
    // ══════════════════════════════════════════════

    /// 单调时钟，毫秒精度
    ///
    /// 替代 SDL `SDL_GetTicks()`。被 `PWR_update`、帧计时、`tapped_menu` 等模块使用。
    fn now_ms(&self) -> u32;

    /// 设置系统日期时间（对应 C `PLAT_setDateTime`）
    ///
    /// 原版 C 所有平台均通过 common 层 `date + hwclock` 系统命令设置时间
    /// （`api.c:1717-1722`）——Rust 版保留默认空实现仅为 trait 覆盖点，
    /// **不代表设备不支持 RTC**：设置时间依赖系统 `date`/`hwclock` 命令，
    /// 属于平台相关实现细节，平台 SHALL 按需覆盖（如 tg5040 用
    /// `std::process::Command` 免 shell 转义实现，见 power.rs）。
    ///
    /// （默认实现：空操作——平台相关实现细节，所有平台均支持设置时间）
    fn set_date_time(&mut self, _y: i32, _m: i32, _d: i32, _h: i32, _min: i32, _sec: i32) {}
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用 mock 平台——不覆盖任何能力常量（验证默认值语义）
    struct MockPlatform;

    impl Platform for MockPlatform {
        const SCREEN_WIDTH: u32 = 640;
        const SCREEN_HEIGHT: u32 = 480;
        const SCALE: u32 = 1;
        const BYTES_PER_PIXEL: u8 = 2;
        const HAS_HDMI: bool = false;
        const HAS_POWER_BUTTON: bool = false;
        const HAS_POWEROFF_BUTTON: bool = false;
        const SUPPORTS_OVERSCAN: bool = false;
        const DEVICE_MODEL: &'static str = "Mock";
        const SDCARD_PATH: &'static str = "/mnt/SDCARD";
        const PLATFORM: &'static str = "mock";
        const BTN_SLEEP: u32 = 0;
        const BTN_MOD_BRIGHTNESS: u32 = 0;
        const BTN_MOD_VOLUME: u32 = 0;
        const BTN_MOD_PLUS: u32 = 0;
        const BTN_MOD_MINUS: u32 = 0;

        fn init_video(&mut self) {}
        fn quit_video(&mut self) {}
        fn flip(&mut self, _buffer: &crate::video::VideoBuffer, _wait_vsync: bool) {}
        fn init_input(&mut self) {}
        fn quit_input(&mut self) {}
        fn poll_input(&mut self) -> crate::input::InputState {
            crate::input::InputState::new()
        }
        fn init_audio(&mut self, _sample_rate: u32) -> u32 {
            0
        }
        fn quit_audio(&mut self) {}
        fn pause_audio(&mut self, _pause: bool) {}
        fn push_audio(&mut self, _frames: &[crate::audio::AudioFrame]) -> usize {
            0
        }
        fn power_off(&mut self) {}
        fn get_battery_status(&self) -> crate::power::BatteryStatus {
            crate::power::BatteryStatus {
                percentage: 100,
                charging: false,
            }
        }
        fn enable_backlight(&mut self, _enable: bool) {}
        fn set_cpu_speed(&mut self, _speed: crate::power::CpuSpeed) {}
        fn prepare_sleep(&mut self) {}
        fn complete_wake(&mut self) {}
        fn now_ms(&self) -> u32 {
            0
        }
    }

    /// 未覆盖能力常量的平台，8 个常量默认值全为 false（保守语义）
    ///
    /// 用 const 块做**编译期**断言——能力常量是编译期值，编译失败即测试失败
    #[test]
    fn capability_constants_default_to_false() {
        const {
            assert!(!MockPlatform::HAS_L2);
            assert!(!MockPlatform::HAS_R2);
            assert!(!MockPlatform::HAS_L3);
            assert!(!MockPlatform::HAS_R3);
            assert!(!MockPlatform::HAS_LS);
            assert!(!MockPlatform::HAS_RS);
            assert!(!MockPlatform::HAS_VOLUME);
            assert!(!MockPlatform::HAS_MENU);
        }
    }

    /// 能力常量与既有 HAS_POWER_BUTTON 并列存在（不冲突）
    #[test]
    fn capability_constants_coexist_with_power_button() {
        const {
            assert!(!MockPlatform::HAS_POWER_BUTTON);
            assert!(!MockPlatform::HAS_MENU);
        }
    }
}
