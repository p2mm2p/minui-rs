//! audio.rs 音频队列引擎测试
//!
//! 覆盖 spec「audio 队列引擎状态机」「audio 薄静态层」两个
//! Requirement 的全部 Scenario。
//!
//! 分层测试策略：
//! - `AudioEngine` 纯逻辑引擎用本地实例构造——无静态，各用例互不
//!   干扰，可并行执行
//! - 薄静态层（`AUDIO`）是进程级 OnceLock，注册后无法撤销——生命周期
//!   用例合并为**单个测试串行执行**（同 tests/vibration.rs 的静态薄层决策）

use common::audio::AudioFrame;
use minarch::audio;
use minarch::audio::AudioEngine;

/// 构造测试帧：左声道 `l`、右声道 `r`
fn frame(l: i16, r: i16) -> AudioFrame {
    AudioFrame { left: l, right: r }
}

/// 把引擎队列内容全部弹出（上限 4096 帧），返回 `(左, 右)` 列表
fn drain_all(engine: &mut AudioEngine) -> Vec<(i16, i16)> {
    let mut out = [frame(0, 0); 4096];
    let n = engine.drain(&mut out);
    out[..n].iter().map(|f| (f.left, f.right)).collect()
}

// ── 引擎：直通与边界速率 ─────────────────────────────────────

#[test]
fn same_rate_passthrough_preserves_frames() {
    let mut engine = AudioEngine::new(48000, 48000);
    let input = [frame(1, 2), frame(3, 4), frame(5, 6)];

    assert_eq!(engine.process_batch(&input), 3);
    assert_eq!(drain_all(&mut engine), vec![(1, 2), (3, 4), (5, 6)]);
}

#[test]
fn equal_rates_select_passthrough_single_frame() {
    let mut engine = AudioEngine::new(44100, 44100);

    engine.process_frame(frame(7, 8));
    assert_eq!(drain_all(&mut engine), vec![(7, 8)]);
}

// ── 引擎：重采样（上采样复制 / 下采样跳帧） ────────────────────

#[test]
fn upsample_duplicates_frames() {
    let mut engine = AudioEngine::new(32040, 48000);
    let input = [frame(1, 1), frame(2, 2)];

    assert_eq!(engine.process_batch(&input), 2);
    // 每 2 输入帧产出 3 输出帧，其中 1 帧为复制
    assert_eq!(drain_all(&mut engine).len(), 3);
}

#[test]
fn downsample_skips_frames() {
    let mut engine = AudioEngine::new(55930, 48000);
    let input = [frame(1, 1); 8];

    assert_eq!(engine.process_batch(&input), 8);
    // 每 8 输入帧跳 1 帧（比率 ≈ out/in）
    assert_eq!(drain_all(&mut engine).len(), 7);
}

// ── 引擎：边界（队列满 / 快进 / 空队列 / 部分读取） ───────────────

#[test]
fn queue_full_drops_without_blocking() {
    let mut engine = AudioEngine::new(48000, 48000);
    // 填满队列（容量 4000）
    let fill = [frame(1, 1); 4000];
    assert_eq!(engine.process_batch(&fill), 4000);

    // 溢出批次立即返回、不阻塞、不 panic；队列内容不变
    let overflow = [frame(9, 9); 1000];
    assert_eq!(engine.process_batch(&overflow), 1000);
    assert_eq!(drain_all(&mut engine), vec![(1, 1); 4000]);
}

#[test]
fn fast_forward_gates_audio() {
    let mut engine = AudioEngine::new(48000, 48000);
    engine.set_fast_forward(true);
    assert!(engine.is_fast_forward());

    let batch = [frame(1, 1); 100];
    assert_eq!(engine.process_batch(&batch), 100);
    assert_eq!(engine.drain(&mut [frame(0, 0); 8]), 0);

    // 退出快进后恢复入队（相位与快进前一致）
    engine.set_fast_forward(false);
    engine.process_frame(frame(5, 6));
    assert_eq!(drain_all(&mut engine), vec![(5, 6)]);
}

#[test]
fn drain_empty_queue_returns_zero() {
    let mut engine = AudioEngine::new(48000, 48000);
    let mut out = [frame(0, 0); 512];
    assert_eq!(engine.drain(&mut out), 0);
}

#[test]
fn drain_partial_read() {
    let mut engine = AudioEngine::new(48000, 48000);
    engine.process_batch(&[frame(1, 2), frame(3, 4), frame(5, 6)]);

    let mut out = [frame(0, 0); 2];
    assert_eq!(engine.drain(&mut out), 2);
    assert_eq!(
        out.iter().map(|f| (f.left, f.right)).collect::<Vec<_>>(),
        vec![(1, 2), (3, 4)]
    );

    let mut rest = [frame(0, 0); 2];
    assert_eq!(engine.drain(&mut rest), 1);
    assert_eq!((rest[0].left, rest[0].right), (5, 6));
}

// ── 薄静态层：进程级 OnceLock（单测试串行，同 vibration 决策） ───────

#[test]
fn static_layer_lifecycle() {
    // 未 init：安全默认（batch_handler 返回 frames、drain 返回 0、不 panic）
    let mut out = [frame(0, 0); 8];
    assert_eq!(audio::drain(&mut out), 0);
    audio::sample_handler(1, 2);
    let data = [1i16, 2, 3, 4, 5, 6, 7, 8];
    assert_eq!(audio::batch_handler(data.as_ptr(), 4), 4);
    audio::set_fast_forward(true);

    // init 一次性注册：首次 Ok，重复 Err（携带原引擎）
    assert!(audio::init(48000, 48000).is_ok());
    assert!(audio::init(44100, 44100).is_err());

    // batch_handler 交错数据转帧
    let data = [1i16, 2, 3, 4];
    assert_eq!(audio::batch_handler(data.as_ptr(), 2), 2);
    let mut out = [frame(0, 0); 8];
    let n = audio::drain(&mut out);
    assert_eq!(n, 2);
    assert_eq!((out[0].left, out[0].right), (1, 2));
    assert_eq!((out[1].left, out[1].right), (3, 4));

    // 快进：handler 声称全消费、队列保持空
    audio::set_fast_forward(true);
    let data = [1i16, 2, 3, 4];
    assert_eq!(audio::batch_handler(data.as_ptr(), 2), 2);
    assert_eq!(audio::drain(&mut [frame(0, 0); 8]), 0);
}

#[test]
fn handlers_match_callback_bridge_types() {
    let _: minarch::libretro::AudioSampleHandler = audio::sample_handler;
    let _: minarch::libretro::AudioSampleBatchHandler = audio::batch_handler;
}
