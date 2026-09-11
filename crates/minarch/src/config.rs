//! 前端/核心配置（纯逻辑层）
//!
//! 本模块实现 minarch 配置体系的纯逻辑部分，对应原 C `minarch.c` 的：
//!
//! - `Option`/`OptionList` 数据模型（minarch.c:587-605）
//! - 核心选项注册表：`OptionList_init`/`OptionList_vars`/get/set 族
//!   （minarch.c:1422-1630）
//! - 前端选项表：`config.frontend` 8 个 `minarch_*` 选项（minarch.c:831-930）
//! - cfg 解析与序列化：`Config_getValue`/`Config_write` 选项段
//!   （minarch.c:943-961、:1274-1313）
//!
//! ## 边界（不在本模块）
//!
//! - cfg 文件 IO、`device_tag`、三级 cfg（system/default/user）优先级：
//!   后续 change（依赖装配层，仿 minui recents.rs 的参数化 IO 先例）
//! - `Config_syncFrontend`/`setOverclock` 平台联动：main.rs 装配层
//! - controls/shortcuts 按键映射（`ButtonMapping`）：controls.rs
//! - gamepad 表与 GB palette 特例：后续 change
//!
//! ## 与原 C 代码的对比
//!
//! - C 版用 calloc/strcpy 手工管理约 10 种指针（`OptionList_reset`
//!   是一段 free 迷宫）；Rust 版 `String`/`Vec` 自持所有权，释放由
//!   drop 自动完成，`reset` 函数随之消失
//! - C 版 `OptionList_vars` 在 value 无 `;` 时对 `NULL` 调 `strchr`
//!   而崩溃；Rust 版定义安全规则（见 [`OptionList::from_variables`]）
//! - C 版 `OptionList_setOptionValue` 对不匹配的值静默设为第一项；
//!   Rust 版忽略（见 [`OptionList::set_option_value`]）
//!

use core::ffi::c_char;
use std::ffi::CStr;

use crate::libretro::{RetroCoreOptionDefinition, RetroVariable};

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
// 数据模型
// ═══════════════════════════════════════════════════════════════

/// 单个配置选项（对应 C `typedef struct Option`，minarch.c:587-598）
///
/// 命名 `ConfigOption` 而非 C 的 `Option`：后者会遮蔽 Rust 预置的
/// `Option<T>` 枚举（本模块内写 `Option<String>` 会解析到本结构体），
/// 是 Rust 反模式。
pub struct ConfigOption {
    /// 选项键（如 `gambatte_colorization`）——CFG 文件行与核心
    /// `GET_VARIABLE` 查询使用的标识
    pub key: String,
    /// 显示名称；vars 路径下核心未提供名称时回退为 `key`（见
    /// `from_variables` 的文档）
    pub name: String,
    /// 简短描述（C `info` 字段）；核心未提供时为 `None`（菜单应
    /// 隐藏描述区域）
    pub desc: Option<String>,
    /// 完整描述（截断前的原始 `info` 文本）
    pub full: Option<String>,
    /// 默认值在 `values` 中的索引
    pub default_value: usize,
    /// 当前值在 `values` 中的索引
    pub value: usize,
    /// 可选值个数（等于 `values.len()`）
    pub count: usize,
    /// 是否被 cfg 文件锁定（`-key` 前缀；本期仅解析承载，
    /// 消费方为后续 menu change）
    pub lock: bool,
    /// 可选值（序列化到 cfg 文件的实际值）
    pub values: Vec<String>,
    /// 可选值的显示标签（核心未提供标签时回退为值本身）
    pub labels: Vec<String>,
}

/// 选项注册表（对应 C `typedef struct OptionList`，minarch.c:600-605）
///
/// 一组选项及其变更标记。`changed` 供 `GET_VARIABLE_UPDATE` 查询
/// （environment change 消费后清零，对应 C `config.core.changed`）。
pub struct OptionList {
    /// 选项列表（按核心上报顺序）
    pub options: Vec<ConfigOption>,
    /// 自上次 `GET_VARIABLE_UPDATE` 消费以来是否被修改
    pub changed: bool,
}

/// pcsx 特例键名覆盖表（对应 C `option_key_name`，minarch.c:1409-1413）
const OPTION_KEY_NAMES: &[(&str, &str)] =
    &[("pcsx_rearmed_analog_combo", "DualShock Toggle Combo")];

/// 键名到显示名的覆盖（对应 C `getOptionNameFromKey`，minarch.c:1413-1417）
///
/// # 参数
///
/// - `key`: 选项键
/// - `name`: 核心上报的默认显示名
///
/// # 返回值
///
/// 键在特例表中时返回覆盖名，否则返回原 `name`。
pub fn get_option_name_from_key<'a>(key: &str, name: &'a str) -> &'a str {
    OPTION_KEY_NAMES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(name, |(_, n)| n)
}

// ═══════════════════════════════════════════════════════════════
// 核心选项注册表
// ═══════════════════════════════════════════════════════════════

impl OptionList {
    /// 从核心选项定义数组构造（对应 C `OptionList_init`，minarch.c:1422-1496）
    ///
    /// 解析 `retro_core_option_definition` v1 布局数组（`SET_CORE_OPTIONS`/
    /// `SET_CORE_OPTIONS_INTL` 的载荷），深拷贝全部字符串为 Rust 所有。
    ///
    /// 解析规则：
    /// - 数组以 `key == NULL` 终止
    /// - `values` 以 `value == NULL` 终止；128 项全非 NULL 时安全截断
    ///   为 128 项（核心违反 ABI 约定，选项数据仍完整可用，不报错）
    /// - `info == NULL` 时 `desc`/`full` 为 `None`
    /// - label 为 NULL 时标签回退为值本身（C 同款回退）
    /// - `default_value` 不匹配任何值时索引为 0（C `Option_getValueIndex`
    ///   同款回退）
    ///
    /// # Safety
    ///
    /// - `defs` 必须指向以 `key == NULL` 终止的
    ///   [`RetroCoreOptionDefinition`] 数组（v1 布局）
    /// - 数组在函数执行期间必须保持有效——由核心在
    ///   `SET_CORE_OPTIONS`/`SET_CORE_OPTIONS_INTL` 回调期间保证
    /// - 本函数深拷贝所有字符串，返回后不持有数组的任何借用
    pub unsafe fn from_core_options(defs: *const RetroCoreOptionDefinition) -> OptionList {
        let mut options = Vec::new();
        if !defs.is_null() {
            let mut i = 0usize;
            loop {
                // SAFETY: 调用方保证 defs 为有效数组；终止项之前均有效
                let def = unsafe { &*defs.add(i) };
                if def.key.is_null() {
                    break;
                }
                let key = c_string_to_string(def.key);
                let name =
                    get_option_name_from_key(&key, &c_string_to_string(def.desc)).to_string();

                let mut values = Vec::new();
                let mut labels = Vec::new();
                for value in &def.values {
                    if value.value.is_null() {
                        break;
                    }
                    let value_str = c_string_to_string(value.value);
                    // C: label 为 NULL 时 labels[j] = values[j]
                    let label_str = if value.label.is_null() {
                        value_str.clone()
                    } else {
                        c_string_to_string(value.label)
                    };
                    values.push(value_str);
                    labels.push(label_str);
                }

                let count = values.len();
                let default_value = c_string_to_string(def.default_value);
                let default_index = values.iter().position(|v| *v == default_value).unwrap_or(0);

                let info = if def.info.is_null() {
                    None
                } else {
                    Some(c_string_to_string(def.info))
                };

                options.push(ConfigOption {
                    key,
                    name,
                    desc: info.clone(),
                    full: info,
                    default_value: default_index,
                    value: default_index,
                    count,
                    lock: false,
                    values,
                    labels,
                });
                i += 1;
            }
        }
        OptionList {
            options,
            changed: false,
        }
    }

    /// 从核心变量数组构造（对应 C `OptionList_vars`，minarch.c:1497-1553）
    ///
    /// 解析旧式 `SET_VARIABLES` 的 `retro_variable` 数组，value 格式为
    /// `"<显示名>; <值1>|<值2>|…"`。
    ///
    /// 解析规则（与 C 的三处差异见下）：
    /// - 仅严格 `"; "`（分号+空格）才切分显示名；其余字符全部归值
    /// - value 无 `;` 时：整串按 `|` 切分，`name` 回退为 `key`
    ///   ——**C 版此处对 `NULL` 调 `strchr` 崩溃，Rust 不继承**
    /// - `;` 后非空格时：`name` 回退为 `key`，值从 `;` 位置开始切分
    ///   （首值含前导 `;`，与 C 一致忠实呈现核心的格式错误）
    /// - 空串 value 产生 1 个空值（与 C 一致）
    /// - 无默认值概念：`default_value`/`value` 均为 0（与 C 一致）
    ///
    /// # Safety
    ///
    /// - `vars` 必须指向以 `key == NULL` 终止的 [`RetroVariable`] 数组
    /// - 数组在函数执行期间必须保持有效（核心回调期间保证）
    /// - 本函数深拷贝所有字符串，返回后不持有数组的任何借用
    pub unsafe fn from_variables(vars: *const RetroVariable) -> OptionList {
        let mut options = Vec::new();
        if !vars.is_null() {
            let mut i = 0usize;
            loop {
                // SAFETY: 调用方保证 vars 为有效数组；终止项之前均有效
                let var = unsafe { &*vars.add(i) };
                if var.key.is_null() {
                    break;
                }
                let key = c_string_to_string(var.key);
                let raw = c_string_to_string(var.value);

                // "Name; v1|v2" 解析（严格 `"; "` 切名）
                let (name, values_start) = match raw.find(';') {
                    Some(pos) if raw.as_bytes().get(pos + 1) == Some(&b' ') => {
                        (raw[..pos].to_string(), pos + 2)
                    }
                    Some(pos) => (key.clone(), pos),
                    None => (key.clone(), 0),
                };
                let values: Vec<String> = raw
                    .get(values_start..)
                    .unwrap_or("")
                    .split('|')
                    .map(|s| s.to_string())
                    .collect();

                let count = values.len();
                options.push(ConfigOption {
                    key,
                    name,
                    desc: None,
                    full: None,
                    default_value: 0,
                    value: 0,
                    count,
                    lock: false,
                    labels: values.clone(),
                    values,
                });
                i += 1;
            }
        }
        OptionList {
            options,
            changed: false,
        }
    }

    /// 按键查找选项（对应 C `OptionList_getOption`，minarch.c:1568-1574）
    ///
    /// # 返回值
    ///
    /// 找到返回 `Some(&ConfigOption)`，未知键返回 `None`。
    pub fn get_option(&self, key: &str) -> Option<&ConfigOption> {
        self.options.iter().find(|o| o.key == key)
    }

    /// 按键查当前值字符串（对应 C `OptionList_getOptionValue`，
    /// minarch.c:1591-1597）
    ///
    /// # 返回值
    ///
    /// 未知键返回 `None`（environment 的 `GET_VARIABLE` 据此把
    /// `RetroVariable.value` 留为 NULL，与 C 一致）。
    pub fn get_option_value(&self, key: &str) -> Option<&str> {
        self.get_option(key).map(|o| o.values[o.value].as_str())
    }

    /// 按索引设置当前值（对应 C `OptionList_setOptionRawValue`，
    /// minarch.c:1599-1609）
    ///
    /// 命中则更新 `value` 并置位 `changed`。索引越界或未知键时忽略
    /// ——C 版无越界检查会直接写坏 `value` 字段，Rust 不继承。
    pub fn set_option_raw_value(&mut self, key: &str, value: usize) {
        if let Some(option) = self.options.iter_mut().find(|o| o.key == key)
            && value < option.count
        {
            option.value = value;
            self.changed = true;
        }
    }

    /// 按值字符串设置当前值（对应 C `OptionList_setOptionValue`，
    /// minarch.c:1611-1620）
    ///
    /// 命中则更新 `value` 并置位 `changed`。值字符串不匹配任何可选值
    /// 时**忽略**——C 版 `Option_getValueIndex` 对不匹配返回 0 会静默
    /// 设为第一项，Rust 不继承该静默错误。
    pub fn set_option_value(&mut self, key: &str, value: &str) {
        if let Some(option) = self.options.iter_mut().find(|o| o.key == key)
            && let Some(index) = option.values.iter().position(|v| v == value)
        {
            option.value = index;
            self.changed = true;
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 前端选项表（对应 C `config.frontend`，minarch.c:831-930）
// ═══════════════════════════════════════════════════════════════

/// 开关标签（C `onoff_labels`，minarch.c:610-614）
const ONOFF_LABELS: &[&str] = &["Off", "On"];
/// 缩放标签（C `scaling_labels`，minarch.c:615-621）
const SCALING_LABELS: &[&str] = &["Native", "Aspect", "Fullscreen", "Cropped"];
/// 画面效果标签（C `effect_labels`，minarch.c:622-627）
const EFFECT_LABELS: &[&str] = &["None", "Line", "Grid"];
/// 锐度标签（C `sharpness_labels`，minarch.c:628-633）
const SHARPNESS_LABELS: &[&str] = &["Sharp", "Crisp", "Soft"];
/// 防撕裂标签（C `tearing_labels`，minarch.c:634-639）
const TEARING_LABELS: &[&str] = &["Off", "Lenient", "Strict"];
/// 最大快进标签（C `max_ff_labels`，minarch.c:640-650）
const MAX_FF_LABELS: &[&str] = &["None", "2x", "3x", "4x", "5x", "6x", "7x", "8x"];
/// 超频标签（C `overclock_labels`，minarch.c:788-792）
const OVERCLOCK_LABELS: &[&str] = &["Powersave", "Normal", "Performance"];

/// 无过扫描平台的缩放描述（C `getScreenScalingDesc` false 分支）
const SCALING_DESC_NO_OVERSCAN: &str = "Native uses integer scaling.\nAspect uses core reported aspect ratio.\nFullscreen has non-square pixels.";
/// 支持过扫描平台的缩放描述（C `getScreenScalingDesc` true 分支）
const SCALING_DESC_OVERSCAN: &str = "Native uses integer scaling. Aspect uses core\nreported aspect ratio. Fullscreen has non-square\npixels. Cropped is integer scaled then cropped.";
/// 画面效果描述（C `config.frontend[FE_OPT_EFFECT].desc`）
const EFFECT_DESC: &str = "Grid simulates an LCD grid.\nLine simulates CRT scanlines.\nEffects usually look best at native scaling.";
/// 锐度描述（C `config.frontend[FE_OPT_SHARPNESS].desc`）
const SHARPNESS_DESC: &str = "Sharp uses nearest neighbor sampling.\nCrisp integer upscales before linear sampling.\nSoft uses linear sampling.";
/// 防撕裂描述（C `config.frontend[FE_OPT_TEARING].desc`）
const TEARING_DESC: &str = "Wait for vsync before drawing the next frame.\nLenient only waits when within frame budget.\nStrict always waits.";
/// 超频描述（C `config.frontend[FE_OPT_OVERCLOCK].desc`）
const CPU_SPEED_DESC: &str =
    "Over- or underclock the CPU to prioritize\npure performance or power savings.";
/// 音频优先描述（C `config.frontend[FE_OPT_THREAD].desc`）
const THREAD_VIDEO_DESC: &str =
    "Can eliminate crackle but\nmay cause dropped frames.\nOnly turn on if necessary.";
/// 调试 HUD 描述（C `config.frontend[FE_OPT_DEBUG].desc`）
const DEBUG_HUD_DESC: &str =
    "Show frames per second, cpu load,\nresolution, and scaler information.";
/// 最大快进描述（C `config.frontend[FE_OPT_MAXFF].desc`）
const MAX_FF_SPEED_DESC: &str = "Fast forward will not exceed the\nselected speed (but may be less\ndepending on game and emulator).";

/// 构造单个前端选项
///
/// # 参数
///
/// - `key`/`name`/`desc`: 键、显示名、描述
/// - `default_value`: 默认值索引
/// - `labels`: 标签表（同时也是序列化值，C 版 values 与 labels 同表）
fn build_frontend_option(
    key: &str,
    name: &str,
    desc: &str,
    default_value: usize,
    labels: &[&str],
) -> ConfigOption {
    let values: Vec<String> = labels.iter().map(|s| (*s).to_string()).collect();
    ConfigOption {
        key: key.to_string(),
        name: name.to_string(),
        desc: Some(desc.to_string()),
        full: Some(desc.to_string()),
        default_value,
        value: default_value,
        count: values.len(),
        lock: false,
        labels: values.clone(),
        values,
    }
}

/// 构造 8 个前端选项（对应 C `config.frontend`，minarch.c:831-930）
///
/// 键名/默认值/描述文案与 C 完全一致。
///
/// # 参数
///
/// - `supports_overscan`: 平台是否支持过扫描（`Platform::SUPPORTS_OVERSCAN`，
///   由装配层传入）——决定 scaling 选项的项数（3 或 4）与描述文案
///
/// # 返回值
///
/// 8 个选项：scaling/effect/sharpness/tearing/cpu_speed/thread_video/
/// debug_hud/max_ff_speed（与 C `FE_OPT_*` 顺序一致）。
pub fn frontend_options(supports_overscan: bool) -> Vec<ConfigOption> {
    let (scaling_labels, scaling_desc): (&[&str], &str) = if supports_overscan {
        (SCALING_LABELS, SCALING_DESC_OVERSCAN)
    } else {
        (&SCALING_LABELS[..3], SCALING_DESC_NO_OVERSCAN)
    };

    vec![
        build_frontend_option(
            "minarch_screen_scaling",
            "Screen Scaling",
            scaling_desc,
            1,
            scaling_labels,
        ),
        build_frontend_option(
            "minarch_screen_effect",
            "Screen Effect",
            EFFECT_DESC,
            0,
            EFFECT_LABELS,
        ),
        build_frontend_option(
            "minarch_screen_sharpness",
            "Screen Sharpness",
            SHARPNESS_DESC,
            2,
            SHARPNESS_LABELS,
        ),
        build_frontend_option(
            "minarch_prevent_tearing",
            "Prevent Tearing",
            TEARING_DESC,
            1,
            TEARING_LABELS,
        ),
        build_frontend_option(
            "minarch_cpu_speed",
            "CPU Speed",
            CPU_SPEED_DESC,
            1,
            OVERCLOCK_LABELS,
        ),
        build_frontend_option(
            "minarch_thread_video",
            "Prioritize Audio",
            THREAD_VIDEO_DESC,
            0,
            ONOFF_LABELS,
        ),
        build_frontend_option(
            "minarch_debug_hud",
            "Debug HUD",
            DEBUG_HUD_DESC,
            0,
            ONOFF_LABELS,
        ),
        build_frontend_option(
            "minarch_max_ff_speed",
            "Max FF Speed",
            MAX_FF_SPEED_DESC,
            3,
            MAX_FF_LABELS,
        ),
    ]
}

// ═══════════════════════════════════════════════════════════════
// cfg 解析与序列化（对应 C `Config_getValue`，minarch.c:943-961）
// ═══════════════════════════════════════════════════════════════

/// cfg 文本中一次键值查询的结果
pub struct CfgValue {
    /// 值（行尾 `\n`/`\r` 已截断）
    pub value: String,
    /// 键是否被锁定（键前 `-` 前缀，如 `-minarch_cpu_speed = 2`）
    pub lock: bool,
}

/// cfg 序列化条目（对应 C `Config_write` 的 `fprintf("%s = %s\n")` 行）
pub struct CfgEntry {
    /// 键
    pub key: String,
    /// 值
    pub value: String,
}

/// 在 cfg 文本中查找键值（对应 C `Config_getValue`，minarch.c:943-961）
///
/// # 规则（与 C 一致）
///
/// - 键前一个字符为 `-` 时标记 `lock == true`
/// - `" = "` 严格分隔：`"key=value"` 不匹配
/// - 重复 key 取第一个匹配
/// - 值以 `\n` 或 `\r` 截断
///
/// # 参数
///
/// - `cfg`: cfg 文件全文（纯文本，换行可以是 LF 或 CRLF）
/// - `key`: 要查询的键
///
/// # 返回值
///
/// 匹配返回 `Some(CfgValue)`，未匹配返回 `None`。
pub fn get_cfg_value(cfg: &str, key: &str) -> Option<CfgValue> {
    let mut rest = cfg;
    while let Some(pos) = rest.find(key) {
        let lock = rest[..pos].ends_with('-');
        let after_key = &rest[pos + key.len()..];
        if let Some(value_rest) = after_key.strip_prefix(" = ") {
            let value = value_rest
                .split(['\n', '\r'])
                .next()
                .unwrap_or("")
                .to_string();
            return Some(CfgValue { value, lock });
        }
        // 未匹配 " = "，从键之后继续搜索（C 的 strstr 循环同款语义）
        rest = &rest[pos + key.len()..];
    }
    None
}

/// 将条目序列化为 cfg 文本（对应 C `Config_write` 的选项段，minarch.c:1284-1307）
///
/// 每行 `key = value\n`，与 [`get_cfg_value`] 可逆（round-trip）。
pub fn serialize_cfg(entries: &[CfgEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        out.push_str(&entry.key);
        out.push_str(" = ");
        out.push_str(&entry.value);
        out.push('\n');
    }
    out
}

// ═══════════════════════════════════════════════════════════════
// cfg 文件 IO 层（装配层闭环：三级 cfg 的读写，仿 recents 参数化 IO）
// ═══════════════════════════════════════════════════════════════

/// 读取 cfg 文件全文（对应 C `allocFile`，minarch.c:1207-1256 的读取）
///
/// 文件缺失或读取失败返回 `None`（对应 C `fopen` 失败静默回退），
/// 不区分错误类型——装配层按"无此文件"处理。
///
/// # 参数
///
/// - `path`: cfg 文件路径（三级：system/default/user 之一）
///
/// # 返回值
///
/// - `Some(text)`: 文件内容（空文件返回 `Some("")`）
/// - `None`: 文件不存在或不可读
pub fn load_cfg_text(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// 将条目序列化并写回 cfg 文件（对应 C `Config_write` 的写文件段，
/// minarch.c:1284-1313）
///
/// 截断创建（对应 C `fopen(path, "wb")`），全量写入后 `sync_all()`
/// （对应 C 写后 `sync()` 的窄化偏离——与 sram/savestate 同偏离，
/// 只刷本文件）。
///
/// # 参数
///
/// - `entries`: 要序列化的条目（frontend + core 选项 + gamepad + binds）
/// - `path`: 写回目标路径（user 级：游戏级或控制台级 cfg）
///
/// # 返回值
///
/// - `Ok(())`: 写入并同步成功
/// - `Err`: 打开/写入/同步失败（如目录缺失）
pub fn save_cfg(entries: &[CfgEntry], path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let text = serialize_cfg(entries);
    let mut file = std::fs::File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// 将一份 cfg 文本应用到选项表与按键映射（对应 C `Config_readOptionsString`
/// + `Config_readControlsString` 的合并语义，minarch.c:1108-1192）
///
/// 应用顺序与 C 一致：frontend 8 键（含 `Config_syncFrontend` 副作用由
/// 调用方处理，本函数只更新值）→ `minarch_gamepad_type` → core 选项 →
/// controls bind 行 → shortcuts bind 行。无效行（键不匹配、值不匹配、
/// bind 名未知名）静默忽略，不中断加载。
///
/// # 参数
///
/// - `cfg`: cfg 文件全文
/// - `frontend`: 前端选项表（`minarch_*` 8 键）
/// - `core`: 核心选项表
/// - `mapping`: 按键映射（bind 行按 name 匹配更新 local/mod_）
/// - `shortcuts`: 快捷指令表（bind 行按 name 匹配更新）
/// - `gamepad_type`: 出参——cfg 含 `minarch_gamepad_type` 时写出数值
///
/// # 返回值
///
/// 无（失败静默，与 C `continue` 语义一致）
pub fn apply_cfg(
    cfg: &str,
    frontend: &mut OptionList,
    core: &mut OptionList,
    mapping: &mut [crate::controls::ButtonMapping],
    shortcuts: &mut [crate::controls::Shortcut],
    gamepad_type: &mut u32,
) {
    // frontend 选项：Config_readOptionsString 的 frontend 段（minarch.c:1110-1121）
    for i in 0..frontend.options.len() {
        let key = frontend.options[i].key.clone();
        if let Some(value) = get_cfg_value(cfg, &key) {
            frontend.set_option_value(&key, &value.value);
        }
    }
    // gamepad_type（minarch.c:1122-1125，仅 has_custom_controllers 时 C 才应用；
    // Rust 侧统一解析——选项菜单消费方未实现，值先承载）
    if let Some(value) = get_cfg_value(cfg, "minarch_gamepad_type")
        && let Ok(n) = value.value.parse::<u32>()
    {
        *gamepad_type = n;
    }
    // core 选项（minarch.c:1126-1130）
    for i in 0..core.options.len() {
        let key = core.options[i].key.clone();
        if let Some(value) = get_cfg_value(cfg, &key) {
            core.set_option_value(&key, &value.value);
        }
    }
    // controls bind 行（Config_readControlsString 的 controls 段，minarch.c:1138-1170）
    for item in mapping.iter_mut() {
        let key = format!("bind {}", item.name);
        let Some(value) = get_cfg_value(cfg, &key) else {
            continue;
        };
        // C 会去掉 `:xxx` 绑定残留（default.cfg 的 remap 产物）
        let label = value.value.split(':').next().unwrap_or("");
        if let Some((local, mod_)) = crate::controls::parse_bind_button(label) {
            item.local = local;
            item.mod_ = mod_;
        }
    }
    // shortcuts bind 行（Config_readControlsString 的 shortcuts 段，minarch.c:1172-1192）
    for item in shortcuts.iter_mut() {
        let key = format!("bind {}", item.name);
        let Some(value) = get_cfg_value(cfg, &key) else {
            continue;
        };
        if let Some((local, mod_)) = crate::controls::parse_bind_button(&value.value) {
            item.local = local;
            item.mod_ = mod_;
        }
    }
}
