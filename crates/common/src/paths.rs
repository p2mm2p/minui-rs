//! 平台路径派生函数族
//!
//! 本模块提供 `common::paths` 路径派生自由函数——对应 C 原版
//! `defines.h:13-27` 的 SDCARD 家族路径宏。C 用 `#define` 宏在编译期
//! 拼接(`#define ROMS_PATH SDCARD_PATH "/Roms"`),Rust 版因 trait 关联
//! 常量无法引用其他关联常量(E0401)改为**纯字符串派生函数**。
//!
//! ## 设计
//!
//! - **零依赖**:函数体只做 `format!` 拼接,不执行任何文件系统操作
//!   (`std::fs` 零引用)、不依赖 `Platform` trait 或任何 crate 内部状态
//! - **扁平签名**:每个函数只接受根原语参数(`sdcard_path`,需要时加
//!   `platform`)——"一切路径都从 `SDCARD_PATH`(和 `PLATFORM`)派生"的
//!   单一心智模型,调用方无需持有中间值
//! - **原语来源**:`sdcard_path`/`platform` 由调用方从 `Platform` trait
//!   的关联常量传入(如 `P::SDCARD_PATH`/`P::PLATFORM`)
//! - **调用频率**:路径构造是"每操作一次"粒度(浏览/启动),非热路径;
//!   调用方可将常用值存局部变量复用
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `defines.h:13-27` 用宏拼接(编译期常量、零开销),Rust 版用函数
//! (运行时 `String` 分配)——语义等价、来源单一,分配开销可忽略。
//! 平台层的 `SDCARD_PATH` 各平台不同(C 13 平台各不相同,如 rg35xx 是
//! `/mnt/sdcard`、gkdpixel 是 `/media/roms`),本模块不关心具体值。
//!

// ── 跨进程协议常量（/tmp 绝对路径/纯值，无拼接——区别于 sdcard 派生函数）──

/// 续玩槽位请求文件路径（`/tmp/resume_slot.txt`）
///
/// 对应 C `defines.h:31` 的 `RESUME_SLOT_PATH`。
/// 读写方：minui 写（openRom/autoResume 的 `put_int`）、minarch 读+删
/// （minarch.c:576-580——启动时读取要加载的存档槽位号）。
pub const RESUME_SLOT_PATH: &str = "/tmp/resume_slot.txt";

/// 换碟请求文件路径（`/tmp/change_disc.txt`）
///
/// 对应 C `defines.h:30` 的 `CHANGE_DISC_PATH`。
/// 读写方：minarch 写（游戏内换碟时,minarch.c:380）、minui 读+删
/// （recents 加载时消费换碟请求更新最近列表）。
pub const CHANGE_DISC_PATH: &str = "/tmp/change_disc.txt";

/// 自动续玩默认槽位号（`9`）
///
/// 对应 C `defines.h:24` 的 `AUTO_RESUME_SLOT`。
/// 读写方：minui（autoResume 写入 RESUME_SLOT_PATH）、minarch
/// （minarch.c:571——未指定槽位时的默认初值）。
pub const AUTO_RESUME_SLOT: i32 = 9;

/// `SDCARD_PATH` + `"/Roms"`(对应 C `defines.h:13` 的 `ROMS_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`,来自 `Platform::SDCARD_PATH`)
///
/// # 返回值
///
/// ROM 目录路径,如 `"/mnt/SDCARD/Roms"`。
///
/// # 示例
///
/// ```
/// assert_eq!(common::paths::get_roms_path("/mnt/SDCARD"), "/mnt/SDCARD/Roms");
/// ```
pub fn get_roms_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/Roms")
}

/// `SDCARD_PATH` + `"/.system/"` + `PLATFORM`(对应 C `defines.h:15` 的 `SYSTEM_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
/// - `platform`:平台代码(如 `"tg5040"`,来自 `Platform::PLATFORM`)
///
/// # 返回值
///
/// 平台系统目录路径,如 `"/mnt/SDCARD/.system/tg5040"`。
pub fn get_system_path(sdcard_path: &str, platform: &str) -> String {
    format!("{sdcard_path}/.system/{platform}")
}

/// `SYSTEM_PATH` + `"/paks"`(对应 C `defines.h:20` 的 `PAKS_PATH`)
///
/// 扁平签名:直接取根原语,内部自行拼接 `.system/{platform}` 段。
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
/// - `platform`:平台代码(如 `"tg5040"`)
///
/// # 返回值
///
/// 平台 paks 目录路径,如 `"/mnt/SDCARD/.system/tg5040/paks"`。
pub fn get_paks_path(sdcard_path: &str, platform: &str) -> String {
    format!("{sdcard_path}/.system/{platform}/paks")
}

/// `SDCARD_PATH` + `"/.system/res"`(对应 C `defines.h:16` 的 `RES_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 资源目录路径,如 `"/mnt/SDCARD/.system/res"`。
pub fn get_res_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/.system/res")
}

/// `SDCARD_PATH` + `"/.userdata/"` + `PLATFORM`(对应 C `defines.h:18` 的 `USERDATA_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
/// - `platform`:平台代码(如 `"tg5040"`)
///
/// # 返回值
///
/// 平台用户数据目录路径,如 `"/mnt/SDCARD/.userdata/tg5040"`。
pub fn get_userdata_path(sdcard_path: &str, platform: &str) -> String {
    format!("{sdcard_path}/.userdata/{platform}")
}

/// `SDCARD_PATH` + `"/.userdata/shared"`(对应 C `defines.h:19` 的 `SHARED_USERDATA_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 跨平台共享用户数据目录路径,如 `"/mnt/SDCARD/.userdata/shared"`。
pub fn get_shared_userdata_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/.userdata/shared")
}

/// `SHARED_USERDATA_PATH` + `"/.minui/recent.txt"`(对应 C `defines.h:21` 的 `RECENT_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 最近游戏列表文件路径,如 `"/mnt/SDCARD/.userdata/shared/.minui/recent.txt"`。
pub fn get_recent_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/.userdata/shared/.minui/recent.txt")
}

/// `SHARED_USERDATA_PATH` + `"/.minui/auto_resume.txt"`(对应 C `defines.h:23` 的 `AUTO_RESUME_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 自动续玩记录文件路径,如 `"/mnt/SDCARD/.userdata/shared/.minui/auto_resume.txt"`。
pub fn get_auto_resume_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/.userdata/shared/.minui/auto_resume.txt")
}

/// `SHARED_USERDATA_PATH` + `"/enable-simple-mode"`(对应 C `defines.h:22` 的 `SIMPLE_MODE_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 简洁模式标记文件路径,如 `"/mnt/SDCARD/.userdata/shared/enable-simple-mode"`。
pub fn get_simple_mode_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/.userdata/shared/enable-simple-mode")
}

/// `SDCARD_PATH` + `"/Recently Played"`(对应 C `defines.h:26` 的 `FAUX_RECENT_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 最近游玩伪目录路径,如 `"/mnt/SDCARD/Recently Played"`。
pub fn get_faux_recent_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/Recently Played")
}

/// `SDCARD_PATH` + `"/Collections"`(对应 C `defines.h:27` 的 `COLLECTIONS_PATH`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 合集目录路径,如 `"/mnt/SDCARD/Collections"`。
pub fn get_collections_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/Collections")
}

/// `SDCARD_PATH` + `"/Tools/"` + `PLATFORM`(对应 C `minui.c:749` 的 `tools_path`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
/// - `platform`:平台代码(如 `"tg5040"`)
///
/// # 返回值
///
/// 平台工具目录路径,如 `"/mnt/SDCARD/Tools/tg5040"`。
pub fn get_tools_dir(sdcard_path: &str, platform: &str) -> String {
    format!("{sdcard_path}/Tools/{platform}")
}

/// `SDCARD_PATH` + `"/.system/version.txt"`(对应 C `defines.h:14` 的
/// `ROOT_SYSTEM_PATH` + `"version.txt"`)
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径(如 `"/mnt/SDCARD"`)
///
/// # 返回值
///
/// 版本信息文件路径,如 `"/mnt/SDCARD/.system/version.txt"`。
///
/// # 语义区分
///
/// 对应 C 的 `ROOT_SYSTEM_PATH`(无平台段)——**不同于** `get_system_path`
/// (对应 `SYSTEM_PATH`,含平台段)。版本页读取的 `version.txt` 位于
/// `.system/` 根目录,不在 `.system/{platform}/` 下,两者不可混用。
pub fn get_version_txt_path(sdcard_path: &str) -> String {
    format!("{sdcard_path}/.system/version.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    // 对照 C defines.h:13-27 宏族与 minui.c:749 的 tools_path,
    // 以 tg5040 平台(sdcard="/mnt/SDCARD", platform="tg5040")断言派生值。

    #[test]
    fn roms_path_derivation() {
        assert_eq!(get_roms_path("/mnt/SDCARD"), "/mnt/SDCARD/Roms");
    }

    #[test]
    fn system_path_derivation() {
        assert_eq!(
            get_system_path("/mnt/SDCARD", "tg5040"),
            "/mnt/SDCARD/.system/tg5040"
        );
    }

    #[test]
    fn paks_path_derivation() {
        assert_eq!(
            get_paks_path("/mnt/SDCARD", "tg5040"),
            "/mnt/SDCARD/.system/tg5040/paks"
        );
    }

    #[test]
    fn res_path_derivation() {
        assert_eq!(get_res_path("/mnt/SDCARD"), "/mnt/SDCARD/.system/res");
    }

    #[test]
    fn userdata_path_derivation() {
        assert_eq!(
            get_userdata_path("/mnt/SDCARD", "tg5040"),
            "/mnt/SDCARD/.userdata/tg5040"
        );
    }

    #[test]
    fn shared_userdata_path_derivation() {
        assert_eq!(
            get_shared_userdata_path("/mnt/SDCARD"),
            "/mnt/SDCARD/.userdata/shared"
        );
    }

    #[test]
    fn recent_path_derivation() {
        assert_eq!(
            get_recent_path("/mnt/SDCARD"),
            "/mnt/SDCARD/.userdata/shared/.minui/recent.txt"
        );
    }

    #[test]
    fn auto_resume_path_derivation() {
        assert_eq!(
            get_auto_resume_path("/mnt/SDCARD"),
            "/mnt/SDCARD/.userdata/shared/.minui/auto_resume.txt"
        );
    }

    #[test]
    fn simple_mode_path_derivation() {
        assert_eq!(
            get_simple_mode_path("/mnt/SDCARD"),
            "/mnt/SDCARD/.userdata/shared/enable-simple-mode"
        );
    }

    #[test]
    fn faux_recent_path_derivation() {
        assert_eq!(
            get_faux_recent_path("/mnt/SDCARD"),
            "/mnt/SDCARD/Recently Played"
        );
    }

    #[test]
    fn collections_path_derivation() {
        assert_eq!(
            get_collections_path("/mnt/SDCARD"),
            "/mnt/SDCARD/Collections"
        );
    }

    #[test]
    fn tools_dir_derivation() {
        assert_eq!(
            get_tools_dir("/mnt/SDCARD", "tg5040"),
            "/mnt/SDCARD/Tools/tg5040"
        );
    }

    #[test]
    fn version_txt_path_derivation() {
        assert_eq!(
            get_version_txt_path("/mnt/SDCARD"),
            "/mnt/SDCARD/.system/version.txt"
        );
    }

    #[test]
    fn version_txt_path_has_no_platform_segment() {
        // 对应 C `ROOT_SYSTEM_PATH`（无平台段）与 `SYSTEM_PATH`（有平台段）的语义区分
        assert_ne!(
            get_version_txt_path("/mnt/SDCARD"),
            get_system_path("/mnt/SDCARD", "tg5040")
        );
        assert!(!get_version_txt_path("/mnt/SDCARD").contains("/tg5040"));
    }

    #[test]
    fn protocol_constants_values() {
        assert_eq!(RESUME_SLOT_PATH, "/tmp/resume_slot.txt");
        assert_eq!(CHANGE_DISC_PATH, "/tmp/change_disc.txt");
        assert_eq!(AUTO_RESUME_SLOT, 9);
    }

    #[test]
    fn constant_vs_derived_path_distinction() {
        // 纯值常量（AUTO_RESUME_SLOT）与 sdcard 派生方法（get_auto_resume_path）形态不同
        assert_ne!(
            AUTO_RESUME_SLOT.to_string(),
            get_auto_resume_path("/mnt/SDCARD")
        );
    }
}
