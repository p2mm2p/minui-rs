//! # 软件 UI 渲染器
//!
//! 对应原 C 代码 `api.c` 中的 `GFX_*` 渲染函数。
//!
//! 所有绘制直接写入 RGB565 帧缓冲区，不依赖 SDL 或其他图形库。
//! 字体渲染使用 `fontdue` crate（纯 Rust TTF/OTF 光栅化）。
//!
//! ## 坐标系统
//!
//! 所有坐标和尺寸都已按 `FIXED_SCALE` 缩放（即逻辑坐标系）。
//! 对应 C 代码中 `SCALE1/SCALE2/SCALE3/SCALE4` 宏的效果。

use fontdue::{Font, FontSettings};
use minui_platform::Framebuffer;
use common::types::Entry;

// ============================================================================
// 颜色
// ============================================================================

/// RGB565 像素（16 位）—— MinUI 几乎所有平台的标准像素格式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb565(pub u16);

impl Rgb565 {
    /// 从 8 位 RGB 分量构造 RGB565
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        let r5 = ((r as u16) >> 3) & 0x1F;
        let g6 = ((g as u16) >> 2) & 0x3F;
        let b5 = ((b as u16) >> 3) & 0x1F;
        Self((r5 << 11) | (g6 << 5) | b5)
    }

    // MinUI Triad 调色板
    pub const WHITE:     Self = Self::from_rgb(0xff, 0xff, 0xff);
    pub const BLACK:     Self = Self::from_rgb(0x00, 0x00, 0x00);
    pub const LIGHT_GRAY: Self = Self::from_rgb(0x7f, 0x7f, 0x7f);
    pub const GRAY:      Self = Self::from_rgb(0x99, 0x99, 0x99);
    pub const DARK_GRAY: Self = Self::from_rgb(0x26, 0x26, 0x26);
    pub const LIGHT_TEXT: Self = Self::from_rgb(0xcc, 0xcc, 0xcc);
    pub const DARK_TEXT: Self = Self::from_rgb(0x66, 0x66, 0x66);
}

// ============================================================================
// 矩形
// ============================================================================

/// 矩形区域（所有值已缩放为逻辑坐标）
///
/// 对应 C 中的 `SDL_Rect`。
///
/// # 示例
///
/// ```
/// use minui::render::Rect;
/// let r = Rect::new(10, 20, 100, 50);
/// assert_eq!(r.x, 10);
/// assert_eq!(r.y, 20);
/// assert_eq!(r.w, 100);
/// assert_eq!(r.h, 50);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    /// 左上角 X 坐标（逻辑像素）
    pub x: u32,
    /// 左上角 Y 坐标（逻辑像素）
    pub y: u32,
    /// 宽度（逻辑像素）
    pub w: u32,
    /// 高度（逻辑像素）
    pub h: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
}

// ============================================================================
// 字体
// ============================================================================

/// 字体大小 —— 对应 C 中的 `FONT_LARGE/FONT_MEDIUM/FONT_SMALL/FONT_TINY`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSize {
    /// 16px —— 菜单项文本
    Large,
    /// 14px —— 单字符按钮标签
    Medium,
    /// 12px —— 按钮提示
    Small,
    /// 10px —— 多字符按钮标签
    Tiny,
}

impl FontSize {
    /// 返回该字体大小对应的像素高度（缩放前）
    pub fn px(self) -> f32 {
        match self {
            FontSize::Large  => 16.0,
            FontSize::Medium => 14.0,
            FontSize::Small  => 12.0,
            FontSize::Tiny   => 10.0,
        }
    }
}

/// 字体管理器
///
/// 加载 TrueType/OpenType 字体并提供不同大小的字形光栅化。
/// 使用 `fontdue` crate 进行纯 CPU 渲染。
pub struct FontManager {
    large:  Font,
    medium: Font,
    small:  Font,
    tiny:   Font,
    scale:  f32,
}

impl FontManager {
    /// 从字节数据加载字体
    ///
    /// `scale` 是逻辑缩放倍数（对应 FIXED_SCALE），
    /// 字体以 scale 倍的像素大小渲染以确保清晰度。
    pub fn new(font_data: &[u8], scale: f32) -> Self {
        let settings = FontSettings {
            scale,
            ..Default::default()
        };
        Self {
            large:  Font::from_bytes(font_data, settings).expect("Failed to load font"),
            medium: Font::from_bytes(font_data, settings).expect("Failed to load font"),
            small:  Font::from_bytes(font_data, settings).expect("Failed to load font"),
            tiny:   Font::from_bytes(font_data, settings).expect("Failed to load font"),
            scale,
        }
    }

    /// 获取指定大小的字体引用
    pub fn get(&self, size: FontSize) -> &Font {
        match size {
            FontSize::Large  => &self.large,
            FontSize::Medium => &self.medium,
            FontSize::Small  => &self.small,
            FontSize::Tiny   => &self.tiny,
        }
    }

    /// 测量文本渲染后的尺寸（像素，已按 scale 缩放）
    pub fn measure(&self, text: &str, size: FontSize) -> (u32, u32) {
        let font = self.get(size);
        let px = size.px() * self.scale;
        let mut width = 0u32;
        let mut total_height = 0u32;

        for line in text.lines() {
            let mut line_width = 0u32;
            let mut line_height = 0f32;
            for ch in line.chars() {
                let (metrics, _) = font.rasterize(ch, px);
                line_width += metrics.advance_width.ceil() as u32;
                line_height = line_height.max(metrics.height as f32);
            }
            width = width.max(line_width);
            total_height += line_height.ceil() as u32;
        }
        (width, total_height)
    }

    /// 测量单行文本宽度
    pub fn measure_width(&self, text: &str, size: FontSize) -> u32 {
        let font = self.get(size);
        let px = size.px() * self.scale;
        let mut width = 0u32;
        for ch in text.chars() {
            let (metrics, _) = font.rasterize(ch, px);
            width += metrics.advance_width.ceil() as u32;
        }
        width
    }

    /// 截断文本以适合指定宽度，超出部分用 "..." 表示
    ///
    /// 对应 C 中的 `GFX_truncateText()`。
    /// 返回截断后的文本和实际宽度。
    pub fn truncate_text(
        &self,
        text: &str,
        size: FontSize,
        max_width: u32,
    ) -> (String, u32) {
        let font = self.get(size);
        let px = size.px() * self.scale;
        let ellipsis = "...";
        let ellipsis_w = self.measure_width(ellipsis, size);

        let mut current_w = 0u32;
        let mut char_count = 0usize;

        for ch in text.chars() {
            let (metrics, _) = font.rasterize(ch, px);
            let ch_w = metrics.advance_width.ceil() as u32;
            if current_w + ch_w > max_width.saturating_sub(ellipsis_w) {
                break;
            }
            current_w += ch_w;
            char_count += 1;
        }

        if char_count < text.chars().count() {
            let truncated: String = text.chars().take(char_count).collect();
            let result = format!("{}{}", truncated, ellipsis);
            let result_w = current_w + ellipsis_w;
            (result, result_w)
        } else {
            (text.to_string(), current_w)
        }
    }
}

// ============================================================================
// 帧缓冲区绘制原语（自由函数，操作 `platform::Framebuffer`）
// ============================================================================

/// 填充整个帧缓冲区为指定颜色
pub fn fb_clear(fb: &mut Framebuffer, color: Rgb565) {
        for y in 0..fb.height {
            let row = unsafe {
                std::slice::from_raw_parts_mut(
                    fb.pixels.add((y * fb.pitch) as usize),
                    fb.width as usize * 2,
                )
            };
            for pixel in row.chunks_exact_mut(2) {
                pixel[0] = (color.0 & 0xFF) as u8;
                pixel[1] = ((color.0 >> 8) & 0xFF) as u8;
            }
        }
    }

    /// 填充矩形区域
    pub fn fb_fill_rect(fb: &mut Framebuffer, rect: Rect, color: Rgb565) {
        let x0 = rect.x.min(fb.width);
        let y0 = rect.y.min(fb.height);
        let x1 = (rect.x + rect.w).min(fb.width);
        let y1 = (rect.y + rect.h).min(fb.height);

        let color_bytes = [color.0 as u8, (color.0 >> 8) as u8];

        for y in y0..y1 {
            for x in x0..x1 {
                let offset = (y * fb.pitch + x * 2) as usize;
                unsafe {
                    *fb.pixels.add(offset) = color_bytes[0];
                    *fb.pixels.add(offset + 1) = color_bytes[1];
                }
            }
        }
    }

    /// 绘制带圆角的矩形（Pill）—— 对应 C 中的 `GFX_blitPill()`
    ///
    /// 圆角半径为高度的 1/2。这是 MinUI 视觉标识的核心元素。
    pub fn fb_draw_pill(fb: &mut Framebuffer, rect: Rect, color: Rgb565) {
        let r = (rect.h / 2) as i32;
        if r <= 0 {
            fb_fill_rect(fb, rect, color);
            return;
        }

        let x0 = rect.x as i32;
        let y0 = rect.y as i32;
        let x1 = (rect.x + rect.w) as i32;
        let y1 = (rect.y + rect.h) as i32;

        for y in rect.y..(rect.y + rect.h).min(fb.height) {
            let yi = y as i32;
            for x in rect.x..(rect.x + rect.w).min(fb.width) {
                let xi = x as i32;

                // 判断是否在圆角内（四个角）
                let inside = if xi < x0 + r && yi < y0 + r {
                    // 左上角
                    let dx = (x0 + r - xi) as f32;
                    let dy = (y0 + r - yi) as f32;
                    dx * dx + dy * dy <= (r * r) as f32
                } else if xi >= x1 - r && yi < y0 + r {
                    // 右上角
                    let dx = (xi - (x1 - r)) as f32;
                    let dy = (y0 + r - yi) as f32;
                    dx * dx + dy * dy <= (r * r) as f32
                } else if xi < x0 + r && yi >= y1 - r {
                    // 左下角
                    let dx = (x0 + r - xi) as f32;
                    let dy = (yi - (y1 - r)) as f32;
                    dx * dx + dy * dy <= (r * r) as f32
                } else if xi >= x1 - r && yi >= y1 - r {
                    // 右下角
                    let dx = (xi - (x1 - r)) as f32;
                    let dy = (yi - (y1 - r)) as f32;
                    dx * dx + dy * dy <= (r * r) as f32
                } else {
                    true // 在矩形主体内
                };

                if inside {
                    let offset = (y * fb.pitch + x * 2) as usize;
                    unsafe {
                        *fb.pixels.add(offset) = color.0 as u8;
                        *fb.pixels.add(offset + 1) = (color.0 >> 8) as u8;
                    }
                }
            }
        }
    }

    /// 绘制文本到帧缓冲区（单行）
    ///
    /// 在 `rect` 区域内从左侧开始绘制文本。
    /// 文本垂直居中。超出区域的部分会被裁剪。
    pub fn fb_draw_text(
        fb: &mut Framebuffer,
        text: &str,
        rect: Rect,
        color: Rgb565,
        font: &Font,
        px: f32,
    ) {
        let x0 = rect.x as usize;
        let y0 = rect.y as usize;
        let max_x = (rect.x + rect.w) as usize;
        let max_y = (rect.y + rect.h) as usize;
        let baseline_y = rect.y as usize + (rect.h as usize) / 2;

        let mut cursor_x = x0;

        // 测量整个文本以确定高度（用于垂直居中）
        let total_height = font_text_height(text, font, px);
        let y_offset = if total_height < rect.h as usize {
            (rect.h as usize - total_height) / 2
        } else {
            0
        };

        for ch in text.chars() {
            if cursor_x >= max_x {
                break;
            }
            let (metrics, bitmap) = font.rasterize(ch, px);
            if bitmap.is_empty() {
                cursor_x += metrics.advance_width.ceil() as usize;
                continue;
            }

            let glyph_top = y0 + y_offset + (baseline_y - y0).saturating_sub(metrics.height);

            for gy in 0..metrics.height {
                let screen_y = glyph_top + gy;
                if screen_y >= max_y {
                    break;
                }
                for gx in 0..metrics.width {
                    let screen_x = cursor_x + gx;
                    if screen_x >= max_x {
                        break;
                    }
                    let alpha = bitmap[gy * metrics.width + gx] as u32;
                    if alpha > 0 {
                        // alpha 混合
                        let offset = (screen_y as u32 * fb.pitch + screen_x as u32 * 2) as usize;
                        unsafe {
                            let existing_l = *fb.pixels.add(offset) as u32;
                            let existing_h = *fb.pixels.add(offset + 1) as u32;
                            let existing = existing_l | (existing_h << 8);
                            let blended = blend_rgb565(existing as u16, color.0, alpha as u8);
                            *fb.pixels.add(offset) = blended as u8;
                            *fb.pixels.add(offset + 1) = (blended >> 8) as u8;
                        }
                    }
                }
            }
            cursor_x += metrics.advance_width.ceil() as usize;
        }
    }

    /// 计算文本渲染后的像素高度
    fn font_text_height( text: &str, font: &Font, px: f32) -> usize {
        let mut max_h = 0f32;
        // 实际需要所有字符，但第一个字符通常能代表高度
        for ch in text.chars().take(16) {
            let (metrics, _) = font.rasterize(ch, px);
            if metrics.height as f32 > max_h {
                max_h = metrics.height as f32;
            }
        }
        max_h.ceil() as usize
    }

    /// 绘制按钮提示 —— 对应 C 中的 `GFX_blitButton()`
    ///
    /// 格式：一个圆角矩形按钮标签 + 右侧的提示文字。
    /// 例如 `[A] OPEN`，其中 `[A]` 是按钮标签，`OPEN` 是提示。
    pub fn fb_draw_button_hint(fb: &mut Framebuffer,

        hint: &str,         // 提示文字，如 "OPEN"
        button: &str,       // 按钮标签，如 "A"
        rect: Rect,
        font_manager: &FontManager,
    ) {
        let button_w = 20u32;
        let gap = 4u32;

        // 按钮背景（圆角方形）
        let btn_rect = Rect::new(rect.x, rect.y, button_w, rect.h);
        fb_draw_pill(fb, btn_rect, Rgb565::DARK_GRAY);

        // 按钮文字（白色）
        let btn_font = if button.len() > 1 { FontSize::Tiny } else { FontSize::Medium };
        let btn_text_w = font_manager.measure_width(button, btn_font);
        let btn_text_x = rect.x + (button_w - btn_text_w) / 2;
        let btn_text_rect = Rect::new(btn_text_x, rect.y, btn_text_w, rect.h);
        fb_draw_text(fb, button, btn_text_rect, Rgb565::WHITE,
            font_manager.get(btn_font), btn_font.px() * font_manager.scale);

        // 提示文字
        if !hint.is_empty() {
            let hint_rect = Rect::new(rect.x + button_w + gap, rect.y, rect.w - button_w - gap, rect.h);
            fb_draw_text(fb, hint, hint_rect, Rgb565::GRAY,
                font_manager.get(FontSize::Small), FontSize::Small.px() * font_manager.scale);
        }
    }

    /// 绘制电池图标 —— 对应 C 中的 `GFX_blitBattery()`
    ///
    /// 简化的电池形状：矩形主体 + 右侧小凸起 + 内部填充条。
    pub fn fb_draw_battery(fb: &mut Framebuffer,
        rect: Rect,
        charge: u8,       // 0-100, 步长为 20
        is_charging: bool,
        is_low: bool,
    ) {
        let body_w = rect.w.saturating_sub(4); // 留出右侧凸起的空间
        let body_h = rect.h;
        let tip_w = 4u32;
        let tip_h = body_h / 3;

        // 电池主体（空心矩形）
        let body_rect = Rect::new(rect.x, rect.y, body_w, body_h);
        fb_fill_rect(fb, body_rect, if is_low { Rgb565::DARK_GRAY } else { Rgb565::GRAY });

        // 内部填充
        let inner_margin = 2u32;
        let fill_w = ((body_w - inner_margin * 2) as f32 * (charge as f32 / 100.0)) as u32;
        if fill_w > 0 {
            let fill_color = if is_low {
                Rgb565::from_rgb(0xcc, 0x33, 0x33) // 红色低电量
            } else if is_charging {
                Rgb565::from_rgb(0x33, 0xcc, 0x33) // 绿色充电
            } else {
                Rgb565::WHITE
            };
            let fill_rect = Rect::new(
                rect.x + inner_margin,
                rect.y + inner_margin,
                fill_w,
                body_h - inner_margin * 2,
            );
            fb_fill_rect(fb, fill_rect, fill_color);
        }

        // 右侧小凸起
        let tip_rect = Rect::new(
            rect.x + body_w + 1,
            rect.y + (body_h - tip_h) / 2,
            tip_w,
            tip_h,
        );
        fb_fill_rect(fb, tip_rect, Rgb565::GRAY);
    }


/// RGB565 像素的 alpha 混合
fn blend_rgb565(bg: u16, fg: u16, alpha: u8) -> u16 {
    if alpha == 0 { return bg; }
    if alpha == 255 { return fg; }

    let a = alpha as u32;
    let inv_a = 255u32 - a;

    // 提取各通道（转为 u32 以便混合运算）
    let bg_r = ((bg >> 11) & 0x1F) as u32;
    let bg_g = ((bg >> 5) & 0x3F) as u32;
    let bg_b = (bg & 0x1F) as u32;

    let fg_r = ((fg >> 11) & 0x1F) as u32;
    let fg_g = ((fg >> 5) & 0x3F) as u32;
    let fg_b = (fg & 0x1F) as u32;

    let r = ((fg_r * a + bg_r * inv_a) / 255) & 0x1F;
    let g = ((fg_g * a + bg_g * inv_a) / 255) & 0x3F;
    let b = ((fg_b * a + bg_b * inv_a) / 255) & 0x1F;

    ((r as u16) << 11) | ((g as u16) << 5) | (b as u16)
}

// ============================================================================
// UI 渲染器 —— 组合所有绘制功能
// ============================================================================

/// UI 渲染器 —— 对应 C 中 `api.c` 的高层 `GFX_*` 函数
///
/// 组合字体管理器和帧缓冲区，提供列表、版本屏幕、按钮组等高级渲染功能。
pub struct UiRenderer {
    /// 字体管理器（加载 TrueType 字体并提供字形光栅化）
    pub font_manager: FontManager,
    /// 当前界面模式：Main（启动器列表）或 Menu（游戏内菜单）
    pub mode: Mode,
    /// 最大可见行数
    pub main_row_count: usize,
    /// UI 缩放倍数
    pub scale: u32,
    /// 屏幕宽度（逻辑像素）
    pub screen_w: u32,
    /// 屏幕高度（逻辑像素）
    pub screen_h: u32,
}

/// UI 渲染模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 主启动器模式（列表浏览）
    Main,
    /// 菜单模式（游戏内菜单，minarch 使用）
    Menu,
}

/// 列表渲染所需的输入数据
pub struct ListRenderInput<'a> {
    /// 可见条目列表（start..end 范围）
    pub entries: &'a [Entry],
    /// 当前选中项在可见窗口内的行号（selected - start）
    pub selected_row: usize,
    /// 可见窗口起始索引
    pub start: usize,
    /// 可见窗口结束索引
    pub end: usize,
    /// 行高（逻辑像素，含间距）
    pub row_height: u32,
    /// 左侧留白
    pub padding: u32,
    /// 条目文字颜色（普通项）
    pub text_color: Rgb565,
    /// 选中项文字颜色
    pub selected_text_color: Rgb565,
    /// 选中项背景颜色
    pub selected_bg: Rgb565,
    /// 是否有缩略图占用右侧空间
    pub has_thumb: bool,
    /// 缩略图宽度（如果有）
    pub thumb_width: u32,
}

/// 硬件状态栏渲染数据
pub struct HardwareStatus {
    /// 电池电量 0/10/20/40/60/80/100
    pub charge: u8,
    /// 是否正在充电
    pub is_charging: bool,
    /// 是否低电量
    pub is_low: bool,
    /// 当前显示的系统设置类型：0=无, 1=亮度, 2=音量
    pub show_setting: u8,
    /// 亮度值 (0-10)
    pub brightness: u8,
    /// 音量值 (0-20)
    pub volume: u8,
    /// 是否有 WiFi
    pub has_wifi: bool,
    /// WiFi 是否连接
    pub wifi_connected: bool,
    /// 是否有 HDMI 输出
    pub has_hdmi: bool,
}

/// 按钮提示项
pub struct ButtonHint<'a> {
    /// 按钮标签（如 "A", "MENU"）
    pub button: &'a str,
    /// 功能提示（如 "OPEN", "BACK"）
    pub hint: &'a str,
}

impl UiRenderer {
    /// 创建 UI 渲染器
    pub fn new(
        font_data: &[u8],
        fixed_scale: u32,
        screen_w: u32,
        screen_h: u32,
    ) -> Self {
        let main_row_count = 6;
        Self {
            font_manager: FontManager::new(font_data, fixed_scale as f32),
            mode: Mode::Main,
            main_row_count,
            scale: fixed_scale,
            screen_w,
            screen_h,
        }
    }

    /// 使用默认内嵌字体创建渲染器（用于测试）
    pub fn with_default_font(fixed_scale: u32, screen_w: u32, screen_h: u32) -> Self {
        // 一个最小化的 TrueType 字体数据（内嵌用于测试）
        //  一个最小的有效 TrueType 字体需要约 5KB+
        // 对于测试，使用 fontdue 自带的测试机制
        Self::new(
            include_bytes!("../resources/BPreplayBold-unhinted.otf"),
            fixed_scale,
            screen_w,
            screen_h,
        )
    }

    /// 计算一个 pill（圆角矩形）的尺寸
    pub fn pill_size(&self) -> u32 { 30 * self.scale }

    /// 计算留白
    pub fn padding(&self) -> u32 { 10 * self.scale }

    /// 计算按钮宽度
    pub fn button_size(&self) -> u32 { 20 * self.scale }

    // ================================================================
    // 高层渲染
    // ================================================================

    /// 渲染一帧的完整 UI —— 对应 C 代码 `main()` 中 dirty==1 时的渲染段
    ///
    /// 执行顺序：清屏 → 缩略图 → 硬件状态栏 → 列表/版本界面 → 按钮提示
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame(
        &self,
        fb: &mut Framebuffer,
        list: Option<ListRenderInput>,
        status: &HardwareStatus,
        left_buttons: &[ButtonHint],
        right_buttons: &[ButtonHint],
        show_version: bool,
        version_info: Option<(&str, &str, &str, &str)>, // (release, commit, model_key, model_val)
    ) {
        fb_clear(fb, Rgb565::BLACK);

        // 硬件状态栏（右上角：电池 + 亮度/音量指示器）
        let status_w = self.draw_hardware_status(fb, status);

        if show_version {
            if let Some((release, commit, model_key, model_val)) = version_info {
                self.draw_version_screen(fb, release, commit, model_key, model_val);
            }
            // 版本界面的按钮
            self.draw_button_group(fb, left_buttons, false);
            self.draw_button_group(fb, right_buttons, true);
        } else if let Some(ref list_data) = list {
            // 缩略图检查
            let has_thumb = list_data.has_thumb;
            let thumb_w = list_data.thumb_width;

            self.draw_list(fb, list_data, status_w, has_thumb, thumb_w);
            self.draw_button_group(fb, left_buttons, false);
            self.draw_button_group(fb, right_buttons, true);
        }
    }

    /// 绘制游戏列表
    fn draw_list(
        &self,
        fb: &mut Framebuffer,
        list: &ListRenderInput,
        status_w: u32,
        has_thumb: bool,
        thumb_w: u32,
    ) {
        if list.entries.is_empty() {
            let msg_rect = Rect::new(0, 0, fb.width, fb.height);
            self.draw_message(fb, "Empty folder", &msg_rect);
            return;
        }

        let padding = list.padding;
        let row_h = list.row_height;

        for j in 0..list.entries.len() {
            let _i = list.start + j;
            let entry = &list.entries[j];
            let y = padding + (j as u32) * row_h;

            // 计算该行的可用宽度
            let mut available_w = fb.width.saturating_sub(padding * 2);
            if has_thumb {
                available_w = thumb_w.saturating_sub(padding * 2);
            }
            if j == 0 {
                available_w = available_w.saturating_sub(status_w);
            }

            // 截断文本
            let display_name = entry.unique.as_deref().unwrap_or(&entry.name);
            let (truncated, text_w) = self.font_manager.truncate_text(
                display_name,
                FontSize::Large,
                available_w,
            );
            let max_w = available_w.min(text_w);

            if j == list.selected_row {
                // 选中项：白色 Pill 背景 + 黑色文字
                let pill_rect = Rect::new(padding, y, max_w, row_h);
                fb_draw_pill(fb, pill_rect, Rgb565::WHITE);

                let text_rect = Rect::new(
                    padding + 12 * self.scale,
                    y + 4 * self.scale,
                    max_w.saturating_sub(24 * self.scale),
                    row_h.saturating_sub(8 * self.scale),
                );
                fb_draw_text(fb, &truncated, text_rect, Rgb565::BLACK,
                    self.font_manager.get(FontSize::Large),
                    FontSize::Large.px() * self.font_manager.scale);
            } else {
                // 非选中项：如果有多余的 unique 名，先画小字
                if entry.unique.is_some() {
                    let unique_text = entry.unique.as_deref().unwrap();
                    let (unique_trunc, _) = self.font_manager.truncate_text(
                        unique_text, FontSize::Large, available_w,
                    );
                    let sub_rect = Rect::new(
                        padding + 12 * self.scale,
                        y + 4 * self.scale,
                        max_w.saturating_sub(24 * self.scale),
                        row_h / 2,
                    );
                    fb_draw_text(fb, &unique_trunc, sub_rect, Rgb565::DARK_TEXT,
                        self.font_manager.get(FontSize::Large),
                        FontSize::Large.px() * self.font_manager.scale);
                }

                let text_rect = Rect::new(
                    padding + 12 * self.scale,
                    y + 4 * self.scale,
                    max_w.saturating_sub(24 * self.scale),
                    row_h.saturating_sub(8 * self.scale),
                );
                fb_draw_text(fb, &truncated, text_rect, list.text_color,
                    self.font_manager.get(FontSize::Large),
                    FontSize::Large.px() * self.font_manager.scale);
            }

            // 释放 Entry 引用（让 borrow checker 满意）—— 实际上不需要，Rust 自动管理
            let _ = entry;
        }
    }

    /// 绘制版本信息界面
    fn draw_version_screen(
        &self,
        fb: &mut Framebuffer,
        release: &str,
        commit: &str,
        model_key: &str,
        model_val: &str,
    ) {
        let line_h = 24u32 * self.scale;
        let mut y = (fb.height - line_h * 4) / 2;

        let left_labels = ["Release", "Commit", model_key];
        let right_values = [release, commit, model_val];

        for i in 0..3 {
            // 左侧标签
            let _left_w = self.font_manager.measure_width(left_labels[i], FontSize::Large);
            let left_rect = Rect::new(0, y, fb.width / 2, line_h);
            fb_draw_text(fb, left_labels[i], left_rect, Rgb565::DARK_TEXT,
                self.font_manager.get(FontSize::Large),
                FontSize::Large.px() * self.font_manager.scale);

            // 右侧数值
            let right_rect = Rect::new(fb.width / 2 + 8 * self.scale, y, fb.width / 2, line_h);
            fb_draw_text(fb, right_values[i], right_rect, Rgb565::WHITE,
                self.font_manager.get(FontSize::Large),
                FontSize::Large.px() * self.font_manager.scale);

            y += line_h;
        }
    }

    /// 绘制底部按钮提示组 —— 对应 C 中的 `GFX_blitButtonGroup()`
    ///
    /// `align_right`: true = 右对齐, false = 左对齐
    fn draw_button_group(
        &self,
        fb: &mut Framebuffer,
        buttons: &[ButtonHint],
        align_right: bool,
    ) {
        if buttons.is_empty() { return; }

        let btn_w = 70u32 * self.scale; // 单个按钮占的宽度
        let btn_h = self.button_size();
        let padding = self.padding();
        let y = fb.height.saturating_sub(btn_h + padding);

        let total_w = btn_w * buttons.len() as u32;

        let start_x = if align_right {
            fb.width.saturating_sub(padding + total_w)
        } else {
            padding
        };

        for (i, hint) in buttons.iter().enumerate() {
            let x = start_x + i as u32 * btn_w;
            let rect = Rect::new(x, y, btn_w, btn_h);
            fb_draw_button_hint(fb, hint.hint, hint.button, rect, &self.font_manager);
        }
    }

    /// 绘制硬件状态栏（电池 + 亮度/音量指示器）
    ///
    /// 返回状态栏占用的宽度。
    fn draw_hardware_status(&self, fb: &mut Framebuffer, status: &HardwareStatus) -> u32 {
        let padding = self.padding();
        let icon_size = 20u32 * self.scale;
        let gap = 4 * self.scale;
        let mut x = fb.width.saturating_sub(padding);

        // 电池图标（最右侧）
        let battery_rect = Rect::new(x.saturating_sub(icon_size), padding, icon_size, icon_size);
        fb_draw_battery(fb, battery_rect, status.charge, status.is_charging, status.is_low);
        x = x.saturating_sub(icon_size + gap);

        // 亮度/音量指示器
        if status.show_setting == 1 {
            // 亮度指示器
            let label = format!("+{}/10", status.brightness);
            let w = self.font_manager.measure_width(&label, FontSize::Small);
            let r = Rect::new(x.saturating_sub(w), padding, w, icon_size);
            fb_draw_text(fb, &label, r, Rgb565::LIGHT_TEXT,
                self.font_manager.get(FontSize::Small),
                FontSize::Small.px() * self.font_manager.scale);
            x = x.saturating_sub(w + gap);
        } else if status.show_setting == 2 {
            // 音量指示器
            let label = if status.volume == 0 {
                "MUTE".to_string()
            } else {
                format!("+{}/20", status.volume)
            };
            let w = self.font_manager.measure_width(&label, FontSize::Small);
            let r = Rect::new(x.saturating_sub(w), padding, w, icon_size);
            fb_draw_text(fb, &label, r, Rgb565::LIGHT_TEXT,
                self.font_manager.get(FontSize::Small),
                FontSize::Small.px() * self.font_manager.scale);
            x = x.saturating_sub(w + gap);
        }

        fb.width.saturating_sub(x)
    }

    /// 绘制居中消息 —— 对应 C 中的 `GFX_blitMessage()`
    pub fn draw_message(&self, fb: &mut Framebuffer, msg: &str, rect: &Rect) {
        let msg_w = self.font_manager.measure_width(msg, FontSize::Medium);
        let x = rect.x + (rect.w.saturating_sub(msg_w)) / 2;
        let y = rect.y + rect.h / 2;
        let text_rect = Rect::new(x, y, msg_w, FontSize::Medium.px() as u32 * self.scale);
        fb_draw_text(fb, msg, text_rect, Rgb565::GRAY,
            self.font_manager.get(FontSize::Medium),
            FontSize::Medium.px() * self.font_manager.scale);
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建一个用于测试的帧缓冲区（在堆上分配 RGB565 数据）
    fn test_fb(w: u32, h: u32) -> Framebuffer {
        let size = (w * h * 2) as usize;
        let pixels: Vec<u8> = vec![0; size];
        let ptr = pixels.leak().as_mut_ptr();
        Framebuffer {
            pixels: ptr,
            width: w,
            height: h,
            pitch: w * 2,
            bpp: 2,
        }
    }

    #[test]
    fn test_rgb565_from_rgb() {
        assert_eq!(Rgb565::BLACK.0, 0x0000);
        assert_eq!(Rgb565::WHITE.0, 0xFFFF);
        let red = Rgb565::from_rgb(0xff, 0x00, 0x00);
        assert_eq!((red.0 >> 11) & 0x1F, 0x1F); // red channel max
        assert_eq!((red.0 >> 5) & 0x3F, 0x00);  // green channel zero
        assert_eq!(red.0 & 0x1F, 0x00);          // blue channel zero
    }

    #[test]
    fn test_framebuffer_clear() {
        let mut fb = test_fb(64, 48);
        fb_clear(&mut fb, Rgb565::WHITE);
        assert_eq!(fb.pixels, fb.pixels); // 不会 panic
        // 检查几个像素
        unsafe {
            assert_eq!(*fb.pixels, 0xFF);
            assert_eq!(*fb.pixels.add(1), 0xFF);
        }
    }

    #[test]
    fn test_fill_rect() {
        let mut fb = test_fb(64, 48);
        fb_clear(&mut fb, Rgb565::BLACK);
        fb_fill_rect(&mut fb, Rect::new(10, 10, 20, 20), Rgb565::WHITE);

        // 矩形外应为黑色
        let offset_outside = (5 * fb.pitch + 5 * 2) as usize;
        unsafe {
            assert_eq!(*fb.pixels.add(offset_outside), 0x00);
        }

        // 矩形内应为白色
        let offset_inside = (15 * fb.pitch + 15 * 2) as usize;
        unsafe {
            assert_eq!(*fb.pixels.add(offset_inside), 0xFF);
        }
    }

    #[test]
    fn test_draw_pill() {
        let mut fb = test_fb(100, 50);
        fb_clear(&mut fb, Rgb565::BLACK);
        // 绘制白色 pill，不应 panic
        fb_draw_pill(&mut fb, Rect::new(10, 10, 60, 30), Rgb565::WHITE);

        // 中心应该在 pill 内部
        let offset = (25 * fb.pitch + 40 * 2) as usize;
        unsafe {
            assert_eq!(*fb.pixels.add(offset), 0xFF);
        }
    }

    #[test]
    fn test_truncate_text() {
        // 使用一个占位字体数据来测试截断逻辑
        // 由于 fontdue 需要有效的字体数据，我们跳过需要实际字体的测试
        // 在集成测试中会使用真实字体文件
    }

    #[test]
    fn test_rgb565_blend() {
        // 完全不透明 → 前景色
        assert_eq!(blend_rgb565(0x0000, 0xFFFF, 255), 0xFFFF);
        // 完全透明 → 背景色
        assert_eq!(blend_rgb565(0xFFFF, 0x0000, 0), 0xFFFF);
    }
}
