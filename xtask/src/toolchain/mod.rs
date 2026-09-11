//! toolchain 子命令组模块
//!
//! 构建流水线：对齐 C 原版顶层 makefile（`MinUI/makefile`）的
//! setup/special/tidy/cores/build/system/package 目标。
//! 本模块定义 `ToolchainCommands` 枚举（clap 子命令组）并分发到各步骤模块。
//!
//! 各步骤职责（详见各模块文档）：
//!
//! | 子命令 | 对应 C 原版 | --platform | --device |
//! |--------|-------------|-----------|----------|
//! | all    | make PLATFORM=x | 必填 | 必填（设备 feature 必选，无默认） |
//! | setup  | setup | 不要求 | 不要求 |
//! | special| special | 不要求 | 不要求 |
//! | tidy   | tidy | 必填 | 不要求 |
//! | build  | build | 必填 | 必填（设备 feature 必选，无默认） |
//! | system | system | 必填 | 不要求 |
//! | platform | cores（改名） | 必填 | 必填（透传平台子 xtask） |
//! | package| package | 必填 | 必填（发布名含平台/设备段） |
//!

mod all;
mod build;
mod package;
mod platform;
mod setup;
mod special;
mod system;
mod tidy;

use clap::Subcommand;

/// toolchain 子命令组。
#[derive(Subcommand, Debug)]
pub enum ToolchainCommands {
    /// 完整流水线: setup → build → system → platform → special → tidy → package
    All {
        /// 目标平台（如 tg5040）
        #[arg(long)]
        platform: String,
        /// 设备参数（如 smart/brick——设备 feature 必选，无默认）
        #[arg(long)]
        device: String,
    },
    /// 准备干净的 build 目录并复制 skeleton（全局步骤）
    Setup,
    /// 处理 BOOT 目录重命名（全局步骤）
    Special,
    /// 兼容旧卡（复制新平台 install.sh 到旧平台位置）
    Tidy {
        /// 目标平台（如 tg5040）
        #[arg(long)]
        platform: String,
    },
    /// 编译通用二进制（minui/minarch/clock/minput）
    Build {
        /// 目标平台（如 tg5040）
        #[arg(long)]
        platform: String,
        /// 设备参数（如 smart/brick——决定编译 feature，设备 feature 必选）
        #[arg(long)]
        device: String,
    },
    /// 复制二进制到 `build/SYSTEM/<platform>/bin/`
    System {
        /// 目标平台（如 tg5040）
        #[arg(long)]
        platform: String,
    },
    /// 调用平台子 xtask 完成平台侧全流程（原 C cores 步骤，含 show/keymon/cores）
    Platform {
        /// 目标平台（如 tg5040）
        #[arg(long)]
        platform: String,
        /// 设备参数（如 smart/brick，透传给平台子 xtask）
        #[arg(long)]
        device: String,
    },
    /// 打包发布（生成 version.txt/commits.txt + zip；发布名含平台与设备段，
    /// 故要求 --platform 与 --device）
    Package {
        /// 目标平台（如 tg5040）
        #[arg(long)]
        platform: String,
        /// 设备参数（如 smart/brick，进入发布名区分产物）
        #[arg(long)]
        device: String,
    },
}

/// toolchain 分发入口：按子命令调用各步骤模块的 `run()`。
///
/// # 参数
///
/// - `command`：解析出的 toolchain 子命令
///
/// # 返回值
///
/// - `Ok(())`：步骤执行成功
/// - `Err(String)`：步骤执行失败——错误信息由各步骤模块提供
pub fn run(command: &ToolchainCommands) -> Result<(), String> {
    match command {
        ToolchainCommands::All { platform, device } => all::run(platform, device),
        ToolchainCommands::Setup => setup::run(),
        ToolchainCommands::Special => special::run(),
        ToolchainCommands::Tidy { platform } => tidy::run(platform),
        ToolchainCommands::Build { platform, device } => build::run(platform, device),
        ToolchainCommands::System { platform } => system::run(platform),
        ToolchainCommands::Platform { platform, device } => platform::run(platform, device),
        ToolchainCommands::Package { platform, device } => package::run(platform, device),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Result<ToolchainCommands, clap::Error> {
        let mut full = vec!["xtask", "toolchain"];
        full.extend_from_slice(args);
        let cli = crate::Cli::try_parse_from(full)?;
        match cli.command {
            crate::Commands::Toolchain { command } => Ok(command),
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn parses_all_variants() {
        assert!(parse(&["all", "--platform", "tg5040", "--device", "smart"]).is_ok());
        assert!(parse(&["setup"]).is_ok());
        assert!(parse(&["special"]).is_ok());
        assert!(parse(&["tidy", "--platform", "tg5040"]).is_ok());
        assert!(parse(&["build", "--platform", "tg5040", "--device", "brick"]).is_ok());
        assert!(parse(&["system", "--platform", "tg5040"]).is_ok());
        assert!(parse(&["platform", "--platform", "tg5040", "--device", "smart"]).is_ok());
        assert!(parse(&["package", "--platform", "tg5040", "--device", "smart"]).is_ok());
    }

    #[test]
    fn platform_required_variants_fail_without_platform() {
        assert!(parse(&["all", "--device", "smart"]).is_err());
        assert!(parse(&["tidy"]).is_err());
        assert!(parse(&["build", "--device", "brick"]).is_err());
        assert!(parse(&["system"]).is_err());
        assert!(parse(&["platform", "--device", "smart"]).is_err());
        assert!(parse(&["package"]).is_err());
        assert!(parse(&["package", "--platform", "tg5040"]).is_err());
    }

    #[test]
    fn global_variants_parse_without_platform() {
        assert!(parse(&["setup"]).is_ok());
        assert!(parse(&["special"]).is_ok());
    }

    #[test]
    fn package_requires_platform_and_device() {
        // package 发布名含平台/设备段（build-release-pipeline 决策 10），
        // 不再是全局步骤——platform/device 必填
        let cmd = parse(&["package", "--platform", "tg5040", "--device", "brick"])
            .expect("package 应接受 --platform --device");
        match cmd {
            ToolchainCommands::Package { platform, device } => {
                assert_eq!(platform, "tg5040");
                assert_eq!(device, "brick");
            }
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn platform_variant_accepts_device() {
        let cmd = parse(&["platform", "--platform", "tg5040", "--device", "brick"])
            .expect("platform 应接受 --device");
        match cmd {
            ToolchainCommands::Platform { platform, device } => {
                assert_eq!(platform, "tg5040");
                assert_eq!(device, "brick");
            }
            other => panic!("解析结果错误: {other:?}"),
        }
    }

    #[test]
    fn build_requires_device() {
        // 设备 feature 必选（无默认）——缺 --device 解析失败
        assert!(parse(&["build", "--platform", "tg5040"]).is_err());
        let cmd =
            parse(&["build", "--platform", "tg5040", "--device", "smart"]).expect("build 应可解析");
        match cmd {
            ToolchainCommands::Build { platform, device } => {
                assert_eq!(platform, "tg5040");
                assert_eq!(device, "smart");
            }
            other => panic!("解析结果错误: {other:?}"),
        }
    }
}
