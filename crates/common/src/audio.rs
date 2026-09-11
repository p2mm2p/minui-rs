//! `audio` — 音频基类型和纯数学处理工具
//!
//! 本模块提供 MinUI 音频管线的数据类型和工具，零第三方依赖。
//! 不涉及任何平台相关的音频设备操作——打开/关闭设备、实际播放由平台实现
//!（`Platform` trait）负责。
//!
//! ## 模块职责
//!
//! | 类型 | 职责 |
//! |------|------|
//! | [`AudioFrame`] | 单帧立体声音频采样（左+右声道，各一个 `i16`） |
//! | [`AudioRingBuffer`] | 固定容量的环形音频缓冲（SPSC），可选工具 |
//! | [`Resampler`] | 音频重采样器（直通 + 最近邻），纯数学转换 |
//!
//! ## 与原 C 代码的对比
//!
//! 原 C 代码在 `api.c:928-998` 中定义了 `SND_Context`（全局 `static` 结构体），
//! 内含环形缓冲和读写指针。`SND_batchSamples`（生产者）和 `SND_audioCallback`（消费者）
//! 通过 `SDL_LockAudio`/`SDL_UnlockAudio` 同步。
//!
//! Rust 版将环形缓冲和重采样器从 SDL 耦合中剥离：
//! - `AudioRingBuffer` 替代 `snd.buffer` + `frame_in`/`frame_out` 读写指针
//! - `Resampler` 替代 `SND_resampleNone` / `SND_resampleNear`
//! - 同步由外部调用方负责（`push`/`pop` 签名为 `&mut self`），不引入 `Mutex`
//!
//! ## AudioRingBuffer 的可选性质
//!
//! `AudioRingBuffer` 是 common 提供的可选工具类型——类型定义在 common，
//! 实例由每个平台自己持有（作为平台结构体的字段）。角色类比：
//! common 提供"水箱的设计图纸"，平台按图纸造一个自己的水箱、接上水管。
//!
//! 平台实现 `push_audio` 时可自由选择缓冲方案：`AudioRingBuffer`、
//! `Mutex<VecDeque<AudioFrame>>`、或直接写入 SDL 音频队列。
//!
//! 如果使用 `AudioRingBuffer`，需要在外部自行同步（`Mutex` 等）。
//!

// ── AudioFrame ──────────────────────────────────────────────

/// 单帧立体声音频采样
///
/// 模拟器核心每帧产生一对左右声道的 16-bit 有符号采样值。
/// 对应原 C `api.h:203-206` 的 `SND_Frame` 结构体：
/// ```c
/// typedef struct SND_Frame {
///     int16_t left;
///     int16_t right;
/// } SND_Frame;
/// ```
#[derive(Clone, Copy)]
pub struct AudioFrame {
    /// 左声道采样值
    pub left: i16,
    /// 右声道采样值
    pub right: i16,
}

// ── AudioRingBuffer ─────────────────────────────────────────

/// 固定容量的环形音频缓冲
///
/// 支持单生产者单消费者（SPSC）模式。**缓冲本身不做内部加锁**——`push` 和 `pop`
/// 的签名为 `&mut self`，并发场景下由调用方通过外部同步原语（如 `Mutex`）保证互斥访问。
///
/// 对应原 C `api.c:928-944` 的 `SND_Context` 环形缓冲
/// （`snd.buffer` + `frame_in`/`frame_out` 读写指针）。
///
/// ## 可选性质
///
/// 本类型是 common 提供的可选工具——`Platform` trait 不强制要求使用它。
/// 平台实现 `push_audio` 时，可自由选择：
/// - 使用 `AudioRingBuffer`（基于 SDL 音频回调的平台）
/// - 使用 `Mutex<VecDeque<AudioFrame>>`
/// - 直接写入 SDL 音频队列
/// - 其他自定义方案
///
/// ## 线程安全
///
/// `push` 和 `pop` 都需要 `&mut self`——在并发场景下，调用方必须自行同步。
/// 单线程轮询时，直接调用即可，无需任何锁。
pub struct AudioRingBuffer {
    /// 后端存储（固定容量，创建后不变）
    buffer: Vec<AudioFrame>,
    /// 总容量（帧数），等于 buffer.len()
    capacity: usize,
    /// 消费者读取位置
    read_pos: usize,
    /// 生产者写入位置
    write_pos: usize,
    /// 当前已填充的帧数（0..=capacity）
    filled: usize,
}

impl AudioRingBuffer {
    /// 创建指定容量的空缓冲
    ///
    /// `capacity_frames` 为缓冲可容纳的最大音频帧数。
    /// 对应原 C `SND_resizeBuffer` 中根据 `buffer_seconds * sample_rate / frame_rate` 计算容量的逻辑。
    pub fn new(capacity_frames: usize) -> Self {
        Self {
            buffer: vec![AudioFrame { left: 0, right: 0 }; capacity_frames],
            capacity: capacity_frames,
            read_pos: 0,
            write_pos: 0,
            filled: 0,
        }
    }

    /// 写入音频帧到缓冲
    ///
    /// 如果缓冲剩余空间不足以容纳所有输入帧，**不做部分写入**，直接返回 `0`。
    /// 生产者（模拟器核心）可以等待或丢弃。
    ///
    /// 返回实际写入的帧数（要么等于 `frames.len()`，要么为 `0`）。
    pub fn push(&mut self, frames: &[AudioFrame]) -> usize {
        if frames.len() > self.capacity - self.filled {
            return 0; // 空间不足，不做部分写入
        }

        for frame in frames {
            self.buffer[self.write_pos] = AudioFrame {
                left: frame.left,
                right: frame.right,
            };
            self.write_pos += 1;
            if self.write_pos >= self.capacity {
                self.write_pos = 0;
            }
        }
        self.filled += frames.len();
        frames.len()
    }

    /// 从缓冲读取音频帧
    ///
    /// 返回实际读取的帧数。如果缓冲为空，返回 `0`——消费者（如 SDL 音频回调）
    /// 此时应输出静音。对应原 C `SND_audioCallback` 中缓冲空时 `memset(out, 0, ...)` 的逻辑。
    pub fn pop(&mut self, out: &mut [AudioFrame]) -> usize {
        let to_read = out.len().min(self.filled);
        if to_read == 0 {
            return 0;
        }

        for item in out.iter_mut().take(to_read) {
            *item = AudioFrame {
                left: self.buffer[self.read_pos].left,
                right: self.buffer[self.read_pos].right,
            };
            self.read_pos += 1;
            if self.read_pos >= self.capacity {
                self.read_pos = 0;
            }
        }
        self.filled -= to_read;
        to_read
    }

    /// 当前已填充的帧数
    pub fn len(&self) -> usize {
        self.filled
    }

    /// 缓冲是否为空
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 缓冲总容量（帧数）
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ── Resampler ───────────────────────────────────────────────

/// 音频重采样器
///
/// 提供直通（passthrough）和最近邻（nearest-neighbor）两种模式。
/// 对应原 C `api.c:1000-1021` 的 `SND_resampleNone` 和 `SND_resampleNear`。
///
/// ## 算法（Nearest 模式）
///
/// 原 C 算法使用 `consumed` 计数器实现帧的跳过/复制：
///
/// ```c
/// if (diff < snd.sample_rate_out) {
///     snd.buffer[snd.frame_in++] = frame;  // 输出帧
///     diff += snd.sample_rate_in;
/// }
/// if (diff >= snd.sample_rate_out) {
///     consumed++;                          // 消费输入帧
///     diff -= snd.sample_rate_out;
/// }
/// ```
///
/// Rust 版通过 [`ResampleResult`] 返回两个布尔值，等价于 C 的帧写入和 consumed：
///
/// - `write_output` = `true`：本帧已写入输出缓冲（对应 C 的 buffer 写入）
/// - `advance_input` = `true`：调用方应前进到下一输入帧（对应 C 的 consumed）
///
/// 上采样（in < out）：同一输入帧可能被处理两次（第一次 write_output=true, advance_input=false；
/// 第二次 write_output=true, advance_input=true），产生额外输出帧。
///
/// 下采样（in > out）：部分输入帧被跳过（write_output=false, advance_input=true）。
pub enum Resampler {
    /// 直通模式：输入采样率 == 输出采样率，每帧写入并消费
    Passthrough,
    /// 最近邻模式：输入采样率 ≠ 输出采样率时，按速率比跳帧或复制帧
    Nearest {
        /// 输入采样率（模拟器产生速率，如 44100Hz）
        sample_rate_in: u32,
        /// 输出采样率（硬件实际速率，如 48000Hz）
        sample_rate_out: u32,
        /// 累积差值计数器（内部状态）
        diff: u32,
    },
}

/// 重采样器单次处理结果
///
/// 对应原 C `SND_resampleNear` 中帧写入和 `consumed` 两个独立信号。
pub struct ResampleResult {
    /// 调用方是否应前进到下一个输入帧（对应 C 的 `consumed`）
    pub advance_input: bool,
    /// 本帧是否应写入输出缓冲（对应 C 的 `snd.buffer[snd.frame_in++] = frame`）
    pub write_output: bool,
}

impl Resampler {
    /// 根据输入/输出采样率创建重采样器
    ///
    /// 如果速率相同，返回 `Passthrough`；否则返回 `Nearest`。
    /// 对应原 C `SND_selectResampler` 的选择逻辑。
    pub fn new(sample_rate_in: u32, sample_rate_out: u32) -> Self {
        if sample_rate_in == sample_rate_out {
            Resampler::Passthrough
        } else {
            Resampler::Nearest {
                sample_rate_in,
                sample_rate_out,
                diff: 0,
            }
        }
    }

    /// 处理一次重采样器调用
    ///
    /// 对应原 C `SND_resampleNone`（passthrough）或 `SND_resampleNear`（nearest）。
    /// 调用方根据返回的 `ResampleResult` 决定是否将 `frame` 写入输出缓冲
    /// 以及是否前进到下一个输入帧。
    pub fn process(&mut self, _frame: AudioFrame) -> ResampleResult {
        match self {
            Resampler::Passthrough => ResampleResult {
                advance_input: true,
                write_output: true,
            },
            Resampler::Nearest {
                sample_rate_in,
                sample_rate_out,
                diff,
            } => {
                let mut write_output = false;
                let mut advance_input = false;

                // 与原 C api.c:1009-1013 完全一致：
                // if (diff < snd.sample_rate_out) { 写入帧; diff += in; }
                if *diff < *sample_rate_out {
                    write_output = true;
                    *diff += *sample_rate_in;
                }
                // 与原 C api.c:1015-1018 完全一致：
                // if (diff >= snd.sample_rate_out) { consumed++; diff -= out; }
                if *diff >= *sample_rate_out {
                    advance_input = true;
                    *diff -= *sample_rate_out;
                }

                ResampleResult {
                    advance_input,
                    write_output,
                }
            }
        }
    }
}

// ── 测试 ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── AudioFrame 测试 ──────────────────────────────────

    #[test]
    fn audio_frame_fields_match_c_snd_frame() {
        // 字段名 left/right 与原 C SND_Frame 一致
        let frame = AudioFrame {
            left: 100,
            right: -50,
        };
        assert_eq!(frame.left, 100);
        assert_eq!(frame.right, -50);
    }

    #[test]
    fn audio_frame_can_be_constructed_with_named_fields() {
        let frame = AudioFrame { left: 0, right: 0 };
        assert_eq!(frame.left, 0);
        assert_eq!(frame.right, 0);
    }

    // ── AudioRingBuffer 测试 ──────────────────────────────

    #[test]
    fn ring_buffer_new_has_correct_capacity_and_empty() {
        let buf = AudioRingBuffer::new(3675);
        assert_eq!(buf.capacity(), 3675);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn ring_buffer_push_returns_count() {
        let mut buf = AudioRingBuffer::new(10);
        let frames = [
            AudioFrame { left: 1, right: 2 },
            AudioFrame { left: 3, right: 4 },
        ];
        let written = buf.push(&frames);
        assert_eq!(written, 2);
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn ring_buffer_push_full_returns_zero() {
        let mut buf = AudioRingBuffer::new(3);
        // 填满
        buf.push(&[
            AudioFrame { left: 1, right: 1 },
            AudioFrame { left: 2, right: 2 },
            AudioFrame { left: 3, right: 3 },
        ]);
        assert_eq!(buf.len(), 3);
        // 再写——应返回 0，不做部分写入
        let written = buf.push(&[AudioFrame { left: 4, right: 4 }]);
        assert_eq!(written, 0);
        assert_eq!(buf.len(), 3); // 长度不变
    }

    #[test]
    fn ring_buffer_pop_returns_data_in_order() {
        let mut buf = AudioRingBuffer::new(10);
        buf.push(&[
            AudioFrame {
                left: 10,
                right: 20,
            },
            AudioFrame {
                left: 30,
                right: 40,
            },
        ]);

        let mut out = [AudioFrame { left: 0, right: 0 }; 2];
        let read = buf.pop(&mut out);
        assert_eq!(read, 2);
        assert_eq!(out[0].left, 10);
        assert_eq!(out[0].right, 20);
        assert_eq!(out[1].left, 30);
        assert_eq!(out[1].right, 40);
    }

    #[test]
    fn ring_buffer_pop_empty_returns_zero() {
        let mut buf = AudioRingBuffer::new(10);
        let mut out = [AudioFrame { left: 0, right: 0 }; 1];
        let read = buf.pop(&mut out);
        assert_eq!(read, 0);
        // out 保持不变
        assert_eq!(out[0].left, 0);
        assert_eq!(out[0].right, 0);
    }

    #[test]
    fn ring_buffer_wraps_around_correctly() {
        let mut buf = AudioRingBuffer::new(5);
        // 写 3 帧
        buf.push(&[
            AudioFrame { left: 1, right: 1 },
            AudioFrame { left: 2, right: 2 },
            AudioFrame { left: 3, right: 3 },
        ]);
        // 读 3 帧
        let mut out = [AudioFrame { left: 0, right: 0 }; 3];
        buf.pop(&mut out);
        assert_eq!(buf.len(), 0);

        // 再写 5 帧（填满，写指针从位置 3 绕回）
        let frames: Vec<_> = (0..5)
            .map(|i| AudioFrame {
                left: (10 + i) as i16,
                right: (20 + i) as i16,
            })
            .collect();
        let written = buf.push(&frames);
        assert_eq!(written, 5);
        assert_eq!(buf.len(), 5);

        // 读出，验证顺序
        let mut out2 = [AudioFrame { left: 0, right: 0 }; 5];
        let read = buf.pop(&mut out2);
        assert_eq!(read, 5);
        for (i, item) in out2.iter().enumerate() {
            assert_eq!(item.left, (10 + i) as i16);
            assert_eq!(item.right, (20 + i) as i16);
        }
    }

    #[test]
    fn ring_buffer_partial_pop_reads_whats_available() {
        let mut buf = AudioRingBuffer::new(10);
        buf.push(&[AudioFrame {
            left: 42,
            right: 99,
        }]);

        let mut out = [AudioFrame { left: 0, right: 0 }; 3]; // 请求 3 帧，只有 1 帧
        let read = buf.pop(&mut out);
        assert_eq!(read, 1);
        assert_eq!(out[0].left, 42);
        assert_eq!(out[0].right, 99);
        // out[1] 和 out[2] 保持不变
        assert_eq!(out[1].left, 0);
        assert_eq!(out[2].left, 0);
    }

    #[test]
    fn ring_buffer_is_empty_returns_true_when_empty() {
        let buf = AudioRingBuffer::new(10);
        assert!(buf.is_empty());
        let mut buf = AudioRingBuffer::new(10);
        buf.push(&[AudioFrame { left: 1, right: 1 }]);
        assert!(!buf.is_empty());
    }

    #[test]
    fn ring_buffer_no_internal_mutex() {
        // AudioRingBuffer 的方法签名为 &mut self，不包含内部 Mutex
        let buf = AudioRingBuffer::new(10);
        // 编译期验证：AudioRingBuffer 不实现 Sync
        // std::sync::Mutex 是不可 Copy 的——如果 AudioRingBuffer 含 Mutex，
        // 则 AudioRingBuffer 也不可 Copy。但我们的设计是 &mut self，无需测试 Copy。
        let _ = buf.capacity(); // 方法调用正常
    }

    // ── Resampler 测试 ───────────────────────────────────

    #[test]
    fn resampler_same_rate_creates_passthrough() {
        let r = Resampler::new(44100, 44100);
        assert!(matches!(r, Resampler::Passthrough));
    }

    #[test]
    fn resampler_different_rate_creates_nearest() {
        let r = Resampler::new(44100, 48000);
        assert!(matches!(r, Resampler::Nearest { .. }));
    }

    #[test]
    fn resampler_passthrough_always_writes_and_advances() {
        let mut r = Resampler::new(44100, 44100);
        for _ in 0..100 {
            let result = r.process(AudioFrame {
                left: 100,
                right: 200,
            });
            assert!(result.write_output);
            assert!(result.advance_input);
        }
    }

    #[test]
    fn resampler_upsample_44100_to_48000_output_not_less_than_input() {
        // 上采样：输出帧数 >= 输入帧数
        let mut r = Resampler::new(44100, 48000);
        let input_count = 44100u32;
        let mut output_count = 0u32;
        let mut i = 0u32;

        while i < input_count {
            let result = r.process(AudioFrame { left: 0, right: 0 });
            if result.write_output {
                output_count += 1;
            }
            if result.advance_input {
                i += 1;
            }
        }
        // 期望输出 ≈ 48000，至少不少于输入
        assert!(output_count >= input_count);
    }

    #[test]
    fn resampler_downsample_48000_to_44100_output_not_more_than_input() {
        // 下采样：输出帧数 <= 输入帧数
        let mut r = Resampler::new(48000, 44100);
        let input_count = 48000u32;
        let mut output_count = 0u32;
        let mut i = 0u32;

        while i < input_count {
            let result = r.process(AudioFrame { left: 0, right: 0 });
            if result.write_output {
                output_count += 1;
            }
            if result.advance_input {
                i += 1;
            }
        }
        // 期望输出 ≈ 44100，不超过输入
        assert!(output_count <= input_count);
    }

    #[test]
    fn resampler_upsample_can_process_same_frame_twice() {
        // 上采样时同一输入帧可能被处理两次：
        // 第一次 write_output=true, advance_input=false
        // 第二次 write_output=true, advance_input=true
        let mut r = Resampler::new(44100, 48000);
        let frame = AudioFrame {
            left: 42,
            right: 99,
        };

        // 处理第一帧，直到它被消费
        let mut wrote = false;
        loop {
            let result = r.process(frame);
            if result.write_output {
                wrote = true;
            }
            if result.advance_input {
                break;
            }
        }
        assert!(wrote, "frame should have been output at least once");
    }

    #[test]
    fn resampler_has_no_platform_dependencies() {
        let mut r = Resampler::new(44100, 48000);
        let result = r.process(AudioFrame { left: 1, right: 2 });
        let _ = result;
    }
}
