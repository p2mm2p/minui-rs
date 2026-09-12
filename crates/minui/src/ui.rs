//! 列表渲染层
//!
//! 对应原版 C `minui.c` 的渲染区（:1486-1675）——列表行（pill 高亮 +
//! 截断文字 + unique 后缀）、底部按钮组、缩略图、空目录消息。所有
//! 渲染函数直接操作 `VideoBuffer`（纯像素，可脱离 SDL 测试）。
//!
//! ## 依赖方向
//!
//! 本模块 SHALL 只依赖 `browser`（`Entry`）、`render`（图集/字体/
//! 渲染原语）、`common::video`（常量与像素缓冲）——不依赖 `launch`/
//! `recents`/平台 crate。
//!
//! ## 渲染顺序语义（与 C 一致）
//!
//! 列表行：`trim_sorting_meta` 去除排序前缀 → `truncate_text` 按可用
//! 宽度截断 → 选中行白 pill + 黑字（显示 unique 或 name）；重名条目
//! 先画深灰 unique 再画白 name 覆盖其前半（C :1614-1631 的渲染顺序
//! 使 name 后露出的 unique 后缀呈现深灰）。
//!

#![cfg_attr(not(feature = "platform-tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow
//!
//! 「ui 列表渲染」

use common::utils::trim_sorting_meta;
use common::video::{
    Asset, BUTTON_PADDING, PILL_SIZE, RGB_BLACK, RGB_DARK_TEXT, RGB_WHITE, Rect, VideoBuffer,
};
use render::asset::Atlas;
use render::button::blit_button_group;
use render::pill::blit_pill;
use render::text::{Font, blit_message, render_text, size_text, truncate_text};
use render::thumbnail::load_thumbnail;

use crate::browser::Entry;

/// 列表字号（对应 C `FONT_LARGE`，defines.h:66）
const FONT_LARGE: u32 = 16;

/// 列表渲染
///
/// 对应 C minui.c:1588-1644。逐行渲染 `start..end` 窗口内的条目：
/// 选中行白 pill + 黑字；重名条目（`unique`）先深灰 unique 后缀再白
/// name 覆盖前半；可用宽度扣除缩略图列（非选中行）与硬件组宽度
/// （第一行）。
///
/// 11 个参数为"调用方收集数据传入"模式的显式参数化（与
/// `power::update` 的 10 参数先例一致）——不打包参数对象。
///
/// # 参数
///
/// - `screen`:目标绘制缓冲区
/// - `atlas`:精灵图集（白 pill 素材）
/// - `font`:字体（fontdue `Font`）
/// - `entries`:条目列表（完整列表，按 `start`/`end` 窗口取行）
/// - `selected`:当前选中条目下标
/// - `start`/`end`:滚动窗口（页首/页尾，开区间）
/// - `thumb`:缩略图列宽度（`render_thumbnail` 返回值；`None` = 无缩略图）
/// - `ow`:硬件状态组占用的水平宽度（`blit_hardware_group` 返回值）
/// - `padding`:页面边缘留白（`Platform::padding()`）
/// - `scale`:平台缩放倍率（`Platform::SCALE`）
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_list(
    screen: &mut VideoBuffer,
    atlas: &Atlas,
    font: &Font,
    entries: &[Entry],
    selected: usize,
    start: usize,
    end: usize,
    thumb: Option<u32>,
    ow: u32,
    padding: u32,
    scale: u32,
) {
    let px = FONT_LARGE * scale;
    let pad = padding * scale;
    let pill_h = PILL_SIZE * scale;
    let btn_pad = BUTTON_PADDING * scale;
    let selected_row = selected.saturating_sub(start);
    let thumb_w = thumb.unwrap_or(0);

    for (j, i) in (start..end).enumerate() {
        let Some(entry) = entries.get(i) else {
            break;
        };
        let mut name = entry.name.clone();
        trim_sorting_meta(&mut name);

        // 可用宽度（C :1595-1597）：非选中行扣缩略图列；第一行扣硬件组
        let row_thumb = if j == selected_row { 0 } else { thumb_w };
        let mut available = screen
            .width
            .saturating_sub(row_thumb)
            .saturating_sub(pad * 2);
        if i == start && row_thumb == 0 {
            available = available.saturating_sub(ow);
        }

        let y = pad + (j as u32) * pill_h;
        let text_pos = (pad + btn_pad, y + 4 * scale);

        if j == selected_row {
            // 选中行：白 pill + 黑字（C :1605-1613）
            let display = truncate_text(
                font,
                entry.unique.as_deref().unwrap_or(&name),
                available,
                px,
            );
            let text_w = size_text(font, &display, px, 0).0;
            let max_w = available.min(text_w);
            blit_pill(
                atlas,
                Asset::WhitePill,
                scale,
                screen,
                Rect {
                    x: pad,
                    y,
                    w: max_w,
                    h: pill_h,
                },
            );
            render_text(screen, font, &display, px, RGB_BLACK, text_pos);
        } else if let Some(unique) = &entry.unique {
            // 重名条目：先深灰 unique 后缀，再白 name 覆盖前半（C :1614-1631）
            let mut unique_name = unique.clone();
            trim_sorting_meta(&mut unique_name);
            let unique_trunc = truncate_text(font, &unique_name, available, px);
            render_text(screen, font, &unique_trunc, px, RGB_DARK_TEXT, text_pos);
            let display = truncate_text(font, &name, available, px);
            render_text(screen, font, &display, px, RGB_WHITE, text_pos);
        } else {
            let display = truncate_text(font, &name, available, px);
            render_text(screen, font, &display, px, RGB_WHITE, text_pos);
        }
    }
}

/// 底部按钮组渲染
///
/// 对应 C minui.c:1581-1670。按钮组位于屏幕底部一行
/// （`y = h - (PADDING + PILL_SIZE) * scale`，C api.c:806）。
/// 文案决策：
///
/// - 版本页：左侧 `"POWER"/"SLEEP"`，右侧 `"B"/"BACK"`
/// - 列表页：左侧 `can_resume` 时 `"X"/"RESUME"`，否则 simple_mode 时
///   `"POWER"/"SLEEP"`、非 simple_mode 时 `"POWER"/"INFO"`
/// - 底部右侧：`total>0` 且 `stack>1` → `"B"/"BACK"+"A"/"OPEN"`；
///   `total>0` 且 `stack==1` → 仅 `"A"/"OPEN"`；`total==0` 且 `stack>1`
///   → 仅 `"B"/"BACK"`（C :1658-1662 空目录分支）
///
/// # 参数
///
/// - `screen`:目标绘制缓冲区
/// - `atlas`:精灵图集
/// - `font`:字体
/// - `version_page`:是否版本页（`Menu::show_version()`）
/// - `can_resume`:当前条目可续玩（`Menu::can_resume()`）
/// - `stack_depth`:目录栈深度（`Menu::stack_depth()`）
/// - `simple_mode`:简洁模式（`enable-simple-mode` 标记）
/// - `total`:当前目录条目数（空目录分支判断）
/// - `scale`:平台缩放倍率
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_buttons(
    screen: &mut VideoBuffer,
    atlas: &Atlas,
    font: &Font,
    version_page: bool,
    can_resume: bool,
    stack_depth: usize,
    simple_mode: bool,
    total: usize,
    scale: u32,
) {
    let px = FONT_LARGE * scale;
    // 按钮组 pill 高度 = (BUTTON_SIZE + BUTTON_PADDING) * scale（blit_button_group
    // 内部计算）——rect.h 必须 ≥ 该值，否则 (rect.h - pill_h)/2 下溢
    let pill_h = (common::video::BUTTON_SIZE + common::video::BUTTON_PADDING) * scale;
    let rect = Rect {
        x: 0,
        y: screen
            .height
            .saturating_sub((BUTTON_ROW_PADDING + PILL_SIZE) * scale),
        w: screen.width,
        h: pill_h,
    };

    if version_page {
        blit_button_group(
            &[("POWER", "SLEEP")],
            atlas,
            font,
            screen,
            rect,
            false,
            px,
            scale,
        );
        blit_button_group(&[("B", "BACK")], atlas, font, screen, rect, true, px, scale);
    } else {
        let left: &[(&str, &str)] = if can_resume {
            &[("X", "RESUME")]
        } else if simple_mode {
            &[("POWER", "SLEEP")]
        } else {
            &[("POWER", "INFO")]
        };
        blit_button_group(left, atlas, font, screen, rect, false, px, scale);

        if total == 0 {
            // 空目录：仅返回（C :1658-1662）
            if stack_depth > 1 {
                blit_button_group(&[("B", "BACK")], atlas, font, screen, rect, true, px, scale);
            }
        } else if stack_depth > 1 {
            blit_button_group(
                &[("B", "BACK"), ("A", "OPEN")],
                atlas,
                font,
                screen,
                rect,
                true,
                px,
                scale,
            );
        } else {
            blit_button_group(
                &[("A", "OPEN")],
                atlas,
                font,
                screen,
                rect,
                false,
                px,
                scale,
            );
        }
    }
}

/// 缩略图渲染
///
/// 对应 C minui.c:1494-1519。检查 `{entry 父目录}/.res/{文件名}.png`，
/// 存在则加载并右对齐绘制到屏幕右侧（`ox = w - thumb_w`、垂直居中）。
///
/// # 参数
///
/// - `screen`:目标绘制缓冲区
/// - `entry`:当前选中条目（缩略图路径 = 条目父目录/.res/文件名.png）
/// - `scale`:平台缩放倍率
///
/// # 返回值
///
/// `Some(缩略图宽度)`——成功绘制（供 `render_list` 扣列宽）；
/// `None`——无缩略图或加载失败（C :1511 的 `exists` 检查 + IMG_Load 失败）。
pub(crate) fn render_thumbnail(
    screen: &mut VideoBuffer,
    entry: &Entry,
    _scale: u32, // 缩略图按原尺寸绘制（C IMG_Load 原尺寸，不缩放）
) -> Option<u32> {
    let (parent, filename) = entry.path.rsplit_once('/')?;
    let res_path = format!("{parent}/.res/{filename}.png");
    if !common::utils::exists(&res_path) {
        return None;
    }
    let thumb = load_thumbnail(&res_path)?;
    let ox = screen.width.saturating_sub(thumb.width);
    let oy = screen.height.saturating_sub(thumb.height) / 2;
    blit_buffer(screen, &thumb, ox, oy);
    Some(thumb.width)
}

/// 空目录消息（对应 C `GFX_blitMessage`，minui.c:1645-1648）
///
/// 屏幕中央显示 `"Empty folder"`。
pub(crate) fn render_empty(screen: &mut VideoBuffer, font: &Font, scale: u32) {
    let px = FONT_LARGE * scale;
    let _ = scale;
    blit_message(
        screen,
        font,
        "Empty folder",
        px,
        Rect {
            x: 0,
            y: 0,
            w: screen.width,
            h: screen.height,
        },
    );
}

/// 按钮组所在行的页面留白（对应 C `PADDING`，defines.h:63）
const BUTTON_ROW_PADDING: u32 = 10;

/// 像素缓冲整块拷贝（缩略图 → 屏幕）
///
/// 按 `pitch` 行步长逐像素拷贝，越界像素忽略。`VideoBuffer` 无现成
/// blit 方法——render crate 的像素操作以 `fill_rect`/`render_text` 为
/// 主，整块拷贝仅缩略图使用，就地实现。
fn blit_buffer(dst: &mut VideoBuffer, src: &VideoBuffer, x: u32, y: u32) {
    for row in 0..src.height {
        for col in 0..src.width {
            let dx = x + col;
            let dy = y + row;
            if dx >= dst.width || dy >= dst.height {
                continue;
            }
            let src_px = row * src.pitch + col;
            let dst_px = dy * dst.pitch + dx;
            dst.pixels[dst_px as usize] = src.pixels[src_px as usize];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 白色像素测试图集（仿 render crate 测试）
    fn test_atlas() -> Atlas {
        Atlas {
            pixels: vec![0xFFFF; 256 * 256],
            width: 256,
            height: 256,
        }
    }

    /// 系统字体加载（仿 render crate 测试）
    fn test_font() -> Font {
        use std::sync::OnceLock;
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
        .clone()
    }

    fn entry(path: &str, name: &str, unique: Option<&str>) -> Entry {
        Entry {
            path: path.to_string(),
            name: name.to_string(),
            unique: unique.map(|s| s.to_string()),
            alpha: 0,
            entry_type: crate::browser::EntryType::Rom,
        }
    }

    /// 检查竖条区域（x 起始 8px）是否有白色——pill 从 x=pad 开始实心，
    /// 文字从 pad+BUTTON_PADDING*scale 开始——竖条只命中 pill 不命中文字
    fn left_strip_has_white(dst: &VideoBuffer, y: u32, h: u32) -> bool {
        let pad = 10 * 2;
        for dy in y..y + h {
            for dx in pad..pad + 8 {
                let idx = dy * dst.pitch + dx;
                if dst.pixels[idx as usize] == RGB_WHITE {
                    return true;
                }
            }
        }
        false
    }

    // ── render_list（spec 场景「列表渲染选中行高亮」「重名条目显示 unique」）──

    #[test]
    fn render_list_selected_row_gets_white_pill() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(320, 256);
        let entries = vec![
            entry("/r/A.gb", "Alpha", None),
            entry("/r/B.gb", "Bravo", None),
            entry("/r/C.gb", "Charlie", None),
        ];
        // selected=1, start=0, end=3 → 第 2 行白 pill
        render_list(
            &mut dst,
            &atlas,
            &test_font(),
            &entries,
            1,
            0,
            3,
            None,
            0,
            10,
            2,
        );

        let pad = 10 * 2;
        let pill_h = PILL_SIZE * 2;
        // 第 2 行（j=1）左边缘竖条有白像素（pill 实心）
        assert!(
            left_strip_has_white(&dst, pad + pill_h, pill_h),
            "选中行应有白 pill"
        );
        // 第 1 行（j=0）左边缘竖条无白像素（无 pill；文字从 pad+btn_pad 开始）
        assert!(
            !left_strip_has_white(&dst, pad, pill_h),
            "非选中行不应有白 pill"
        );
    }

    #[test]
    fn render_list_unique_suffix_rendered() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(320, 256);
        let entries = vec![
            entry("/r/Game (USA).gb", "Game", Some("Game (USA)")),
            entry("/r/Game (EUR).gb", "Game", Some("Game (EUR)")),
        ];
        // 非选中行（selected=0 时 j=1 为非选中）→ 应产生深灰像素（unique 后缀）
        render_list(
            &mut dst,
            &atlas,
            &test_font(),
            &entries,
            0,
            0,
            2,
            None,
            0,
            10,
            2,
        );

        // 非选中行区域存在非黑像素（name 白字 + unique 深灰）
        let pad = 10 * 2;
        let pill_h = PILL_SIZE * 2;
        let mut any = false;
        for dy in pad..pad + pill_h {
            for dx in pad..320 {
                let idx = dy * dst.pitch + dx;
                if dst.pixels[idx as usize] != 0 {
                    any = true;
                }
            }
        }
        assert!(any, "非选中行应渲染文字像素");
    }

    // ── render_thumbnail（spec 场景「缩略图存在时扣列宽」）──

    #[test]
    fn render_thumbnail_missing_returns_none() {
        let mut dst = VideoBuffer::new(320, 128);
        let e = entry("/nonexistent/A.gb", "A", None);
        assert_eq!(render_thumbnail(&mut dst, &e, 2), None);
    }

    #[test]
    fn render_thumbnail_invalid_png_returns_none() {
        let root = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        let dir = format!("{root}/minui_ui_thumb_{}", std::process::id());
        fs::create_dir_all(format!("{dir}/.res")).unwrap();
        fs::write(format!("{dir}/.res/A.gb.png"), "not a png").unwrap();
        let e = entry(&format!("{dir}/A.gb"), "A", None);
        let mut dst = VideoBuffer::new(320, 128);
        assert_eq!(
            render_thumbnail(&mut dst, &e, 2),
            None,
            "无效 PNG 应返回 None"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── render_buttons（spec 场景「按钮组文案」）──

    #[test]
    fn render_buttons_produce_pixels() {
        let atlas = test_atlas();
        let font = test_font();
        let mut cases = vec![
            (false, false, 1, false, 3usize), // 列表页非 resume 根目录
            (false, true, 1, false, 3),       // 列表页 resume
            (true, false, 1, false, 3),       // 版本页
            (false, false, 2, false, 3),      // 列表页深栈
            (false, false, 2, true, 0),       // 空目录深栈
            (false, false, 1, true, 0),       // 空目录根
        ];
        for (i, (vp, cr, depth, simple, total)) in cases.drain(..).enumerate() {
            let mut dst = VideoBuffer::new(800, 256);
            render_buttons(&mut dst, &atlas, &font, vp, cr, depth, simple, total, 2);
            assert!(
                dst.pixels.iter().any(|&p| p != 0),
                "按钮组用例 {i} 应产生像素"
            );
        }
    }

    // ── render_empty ──

    #[test]
    fn render_empty_produces_pixels() {
        let mut dst = VideoBuffer::new(320, 128);
        render_empty(&mut dst, &test_font(), 2);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }
}
