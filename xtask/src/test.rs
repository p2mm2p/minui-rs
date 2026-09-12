//! `test` 命令模块
//!
//! 运行单元测试，两种形态（命令必须明确，无默认值）：
//!
//! - `cargo xtask test`：排除**全部**平台相关 crate（`platforms/` 下自动
//!   扫描——新增平台零维护），只测试通用层代码
//! - `cargo xtask test --platform <p> --device <d>`：`--platform` 与
//!   `--device` **成对必填**（无默认），排除**其他**平台 crate、保留该
//!   平台 + 通用层，并加 `--features <p>/<d>` 连带测试该平台代码
//!   （device feature 是编译期二选一，平台 lib 无默认，必须显式指定）
//!
//! 对应 C 原版：无直接对应（Rust 项目新增的辅助命令）。
//!
//! 已知限制：render crate 的 8 个测试依赖 macOS/Linux 字体路径，
//! 在部分环境失败——由后续 `fix-render-font-test` 变更修复。
//!

use crate::utils;

/// 返回 test 命令的参数列表（含 workspace 范围参数）。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`；None = 排除全部平台）
/// - `device`：设备参数（如 `smart`/`brick`；platform 为 Some 时必填）
///
/// # 返回值
///
/// `cargo test` 的参数列表（`--workspace` + 范围参数）。
fn test_args(platform: Option<&str>, device: &str) -> Vec<String> {
    let mut args = vec!["test".to_string(), "--workspace".to_string()];
    args.extend(utils::aux_scope_args(platform, device));
    args
}

/// 执行 `test` 命令：运行 workspace 单元测试。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`；None = 只测通用层，排除全部平台）
/// - `device`：设备参数（platform 为 Some 时必填，调用方保证成对）
///
/// # 返回值
///
/// - `Ok(())`：测试全部通过
/// - `Err(String)`：未知平台、测试失败（含 render 字体已知限制）或命令执行失败
pub(crate) fn run(platform: Option<&str>, device: &str) -> Result<(), String> {
    if let Some(p) = platform {
        utils::validate_platform(p)?;
    }
    let args = test_args(platform, device);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    utils::run_command("cargo", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_platform_excludes_all_platform_crates() {
        let args = test_args(None, "smart");
        let joined = args.join(" ");
        assert!(joined.contains("--workspace"), "{joined}");
        // 排除全部平台 crate（自动扫描 platforms/ 的包名）
        assert!(joined.contains("--exclude platform-tg5040"), "{joined}");
        assert!(
            joined.contains("--exclude platform-tg5040-show"),
            "{joined}"
        );
        assert!(
            joined.contains("--exclude platform-tg5040-keymon"),
            "{joined}"
        );
        assert!(joined.contains("--exclude tg5040-xtask"), "{joined}");
        // 无 platform 时不得带 --features（命令无默认值）
        assert!(!joined.contains("--features"), "{joined}");
    }

    #[test]
    fn with_platform_keeps_platform_and_adds_features() {
        let args = test_args(Some("tg5040"), "brick");
        let joined = args.join(" ");
        // 该平台 crate 不排除
        assert!(!joined.contains("--exclude platform-tg5040 "), "{joined}");
        assert!(!joined.contains("--exclude tg5040-xtask"), "{joined}");
        // 带显式 device feature（成对必填，无默认）
        assert!(
            joined.contains("--features platform-tg5040/brick"),
            "{joined}"
        );
        assert!(!joined.contains("platform-tg5040/smart"), "{joined}");
    }
}
