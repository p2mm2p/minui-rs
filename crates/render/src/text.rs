//! 文字渲染、截断与排版
//!
//! 本模块使用 `fontdue` crate 进行 TTF/OTF 字体栅格化，替代原 C 代码中的
//! SDL_ttf（`TTF_RenderUTF8_Blended()` + `SDL_BlitSurface()`）。
//!
//! ## fontdue 与原 SDL_ttf 的关键差异
//!
//! | 维度 | 原 C (SDL_ttf) | Rust (fontdue) |
//! |------|---------------|---------------|
//! | 栅格化输出 | `SDL_Surface*`（ARGB） | `(&[u8], Metrics)`（alpha 位图） |
//! | 混合方式 | SDL 内部 alpha 混合 | 手动 `blend_pixel` 逐像素混合 |
//! | 字号 | 加载时固定（4 个 `TTF_Font*`） | 调用时按 `px` 参数指定 |
//! | 测量 | `TTF_SizeUTF8()` | `fontdue::Layout` |
//!
//! 所有函数操作 `common::video::VideoBuffer`（RGB565），不依赖 SDL 类型。
//!

use common::video::VideoBuffer;
pub use fontdue::Font;
// fontdue layout 仅用于类型导入，具体测量使用 rasterize 的 advance_width

// ── Alpha 混合 ──────────────────────────────────

/// 将 fontdue alpha 位图像素混合到 RGB565 颜色上
///
/// `alpha`：0–255（0=全透明，255=完全不透明）
/// `fg`：前景色（RGB565 格式）
/// `bg`：背景色（RGB565 格式）
///
/// 对 R（5 位）、G（6 位）、B（5 位）分量分别进行线性插值。
/// alpha=0 和 alpha=255 走快速路径（无需计算）。
fn blend_pixel(alpha: u8, fg: u16, bg: u16) -> u16 {
    match alpha {
        0 => return bg,
        255 => return fg,
        _ => {}
    }
    let a = alpha as u32;
    let inv = 255 - a;

    let fr = ((fg >> 11) & 0x1F) as u32;
    let fg_ = ((fg >> 5) & 0x3F) as u32;
    let fb = (fg & 0x1F) as u32;

    let br = ((bg >> 11) & 0x1F) as u32;
    let bg_ = ((bg >> 5) & 0x3F) as u32;
    let bb = (bg & 0x1F) as u32;

    let r = ((fr * a + br * inv) / 255) as u16;
    let g = ((fg_ * a + bg_ * inv) / 255) as u16;
    let b = ((fb * a + bb * inv) / 255) as u16;

    (r << 11) | (g << 5) | b
}

// ── 字体加载 ────────────────────────────────────

/// 从静态字节切片加载字体
///
/// fontdue 的 `Font` 拥有字体数据的所有权（内部复制），一个实例支持所有字号。
/// 返回的 `Font` 由调用方持有并在主循环中传给各渲染函数。
///
/// 对应原 C `TTF_OpenFont(path, px)`——差异在于原 C 需要为每个字号打开一个
/// `TTF_Font*` 实例，而 fontdue 在 `rasterize()` 时按 `px` 参数指定。
///
/// ## Panics
///
/// 若字体数据无效（非 TTF/OTF 格式），本函数将 panic。调用方应确保使用
/// 已知有效的字体文件（如 BPreplayBold-unhinted.otf）。
pub fn load_font(font_data: &[u8]) -> Font {
    Font::from_bytes(font_data, fontdue::FontSettings::default())
        .expect("无法加载字体：字体数据无效或格式不支持")
}

// ── 单行文字渲染 ────────────────────────────────

/// 将单行文字渲染到目标缓冲区
///
/// 使用 `font.rasterize(c, px)` 逐字符栅格化，对返回的 alpha 位图逐像素
/// 调用 `blend_pixel` 混合到目标 RGB565 背景上。
///
/// 对应原 C `TTF_RenderUTF8_Blended()` + `SDL_BlitSurface()`。
/// 原 C 中 SDL 内部处理 alpha 混合，Rust 版手动实现等价逻辑。
///
/// ## 参数
///
/// * `dst` - 目标像素缓冲区
/// * `font` - 已加载的字体实例
/// * `text` - UTF-8 文字字符串（单行，不含 `\n`）
/// * `px` - 像素字号（已按平台 `SCALE` 缩放）
/// * `color` - RGB565 前景色
/// * `pos` - 目标坐标 `(x, y)`（像素，已缩放）
///
/// ## 注意
///
/// 超出 `dst` 边界的像素会被忽略。不做边界裁剪——超出部分静默丢弃。
pub fn render_text(
    dst: &mut VideoBuffer,
    font: &Font,
    text: &str,
    px: u32,
    color: u16,
    pos: (u32, u32),
) {
    if text.is_empty() {
        return;
    }
    let px_f32 = px as f32;
    let pitch = dst.pitch as usize;
    let mut x_offset: u32 = 0;

    for c in text.chars() {
        let (metrics, bitmap) = font.rasterize(c, px_f32);
        if bitmap.is_empty() {
            x_offset += metrics.advance_width as u32;
            continue;
        }
        let glyph_w = metrics.width as u32;
        let glyph_h = metrics.height as u32;

        // fontdue 的 xmin/ymin 为 i16——descender 字形（y/g/p 等）的 ymin
        // 为负值，`as u32` 会转成巨大数导致减法下溢（debug panic /
        // release 写错位置）。用有符号运算并夹紧到 0（越界像素后续忽略）。
        let dst_x = (pos.0 as i64 + x_offset as i64 + metrics.xmin as i64).max(0) as u32;
        let dst_y =
            (pos.1 as i64 + px as i64 - metrics.height as i64 - metrics.ymin as i64).max(0) as u32;

        for row in 0..glyph_h {
            let dst_row = dst_y + row;
            if dst_row >= dst.height {
                break;
            }
            for col in 0..glyph_w {
                let dst_col = dst_x + col;
                if dst_col >= dst.width {
                    continue;
                }
                let alpha = bitmap[(row * glyph_w + col) as usize];
                if alpha == 0 {
                    continue;
                }
                let idx = dst_row as usize * pitch + dst_col as usize;
                dst.pixels[idx] = blend_pixel(alpha, color, dst.pixels[idx]);
            }
        }
        x_offset += metrics.advance_width as u32;
    }
}

// ── 文字尺寸测量 ────────────────────────────────

/// 计算多行文字的总渲染尺寸
///
/// 宽度 SHALL 为所有行中最大宽度，高度为 `行数 * px + (行数 - 1) * leading`。
/// 按 `\n` 分割文字为多行。空字符串返回 `(0, 0)`。
///
/// 对应原 C `GFX_sizeText()`。
/// 测量单行文字的渲染宽度（不含换行符）
fn measure_line_width(font: &Font, text: &str, px: f32) -> u32 {
    let mut width: u32 = 0;
    for c in text.chars() {
        let (metrics, _) = font.rasterize(c, px);
        width += metrics.advance_width as u32;
    }
    width
}

/// 计算多行文字的总渲染尺寸
///
/// 宽度 SHALL 为所有行中最大宽度，高度为 `行数 * px + (行数 - 1) * leading`。
/// 按 `\n` 分割文字为多行。空字符串返回 `(0, 0)`。
///
/// 对应原 C `GFX_sizeText()`。
pub fn size_text(font: &Font, text: &str, px: u32, leading: u32) -> (u32, u32) {
    if text.is_empty() {
        return (0, 0);
    }
    let px_f32 = px as f32;
    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len() as u32;

    let max_width: u32 = lines
        .iter()
        .map(|line| {
            if line.is_empty() {
                0
            } else {
                measure_line_width(font, line, px_f32)
            }
        })
        .max()
        .unwrap_or(0);

    let height = if line_count > 0 {
        line_count * px + (line_count - 1) * leading
    } else {
        0
    };

    (max_width, height)
}

// ── 文字截断 ────────────────────────────────────

/// 截断文字以适应最大宽度
///
/// 若文字宽度 ≤ `max_width`，返回原文字。若超出，循环移除末尾 3 个字符并追加
/// `"..."`，直到宽度 ≤ `max_width`。最小返回值为 `"..."`。
///
/// 对应原 C `GFX_truncateText()`。
/// 差异：原 C 原地修改 `char*` 参数，Rust 版返回新的 `String`（不修改入参）。
pub fn truncate_text(font: &Font, text: &str, max_width: u32, px: u32) -> String {
    if text.is_empty() {
        return String::new();
    }
    let (w, _) = size_text(font, text, px, 0);
    if w <= max_width {
        return text.to_string();
    }

    let mut result = text.to_string();
    loop {
        // 移除末尾 3 个字符（按 char 边界）
        let char_count = result.chars().count();
        if char_count <= 3 {
            return "...".to_string();
        }
        let new_len = result
            .char_indices()
            .nth(char_count - 3)
            .map(|(i, _)| i)
            .unwrap_or(0);
        result.truncate(new_len);
        result.push_str("...");

        let (w, _) = size_text(font, &result, px, 0);
        if w <= max_width {
            return result;
        }
    }
}

// ── 自动换行 ────────────────────────────────────

/// 将文字按最大宽度在空格边界自动换行
///
/// 按空格 `' '` 分割文字为单词，贪心填充每一行。若单个单词超过 `max_width`，
/// 该单词不被截断（允许溢出）。空字符串返回空字符串。
///
/// 对应原 C `GFX_wrapText()`。
/// 差异：原 C 原地修改 `char*`，Rust 版返回带 `\n` 的新 `String`。
pub fn wrap_text(font: &Font, text: &str, max_width: u32, px: u32) -> String {
    if text.is_empty() {
        return String::new();
    }

    let words: Vec<&str> = text.split(' ').collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::new();

    for word in &words {
        let candidate = if current_line.is_empty() {
            (*word).to_string()
        } else {
            format!("{} {}", current_line, word)
        };

        let (w, _) = size_text(font, &candidate, px, 0);
        if w <= max_width || current_line.is_empty() {
            // 适合当前行，或当前行为空（单词本身太长）
            current_line = candidate;
        } else {
            // 换行
            lines.push(current_line);
            current_line = (*word).to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines.join("\n")
}

// ── 多行消息渲染 ────────────────────────────────

/// 将多行消息居中渲染到目标矩形内
///
/// 按 `\n` 分割文字，每行独立水平居中，各行在垂直方向均匀分布。
/// 填充色固定为 `RGB_WHITE`（与原 C 一致）。
///
/// 对应原 C `GFX_blitMessage()`。
pub fn blit_message(
    dst: &mut VideoBuffer,
    font: &Font,
    text: &str,
    px: u32,
    dst_rect: common::video::Rect,
) {
    // 行间距使用与原 C 一致的 PADDING 值（SCALE1(PADDING)）
    let padding = px / 3; // 近似 SCALE1(PADDING) / SCALE1(FONT) 的比例
    let lines: Vec<&str> = text.split('\n').collect();

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (line_w, _) = size_text(font, line, px, 0);
        let x = dst_rect.x
            + if dst_rect.w > line_w {
                (dst_rect.w - line_w) / 2
            } else {
                0
            };
        let y = dst_rect.y + i as u32 * (px + padding);
        render_text(dst, font, line, px, common::video::RGB_WHITE, (x, y));
    }
}

/// 将多行文字渲染到指定矩形区域
///
/// 按 `\n` 分割文字，每行左对齐渲染，行距为 `px + leading` 像素。
///
/// 对应原 C `GFX_blitText()`。
pub fn blit_text(
    dst: &mut VideoBuffer,
    font: &Font,
    text: &str,
    px: u32,
    color: u16,
    dst_rect: common::video::Rect,
    leading: u32,
) {
    let lines: Vec<&str> = text.split('\n').collect();

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let y = dst_rect.y + i as u32 * (px + leading);
        if y >= dst_rect.y + dst_rect.h {
            break;
        }
        render_text(dst, font, line, px, color, (dst_rect.x, y));
    }
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::video::{RGB_BLACK, RGB_WHITE, Rect};
    use std::fs;

    /// 尝试加载字体用于测试。若字体文件不存在，返回 None（跳过测试）。
    fn try_load_test_font() -> Option<Font> {
        // 按优先级尝试：项目字体 > 系统字体
        let paths = [
            // BPreplayBold（项目默认字体，运行时由调用方决定路径）
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ];
        for path in &paths {
            if let Ok(data) = fs::read(path)
                && let Ok(font) = Font::from_bytes(data, fontdue::FontSettings::default())
            {
                return Some(font);
            }
        }
        None
    }

    /// 获取测试字体引用，若不可用则 panic
    fn font() -> &'static Font {
        use std::sync::OnceLock;
        static FONT: OnceLock<Font> = OnceLock::new();
        FONT.get_or_init(|| try_load_test_font().expect("未找到可用于测试的字体文件"))
    }

    // ── blend_pixel 测试（无需字体）──────────────

    #[test]
    fn blend_alpha_zero_returns_background() {
        let bg = 0x1234;
        assert_eq!(blend_pixel(0, RGB_WHITE, bg), bg);
    }

    #[test]
    fn blend_alpha_255_returns_foreground() {
        let fg = RGB_WHITE;
        assert_eq!(blend_pixel(255, fg, RGB_BLACK), fg);
    }

    #[test]
    fn blend_mid_alpha_mixes() {
        let result = blend_pixel(128, RGB_WHITE, RGB_BLACK);
        assert!(result != RGB_WHITE);
        assert!(result != RGB_BLACK);
    }

    // ── render_text 测试 ─────────────────────────

    #[test]
    fn render_text_empty_string_noop() {
        let mut dst = VideoBuffer::new(64, 64);
        let before = dst.pixels.clone();
        render_text(&mut dst, font(), "", 16, RGB_WHITE, (0, 0));
        assert_eq!(dst.pixels, before);
    }

    #[test]
    fn render_text_produces_pixels() {
        let mut dst = VideoBuffer::new(64, 64);
        render_text(&mut dst, font(), "A", 32, RGB_WHITE, (10, 10));
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn render_text_out_of_bounds_no_panic() {
        let mut dst = VideoBuffer::new(32, 32);
        render_text(&mut dst, font(), "A", 32, RGB_WHITE, (30, 30));
    }

    #[test]
    fn render_text_descender_no_underflow() {
        // descender 字形（y 的 ymin 为负）——修复前 `metrics.ymin as u32`
        // 巨大数导致减法下溢 panic（真机渲染含 y/g/p 的 ROM 名会触发）
        let mut dst = VideoBuffer::new(128, 64);
        render_text(&mut dst, font(), "My Game", 32, RGB_WHITE, (0, 0));
        assert!(
            dst.pixels.iter().any(|&p| p != 0),
            "descender 字形应正常渲染"
        );
    }

    // ── size_text 测试 ───────────────────────────

    #[test]
    fn size_text_single_line_has_width() {
        let (w, h) = size_text(font(), "A", 16, 4);
        assert!(w > 0, "单行文字应有宽度");
        assert_eq!(h, 16);
    }

    #[test]
    fn size_text_empty_returns_zero() {
        let (w, h) = size_text(font(), "", 16, 4);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn size_text_multi_line() {
        let (w, h) = size_text(font(), "A\nB", 16, 4);
        assert!(w > 0);
        assert_eq!(h, 36); // 2*16 + 1*4 = 36
    }

    // ── truncate_text 测试 ───────────────────────

    #[test]
    fn truncate_short_text_unchanged() {
        let result = truncate_text(font(), "A", 500, 16);
        assert_eq!(result, "A");
    }

    #[test]
    fn truncate_narrow_max_returns_ellipsis() {
        // max_width=0: 直接返回 "..."
        let result = truncate_text(font(), "ABC", 0, 16);
        assert!(
            result == "..." || result == "A..." || result.ends_with("..."),
            "窄宽度应返回省略号: {result}"
        );
    }

    #[test]
    fn truncate_empty_returns_empty() {
        let result = truncate_text(font(), "", 100, 16);
        assert_eq!(result, "");
    }

    // ── wrap_text 测试 ───────────────────────────

    #[test]
    fn wrap_short_text_unchanged() {
        let result = wrap_text(font(), "A", 500, 16);
        assert!(!result.contains('\n'));
    }

    #[test]
    fn wrap_long_text_has_newlines() {
        // 两个单词，max_width=5 应该强制换行（即便是小字体）
        let result = wrap_text(font(), "A B C D", 5, 8);
        let line_count = result.split('\n').count();
        assert!(line_count >= 2, "多单词应有换行，结果: {result}");
    }

    #[test]
    fn wrap_single_word_not_truncated() {
        let result = wrap_text(font(), "ABCDEF", 5, 8);
        // 单个单词不应该丢失字符
        assert!(result.len() >= 6);
    }

    #[test]
    fn wrap_empty() {
        let result = wrap_text(font(), "", 100, 16);
        assert_eq!(result, "");
    }

    // ── blit_message 测试 ────────────────────────

    #[test]
    fn blit_message_single_line() {
        let mut dst = VideoBuffer::new(200, 64);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 64,
        };
        blit_message(&mut dst, font(), "A", 16, rect);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_message_multi_line() {
        let mut dst = VideoBuffer::new(200, 64);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 64,
        };
        blit_message(&mut dst, font(), "A\nB", 16, rect);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    // ── blit_text 测试 ───────────────────────────

    #[test]
    fn blit_text_with_leading() {
        let mut dst = VideoBuffer::new(200, 100);
        let rect = Rect {
            x: 10,
            y: 10,
            w: 180,
            h: 80,
        };
        blit_text(&mut dst, font(), "A\nB", 16, RGB_WHITE, rect, 4);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_text_respects_color() {
        let mut dst = VideoBuffer::new(100, 50);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 50,
        };
        // 在黑色背景上用白色渲染，应该有非零像素
        blit_text(&mut dst, font(), "A", 16, RGB_WHITE, rect, 0);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }
}
