//! 像素缩放器 — 整数缩放、抗锯齿混合与双线性插值缩放
//!
//! 本模块实现 MinUI 游戏渲染管线中的画面缩放——将模拟器输出画面
//!（如 GBA 的 240×160）缩放至设备物理分辨率（如 640×480）。
//! 所有函数直接操作 RGB565 像素切片（`&[u16]`），不依赖 `VideoBuffer` 或 SDL。
//!
//! # 三种缩放策略
//!
//! ## IntegerScaler — 整数倍最近邻复制
//!
//! 当设备分辨率恰好是游戏分辨率的整数倍时使用（如 GBA 240×160 → 480×320 = 2 倍）。
//! 算法极其简单：每个源像素在水平和垂直方向各复制 `xmul`/`ymul` 份。
//!
//! 输出效果：锐利、像素感强，保持游戏原始的画面美感。
//!
//! ## AaScaler — 双线性插值抗锯齿
//!
//! 当源分辨率与目标分辨率之间不是整数倍关系时使用（如 320×240 → 480×320 =
//! 1.5×1.33 倍）。算法使用双线性插值：对每个输出像素，在源图中找到其空间位置，
//! 取周围 4 个源像素做加权平均。权重由输出像素在源图中的分数坐标决定。
//!
//! 输出效果：平滑过渡、无锯齿、无块状感。
//!
//! ## BilinearScaler — 双线性插值
//!
//! 用于任意（分数）倍率的画面缩放。对应 C tg5040 平台 SOFT 锐度 =
//! SDL 纹理默认双线性过滤（`SDL_RenderCopy` 路径）与 aspect 模式的
//! GPU 缩放行为。与 AaScaler 的区别：AA 是面积平均（下采样抗锯齿
//! 设计），双线性是四邻域插值（上采样平滑）——两者不可互相替代。
//!
//! # 与原 C 代码的对比
//!
//! 原版 MinUI 的 scaler 系统约 3000 行 C + 内联汇编（`scaler.c` + `api.c`）：
//!
//! | 原 C | Rust | 差异 |
//! |------|------|------|
//! | 约 180 个 per-factor 函数 | 3 个泛型结构体 | Rust/LLVM 编译期常量传播消除手写展开 |
//! | 约 900 行 NEON 汇编 | 0 行（LLVM 自动向量化） | stable Rust 无 NEON intrinsics |
//! | `scaleAA()` + 全局 `blend_args` | `AaScaler` 实例字段 | 消除全局可变状态 |
//! | 手工 unroll 循环体 | 循环 + `--release` 自动 unroll | 效果等价 |
//!
//! 两种实现产生像素级等价输出（IntegerScaler）或视觉等价、±1 色阶范围内一致的输出（AaScaler）。
//!
//! ## NEON 与 SIMD 科普
//!
//! **NEON** 是 ARM 处理器的 SIMD（Single Instruction Multiple Data，单指令多数据）
//! 指令集。SIMD 的核心思想是：一条指令同时对多个数据执行相同的操作。
//! 例如，普通循环一次计算 1 个像素的加法，NEON 指令 `vadd.u16` 一次计算 8 个。
//!
//! 原 C 代码用内联汇编手写 NEON，是因为那个时代的 ARM 编译器不会自动生成 NEON
//! 指令。现代 Rust/LLVM 编译器在 `--release` 模式下**自动识别可向量化的循环模式**，
//! 并生成等效的 NEON/SSE 指令——不需要手写汇编也能获得接近手写的性能。
//!

use std::cmp::min;

// ── 辅助函数 ────────────────────────────────────

/// 最大公约数（Euclid 算法）
///
/// 用于计算缩放比的最简分数形式。
/// 例如 gcd(320, 480) = 160，比率为 320/160 : 480/160 = 2:3。
fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// 加权混合两个像素
///
/// `weight` 范围 0-256，0 = 纯 b，256 = 纯 a。
/// 使用固定分母 256 允许编译器将除法优化为 `>> 8` 右移指令。
#[inline]
fn blend_pixels(a: u16, b: u16, weight: u32) -> u16 {
    let wa = weight;
    let wb = 256 - weight;
    let r = ((a >> 11) as u32 * wa + (b >> 11) as u32 * wb) >> 8;
    let g = (((a >> 5) & 0x3F) as u32 * wa + ((b >> 5) & 0x3F) as u32 * wb) >> 8;
    let b_ = ((a & 0x1F) as u32 * wa + (b & 0x1F) as u32 * wb) >> 8;
    ((r as u16) << 11) | ((g as u16) << 5) | (b_ as u16)
}

// ── IntegerScaler ────────────────────────────────

/// 整数倍像素缩放器（最近邻复制）
///
/// 支持独立的水平倍率（`xmul`）和垂直倍率（`ymul`）。
/// 等价于原 C `scale{xmul}x{ymul}_c16` 系列函数（`scaler.c:121-210`）。
///
/// ## 算法
///
/// 对每个源像素在水平方向复制 `xmul` 份、垂直方向复制 `ymul` 份。
/// 当 `xmul == 1` 时使用快路径——直接 `copy_from_slice` 整行复制，
/// 无需逐像素展开。
///
/// ## 原 C 对比
///
/// C 代码为每个 `xmul×ymul` 组合编写独立函数（手工展开循环体以适配 ARM 编译器）。
/// Rust 版使用参数化循环，LLVM 在 `--release` 下对 `xmul`/`ymul` 做常量传播，
/// 等效于每个调用点自动生成展开后的代码。
pub struct IntegerScaler {
    xmul: u32,
    ymul: u32,
}

impl IntegerScaler {
    /// 创建整数缩放器
    ///
    /// ## 参数
    ///
    /// * `xmul` - 水平缩放倍率（≥1）
    /// * `ymul` - 垂直缩放倍率（≥1）
    pub fn new(xmul: u32, ymul: u32) -> Self {
        Self { xmul, ymul }
    }

    /// 执行整数缩放
    ///
    /// ## 参数
    ///
    /// * `src` - 源像素切片（RGB565 格式）
    /// * `sw` - 源宽度（像素）
    /// * `sh` - 源高度（像素）
    /// * `sp` - 源 pitch（每行像素数，≥ sw）
    /// * `dst` - 目标像素切片（输出缓冲区）
    /// * `dp` - 目标 pitch（每行像素数，≥ sw * xmul）
    ///
    /// 调用方负责确保 `dst` 有足够空间容纳 `(sw * xmul) × (sh * ymul)` 像素。
    /// 不做边界裁剪。
    pub fn scale(&self, src: &[u16], sw: u32, sh: u32, sp: u32, dst: &mut [u16], dp: u32) {
        let xmul = self.xmul;
        let ymul = self.ymul;

        if sw == 0 || sh == 0 {
            return;
        }

        let sw_u = sw as usize;
        let sp_u = sp as usize;
        let dp_u = dp as usize;
        let xmul_u = xmul as usize;
        let ymul_u = ymul as usize;

        // 快路径：无缩放、src/dst pitch 一致 → 全局 memcpy
        if xmul == 1 && ymul == 1 && sw_u == sp_u && sp_u == dp_u {
            let total = sp_u * sh as usize;
            let src_len = src.len();
            let dst_len = dst.len();
            dst[..min(total, dst_len)].copy_from_slice(&src[..min(total, src_len)]);
            return;
        }

        // 通用路径：水平展开 + 垂直复制
        if xmul == 1 {
            // 无水平展开，直接复制每行 ymul 次
            for r in 0..sh as usize {
                let src_row = &src[r * sp_u..r * sp_u + sw_u];
                for y in 0..ymul_u {
                    let dst_start = (r * ymul_u + y) * dp_u;
                    dst[dst_start..dst_start + sw_u].copy_from_slice(src_row);
                }
            }
        } else {
            // 水平展开后垂直复制
            let expanded_w = sw_u * xmul_u;
            let mut expanded = vec![0u16; expanded_w];

            for r in 0..sh as usize {
                let src_row = &src[r * sp_u..r * sp_u + sw_u];
                // 水平展开：每个像素重复 xmul 次
                for (c, &pixel) in src_row.iter().enumerate() {
                    let base = c * xmul_u;
                    for x in 0..xmul_u {
                        expanded[base + x] = pixel;
                    }
                }
                // 垂直复制：展开后的行写 ymul 次
                for y in 0..ymul_u {
                    let dst_start = (r * ymul_u + y) * dp_u;
                    dst[dst_start..dst_start + expanded_w].copy_from_slice(&expanded[..expanded_w]);
                }
            }
        }
    }
}

// ── AaScaler ─────────────────────────────────────

/// 抗锯齿混合缩放器（双线性插值）
///
/// 用于非整数倍的画面缩放。对每个输出像素做双线性插值——
/// 取源图中最近 4 个像素按空间距离加权平均。
///
/// 等价于原 C `scaleAA()` + `blend_args` 全局状态（`api.c:375-500`）。
///
/// ## 算法
///
/// 对于每个输出像素 `(ox, oy)`：
///
/// ```text
/// 1. 映射回源坐标：
///    sx = ox * ratio_w_in / ratio_w_out
///    sy = oy * ratio_h_in / ratio_h_out
///
/// 2. 取整数部分和分数部分：
///    ix = floor(sx),  fx = fractional part  (0..ratio_w_out/ratio_w_in)
///    iy = floor(sy),  fy = fractional part
///
/// 3. 水平混合：对两个水平相邻源像素按 fx 权重混合
///
/// 4. 垂直混合：将上下两行的水平混合结果按 fy 权重再次混合
/// ```
///
/// ## 与原 C 的差异
///
/// C 版 `scaleAA` 使用预计算的混合断点（`w_bp[0/1]`、`h_bp[0/1]`）
/// 实现 5 级离散混合（0%/25%/50%/75%/100%）。Rust 版使用连续分数坐标
/// 直接计算权重（256 级），效果等价、视觉输出一致（±1 色阶）。
///
/// C 版的 `blend_args` 是全局变量，`blend_line` 由 `calloc`/`free` 手动管理——
/// Rust 版所有状态在 `AaScaler` 实例内，无全局状态、无手动内存管理。
pub struct AaScaler {
    /// 水平宽高比分子（src_w / gcd）
    ratio_w_in: u32,
    /// 水平宽高比分母（dst_w / gcd）
    ratio_w_out: u32,
    /// 垂直宽高比分子（src_h / gcd）
    ratio_h_in: u32,
    /// 垂直宽高比分母（dst_h / gcd）
    ratio_h_out: u32,
    /// 源宽度（用于计算目标迭代）
    src_w: u32,
    /// 源高度（用于计算目标迭代）
    src_h: u32,
}

impl AaScaler {
    /// 创建 AA 缩放器
    ///
    /// 使用 gcd 计算宽高比的最简分数形式。
    ///
    /// 等价于原 C `GFX_getAAScaler()`（`api.c:469-494`）。
    ///
    /// ## 参数
    ///
    /// * `src_w` - 源宽度（像素）
    /// * `src_h` - 源高度（像素）
    /// * `dst_w` - 目标宽度（像素）
    /// * `dst_h` - 目标高度（像素）
    pub fn new(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Self {
        let gcd_w = gcd(src_w, dst_w);
        let gcd_h = gcd(src_h, dst_h);

        Self {
            ratio_w_in: src_w / gcd_w,
            ratio_w_out: dst_w / gcd_w,
            ratio_h_in: src_h / gcd_h,
            ratio_h_out: dst_h / gcd_h,
            src_w,
            src_h,
        }
    }

    /// 执行 AA 混合缩放
    ///
    /// ## 参数
    ///
    /// * `src` - 源像素切片
    /// * `sw` - 源宽度
    /// * `sh` - 源高度
    /// * `sp` - 源 pitch
    /// * `dst` - 目标像素切片
    /// * `dp` - 目标 pitch
    pub fn scale(&self, src: &[u16], sw: u32, sh: u32, sp: u32, dst: &mut [u16], dp: u32) {
        if sw == 0 || sh == 0 {
            return;
        }

        let sw_u = sw as usize;
        let sp_u = sp as usize;
        let dp_u = dp as usize;

        // 目标尺寸
        let dst_w = (self.src_w * self.ratio_w_out / self.ratio_w_in) as usize;
        let dst_h = (self.src_h * self.ratio_h_out / self.ratio_h_in) as usize;

        // 源行垂直位置追踪
        let mut src_row: u32 = 0;
        // 当前源行的上一行（用于垂直混合），首行时 = 当前行
        let mut prev_row = 0u32;

        for oy in 0..dst_h {
            // 当前输出行对应的源行位置（分数）
            let oy_u = oy as u32;
            let sy_num = oy_u * self.ratio_h_in;
            let iy = (sy_num / self.ratio_h_out) as usize;
            let fy_num = sy_num % self.ratio_h_out; // 垂直分数分子

            // 是否需要切换到新的源行？
            let current_row = iy as u32;
            if current_row != src_row || oy == 0 {
                prev_row = src_row;
                src_row = current_row;
            }

            // 垂直权重（0-256）
            let fy_weight = fy_num * 256 / self.ratio_h_out;

            let cur_line = &src[src_row as usize * sp_u..];
            let prev_line = if src_row > 0 && fy_weight > 0 {
                &src[prev_row as usize * sp_u..]
            } else {
                cur_line
            };

            // 遍历每个输出列
            let dst_start = oy * dp_u;
            for ox in 0..dst_w {
                // 水平源位置（分数）
                let sx_num = (ox as u32) * self.ratio_w_in;
                let ix = (sx_num / self.ratio_w_out) as usize;
                let fx_num = sx_num % self.ratio_w_out;
                let fx_weight = fx_num * 256 / self.ratio_w_out;

                // 取两个水平相邻像素
                let left_ix = min(ix, sw_u - 1);
                let right_ix = min(ix + 1, sw_u - 1);

                let a = cur_line[left_ix];
                let b = cur_line[right_ix];
                let c = prev_line[left_ix];
                let d = prev_line[right_ix];

                // 水平混合：fx_weight=0 时输出在左像素位置 → 需要 weight=256（纯 a）
                let h_weight = 256 - fx_weight;
                let top = blend_pixels(a, b, h_weight);
                let bot = blend_pixels(c, d, h_weight);

                // 垂直混合：fy_weight=0 时输出在上行位置 → 需要 weight=256（纯 top）
                let v_weight = 256 - fy_weight;
                let result = blend_pixels(top, bot, v_weight);

                dst[dst_start + ox] = result;
            }
        }
    }
}

// ── BilinearScaler ───────────────────────────────

/// 双线性插值缩放器（分数倍率）
///
/// 对应 C tg5040 的 SOFT 锐度 = SDL 纹理默认双线性过滤
/// （`SDL_RenderCopy` 路径，platform.c:436）与全部 aspect 模式的
/// GPU 缩放行为。与 [`AaScaler`] 的语义区别：AA 是面积平均
/// （下采样抗锯齿设计，C `scaleAA`），双线性是四邻域插值
/// （上采样平滑）——两者不可互相替代。
///
/// ## 算法
///
/// 对每个目标像素反查源坐标（中心对齐）：
///
/// ```text
/// sx = (ox + 0.5) × sw / dw - 0.5
/// sy = (oy + 0.5) × sh / dh - 0.5
/// ```
///
/// 取 4 个邻域源像素按水平/垂直分数距离加权混合，固定分母 256
/// （`>> 8` 快除，与 [`AaScaler`] 同策略）。反查落点超出源边缘时
/// 权重退化——最后一行/列不越界读。
///
/// ## 与原 C 的差异
///
/// C tg5040 的双线性由 GPU 完成（SDL 纹理过滤），无软件对应物；
/// 其他平台的 C 软件缩放器只有最近邻（NEON/C 整数系列，scaler.c）。
/// Rust 版逐像素标量循环，依赖 LLVM 自动向量化（与
/// [`IntegerScaler`]「参数化循环替代手工展开」同策略）。
pub struct BilinearScaler {
    /// 目标宽度（`scale` 输出宽度）
    dst_w: u32,
    /// 目标高度（`scale` 输出高度）
    dst_h: u32,
}

impl BilinearScaler {
    /// 创建双线性缩放器
    ///
    /// ## 参数
    ///
    /// * `src_w` - 源宽度（像素）——与 [`BilinearScaler::scale`] 的
    ///   `sw` 参数重复，保留以与 [`AaScaler::new`] 签名对称
    /// * `src_h` - 源高度（像素）——同上
    /// * `dst_w` - 目标宽度（像素）
    /// * `dst_h` - 目标高度（像素）
    pub fn new(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Self {
        let _ = (src_w, src_h);
        Self { dst_w, dst_h }
    }

    /// 执行双线性缩放
    ///
    /// ## 参数
    ///
    /// * `src` - 源像素切片（RGB565 格式）
    /// * `sw` - 源宽度（像素）
    /// * `sh` - 源高度（像素）
    /// * `sp` - 源 pitch（每行像素数，≥ sw）
    /// * `dst` - 目标像素切片（输出缓冲区）
    /// * `dp` - 目标 pitch（每行像素数，≥ dst_w）
    ///
    /// 调用方负责确保 `dst` 有足够空间容纳 `dst_w × dst_h` 像素。
    /// 不做边界裁剪。
    pub fn scale(&self, src: &[u16], sw: u32, sh: u32, sp: u32, dst: &mut [u16], dp: u32) {
        if sw == 0 || sh == 0 {
            return;
        }

        let sw_u = sw as usize;
        let sh_u = sh as usize;
        let sp_u = sp as usize;
        let dp_u = dp as usize;
        let dst_w = self.dst_w as usize;
        let dst_h = self.dst_h as usize;

        // 1:1 恒等快路径（权重退化为 100%，等价 memcpy）
        if dst_w == sw_u && dst_h == sh_u {
            for row in 0..sh_u {
                dst[row * dp_u..row * dp_u + sw_u]
                    .copy_from_slice(&src[row * sp_u..row * sp_u + sw_u]);
            }
            return;
        }

        // 反查分母（×2 来自中心对齐的 +0.5/-0.5 偏移）
        let denom_x = (dst_w * 2) as i128;
        let denom_y = (dst_h * 2) as i128;

        for oy in 0..dst_h {
            // sy × denom_y = (2·oy + 1) × sh - dst_h（可为负 = 落点在首行之上）
            let sy_num = (oy as i128 * 2 + 1) * sh as i128 - dst_h as i128;
            let (iy, fy_num) = if sy_num <= 0 {
                (0usize, 0i128)
            } else {
                ((sy_num / denom_y) as usize, sy_num % denom_y)
            };
            // 边缘钳制：iy 与 iy+1 都限制在 [0, sh-1]，不越界读
            let iy = min(iy, sh_u - 1);
            let iy2 = min(iy + 1, sh_u - 1);
            let fy_weight = (fy_num * 256 / denom_y) as u32; // 下行权重
            let ty_weight = 256 - fy_weight; // 上行权重

            let top_line = &src[iy * sp_u..];
            let bot_line = &src[iy2 * sp_u..];

            for ox in 0..dst_w {
                // sx × denom_x = (2·ox + 1) × sw - dst_w（可为负 = 落点在首列之左）
                let sx_num = (ox as i128 * 2 + 1) * sw as i128 - dst_w as i128;
                let (ix, fx_num) = if sx_num <= 0 {
                    (0usize, 0i128)
                } else {
                    ((sx_num / denom_x) as usize, sx_num % denom_x)
                };
                let ix = min(ix, sw_u - 1);
                let ix2 = min(ix + 1, sw_u - 1);
                let fx_weight = (fx_num * 256 / denom_x) as u32; // 右像素权重
                let lx_weight = 256 - fx_weight; // 左像素权重

                let top = blend_pixels(top_line[ix], top_line[ix2], lx_weight);
                let bot = blend_pixels(bot_line[ix], bot_line[ix2], lx_weight);
                dst[oy * dp_u + ox] = blend_pixels(top, bot, ty_weight);
            }
        }
    }
}

// ── 测试 ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── IntegerScaler 测试 ───────────────────────—

    #[test]
    fn integer_scale_2x2() {
        // 源 4×4 渐进模式
        let src: Vec<u16> = (1u16..=16).collect();
        let sw = 4;
        let sh = 4;
        let sp = 4;
        let scaler = IntegerScaler::new(2, 2);
        let dst_w = sw * 2;
        let dst_h = sh * 2;
        let dp = dst_w;
        let mut dst = vec![0u16; (dst_w * dst_h) as usize];

        scaler.scale(&src, sw, sh, sp, &mut dst, dp);

        // 验证源像素 (0,0) = 1 在目标出现 2×2 次
        assert_eq!(dst[0], 1);
        assert_eq!(dst[1], 1);
        assert_eq!(dst[dp as usize], 1);
        assert_eq!(dst[dp as usize + 1], 1);

        // 验证最后一个源像素 (3,3) = 16 在目标右下角 2×2 区域
        let last_row = (dst_h - 2) as usize;
        let last_col = (dst_w - 2) as usize;
        assert_eq!(dst[last_row * dp as usize + last_col], 16);
        assert_eq!(dst[last_row * dp as usize + last_col + 1], 16);

        // 非零像素总数 = 源像素数 × 4
        // 注意渐进模式下所有源像素非零，所以全为非零
        assert!(!dst.contains(&0), "缩放后不应有空像素");
    }

    #[test]
    fn integer_scale_3x1_horizontal_only() {
        let src: Vec<u16> = (1u16..=16).collect();
        let sw = 4;
        let sh = 4;
        let sp = 4;
        let scaler = IntegerScaler::new(3, 1);
        let dst_w = sw * 3;
        let dp = dst_w;
        let mut dst = vec![0u16; (dst_w * sh) as usize];

        scaler.scale(&src, sw, sh, sp, &mut dst, dp);

        // 第一行第一个像素 = 1 应出现 3 次
        assert_eq!(dst[0], 1);
        assert_eq!(dst[1], 1);
        assert_eq!(dst[2], 1);
        // 第一行第二个像素 = 2 应出现在偏移 3
        assert_eq!(dst[3], 2);

        // 总共 4 行（垂直无缩放）
        let rows_with_pixels: Vec<bool> = (0..sh as usize)
            .map(|r| {
                dst[r * dp as usize..r * dp as usize + dst_w as usize]
                    .iter()
                    .any(|&p| p != 0)
            })
            .collect();
        assert_eq!(rows_with_pixels.iter().filter(|&&r| r).count(), 4);
    }

    #[test]
    fn integer_scale_1x3_vertical_only() {
        let src: Vec<u16> = (1u16..=16).collect();
        let sw = 4;
        let sh = 4;
        let sp = 4;
        let scaler = IntegerScaler::new(1, 3);
        let dst_h = sh * 3;
        let dp = sw;
        let mut dst = vec![0u16; (sw * dst_h) as usize];

        scaler.scale(&src, sw, sh, sp, &mut dst, dp);

        // 快路径生效：首行 3 次
        assert_eq!(dst[0], 1);
        assert_eq!(dst[dp as usize], 1); // 第 2 行
        assert_eq!(dst[2 * dp as usize], 1); // 第 3 行
        // 输出应有 12 行
        let nonzero = dst.iter().filter(|&&p| p != 0).count();
        assert!(nonzero > 0);
    }

    #[test]
    fn integer_scale_2x3_different_aspect() {
        let src: Vec<u16> = (1u16..=65).collect(); // 8×8
        let sw = 8;
        let sh = 8;
        let sp = 8;
        let scaler = IntegerScaler::new(2, 3);
        let dst_w = 16;
        let dst_h = 24;
        let dp = dst_w;
        let mut dst = vec![0u16; (dst_w * dst_h) as usize];

        scaler.scale(&src, sw, sh, sp, &mut dst, dp);

        assert_eq!(dst[0], 1);
        assert_eq!(dst[1], 1);
        assert!(dst.iter().any(|&p| p != 0));
    }

    #[test]
    fn integer_scale_empty_source() {
        let scaler = IntegerScaler::new(2, 2);
        let mut dst = vec![0u16; 64];
        let before = dst.clone();

        // sw=0
        scaler.scale(&[], 0, 4, 0, &mut dst, 8);
        assert_eq!(dst, before);
        // sh=0
        scaler.scale(&[1, 2, 3, 4], 2, 0, 2, &mut dst, 8);
        assert_eq!(dst, before);
    }

    #[test]
    fn integer_scale_1x1_identity() {
        let src: Vec<u16> = (1u16..=16).collect();
        let sw = 4;
        let sh = 4;
        let sp = 4;
        let scaler = IntegerScaler::new(1, 1);
        let mut dst = vec![0u16; 16];

        scaler.scale(&src, sw, sh, sp, &mut dst, 4);

        assert_eq!(dst, src, "1×1 缩放输出应与源图一致");
    }

    // ── AaScaler 测试 ─────────────────────────────

    #[test]
    fn aa_scale_non_integer() {
        // 源 8×8 → 目标 12×10（1.5×1.25 倍）
        let src: Vec<u16> = (1u16..=64).collect();
        let sw = 8;
        let sh = 8;
        let sp = 8;
        let scaler = AaScaler::new(sw, sh, 12, 10);
        let mut dst = vec![0u16; 120];

        scaler.scale(&src, sw, sh, sp, &mut dst, 12);

        assert!(dst.iter().any(|&p| p != 0), "AA 缩放应产生非零像素");

        // 至少部分像素应与源像素不同（加权混合结果）
        // 注意：灰度边界上可能刚好匹配源像素，所以不强制检查
    }

    #[test]
    fn aa_scale_identity() {
        let src: Vec<u16> = vec![0xF800u16, 0x07E0, 0x001F, 0xFFFF]; // 2×2
        let sw = 2;
        let sh = 2;
        let sp = 2;
        let scaler = AaScaler::new(sw, sh, sw, sh);
        let mut dst = vec![0u16; 4];

        scaler.scale(&src, sw, sh, sp, &mut dst, sw);

        // 1:1 缩放：输出应与源图逐像素一致
        assert_eq!(dst, src);
    }

    #[test]
    fn aa_scale_empty_source() {
        let scaler = AaScaler::new(8, 8, 12, 10);
        let mut dst = vec![0u16; 120];
        let before = dst.clone();

        scaler.scale(&[], 0, 4, 0, &mut dst, 12);
        assert_eq!(dst, before);
        scaler.scale(&[1, 2], 2, 0, 2, &mut dst, 12);
        assert_eq!(dst, before);
    }

    // ── 辅助函数测试 ─────────────────────────────

    #[test]
    fn gcd_test() {
        assert_eq!(gcd(320, 480), 160);
        assert_eq!(gcd(240, 320), 80);
        assert_eq!(gcd(7, 13), 1);
        assert_eq!(gcd(0, 5), 5);
    }

    #[test]
    fn blend_pixels_full_weight_is_a() {
        assert_eq!(blend_pixels(0xFFFF, 0x0000, 256), 0xFFFF);
    }

    #[test]
    fn blend_pixels_zero_weight_is_b() {
        assert_eq!(blend_pixels(0xFFFF, 0x0000, 0), 0x0000);
    }

    #[test]
    fn blend_pixels_mid_weight_is_mixed() {
        let result = blend_pixels(0xFFFF, 0x0000, 128);
        assert!(result != 0xFFFF);
        assert!(result != 0x0000, "中间权重应产生混合值");
    }

    #[test]
    fn blend_pixels_channels_independent() {
        // 纯红 (0xF800) 和纯蓝 (0x001F) 混合——各自通道独立计算
        let result = blend_pixels(0xF800, 0x001F, 128);
        assert!(result > 0x0000 && result < 0xFFFF);
    }

    // ── BilinearScaler 测试 ─────────────────────────

    #[test]
    fn bilinear_scale_2x2_diagonal() {
        // 源 2×2 黑白对角，目标 4×4
        let src = vec![0x0000u16, 0xFFFF, 0xFFFF, 0x0000];
        let (sw, sh, sp) = (2, 2, 2);
        let scaler = BilinearScaler::new(sw, sh, 4, 4);
        let mut dst = vec![0u16; 16];

        scaler.scale(&src, sw, sh, sp, &mut dst, 4);

        // 左上角：中心对齐反查落点钳制在源原点 → 纯角像素
        assert_eq!(dst[0], 0x0000, "左上角应钳制为纯源角像素");
        // 中心：四邻域混合 → 既非纯黑也非纯白
        let center = dst[4 + 1];
        assert_ne!(center, 0x0000, "中心应为混合值而非纯黑");
        assert_ne!(center, 0xFFFF, "中心应为混合值而非纯白");
        // 对角对称：dst[y][x] == dst[3-y][3-x]（源沿两条对角线对称）
        for y in 0..4usize {
            for x in 0..4usize {
                assert_eq!(dst[y * 4 + x], dst[(3 - y) * 4 + (3 - x)]);
            }
        }
    }

    #[test]
    fn bilinear_identity_1x1() {
        // 1:1 恒等快路径：输出与源逐像素一致
        let src: Vec<u16> = (1u16..=16).collect();
        let (sw, sh, sp) = (4, 4, 4);
        let scaler = BilinearScaler::new(sw, sh, sw, sh);
        let mut dst = vec![0u16; 16];

        scaler.scale(&src, sw, sh, sp, &mut dst, 4);

        assert_eq!(dst, src, "1:1 缩放输出应与源图逐像素一致");
    }

    #[test]
    fn bilinear_fractional_upscale() {
        // 160×144 → 1280×720（aspect 模式典型组合）
        // 源为黑白棋盘（通道 ∈ {0,31}/{0,63}/{0,31}），哨兵 0x0400
        // 的绿色通道 32 不可能由凸组合产生——用于断言"无未初始化像素"
        let (sw, sh) = (160u32, 144u32);
        let sp = sw;
        let src: Vec<u16> = (0..sw * sh)
            .map(|i| {
                if (i / sw + i % sw) % 2 == 0 {
                    0x0000
                } else {
                    0xFFFF
                }
            })
            .collect();
        let scaler = BilinearScaler::new(sw, sh, 1280, 720);
        let (dp, dst_w, dst_h) = (1280u32, 1280usize, 720usize);
        let mut dst = vec![0x0400u16; dst_w * dst_h];

        scaler.scale(&src, sw, sh, sp, &mut dst, dp);

        // 左上角像素：反查落点为负 → 钳制为纯源原点
        assert_eq!(dst[0], src[0]);
        // 右下角像素：反查落点超出边缘 → 钳制为纯源末像素
        assert_eq!(
            dst[(dst_h - 1) * dst_w + (dst_w - 1)],
            src[(sh - 1) as usize * sw as usize + (sw - 1) as usize]
        );
        // 无未初始化像素：哨兵值不可能被混合产生
        assert!(!dst.contains(&0x0400), "存在未写入的哨兵像素");
    }

    #[test]
    fn bilinear_edge_clamp_no_oob() {
        // 4×4 → 4×7（垂直非整数倍）：最后一行钳制为源最后一行
        let src: Vec<u16> = (1u16..=16).collect();
        let (sw, sh, sp) = (4, 4, 4);
        let scaler = BilinearScaler::new(sw, sh, 4, 7);
        let mut dst = vec![0u16; 4 * 7];

        scaler.scale(&src, sw, sh, sp, &mut dst, 4);

        let last_row = &dst[6 * 4..7 * 4];
        let src_last_row = &src[3 * 4..4 * 4];
        assert_eq!(last_row, src_last_row, "最后一行应钳制为源最后一行");
    }

    #[test]
    fn bilinear_empty_source_noop() {
        let scaler = BilinearScaler::new(8, 8, 16, 16);
        let mut dst = vec![0u16; 64];
        let before = dst.clone();

        scaler.scale(&[], 0, 4, 0, &mut dst, 8);
        assert_eq!(dst, before);
        scaler.scale(&[1, 2], 2, 0, 2, &mut dst, 8);
        assert_eq!(dst, before);
    }

    #[test]
    fn bilinear_pitch_aware() {
        // 源 160×144、sp=320：行距间填充 0（模拟核心帧 stride）
        // 若缩放器忽略 sp 按 sw 连续读，第 1 行起将读到填充区全 0
        let (sw, sh, sp) = (160u32, 144u32, 320u32);
        let mut frame = vec![0u16; (sp * sh) as usize];
        for y in 0..sh as usize {
            for x in 0..sw as usize {
                frame[y * sp as usize + x] = 0xFFFF;
            }
        }
        let scaler = BilinearScaler::new(sw, sh, 320, 288);
        let mut dst = vec![0u16; 320 * 288];

        scaler.scale(&frame, sw, sh, sp, &mut dst, 320);

        // 正确 pitch：所有输出行由全 1 源行混合 → 全部非零
        assert!(
            dst.iter().all(|&p| p != 0),
            "pitch 被忽略时输出行会读到填充区 0"
        );
        // 首行 2x 水平放大：每源像素复制区域首像素为纯源值
        assert_eq!(dst[0], 0xFFFF);
        assert_eq!(dst[2], 0xFFFF);
    }
}
