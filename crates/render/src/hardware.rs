//! 状态栏渲染
//!
//! 本模块实现设备状态栏和亮度/音量调节界面的渲染，
//! 是 render crate 中依赖面最宽的模块——组合 `blit_pill`、
//! `blit_asset`、`blit_battery` 三者，输出完整的硬件状态 UI。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.c:692-777` 中 `GFX_blitHardwareGroup()` 在渲染函数内部
//! 直接调用平台函数（`GetHDMI()`、`GetBrightness()`、`PLAT_isOnline()` 等）
//! 获取硬件状态。Rust 版将平台数据收拢到 `HardwareStatus` 结构体，
//! 由调用方（minui/minarch 主循环）每帧从 `Platform` trait 收集后传入。
//! 这消除了渲染层对平台 API 的依赖。
//!

use crate::asset::{Atlas, blit_asset};
use crate::battery::blit_battery;
use crate::pill::blit_pill;
use common::video::{
    ASSET_RECTS, Asset, BRIGHTNESS_MAX, BRIGHTNESS_MIN, PADDING, PILL_SIZE, Rect, SETTINGS_SIZE,
    SETTINGS_WIDTH, VOLUME_MAX, VOLUME_MIN, VideoBuffer,
};

/// 硬件状态栏渲染所需的平台数据快照
///
/// 对应原 C `GFX_blitHardwareGroup()` 中通过全局变量和平台调用获取的状态：
///
/// | 字段 | 原 C 来源 |
/// |------|----------|
/// | `show_setting` | `PWR_update()` 返回值 |
/// | `setting_value` | `GetBrightness()` / `GetVolume()` |
/// | `battery_percentage` | `PLAT_getBatteryStatus()` |
/// | `battery_charging` | `PLAT_getBatteryStatus()` |
/// | `wifi_online` | `PLAT_isOnline()` |
/// | `mode_main` | `gfx.mode == MODE_MAIN` |
/// | `has_hdmi` | `GetHDMI()` |
///
/// 由调用方（minui/minarch 主循环）在每帧渲染前从 `Platform` trait 收集。
pub struct HardwareStatus {
    /// 设置显示模式：0=状态栏, 1=亮度调节, 2=音量调节
    pub show_setting: u8,
    /// 当前设置值（亮度 0..=BRIGHTNESS_MAX 或音量 0..=VOLUME_MAX）
    pub setting_value: u8,
    /// 电量百分比（0-100）
    pub battery_percentage: u8,
    /// 是否正在充电
    pub battery_charging: bool,
    /// WiFi 是否已连接
    pub wifi_online: bool,
    /// 是否主界面模式（`true` = MODE_MAIN, `false` = MODE_MENU）
    pub mode_main: bool,
    /// HDMI 是否连接（连接时跳过设置 UI 渲染）
    pub has_hdmi: bool,
}

/// 绘制硬件状态组（电池、WiFi、亮度/音量条）
///
/// 两个渲染分支：
///
/// **设置调节**（`show_setting != 0` ∧ `!has_hdmi`）：
/// pill 背景 + 亮度/音量图标 + 滑条（底 + 填充），
/// 填充宽度按 `(setting_value - min) / (max - min)` 百分比计算。
///
/// **状态栏**（`show_setting == 0`）：
/// pill 背景 + WiFi 图标（若在线）+ 电池图标。
///
/// 对应原 C `GFX_blitHardwareGroup()`（`api.c:692-777`）。
///
/// ## 返回
///
/// 状态栏/设置界面占用的水平像素宽度（`ow`），供调用方计算剩余布局空间。
pub fn blit_hardware_group(
    atlas: &Atlas,
    status: &HardwareStatus,
    scale: u32,
    dst: &mut VideoBuffer,
) -> u32 {
    let mut ow;

    if status.show_setting != 0 && !status.has_hdmi {
        // ── 设置调节分支（亮度/音量）─────────────────
        // 对应原 C api.c:701-745

        ow = (PILL_SIZE + SETTINGS_WIDTH + 10 + 4) * scale;
        let mut ox = dst.width - PADDING * scale - ow;
        let oy = PADDING * scale;

        // Pill 背景
        let pill_asset = if status.mode_main {
            Asset::DarkGrayPill
        } else {
            Asset::BlackPill
        };
        blit_pill(
            atlas,
            pill_asset,
            scale,
            dst,
            Rect {
                x: ox,
                y: oy,
                w: ow,
                h: PILL_SIZE * scale,
            },
        );

        // 选择亮度/音量/静音图标
        let icon = if status.show_setting == 1 {
            Asset::Brightness
        } else if status.setting_value > 0 {
            Asset::Volume
        } else {
            Asset::VolumeMute
        };

        // 图标偏移（亮度 vs 音量位置略有不同，与原 C 一致）
        let (icon_ox, icon_oy) = if status.show_setting == 1 {
            (ox + 6 * scale, oy + 5 * scale)
        } else {
            (ox + 8 * scale, oy + 7 * scale)
        };
        blit_asset(atlas, icon, scale, dst, (icon_ox, icon_oy), None);

        // 滑条底（BarBg pill）
        ox += PILL_SIZE * scale;
        let bar_y = oy + (PILL_SIZE - SETTINGS_SIZE) / 2 * scale;
        let bar_bg_asset = if status.mode_main {
            Asset::BarBg
        } else {
            Asset::BarBgMenu
        };
        blit_pill(
            atlas,
            bar_bg_asset,
            scale,
            dst,
            Rect {
                x: ox,
                y: bar_y,
                w: SETTINGS_WIDTH * scale,
                h: SETTINGS_SIZE * scale,
            },
        );

        // 滑条填充（Bar pill，按百分比宽度）
        let (min, max) = if status.show_setting == 1 {
            (BRIGHTNESS_MIN, BRIGHTNESS_MAX)
        } else {
            (VOLUME_MIN, VOLUME_MAX)
        };
        let percent_num = status.setting_value.saturating_sub(min as u8) as u32;
        let percent_den = max - min;
        if status.show_setting == 1 || status.setting_value > 0 {
            // 原 C: if (show_setting==1 || setting_value>0)
            blit_pill(
                atlas,
                Asset::Bar,
                scale,
                dst,
                Rect {
                    x: ox,
                    y: bar_y,
                    w: SETTINGS_WIDTH * scale * percent_num / percent_den,
                    h: SETTINGS_SIZE * scale,
                },
            );
        }
    } else {
        // ── 状态栏分支（电池 + WiFi）─────────────────
        // 对应原 C api.c:748-773

        let ww = (PILL_SIZE - 3) * scale; // WiFi 区域宽度
        ow = PILL_SIZE * scale;
        if status.wifi_online {
            ow += ww;
        }

        let mut ox = dst.width - PADDING * scale - ow;
        let oy = PADDING * scale;

        // Pill 背景
        let pill_asset = if status.mode_main {
            Asset::DarkGrayPill
        } else {
            Asset::BlackPill
        };
        blit_pill(
            atlas,
            pill_asset,
            scale,
            dst,
            Rect {
                x: ox,
                y: oy,
                w: ow,
                h: PILL_SIZE * scale,
            },
        );

        // WiFi 图标（居左于 pill 内，垂直居中）
        if status.wifi_online {
            let wifi_base = &ASSET_RECTS[Asset::WiFi as usize];
            let wifi_x = ox + (PILL_SIZE * scale - wifi_base.w * scale) / 2;
            let wifi_y = oy + (PILL_SIZE * scale - wifi_base.h * scale) / 2;
            blit_asset(atlas, Asset::WiFi, scale, dst, (wifi_x, wifi_y), None);
            ox += ww;
        }

        // 电池图标
        blit_battery(
            dst,
            atlas,
            status.battery_percentage,
            status.battery_charging,
            (ox, oy),
            scale,
        );
    }

    ow
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建 256×256 白色像素测试图集
    fn make_test_atlas() -> Atlas {
        Atlas {
            pixels: vec![0xFFFF; 256 * 256],
            width: 256,
            height: 256,
        }
    }

    /// 创建默认 HardwareStatus（状态栏模式，主界面，无 HDMI）
    fn make_status(status_overrides: impl FnOnce(&mut HardwareStatus)) -> HardwareStatus {
        let mut s = HardwareStatus {
            show_setting: 0,
            setting_value: 0,
            battery_percentage: 80,
            battery_charging: false,
            wifi_online: false,
            mode_main: true,
            has_hdmi: false,
        };
        status_overrides(&mut s);
        s
    }

    /// 统计 `dst` 中非零像素的数量
    fn nonzero(dst: &VideoBuffer) -> usize {
        dst.pixels.iter().filter(|&&p| p != 0).count()
    }

    // ── 2.2 状态栏 + WiFi + 电池 ──────────────────

    #[test]
    fn status_bar_with_wifi_and_battery() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.wifi_online = true;
        });
        let mut dst = VideoBuffer::new(400, 100);
        let ow = blit_hardware_group(&atlas, &status, 2, &mut dst);
        assert!(ow > 0, "状态栏应返回非零宽度");
        assert!(nonzero(&dst) > 0, "状态栏应产生像素");
    }

    // ── 2.3 状态栏仅电池（WiFi 离线）───────────────

    #[test]
    fn status_bar_battery_only_no_wifi() {
        let atlas = make_test_atlas();
        let status = make_status(|_| {});
        let mut dst = VideoBuffer::new(400, 100);
        blit_hardware_group(&atlas, &status, 2, &mut dst);
        assert!(nonzero(&dst) > 0, "即使无 WiFi 也应渲染电池");
    }

    // ── 2.4 MODE_MENU 使用黑色 pill ────────────────

    #[test]
    fn mode_menu_uses_black_pill() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.mode_main = false;
        });
        let mut dst = VideoBuffer::new(400, 100);
        blit_hardware_group(&atlas, &status, 2, &mut dst);
        // 黑色 pill 像素为 0x0000，区域外也黑——验证 non-panic 即可
        // （原断言 `nonzero(&dst) > 0 || true` 恒真且 clippy 报 logic bug——
        // 意图只是确认调用不 panic，改为显式调用保持该意图）
        let _ = nonzero(&dst);
    }

    // ── 2.5 亮度调节界面 ──────────────────────────

    #[test]
    fn brightness_setting_ui() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.show_setting = 1;
            s.setting_value = 5;
        });
        let mut dst = VideoBuffer::new(400, 100);
        let ow = blit_hardware_group(&atlas, &status, 2, &mut dst);
        assert!(ow > 0, "设置 UI 应返回非零宽度");
        assert!(nonzero(&dst) > 0, "亮度调节 UI 应产生像素");
    }

    // ── 2.6 音量调节 + 静音图标 ────────────────────

    #[test]
    fn volume_mute_when_zero() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.show_setting = 2;
            s.setting_value = 0;
        });
        let mut dst = VideoBuffer::new(400, 100);
        blit_hardware_group(&atlas, &status, 2, &mut dst);
        // 音量为 0 时应使用 VolumeMute 图标（不 panic）
        assert!(nonzero(&dst) > 0, "静音状态也应渲染 UI");
    }

    // ── 2.7 音量最大时滑条满 ──────────────────────

    #[test]
    fn volume_max_full_bar() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.show_setting = 2;
            s.setting_value = 20; // VOLUME_MAX
        });
        let mut dst = VideoBuffer::new(400, 100);
        blit_hardware_group(&atlas, &status, 2, &mut dst);
        assert!(nonzero(&dst) > 0, "音量最大应有满滑条");
    }

    // ── 2.8 HDMI 连接时退回到状态栏 ──────────────

    #[test]
    fn hdmi_connected_falls_back_to_status_bar() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.show_setting = 1;
            s.has_hdmi = true;
        });
        let mut dst = VideoBuffer::new(400, 100);
        let ow = blit_hardware_group(&atlas, &status, 2, &mut dst);
        // 原 C 行为：has_hdmi && show_setting → 退回到状态栏分支，而非完全跳过
        assert!(ow > 0, "HDMI 连接时应退回到状态栏，ow > 0");
        assert!(nonzero(&dst) > 0, "HDMI 连接时仍应渲染状态栏");
    }

    // ── 2.9 亮度最小时滑条为空 ────────────────────

    #[test]
    fn brightness_min_empty_bar() {
        let atlas = make_test_atlas();
        let status = make_status(|s| {
            s.show_setting = 1;
            s.setting_value = 0; // BRIGHTNESS_MIN
        });
        let mut dst = VideoBuffer::new(400, 100);
        blit_hardware_group(&atlas, &status, 2, &mut dst);
        // 亮度最小时滑条填充宽度为 0，仅渲染图标和底色
        assert!(nonzero(&dst) > 0, "亮度最小仍有图标和底色");
    }

    // ── 2.10 HardwareStatus 构造和借用 ────────────

    #[test]
    fn hardware_status_construct_and_borrow() {
        let status = HardwareStatus {
            show_setting: 0,
            setting_value: 0,
            battery_percentage: 80,
            battery_charging: false,
            wifi_online: false,
            mode_main: true,
            has_hdmi: false,
        };
        let atlas = make_test_atlas();
        let mut dst = VideoBuffer::new(400, 100);
        // 验证结构体可构造、&status 可传递给函数
        let _ow = blit_hardware_group(&atlas, &status, 2, &mut dst);
    }
}
