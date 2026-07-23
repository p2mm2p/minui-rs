//! # Audio — 环形缓冲区 + 重采样
//!
//! 对应原 C 代码中的 `SND_*` 函数族。

use minui_platform::AudioFrame;

/// 音频缓冲区（环形缓冲）
pub struct AudioRing {
    /// 缓冲区
    buffer: Vec<AudioFrame>,
    /// 缓冲区容量（帧数）
    capacity: usize,
    /// 写指针
    write_pos: usize,
    /// 读指针
    read_pos: usize,
    /// 上次消费位置（用于反压检测）
    last_consumed: usize,
    /// 输入采样率
    sample_rate_in: u32,
    /// 输出采样率
    sample_rate_out: u32,
    /// 帧率
    frame_rate: f64,
    /// 重采样误差累积
    resample_error: i32,
}

impl AudioRing {
    /// 创建音频缓冲区
    ///
    /// `buffer_seconds`: 缓冲时长（秒），默认 5
    pub fn new(sample_rate_in: u32, sample_rate_out: u32, frame_rate: f64, buffer_seconds: u32) -> Self {
        let frame_count = (buffer_seconds as f64 * sample_rate_in as f64 / frame_rate) as usize;
        Self {
            buffer: vec![AudioFrame::default(); frame_count],
            capacity: frame_count,
            write_pos: 0,
            read_pos: 0,
            last_consumed: frame_count.saturating_sub(1),
            sample_rate_in,
            sample_rate_out,
            frame_rate,
            resample_error: 0,
        }
    }

    /// 批量写入采样帧（模拟器核心调用）
    ///
    /// 返回实际消费的帧数。
    pub fn write(&mut self, frames: &[AudioFrame]) -> usize {
        // TODO: 实现 Nearest 重采样 + 环形写入
        let _ = frames;
        0
    }

    /// 从缓冲区读取采样帧（音频驱动调用）
    ///
    /// 返回实际读取的帧数。
    pub fn read(&mut self, out: &mut [AudioFrame]) -> usize {
        // TODO: 实现环形读取 + 欠载处理
        let _ = out;
        0
    }

    /// 缓冲区是否为空
    pub fn is_empty(&self) -> bool {
        self.write_pos == self.read_pos
    }
}
