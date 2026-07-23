//! # Game — ROM 加载、ZIP 解压、M3U 解析
//!
//! 对应原 C 代码中的 `Game_open()`, `Game_close()`, ZIP 处理, M3U 处理。

/// 加载的游戏状态
pub struct Game {
    /// ROM 完整路径
    pub path: String,
    /// 文件名（不含目录）
    pub name: String,
    /// M3U 文件路径（如果有）
    pub m3u_path: Option<String>,
    /// ZIP 解压临时路径
    pub tmp_path: Option<String>,
    /// ROM 数据（加载到内存）
    pub data: Option<Vec<u8>>,
    /// 是否已加载
    pub is_open: bool,
}

impl Game {
    /// 打开 ROM 文件
    ///
    /// 处理 ZIP 解压、M3U 检测。
    pub fn open(_path: &str) -> Result<Self, String> {
        // TODO: 实现 ROM 加载
        // 1. 检测是否是 ZIP → 解压
        // 2. 检测 M3U → 记录路径
        // 3. 读取 ROM 到内存
        Err("Game::open not yet implemented".into())
    }

    /// 关闭并清理
    pub fn close(&mut self) {
        // TODO: 释放内存，删除临时文件
    }
}

/// 从 ZIP 中提取 ROM 文件
///
/// 仅支持 Store 和 Deflate 压缩。
pub fn extract_from_zip(_zip_path: &str) -> Result<(Vec<u8>, String), String> {
    // TODO: 实现 ZIP 解压
    Err("extract_from_zip not yet implemented".into())
}

/// 解析 M3U 文件，返回碟片路径列表
pub fn parse_m3u(_m3u_path: &str) -> Result<Vec<String>, String> {
    // TODO: 实现 M3U 解析
    Err("parse_m3u not yet implemented".into())
}
