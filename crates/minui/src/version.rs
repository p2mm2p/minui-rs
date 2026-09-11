//! 版本信息画面
//!
//! 按 MENU 键显示当前 MinUI 版本号、commit hash 和设备型号。
//! 对应原 C `minui.c` 的 `show_version` 渲染区（:1523-1586）。
//!
//! ## 渲染策略
//!
//! 进入版本页时**每帧直接渲染**（不缓存合成面板）——C 版懒构建缓存
//! SDL surface 的成本在 Rust 的 fontdue 文本渲染下可忽略，简单为主
//! （见 design 决策 2）。
//!
//! ## 依赖方向
//!
//! 本模块零内部依赖——只消费 `common`（utils/paths/video）与
//! `render::text`，不依赖 minui 的任何业务模块。
//!

#![cfg_attr(not(feature = "tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow

use common::video::{RGB_DARK_TEXT, RGB_WHITE};
use render::text::{Font, render_text, size_text};

/// 解析 version.txt 内容——release = 第一行，commit = 第二行
///
/// 对应原 C `minui.c:1526-1533`（getFile 后按换行截断）：release 取第一个
/// `\n` 之前的内容，commit 取倒数第二个 `\n` 之后的内容（即第二行）。
/// C 版在无第二行时崩溃（`strrchr` 返回 NULL 后解引用），Rust 版以
/// `lines()` 安全处理——无第二行时 commit 为空串（安全化改进）。
///
/// # 参数
///
/// - `content`:version.txt 的完整内容（UTF-8 文本，可含 CRLF）
///
/// # 返回值
///
/// `(release, commit)`——release 为第一行（无行时为空串），commit 为
/// 第二行（无第二行时为空串）。
pub(crate) fn parse_version(content: &str) -> (String, String) {
    let mut lines = content.lines();
    let release = lines.next().unwrap_or("").to_string();
    let commit = lines.next().unwrap_or("").to_string();
    (release, commit)
}

/// 渲染版本信息面板（Release/Commit/Model 三行，左标签右值）
///
/// 每帧直接渲染，不缓存。对应原 C `minui.c:1539-1579` 的 surface 构建
/// 逻辑：三行内容，左列标签（`RGB_DARK_TEXT`）、右列值（`RGB_WHITE`），
/// 面板整体居中于屏幕。模型行数据来自 `Platform::DEVICE_MODEL`（C 的
/// `PLAT_getModel()`）。
///
/// # 参数
///
/// - `screen`:目标绘制缓冲区（主屏幕）
/// - `font`:已加载的字体（fontdue `Font`，支持任意字号）
/// - `release`:版本日期（version.txt 第一行）
/// - `commit`:commit hash（version.txt 第二行）
/// - `model`:设备型号（`Platform::DEVICE_MODEL`）
/// - `scale`:平台缩放倍率（`Platform::SCALE`）
///
/// # 布局（未缩放值，对应 C）
///
/// - 字号 `FONT_LARGE=16`（defines.h:66）；行高 `VERSION_LINE_HEIGHT=24`（minui.c:1558）
/// - 面板宽度 = 左列最大宽 + `SCALE1(8)` + 右列最大宽；高度 = `SCALE1(24*4)`（minui.c:1559-1561）
pub(crate) fn render_version(
    screen: &mut common::video::VideoBuffer,
    font: &Font,
    release: &str,
    commit: &str,
    model: &str,
    scale: u32,
) {
    const FONT_LARGE: u32 = 16; // C defines.h FONT_LARGE
    const VERSION_LINE_HEIGHT: u32 = 24; // C minui.c:1558
    let px = FONT_LARGE * scale;
    let line_height = VERSION_LINE_HEIGHT * scale;

    let labels = ["Release", "Commit", "Model"];
    let values = [release, commit, model];

    // 左右列最大宽度（C minui.c:1550-1556 的 l_width/r_width 比较）
    let l_width = labels
        .iter()
        .map(|s| size_text(font, s, px, 0).0)
        .max()
        .unwrap_or(0);
    let r_width = values
        .iter()
        .map(|s| size_text(font, s, px, 0).0)
        .max()
        .unwrap_or(0);

    let x = l_width + 8 * scale; // C minui.c:1559
    let w = x + r_width;
    let h = line_height * 4; // C minui.c:1561（VERSION_LINE_HEIGHT*4）

    // 面板居中（C minui.c:1579）
    let ox = screen.width.saturating_sub(w) / 2;
    let oy = screen.height.saturating_sub(h) / 2;

    for (i, (label, value)) in labels.iter().zip(values.iter()).enumerate() {
        let y = oy + (i as u32) * line_height;
        render_text(screen, font, label, px, RGB_DARK_TEXT, (ox, y));
        render_text(screen, font, value, px, RGB_WHITE, (ox + x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_version ───────────────────────────

    #[test]
    fn parse_version_multi_line() {
        // C 原版格式：release 行 + commit 行
        let (release, commit) = parse_version("2024-05-11\n84d1a2b3\n");
        assert_eq!(release, "2024-05-11");
        assert_eq!(commit, "84d1a2b3");
    }

    #[test]
    fn parse_version_single_line() {
        // 无第二行——C 版在此崩溃，Rust 版安全返回空 commit
        let (release, commit) = parse_version("2024-05-11");
        assert_eq!(release, "2024-05-11");
        assert_eq!(commit, "");
    }

    #[test]
    fn parse_version_empty() {
        let (release, commit) = parse_version("");
        assert_eq!(release, "");
        assert_eq!(commit, "");
    }

    #[test]
    fn parse_version_crlf() {
        // Windows 风格 CRLF 也应正确解析（lines() 自动处理 \r\n）
        let (release, commit) = parse_version("2024-05-11\r\n84d1a2b3\r\n");
        assert_eq!(release, "2024-05-11");
        assert_eq!(commit, "84d1a2b3");
    }
}
