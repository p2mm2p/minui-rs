//! `platform` 步骤模块
//!
//! 调用平台子 xtask 完成平台侧全流程（要求 --platform）。
//! 原 C 原版 `cores` 步骤改名而来。
//!
//! 平台自治（平台子 xtask 模式）：平台侧的全部工作——show/keymon 编译、
//! libretro.so 编译与复制、其余平台资源（install/ 等）复制——由平台
//! 自己的子 xtask 承担（`platforms/<platform>/xtask/`，包名
//! `<platform>-xtask`）。参见
//! （打包架构——平台子 xtask 全流程自治）。
//!
//! 本模块的职责边界：只负责执行
//! `cargo run --quiet -p <platform>-xtask -- <device>`，xtask 不得包含
//! 职责边界 Requirement）。
//!

use crate::utils;

/// 构造 platform 步骤命令（可测纯函数）。
///
/// 返回 `(程序, 参数)`：执行 `cargo run --quiet -p <platform>-xtask -- <device>`。
/// 平台子 xtask 是 Rust host 工具（父 workspace 成员），经 cargo run 调用——
/// 不依赖任何 `.sh` 脚本/关联执行器。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`，平台子 xtask 包名为 `<platform>-xtask`）
/// - `device`：设备参数（如 `smart`/`brick`，透传给平台子 xtask）
///
/// # 返回值
///
/// `(程序, 参数)` 列表（单条命令）。
fn platform_command(platform: &str, device: &str) -> (String, Vec<String>) {
    let pkg = format!("{platform}-xtask");
    let args = vec![
        "run".to_string(),
        "--quiet".to_string(),
        "-p".to_string(),
        pkg,
        "--".to_string(),
        device.to_string(),
    ];
    ("cargo".to_string(), args)
}

/// 执行 `platform` 步骤：调用平台子 xtask。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`）
/// - `device`：设备参数（如 `smart`/`brick`，透传给平台子 xtask）
///
/// # 返回值
///
/// - `Ok(())`：平台子 xtask 执行成功
/// - `Err(String)`：cargo 缺失、平台子 xtask 缺失或执行失败——错误信息含 stderr
pub fn run(platform: &str, device: &str) -> Result<(), String> {
    let (program, args) = platform_command(platform, device);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    utils::run_command(&program, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_command_invokes_platform_xtask_with_device() {
        let (program, args) = platform_command("tg5040", "brick");
        // spec「platform 调用平台子 xtask」：cargo run --quiet -p tg5040-xtask -- brick
        assert_eq!(program, "cargo");
        let joined = args.join(" ");
        assert!(joined.contains("run --quiet -p tg5040-xtask"), "{joined}");
        assert!(joined.contains("-- brick"), "{joined}");
    }

    #[test]
    fn platform_command_passes_device() {
        let (_, args) = platform_command("tg5040", "smart");
        // device 是 `--` 之后的第一个参数
        let sep = args.iter().position(|a| a == "--").expect("应含 --");
        assert_eq!(args[sep + 1], "smart");
    }

    #[test]
    fn platform_command_uses_dash_xtask_naming() {
        // 命名约定：平台子 xtask 包名为 <platform>-xtask（对任何平台成立）
        let (_, args) = platform_command("rg35xx", "smart");
        let joined = args.join(" ");
        assert!(joined.contains("-p rg35xx-xtask"), "{joined}");
    }
}
