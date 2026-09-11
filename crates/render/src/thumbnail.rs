//! 缩略图加载与解码
//!
//! 本模块实现 ROM 缩略图的 PNG 加载和 RGB565 转换，
//! 是 render crate 中唯一仅依赖 `png` crate 的模块。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `minui.c:1513` 使用 `IMG_Load(res_path)` (SDL_image) 加载缩略图，
//! 随后 `SDL_BlitSurface` 到 screen surface 并 `SDL_FreeSurface`。
//! Rust 版使用 `png` crate 解码 + `VideoBuffer` 返回，由调用方决定
//! blit 目标位置，消除了对 SDL 的依赖。
//!

use common::video::VideoBuffer;
use std::fs::File;

/// 加载 PNG 缩略图并转换为 RGB565 像素缓冲
///
/// MinUI 支持为 ROM 文件提供缩略图：`{rom_dir}/.res/{rom_file}.png`。
/// 对应原 C `IMG_Load(res_path)` + `SDL_BlitSurface()`（`minui.c:1513-1516`）。
///
/// ## 差异
///
/// 原 C 使用 SDL_image 加载后立即 blit 到 screen surface；
/// Rust 版返回独立 `VideoBuffer`，由调用方决定 blit 目标位置。
///
/// ## 参数
///
/// * `path` - PNG 文件路径（如 `.res/MyGame.gb.png`）
///
/// ## 返回
///
/// * `Some(VideoBuffer)` - PNG 解码成功（RGBA→RGB565 转换，pitch = width）
/// * `None` - 文件不存在、PNG 格式错误、或不支持的色彩类型
pub fn load_thumbnail(path: &str) -> Option<VideoBuffer> {
    let file = File::open(path).ok()?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().ok()?;

    let info = reader.info();
    let width = info.width;
    let height = info.height;
    let pixel_count = (width * height) as usize;
    let mut pixels: Vec<u16> = Vec::with_capacity(pixel_count);

    let color_type = reader.output_color_type().0;
    let bytes_per_pixel = match color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return None,
    };

    // 逐行解码 → RGB565
    // color_type match 的结构与 load_atlas 相同但独立实现——
    // 两者返回类型不同（Atlas vs VideoBuffer），提取共享函数不会减少总代码量
    while let Some(row) = reader.next_row().ok()? {
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
                for &gray in row_data.iter().step_by(bytes_per_pixel) {
                    let g = gray as u16;
                    let rgb565 = ((g >> 3) << 11) | ((g >> 2) << 5) | (g >> 3);
                    pixels.push(rgb565);
                }
            }
            _ => unreachable!(),
        }
    }

    Some(VideoBuffer {
        pixels,
        width,
        height,
        pitch: width,
    })
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use png::{BitDepth, ColorType, Encoder};
    use std::fs::File;
    use std::io::Write;

    /// 测试辅助函数：在临时目录创建指定色彩类型的 PNG 文件，返回文件路径
    ///
    /// `data` 是原始像素字节（大小 = width × height × bytes_per_pixel）。
    /// 各 `color_type` 的每像素字节数：Rgb=3, Rgba=4, Grayscale=1, GrayscaleAlpha=2。
    fn create_test_png(data: &[u8], width: u32, height: u32, color_type: ColorType) -> String {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("test_thumb_{id}.png"));
        let file = File::create(&path).unwrap();
        let mut encoder = Encoder::new(file, width, height);
        encoder.set_color(color_type);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        path.to_str().unwrap().to_string()
    }

    // ── 1.2 加载有效 RGB PNG ──────────────────────

    #[test]
    fn load_rgb_png_returns_video_buffer() {
        // 2×2 RGB PNG: red, green, blue, white
        let data: Vec<u8> = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let path = create_test_png(&data, 2, 2, ColorType::Rgb);
        let thumb = load_thumbnail(&path).unwrap();

        assert_eq!(thumb.width, 2);
        assert_eq!(thumb.height, 2);
        assert_eq!(thumb.pixels.len(), 4);
        // pitch SHALL 等于 width
        assert_eq!(thumb.pitch, thumb.width);
    }

    // ── 1.3 加载有效 RGBA PNG ─────────────────────

    #[test]
    fn load_rgba_png_returns_video_buffer() {
        // 2×2 RGBA PNG: 半透明红像素
        let data: Vec<u8> = vec![
            255, 0, 0, 128, 0, 255, 0, 128, 0, 0, 255, 128, 128, 128, 128, 128,
        ];
        let path = create_test_png(&data, 2, 2, ColorType::Rgba);
        let thumb = load_thumbnail(&path).unwrap();

        assert_eq!(thumb.width, 2);
        assert_eq!(thumb.height, 2);
        assert_eq!(thumb.pixels.len(), 4);
        // alpha 通道在转换中应被丢弃——像素不应全为零
        assert!(thumb.pixels.iter().any(|&p| p != 0));
    }

    // ── 1.4 加载有效灰度 PNG ──────────────────────

    #[test]
    fn load_grayscale_png_returns_video_buffer() {
        // 2×2 Grayscale PNG: 黑、灰、白、黑
        let data: Vec<u8> = vec![0, 128, 255, 64];
        let path = create_test_png(&data, 2, 2, ColorType::Grayscale);
        let thumb = load_thumbnail(&path).unwrap();

        assert_eq!(thumb.width, 2);
        assert_eq!(thumb.height, 2);
        // 灰度值应映射为 RGB565
        assert!(thumb.pixels.iter().any(|&p| p != 0));
    }

    // ── 1.5 RGBA→RGB565 颜色转换精度 ──────────────

    #[test]
    fn color_conversion_precision() {
        // 2×2 RGBA: pure red, green, blue, white (alpha=255)
        let data: Vec<u8> = vec![
            255, 0, 0, 255, // Red
            0, 255, 0, 255, // Green
            0, 0, 255, 255, // Blue
            255, 255, 255, 255, // White
        ];
        let path = create_test_png(&data, 2, 2, ColorType::Rgba);
        let thumb = load_thumbnail(&path).unwrap();

        // R=255→31(5bit), G=0→0(6bit), B=0→0(5bit) → 0xF800
        // R=0→0,       G=255→63,     B=0→0        → 0x07E0
        // R=0→0,       G=0→0,        B=255→31     → 0x001F
        // R=255→31,    G=255→63,     B=255→31     → 0xFFFF
        assert_eq!(
            thumb.pixels,
            vec![0xF800, 0x07E0, 0x001F, 0xFFFF],
            "RGBA→RGB565 转换色值应精确"
        );
    }

    // ── 1.6 文件不存在返回 None ──────────────────

    #[test]
    fn nonexistent_file_returns_none() {
        let path = "/tmp/nonexistent_thumbnail_xxxx.png";
        let result = load_thumbnail(path);
        assert!(result.is_none(), "不存在的文件应返回 None");
    }

    // ── 空路径返回 None ──────────────────────────

    #[test]
    fn empty_path_returns_none() {
        let result = load_thumbnail("");
        assert!(result.is_none(), "空路径应返回 None");
    }

    // ── 1.7 非法 PNG 返回 None ───────────────────

    #[test]
    fn invalid_png_returns_none() {
        // 创建一个内容不是合法 PNG 的临时文件
        let path = std::env::temp_dir().join(format!("test_bad_{}.png", std::process::id()));
        let mut file = File::create(&path).unwrap();
        file.write_all(b"not a png file").unwrap();
        let path_str = path.to_str().unwrap();

        let result = load_thumbnail(path_str);
        assert!(result.is_none(), "非法 PNG 应返回 None，不应 panic");
    }
}
