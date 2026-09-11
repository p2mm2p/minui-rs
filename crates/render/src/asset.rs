//! 精灵图集加载与裁切
//!
//! 本模块实现精灵图集（`assets@<N>x.png`）的 PNG 加载和像素级 blit 操作，
//! 是 render crate 中所有 `blit_*` 函数的基础。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `api.c` 中 `GFX_init()` 使用 `IMG_Load()` 加载图集，`GFX_blitAsset()`
//! 使用 `SDL_BlitSurface()` 复制像素。Rust 版使用 `png` crate 解码 +
//! 逐行 memcpy 实现等价功能，不依赖 SDL。
//!

use common::video::{ASSET_RECTS, Asset, Rect, VideoBuffer};
use std::fs::File;

/// 已加载的精灵图集
///
/// 存储 PNG 解码后的 RGB565 像素数据及图集尺寸。
/// 精灵图集原始画布为 100×64 像素（未缩放），实际加载的 PNG 可能已按
/// 平台 `SCALE` 缩放（如 `assets@2x.png` 为 200×128）。
pub struct Atlas {
    /// RGB565 像素数据
    pub pixels: Vec<u16>,
    /// 图集宽度（像素）
    pub width: u32,
    /// 图集高度（像素）
    pub height: u32,
}

/// 加载精灵图集 PNG 文件
///
/// 使用 `png` crate 解码 PNG，将 RGBA 像素转换为 RGB565 格式。
/// 对应原版 C `GFX_init()` 中的 `IMG_Load(asset_path)`。
///
/// ## 参数
///
/// - `path`：PNG 文件路径（如 `.system/res/assets@2x.png`）
///
/// ## 返回
///
/// - `Ok(Atlas)`：像素缓冲 + 宽高
/// - `Err(String)`：文件打开失败、PNG 解码失败等
///
/// ## 格式转换
///
/// RGBA → RGB565：R 取高 5 位，G 取高 6 位，B 取高 5 位。
/// Alpha 通道在转换时丢弃——原版 MinUI 的精灵图集像素已预渲染为最终颜色，
/// 不需要 alpha 混合（单色素材的着色由 `blit_pill` 等函数用 `fill_rect` 完成）。
pub fn load_atlas(path: &str) -> Result<Atlas, String> {
    let file = File::open(path).map_err(|e| format!("无法打开图集文件: {e}"))?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG 解码失败: {e}"))?;

    let (color_type, _bit_depth) = reader.output_color_type();
    let info = reader.info();
    let width = info.width;
    let height = info.height;
    let pixel_count = (width * height) as usize;
    let mut pixels: Vec<u16> = Vec::with_capacity(pixel_count);

    let bytes_per_pixel = match color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => {
            return Err(format!("不支持的 PNG 颜色格式: {color_type:?}"));
        }
    };

    // 逐行解码 → RGB565
    while let Some(row) = reader.next_row().map_err(|e| format!("解码错误: {e}"))? {
        let row_data: &[u8] = row.data();
        match color_type {
            png::ColorType::Rgba => {
                for chunk in row_data.chunks_exact(bytes_per_pixel) {
                    let r = chunk[0] as u16;
                    let g = chunk[1] as u16;
                    let b = chunk[2] as u16;
                    let rgb565 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                    pixels.push(rgb565);
                }
            }
            png::ColorType::Rgb => {
                for chunk in row_data.chunks_exact(bytes_per_pixel) {
                    let r = chunk[0] as u16;
                    let g = chunk[1] as u16;
                    let b = chunk[2] as u16;
                    let rgb565 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                    pixels.push(rgb565);
                }
            }
            png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => {
                for &gray in row_data.iter().step_by(bytes_per_pixel as usize) {
                    let g = gray as u16;
                    let rgb565 = ((g >> 3) << 11) | ((g >> 2) << 5) | (g >> 3);
                    pixels.push(rgb565);
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(Atlas {
        pixels,
        width,
        height,
    })
}

/// 从精灵图集中裁切并复制像素到目标缓冲区
///
/// 对应原版 C `GFX_blitAsset()`。
///
/// ## 参数
///
/// - `atlas`：已加载的精灵图集
/// - `asset`：素材标识（从 `ASSET_RECTS[asset as usize]` 获取未缩放源矩形）
/// - `scale`：平台缩放倍率，源矩形的 x/y/w/h 均乘以此值
/// - `dst`：目标像素缓冲
/// - `dst_pos`：目标位置（左上角坐标）
/// - `src_rect`：可选裁切矩形（相对于 Asset 源矩形左上角）。`None` 表示使用整个源矩形
///
/// ## 注意
///
/// 调用方负责确保目标区域不超出 `dst` 边界。本函数不做边界裁剪。
pub fn blit_asset(
    atlas: &Atlas,
    asset: Asset,
    scale: u32,
    dst: &mut VideoBuffer,
    dst_pos: (u32, u32),
    src_rect: Option<Rect>,
) {
    let base = &ASSET_RECTS[asset as usize];

    // 如果提供了 src_rect，使用它相对于 base 的偏移裁切
    let (sx_off, sy_off, sw, sh) = if let Some(clip) = src_rect {
        (clip.x, clip.y, clip.w, clip.h)
    } else {
        (0, 0, base.w, base.h)
    };

    // 按 scale 缩放
    let sx = (base.x + sx_off) * scale;
    let sy = (base.y + sy_off) * scale;
    let sw = sw * scale;
    let sh = sh * scale;

    let atlas_w = atlas.width as usize;
    let dst_pitch = dst.pitch as usize;

    for row in 0..sh as usize {
        let src_offset = (sy as usize + row) * atlas_w + sx as usize;
        let dst_offset = (dst_pos.1 as usize + row) * dst_pitch + dst_pos.0 as usize;
        let len = sw as usize;
        dst.pixels[dst_offset..dst_offset + len]
            .copy_from_slice(&atlas.pixels[src_offset..src_offset + len]);
    }
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use common::video::Rect;

    /// 在临时文件中创建一个最小 RGB PNG 用于测试
    fn create_test_png(data: &[u8], width: u32, height: u32) -> String {
        let path = std::env::temp_dir().join("test_atlas.png");
        let file = File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn load_atlas_loads_valid_png() {
        // 2×2 RGB PNG: red, green, blue, white
        let data: Vec<u8> = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let path = create_test_png(&data, 2, 2);
        let atlas = load_atlas(&path).unwrap();

        assert_eq!(atlas.width, 2);
        assert_eq!(atlas.height, 2);
        assert_eq!(atlas.pixels.len(), 4);

        // Red:   R=255→31(5bit), G=0→0(6bit), B=0→0(5bit)
        assert_eq!(atlas.pixels[0], 0xF800);
        // Green: R=0→0,       G=255→63(6bit), B=0→0
        assert_eq!(atlas.pixels[1], 0x07E0);
        // Blue:  R=0→0,       G=0→0,          B=255→31(5bit)
        assert_eq!(atlas.pixels[2], 0x001F);
        // White: all max
        assert_eq!(atlas.pixels[3], 0xFFFF);
    }

    #[test]
    fn load_atlas_returns_err_for_nonexistent() {
        let result = load_atlas("/tmp/nonexistent_xxxx.png");
        assert!(result.is_err());
    }

    // ── blit_asset 测试 ──────────────────────────

    fn make_test_atlas() -> Atlas {
        // WhitePill rect is (1, 1, 30, 30) — needs atlas at least 31×31
        // Create a 32×32 atlas filled with white pixels
        let width: u32 = 32;
        let height: u32 = 32;
        Atlas {
            pixels: vec![0xFFFF; (width * height) as usize],
            width,
            height,
        }
    }

    #[test]
    fn blit_asset_full_sprite() {
        let atlas = make_test_atlas();
        let mut dst = VideoBuffer::new(32, 32);

        blit_asset(&atlas, Asset::WhitePill, 1, &mut dst, (0, 0), None);
        // All copied pixels should be white (0xFFFF from atlas)
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_asset_with_src_rect_clip() {
        let atlas = make_test_atlas();
        let mut dst = VideoBuffer::new(32, 32);

        blit_asset(
            &atlas,
            Asset::WhitePill,
            1,
            &mut dst,
            (0, 0),
            Some(Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            }),
        );
        assert!(dst.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn blit_asset_different_scales() {
        // Use a large atlas so both scale=2 and scale=3 fit
        // WhitePill at scale=3: (3, 3, 90, 90), needs 93×93 atlas
        let atlas = Atlas {
            pixels: vec![0xFFFF; (100 * 100) as usize],
            width: 100,
            height: 100,
        };
        let mut dst1 = VideoBuffer::new(60, 60);
        let mut dst2 = VideoBuffer::new(90, 90);

        blit_asset(&atlas, Asset::WhitePill, 2, &mut dst1, (0, 0), None);
        blit_asset(&atlas, Asset::WhitePill, 3, &mut dst2, (0, 0), None);
        // Both should complete without panic; scale=3 copies more pixels
        assert!(dst1.pixels.iter().any(|&p| p != 0));
        assert!(dst2.pixels.iter().any(|&p| p != 0));
    }
}
