//! libretro 环境回调（纯分发 + 薄静态）
//!
//! 本模块实现 minarch 的环境回调体系，对应原 C `minarch.c` 的：
//!
//! - `environment_callback` 30 case 分发（minarch.c:1860-2158）
//! - `set_rumble_state`（minarch.c:1855-1859）→ [`rumble_trampoline`]
//! - 全局变量归属：`config.core`/`config.controls`/`has_custom_controllers`/
//!   `disk_control_ext`
//!
//! ## 分层：纯分发 + 薄静态
//!
//! C 版 `environment_callback` 是一个直接摸全局变量的 switch（30 个
//! case 分 6 组）。Rust 版拆成两层：
//!
//! 1. **纯分发层** [`dispatch`]：全部 case 的可测核心，只操作本地
//!    [`EnvironmentState`]/[`EnvironmentRuntime`] 实例，不碰全局。
//!    测试用本地实例构造——没有跨测试污染（config/controls 测试已
//!    证明本地实例模式的有效性）。
//! 2. **薄静态层**：进程级 `STATE`/`RUNTIME`/`RUMBLE_HOOK` 静态。
//!    [`handle`] 锁 `RUNTIME` 后转发 `dispatch`——核心经
//!    `FrontendState.environment` 注册进入的唯一入口。
//!    `RUNTIME` 的 Mutex 是**数据归属**而非"内部加锁"：核心选项表/
//!    换碟表是跨回调（核心线程）与跨模块（未来 menu）共享的可变
//!    状态，Mutex 是唯一诚实载体；`STATE`/`RUMBLE_HOOK` 注册后只读，
//!    OnceLock 无锁。
//!
//! ## 与原 C 代码的对比
//!
//! - C 全局变量（`config.core` 等）→ Rust 薄静态 + Mutex 数据归属
//! - C 的 switch 摸全局 → Rust 纯分发（本地实例可测）
//! - C 的 8→9 fallthrough 野写（minarch.c:1885-1889）→ Rust 显式
//!   区分 case（缺陷不继承，见 [`dispatch`] 的 SET_PERFORMANCE_LEVEL
//!   分支）
//! - C 的 `exactMatch`（strncmp 大小写敏感）→ Rust 大小写不敏感扫描
//!   （见 [`dispatch`] 的 SET_CONTROLLER_INFO 分支）
//! - C 日志（LOG_info）→ SET_MESSAGE 用 `println!`，其余静默（日志
//!   设施缺失期的简化，design 决策 7，待统一日志模块回填）
//!
//! ## 边界（半环闭环清单）
//!
//! handler 注册进 `FrontendState`、rumble hook 接线（经
//! `vibration::set_strength`，SHALL NOT 直连平台自由函数）、
//! `controls_mapping`/`disc_control`/`has_custom_controllers` 的消费方
//! 「minarch 半环闭环清单」。
//!

use core::ffi::{c_char, c_void};
use std::ffi::CStr;
use std::ffi::CString;
use std::sync::{Mutex, OnceLock};

use crate::config::OptionList;
use crate::controls::{self, ButtonMapping};
use crate::libretro::{
    RETRO_AV_ENABLE_AUDIO, RETRO_AV_ENABLE_VIDEO, RETRO_DEVICE_ANALOG, RETRO_DEVICE_JOYPAD,
    RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE, RETRO_ENVIRONMENT_GET_CAN_DUPE,
    RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION, RETRO_ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER,
    RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION, RETRO_ENVIRONMENT_GET_INPUT_BITMASKS,
    RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES, RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
    RETRO_ENVIRONMENT_GET_OVERSCAN, RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
    RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY,
    RETRO_ENVIRONMENT_GET_VARIABLE, RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
    RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK, RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE,
    RETRO_ENVIRONMENT_SET_CONTROLLER_INFO, RETRO_ENVIRONMENT_SET_CORE_OPTIONS,
    RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY, RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL,
    RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE, RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE,
    RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK, RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
    RETRO_ENVIRONMENT_SET_MESSAGE, RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL,
    RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
    RETRO_ENVIRONMENT_SET_VARIABLE, RETRO_ENVIRONMENT_SET_VARIABLES, RetroControllerInfo,
    RetroCoreOptionDefinition, RetroCoreOptionsIntl, RetroDiskControlCallback,
    RetroDiskControlExtCallback, RetroInputDescriptor, RetroMessage, RetroPixelFormat,
    RetroRumbleEffect, RetroRumbleInterface, RetroVariable,
};

/// 读取核心提供的 C 字符串并复制为 Rust 所有（`NULL` 按空串处理）
fn c_string_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: libretro.h 规定核心字符串为 NUL 结尾且在回调期间有效
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

// ═══════════════════════════════════════════════════════════════
// 运行时状态与 hook 基础设施
// ═══════════════════════════════════════════════════════════════

/// 运行时可变状态（C `config.core`/`config.controls`/
/// `has_custom_controllers`/`disk_control_ext` 全局变量的 Rust 归属）
///
/// 全部可变状态集中在 `RUNTIME` 静态的 Mutex 内——核心回调线程与
/// 未来 menu 线程共享，Mutex 是唯一诚实载体（见模块文档「分层」）。
pub struct EnvironmentRuntime {
    /// 核心选项表（C `config.core`；SET_CORE_OPTIONS/_INTL/
    /// SET_VARIABLES 替换）
    pub core_options: OptionList,
    /// 当前按键映射（C `config.controls`；SET_INPUT_DESCRIPTORS 替换，
    /// 初值 [`controls::default_button_mapping`]）
    pub controls_mapping: Vec<ButtonMapping>,
    /// 是否有自定义控制器（C 静态 int；SET_CONTROLLER_INFO 扫描
    /// dualshock 置位）
    pub has_custom_controllers: bool,
    /// 换碟回调表（C 全局 `disk_control_ext`；存储语义见 [`dispatch`]
    /// 的 D 组分支）
    pub disc: Option<RetroDiskControlExtCallback>,
}

/// [`EnvironmentRuntime`] 的初值（与 C 全局变量零初始化语义对应）：
/// 空选项表、默认按键映射、无自定义控制器、无换碟表。
impl Default for EnvironmentRuntime {
    fn default() -> Self {
        Self {
            core_options: OptionList {
                options: Vec::new(),
                changed: false,
            },
            controls_mapping: controls::default_button_mapping(),
            has_custom_controllers: false,
            disc: None,
        }
    }
}

/// 注册后只读的目录状态（C 回调读 `core.bios_dir`/`core.saves_dir`）
///
/// 由 [`init`] 注册一次，此后只读——`STATE` 是 OnceLock，无锁读取。
pub struct EnvironmentState {
    /// BIOS 目录（CString 保证 NUL 结尾与生命周期稳定）
    pub bios_dir: CString,
    /// 存档目录
    pub saves_dir: CString,
}

/// 目录状态静态（OnceLock：注册后只读，无锁）
static STATE: OnceLock<EnvironmentState> = OnceLock::new();
/// 运行时状态静态（Mutex：核心线程/未来 menu 线程共享的可变数据）
static RUNTIME: OnceLock<Mutex<EnvironmentRuntime>> = OnceLock::new();
/// 振动 hook 静态（OnceLock：一次性注册后只读）
static RUMBLE_HOOK: OnceLock<fn(u8)> = OnceLock::new();

/// 初始化目录状态与运行时（装配层在核心加载前调用一次）
///
/// 首次调用注册 [`EnvironmentState`]（BIOS/存档目录）并初始化
/// [`EnvironmentRuntime`]（空选项表、默认按键映射 16 项、
/// `has_custom_controllers == false`、`disc == None`）。
///
/// # 参数
///
/// - `bios_dir`: BIOS 目录路径
/// - `saves_dir`: 存档目录路径
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(state)`: 已注册过（重复注册是误用），携带本次传入的状态原值
pub fn init(bios_dir: CString, saves_dir: CString) -> Result<(), EnvironmentState> {
    match STATE.set(EnvironmentState {
        bios_dir,
        saves_dir,
    }) {
        Ok(()) => {
            // STATE 与 RUNTIME 只在首次 init 成对写入——此处 STATE
            // 刚成功，RUNTIME 必然未注册
            let _ = RUNTIME.set(Mutex::new(EnvironmentRuntime::default()));
            Ok(())
        }
        Err(state) => Err(state),
    }
}

/// handler：锁 `RUNTIME` 后转发 [`dispatch`]（注册进
/// `FrontendState.environment` 的入口，签名与 `EnvironmentHandler` 兼容）
///
/// 未 [`init`] 时返回 `false`（与 trampoline 未注册一致的默认语义）。
///
/// 锁毒化恢复：`lock().unwrap_or_else(|p| p.into_inner())`——回调中的
/// panic 不应毒死后续分发（核心回调线程与装配层的边界约定）。
///
/// # 参数
///
/// - `cmd`: `RETRO_ENVIRONMENT_*` 命令码
/// - `data`: 随命令语义变化的载荷指针（有效性契约见 [`dispatch`]）
///
/// # 返回值
///
/// 已初始化时转发 `dispatch` 的返回值；未初始化时 `false`。
///
/// 安全说明：`data` 的有效性契约与 [`dispatch`] 的 `# Safety` 完全
/// 相同（核心保证载荷与命令匹配）。本函数保持安全签名是设计约束——
/// 必须兼容 `EnvironmentHandler`（安全 fn 指针类型）以注册进
/// `FrontendState.environment`；真正的信任边界在 unsafe trampoline
/// （核心经它调用）与 [`dispatch`] 的 unsafe 块之间。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 签名兼容约束见上：handler 必须为安全 fn
pub fn handle(cmd: u32, data: *mut c_void) -> bool {
    let Some(state) = STATE.get() else {
        return false;
    };
    let Some(runtime) = RUNTIME.get() else {
        return false;
    };
    // 锁毒化恢复：回调中的 panic 不应毒死后续分发（into_inner 取回数据）
    let mut guard = runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: 契约转发给 dispatch——data 与 cmd 的匹配由核心保证
    unsafe { dispatch(cmd, data, state, &mut guard) }
}

/// 注册振动强度消费函数（装配层接线：经振动引擎，即
/// `|s| vibration::set_strength(s)`——引擎见 `vibration` 模块；
/// 主循环每帧 `vibration::tick()` 驱动并将输出经
/// `Platform::set_rumble` 应用。hook 与 apply 均 SHALL NOT
/// 直连平台自由函数）
///
/// OnceLock 一次性注册，注册后只读。未注册时 [`rumble_trampoline`]
/// 静默空操作（恒返回 true）。
///
/// # 参数
///
/// - `hook`: 接收 `u8` 强度的消费函数（`u16 → u8` 截断是平台能力
///   上限，见 [`rumble_trampoline`]）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(hook)`: 已注册过（重复注册是误用），携带本次传入的原值
pub fn set_rumble_hook(hook: fn(u8)) -> Result<(), fn(u8)> {
    RUMBLE_HOOK.set(hook)
}

/// 振动 trampoline（对应 C `set_rumble_state`，minarch.c:1855-1859）
///
/// GET_RUMBLE_INTERFACE 填入核心的振动回调：核心每帧按需调用。
/// hook 已注册时把强度截断为 `u8` 转交；未注册时空操作。
/// **恒返回 true**（C `return 1`）。
///
/// 与 C 的偏离：`u16 → u8` 截断是平台能力上限（`Platform::set_rumble(u8)`）；
/// `_port`/`_effect` 忽略（C TODO 同款，minarch.c:1856）。
///
/// # Safety
///
/// 本函数由核心 C 代码经函数指针调用；参数为标量，无前置条件。
pub unsafe extern "C" fn rumble_trampoline(
    _port: u32,
    _effect: RetroRumbleEffect,
    strength: u16,
) -> bool {
    if let Some(hook) = RUMBLE_HOOK.get() {
        hook(strength as u8);
    }
    true
}

/// 查询当前按键映射（装配/菜单消费用，锁内拷贝）
///
/// 未 [`init`] 时返回默认映射（16 项，与 init 的初值一致）。
pub fn controls_mapping() -> Vec<ButtonMapping> {
    RUNTIME
        .get()
        .map_or_else(controls::default_button_mapping, |runtime| {
            runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .controls_mapping
                .clone()
        })
}

/// 查询换碟回调表（装配/换碟 UI 消费用，锁内拷贝）
///
/// 未 [`init`] 或核心未上报时为 `None`。
pub fn disc_control() -> Option<RetroDiskControlExtCallback> {
    RUNTIME.get().and_then(|runtime| {
        runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .disc
    })
}

/// 查询是否有自定义控制器（gamepad 选项消费用）
///
/// 未 [`init`] 或核心未上报 dualshock 时为 `false`。
pub fn has_custom_controllers() -> bool {
    RUNTIME.get().is_some_and(|runtime| {
        runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .has_custom_controllers
    })
}

// ═══════════════════════════════════════════════════════════════
// 纯分发层
// ═══════════════════════════════════════════════════════════════

/// 纯分发函数：全部 30 case 的可测核心（对应 C `environment_callback`，
/// minarch.c:1860-2158）
///
/// 只操作传入的本地 `state`/`runtime`，不碰全局——测试用本地实例构造。
/// case 分 6 组：A 常量/简单查询 19、B 选项体系 6、C 输入描述符 1、
/// D 换碟 2、E 振动 1、F 控制器信息 1。出参 case 在 `data` 非空时写入、
/// 空时跳过写入并正常返回（C 各 case 均有 `if (out)` 判空）。
///
/// 三处恒 false（与 C 原样一致，非偏离）：SET_INPUT_DESCRIPTORS
/// （C :1909）、GET_LOG_INTERFACE（Rust 无法稳定定义 C 可变参函数）、
/// SET_CONTROLLER_INFO（C :2007 TODO）。
///
/// # 参数
///
/// - `cmd`: `RETRO_ENVIRONMENT_*` 命令码
/// - `data`: 随命令语义变化的载荷指针（可为空）
/// - `state`: 注册后只读的目录状态
/// - `runtime`: 运行时可变状态（调用方持有）
///
/// # 返回值
///
/// 命令被处理返回 `true`，不支持/未知命令返回 `false`。
///
/// # Safety
///
/// 本函数按命令解引用 `data`，其有效性由调用方保证——libretro 规范
/// 规定核心传入与命令匹配的载荷：
///
/// - 出参 case（`bool*`/`u32*`/`i32*`/`*const c_char*`/结构体指针）：
///   `data` 为空，或指向对应类型的可写内存（本函数仅写入命令规定
///   的字节数）
/// - 数组 case（SET_INPUT_DESCRIPTORS/SET_CORE_OPTIONS/SET_VARIABLES）：
///   `data` 指向以约定哨兵终止的数组，且数组在调用期间有效（核心
///   回调期间保证）
/// - SET_CONTROLLER_INFO：`data` 指向 `RetroControllerInfo`，其
///   `types` 数组长度为 `num_types`
/// - SET_VARIABLE 双模式：`data` 指向 `RetroVariable`（key 非空）
///   或 `i32`（key 位置为空）。ABI 无法静态区分两种载荷（`i32` 4 字节
///   vs `RetroVariable` 16 字节），本函数读取 key 字段（指针宽度）——
///   调用方须保证至少指针宽度的可读内存（C 原版同样读取
///   `var->key`，minarch.c:2126，行为一致）
pub unsafe fn dispatch(
    cmd: u32,
    data: *mut c_void,
    state: &EnvironmentState,
    runtime: &mut EnvironmentRuntime,
) -> bool {
    match cmd {
        // ── A 组：常量与简单查询（19 case）──────────────────────
        RETRO_ENVIRONMENT_GET_OVERSCAN
        | RETRO_ENVIRONMENT_GET_CAN_DUPE
        | RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => {
            if !data.is_null() {
                // SAFETY: data 非空且 libretro 规范保证指向 bool* 出参
                unsafe { *(data as *mut bool) = true };
            }
            true
        }
        RETRO_ENVIRONMENT_SET_MESSAGE => {
            // C: LOG_info("%s\n", message->msg)（stdout）——println!
            // 等价（design 决策 7；minui main.rs:95 先例）
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroMessage
                let message = unsafe { &*(data as *const RetroMessage) };
                if !message.msg.is_null() {
                    println!("{}", c_string_to_string(message.msg));
                }
            }
            true
        }
        RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL => {
            // 无操作。C 此处无 break 落穿进 GET_SYSTEM_DIRECTORY 把
            // data 当 char** 野写 bios_dir（minarch.c:1885-1889）——
            // 缺陷不继承（design 决策 8）
            true
        }
        RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
            if !data.is_null() {
                // SAFETY: data 指向 *const c_char* 出参
                unsafe { *(data as *mut *const c_char) = state.bios_dir.as_ptr() };
            }
            true
        }
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
            // 仅支持 RGB565（C :1899）；data 为空时跳过读取正常返回
            // （C 无判空直接解引用，NULL 会崩溃——不继承）
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroPixelFormat
                if unsafe { *(data as *const RetroPixelFormat) } != RetroPixelFormat::Rgb565 {
                    return false;
                }
            }
            true
        }
        RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME
        | RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK
        | RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK
        | RETRO_ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER
        | RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY
        | RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE => {
            // C 各 case 均为空 body——no-op 应答
            true
        }
        RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => {
            if !data.is_null() {
                // SAFETY: data 指向 u32* 出参
                unsafe {
                    *(data as *mut u32) = (1 << RETRO_DEVICE_JOYPAD) | (1 << RETRO_DEVICE_ANALOG);
                }
            }
            true
        }
        RETRO_ENVIRONMENT_GET_LOG_INTERFACE => {
            // Rust 无法稳定定义 C 可变参函数（spike 结论）——不提供
            // 日志接口（design 决策 8）
            false
        }
        RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => {
            if !data.is_null() {
                // SAFETY: data 指向 *const c_char* 出参
                unsafe { *(data as *mut *const c_char) = state.saves_dir.as_ptr() };
            }
            true
        }
        RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE => {
            if !data.is_null() {
                // SAFETY: data 指向 i32* 出参
                unsafe {
                    *(data as *mut i32) = (RETRO_AV_ENABLE_VIDEO | RETRO_AV_ENABLE_AUDIO) as i32;
                }
            }
            true
        }
        RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION
        | RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION => {
            if !data.is_null() {
                // SAFETY: data 指向 u32* 出参
                unsafe { *(data as *mut u32) = 1 };
            }
            true
        }

        // ── B 组：选项体系（6 case）─────────────────────────────
        RETRO_ENVIRONMENT_GET_VARIABLE => {
            // 指针契约：var->value 写**指向 core_options 内
            // values[value] String 数据**的指针（不复制）——指针在
            // 下一次列表替换（SET_CORE_OPTIONS/_INTL/SET_VARIABLES）
            // 前有效；set_option_value/set_option_raw_value 只改索引
            // 不触碰字符串内容，列表替换前数据不会挪动。与 C 的契约
            // 完全一致（C 指针同样在 OptionList_reset 后失效）；核心
            // 须同步读取（libretro 规范）
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroVariable
                let var = unsafe { &mut *(data as *mut RetroVariable) };
                if !var.key.is_null() {
                    let key = c_string_to_string(var.key);
                    if let Some(value) = runtime.core_options.get_option_value(&key) {
                        var.value = value.as_ptr() as *const c_char;
                    }
                    // 未知 key 不写 value 字段（C getOptionValue 返回
                    // NULL 时不赋值）
                }
            }
            true
        }
        RETRO_ENVIRONMENT_SET_VARIABLES => {
            if !data.is_null() {
                // SAFETY: data 指向以 key == NULL 终止的 RetroVariable 数组
                runtime.core_options =
                    unsafe { OptionList::from_variables(data as *const RetroVariable) };
            }
            true
        }
        RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
            if !data.is_null() {
                // SAFETY: data 指向 bool* 出参
                unsafe { *(data as *mut bool) = runtime.core_options.changed };
                // 消费清零（C :1953-1954，仅 out 非空时清零）
                runtime.core_options.changed = false;
            }
            true
        }
        RETRO_ENVIRONMENT_SET_CORE_OPTIONS => {
            if !data.is_null() {
                // SAFETY: data 指向以 key == NULL 终止的
                // RetroCoreOptionDefinition 数组
                runtime.core_options = unsafe {
                    OptionList::from_core_options(data as *const RetroCoreOptionDefinition)
                };
            }
            true
        }
        RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL => {
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroCoreOptionsIntl
                let options = unsafe { &*(data as *const RetroCoreOptionsIntl) };
                if !options.us.is_null() {
                    // 仅 us 非空时替换（C :2044-2048）
                    // SAFETY: us 指向以 key == NULL 终止的数组
                    runtime.core_options = unsafe { OptionList::from_core_options(options.us) };
                }
            }
            true
        }
        RETRO_ENVIRONMENT_SET_VARIABLE => {
            // 双模式（C :2124-2137）：data 为 RetroVariable*（key 非空）
            // → 设置选项值；否则 data 为 i32* → 写 1 表示支持
            if !data.is_null() {
                // SAFETY: 见本函数 # Safety 的 SET_VARIABLE 条目
                let key_ptr = unsafe { *(data as *const *const c_char) };
                if !key_ptr.is_null() {
                    let key = c_string_to_string(key_ptr);
                    // SAFETY: key 非空时 data 指向完整 RetroVariable
                    let value_ptr = unsafe { (*(data as *const RetroVariable)).value };
                    let value = c_string_to_string(value_ptr);
                    runtime.core_options.set_option_value(&key, &value);
                } else {
                    // i32 模式：核心探测支持性——写 1 表示支持
                    // SAFETY: data 指向 i32 可写内存
                    unsafe { *(data as *mut i32) = 1 };
                }
            }
            true
        }

        // ── C 组：输入描述符（1 case）────────────────────────────
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS => {
            // 替换 controls_mapping（C Input_init 的映射段，
            // minarch.c:1796-1853）；bind 行 cfg 应用留装配层——
            // defaults 用默认表
            // SAFETY: data 指向以 description == NULL 终止的
            // RetroInputDescriptor 数组
            runtime.controls_mapping = unsafe {
                controls::init_core_mapping(
                    &controls::default_button_mapping(),
                    data as *const RetroInputDescriptor,
                )
            };
            // C :1909 恒返回 false（非偏离）
            false
        }

        // ── D 组：换碟（2 case）──────────────────────────────────
        RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE => {
            // v1 表：先清零再拷前 7 字段——保留 C 的"先清零防陈旧残留"
            // 语义（memset 同构；libretro.h 规定 ext-only 字段
            // Optional: not called if NULL，NULL 不会被核心调用）
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroDiskControlCallback
                let var = unsafe { &*(data as *const RetroDiskControlCallback) };
                // C memset 同构：先清零再拷前 7 字段。Rust 的 fn 指针
                // 非空不可表示，`MaybeUninit::zeroed().assume_init()`
                // 显式越过 `mem::zeroed` 的运行时校验（后者对含 fn 指针
                // 字段的类型 panic）——全零 fn 指针即 C NULL，libretro.h
                // 规定 ext-only 字段 Optional: not called if NULL，消费方
                // 不得调用（design 决策 5 已记录该风险）。invalid_value
                // 警告是这一 C 等价语义的必然代价，逐行 allow
                #[allow(invalid_value)]
                let mut ext: RetroDiskControlExtCallback =
                    unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
                ext.set_eject_state = var.set_eject_state;
                ext.get_eject_state = var.get_eject_state;
                ext.get_image_index = var.get_image_index;
                ext.set_image_index = var.set_image_index;
                ext.get_num_images = var.get_num_images;
                ext.replace_image_index = var.replace_image_index;
                ext.add_image_index = var.add_image_index;
                runtime.disc = Some(ext);
            }
            true
        }
        RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE => {
            // ext 表：10 字段全拷，last-wins
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroDiskControlExtCallback
                runtime.disc = Some(unsafe { *(data as *const RetroDiskControlExtCallback) });
            }
            true
        }

        // ── E 组：振动（1 case）──────────────────────────────────
        RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE => {
            if !data.is_null() {
                // SAFETY: data 指向 RetroRumbleInterface 出参
                unsafe {
                    (*(data as *mut RetroRumbleInterface)).set_rumble_state = rumble_trampoline
                };
            }
            true
        }

        // ── F 组：控制器信息（1 case）────────────────────────────
        RETRO_ENVIRONMENT_SET_CONTROLLER_INFO => {
            if !data.is_null() {
                // SAFETY: libretro 规范保证 data 指向 RetroControllerInfo
                // 且 types 数组长度为 num_types
                let infos = unsafe { &*(data as *const RetroControllerInfo) };
                if !infos.types.is_null() {
                    let types = unsafe {
                        core::slice::from_raw_parts(infos.types, infos.num_types as usize)
                    };
                    // 大小写不敏感扫描（C exactMatch 用 strncmp 区分
                    // 大小写——本处按 design 决策 2 大小写不敏感，偏离）
                    if types.iter().any(|t| {
                        !t.desc.is_null()
                            && c_string_to_string(t.desc).eq_ignore_ascii_case("dualshock")
                    }) {
                        runtime.has_custom_controllers = true;
                    }
                }
            }
            // C :2007 TODO 恒 false（非偏离）
            false
        }

        // 未识别命令
        _ => false,
    }
}
