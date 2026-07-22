//! # SD 卡路径常量
//!
//! 这些路径从 Platform trait 的关联常量派生而来。
//! 对应原 C 代码 `defines.h` 中的 `#define` 路径宏。
//!
//! ## 目录结构回顾
//!
//! ```text
//! <SDCARD_PATH>/
//! ├── Roms/                    ← ROMS_PATH
//! │   └── Game Boy (GB)/       ← 游戏主机目录
//! ├── Bios/                    ← BIOS 文件
//! ├── Saves/                   ← 游戏存档
//! ├── .system/                 ← ROOT_SYSTEM_PATH (更新时覆盖)
//! │   ├── <PLATFORM>/          ← SYSTEM_PATH
//! │   │   ├── bin/             ← 可执行文件
//! │   │   ├── cores/           ← libretro 核心
//! │   │   └── paks/            ← PAKS_PATH
//! │   └── res/                 ← RES_PATH (字体、图片)
//! ├── .userdata/               ← 用户数据 (更新时保留)
//! │   ├── <PLATFORM>/          ← USERDATA_PATH
//! │   └── shared/              ← SHARED_USERDATA_PATH
//! │       └── .minui/          ← MinUI 持久化状态
//! │           ├── recent.txt   ← RECENT_PATH
//! │           └── auto_resume.txt ← AUTO_RESUME_PATH
//! ├── Emus/                    ← 额外模拟器
//! ├── Tools/                   ← 工具 Pak
//! └── Collections/             ← 收藏列表
//! ```

use crate::platform::Platform;

// ============================================================================
// 主路径 —— 对应 C 中 defines.h 的各路径宏
// ============================================================================

/// ROM 存放目录
///
/// C: `#define ROMS_PATH SDCARD_PATH "/Roms"`
pub fn roms_path<P: Platform>() -> String {
    format!("{}/Roms", P::SDCARD_PATH)
}

/// .system 根目录 — 更新时会被覆盖
///
/// C: `#define ROOT_SYSTEM_PATH SDCARD_PATH "/.system/"`
pub fn root_system_path<P: Platform>() -> String {
    format!("{}/.system", P::SDCARD_PATH)
}

/// 当前平台的 .system 子目录
///
/// C: `#define SYSTEM_PATH SDCARD_PATH "/.system/" PLATFORM`
pub fn system_path<P: Platform>() -> String {
    format!("{}/.system/{}", P::SDCARD_PATH, P::PLATFORM_TAG)
}

/// 系统资源目录（字体、图片）
///
/// C: `#define RES_PATH SDCARD_PATH "/.system/res"`
pub fn res_path<P: Platform>() -> String {
    format!("{}/.system/res", P::SDCARD_PATH)
}

/// 字体文件路径
///
/// C: `#define FONT_PATH RES_PATH "/BPreplayBold-unhinted.otf"`
pub fn font_path<P: Platform>() -> String {
    format!("{}/BPreplayBold-unhinted.otf", res_path::<P>())
}

/// 当前平台的用户数据目录 — 更新时保留
///
/// C: `#define USERDATA_PATH SDCARD_PATH "/.userdata/" PLATFORM`
pub fn userdata_path<P: Platform>() -> String {
    format!("{}/.userdata/{}", P::SDCARD_PATH, P::PLATFORM_TAG)
}

/// 共享用户数据目录 — 跨平台共享
///
/// C: `#define SHARED_USERDATA_PATH SDCARD_PATH "/.userdata/shared"`
pub fn shared_userdata_path<P: Platform>() -> String {
    format!("{}/.userdata/shared", P::SDCARD_PATH)
}

/// Pak 存放目录
///
/// C: `#define PAKS_PATH SYSTEM_PATH "/paks"`
pub fn paks_path<P: Platform>() -> String {
    format!("{}/paks", system_path::<P>())
}

/// 最近游戏列表文件
///
/// C: `#define RECENT_PATH SHARED_USERDATA_PATH "/.minui/recent.txt"`
pub fn recent_path<P: Platform>() -> String {
    format!("{}/.minui/recent.txt", shared_userdata_path::<P>())
}

/// 简化模式标记文件
///
/// C: `#define SIMPLE_MODE_PATH SHARED_USERDATA_PATH "/enable-simple-mode"`
pub fn simple_mode_path<P: Platform>() -> String {
    format!("{}/enable-simple-mode", shared_userdata_path::<P>())
}

/// `simple_mode_path` 的非泛型版本
pub fn simple_mode_path_direct(sdcard: &str) -> String {
    format!("{}/.userdata/shared/enable-simple-mode", sdcard)
}

/// 自动恢复文件 — minarch 异常退出时写入，minui 启动时检测并自动恢复游戏
///
/// C: `#define AUTO_RESUME_PATH SHARED_USERDATA_PATH "/.minui/auto_resume.txt"`
pub fn auto_resume_path<P: Platform>() -> String {
    format!("{}/.minui/auto_resume.txt", shared_userdata_path::<P>())
}

/// `auto_resume_path` 的非泛型版本 — 直接传入 SD 卡路径，便于测试
pub fn auto_resume_path_direct(sdcard: &str) -> String {
    format!("{}/.userdata/shared/.minui/auto_resume.txt", sdcard)
}

// ============================================================================
// 临时文件路径 (tmpfs — 重启消失)
// ============================================================================

/// 上次浏览位置
///
/// C: `#define LAST_PATH "/tmp/last.txt"`
pub const LAST_PATH: &str = "/tmp/last.txt";

/// 换碟标记文件
///
/// C: `#define CHANGE_DISC_PATH "/tmp/change_disc.txt"`
pub const CHANGE_DISC_PATH: &str = "/tmp/change_disc.txt";

/// 恢复存档槽位号
///
/// C: `#define RESUME_SLOT_PATH "/tmp/resume_slot.txt"`
pub const RESUME_SLOT_PATH: &str = "/tmp/resume_slot.txt";

/// 下一阶段命令 — minui 写入，外层 shell 脚本读取
///
/// C: 写入的是 `/tmp/next`，读取的也是 `/tmp/next`
/// 对应 `queueNext()` 函数中的 `putFile("/tmp/next", cmd)`
pub const NEXT_CMD_PATH: &str = "/tmp/next";

/// 无 UI 标记 — 跳过 UI 直接启动游戏
///
/// C: `#define NOUI_PATH "/tmp/noui"`
pub const NOUI_PATH: &str = "/tmp/noui";

// ============================================================================
// 伪路径 — 不是真实存在的目录
// ============================================================================

/// 最近游戏伪目录（不实际存在于文件系统中）
pub fn faux_recent_path<P: Platform>() -> String {
    format!("{}/Recently Played", P::SDCARD_PATH)
}

/// 收藏夹目录
pub fn collections_path<P: Platform>() -> String {
    format!("{}/Collections", P::SDCARD_PATH)
}

// ============================================================================
// 存档状态路径
// ============================================================================

/// 构建 ROM 的存档槽位文件路径
///
/// 格式：`<SHARED_USERDATA>/.minui/<EMU_TAG>/<ROM_FILENAME>.txt`
///
/// C 中的路径构造逻辑在 `readyResumePath()`:
/// ```c
/// sprintf(slot_path, "%s/.minui/%s/%s.txt",
///     SHARED_USERDATA_PATH, emu_name, rom_file);
/// ```
pub fn slot_path<P: Platform>(emu_name: &str, rom_file: &str) -> String {
    format!(
        "{}/.minui/{}/{}.txt",
        shared_userdata_path::<P>(),
        emu_name,
        rom_file
    )
}

/// `slot_path` 的非泛型版本 — 直接传入 SD 卡路径，便于测试
pub fn slot_path_direct(sdcard: &str, emu_name: &str, rom_file: &str) -> String {
    format!(
        "{}/.userdata/shared/.minui/{}/{}.txt",
        sdcard, emu_name, rom_file
    )
}

/// 构建存档槽位关联的碟号文件路径（用于多碟游戏）
///
/// 格式：`<SHARED_USERDATA>/.minui/<EMU_TAG>/<ROM_FILE>.<SLOT>.txt`
///
/// C 中的路径构造逻辑在 `openRom()`:
/// ```c
/// sprintf(disc_path_path, "%s/.minui/%s/%s.%s.txt",
///     SHARED_USERDATA_PATH, emu_name, rom_file, slot);
/// ```
pub fn disc_slot_path<P: Platform>(emu_name: &str, rom_file: &str, slot: u8) -> String {
    format!(
        "{}/.minui/{}/{}.{}.txt",
        shared_userdata_path::<P>(),
        emu_name,
        rom_file,
        slot
    )
}

/// `disc_slot_path` 的非泛型版本 — 直接传入 SD 卡路径和槽位号
///
/// `slot` 是从存档槽位文件（如 `Zelda.gb.txt`）中读取的存档编号（如 "0"）。
pub fn disc_slot_path_direct(sdcard: &str, emu_name: &str, rom_file: &str, slot: &str) -> String {
    format!(
        "{}/.userdata/shared/.minui/{}/{}.{}.txt",
        sdcard, emu_name, rom_file, slot
    )
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::test_platform::TestPlatform;

    #[test]
    fn test_paths_with_test_platform() {
        assert_eq!(roms_path::<TestPlatform>(), "/tmp/test_sdcard/Roms");
        assert_eq!(system_path::<TestPlatform>(), "/tmp/test_sdcard/.system/test");
        assert_eq!(recent_path::<TestPlatform>(), "/tmp/test_sdcard/.userdata/shared/.minui/recent.txt");
        assert_eq!(slot_path::<TestPlatform>("GB", "Zelda.gb"),
            "/tmp/test_sdcard/.userdata/shared/.minui/GB/Zelda.gb.txt");
    }
}
