//! `lint` 命令模块
//!
//! 执行格式化检查与静态检查（严格模式），两种形态（命令必须明确，
//! 无默认值）：
//!
//! - `cargo xtask lint`：排除**全部**平台相关 crate（`platforms/` 下自动
//!   扫描——新增平台零维护），只检查通用层代码
//! - `cargo xtask lint --platform <p> --device <d>`：`--platform` 与
//!   `--device` **成对必填**（无默认），排除**其他**平台 crate、保留该
//!   平台 + 通用层，并加 `--features <p>/<d>` 连带检查该平台代码
//!   （device feature 是编译期二选一，平台 lib 无默认，必须显式指定）
//!
//! fmt 始终覆盖全 workspace（格式化与平台无关，不排除）；clippy 按
//! 上述范围执行。
//!
//! 对应 C 原版：无直接对应（Rust 项目新增的辅助命令）。
//! 严格模式（`-D warnings`）下当前代码库的既有格式与 clippy 问题
//! 会导致命令失败——这是预期行为（lint 的意义就是暴露问题），
//! 由后续 `fix-workspace-fmt` 变更清理。
//!
//! 平台 crate 的接口约束由 Rust 可见性（`pub(crate)`）承担，lint 不
//! 维护平台引用白名单检查（统一标准，平台特有规则已删除）。
//!

use crate::utils;

/// 返回 lint 检查的命令序列（fmt 先行，clippy 随后）。
///
/// fmt 覆盖全 workspace（无排除）；clippy 按平台范围参数执行。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`；None = clippy 排除全部平台）
/// - `device`：设备参数（platform 为 Some 时必填，调用方保证成对）
///
/// # 返回值
///
/// 按执行顺序排列的 `(程序, 参数)` 列表。
fn lint_commands(platform: Option<&str>, device: &str) -> Vec<(&'static str, Vec<String>)> {
    let mut clippy_args = vec![
        "clippy".to_string(),
        "--workspace".to_string(),
        "--all-targets".to_string(),
    ];
    clippy_args.extend(utils::aux_scope_args(platform, device));
    clippy_args.extend(["--".to_string(), "-D".to_string(), "warnings".to_string()]);
    vec![
        (
            "cargo",
            vec![
                "fmt".to_string(),
                "--all".to_string(),
                "--check".to_string(),
            ],
        ),
        ("cargo", clippy_args),
    ]
}

/// 执行 `lint` 命令：依次运行 fmt 与 clippy 严格检查。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`；None = clippy 排除全部平台）
/// - `device`：设备参数（platform 为 Some 时必填，调用方保证成对）
///
/// # 返回值
///
/// - `Ok(())`：检查全部通过
/// - `Err(String)`：任一检查失败——错误信息含失败命令名与 stderr
pub(crate) fn run(platform: Option<&str>, device: &str) -> Result<(), String> {
    if let Some(p) = platform {
        utils::validate_platform(p)?;
    }
    for (program, args) in lint_commands(platform, device) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        utils::run_command(program, &args)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_commands_run_fmt_first_then_clippy() {
        let commands = lint_commands(None, "smart");
        assert_eq!(commands.len(), 2);
        // fmt 全 workspace 无排除
        let (program, args) = &commands[0];
        assert_eq!(program, &"cargo");
        assert_eq!(args, &vec!["fmt", "--all", "--check"]);
        // clippy 排除全部平台 crate
        let (program, args) = &commands[1];
        assert_eq!(program, &"cargo");
        let joined = args.join(" ");
        assert!(
            joined.contains("clippy --workspace --all-targets"),
            "{joined}"
        );
        assert!(joined.contains("--exclude platform-tg5040"), "{joined}");
        assert!(joined.contains("-D warnings"), "{joined}");
        // 无 platform 时 clippy 不得带 --features
        assert!(!joined.contains("--features"), "{joined}");
    }

    #[test]
    fn lint_with_platform_keeps_platform_and_adds_features() {
        let commands = lint_commands(Some("tg5040"), "brick");
        let (_, args) = &commands[1];
        let joined = args.join(" ");
        assert!(!joined.contains("--exclude platform-tg5040 "), "{joined}");
        assert!(joined.contains("--features tg5040/brick"), "{joined}");
        assert!(!joined.contains("tg5040/smart"), "{joined}");
    }
}
