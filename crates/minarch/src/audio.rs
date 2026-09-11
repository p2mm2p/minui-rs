//! `audio` — 音频队列引擎
//!
//! 负责接收核心吐出的音频（经 libretro 回调桥转来的 [`sample_handler`]/
//! [`batch_handler`]），完成重采样后暂存进引擎队列；主循环每帧经
//! [`drain`] 取出并交给 `Platform::push_audio`（装配层负责）。
//!
//! ## 数据流
//!
//! ```text
//! 核心线程（thread_video 时独立）          主线程（装配层主循环）
//!   retro_audio_sample(_batch)              每帧 drain
//!         │                                      │
//!         ▼                                      ▼
//!   handler ──▶ AudioEngine ──▶ platform.push_audio ──▶ 平台环形缓冲
//!             （队列 + 重采样 + 快进门控）
//! ```
//!
//! ## 与原 C 代码的对比
//!
//! 原 C 代码（api.c:918-1130）用全局 `SND_Context`（环形缓冲 + 重采样器 +
//! 读写指针）配合 `SDL_LockAudio` 实现同样职责。Rust 版拆分为：
//!
//! - [`AudioEngine`] 纯逻辑引擎（本模块）——重采样器与引擎队列，
//!   状态显式归属实例、无全局可变
//! - 环形缓冲由 `common::audio::AudioRingBuffer` 提供
//! - 队列满时的 10ms 等待重试（`SDL_Delay(1)`×10）改为丢帧不阻塞
//! - C 的裸 `static int fast_forward` 数据竞争改为引擎字段（Mutex 保护）
//!

use common::audio::{AudioFrame, AudioRingBuffer, Resampler};
use std::sync::{Mutex, OnceLock};

/// 引擎队列容量（帧）——对应 C 4000 帧环形缓冲（api.c:983 计算结果的等价物）
const ENGINE_BUFFER_FRAMES: usize = 4000;

/// 采样率协商上限——对应 C `#define MAX_SAMPLE_RATE 48000`（api.c:918）
pub const MAX_SAMPLE_RATE: u32 = 48_000;

/// 音频队列引擎
///
/// 纯逻辑、无平台依赖、无线程：接收核心音频帧 → 重采样（核心采样率 →
/// 设备采样率）→ 入队；主循环 [`drain`](AudioEngine::drain) 取走。
/// 对应 C `SND_Context`（api.c:928-944）的 Rust 化。
///
/// 线程安全：实例由薄静态层的 `Mutex` 承载；本地实例单线程使用
/// 无需任何锁。
pub struct AudioEngine {
    /// 重采样器（Passthrough 或 Nearest，由构造时的采样率决定）
    resampler: Resampler,
    /// 引擎队列（固定容量环形缓冲，帧粒度部分写入）
    queue: AudioRingBuffer,
    /// 快进标志（`true` 时丢弃全部音频且不推进重采样相位）
    fast_forward: bool,
}

impl AudioEngine {
    /// 构造音频队列引擎
    ///
    /// # 参数
    ///
    /// - `sample_rate_in`: 核心采样率（如 GB 核心 43690Hz）
    /// - `sample_rate_out`: 设备实际采样率（`Platform::init_audio` 返回值）
    ///
    /// 采样率相等时选择直通重采样，否则选择最近邻（对应 C
    /// `SND_selectResampler`，api.c:1022-1029）。
    pub fn new(sample_rate_in: u32, sample_rate_out: u32) -> Self {
        Self {
            resampler: Resampler::new(sample_rate_in, sample_rate_out),
            queue: AudioRingBuffer::new(ENGINE_BUFFER_FRAMES),
            fast_forward: false,
        }
    }

    /// 处理单个音频帧：重采样后尝试入队
    ///
    /// # 参数
    ///
    /// - `frame`: 输入音频帧（核心采样率）
    ///
    /// # 行为
    ///
    /// - 快进时直接返回：不重采样、不入队（重采样相位与 C 一致地冻结）
    /// - 否则循环调用重采样器：需要输出时单帧入队（队列满则丢弃本帧，
    ///   帧粒度部分写入），上采样时同一输入帧被多次处理以复制输出
    ///   （对应 C `SND_resampleNone`/`SND_resampleNear`，api.c:1000-1021）
    pub fn process_frame(&mut self, frame: AudioFrame) {
        if self.fast_forward {
            return;
        }
        loop {
            let result = self.resampler.process(frame);
            if result.write_output {
                // 单帧 push：AudioRingBuffer 全有或全无 → 有空间写入，
                // 满则丢弃本帧（帧粒度部分写入）
                let _ = self.queue.push(core::slice::from_ref(&frame));
            }
            if result.advance_input {
                break;
            }
            // advance_input == false：上采样时同一输入帧再处理一次（复制输出）
        }
    }

    /// 处理一批音频帧（对应 C `SND_batchSamples`，api.c:1030-1066）
    ///
    /// # 参数
    ///
    /// - `frames`: 输入音频帧批次（核心采样率）
    ///
    /// # 返回值
    ///
    /// 恒返回 `frames.len()`——引擎永不提前中止：队列满时丢帧不阻塞，
    /// 输入帧全部被消费（与 C 中途挂起时可能返回部分帧数不同，见
    /// design 决策 3）
    pub fn process_batch(&mut self, frames: &[AudioFrame]) -> usize {
        for &frame in frames {
            self.process_frame(frame);
        }
        frames.len()
    }

    /// 弹出队列中的音频帧（对应 C `SND_audioCallback` 的消费侧）
    ///
    /// # 参数
    ///
    /// - `out`: 输出缓冲（容量任意，只弹出能容纳的部分）
    ///
    /// # 返回值
    ///
    /// 实际弹出的帧数；队列空时返回 `0`（消费者据此输出静音）
    pub fn drain(&mut self, out: &mut [AudioFrame]) -> usize {
        self.queue.pop(out)
    }

    /// 设置快进开关
    ///
    /// # 参数
    ///
    /// - `enable`: `true` 进入快进（丢弃全部音频），`false` 恢复正常
    pub fn set_fast_forward(&mut self, enable: bool) {
        self.fast_forward = enable;
    }

    /// 查询快进状态
    ///
    /// # 返回值
    ///
    /// 当前是否处于快进（丢弃音频）状态
    pub fn is_fast_forward(&self) -> bool {
        self.fast_forward
    }
}

// ═══════════════════════════════════════════════════════════════
// 薄静态层
// ═══════════════════════════════════════════════════════════════

/// 进程级音频引擎静态（OnceLock 注册后只读；Mutex 承载可变引擎状态）
///
/// 与 `vibration::VIBRATION` 同构的薄静态层。`FrontendState` 的 handler
/// 字段是 `fn` 指针、不可捕获状态，引擎只能以静态承载；Mutex 是跨线程
/// （核心回调线程写入 / 主循环 drain）共享可变状态的唯一诚实载体。
static AUDIO: OnceLock<Mutex<AudioEngine>> = OnceLock::new();

/// 注册进程级引擎（装配层在核心加载前调用一次）
///
/// # 参数
///
/// - `sample_rate_in`: 核心采样率（`av_info.timing.sample_rate`）
/// - `sample_rate_out`: 设备实际采样率（`Platform::init_audio` 返回值）
///
/// # 返回值
///
/// - `Ok(())`: 首次注册成功
/// - `Err(engine)`: 已注册过（重复注册是误用），携带本次构造的引擎
///   原值（`OnceLock::set` 语义）
pub fn init(sample_rate_in: u32, sample_rate_out: u32) -> Result<(), AudioEngine> {
    AUDIO
        .set(Mutex::new(AudioEngine::new(
            sample_rate_in,
            sample_rate_out,
        )))
        .map_err(|m| m.into_inner().unwrap_or_else(|p| p.into_inner()))
}

/// 音频单样本处理器：锁内转发 [`AudioEngine::process_frame`]
///
/// 未 `init` 时空操作（与 `environment::handle` 未 init 语义一致）。
/// 可直接注册进 `FrontendState.audio_sample`（类型匹配
/// `AudioSampleHandler`）。
///
/// # 参数
///
/// - `left`/`right`: 左右声道采样值
pub fn sample_handler(left: i16, right: i16) {
    if let Some(engine) = AUDIO.get() {
        engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .process_frame(AudioFrame { left, right });
    }
}

/// 音频批量处理器：交错 i16 转帧后锁内逐帧入队
///
/// 未 `init` 时返回 `frames`（与 trampoline 未注册默认一致）。
/// 可直接注册进 `FrontendState.audio_sample_batch`（类型匹配
/// `AudioSampleBatchHandler`）。
///
/// # 参数
///
/// - `data`: 交错立体声样本起始指针（左/右交替，共 `frames × 2` 个 `i16`）
/// - `frames`: 帧数
///
/// # 返回值
///
/// 恒返回 `frames`——输入帧全部被消费（引擎永不提前中止，满则丢帧）
///
/// # Safety
///
/// `data` 必须指向 `frames × 2` 个可读 `i16`——由核心经
/// `libretro::audio_sample_batch_trampoline` 调用保证（libretro.h
/// 契约）。本函数不保留切片引用。
/// 本函数保持安全签名是设计约束——必须兼容 `AudioSampleBatchHandler`
/// （安全 fn 指针类型）以注册进 `FrontendState.audio_sample_batch`；
/// 真正的信任边界在 unsafe trampoline（核心经它调用）与下方的
/// unsafe 块之间。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // 签名兼容约束见上：handler 必须为安全 fn
pub fn batch_handler(data: *const i16, frames: usize) -> usize {
    let Some(engine) = AUDIO.get() else {
        return frames;
    };
    let mut engine = engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: 见函数 Safety 章节——指针有效性由调用方保证，切片仅在
    // 本函数内使用
    let samples = unsafe { core::slice::from_raw_parts(data, frames * 2) };
    for pair in samples.chunks_exact(2) {
        engine.process_frame(AudioFrame {
            left: pair[0],
            right: pair[1],
        });
    }
    frames
}

/// 全局弹出队列：锁内转发 [`AudioEngine::drain`]
///
/// # 参数
///
/// - `out`: 输出缓冲（由装配层提供，弹出后调 `Platform::push_audio`）
///
/// # 返回值
///
/// 实际弹出的帧数；未 `init` 时返回 `0`
pub fn drain(out: &mut [AudioFrame]) -> usize {
    AUDIO
        .get()
        .map(|engine| {
            engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .drain(out)
        })
        .unwrap_or(0)
}

/// 全局设置快进开关：锁内转发 [`AudioEngine::set_fast_forward`]
///
/// 未 `init` 时空操作。
///
/// # 参数
///
/// - `enable`: `true` 进入快进（丢弃全部音频），`false` 恢复正常
pub fn set_fast_forward(enable: bool) {
    if let Some(engine) = AUDIO.get() {
        engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_fast_forward(enable);
    }
}
