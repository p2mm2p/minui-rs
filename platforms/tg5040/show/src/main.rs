//! `show`——启动画面显示工具（平台自治 bin，独立 crate）
//!
//! 独立二进制：加载一张 PNG 图片居中显示在屏幕上，等待指定延迟后退出。
//! 用于安装/更新流程中向用户展示进度提示图（由 boot.sh 等脚本调用）。
//!
//! 对应原版 C `show.c`（41 行）。本 crate 是
//! 平台自治 bin 的独立包（`platform-tg5040-show`）——**平台自治工具**：
//! 是否打包由平台决定（平台子 xtask 编译并复制），不复用 render，自实现
//! PNG 加载（`load_png`）与像素拷贝（`blit_buffer`），平台 lib 不依赖 render。
//!
//! 视频访问经 `Tg5040::new()`（`pub`）+ `common::platform::Platform` trait
//! 方法（`init_video`/`flip`/`quit_video`）——经平台 lib 的 pub 面
//! （Rust 可见性上 bin 是独立 crate，只能访问平台 lib 的 pub 面）。
//! 屏幕分辨率经 `Tg5040::SCREEN_WIDTH/HEIGHT` 读取——随编译期启用的
//! device feature 取值（smart 1280×720 / brick 1024×768）。
//!
//! ## 主流程
//!
//! 解析参数（path, delay=2）→ 图片不存在则静默退出 → `init_video` →
//! `load_png`（失败静默退出）→ 居中计算 → `blit_buffer` → `flip` →
//! 纯 `sleep(delay)` → `quit_video`。**不处理输入**——show 是一次性
//! 显示工具，忠实 C 原版纯 sleep（与 keymon 的常驻守护不同）。
//!

use std::fs::File;
use std::time::Duration;

use common::platform::Platform;
use common::video::{RGB_BLACK, Rect, VideoBuffer};
use platform_tg5040::Tg5040;

/// 默认延迟秒数（对应 C `argc>2 ? atoi(argv[2]) : 2`，show.c:20）
const DEFAULT_DELAY: u64 = 2;

/// 解析命令行参数
///
/// 对应原版 C 的 `argc<2` 检查与 `atoi(argv[2])`（show.c:11-20）。
///
/// ## 参数
///
/// - `args`：命令行参数（`args[0]` 为程序名）
///
/// ## 返回
///
/// - `Some((path, delay))`：图片路径与延迟秒数（未指定延迟时为 `DEFAULT_DELAY`）
/// - `None`：缺少图片路径参数（应打印 Usage 退出）
fn parse_args(args: &[String]) -> Option<(&str, u64)> {
    let path = args.get(1)?;
    let delay = args
        .get(2)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_DELAY);
    Some((path, delay))
}

/// 计算图片居中显示的左上角位置
///
/// 对应原版 C `(screen->w-img->w)/2, (screen->h-img->h)/2`（show.c:32）。
/// 用 `saturating_sub` 避免图片大于屏幕时 u32 下溢 panic——
/// 图片大于屏幕时结果为 0（贴左上角），越界部分由 `blit_buffer` 裁剪。
///
/// ## 参数
///
/// - `screen_w`：屏幕宽度
/// - `screen_h`：屏幕高度
/// - `img_w`：图片宽度
/// - `img_h`：图片高度
///
/// ## 返回
///
/// `(x, y)` 左上角坐标
fn centered_pos(screen_w: u32, screen_h: u32, img_w: u32, img_h: u32) -> (u32, u32) {
    let x = screen_w.saturating_sub(img_w) / 2;
    let y = screen_h.saturating_sub(img_h) / 2;
    (x, y)
}

/// 加载任意 PNG 图像并转换为 RGB565 像素缓冲
///
/// 对应原版 C `IMG_Load(path)`（`tg5040/show/show.c:31`）。自实现移植自
/// `render::image::load_png`（语义逐行等价）——平台自治 bin 不依赖 render。
///
/// ## 参数
///
/// * `path` - PNG 文件路径（如 `installing.png`）
///
/// ## 返回
///
/// * `Some(VideoBuffer)` - PNG 解码成功（RGBA→RGB565 转换，pitch = width）
/// * `None` - 文件不存在、PNG 格式错误、或不支持的色彩类型
fn load_png(path: &str) -> Option<VideoBuffer> {
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

/// 像素缓冲整块拷贝（源图像 → 目标缓冲）
///
/// 对应原版 C `SDL_BlitSurface()`（`tg5040/show/show.c:32`）。自实现移植自
/// `render::image::blit_buffer`（语义逐行等价）——平台自治 bin 不依赖 render。
///
/// 按 `pitch` 行步长逐像素拷贝，越界像素忽略（不 panic）。
/// **位置计算是调用方职责**——本函数只做拷贝与裁剪，不包含
/// 居中/对齐等布局逻辑。
///
/// ## 参数
///
/// * `dst` - 目标像素缓冲（屏幕）
/// * `src` - 源像素缓冲（图像）
/// * `x` - 目标位置左上角 x 坐标
/// * `y` - 目标位置左上角 y 坐标
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

/// 主流程（对应 C `main`，show.c:10-41）
///
/// # 参数
///
/// - `platform`: 平台实例（`Platform` trait 实现）
/// - `args`: 命令行参数
fn run(platform: &mut Tg5040, args: &[String]) {
    // 1. 参数解析（C :11-20）——无参数打印 Usage 退出
    let Some((path, delay)) = parse_args(args) else {
        println!("Usage: show image.png delay");
        return;
    };

    // 2. 图片不存在检查（C :18——`access(path, F_OK)`，"nothing to show"）
    if !common::utils::exists(path) {
        return;
    }

    // 3. 初始化视频（C :22-23）
    platform.init_video();

    // 4. 加载图片（C :31——IMG_Load）；解码失败静默退出
    let Some(img) = load_png(path) else {
        platform.quit_video();
        return;
    };

    // 5. 屏幕缓冲 + 居中位置（C :32——SDL_BlitSurface 目标矩形）
    let mut screen = VideoBuffer::new(Tg5040::SCREEN_WIDTH, Tg5040::SCREEN_HEIGHT);
    let (ox, oy) = centered_pos(screen.width, screen.height, img.width, img.height);

    // 6. 绘制：清屏 → 居中拷贝 → 呈现（C :29-34）
    screen.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w: screen.width,
            h: screen.height,
        },
        RGB_BLACK,
    );
    blit_buffer(&mut screen, &img, ox, oy);
    platform.flip(&screen, false);

    // 7. 纯 sleep 延迟（C :35——`sleep(delay)`，无输入/信号处理）
    std::thread::sleep(Duration::from_secs(delay));

    // 8. 退出清理（C :37-39）
    platform.quit_video();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut platform = Tg5040::new();
    run(&mut platform, &args);
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use png::{BitDepth, ColorType, Encoder};
    use std::io::Write;

    /// 测试辅助函数：在临时目录创建指定色彩类型的 PNG 文件，返回文件路径
    ///
    /// `data` 是原始像素字节（大小 = width × height × bytes_per_pixel）。
    /// 各 `color_type` 的每像素字节数：Rgb=3, Rgba=4, Grayscale=1, GrayscaleAlpha=2。
    fn create_test_png(data: &[u8], width: u32, height: u32, color_type: ColorType) -> String {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("test_show_image_{id}.png"));
        let file = File::create(&path).unwrap();
        let mut encoder = Encoder::new(file, width, height);
        encoder.set_color(color_type);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        path.to_str().unwrap().to_string()
    }

    // ── 参数解析 ────────────────────────────────

    #[test]
    fn no_args_returns_none() {
        // 无参数（只有程序名）→ None（打印 Usage 退出路径）
        let args = vec!["show".to_string()];
        assert!(parse_args(&args).is_none(), "无参数应返回 None");
    }

    #[test]
    fn path_without_delay_defaults_to_2() {
        // 有 path 无 delay → 默认 2 秒
        let args = vec!["show".to_string(), "/tmp/img.png".to_string()];
        let (path, delay) = parse_args(&args).unwrap();
        assert_eq!(path, "/tmp/img.png");
        assert_eq!(delay, 2, "未指定延迟应默认 2 秒");
    }

    #[test]
    fn path_with_delay_parses_seconds() {
        // 有 path + delay → 解析秒数
        let args = vec![
            "show".to_string(),
            "/tmp/img.png".to_string(),
            "3".to_string(),
        ];
        let (path, delay) = parse_args(&args).unwrap();
        assert_eq!(path, "/tmp/img.png");
        assert_eq!(delay, 3, "显式延迟应解析为秒数");
    }

    #[test]
    fn invalid_delay_falls_back_to_default() {
        // delay 非数字 → 回退默认 2（对应 C atoi 返回 0 的行为差异——
        // atoi("abc")=0 会导致 sleep(0)，Rust 回退默认更安全）
        let args = vec![
            "show".to_string(),
            "/tmp/img.png".to_string(),
            "abc".to_string(),
        ];
        let (_, delay) = parse_args(&args).unwrap();
        assert_eq!(delay, 2, "非法延迟应回退默认 2 秒");
    }

    // ── 居中位置计算 ─────────────────────────────

    #[test]
    fn centered_pos_normal() {
        // 屏幕 100×100、图片 10×10 → (45, 45)
        let (x, y) = centered_pos(100, 100, 10, 10);
        assert_eq!((x, y), (45, 45), "正常尺寸应居中");
    }

    #[test]
    fn centered_pos_odd_remainder() {
        // 屏幕 100×100、图片 11×11 → (44, 44)（整数除法向下取整，与 C 一致）
        let (x, y) = centered_pos(100, 100, 11, 11);
        assert_eq!((x, y), (44, 44), "奇数余数应向下取整");
    }

    #[test]
    fn centered_pos_image_larger_than_screen_no_panic() {
        // 图片大于屏幕（200×200 > 100×100）→ saturating_sub 结果为 0，不 panic
        let (x, y) = centered_pos(100, 100, 200, 200);
        assert_eq!((x, y), (0, 0), "图片大于屏幕时应贴左上角，不 panic");
    }

    #[test]
    fn centered_pos_image_equal_to_screen() {
        // 图片等于屏幕 → (0, 0)
        let (x, y) = centered_pos(100, 100, 100, 100);
        assert_eq!((x, y), (0, 0), "图片等于屏幕时应 (0,0)");
    }

    // ── load_png：加载有效 PNG ────────────────────

    #[test]
    fn load_rgb_png_returns_video_buffer() {
        // 2×2 RGB PNG: red, green, blue, white
        let data: Vec<u8> = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let path = create_test_png(&data, 2, 2, ColorType::Rgb);
        let img = load_png(&path).unwrap();

        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.pixels.len(), 4);
        // pitch SHALL 等于 width
        assert_eq!(img.pitch, img.width);
    }

    #[test]
    fn load_rgba_png_returns_video_buffer() {
        // 2×2 RGBA PNG: 半透明红像素
        let data: Vec<u8> = vec![
            255, 0, 0, 128, 0, 255, 0, 128, 0, 0, 255, 128, 128, 128, 128, 128,
        ];
        let path = create_test_png(&data, 2, 2, ColorType::Rgba);
        let img = load_png(&path).unwrap();

        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.pixels.len(), 4);
        // alpha 通道在转换中应被丢弃——像素不应全为零
        assert!(img.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn load_grayscale_png_returns_video_buffer() {
        // 2×2 Grayscale PNG: 黑、灰、白、黑
        let data: Vec<u8> = vec![0, 128, 255, 64];
        let path = create_test_png(&data, 2, 2, ColorType::Grayscale);
        let img = load_png(&path).unwrap();

        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        // 灰度值应映射为 RGB565
        assert!(img.pixels.iter().any(|&p| p != 0));
    }

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
        let img = load_png(&path).unwrap();

        // R=255→31(5bit), G=0→0(6bit), B=0→0(5bit) → 0xF800
        // R=0→0,       G=255→63,     B=0→0        → 0x07E0
        // R=0→0,       G=0→0,        B=255→31     → 0x001F
        // R=255→31,    G=255→63,     B=255→31     → 0xFFFF
        assert_eq!(
            img.pixels,
            vec![0xF800, 0x07E0, 0x001F, 0xFFFF],
            "RGBA→RGB565 转换色值应精确"
        );
    }

    // ── load_png：错误场景 ────────────────────────

    #[test]
    fn nonexistent_file_returns_none() {
        let path = "/tmp/nonexistent_show_image_xxxx.png";
        let result = load_png(path);
        assert!(result.is_none(), "不存在的文件应返回 None");
    }

    #[test]
    fn empty_path_returns_none() {
        let result = load_png("");
        assert!(result.is_none(), "空路径应返回 None");
    }

    #[test]
    fn invalid_png_returns_none() {
        // 创建一个内容不是合法 PNG 的临时文件
        let path =
            std::env::temp_dir().join(format!("test_show_bad_image_{}.png", std::process::id()));
        let mut file = File::create(&path).unwrap();
        file.write_all(b"not a png file").unwrap();
        let path_str = path.to_str().unwrap();

        let result = load_png(path_str);
        assert!(result.is_none(), "非法 PNG 应返回 None，不应 panic");
    }

    // ── blit_buffer：正常拷贝 ─────────────────────

    #[test]
    fn blit_buffer_copies_at_position() {
        // dst 100×100（全零），src 10×10（全 0xFFFF），目标 (5,5)
        let mut dst = VideoBuffer::new(100, 100);
        let src = VideoBuffer {
            pixels: vec![0xFFFF; 10 * 10],
            width: 10,
            height: 10,
            pitch: 10,
        };

        blit_buffer(&mut dst, &src, 5, 5);

        // (5..15, 5..15) 区域与 src 一致（全 0xFFFF）
        for row in 5..15 {
            for col in 5..15 {
                assert_eq!(
                    dst.pixels[row * 100 + col],
                    0xFFFF,
                    "({col},{row}) 应被拷贝"
                );
            }
        }
        // 其余区域不变（全零）
        assert_eq!(dst.pixels[0], 0, "左上角不应被写入");
        assert_eq!(dst.pixels[99], 0, "右下角不应被写入");
        assert_eq!(dst.pixels[4 * 100 + 5], 0, "目标区上方不应被写入");
    }

    // ── blit_buffer：越界裁剪 ─────────────────────

    #[test]
    fn blit_buffer_clips_right_overflow() {
        // dst 100×100，src 10×10，目标 (95,5)——右 5 列越界
        let mut dst = VideoBuffer::new(100, 100);
        let src = VideoBuffer {
            pixels: vec![0xFFFF; 10 * 10],
            width: 10,
            height: 10,
            pitch: 10,
        };

        blit_buffer(&mut dst, &src, 95, 5);

        // (95..100, 5..15) 应被拷贝（左 5 列）
        for row in 5..15 {
            for col in 95..100 {
                assert_eq!(
                    dst.pixels[row * 100 + col],
                    0xFFFF,
                    "({col},{row}) 应被拷贝"
                );
            }
        }
        // 越界部分不 panic，且不写入
        assert_eq!(dst.pixels[4 * 100 + 95], 0, "目标区上方不应被写入");
    }

    #[test]
    fn blit_buffer_clips_bottom_overflow() {
        // dst 100×100，src 10×10，目标 (5,95)——下 5 行越界
        let mut dst = VideoBuffer::new(100, 100);
        let src = VideoBuffer {
            pixels: vec![0xFFFF; 10 * 10],
            width: 10,
            height: 10,
            pitch: 10,
        };

        blit_buffer(&mut dst, &src, 5, 95);

        // (5..15, 95..100) 应被拷贝（上 5 行）
        for row in 95..100 {
            for col in 5..15 {
                assert_eq!(
                    dst.pixels[row * 100 + col],
                    0xFFFF,
                    "({col},{row}) 应被拷贝"
                );
            }
        }
        // 越界部分不 panic，且不写入
        assert_eq!(dst.pixels[94 * 100 + 5], 0, "目标区左侧不应被写入");
    }

    #[test]
    fn blit_buffer_fully_out_of_bounds_no_write() {
        // dst 100×100，src 10×10，目标 (200,200)——完全越界
        let mut dst = VideoBuffer::new(100, 100);
        let src = VideoBuffer {
            pixels: vec![0xFFFF; 10 * 10],
            width: 10,
            height: 10,
            pitch: 10,
        };

        blit_buffer(&mut dst, &src, 200, 200);

        // dst 应保持不变（全零）
        assert!(
            dst.pixels.iter().all(|&p| p == 0),
            "完全越界时 dst 不应有任何写入"
        );
    }

    // ── blit_buffer：pitch 步长 ───────────────────

    #[test]
    fn blit_buffer_respects_src_pitch() {
        // src 宽 2、高 2，但 pitch=4（行填充 2）——每行数据后跟 2 个填充像素
        // 第一行: [0x1111, 0x2222, 填充, 填充]
        // 第二行: [0x3333, 0x4444, 填充, 填充]
        let mut dst = VideoBuffer::new(4, 2);
        let src = VideoBuffer {
            pixels: vec![
                0x1111, 0x2222, 0xFFFF, 0xFFFF, 0x3333, 0x4444, 0xFFFF, 0xFFFF,
            ],
            width: 2,
            height: 2,
            pitch: 4,
        };

        blit_buffer(&mut dst, &src, 0, 0);

        assert_eq!(dst.pixels[0], 0x1111);
        assert_eq!(dst.pixels[1], 0x2222);
        assert_eq!(dst.pixels[2], 0, "行填充像素不应被拷贝（保持 0）");
        assert_eq!(dst.pixels[3], 0);
        assert_eq!(dst.pixels[4], 0x3333);
        assert_eq!(dst.pixels[5], 0x4444);
        assert_eq!(dst.pixels[6], 0, "行填充像素不应被拷贝（保持 0）");
        assert_eq!(dst.pixels[7], 0);
    }
}
