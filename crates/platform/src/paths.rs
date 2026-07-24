//! # 平台关联路径（泛型版本）
//!
//! 这些函数使用 Platform trait 的关联常量来构造路径。
//! 对应的非泛型版本（直接传 sdcard 参数）在 common crate 中。

use crate::Platform;

/// ROM 存放目录
pub fn roms_path<P: Platform>() -> String {
    format!("{}/Roms", P::SDCARD_PATH)
}

/// .system 根目录
pub fn root_system_path<P: Platform>() -> String {
    format!("{}/.system", P::SDCARD_PATH)
}

/// 当前平台的 .system 子目录
pub fn system_path<P: Platform>() -> String {
    format!("{}/.system/{}", P::SDCARD_PATH, P::PLATFORM_TAG)
}

/// 系统资源目录
pub fn res_path<P: Platform>() -> String {
    format!("{}/.system/res", P::SDCARD_PATH)
}

/// 字体文件路径
pub fn font_path<P: Platform>() -> String {
    format!("{}/BPreplayBold-unhinted.otf", res_path::<P>())
}

/// 当前平台的用户数据目录
pub fn userdata_path<P: Platform>() -> String {
    format!("{}/.userdata/{}", P::SDCARD_PATH, P::PLATFORM_TAG)
}

/// 共享用户数据目录
pub fn shared_userdata_path<P: Platform>() -> String {
    format!("{}/.userdata/shared", P::SDCARD_PATH)
}

/// Pak 存放目录
pub fn paks_path<P: Platform>() -> String {
    format!("{}/paks", system_path::<P>())
}

/// 最近游戏列表文件
pub fn recent_path<P: Platform>() -> String {
    format!("{}/.minui/recent.txt", shared_userdata_path::<P>())
}

/// 简化模式标记文件
pub fn simple_mode_path<P: Platform>() -> String {
    format!("{}/enable-simple-mode", shared_userdata_path::<P>())
}

/// 自动恢复文件
pub fn auto_resume_path<P: Platform>() -> String {
    format!("{}/.minui/auto_resume.txt", shared_userdata_path::<P>())
}

/// 最近游戏伪目录
pub fn faux_recent_path<P: Platform>() -> String {
    format!("{}/Recently Played", P::SDCARD_PATH)
}

/// 收藏夹目录
pub fn collections_path<P: Platform>() -> String {
    format!("{}/Collections", P::SDCARD_PATH)
}

/// 存档槽位文件路径
pub fn slot_path<P: Platform>(emu_name: &str, rom_file: &str) -> String {
    format!(
        "{}/.minui/{}/{}.txt",
        shared_userdata_path::<P>(),
        emu_name,
        rom_file
    )
}

/// 碟号文件路径
pub fn disc_slot_path<P: Platform>(emu_name: &str, rom_file: &str, slot: u8) -> String {
    format!(
        "{}/.minui/{}/{}.{}.txt",
        shared_userdata_path::<P>(),
        emu_name,
        rom_file,
        slot
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_platform::TestPlatform;

    #[test]
    fn test_paths_with_test_platform() {
        assert_eq!(roms_path::<TestPlatform>(), "/tmp/test_sdcard/Roms");
        assert_eq!(system_path::<TestPlatform>(), "/tmp/test_sdcard/.system/test");
        assert_eq!(recent_path::<TestPlatform>(), "/tmp/test_sdcard/.userdata/shared/.minui/recent.txt");
        assert_eq!(slot_path::<TestPlatform>("GB", "Zelda.gb"),
            "/tmp/test_sdcard/.userdata/shared/.minui/GB/Zelda.gb.txt");
    }
}
