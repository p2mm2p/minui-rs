//! `xtask` — MinUI 构建辅助工具（Rust 重写版）
//!
//! 使用 `cargo xtask <子命令>` 模式（经 `.cargo/config.toml` 的 alias）：
//!
//! - `toolchain <all|setup|special|tidy|build|system|platform|package>`：
//!   构建流水线，对齐 C 原版 makefile（`MinUI/makefile`）的
//!   setup/special/tidy/cores/build/system/package 目标
//! - `clean`：删除 build/（`--all` 连带删除 target/）
//! - `doc`：生成 rustdoc 文档并自动打开首页
//! - `test`：运行单元测试
//! - `lint`：格式化 + Lint（cargo fmt + cargo clippy）
//! - `help`：显示 xtask 的详细使用说明
//!
//! 本文件只做 clap 入口与顶层分发，各命令的行为实现在对应模块
//! （`clean`/`doc`/`lint`/`test`/`help`/`toolchain`）中。
//!

mod clean;
mod doc;
mod help;
mod lint;
mod test;
mod toolchain;
mod utils;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// xtask 命令行入口（clap 定义）。
#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "MinUI 构建辅助工具（Rust 重写版）")]
#[command(disable_help_subcommand = true)] // 自定义 help 子命令，禁用 clap 内置 help
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// 顶层子命令。
#[derive(Subcommand, Debug)]
enum Commands {
    /// 构建流水线（对齐 C 原版 makefile 的 setup/special/tidy/build/system/cores/package）
    Toolchain {
        #[command(subcommand)]
        command: toolchain::ToolchainCommands,
    },
    /// 清理构建产物：删除 build/，--all 时连带删除 target/
    Clean {
        /// 连带删除 <workspace 根>/target 编译产物目录
        #[arg(long)]
        all: bool,
    },
    /// 生成 rustdoc 文档并自动打开聚合首页
    ///
    /// 不带参数时排除全部平台 crate（只查通用层）；带 --platform 与
    /// --device（成对必填）时连带检查该平台
    Doc {
        /// 目标平台（如 tg5040）——指定则连带检查该平台
        #[arg(long, requires = "device")]
        platform: Option<String>,
        /// 设备参数（如 smart/brick；与 --platform 成对必填，无默认）
        #[arg(long, requires = "platform")]
        device: Option<String>,
    },
    /// 运行单元测试
    ///
    /// 不带参数时排除全部平台 crate（只测通用层）；带 --platform 与
    /// --device（成对必填）时连带测试该平台
    Test {
        /// 目标平台（如 tg5040）——指定则连带测试该平台
        #[arg(long, requires = "device")]
        platform: Option<String>,
        /// 设备参数（如 smart/brick；与 --platform 成对必填，无默认）
        #[arg(long, requires = "platform")]
        device: Option<String>,
    },
    /// 格式化 + Lint（cargo fmt + cargo clippy）
    ///
    /// 不带参数时排除全部平台 crate（只查通用层）；带 --platform 与
    /// --device（成对必填）时连带检查该平台
    Lint {
        /// 目标平台（如 tg5040）——指定则连带检查该平台
        #[arg(long, requires = "device")]
        platform: Option<String>,
        /// 设备参数（如 smart/brick；与 --platform 成对必填，无默认）
        #[arg(long, requires = "platform")]
        device: Option<String>,
    },
    /// 显示 xtask 的详细使用说明
    Help,
}

/// 程序入口：解析 CLI、分发执行、统一错误处理。
///
/// 错误信息打印到 stderr 并以退出码 1 结束（对齐 C 原版 makefile
/// 非零退出码语义）；clap 解析错误由 clap 自行处理（退出码 2）。
fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 顶层分发：按子命令调用对应模块的 `run()`。
///
/// # 参数
///
/// - `command`：解析出的顶层子命令
///
/// # 返回值
///
/// - `Ok(())`：命令执行成功
/// - `Err(String)`：命令执行失败——错误信息由各模块提供
fn dispatch(command: &Commands) -> Result<(), String> {
    match command {
        Commands::Toolchain { command } => toolchain::run(command),
        Commands::Clean { all } => clean::run(*all),
        // --device 有 clap `requires = "platform"` 约束:platform 为 None 时
        // device 必为 None(无参形态),platform 为 Some 时 device 必为 Some。
        // device 只在 platform 存在时被使用(aux_scope_args 按 platform 分支)
        Commands::Doc { platform, device } => {
            doc::run(platform.as_deref(), device.as_deref().unwrap_or(""))
        }
        Commands::Test { platform, device } => {
            test::run(platform.as_deref(), device.as_deref().unwrap_or(""))
        }
        Commands::Lint { platform, device } => {
            lint::run(platform.as_deref(), device.as_deref().unwrap_or(""))
        }
        Commands::Help => help::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toolchain_all_with_platform_and_device() {
        let cli = Cli::try_parse_from([
            "xtask",
            "toolchain",
            "all",
            "--platform",
            "tg5040",
            "--device",
            "brick",
        ])
        .expect("应能解析 toolchain all");
        match &cli.command {
            Commands::Toolchain {
                command: toolchain::ToolchainCommands::All { platform, device },
            } => {
                assert_eq!(platform, "tg5040");
                assert_eq!(device, "brick");
            }
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn build_requires_device() {
        // 设备 feature 必选（无默认）——缺 --device 解析失败
        assert!(
            Cli::try_parse_from(["xtask", "toolchain", "build", "--platform", "tg5040"]).is_err()
        );
        let cli = Cli::try_parse_from([
            "xtask",
            "toolchain",
            "build",
            "--platform",
            "tg5040",
            "--device",
            "smart",
        ])
        .expect("应能解析 toolchain build");
        match &cli.command {
            Commands::Toolchain {
                command: toolchain::ToolchainCommands::Build { platform, device },
            } => {
                assert_eq!(platform, "tg5040");
                assert_eq!(device, "smart");
            }
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn parses_platform_subcommand_without_platform_arg() {
        let cli = Cli::try_parse_from(["xtask", "toolchain", "setup"]).expect("setup 无需平台参数");
        assert!(matches!(
            &cli.command,
            Commands::Toolchain {
                command: toolchain::ToolchainCommands::Setup,
            }
        ));
    }

    #[test]
    fn build_requires_platform() {
        assert!(Cli::try_parse_from(["xtask", "toolchain", "build"]).is_err());
    }

    #[test]
    fn parses_helper_commands() {
        assert!(Cli::try_parse_from(["xtask", "doc"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "test"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "lint"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "help"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "clean"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "clean", "--all"]).is_ok());
        // 辅助命令带 --platform/--device（成对）
        assert!(
            Cli::try_parse_from(["xtask", "test", "--platform", "tg5040", "--device", "smart"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from(["xtask", "lint", "--platform", "tg5040", "--device", "brick"])
                .is_ok()
        );
    }

    #[test]
    fn parses_helper_platform_scope() {
        let cli =
            Cli::try_parse_from(["xtask", "test", "--platform", "tg5040", "--device", "smart"])
                .expect("test --platform --device 应可解析");
        match &cli.command {
            Commands::Test { platform, device } => {
                assert_eq!(platform.as_deref(), Some("tg5040"));
                assert_eq!(device.as_deref(), Some("smart"));
            }
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn helper_device_requires_platform() {
        // --device 不能单独出现（须与 --platform 成对，命令必须明确无默认）
        assert!(Cli::try_parse_from(["xtask", "test", "--device", "smart"]).is_err());
        assert!(Cli::try_parse_from(["xtask", "test", "--platform", "tg5040"]).is_err());
    }

    #[test]
    fn clean_flag_defaults_false() {
        let cli = Cli::try_parse_from(["xtask", "clean"]).expect("clean 无需参数");
        match &cli.command {
            Commands::Clean { all } => assert!(!all, "未加 --all 时 all 应为 false"),
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn clean_all_flag_parses_true() {
        let cli = Cli::try_parse_from(["xtask", "clean", "--all"]).expect("clean --all 应可解析");
        match &cli.command {
            Commands::Clean { all } => assert!(*all, "加 --all 时 all 应为 true"),
            other => panic!("解析结果错误: {other:?}"),
        }
    }
}
