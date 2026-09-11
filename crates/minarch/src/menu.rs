//! 游戏内菜单 UI：主菜单状态机、选项子菜单框架、存档交互编排、菜单绘制
//!
//! 对应原 C `minarch.c` 的菜单区（`Menu_loop`/`Menu_options`/
//! `Menu_saveState`/`Menu_loadState`，minarch.c:3006-4572）。
//!
//! 本模块是 MinUI 游戏内暂停菜单：游戏运行中按 MENU 键弹出，提供
//! Continue/Save/Load/Options/Quit 五项主操作，其中 Options 下钻到
//! 可配置的选项子菜单（前端/模拟器/按键/快捷键/保存更改）。
//!
//! ## 设计（与 C 的差异）
//!
//! C 版是两个嵌套的**阻塞式循环**（`Menu_loop` 内嵌 `Menu_options`），
//! 直接操作全局 SDL surface 与 PAD 状态。Rust 版改为**纯逻辑状态机 +
//! 注入式 IO**：
//!
//! - 按键状态经 [`MenuInput`] 由装配层（`main.rs`）逐帧注入，菜单
//!   本身不轮询 PAD
//! - 绘制是纯函数（`draw_main_menu`/`draw_option_menu`），操作
//!   `common::video::VideoBuffer`，不接触 SDL/Platform
//! - 存档交互只做**文件层编排**（记忆文件/路径/存在性查询），序列化
//!   FFI 调用归装配层
//!
//! ## 依赖方向
//!
//! 本模块 SHALL 只依赖 `common`（`video`/`utils`/`paths`）、`render`
//! （绘制原语）、`crate::savestate`（`state_path`）——不依赖
//! `config`/`controls`/`environment`/`core`（选项列表与按键绑定由
//! 装配层构建后传入 [`MenuItem`]）。
//!

use common::utils::{exists, get_file, get_int, put_file, put_int};
use common::video::{Rect, VideoBuffer};
use render::asset::Atlas;
use render::pill::blit_pill;
use render::text::{Font, render_text, size_text, truncate_text};

// ═══════════════════════════════════════════════════════════════
// 主菜单状态机（MainMenu）
// ═══════════════════════════════════════════════════════════════

/// 主菜单固定项数（对应 C `MENU_ITEM_COUNT 5`，minarch.c:3008）
pub const MENU_ITEM_COUNT: usize = 5;
/// 存档槽位数（对应 C `MENU_SLOT_COUNT 8`，minarch.c:3009）
pub const MENU_SLOT_COUNT: usize = 8;

/// 主菜单项下标（对应 C `ITEM_*` 枚举，minarch.c:3011-3017）
pub mod item {
    /// Continue：继续游戏（多碟时先换碟）
    pub const CONT: usize = 0;
    /// Save：保存到当前槽位
    pub const SAVE: usize = 1;
    /// Load：从当前槽位读档
    pub const LOAD: usize = 2;
    /// Options：打开选项子菜单（simple_mode 下为 Reset）
    pub const OPTS: usize = 3;
    /// Quit：退出游戏
    pub const QUIT: usize = 4;
}

/// 主菜单 A 键分派结果（对应 C `STATUS_*` 枚举，minarch.c:3020-3027）
///
/// 装配层消费此枚举执行实际动作（换碟/存档序列化/读档序列化/退出等）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainAction {
    /// 继续游戏（对应 `STATUS_CONT`）
    Continue,
    /// 请求存档（对应 `STATUS_SAVE`；装配层执行序列化）
    Save,
    /// 请求读档（对应 `STATUS_LOAD`；装配层执行反序列化）
    Load,
    /// 打开选项子菜单（对应 `STATUS_OPTS`）
    Options,
    /// simple_mode 复位游戏（对应 `STATUS_RESET`）
    Reset,
    /// 换碟后继续游戏（对应 `STATUS_DISC`）
    DiscChange,
    /// 退出游戏（对应 `STATUS_QUIT`）
    Quit,
}

/// 单帧按键输入（装配层从 PAD 收集后传入）
///
/// 字段语义由装配层保证：`up`/`down`/`left`/`right` 为「按住重复」
/// 语义（对应 C `PAD_justRepeated`），`a`/`b`/`x`/`menu` 为「刚按下」
/// 语义（对应 C `PAD_justPressed`）。菜单状态机不感知原始位掩码。
#[derive(Debug, Default, Clone, Copy)]
pub struct MenuInput {
    /// 上方向（重复）
    pub up: bool,
    /// 下方向（重复）
    pub down: bool,
    /// 左方向（重复）
    pub left: bool,
    /// 右方向（重复）
    pub right: bool,
    /// A 键（刚按下）
    pub a: bool,
    /// B 键（刚按下）
    pub b: bool,
    /// X 键（刚按下）
    pub x: bool,
    /// MENU 键（刚按下）
    pub menu: bool,
}

/// 主菜单状态机（对应 C `Menu_loop` 的导航/分派段，minarch.c:4289-4391）
///
/// 收拢 C 的全局状态（`selected`/`menu.disc`/`menu.slot`/`simple_mode`），
/// 提供逐帧推进的 `update()`。碟片路径表由装配层持有——本结构体只存
/// 碟片计数与当前下标，装配层消费 [`MainAction::DiscChange`] 时自行
/// 解析路径。
#[derive(Debug, Clone)]
pub struct MainMenu {
    /// 选中项下标 0..4（`item::CONT`..`item::QUIT`）
    pub selected: usize,
    /// 当前碟片下标（多碟游戏；单碟恒为 0）
    pub disc: usize,
    /// 当前存档槽位 0..7（对应 C `menu.slot`）
    pub slot: usize,
    /// 是否 simple_mode（Options 项显示 Reset 且 A 确认直接复位）
    pub simple_mode: bool,
    /// 碟片总数（1 = 单碟；对应 C `menu.total_discs`）
    pub disc_count: usize,
}

impl MainMenu {
    /// 创建主菜单状态机
    ///
    /// # 参数
    ///
    /// - `simple_mode`: simple_mode 标志（装配层从启动参数判定，对应
    ///   C 全局 `simple_mode`，minarch.c:27）
    /// - `disc_count`: 碟片总数（装配层从 m3u 探测结果传入；单碟传 1）
    pub fn new(simple_mode: bool, disc_count: usize) -> Self {
        Self {
            selected: item::CONT,
            disc: 0,
            slot: 0,
            simple_mode,
            disc_count,
        }
    }

    /// 施加一帧输入，推进状态并返回分派结果
    ///
    /// # 参数
    ///
    /// - `input`: 本帧按键状态（装配层收集）
    ///
    /// # 返回值
    ///
    /// - `Some(action)`: 本帧触发了一次 A 键分派（`Continue`/`Save`/
    ///   `Load`/`Options`/`Reset`/`DiscChange`/`Quit`）或 B 键
    ///   （`Continue`）——装配层据此执行动作
    /// - `None`: 无动作（导航/换碟/换槽位，或空闲）
    ///
    /// 优先级与 C 一致：UP/DOWN 导航 → LEFT/RIGHT 换碟/换槽位 →
    /// B 返回 → A 分派。同帧多键时导航优先于分派。
    pub fn update(&mut self, input: &MenuInput) -> Option<MainAction> {
        if input.up {
            self.selected = (self.selected + MENU_ITEM_COUNT - 1) % MENU_ITEM_COUNT;
        } else if input.down {
            self.selected = (self.selected + 1) % MENU_ITEM_COUNT;
        } else if input.left {
            self.step(-1);
        } else if input.right {
            self.step(1);
        } else if input.b {
            return Some(MainAction::Continue);
        } else if input.a {
            return Some(self.confirm());
        }
        None
    }

    /// LEFT/RIGHT 的上下文动作：Continue 项换碟、Save/Load 项换槽位
    fn step(&mut self, dir: i32) {
        match self.selected {
            item::CONT if self.disc_count > 1 => {
                self.disc = (self.disc as i32 + dir).rem_euclid(self.disc_count as i32) as usize;
            }
            item::SAVE | item::LOAD => {
                self.slot = (self.slot as i32 + dir).rem_euclid(MENU_SLOT_COUNT as i32) as usize;
            }
            _ => {}
        }
    }

    /// A 键按当前选中项分派（对应 C switch(selected)，minarch.c:4334-4389）
    fn confirm(&mut self) -> MainAction {
        match self.selected {
            item::CONT => {
                if self.disc_count > 1 && self.disc != 0 {
                    MainAction::DiscChange
                } else {
                    MainAction::Continue
                }
            }
            item::SAVE => MainAction::Save,
            item::LOAD => MainAction::Load,
            item::OPTS => {
                if self.simple_mode {
                    MainAction::Reset
                } else {
                    MainAction::Options
                }
            }
            _ => MainAction::Quit,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 选项子菜单框架（OptionMenu）
// ═══════════════════════════════════════════════════════════════

/// 菜单项类型（对应 C `MENU_LIST`/`MENU_VAR`/`MENU_FIXED`/`MENU_INPUT`
/// 枚举，minarch.c:3146-3151）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// 普通列表项（如存档/主菜单子项；A 确认走回调或下钻）
    List,
    /// 值可变项（前端选项；LEFT/RIGHT 循环值表）
    Var,
    /// 固定项（模拟器选项；整行灰底 + 右侧值）
    Fixed,
    /// 输入绑定项（按键/快捷键；A 进入 await_input 绑定流程）
    Input,
}

/// A 键确认动作（对应 C `MenuItem.on_confirm` 与列表级 `on_confirm`，
/// minarch.c:3691-3698 的分发优先级）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// 下钻到子菜单（`usize` 为子菜单下标，装配层维护子菜单表）
    OpenSubmenu(usize),
    /// 触发动作（如保存配置/退出；装配层消费）
    Request(MainAction),
    /// 进入 await_input 绑定流程（MENU_INPUT 专用）
    Bind,
}

/// LEFT/RIGHT 修改值后的回调动作（对应 C `on_change`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeAction {
    /// 应用配置（装配层执行 `Config_syncFrontend` 等）
    ApplyConfig,
    /// 清除绑定（MENU_INPUT 的 X 键）
    ClearBinding,
}

/// 单个菜单项（对应 C `struct MenuItem`，minarch.c:3134-3144）
#[derive(Debug, Clone)]
pub struct MenuItem {
    /// 显示名称（C `name`）
    pub name: String,
    /// 描述文字（C `desc`；选中行显示在底部）
    pub desc: Option<String>,
    /// LEFT/RIGHT 循环的值表（C `values`；空 = 不可修改）
    pub values: Vec<String>,
    /// 选项键（C `key`，供配置联动；非选项项为 `None`）
    pub key: Option<String>,
    /// 绑定 id（C `id`，供 controls 联动；非绑定项为 0）
    pub id: usize,
    /// 当前值下标（C `value`）
    pub value: usize,
    /// 项类型（决定渲染与交互语义）
    pub kind: ItemKind,
    /// A 键确认动作（C `on_confirm`）
    pub on_confirm: Option<ConfirmAction>,
    /// 值修改回调（C `on_change`）
    pub on_change: Option<ChangeAction>,
}

impl MenuItem {
    /// 创建普通列表项
    pub fn list(name: &str) -> Self {
        Self {
            name: name.to_string(),
            desc: None,
            values: Vec::new(),
            key: None,
            id: 0,
            value: 0,
            kind: ItemKind::List,
            on_confirm: None,
            on_change: None,
        }
    }

    /// 创建值可变项（`values` 为可选项表，`value` 为当前下标）
    pub fn var(name: &str, values: Vec<String>, value: usize) -> Self {
        Self {
            name: name.to_string(),
            desc: None,
            values,
            key: None,
            id: 0,
            value,
            kind: ItemKind::Var,
            on_confirm: None,
            on_change: None,
        }
    }

    /// 当前值文本（`values` 非空时取 `values[value]`，否则空串）
    pub fn value_text(&self) -> &str {
        self.values
            .get(self.value)
            .map(String::as_str)
            .unwrap_or("")
    }
}

/// 选项菜单单帧事件（装配层消费）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionMenuEvent {
    /// 无动作
    None,
    /// A 键确认了选中项——装配层执行 `on_confirm`（绑定记录等）
    Confirm(usize),
    /// 值已修改（LEFT/RIGHT）——装配层执行 `on_change`
    Change(usize),
    /// B 键返回上一级（装配层关闭子菜单或退出选项）
    Close,
    /// await_input 完成后的选中项推进（装配层调 `finish_await` 后返回）
    Advance,
}

/// 选项子菜单状态机（对应 C `Menu_options`，minarch.c:3586-3970）
///
/// 收拢 C 的滚动窗口（`selected`/`start`/`end`）、`await_input`/
/// `defer_menu` 标志与列表级回调。`items` 由装配层从
/// `config::OptionList`/`controls` 表构建（`lock` 过滤见
/// [`OptionMenu::from_options`]）。
#[derive(Debug, Clone)]
pub struct OptionMenu {
    /// 菜单项列表
    pub items: Vec<MenuItem>,
    /// 选中项下标
    pub selected: usize,
    /// 滚动窗口首项下标（含）
    pub start: usize,
    /// 滚动窗口末项下标（不含）
    pub end: usize,
    /// 可见行数（装配层注入，对应 C `max_visible_options`，
    /// minarch.c:3596）
    pub max_visible: usize,
    /// 是否处于 await_input 绑定等待状态（对应 C `await_input`）
    pub awaiting: bool,
    /// 列表级确认回调（C `list->on_confirm`，MENU_INPUT 绑定用）
    pub on_confirm: Option<ConfirmAction>,
    /// 列表级描述（C `list->desc`）
    pub desc: Option<String>,
}

impl OptionMenu {
    /// 创建选项菜单
    ///
    /// # 参数
    ///
    /// - `items`: 菜单项（装配层构建，已过滤锁定项）
    /// - `max_visible`: 可见行数（`screen_h` 推算，装配层注入）
    /// - `on_confirm`: 列表级确认回调（MENU_INPUT 的按键绑定流程用）
    /// - `desc`: 列表级描述（显示在底部）
    pub fn new(
        items: Vec<MenuItem>,
        max_visible: usize,
        on_confirm: Option<ConfirmAction>,
        desc: Option<String>,
    ) -> Self {
        let count = items.len();
        let end = count.min(max_visible);
        Self {
            items,
            selected: 0,
            start: 0,
            end,
            max_visible,
            awaiting: false,
            on_confirm,
            desc,
        }
    }

    /// 从配置选项构建菜单项（对应 C `OptionFrontend_openMenu`/
    /// `OptionEmulator_openMenu` 的 `item->lock` 跳过，minarch.c:3211-3218
    /// /:3275-3282——半环清单 config「Option.lock 消费」闭环）
    ///
    /// # 参数
    ///
    /// - `options`: 配置选项表（`config::OptionList::options` 或其子集）
    /// - `kind`: 构建出的项类型（前端选项用 `Var`，模拟器选项用 `Fixed`）
    ///
    /// # 返回值
    ///
    /// 过滤掉 `lock == true` 项后的菜单项列表（顺序保持）。
    pub fn from_options(options: &[crate::config::ConfigOption], kind: ItemKind) -> Vec<MenuItem> {
        options
            .iter()
            .filter(|o| !o.lock)
            .map(|o| MenuItem {
                name: o.name.clone(),
                desc: o.desc.clone(),
                values: o.labels.clone(),
                key: Some(o.key.clone()),
                id: 0,
                value: o.value,
                kind,
                on_confirm: None,
                on_change: Some(ChangeAction::ApplyConfig),
            })
            .collect()
    }

    /// 施加一帧输入，推进状态并返回事件
    ///
    /// # 参数
    ///
    /// - `input`: 本帧按键状态
    ///
    /// # 返回值
    ///
    /// 本帧产生的事件（`Confirm`/`Change`/`Close`/`Advance`/`None`）。
    /// `awaiting == true` 时 SHALL 跳过全部输入处理并返回 `Advance`
    /// （对应 C「await_input 时 defer_menu、下一帧执行 on_confirm」，
    /// minarch.c:3609-3625）。
    pub fn update(&mut self, input: &MenuInput) -> OptionMenuEvent {
        if self.awaiting {
            return OptionMenuEvent::Advance;
        }
        if input.up {
            self.move_up();
        } else if input.down {
            self.move_down();
        } else if input.left {
            return self.change_value(-1);
        } else if input.right {
            return self.change_value(1);
        } else if input.b {
            return OptionMenuEvent::Close;
        } else if input.a {
            return self.confirm();
        } else if input.x {
            let item = &self.items[self.selected];
            if item.kind == ItemKind::Input {
                self.clear_binding();
                self.move_down();
                return OptionMenuEvent::Change(self.selected);
            }
        }
        OptionMenuEvent::None
    }

    /// UP：选中项上移，窗口滑动；顶部回绕到末项并对齐末页
    /// （对应 C :3630-3641）
    fn move_up(&mut self) {
        let count = self.items.len();
        if self.selected == 0 {
            self.selected = count - 1;
            self.start = count.saturating_sub(self.max_visible);
            self.end = count;
        } else {
            self.selected -= 1;
            if self.selected < self.start {
                self.start -= 1;
                self.end -= 1;
            }
        }
    }

    /// DOWN：选中项下移，窗口滑动；底部回绕到首项并对齐首页
    /// （对应 C :3643-3654）
    fn move_down(&mut self) {
        let count = self.items.len();
        if self.selected + 1 >= count {
            self.selected = 0;
            self.start = 0;
            self.end = count.min(self.max_visible);
        } else {
            self.selected += 1;
            if self.selected >= self.end {
                self.start += 1;
                self.end += 1;
            }
        }
    }

    /// LEFT/RIGHT 修改选中项的值（`values` 表循环，对应 C :3658-3681）
    ///
    /// 值表为空（`List`/无值项）时无操作。修改后返回 `Change` 事件
    /// 供装配层执行 `on_change`。
    fn change_value(&mut self, dir: i32) -> OptionMenuEvent {
        let item = &mut self.items[self.selected];
        if item.values.is_empty() {
            return OptionMenuEvent::None;
        }
        let len = item.values.len() as i32;
        item.value = (item.value as i32 + dir).rem_euclid(len) as usize;
        OptionMenuEvent::Change(self.selected)
    }

    /// A 键确认分发（对应 C :3688-3698 的优先级：item 回调 → submenu
    /// 下钻 → list 回调）
    fn confirm(&mut self) -> OptionMenuEvent {
        let item = &self.items[self.selected];
        match &item.on_confirm {
            Some(ConfirmAction::Bind) => {
                self.awaiting = true;
                OptionMenuEvent::Confirm(self.selected)
            }
            Some(_) => OptionMenuEvent::Confirm(self.selected),
            None => {
                // 无 item 回调：MENU_INPUT 且值为绑定表时进入 await_input
                if item.kind == ItemKind::Input && !item.values.is_empty() {
                    self.awaiting = true;
                    return OptionMenuEvent::Confirm(self.selected);
                }
                if self.on_confirm.is_some() {
                    // 列表级回调：仅 MENU_INPUT 走 await_input（C :3696）
                    OptionMenuEvent::Confirm(self.selected)
                } else {
                    OptionMenuEvent::None
                }
            }
        }
    }

    /// X 键清除绑定（对应 C `OptionControls_unbind`/`OptionShortcuts_unbind`，
    /// minarch.c:3354-3362/:3467-3473）
    fn clear_binding(&mut self) {
        let item = &mut self.items[self.selected];
        item.value = 0;
    }

    /// await_input 绑定完成后的推进（对应 C「await_input 时 defer_menu、
    /// 下一帧先执行 on_confirm 再推进」，minarch.c:3609-3625/:3701-3713）
    ///
    /// 装配层在收到 `Confirm` 事件并完成实际绑定记录后调用本函数：
    /// 清除等待状态并下移选中项（回绕语义与 DOWN 一致）。
    pub fn finish_await(&mut self) {
        self.awaiting = false;
        self.move_down();
    }
}

// ═══════════════════════════════════════════════════════════════
// 存档交互编排（SaveStateIo）
// ═══════════════════════════════════════════════════════════════

/// 换碟请求（读档时记忆碟片 ≠ 当前碟片，对应 C `Game_changeDisc`
/// 调用点，minarch.c:4164-4176——实际换碟由装配层执行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscRequest {
    /// 目标碟片绝对路径
    pub path: String,
}

/// 菜单存档交互的文件层编排（对应 C `Menu_saveState`/`Menu_loadState`
/// 的文件部分，minarch.c:4135-4182）
///
/// 承载「记忆文件读写 + 路径拼装 + 存在性查询」，序列化 FFI 调用
/// （`serialize_size`/`serialize`/`unserialize`）、`state_slot` 全局切换、
/// 快进门控 SHALL NOT 在本模块——装配层收到存档/读档请求后执行
/// （半环清单 savestate「序列化 FFI 数据收集」行）。
#[derive(Debug, Clone)]
pub struct SaveStateIo {
    /// 游戏专属存档目录（`{shared_userdata}/.minui/{emu_name}`，
    /// 对应 C `menu.minui_dir`，minarch.c:3068）
    pub minui_dir: String,
    /// 状态快照目录（`Core::open` 的 `states_dir`）
    pub states_dir: String,
    /// 游戏名（**完整文件名含扩展名**，同 savestate 契约）
    pub game_name: String,
    /// 碟片绝对路径表（多碟；单碟为空表）
    pub disc_paths: Vec<String>,
    /// 碟片路径基准目录（多碟时 `.txt` 记忆为相对此基准的路径，
    /// 对应 C `menu.base_path`，minarch.c:3077）
    pub base_path: String,
}

/// slot 记忆文件路径（`{minui_dir}/{game_name}.txt`，对应 C
/// `menu.slot_path`，minarch.c:3071）
pub fn slot_memory_path(minui_dir: &str, game_name: &str) -> String {
    format!("{minui_dir}/{game_name}.txt")
}

/// 多碟记忆文件路径（`{minui_dir}/{game_name}.{slot}.txt`，对应 C
/// `menu.txt_path`，minarch.c:4127）
pub fn disc_memory_path(minui_dir: &str, game_name: &str, slot: usize) -> String {
    format!("{minui_dir}/{game_name}.{slot}.txt")
}

impl SaveStateIo {
    /// 创建存档交互编排
    ///
    /// # 参数
    ///
    /// - `minui_dir`: 游戏专属存档目录（装配层经
    ///   `common::utils::get_emu_name` + `common::paths::get_shared_userdata_path`
    ///   拼装）
    /// - `states_dir`: 状态快照目录
    /// - `game_name`: 游戏名（完整文件名含扩展名）
    /// - `disc_paths`: 碟片绝对路径表（多碟；单碟传空表）
    /// - `base_path`: 碟片基准目录（多碟时传 m3u 所在目录，末尾含 `/`）
    pub fn new(
        minui_dir: &str,
        states_dir: &str,
        game_name: &str,
        disc_paths: Vec<String>,
        base_path: &str,
    ) -> Self {
        Self {
            minui_dir: minui_dir.to_string(),
            states_dir: states_dir.to_string(),
            game_name: game_name.to_string(),
            disc_paths,
            base_path: base_path.to_string(),
        }
    }

    /// 读取 slot 记忆文件（对应 C `Menu_initState`，minarch.c:4109-4110）
    ///
    /// 无文件视为 0；内容为 8 视为 0（对应 C `if (menu.slot==8)
    /// menu.slot = 0`）。
    pub fn load_slot(&self) -> usize {
        let path = slot_memory_path(&self.minui_dir, &self.game_name);
        match get_int(&path) {
            Some(8) => 0,
            Some(v) if v > 0 => v as usize,
            _ => 0,
        }
    }

    /// 写入 slot 记忆文件（对应 C `putInt(menu.slot_path, menu.slot)`，
    /// minarch.c:4155）
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 写入成功
    /// - `Err`: 目录缺失等 IO 错误
    pub fn store_slot(&self, slot: usize) -> std::io::Result<()> {
        put_int(
            &slot_memory_path(&self.minui_dir, &self.game_name),
            slot as i32,
        )
    }

    /// 写入多碟记忆文件（对应 C `putFile(menu.txt_path, …)`，
    /// minarch.c:4140-4143）
    ///
    /// 单碟（`disc_paths` 为空）时无操作。内容为当前碟片路径相对
    /// `base_path` 的部分（C 的 `disc_path + strlen(base_path)`）。
    ///
    /// # 返回值
    ///
    /// - `Ok(())`: 写入成功或无操作
    /// - `Err`: 目录缺失等 IO 错误
    pub fn store_disc(&self, slot: usize, disc: usize) -> std::io::Result<()> {
        let Some(path) = self.disc_paths.get(disc) else {
            return Ok(());
        };
        let rel = path
            .strip_prefix(&self.base_path)
            .unwrap_or(path)
            .to_string();
        put_file(
            &disc_memory_path(&self.minui_dir, &self.game_name, slot),
            &rel,
        )
    }

    /// 查询当前槽位是否有快照（对应 C `Menu_updateState` 的
    /// `save_exists`，minarch.c:4129）
    pub fn save_exists(&self, slot: usize) -> bool {
        let path = crate::savestate::state_path(&self.states_dir, &self.game_name, slot as i32);
        exists(&path)
    }

    /// 查询当前槽位是否有预览图（对应 C `menu.preview_exists`，
    /// minarch.c:4130——`save_exists && bmp 存在`）
    ///
    /// `.bmp` 预览文件路径为 `{minui_dir}/{game_name}.{slot}.bmp`
    /// （对应 C `menu.bmp_path`，minarch.c:4126）。
    pub fn preview_exists(&self, slot: usize) -> bool {
        if !self.save_exists(slot) {
            return false;
        }
        let bmp = format!("{}/{}.{}.bmp", self.minui_dir, self.game_name, slot);
        exists(&bmp)
    }

    /// 读档前的碟片比对（对应 C `Menu_loadState` 的碟片切换段，
    /// minarch.c:4164-4176）
    ///
    /// 读取 `{game}.{slot}.txt` 记忆的碟片相对路径，与当前碟片
    /// （`disc_paths[current_disc]`）比对：
    ///
    /// - 记忆文件存在且路径与当前碟片不一致 → `Some(DiscRequest)`
    ///   （目标为 `base_path + 记忆路径`，记忆以 `/` 开头时为绝对路径）
    /// - 无记忆文件 / 记忆路径与当前碟片一致 / 无碟片表 → `None`
    pub fn disc_change_request(&self, slot: usize, current_disc: usize) -> Option<DiscRequest> {
        if self.disc_paths.is_empty() {
            return None;
        }
        let path = disc_memory_path(&self.minui_dir, &self.game_name, slot);
        let mem = get_file(&path)?;
        let mem = mem.trim().to_string();
        if mem.is_empty() {
            return None;
        }
        let target = if mem.starts_with('/') {
            mem
        } else {
            format!("{}{}", self.base_path, mem)
        };
        let current = self.disc_paths.get(current_disc)?;
        if target == *current {
            None
        } else {
            Some(DiscRequest { path: target })
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 菜单绘制
// ═══════════════════════════════════════════════════════════════

/// 绘制资源集合（装配层注入，对应 C 全局 `font.*` 与 `menu.overlay`）
pub struct MenuTheme<'a> {
    /// 小号字体（菜单项，对应 C `font.small`）
    pub font_small: &'a Font,
    /// 极小号字体（值/描述，对应 C `font.tiny`）
    pub font_tiny: &'a Font,
    /// 大号字体（主菜单项与名称条，对应 C `font.large`）
    pub font_large: &'a Font,
    /// 精灵图集（药丸等素材）
    pub atlas: &'a Atlas,
    /// 平台缩放倍率（对应 C `SCALE1`）
    pub scale: u32,
}

/// 槽位状态（Save/Load 选中时注入绘制）
pub struct SlotState<'a> {
    /// 当前槽位是否有快照
    pub save_exists: bool,
    /// 已解码的预览缩略图（`None` = 无预览；`.bmp` 解码归 render
    /// BMP 支持 change，装配层注入）
    pub preview: Option<&'a VideoBuffer>,
}

/// 选项菜单绘制状态（`draw_option_menu` 的参数）
pub struct OptionDrawState<'a> {
    /// 菜单状态机
    pub menu: &'a OptionMenu,
    /// 是否显示硬件提示（亮度/音量，对应 C `show_setting`）
    pub show_settings: bool,
}

/// 主菜单文字（对应 C `menu.items[]`，minarch.c:3053-3058）
pub const MAIN_ITEMS: [&str; MENU_ITEM_COUNT] = ["Continue", "Save", "Load", "Options", "Quit"];

/// simple_mode 下第 4 项文案（对应 C :3073）
pub const SIMPLE_MODE_OPTS_LABEL: &str = "Reset";

/// 绘制主菜单（对应 C `Menu_loop` 绘制段，minarch.c:4395-4534）
///
/// 组成（自上而下）：半透明遮罩 → 顶部游戏名黑色药丸条 → 底部按钮组
/// （左 POWER/MENU SLEEP、右 B BACK / A OKAY）→ 5 个菜单项（选中项
/// 白色药丸 + 黑字，未选中白字 + 黑色阴影）→ Save/Load 选中时的槽位
/// 预览窗（窗口骨架 + 分页点 + 空态文案或注入缩略图）。
///
/// # 参数
///
/// - `theme`: 绘制资源
/// - `screen`: 目标帧缓冲（**已含游戏帧**——本函数叠加遮罩与菜单）
/// - `overlay`: 半透明遮罩缓冲（装配层预生成，对应 C `menu.overlay`）
/// - `game_name`: 游戏显示名（装配层截断）
/// - `menu`: 主菜单状态机
/// - `disc_name`: 多碟时的当前碟片名（"Disc N"；单碟传 `None`）
/// - `slot_state`: Save/Load 选中时的槽位状态（其余项传 `None`）
pub fn draw_main_menu(
    theme: &MenuTheme,
    screen: &mut VideoBuffer,
    overlay: &VideoBuffer,
    game_name: &str,
    menu: &MainMenu,
    disc_name: Option<&str>,
    slot_state: Option<&SlotState>,
) {
    blit_overlay(screen, overlay);

    let scale = theme.scale;
    let pad = common::video::PADDING * scale;
    let pill = common::video::PILL_SIZE * scale;
    let button_pad = common::video::BUTTON_PADDING * scale;

    // 顶部名称条（C :4401-4426）：黑药丸 + 截断的游戏名
    let max_width = screen.width - pad * 2;
    let display = truncate_text(theme.font_large, game_name, max_width, pill);
    let (text_w, _) = size_text(theme.font_large, &display, pill, 0);
    let pill_w = text_w + button_pad * 2;
    blit_pill(
        theme.atlas,
        common::video::Asset::BlackPill,
        scale,
        screen,
        Rect {
            x: pad,
            y: pad,
            w: pill_w,
            h: pill,
        },
    );
    render_text(
        screen,
        theme.font_large,
        &display,
        pill,
        common::video::RGB_WHITE,
        (pad + button_pad, pad + 4),
    );

    // 底部按钮组（C :4429-4430）：右 B BACK / A OKAY
    let btn_group = [("BACK", "B"), ("OKAY", "A")];
    render::button::blit_button_group(
        &btn_group,
        theme.atlas,
        theme.font_small,
        screen,
        Rect {
            x: 0,
            y: screen.height - pill - pad,
            w: screen.width,
            h: pill + pad,
        },
        true,
        pill / 2,
        scale,
    );

    // 菜单项列表（C :4433-4484）：5 项垂直居中
    let list_h = (MENU_ITEM_COUNT as u32) * pill;
    let oy = (screen.height.saturating_sub(list_h)) / 2;
    for (i, label) in MAIN_ITEMS.iter().enumerate() {
        let selected = i == menu.selected;
        // simple_mode 下 Options 项显示 Reset（C :3073）
        let text = if i == item::OPTS && menu.simple_mode {
            SIMPLE_MODE_OPTS_LABEL
        } else {
            label
        };

        // 多碟时 Continue 选中行右侧显示当前碟片（C :4440-4453）
        if selected
            && i == item::CONT
            && let Some(name) = disc_name
        {
            let (dw, _) = size_text(theme.font_large, name, pill, 0);
            let dx = screen.width - pad - button_pad - dw;
            blit_pill(
                theme.atlas,
                common::video::Asset::DarkGrayPill,
                scale,
                screen,
                Rect {
                    x: dx - pad,
                    y: oy + pad,
                    w: dw + pad * 2,
                    h: pill,
                },
            );
            render_text(
                screen,
                theme.font_large,
                name,
                pill,
                common::video::RGB_WHITE,
                (dx, oy + pad + 4),
            );
        }

        if selected {
            // 选中项：白色药丸 + 黑字（C :4459-4465）
            let (w, _) = size_text(theme.font_large, text, pill, 0);
            blit_pill(
                theme.atlas,
                common::video::Asset::WhitePill,
                scale,
                screen,
                Rect {
                    x: pad,
                    y: oy + pad + (i as u32) * pill,
                    w: w + button_pad * 2,
                    h: pill,
                },
            );
            render_text(
                screen,
                theme.font_large,
                text,
                pill,
                common::video::RGB_BLACK,
                (pad + button_pad, oy + pad + (i as u32) * pill + 4),
            );
        } else {
            // 未选中项：白字 + 黑色阴影（偏移 2,1，C :4467-4475）
            render_text(
                screen,
                theme.font_large,
                text,
                pill,
                common::video::RGB_BLACK,
                (pad + button_pad + 2, oy + pad + (i as u32) * pill + 5),
            );
            render_text(
                screen,
                theme.font_large,
                text,
                pill,
                common::video::RGB_WHITE,
                (pad + button_pad, oy + pad + (i as u32) * pill + 4),
            );
        }
    }

    // 槽位预览窗（C :4487-4530）
    if let Some(state) = slot_state {
        draw_slot_preview(theme, screen, menu, state);
    }
}

/// 把半透明遮罩叠加到屏幕（对应 C `SDL_BlitSurface(menu.overlay)`，
/// minarch.c:4399）
///
/// 遮罩为**预混合**的半透明黑（装配层生成，对应 C `SDLX_SetAlpha` +
/// `SDL_FillRect`，minarch.c:3062-3064）——直接覆盖写，不在此混合。
fn blit_overlay(screen: &mut VideoBuffer, overlay: &VideoBuffer) {
    let rows = screen.height.min(overlay.height) as usize;
    let cols = screen.width.min(overlay.width) as usize;
    let sp = overlay.pitch as usize;
    let dp = screen.pitch as usize;
    for row in 0..rows {
        let src = row * sp;
        let dst = row * dp;
        screen.pixels[dst..dst + cols].copy_from_slice(&overlay.pixels[src..src + cols]);
    }
}

/// 绘制槽位预览窗（对应 C :4487-4530）
///
/// 右侧圆角窗口（`StateBg` 骨架）内：有预览 → 注入缩略图；有存档无
/// 预览 → "No Preview"；空槽 → "Empty Slot"。底部 8 个分页点，当前
/// 槽位高亮（`Page`），其余为 `Dot`。
fn draw_slot_preview(
    theme: &MenuTheme,
    screen: &mut VideoBuffer,
    menu: &MainMenu,
    state: &SlotState,
) {
    let scale = theme.scale;
    let pad = common::video::PADDING * scale;
    let pill = common::video::PILL_SIZE * scale;

    // 窗口尺寸：半屏 + 圆角边距 + 分页区（对应 C :4490-4496）
    let hw = screen.width / 2;
    let hh = screen.height / 2;
    let radius = 4 * scale;
    let pagination = 6 * scale;
    let pw = hw + radius * 2;
    let ph = hh + radius * 2 + pagination + radius;
    let ox = screen.width - pw - pad;
    let oy = (screen.height - ph) / 2;

    // 窗口骨架（C :4499）
    render::pill::blit_rect(
        theme.atlas,
        common::video::Asset::StateBg,
        scale,
        screen,
        Rect {
            x: ox,
            y: oy,
            w: pw,
            h: ph,
        },
    );
    let ix = ox + radius;
    let iy = oy + radius;

    // 预览区三态（C :4503-4521）
    let preview_rect = Rect {
        x: ix,
        y: iy,
        w: hw,
        h: hh,
    };
    if let Some(preview) = state.preview {
        // 注入的缩略图：缩放到预览区（装配层已按目标尺寸缩放）
        let pw_src = preview.width.min(hw);
        let ph_src = preview.height.min(hh);
        let sx = (hw - pw_src) / 2;
        let sy = (hh - ph_src) / 2;
        blit_buffer_region(
            screen,
            preview,
            preview_rect.x + sx,
            preview_rect.y + sy,
            pw_src,
            ph_src,
        );
    } else {
        let msg = if state.save_exists {
            "No Preview"
        } else {
            "Empty Slot"
        };
        render::text::blit_message(screen, theme.font_large, msg, pill / 2, preview_rect);
    }

    // 分页点（C :4524-4529）：8 个，当前槽位高亮
    let dot_spacing = 15 * scale;
    let mut dx = ix + (pw - dot_spacing * MENU_SLOT_COUNT as u32) / 2;
    let dy = iy + hh + radius;
    for i in 0..MENU_SLOT_COUNT {
        let asset = if i == menu.slot {
            common::video::Asset::Page
        } else {
            common::video::Asset::Dot
        };
        let base = &common::video::ASSET_RECTS[asset as usize];
        let offset = if i == menu.slot { 0 } else { 4 * scale };
        render::asset::blit_asset(
            theme.atlas,
            asset,
            scale,
            screen,
            (dx + offset, dy + offset),
            None,
        );
        let _ = base;
        dx += dot_spacing;
    }
}

/// 把预览缓冲的指定区域拷贝到屏幕（逐行 memcpy）
fn blit_buffer_region(
    screen: &mut VideoBuffer,
    src: &VideoBuffer,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
) {
    let sp = src.pitch as usize;
    let dp = screen.pitch as usize;
    for row in 0..h as usize {
        let s_off = row * sp;
        let d_off = (dy as usize + row) * dp + dx as usize;
        screen.pixels[d_off..d_off + w as usize]
            .copy_from_slice(&src.pixels[s_off..s_off + w as usize]);
    }
}

/// 绘制选项菜单（对应 C `Menu_options` 绘制段，minarch.c:3744-3960）
///
/// 按 [`ItemKind`] 分四种布局：
///
/// - `List`：居中列，选中行白色药丸，行宽 = 最宽项 + 内边距（缓存）
/// - `Fixed`：整行灰药丸（选中行）+ 右侧值 + 选中行白色药丸（仅文本宽）
/// - `Var`/`Input`：行宽 = 名称 + 最宽值；选中行灰白双药丸；值右对齐
///
/// 另有滚动箭头（超一屏时顶部/底部灰色三角）与底部描述文字。
///
/// # 参数
///
/// - `theme`: 绘制资源
/// - `screen`: 目标帧缓冲（**已清屏**——本函数只画菜单，不叠加游戏帧）
/// - `state`: 选项菜单绘制状态（含菜单状态机与 `show_settings`）
pub fn draw_option_menu(theme: &MenuTheme, screen: &mut VideoBuffer, state: &OptionDrawState) {
    let menu = state.menu;
    let scale = theme.scale;
    let pad = common::video::PADDING * scale;
    let pill = common::video::PILL_SIZE * scale;
    let option_pad = 8 * scale; // 对应 C `OPTION_PADDING 8`，minarch.c:3584

    // 行宽：按类型计算（C :3751-3764/:3798-3800/:3856-3885）
    let row_w = match menu.items.first().map(|i| i.kind) {
        Some(ItemKind::Fixed) => screen.width - pad * 2,
        Some(ItemKind::Var) | Some(ItemKind::Input) => {
            let mut mw = 0u32;
            for item in &menu.items {
                let (lw, _) = size_text(theme.font_small, &item.name, pill / 2, 0);
                let mut rw = 0u32;
                for v in &item.values {
                    let (vw, _) = size_text(theme.font_tiny, v, pill / 3, 0);
                    rw = rw.max(vw);
                }
                mw = mw.max(lw + rw + option_pad * 4);
            }
            mw.min(screen.width - pad * 2)
        }
        _ => {
            let mut mw = 0u32;
            for item in &menu.items {
                let (w, _) = size_text(theme.font_small, &item.name, pill / 2, 0);
                mw = mw.max(w + option_pad * 2);
            }
            mw.min(screen.width - pad * 2)
        }
    };

    let ox = (screen.width - row_w) / 2;
    let oy = pad + pill;
    let selected_row = menu.selected - menu.start;
    // 行距 = PILL_SIZE × scale：render 的 blit_pill 高度固定为精灵高
    // （WhitePill 30×scale），不按传入 h 裁切——行距取精灵高避免重叠
    // 与越界（C 用 BUTTON_SIZE 行距、GFX_blitPill 裁切，见设计偏离记录）
    let row_h = pill;

    for (i, item) in menu
        .items
        .iter()
        .enumerate()
        .skip(menu.start)
        .take(menu.end - menu.start)
    {
        let j = i - menu.start;
        let row_y = oy + (j as u32) * row_h;
        // 越界保护：最后一行超出屏幕时跳过（渲染函数不做边界裁剪）
        if row_y + pill > screen.height {
            continue;
        }
        let selected = j == selected_row;

        match item.kind {
            ItemKind::Fixed => {
                // 整行灰药丸（C :3812-3819）
                if selected {
                    blit_pill(
                        theme.atlas,
                        common::video::Asset::Option,
                        scale,
                        screen,
                        Rect {
                            x: ox,
                            y: row_y,
                            w: row_w,
                            h: row_h,
                        },
                    );
                }
                // 右侧值（C :3822-3829）
                if item.value < item.values.len() {
                    let (vw, _) = size_text(theme.font_tiny, item.value_text(), pill / 3, 0);
                    render_text(
                        screen,
                        theme.font_tiny,
                        item.value_text(),
                        pill / 3,
                        common::video::RGB_WHITE,
                        (ox + row_w - vw - option_pad, row_y + 3),
                    );
                }
                // 选中行白色药丸（仅文本宽，C :3832-3843）
                if selected {
                    let (w, _) = size_text(theme.font_small, &item.name, pill / 2, 0);
                    blit_pill(
                        theme.atlas,
                        common::video::Asset::WhitePill,
                        scale,
                        screen,
                        Rect {
                            x: ox,
                            y: row_y,
                            w: w + option_pad * 2,
                            h: row_h,
                        },
                    );
                }
            }
            ItemKind::Var | ItemKind::Input => {
                if selected {
                    // 整行灰药丸 + 白色药丸（C :3894-3916）
                    blit_pill(
                        theme.atlas,
                        common::video::Asset::Option,
                        scale,
                        screen,
                        Rect {
                            x: ox,
                            y: row_y,
                            w: row_w,
                            h: row_h,
                        },
                    );
                    let (w, _) = size_text(theme.font_small, &item.name, pill / 2, 0);
                    blit_pill(
                        theme.atlas,
                        common::video::Asset::WhitePill,
                        scale,
                        screen,
                        Rect {
                            x: ox,
                            y: row_y,
                            w: w + option_pad * 2,
                            h: row_h,
                        },
                    );
                }
                // 右侧值（C :3927-3934）
                if item.value < item.values.len() {
                    let (vw, _) = size_text(theme.font_tiny, item.value_text(), pill / 3, 0);
                    render_text(
                        screen,
                        theme.font_tiny,
                        item.value_text(),
                        pill / 3,
                        common::video::RGB_WHITE,
                        (ox + row_w - vw - option_pad, row_y + 3),
                    );
                }
            }
            ItemKind::List => {
                if selected {
                    let (w, _) = size_text(theme.font_small, &item.name, pill / 2, 0);
                    blit_pill(
                        theme.atlas,
                        common::video::Asset::WhitePill,
                        scale,
                        screen,
                        Rect {
                            x: ox,
                            y: row_y,
                            w: w + option_pad * 2,
                            h: row_h,
                        },
                    );
                }
            }
        }

        // 项文字（选中黑字，未选中白字；C :3847-3852/:3917-3921）
        let color = if selected {
            common::video::RGB_BLACK
        } else {
            common::video::RGB_WHITE
        };
        render_text(
            screen,
            theme.font_small,
            &item.name,
            pill / 2,
            color,
            (ox + option_pad, row_y + 1),
        );
    }

    // 滚动指示（C :3938-3945）：超一屏时顶部/底部灰色三角
    if menu.items.len() > menu.max_visible {
        let scroll_w = 24 * scale;
        let scroll_h = 4 * scale;
        let sx = (screen.width - scroll_w) / 2;
        if menu.start > 0 {
            render::asset::blit_asset(
                theme.atlas,
                common::video::Asset::ScrollUp,
                scale,
                screen,
                (sx, pad),
                None,
            );
        }
        if menu.end < menu.items.len() {
            let sy = screen.height - pad - pill - row_h + (pill - scroll_h) / 2;
            render::asset::blit_asset(
                theme.atlas,
                common::video::Asset::ScrollDown,
                scale,
                screen,
                (sx, sy),
                None,
            );
        }
    }

    // 描述文字（C :3947-3957）：选中项 desc 优先，其次列表 desc
    let desc = menu
        .items
        .get(menu.selected)
        .and_then(|i| i.desc.as_deref())
        .or(menu.desc.as_deref());
    if let Some(desc) = desc {
        let leading = 12 * scale;
        let (w, h) = size_text(theme.font_tiny, desc, pill / 3, leading);
        let x = (screen.width - w) / 2;
        let y = screen.height - pad - h;
        render::text::blit_text(
            screen,
            theme.font_tiny,
            desc,
            pill / 3,
            common::video::RGB_WHITE,
            Rect { x, y, w, h },
            leading,
        );
    }
}
