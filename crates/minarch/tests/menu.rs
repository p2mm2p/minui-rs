//! menu.rs 游戏内菜单测试
//!
//! 覆盖 spec「游戏内菜单框架」「菜单存档交互编排」「菜单绘制」
//! Requirement 的全部 Scenario。
//!
//! 分层测试策略：
//! - 状态机测试（`MainMenu`/`OptionMenu`）：纯逻辑，无 IO
//! - 存档交互测试（`SaveStateIo`）：temp dir 铺真实文件
//! - 绘制测试：真实字体（系统字体回退）+ 测试图集（白色填充），
//!   断言「非空像素区域」与「布局数值」，不做像素级 golden 比对
//!
//! 字体加载仿 `render`/`minui` 测试先例（系统字体路径回退）。

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use common::video::{RGB_BLACK, RGB_WHITE, VideoBuffer};
use minarch::menu::{
    DiscRequest, ItemKind, MAIN_ITEMS, MENU_ITEM_COUNT, MENU_SLOT_COUNT, MainAction, MainMenu,
    MenuInput, MenuItem, MenuTheme, OptionMenu, OptionMenuEvent, SIMPLE_MODE_OPTS_LABEL,
    SaveStateIo, disc_memory_path, draw_main_menu, draw_option_menu, item, slot_memory_path,
};
use render::asset::Atlas;
use render::text::Font;

// ═══════════════════════════════════════════════════════════════
// 共享辅助
// ═══════════════════════════════════════════════════════════════

/// 单个按键的输入构造
fn input(up: bool, down: bool, left: bool, right: bool, a: bool, b: bool) -> MenuInput {
    MenuInput {
        up,
        down,
        left,
        right,
        a,
        b,
        x: false,
        menu: false,
    }
}

fn press_a() -> MenuInput {
    input(false, false, false, false, true, false)
}

fn press_b() -> MenuInput {
    input(false, false, false, false, false, true)
}

fn press_up() -> MenuInput {
    input(true, false, false, false, false, false)
}

fn press_down() -> MenuInput {
    input(false, true, false, false, false, false)
}

fn press_left() -> MenuInput {
    input(false, false, true, false, false, false)
}

fn press_right() -> MenuInput {
    input(false, false, false, true, false, false)
}

fn idle() -> MenuInput {
    input(false, false, false, false, false, false)
}

/// 系统字体加载（仿 render/minui 测试先例）
fn test_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        let paths = [
            "/System/Library/Fonts/Helvetica.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ];
        for path in &paths {
            if let Ok(data) = fs::read(path) {
                return render::text::load_font(&data);
            }
        }
        panic!("未找到可用于测试的字体文件");
    })
}

/// 白色测试图集（足够大以容纳所有素材）
fn test_atlas() -> &'static Atlas {
    static ATLAS: OnceLock<Atlas> = OnceLock::new();
    ATLAS.get_or_init(|| Atlas {
        pixels: vec![0xFFFF; 256 * 256],
        width: 256,
        height: 256,
    })
}

/// 测试主题
fn theme() -> MenuTheme<'static> {
    MenuTheme {
        font_small: test_font(),
        font_tiny: test_font(),
        font_large: test_font(),
        atlas: test_atlas(),
        scale: 2,
    }
}

/// 为单个测试创建唯一 temp 目录（并行测试隔离）
fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("minarch_menu_{test}"));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 检查缓冲中指定矩形区域是否有白色像素
fn rect_has_white(dst: &VideoBuffer, x: u32, y: u32, w: u32, h: u32) -> bool {
    let pitch = dst.pitch as usize;
    for row in y..y + h {
        for col in x..x + w {
            let idx = row as usize * pitch + col as usize;
            if dst.pixels[idx] == RGB_WHITE {
                return true;
            }
        }
    }
    false
}

/// 检查缓冲中指定矩形区域是否有黑色像素
fn rect_has_black(dst: &VideoBuffer, x: u32, y: u32, w: u32, h: u32) -> bool {
    let pitch = dst.pitch as usize;
    for row in y..y + h {
        for col in x..x + w {
            let idx = row as usize * pitch + col as usize;
            if dst.pixels[idx] == RGB_BLACK {
                return true;
            }
        }
    }
    false
}

// ═══════════════════════════════════════════════════════════════
// 1. MainMenu 状态机
// ═══════════════════════════════════════════════════════════════

// ── 导航（spec 场景「主菜单环形导航」）──

#[test]
fn main_menu_defaults_to_continue() {
    let menu = MainMenu::new(false, 1);
    assert_eq!(menu.selected, item::CONT);
}

#[test]
fn main_menu_up_wraps_to_quit() {
    let mut menu = MainMenu::new(false, 1);
    // 从 Continue 一路 UP 回绕到 Quit
    menu.update(&press_up());
    assert_eq!(menu.selected, item::QUIT);
}

#[test]
fn main_menu_down_wraps_to_continue() {
    let mut menu = MainMenu::new(false, 1);
    // 从 Continue 一路 DOWN 回绕回 Continue
    menu.update(&press_down());
    assert_eq!(menu.selected, item::SAVE);
    menu.update(&press_down());
    assert_eq!(menu.selected, item::LOAD);
    menu.update(&press_down());
    assert_eq!(menu.selected, item::OPTS);
    menu.update(&press_down());
    assert_eq!(menu.selected, item::QUIT);
    menu.update(&press_down());
    assert_eq!(menu.selected, item::CONT);
}

#[test]
fn main_menu_navigation_never_out_of_bounds() {
    let mut menu = MainMenu::new(false, 1);
    // 连续 UP 100 次：始终在 0..5
    for _ in 0..100 {
        menu.update(&press_up());
        assert!(menu.selected < MENU_ITEM_COUNT);
    }
    // 连续 DOWN 100 次：始终在 0..5
    for _ in 0..100 {
        menu.update(&press_down());
        assert!(menu.selected < MENU_ITEM_COUNT);
    }
}

// ── 换碟（spec 场景「多碟时 Continue 项换碟」「单碟时 LEFT/RIGHT 不改碟」）──

#[test]
fn multi_disc_continue_cycles_discs() {
    let mut menu = MainMenu::new(false, 3);
    assert_eq!(menu.disc, 0);
    // 选中 Continue 时 RIGHT 两次 → 碟 1、碟 2
    menu.update(&press_right());
    assert_eq!(menu.disc, 1);
    menu.update(&press_right());
    assert_eq!(menu.disc, 2);
    // 再 RIGHT 回绕到 0
    menu.update(&press_right());
    assert_eq!(menu.disc, 0);
    // LEFT 反向
    menu.update(&press_left());
    assert_eq!(menu.disc, 2);
}

#[test]
fn single_disc_left_right_noop() {
    let mut menu = MainMenu::new(false, 1);
    menu.update(&press_right());
    assert_eq!(menu.disc, 0);
    assert_eq!(menu.selected, item::CONT);
    menu.update(&press_left());
    assert_eq!(menu.disc, 0);
    assert_eq!(menu.selected, item::CONT);
}

#[test]
fn disc_change_only_on_continue_item() {
    // 非 Continue 项时 LEFT/RIGHT 不应换碟
    let mut menu = MainMenu::new(false, 3);
    menu.update(&press_down()); // SAVE
    menu.update(&press_right());
    assert_eq!(menu.disc, 0, "Save 项 RIGHT 不应换碟");
    menu.update(&press_left());
    assert_eq!(menu.disc, 0);
}

// ── 槽位循环（spec 场景「Save/Load 项切换槽位」）──

#[test]
fn save_item_cycles_slots() {
    let mut menu = MainMenu::new(false, 1);
    menu.update(&press_down()); // SAVE
    assert_eq!(menu.slot, 0);
    menu.update(&press_right());
    assert_eq!(menu.slot, 1);
    menu.update(&press_left());
    assert_eq!(menu.slot, 0);
    // RIGHT 8 次回绕
    for _ in 0..MENU_SLOT_COUNT {
        menu.update(&press_right());
    }
    assert_eq!(menu.slot, 0, "槽位应在 0..7 循环");
}

#[test]
fn load_item_cycles_slots() {
    let mut menu = MainMenu::new(false, 1);
    menu.update(&press_down()); // SAVE
    menu.update(&press_down()); // LOAD
    menu.update(&press_left());
    assert_eq!(menu.slot, MENU_SLOT_COUNT - 1);
    menu.update(&press_right());
    assert_eq!(menu.slot, 0);
}

// ── simple_mode（spec 场景「simple_mode 下 Reset 语义」）──

#[test]
fn simple_mode_opts_label_is_reset() {
    // 文案常量：simple_mode 下第 4 项为 Reset
    assert_eq!(SIMPLE_MODE_OPTS_LABEL, "Reset");
    assert_eq!(MAIN_ITEMS[item::OPTS], "Options");
}

#[test]
fn simple_mode_confirm_resets() {
    let mut menu = MainMenu::new(true, 1);
    // 选中第 4 项（Options/Reset）
    menu.update(&press_down());
    menu.update(&press_down());
    menu.update(&press_down());
    assert_eq!(menu.selected, item::OPTS);
    let action = menu.update(&press_a()).unwrap();
    assert_eq!(action, MainAction::Reset);
}

#[test]
fn normal_mode_confirm_opens_options() {
    let mut menu = MainMenu::new(false, 1);
    menu.update(&press_down());
    menu.update(&press_down());
    menu.update(&press_down());
    let action = menu.update(&press_a()).unwrap();
    assert_eq!(action, MainAction::Options);
}

// ── A 键全分派（spec 场景「主菜单 A 键分派」）──

#[test]
fn main_menu_a_key_dispatch_all_items() {
    let mut menu = MainMenu::new(false, 1);
    // Continue
    assert_eq!(menu.update(&press_a()).unwrap(), MainAction::Continue);
    // Save
    menu.update(&press_down());
    assert_eq!(menu.update(&press_a()).unwrap(), MainAction::Save);
    // Load
    menu.update(&press_down());
    assert_eq!(menu.update(&press_a()).unwrap(), MainAction::Load);
    // Options
    menu.update(&press_down());
    assert_eq!(menu.update(&press_a()).unwrap(), MainAction::Options);
    // Quit
    menu.update(&press_down());
    assert_eq!(menu.update(&press_a()).unwrap(), MainAction::Quit);
}

#[test]
fn main_menu_b_key_returns_continue() {
    let mut menu = MainMenu::new(false, 1);
    menu.update(&press_down());
    menu.update(&press_down());
    let action = menu.update(&press_b()).unwrap();
    assert_eq!(action, MainAction::Continue);
}

#[test]
fn main_menu_disc_change_action_when_disc_selected() {
    // 多碟：选中 Continue 且碟片已切换 → DiscChange
    let mut menu = MainMenu::new(false, 3);
    menu.update(&press_right()); // disc 1
    let action = menu.update(&press_a()).unwrap();
    assert_eq!(action, MainAction::DiscChange);
    // 碟 0 时 A → Continue
    menu.update(&press_left());
    let action = menu.update(&press_a()).unwrap();
    assert_eq!(action, MainAction::Continue);
}

#[test]
fn main_menu_idle_returns_none() {
    let mut menu = MainMenu::new(false, 1);
    assert_eq!(menu.update(&idle()), None);
}

// ═══════════════════════════════════════════════════════════════
// 2. OptionMenu 状态机
// ═══════════════════════════════════════════════════════════════

/// 12 项列表（spec 场景「选项菜单滚动与回绕」）
fn list_of(n: usize) -> Vec<MenuItem> {
    (0..n)
        .map(|i| MenuItem::list(&format!("Item {i}")))
        .collect()
}

// ── 滚动与回绕（spec 场景「选项菜单滚动与回绕」）──

#[test]
fn option_menu_scroll_and_wrap() {
    let mut menu = OptionMenu::new(list_of(12), 7, None, None);
    assert_eq!(menu.selected, 0);
    assert_eq!(menu.start, 0);
    assert_eq!(menu.end, 7);

    // UP 从首项回绕到末项（11），窗口对齐末页 (5..12)
    menu.update(&press_up());
    assert_eq!(menu.selected, 11);
    assert_eq!(menu.start, 5);
    assert_eq!(menu.end, 12);

    // DOWN 从末项回绕到首项，窗口对齐首页
    menu.update(&press_down());
    assert_eq!(menu.selected, 0);
    assert_eq!(menu.start, 0);
    assert_eq!(menu.end, 7);
}

#[test]
fn option_menu_scroll_window_slides() {
    let mut menu = OptionMenu::new(list_of(12), 7, None, None);
    // 向下到第 8 项（index 7）→ 窗口滑动
    for _ in 0..7 {
        menu.update(&press_down());
    }
    assert_eq!(menu.selected, 7);
    assert_eq!(menu.start, 1);
    assert_eq!(menu.end, 8);
    // 再向上一次：仍在窗口内，窗口不动（C 语义——selected 在 [start,end) 内时窗口不滑）
    menu.update(&press_up());
    assert_eq!(menu.selected, 6);
    assert_eq!(menu.start, 1);
    assert_eq!(menu.end, 8);
    // 继续 UP 到窗口上边界 → 窗口滑回
    for _ in 0..6 {
        menu.update(&press_up());
    }
    assert_eq!(menu.selected, 0);
    assert_eq!(menu.start, 0);
    assert_eq!(menu.end, 7);
}

#[test]
fn option_menu_short_list_no_wrap_issue() {
    // 3 项 < 可见行数：UP 回绕到末项，窗口 = 全列表
    let mut menu = OptionMenu::new(list_of(3), 7, None, None);
    menu.update(&press_up());
    assert_eq!(menu.selected, 2);
    assert_eq!(menu.start, 0);
    assert_eq!(menu.end, 3);
}

// ── 值修改（spec 场景「选项菜单值修改与循环」）──

#[test]
fn option_menu_value_cycles_and_emits_change() {
    let mut menu = OptionMenu::new(
        vec![MenuItem::var(
            "Mode",
            vec!["A".into(), "B".into(), "C".into()],
            0,
        )],
        7,
        None,
        None,
    );
    // RIGHT 三次：A→B→C→A
    assert_eq!(menu.update(&press_right()), OptionMenuEvent::Change(0));
    assert_eq!(menu.items[0].value, 1);
    assert_eq!(menu.update(&press_right()), OptionMenuEvent::Change(0));
    assert_eq!(menu.items[0].value, 2);
    assert_eq!(menu.update(&press_right()), OptionMenuEvent::Change(0));
    assert_eq!(menu.items[0].value, 0);
    // LEFT：回 C
    assert_eq!(menu.update(&press_left()), OptionMenuEvent::Change(0));
    assert_eq!(menu.items[0].value, 2);
}

#[test]
fn option_menu_list_item_left_right_noop() {
    // 无 values 的 List 项：LEFT/RIGHT 无事件
    let mut menu = OptionMenu::new(list_of(3), 7, None, None);
    assert_eq!(menu.update(&press_right()), OptionMenuEvent::None);
    assert_eq!(menu.update(&press_left()), OptionMenuEvent::None);
}

// ── 确认分发（spec 场景「选项菜单确认分发优先级」）──

#[test]
fn option_menu_confirm_dispatch_priority() {
    // item 回调 > submenu > list 回调；MENU_INPUT 进 await_input
    let mut with_item_cb = OptionMenu::new(
        vec![MenuItem {
            on_confirm: Some(minarch::menu::ConfirmAction::Request(MainAction::Quit)),
            ..MenuItem::list("Quit")
        }],
        7,
        None,
        None,
    );
    assert_eq!(
        with_item_cb.update(&press_a()),
        OptionMenuEvent::Confirm(0),
        "item 回调应触发 Confirm"
    );

    // submenu 下钻
    let mut with_submenu = OptionMenu::new(
        vec![MenuItem {
            on_confirm: Some(minarch::menu::ConfirmAction::OpenSubmenu(2)),
            ..MenuItem::list("Sub")
        }],
        7,
        None,
        None,
    );
    assert_eq!(with_submenu.update(&press_a()), OptionMenuEvent::Confirm(0));

    // 仅 list 回调
    let mut with_list_cb = OptionMenu::new(
        list_of(2),
        7,
        Some(minarch::menu::ConfirmAction::Request(MainAction::Quit)),
        None,
    );
    assert_eq!(with_list_cb.update(&press_a()), OptionMenuEvent::Confirm(0));

    // MENU_INPUT 且值为绑定表 → 进 await_input
    let mut input_menu = OptionMenu::new(
        vec![MenuItem {
            kind: ItemKind::Input,
            values: vec!["NONE".into(), "A".into()],
            ..MenuItem::list("Bind")
        }],
        7,
        None,
        None,
    );
    assert_eq!(input_menu.update(&press_a()), OptionMenuEvent::Confirm(0));
    assert!(input_menu.awaiting, "MENU_INPUT 确认后应进入 await_input");
}

#[test]
fn option_menu_b_closes() {
    let mut menu = OptionMenu::new(list_of(3), 7, None, None);
    assert_eq!(menu.update(&press_b()), OptionMenuEvent::Close);
}

// ── await_input（spec 场景「await_input 状态推进」）──

#[test]
fn option_menu_await_input_advances() {
    let mut menu = OptionMenu::new(
        vec![
            MenuItem {
                kind: ItemKind::Input,
                values: vec!["NONE".into(), "A".into()],
                ..MenuItem::list("Bind")
            },
            MenuItem::list("Next"),
        ],
        7,
        None,
        None,
    );
    // 确认 → 进入 await_input
    assert_eq!(menu.update(&press_a()), OptionMenuEvent::Confirm(0));
    assert!(menu.awaiting);

    // await_input 期间任何输入都被跳过 → Advance
    assert_eq!(menu.update(&press_down()), OptionMenuEvent::Advance);
    assert_eq!(menu.update(&press_a()), OptionMenuEvent::Advance);
    assert!(menu.awaiting, "await_input 期间状态不变");

    // 装配层完成绑定后 finish_await：清等待 + 下移选中项
    menu.finish_await();
    assert!(!menu.awaiting);
    assert_eq!(menu.selected, 1, "finish_await 应推进到下一项");
}

// ── 锁定项（spec 场景「锁定项被跳过」）──

#[test]
fn option_menu_from_options_filters_locked() {
    use minarch::config::ConfigOption;
    let options = vec![
        ConfigOption {
            key: "a".into(),
            name: "A".into(),
            desc: None,
            full: None,
            default_value: 0,
            value: 0,
            count: 2,
            lock: false,
            values: vec!["0".into(), "1".into()],
            labels: vec!["Zero".into(), "One".into()],
        },
        ConfigOption {
            key: "b".into(),
            name: "B".into(),
            desc: None,
            full: None,
            default_value: 0,
            value: 0,
            count: 2,
            lock: true, // 锁定：应被过滤
            values: vec!["0".into(), "1".into()],
            labels: vec!["Zero".into(), "One".into()],
        },
        ConfigOption {
            key: "c".into(),
            name: "C".into(),
            desc: None,
            full: None,
            default_value: 1,
            value: 1,
            count: 2,
            lock: false,
            values: vec!["0".into(), "1".into()],
            labels: vec!["Zero".into(), "One".into()],
        },
    ];
    let items = OptionMenu::from_options(&options, ItemKind::Var);
    assert_eq!(items.len(), 2, "锁定项应被过滤");
    assert_eq!(items[0].name, "A");
    assert_eq!(items[1].name, "C");
    assert_eq!(items[1].value, 1, "值应保留");
    assert_eq!(items[1].values, vec!["Zero".to_string(), "One".to_string()]);
}

// ═══════════════════════════════════════════════════════════════
// 3. SaveStateIo 存档交互
// ═══════════════════════════════════════════════════════════════

// ── slot 记忆（spec 场景「slot 记忆文件读写」「slot 8 归一」）──

#[test]
fn slot_memory_path_format() {
    assert_eq!(
        slot_memory_path("/tmp/minui/SFC", "Game.sfc"),
        "/tmp/minui/SFC/Game.sfc.txt"
    );
}

#[test]
fn slot_memory_read_write_roundtrip() {
    let dir = temp_dir("slot_roundtrip");
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.sfc",
        vec![],
        "",
    );
    // 无文件 → 0
    assert_eq!(io.load_slot(), 0);
    // 写入后读回
    io.store_slot(3).unwrap();
    assert_eq!(io.load_slot(), 3);
    // 覆盖
    io.store_slot(5).unwrap();
    assert_eq!(io.load_slot(), 5);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slot_memory_eight_normalizes_to_zero() {
    let dir = temp_dir("slot_eight");
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.sfc",
        vec![],
        "",
    );
    io.store_slot(8).unwrap();
    assert_eq!(io.load_slot(), 0, "slot 8 应视为 0（对应 C :4110）");
    let _ = fs::remove_dir_all(&dir);
}

// ── 多碟记忆（spec 场景「多碟存档写碟片记忆」「单碟不写碟片记忆」）──

#[test]
fn disc_memory_path_format() {
    assert_eq!(
        disc_memory_path("/tmp/minui/PS", "Game.m3u", 2),
        "/tmp/minui/PS/Game.m3u.2.txt"
    );
}

#[test]
fn multi_disc_store_disc_writes_relative_path() {
    let dir = temp_dir("disc_store");
    let base = format!("{}/Game (Disc 1)/", dir.to_str().unwrap());
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.m3u",
        vec![
            format!("{base}Disc 1.cue"),
            format!("{base}Disc 2.cue"),
            format!("{base}Disc 3.cue"),
        ],
        &base,
    );
    io.store_disc(1, 2).unwrap(); // slot 1、碟 2
    let content =
        fs::read_to_string(disc_memory_path(dir.to_str().unwrap(), "Game.m3u", 1)).unwrap();
    assert_eq!(content, "Disc 3.cue", "应写相对 base_path 的路径");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn single_disc_store_disc_noop() {
    let dir = temp_dir("disc_single");
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.sfc",
        vec![],
        "",
    );
    io.store_disc(0, 0).unwrap();
    assert!(
        !disc_memory_path(dir.to_str().unwrap(), "Game.sfc", 0).contains("Game.sfc.0.txt")
            || !std::path::Path::new(&disc_memory_path(dir.to_str().unwrap(), "Game.sfc", 0))
                .exists(),
        "单碟不应写碟片记忆"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ── 读档换碟请求（spec 场景「读档换碟请求」「无存档时读档空操作」）──

#[test]
fn disc_change_request_when_memory_differs() {
    let dir = temp_dir("disc_change");
    let base = format!("{}/Game (Disc 1)/", dir.to_str().unwrap());
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.m3u",
        vec![
            format!("{base}Disc 1.cue"),
            format!("{base}Disc 2.cue"),
            format!("{base}Disc 3.cue"),
        ],
        &base,
    );
    // 记忆碟 2，当前碟 0 → 换碟请求
    io.store_disc(0, 1).unwrap();
    let req = io.disc_change_request(0, 0);
    assert_eq!(
        req,
        Some(DiscRequest {
            path: format!("{base}Disc 2.cue")
        })
    );
    // 当前碟已是 2 → 无请求
    assert_eq!(io.disc_change_request(0, 1), None);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn disc_change_request_no_memory_noop() {
    let dir = temp_dir("disc_no_mem");
    let base = format!("{}/Game/", dir.to_str().unwrap());
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.m3u",
        vec![format!("{base}Disc 1.cue"), format!("{base}Disc 2.cue")],
        &base,
    );
    assert_eq!(io.disc_change_request(0, 0), None, "无记忆文件应无请求");
    let _ = fs::remove_dir_all(&dir);
}

// ── save_exists / preview_exists（spec 场景「存档状态查询」）──

#[test]
fn save_exists_reflects_snapshot_file() {
    let dir = temp_dir("save_exists");
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.sfc",
        vec![],
        "",
    );
    assert!(!io.save_exists(0));
    // 铺一个快照文件 {states_dir}/Game.sfc.st0
    fs::write(dir.join("Game.sfc.st0"), [0u8; 4]).unwrap();
    assert!(io.save_exists(0));
    assert!(!io.save_exists(1));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn preview_exists_requires_save_and_bmp() {
    let dir = temp_dir("preview_exists");
    let io = SaveStateIo::new(
        dir.to_str().unwrap(),
        dir.to_str().unwrap(),
        "Game.sfc",
        vec![],
        "",
    );
    // 无快照 → false
    assert!(!io.preview_exists(0));
    // 有快照无 bmp → false
    fs::write(dir.join("Game.sfc.st0"), [0u8; 4]).unwrap();
    assert!(!io.preview_exists(0));
    // 有 bmp → true
    fs::write(dir.join("Game.sfc.0.bmp"), [0u8; 4]).unwrap();
    assert!(io.preview_exists(0));
    let _ = fs::remove_dir_all(&dir);
}

// ═══════════════════════════════════════════════════════════════
// 4. 菜单绘制
// ═══════════════════════════════════════════════════════════════

// ── 主菜单绘制（spec 场景「主菜单绘制结构」）──

#[test]
fn draw_main_menu_produces_structure() {
    let theme = theme();
    // 640×360（scale=2 的 320×180 逻辑屏）
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(false, 1);
    draw_main_menu(&theme, &mut screen, &overlay, "My Game", &menu, None, None);

    // 顶部名称条区域有白色文字（游戏名）
    assert!(
        rect_has_white(&screen, 40, 40, 400, 60),
        "名称条区域应有白色文字"
    );
    // 菜单项区域（垂直居中 5 项）应有内容
    let list_top = (360 - MENU_ITEM_COUNT as u32 * 60) / 2;
    assert!(
        rect_has_white(&screen, 40, list_top, 300, MENU_ITEM_COUNT as u32 * 60),
        "菜单项区域应有内容"
    );
    // 底部按钮组区域
    assert!(
        rect_has_white(&screen, 0, 360 - 100, 640, 100),
        "底部按钮组应有内容"
    );
}

#[test]
fn draw_main_menu_selected_item_white_pill_black_text() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(false, 1);
    draw_main_menu(&theme, &mut screen, &overlay, "Game", &menu, None, None);

    // 选中项（Continue，第一行）所在行：白色药丸背景
    let list_top = (360 - MENU_ITEM_COUNT as u32 * 60) / 2;
    // 白色药丸从 x=pad(20) 开始
    assert!(
        rect_has_white(&screen, 20, list_top + 20, 80, 60),
        "选中项应有白色药丸"
    );
    // 选中行文字为黑色（药丸上的黑字）
    assert!(
        rect_has_black(&screen, 40, list_top + 24, 60, 50),
        "选中项文字应为黑色"
    );
}

#[test]
fn draw_main_menu_unselected_items_have_shadow() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(false, 1);
    draw_main_menu(&theme, &mut screen, &overlay, "Game", &menu, None, None);

    // 未选中项（第 2 行 Save）有白字
    let list_top = (360 - MENU_ITEM_COUNT as u32 * 60) / 2;
    let save_y = list_top + 20 + 60;
    assert!(
        rect_has_white(&screen, 40, save_y, 60, 50),
        "未选中项应有白色文字"
    );
}

#[test]
fn draw_main_menu_simple_mode_shows_reset() {
    // simple_mode 绘制不 panic（Reset 文案替代 Options）
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(true, 1);
    draw_main_menu(&theme, &mut screen, &overlay, "Game", &menu, None, None);
    assert!(screen.pixels.iter().any(|&p| p != 0));
}

// ── 多碟/槽位预览（spec 场景「多碟显示当前碟片」「槽位预览窗三态」）──

#[test]
fn draw_main_menu_disc_name_on_continue() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let mut menu = MainMenu::new(false, 3);
    menu.update(&press_right()); // disc 1
    draw_main_menu(
        &theme,
        &mut screen,
        &overlay,
        "Game",
        &menu,
        Some("Disc 2"),
        None,
    );
    // 右侧碟片名区域应有白色文字（列表 oy=30，Continue 行 y=50..110，碟片名靠右）
    assert!(
        rect_has_white(&screen, 400, 50, 240, 60),
        "Continue 行右侧应显示碟片名"
    );
}

#[test]
fn draw_main_menu_slot_preview_empty_slot() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(false, 1);
    let slot = minarch::menu::SlotState {
        save_exists: false,
        preview: None,
    };
    draw_main_menu(
        &theme,
        &mut screen,
        &overlay,
        "Game",
        &menu,
        None,
        Some(&slot),
    );
    // 预览窗区域（右侧半屏）应有内容（窗口骨架 + "Empty Slot" 文案）
    assert!(
        rect_has_white(&screen, 320, 60, 300, 240),
        "预览窗区域应有内容"
    );
}

#[test]
fn draw_main_menu_slot_preview_no_preview_text() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(false, 1);
    let slot = minarch::menu::SlotState {
        save_exists: true,
        preview: None,
    };
    draw_main_menu(
        &theme,
        &mut screen,
        &overlay,
        "Game",
        &menu,
        None,
        Some(&slot),
    );
    assert!(
        rect_has_white(&screen, 320, 60, 300, 240),
        "有存档无预览应显示 No Preview 文案"
    );
}

#[test]
fn draw_main_menu_slot_preview_injected_thumbnail() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 360);
    let overlay = VideoBuffer::new(640, 360);
    let menu = MainMenu::new(false, 1);
    // 注入一张全白缩略图
    let preview = VideoBuffer::new(100, 80);
    let slot = minarch::menu::SlotState {
        save_exists: true,
        preview: Some(&preview),
    };
    draw_main_menu(
        &theme,
        &mut screen,
        &overlay,
        "Game",
        &menu,
        None,
        Some(&slot),
    );
    assert!(
        rect_has_white(&screen, 340, 80, 200, 160),
        "注入的缩略图应被绘制"
    );
}

// ── 选项菜单绘制（spec 场景「选项菜单行宽缓存」「选项菜单滚动指示」「描述文字渲染」）──

#[test]
fn draw_option_menu_list_layout() {
    let theme = theme();
    // 选项菜单行距 = PILL_SIZE×scale（60），720 高屏可容纳 7 行
    let mut screen = VideoBuffer::new(640, 720);
    let menu = OptionMenu::new(list_of(3), 7, None, None);
    draw_option_menu(
        &theme,
        &mut screen,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );
    // 列表项区域应有白色药丸（选中行；列表居中，行宽 = 最宽项 + 内边距）
    // List 布局 ox 居中于屏幕，检查屏幕中央区域
    assert!(
        rect_has_white(&screen, 200, 80, 240, 40),
        "选项列表应有选中行白药丸"
    );
}

#[test]
fn draw_option_menu_fixed_layout() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 720);
    let menu = OptionMenu::new(
        vec![MenuItem {
            kind: ItemKind::Fixed,
            values: vec!["Native".into(), "Aspect".into()],
            value: 1,
            ..MenuItem::list("Scaling")
        }],
        7,
        None,
        None,
    );
    draw_option_menu(
        &theme,
        &mut screen,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );
    assert!(
        rect_has_white(&screen, 20, 80, 500, 40),
        "Fixed 布局应有整行灰药丸 + 白药丸"
    );
}

#[test]
fn draw_option_menu_var_layout_value_right() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 720);
    let menu = OptionMenu::new(
        vec![MenuItem::var(
            "Mode",
            vec!["Alpha".into(), "Beta".into()],
            1,
        )],
        7,
        None,
        None,
    );
    draw_option_menu(
        &theme,
        &mut screen,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );
    // 右侧值区域应有白色文字（Beta）
    assert!(
        rect_has_white(&screen, 400, 80, 200, 40),
        "Var 布局右侧应显示值"
    );
}

#[test]
fn draw_option_menu_scroll_arrows() {
    let theme = theme();
    let mut menu = OptionMenu::new(list_of(12), 7, None, None);
    // 中间位置（selected=7, start=1）：顶部 + 底部箭头都显示
    for _ in 0..7 {
        menu.update(&press_down());
    }
    let mut mid = VideoBuffer::new(640, 720);
    draw_option_menu(
        &theme,
        &mut mid,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );

    // 首页：无顶部箭头
    let menu_top = OptionMenu::new(list_of(12), 7, None, None);
    let mut top = VideoBuffer::new(640, 720);
    draw_option_menu(
        &theme,
        &mut top,
        &minarch::menu::OptionDrawState {
            menu: &menu_top,
            show_settings: false,
        },
    );

    // 末页：无底部箭头（selected=11, start=5, end=12——11 次 DOWN）
    let mut menu_bottom = OptionMenu::new(list_of(12), 7, None, None);
    for _ in 0..11 {
        menu_bottom.update(&press_down());
    }
    let mut bottom = VideoBuffer::new(640, 720);
    draw_option_menu(
        &theme,
        &mut bottom,
        &minarch::menu::OptionDrawState {
            menu: &menu_bottom,
            show_settings: false,
        },
    );

    // 顶部箭头区域（屏幕顶部中央）
    let arrow_top_mid = rect_has_white(&mid, 280, 10, 80, 20);
    let arrow_top_page = rect_has_white(&top, 280, 10, 80, 20);
    assert!(arrow_top_mid, "中间位置应显示顶部箭头");
    assert!(!arrow_top_page, "首页不应显示顶部箭头");

    // 底部箭头区域：sy = 720 - 20 - 60 - 60 + (60-8)/2 = 606，箭头 606..614
    let arrow_bottom_mid = rect_has_white(&mid, 280, 600, 80, 20);
    let arrow_bottom_page = rect_has_white(&bottom, 280, 600, 80, 20);
    assert!(arrow_bottom_mid, "中间位置应显示底部箭头");
    assert!(!arrow_bottom_page, "末页不应显示底部箭头");
}

#[test]
fn draw_option_menu_desc_bottom() {
    let theme = theme();
    let mut screen = VideoBuffer::new(640, 720);
    let menu = OptionMenu::new(
        vec![MenuItem {
            desc: Some("A detailed description".into()),
            ..MenuItem::var("Mode", vec!["A".into()], 0)
        }],
        7,
        None,
        None,
    );
    draw_option_menu(
        &theme,
        &mut screen,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );
    // 底部描述区域应有白色文字（描述 y = 720 - 20 - h，接近屏幕底部）
    assert!(
        rect_has_white(&screen, 100, 670, 440, 40),
        "描述文字应渲染在底部"
    );
}

#[test]
fn draw_option_menu_list_width_cached() {
    // 行宽缓存：第二次绘制与第一次一致（不重新计算）
    let theme = theme();
    let menu = OptionMenu::new(list_of(3), 7, None, None);
    let mut s1 = VideoBuffer::new(640, 720);
    draw_option_menu(
        &theme,
        &mut s1,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );
    let mut s2 = VideoBuffer::new(640, 360);
    draw_option_menu(
        &theme,
        &mut s2,
        &minarch::menu::OptionDrawState {
            menu: &menu,
            show_settings: false,
        },
    );
    // 两帧非零像素分布一致（布局稳定）
    let nz1 = s1.pixels.iter().filter(|&&p| p != 0).count();
    let nz2 = s2.pixels.iter().filter(|&&p| p != 0).count();
    assert_eq!(nz1, nz2, "两次绘制应产生一致的布局");
}
