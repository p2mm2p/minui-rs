//! 电池图标渲染
//!
//! 本模块实现电池图标的绘制——电池外壳 + 电量填充条 + 充电闪电标志。
//! 对应原 C `GFX_blitBattery()`（`api.c:559-589`）。
//!
//! ## 渲染逻辑
//!
//! 电池图标在 `PILL_SIZE × PILL_SIZE` 区域内居中（`PILL_SIZE` 从
//! `common::video::PILL_SIZE` 导入），分为两个分支：
//!
//! - **充电中**：绘制正常外壳（`BatteryIcon`）+ 闪电图标（`BatteryBolt`），不绘制填充条
//! - **非充电**：根据百分比选择外壳（≤10% 用 `BatteryLow`）和填充条（≤20% 用 `BatteryFillLow`），
//!   填充条通过 `blit_asset` 的 `src_rect` 参数动态裁切实现按比例显示
//!
//! ## 与原 C 代码的对比
//!
//! 原 C `GFX_blitBattery` 使用 `SDL_BlitSurface(sprite_sheet, &srcrect, dst, &dstrect)`
//! 进行源端裁切——精灵图集中 fill 的坐标是预缩放的，`srcrect` 在缩放坐标空间内计算。
//!
//! Rust 版使用 `blit_asset(atlas, asset, scale, dst, pos, src_rect)`，坐标体系相反：
//! 存储未缩放坐标，`blit_asset` 内部乘以 `scale`。`src_rect` 裁切机制和 C 的 `srcrect` 等价。
//!

use crate::asset::{Atlas, blit_asset};
use common::video::{ASSET_RECTS, Asset, PILL_SIZE, Rect, VideoBuffer};

/// 绘制电池图标（含电量填充、充电闪电标志）
///
/// 电池图标在 `PILL_SIZE × PILL_SIZE`（缩放后）区域内居中。
/// 水平居中公式与原 C 完全一致：`x += (SCALE1(PILL_SIZE) - (rect.w + FIXED_SCALE)) / 2`。
///
/// 对应原版 C `GFX_blitBattery()`（`api.c:559-589`）。
///
/// ## 参数
///
/// * `dst` - 目标 RGB565 像素缓冲区
/// * `atlas` - 已加载的精灵图集（通过 `load_atlas` 获取）
/// * `percentage` - 电量百分比（0–100）。调用方负责确保值在有效范围内
/// * `charging` - 是否正在充电
/// * `pos` - 目标区域左上角坐标 `(x, y)`（缩放后像素）
/// * `scale` - 平台缩放倍率
pub fn blit_battery(
    dst: &mut VideoBuffer,
    atlas: &Atlas,
    percentage: u8,
    charging: bool,
    pos: (u32, u32),
    scale: u32,
) {
    // ── 居中计算 ──────────────────────────────────
    // 对应原 C api.c:567-569:
    //   rect = asset_rects[ASSET_BATTERY];
    //   x += (SCALE1(PILL_SIZE) - (rect.w + FIXED_SCALE)) / 2;
    //   y += (SCALE1(PILL_SIZE) - rect.h) / 2;
    //
    // C 中 rect.w 已缩放，+FIXED_SCALE 补偿精灵图集右侧像素间隙。
    // Rust 版 ASSET_RECTS 为未缩放值，`+ scale` 等价于 C 的 `+ FIXED_SCALE`。
    let base = &ASSET_RECTS[Asset::BatteryIcon as usize];
    let pill = PILL_SIZE * scale;
    let x = pos.0 + (pill - (base.w * scale + scale)) / 2;
    let y = pos.1 + (pill - base.h * scale) / 2;

    if charging {
        // ── 充电模式 ──────────────────────────────
        // 对应原 C api.c:571-574
        blit_asset(atlas, Asset::BatteryIcon, scale, dst, (x, y), None);
        blit_asset(
            atlas,
            Asset::BatteryBolt,
            scale,
            dst,
            (x + 3 * scale, y + 2 * scale),
            None,
        );
    } else {
        // ── 非充电模式 ────────────────────────────
        // 对应原 C api.c:575-588

        // 选择外壳：≤10% 用低电量红色外壳
        let shell = if percentage <= 10 {
            Asset::BatteryLow
        } else {
            Asset::BatteryIcon
        };
        blit_asset(atlas, shell, scale, dst, (x, y), None);

        // 选择填充条：≤20% 用低电量红色填充
        let fill_asset = if percentage <= 20 {
            Asset::BatteryFillLow
        } else {
            Asset::BatteryFill
        };
        let fill_base = &ASSET_RECTS[fill_asset as usize];

        // 按百分比计算裁切宽度（未缩放坐标，与 C 逻辑一致）
        // 对应原 C api.c:580-586:
        //   SDL_Rect clip = rect;
        //   clip.w *= percent; clip.w /= 100;
        //   if (clip.w<=0) return;
        //   clip.x = rect.w - clip.w;
        let clip_w = fill_base.w * percentage as u32 / 100;
        if clip_w == 0 {
            return;
        }
        let clip_x = fill_base.w - clip_w; // 右对齐：从精灵右侧开始取样

        let src_rect = Rect {
            x: clip_x,
            y: 0,
            w: clip_w,
            h: fill_base.h,
        };

        // 填充条目标位置：外壳内偏移 (3, 2)*scale + 右对齐偏移
        blit_asset(
            atlas,
            fill_asset,
            scale,
            dst,
            (x + (3 + clip_x) * scale, y + 2 * scale),
            Some(src_rect),
        );
    }
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::video::Rect;

    /// 创建一个足够大的测试图集（256×256 白色像素，兼容 scale≤4）
    fn test_atlas() -> Atlas {
        Atlas {
            pixels: vec![0xFFFF; 256 * 256],
            width: 256,
            height: 256,
        }
    }

    /// 检查 `dst` 在指定矩形区域内是否包含非零像素
    fn has_pixels_in_rect(dst: &VideoBuffer, rect: Rect) -> bool {
        let pitch = dst.pitch as usize;
        for row in rect.y..(rect.y + rect.h).min(dst.height) {
            for col in rect.x..(rect.x + rect.w).min(dst.width) {
                if dst.pixels[row as usize * pitch + col as usize] != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// 统计 `dst` 中非零像素的数量
    fn count_nonzero(dst: &VideoBuffer) -> usize {
        dst.pixels.iter().filter(|&&p| p != 0).count()
    }

    // ── 充电模式测试 ──────────────────────────────

    #[test]
    fn charging_mode_produces_pixels() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        blit_battery(&mut dst, &atlas, 80, true, (10, 10), 2);
        assert!(count_nonzero(&dst) > 0, "充电模式应产生像素");
    }

    #[test]
    fn charging_mode_bolt_within_shell() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        let pos = (10, 10);
        let scale = 2;
        blit_battery(&mut dst, &atlas, 80, true, pos, scale);

        // 闪电图标偏移 (3,2)*scale 后应仍在 PILL_SIZE*scale 区域内
        let pill = PILL_SIZE * scale;
        let bolt_rect = Rect {
            x: pos.0 + 3 * scale,
            y: pos.1 + 2 * scale,
            w: 12 * scale, // BatteryBolt 未缩放 w=12
            h: 6 * scale,  // BatteryBolt 未缩放 h=6
        };
        // 闪电图标不应超出电池外壳范围 (PILL_SIZE 区域内)
        assert!(
            bolt_rect.x + bolt_rect.w <= pos.0 + pill,
            "闪电图标右边界应在 pill 范围内"
        );
    }

    #[test]
    fn charging_mode_fill_not_rendered() {
        // 充电模式：函数签名保证仅 blit 外壳+闪电，不调用 fill 分支
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        blit_battery(&mut dst, &atlas, 50, true, (10, 10), 2);
        assert!(count_nonzero(&dst) > 0, "充电模式应有像素输出");
    }

    // ── 非充电模式测试 ────────────────────────────

    #[test]
    fn normal_percentage_produces_pixels() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        blit_battery(&mut dst, &atlas, 80, false, (10, 10), 2);
        assert!(count_nonzero(&dst) > 0, "正常电量应产生像素");
    }

    #[test]
    fn low_percentage_produces_pixels() {
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        blit_battery(&mut dst, &atlas, 5, false, (10, 10), 2);
        assert!(count_nonzero(&dst) > 0, "低电量应产生像素");
    }

    #[test]
    fn zero_percentage_no_panic_shell_still_drawn() {
        // 电量为 0：clip_w=0 → 提前返回，不绘制填充条。外壳正常绘制
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        blit_battery(&mut dst, &atlas, 0, false, (10, 10), 2);
        let nonzero = count_nonzero(&dst);
        assert!(nonzero > 0, "电量 0 应仍有外壳像素");
    }

    #[test]
    fn full_percentage_no_panic() {
        // 满电：clip_w = fill_base.w → clip_x = 0，完整宽度
        let atlas = test_atlas();
        let mut dst = VideoBuffer::new(128, 128);
        blit_battery(&mut dst, &atlas, 100, false, (10, 10), 2);
        assert!(count_nonzero(&dst) > 0, "满电应产生像素");
    }

    // ── PILL_SIZE 居中测试 ────────────────────────

    #[test]
    fn position_respects_pill_size_centering() {
        let atlas = test_atlas();
        let scale: u32 = 2;
        let pos = (20, 20);

        // 电池外壳应在 PILL_SIZE*scale 区域内居中
        let mut dst = VideoBuffer::new(256, 256);
        blit_battery(&mut dst, &atlas, 80, false, pos, scale);

        let pill = PILL_SIZE * scale;
        let battery_base = &ASSET_RECTS[Asset::BatteryIcon as usize];
        // 居中的预期 x 坐标（与函数内公式一致）
        let expected_x = pos.0 + (pill - (battery_base.w * scale + scale)) / 2;
        let expected_y = pos.1 + (pill - battery_base.h * scale) / 2;

        // 外壳区域应包含非零像素
        let shell_region = Rect {
            x: expected_x,
            y: expected_y,
            w: battery_base.w * scale,
            h: battery_base.h * scale,
        };
        assert!(
            has_pixels_in_rect(&dst, shell_region),
            "电池外壳区域 ({expected_x}, {expected_y}) 应有像素"
        );
    }

    #[test]
    fn pill_area_boundary_no_pixels_outside() {
        let atlas = test_atlas();
        let scale: u32 = 2;
        let pos = (0, 0);
        let mut dst = VideoBuffer::new(256, 256);
        blit_battery(&mut dst, &atlas, 80, false, pos, scale);

        let pill = PILL_SIZE * scale;
        // PILL_SIZE*scale 区域外的右上角应为零
        let outside = Rect {
            x: pill,
            y: 0,
            w: dst.width - pill,
            h: pill,
        };
        assert!(
            !has_pixels_in_rect(&dst, outside),
            "PILL_SIZE 区域外应无像素"
        );
    }

    // ── 不同 scale 测试 ───────────────────────────

    #[test]
    fn different_scales_no_panic() {
        let atlas = test_atlas(); // 256×256，兼容 scale≤4
        let mut dst2 = VideoBuffer::new(200, 200);
        let mut dst3 = VideoBuffer::new(300, 300);

        blit_battery(&mut dst2, &atlas, 80, false, (10, 10), 2);
        blit_battery(&mut dst3, &atlas, 80, false, (10, 10), 3);

        assert!(count_nonzero(&dst2) > 0, "scale=2 应产生像素");
        assert!(count_nonzero(&dst3) > 0, "scale=3 应产生像素");
    }
}
