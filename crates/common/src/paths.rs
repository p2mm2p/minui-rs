//! # SD 卡路径常量和辅助函数
//!
//! 所有不依赖 Platform trait 的路径定义都在这里。
//! 依赖 Platform 关联常量的泛型版本在 `platform` crate 中。
//!
//! ## 目录结构回顾
//!
//! ```text
//! <SDCARD_PATH>/
//! ├── Roms/                    ← ROMS_PATH
//! │   └── Game Boy (GB)/       ← 游戏主机目录
//! ├── .system/                 ← 系统文件 (更新时覆盖)
//! │   ├── <PLATFORM>/          ← 平台专属
//! │   │   └── paks/            ← Pak 目录
//! │   └── res/                 ← 资源 (字体、图片)
//! ├── .userdata/               ← 用户数据 (更新时保留)
//! │   ├── <PLATFORM>/
//! │   └── shared/
//! │       └── .minui/
//! │           ├── recent.txt
//! │           └── auto_resume.txt
//! ├── Emus/                    ← 额外模拟器
//! ├── Tools/                   ← 工具 Pak
//! └── Collections/             ← 收藏列表
//! ```

// ============================================================================
// 临时文件路径 (tmpfs — 重启消失)
// ============================================================================

/// 上次浏览位置
pub const LAST_PATH: &str = "/tmp/last.txt";

/// 换碟标记文件
pub const CHANGE_DISC_PATH: &str = "/tmp/change_disc.txt";

/// 恢复存档槽位号
pub const RESUME_SLOT_PATH: &str = "/tmp/resume_slot.txt";

/// 下一阶段命令 — minui 写入，外层 shell 脚本读取
pub const NEXT_CMD_PATH: &str = "/tmp/next";

/// 无 UI 标记 — 跳过 UI 直接启动游戏
pub const NOUI_PATH: &str = "/tmp/noui";

// ============================================================================
// SD 卡路径（直接传入 sdcard 的非泛型版本，便于测试和管理）
// ============================================================================

/// 简化模式标记文件
pub fn simple_mode_path_direct(sdcard: &str) -> String {
    format!("{}/.userdata/shared/enable-simple-mode", sdcard)
}

/// 自动恢复文件
pub fn auto_resume_path_direct(sdcard: &str) -> String {
    format!("{}/.userdata/shared/.minui/auto_resume.txt", sdcard)
}

// ============================================================================
// 存档状态路径（非泛型版本）
// ============================================================================

/// 构建 ROM 的存档槽位文件路径
///
/// 格式：`{sdcard}/.userdata/shared/.minui/{emu_name}/{rom_file}.txt`
pub fn slot_path_direct(sdcard: &str, emu_name: &str, rom_file: &str) -> String {
    format!(
        "{}/.userdata/shared/.minui/{}/{}.txt",
        sdcard, emu_name, rom_file
    )
}

/// 构建存档槽位关联的碟号文件路径
///
/// 格式：`{sdcard}/.userdata/shared/.minui/{emu_name}/{rom_file}.{slot}.txt`
pub fn disc_slot_path_direct(sdcard: &str, emu_name: &str, rom_file: &str, slot: &str) -> String {
    format!(
        "{}/.userdata/shared/.minui/{}/{}.{}.txt",
        sdcard, emu_name, rom_file, slot
    )
}
