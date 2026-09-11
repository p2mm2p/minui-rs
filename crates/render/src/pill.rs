//! 圆角药丸和圆角矩形渲染
//!
//! 本模块实现 MinUI 中最基础的 UI 绘制原语——圆角药丸（Pill）和圆角矩形。
//! 所有容器控件（菜单高亮条、按钮背景、状态栏、卡片面板）都依赖这两个函数。
//!
//! ## 实现策略
//!
//! 圆角部分从精灵图集 blit（保留精灵原本的抗锯齿边缘），平坦区域用
//! `VideoBuffer::fill_rect` 纯色填充（颜色来自 `ASSET_RGBS`）。这种
//! "精灵 + 填充" 的组合方式与原 C 代码完全一致。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.c` 中：
//! - `GFX_blitPill()`（第 520–539 行）：左/右半圆帽 `SDL_BlitSurface` + 中间 `SDL_FillRect`
//! - `GFX_blitRect()`（第 540–558 行）：四个四分之一角 blit + 三条水平填充
//!
//! Rust 版使用 `blit_asset` 替代 `SDL_BlitSurface`，使用 `fill_rect` 替代
//! `SDL_FillRect`。坐标体系从 C 的预缩放改为运行时缩放（方案 A）。
//!

use crate::asset::{Atlas, blit_asset};
use common::video::{ASSET_RECTS, ASSET_RGBS, Asset, Rect, VideoBuffer};

/// 绘制圆角药丸
///
/// 药丸由三段组成：左半圆帽（blit 精灵左半）、中间纯色填充、右半圆帽
/// （blit 精灵右半）。精灵提供抗锯齿的圆角形状，填充部分覆盖两个半圆帽
/// 之间的矩形区域。
///
/// 对应原 C `GFX_blitPill()`（`api.c:520-539`）。
///
/// ## 参数
///
/// * `atlas` - 已加载的精灵图集（通过 `load_atlas` 获取）
/// * `asset` - 素材标识。仅单色素材（`WhitePill`/`BlackPill`/`DarkGrayPill`），
///   填充色从 `ASSET_RGBS[asset]` 获取
/// * `scale` - 平台缩放倍率（透传给 `blit_asset`）
/// * `dst` - 目标像素缓冲区
/// * `dst_rect` - 目标矩形（缩放后的像素坐标）。若 `h == 0`，默认使用
///   `ASSET_RECTS[asset].h * scale` 作为高度
///
/// ## 注意
///
/// 不做边界裁剪——调用方需确保 `dst_rect` 完全位于 `dst` 范围内。
pub fn blit_pill(atlas: &Atlas, asset: Asset, scale: u32, dst: &mut VideoBuffer, dst_rect: Rect) {
    let base = &ASSET_RECTS[asset as usize];

    // 默认高度：精灵图集高度 × scale
    let h = if dst_rect.h == 0 {
        base.h * scale
    } else {
        dst_rect.h
    };
    let mut x = dst_rect.x;
    let y = dst_rect.y;

    // 药丸宽度至少等于高度，减去两端半圆帽
    let mut w = dst_rect.w;
    if w < h {
        w = h;
    }
    let r = base.h / 2; // 未缩放半径（精灵高度的一半）

    // 左半圆帽
    let left_cap = Rect {
        x: 0,
        y: 0,
        w: r,
        h: base.h,
    };
    blit_asset(atlas, asset, scale, dst, (x, y), Some(left_cap));
    x += r * scale;

    // 中间纯色填充
    let center_w = w - 2 * r * scale;
    if center_w > 0 {
        let center = Rect {
            x,
            y,
            w: center_w,
            h,
        };
        dst.fill_rect(center, ASSET_RGBS[asset as usize]);
        x += center_w;
    }

    // 右半圆帽
    let right_cap = Rect {
        x: r,
        y: 0,
        w: r,
        h: base.h,
    };
    blit_asset(atlas, asset, scale, dst, (x, y), Some(right_cap));
}

/// 绘制圆角矩形
///
/// 矩形由四个四分之一圆角（blit 精灵四角）和三条水平纯色填充条组成。
/// 中间填充条覆盖全宽（包括左右边区域），上/下填充条覆盖两角之间的
/// 水平区域。
///
/// 对应原 C `GFX_blitRect()`（`api.c:540-558`）。
///
/// ## 参数
///
/// * `atlas` - 已加载的精灵图集
/// * `asset` - 素材标识。单色和彩色素材均可使用（如果是彩色素材，角
///   部分会保留精灵像素颜色，填充部分使用 `ASSET_RGBS` 颜色）
/// * `scale` - 平台缩放倍率
/// * `dst` - 目标像素缓冲区
/// * `dst_rect` - 目标矩形（缩放后的像素坐标）
///
/// ## 注意
///
/// 不做边界裁剪——调用方需确保 `dst_rect` 完全位于 `dst` 范围内。
pub fn blit_rect(atlas: &Atlas, asset: Asset, scale: u32, dst: &mut VideoBuffer, dst_rect: Rect) {
    let base = &ASSET_RECTS[asset as usize];

    let d = base.w; // 未缩放精灵宽度
    let r = d / 2; // 未缩放圆角半径
    let rs = r * scale; // 缩放后的圆角半径
    let ds = d * scale; // 缩放后的精灵宽度

    let x = dst_rect.x;
    let y = dst_rect.y;
    let w = dst_rect.w;
    let h = dst_rect.h;
    let color = ASSET_RGBS[asset as usize];

    // 左上角 (TL)
    blit_asset(
        atlas,
        asset,
        scale,
        dst,
        (x, y),
        Some(Rect {
            x: 0,
            y: 0,
            w: r,
            h: r,
        }),
    );
    // 上边填充
    dst.fill_rect(
        Rect {
            x: x + rs,
            y,
            w: w - ds,
            h: rs,
        },
        color,
    );
    // 右上角 (TR)
    blit_asset(
        atlas,
        asset,
        scale,
        dst,
        (x + w - rs, y),
        Some(Rect {
            x: r,
            y: 0,
            w: r,
            h: r,
        }),
    );
    // 中间全宽填充
    dst.fill_rect(
        Rect {
            x,
            y: y + rs,
            w,
            h: h - ds,
        },
        color,
    );
    // 左下角 (BL)
    blit_asset(
        atlas,
        asset,
        scale,
        dst,
        (x, y + h - rs),
        Some(Rect {
            x: 0,
            y: r,
            w: r,
            h: r,
        }),
    );
    // 下边填充
    dst.fill_rect(
        Rect {
            x: x + rs,
            y: y + h - rs,
            w: w - ds,
            h: rs,
        },
        color,
    );
    // 右下角 (BR)
    blit_asset(
        atlas,
        asset,
        scale,
        dst,
        (x + w - rs, y + h - rs),
        Some(Rect {
            x: r,
            y: r,
            w: r,
            h: r,
        }),
    );
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::video::{Rect, VideoBuffer};

    /// 创建一个足够大的测试图集
    fn make_test_atlas(width: u32, height: u32) -> Atlas {
        Atlas {
            pixels: vec![0xFFFF; (width * height) as usize],
            width,
            height,
        }
    }

    fn make_big_atlas() -> Atlas {
        make_test_atlas(128, 128)
    }

    // ── blit_pill 测试 ────────────────────────────

    #[test]
    fn blit_pill_standard_width() {
        let atlas = make_big_atlas();
        let mut dst = VideoBuffer::new(200, 100);
        let rect = Rect {
            x: 10,
            y: 10,
            w: 120,
            h: 60,
        };
        blit_pill(&atlas, Asset::WhitePill, 2, &mut dst, rect);

        // 中间填充区域应为白色
        let pitch = dst.pitch as usize;
        // 中心区域：行 40（y=10，中间为 y+30=40），列 70（x=10+15*2=40 之后，在中心填充中）
        let mid_pixel = 40 * pitch + 70;
        assert_eq!(dst.pixels[mid_pixel], 0xFFFF);
    }

    #[test]
    fn blit_pill_default_height_when_zero() {
        let atlas = make_big_atlas();
        let mut dst = VideoBuffer::new(200, 100);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 0,
        };
        // h=0 时应使用默认高度：ASSET_RECTS[WhitePill].h * scale = 30 * 2 = 60
        blit_pill(&atlas, Asset::DarkGrayPill, 2, &mut dst, rect);

        // 应该绘制了内容
        let pitch = dst.pitch as usize;
        let mid_offset = 30 * pitch + 50;
        assert_ne!(dst.pixels[mid_offset], 0x0000);
    }

    #[test]
    fn blit_pill_narrow_expands_to_min_width() {
        let atlas = make_big_atlas();
        let mut dst = VideoBuffer::new(100, 100);
        // w=10 < h=30（WhitePill 未缩放高度），应自动扩展到 30
        // r=15，两个半圆帽各 15×30，中心宽度=30-30=0，无中心填充
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 30,
        };
        blit_pill(&atlas, Asset::WhitePill, 1, &mut dst, rect);

        // 左帽内部应有像素
        let pitch = dst.pitch as usize;
        let left_cap = 15 * pitch + 5; // 行 15（中间），列 5（左帽内）
        assert_ne!(dst.pixels[left_cap], 0x0000);
        // 右帽内部应有像素
        let right_cap = 15 * pitch + 20; // 行 15，列 20（右帽内，距 x=15 偏移 5）
        assert_ne!(dst.pixels[right_cap], 0x0000);
    }

    #[test]
    fn blit_pill_different_scales() {
        let atlas = make_big_atlas();
        let mut dst2 = VideoBuffer::new(200, 100);
        let mut dst3 = VideoBuffer::new(300, 150);

        let rect = Rect {
            x: 0,
            y: 0,
            w: 120,
            h: 60,
        };
        blit_pill(&atlas, Asset::WhitePill, 2, &mut dst2, rect);
        blit_pill(&atlas, Asset::WhitePill, 3, &mut dst3, rect);

        // 两者都应无 panic 完成；scale=3 使用了更多像素
        let non_zero2 = dst2.pixels.iter().filter(|&&p| p != 0).count();
        let non_zero3 = dst3.pixels.iter().filter(|&&p| p != 0).count();
        assert!(non_zero3 > non_zero2);
    }

    // ── blit_rect 测试 ────────────────────────────

    #[test]
    fn blit_rect_full_coverage_no_holes() {
        let atlas = make_big_atlas();
        let mut dst = VideoBuffer::new(200, 200);
        // Option asset 是 20×20 未缩放，scale=2 → ds=40
        // w=80, h=60，保证 w >= ds, h >= ds
        let rect = Rect {
            x: 5,
            y: 5,
            w: 80,
            h: 60,
        };
        blit_rect(&atlas, Asset::Option, 2, &mut dst, rect);

        // rect 内的每个像素都应非零（无空洞）
        let pitch = dst.pitch as usize;
        for row in rect.y..rect.y + rect.h {
            for col in rect.x..rect.x + rect.w {
                let idx = row as usize * pitch + col as usize;
                assert_ne!(
                    dst.pixels[idx], 0x0000,
                    "像素 ({col}, {row}) 应为非零（空洞！）"
                );
            }
        }
    }

    #[test]
    fn blit_rect_different_assets_different_colors() {
        let atlas = make_big_atlas();
        // WhitePill (30×30 未缩放) at scale=2: ds=60，需要使用 w >= 60
        let rect = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 60,
        };

        let mut dst_white = VideoBuffer::new(128, 128);
        blit_rect(&atlas, Asset::WhitePill, 2, &mut dst_white, rect);
        let mut dst_black = VideoBuffer::new(128, 128);
        blit_rect(&atlas, Asset::BlackPill, 2, &mut dst_black, rect);

        // 中间填充区域的颜色应不同
        let pitch = dst_white.pitch as usize;
        let mid_offset = 30 * pitch + 40; // 行 30（中心），列 40（填充区域内）
        assert_eq!(dst_white.pixels[mid_offset], 0xFFFF);
        assert_eq!(dst_black.pixels[mid_offset], 0x0000);
    }

    #[test]
    fn blit_rect_small_rect() {
        let atlas = make_big_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        // Button asset 是 20×20 未缩放，scale=2: ds=40, rs=20
        // w=ds 刚好让上下边填充宽度为 0（合法：fill_rect 空时无操作）
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 40,
        };
        blit_rect(&atlas, Asset::Button, 2, &mut dst, rect);
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }
}
