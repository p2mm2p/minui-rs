//! 按钮渲染与布局
//!
//! 本模块实现 MinUI 底部按钮栏的渲染——按键图标 + 提示文字标签的组合。
//! 所有函数依赖 `asset.rs`（精灵图集）、`pill.rs`（背景药丸）和 `text.rs`（标签文字）。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.c` 中按钮渲染使用：
//! - `GFX_blitButton()`（`api.c:610`）：单字符按钮用 `GFX_blitAsset(ASSET_BUTTON)`，
//!   多字符标签用 `TTF_RenderUTF8_Blended()` + `SDL_BlitSurface()`
//! - `GFX_blitButtonGroup()`：`GFX_blitPill()` 背景 + 按钮排列
//! - `GFX_blitHardwareHints()`：底部亮度/音量调整提示
//! - `GFX_getButtonWidth()`：`TTF_SizeUTF8()` 测量文字宽度
//!
//! Rust 版使用 `blit_asset` 替代 `SDL_BlitSurface`，`render_text` 替代 SDL_ttf，
//! `blit_pill` 替代 `GFX_blitPill`。
//!

use crate::asset::{Atlas, blit_asset};
use crate::pill::blit_pill;
use crate::text::{render_text, size_text};
use common::video::{
    ASSET_RGBS, Asset, BUTTON_MARGIN, BUTTON_PADDING, BUTTON_SIZE, Rect, VideoBuffer,
};
use fontdue::Font;

/// 亮度/音量按钮的特殊标签（多字符，使用大号字体）
const BRIGHTNESS_BUTTON_LABEL: &str = "+ -";

// ── 宽度计算 ────────────────────────────────────

/// 计算单个按钮的像素宽度
///
/// 单字符按钮（如 "A"）：`BUTTON_SIZE * scale + BUTTON_MARGIN * scale + hint_width`
/// 多字符标签（如 "+ -"）：`BUTTON_SIZE * scale / 2 + label_width + BUTTON_MARGIN * scale + hint_width`
///
/// 对应原 C `GFX_getButtonWidth()`（`api.c:590`）。
/// 差异：使用 `size_text` 替代 `TTF_SizeUTF8`。
pub fn get_button_width(hint: &str, button: &str, font: &Font, px: u32, scale: u32) -> u32 {
    let margin = BUTTON_MARGIN * scale;

    let button_width = if button.len() == 1 {
        BUTTON_SIZE * scale
    } else {
        let (label_w, _) = size_text(font, button, px, 0);
        BUTTON_SIZE * scale / 2 + label_w
    };

    let (hint_w, _) = size_text(font, hint, px, 0);
    button_width + margin + hint_w + margin
}

// ── 单按钮渲染 ──────────────────────────────────

/// 绘制单个按钮提示——sprite 图标 + hint 文字标签
///
/// 单字符按钮：blit `ASSET_BUTTON` sprite（`BUTTON_SIZE × BUTTON_SIZE`），
/// hint 文字在右侧。
/// 多字符标签：先渲染标签文字（使用 `px` 字号），再在右侧渲染 hint 文字。
///
/// 对应原 C `GFX_blitButton()`（`api.c:610`）。
#[allow(clippy::too_many_arguments)]
pub fn blit_button(
    hint: &str,
    button: &str,
    atlas: &Atlas,
    font: &Font,
    dst: &mut VideoBuffer,
    x: u32,
    y: u32,
    px: u32,
    scale: u32,
) {
    let margin = BUTTON_MARGIN * scale;
    let mut ox = x;

    if button.len() == 1 {
        // 单字符按钮：blit ASSET_BUTTON sprite
        let button_px = BUTTON_SIZE * scale;
        blit_asset(atlas, Asset::Button, scale, dst, (ox, y), None);
        ox += button_px + margin;
    } else {
        // 多字符标签（如 "+ -"）：渲染标签文字
        // 标签字号：原 C 对 BRIGHTNESS_BUTTON_LABEL 使用 font.large，其他使用 font.tiny
        // Rust 版统一使用 px（调用方决定字号）
        let label_px = px;
        let (label_w, _) = size_text(font, button, label_px, 0);
        // 标签在按钮 sprite 区域内居中偏右
        let label_x = ox + BUTTON_SIZE * scale / 2;
        render_text(
            dst,
            font,
            button,
            label_px,
            ASSET_RGBS[Asset::Button as usize],
            (label_x, y),
        );
        ox = label_x + label_w + margin;
    }

    // 渲染 hint 文字
    render_text(
        dst,
        font,
        hint,
        px,
        ASSET_RGBS[Asset::Button as usize],
        (ox, y),
    );
}

// ── 按钮组渲染 ──────────────────────────────────

/// 在药丸背景上排列 1–2 个按钮
///
/// 按钮组由可选 pill 背景和最多 2 个按钮组成。支持居中或右对齐。
/// 对应原 C `GFX_blitButtonGroup()`（`api.c:634`）。
#[allow(clippy::too_many_arguments)]
pub fn blit_button_group(
    pairs: &[(&str, &str)],
    atlas: &Atlas,
    font: &Font,
    dst: &mut VideoBuffer,
    pill_rect: Rect,
    align_right: bool,
    px: u32,
    scale: u32,
) -> u32 {
    if pairs.is_empty() {
        return 0;
    }

    let padding = BUTTON_PADDING * scale / 2;

    // 测量所有按钮宽度
    let widths: Vec<u32> = pairs
        .iter()
        .map(|(hint, btn)| get_button_width(hint, btn, font, px, scale))
        .collect();

    let total_width: u32 = widths.iter().sum::<u32>() + if pairs.len() > 1 { padding } else { 0 };

    // 计算药丸尺寸和位置
    let pill_h = BUTTON_SIZE * scale + BUTTON_PADDING * scale;
    let pill_w = total_width + BUTTON_PADDING * scale;
    let pill_x = if align_right {
        pill_rect.x + pill_rect.w - pill_w
    } else if pill_rect.w > pill_w {
        pill_rect.x + (pill_rect.w - pill_w) / 2
    } else {
        pill_rect.x
    };
    let pill_y = if pill_rect.h > pill_h {
        pill_rect.y + (pill_rect.h - pill_h) / 2
    } else {
        pill_rect.y
    };
    let pill_rect_inner = Rect {
        x: pill_x,
        y: pill_y,
        w: pill_w,
        h: pill_h,
    };

    // 绘制 pill 背景
    blit_pill(atlas, Asset::WhitePill, scale, dst, pill_rect_inner);

    // 布局按钮
    let mut btn_x = pill_x + padding;
    let btn_y = pill_y + (pill_h - BUTTON_SIZE * scale) / 2;

    for (i, (hint, btn)) in pairs.iter().enumerate() {
        blit_button(hint, btn, atlas, font, dst, btn_x, btn_y, px, scale);
        btn_x += widths[i];
        if i < pairs.len() - 1 {
            btn_x += padding;
        }
    }

    pill_w
}

// ── 硬件提示渲染 ────────────────────────────────

/// 渲染底部亮度/音量调整提示按钮
///
/// 渲染底部亮度/音量调整提示按钮
///
/// 显示 `BRIGHTNESS_BUTTON_LABEL`（"+ -"）用于亮度/音量调整场景。
/// 仅渲染文字，不涉及精灵图集（原 C `GFX_blitHardwareHints` 也仅使用 TTF 文字）。
///
/// 对应原 C `GFX_blitHardwareHints()`（`api.c:650`）。
pub fn blit_hardware_hints(
    show_setting: bool,
    font: &Font,
    dst: &mut VideoBuffer,
    x: u32,
    y: u32,
    px: u32,
    scale: u32,
) {
    if !show_setting {
        return;
    }

    let label = BRIGHTNESS_BUTTON_LABEL;
    let label_px = px; // 原 C 使用 font.large
    let margin = BUTTON_MARGIN * scale;
    let button_sprite_w = BUTTON_SIZE * scale / 2;

    let (label_w, _) = size_text(font, label, label_px, 0);

    // 渲染标签文字（"+ -"）
    let label_x = x - label_w - button_sprite_w - margin;
    render_text(
        dst,
        font,
        label,
        label_px,
        ASSET_RGBS[Asset::Button as usize],
        (label_x, y),
    );
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::video::Rect;
    use std::sync::OnceLock;

    fn test_atlas() -> Atlas {
        Atlas {
            pixels: vec![0xFFFF; 256 * 256],
            width: 256,
            height: 256,
        }
    }

    fn test_font() -> &'static Font {
        static FONT: OnceLock<Font> = OnceLock::new();
        FONT.get_or_init(|| {
            let paths = [
                "/System/Library/Fonts/Helvetica.ttc",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
            ];
            for path in &paths {
                if let Ok(data) = std::fs::read(path)
                    && let Ok(f) = Font::from_bytes(data, fontdue::FontSettings::default())
                {
                    return f;
                }
            }
            panic!("No test font found");
        })
    }

    // ── get_button_width ─────────────────────────

    #[test]
    fn get_button_width_single_char() {
        let w = get_button_width("OK", "A", test_font(), 16, 2);
        assert!(w > 0, "单字符按钮应有宽度");
    }

    #[test]
    fn get_button_width_multi_char_wider_than_zero() {
        let w = get_button_width("亮度", "+ -", test_font(), 16, 2);
        assert!(w > 0, "多字符标签应有宽度");
    }

    // ── blit_button ──────────────────────────────

    #[test]
    fn blit_button_single_char_produces_pixels() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(200, 64);
        blit_button("打开", "A", &atlas, test_font(), &mut dst, 10, 10, 16, 2);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_button_multi_char_produces_pixels() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(300, 64);
        blit_button(
            "亮度调节",
            "+ -",
            &atlas,
            test_font(),
            &mut dst,
            10,
            10,
            16,
            2,
        );
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    // ── blit_button_group ────────────────────────

    #[test]
    fn blit_button_group_single_button() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(256, 128);
        let pairs = [("打开", "A")];
        // rect needs room for the pill: pill_h ≈ BUTTON_SIZE*scale + BUTTON_PADDING*scale = 64 at scale=2
        let rect = Rect {
            x: 8,
            y: 32,
            w: 200,
            h: 64,
        };
        blit_button_group(&pairs, &atlas, test_font(), &mut dst, rect, false, 16, 2);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_button_group_two_buttons() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(400, 128);
        let pairs = [("打开", "A"), ("返回", "B")];
        let rect = Rect {
            x: 8,
            y: 32,
            w: 360,
            h: 64,
        };
        blit_button_group(&pairs, &atlas, test_font(), &mut dst, rect, false, 16, 2);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    // ── blit_hardware_hints ──────────────────────

    #[test]
    fn blit_hardware_hints_produces_pixels() {
        let mut dst = VideoBuffer::new(320, 48);
        blit_hardware_hints(true, test_font(), &mut dst, 300, 10, 32, 2);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_hardware_hints_disabled_noop() {
        let mut dst = VideoBuffer::new(320, 48);
        let before = dst.pixels.clone();
        blit_hardware_hints(false, test_font(), &mut dst, 300, 10, 16, 2);
        assert_eq!(dst.pixels, before, "show_setting=false 应无操作");
    }
}
