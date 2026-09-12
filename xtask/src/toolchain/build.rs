//! `build` 步骤模块
//!
//! 编译通用平台二进制：minui/minarch/clock/minput（要求 --platform）。
//! show/keymon 是平台自治 bin（独立 crate），由平台子 xtask 编译——
//! 本步骤不涉及。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `build` 目标（经 `makefile.toolchain`
//! 的 docker toolchain 交叉编译）。Rust 版直接调用 `cargo build` 并启用
//! 平台 feature + 交叉编译 target。
//!
//! 平台→target 映射表 xtask 内置（非写死单一 target）——12 平台跨 3 种
//! ARM 架构 + macOS host（探索核验 `makefile.env` 的 ARCH 定义，见
//! 直接交叉编译：环境缺 ARM linker 会报错——环境问题，由执行环境自备
//! 工具链（项目不做开发环境适配）。
//!
//! feature 传递（平台 feature 系统规范——见
//! minput 均用 `--features <platform>/<device>`（依赖 key = 包名
//! `platform-<code>`，无 rename，`dep/feat` 语法生效——平台 lib 作为依赖
//! 被连带编译，无需单独 `-p`）。
//! 设备 feature 必选且无默认（compile_error 断言强制），xtask 的
//! `--device` 参数必填。
//!

/// 平台→target 映射（xtask 内置）。
///
/// 依据 C 原版各平台 `platform/makefile.env` 的 ARCH 定义（探索核验）：
/// rg35xx/trimuismart 是 `-marm -march=armv7-a`（32 位硬浮点），其余
/// ARM 平台是 `-march=armv8`/`-mtune=cortex-a5x`（64 位 aarch64）。
///
/// 每个受支持平台都有确定的交叉 target（无 host 平台）——macos 是
/// C 原版的 dummy 开发辅助平台而非发布目标（见
/// `macos/notes.txt`（原版开发辅助平台）），Rust 版不登记。
///
/// # 参数
///
/// - `platform`：平台名（如 `tg5040`）
///
/// # 返回值
///
/// 交叉编译 target triple（恒有值）。
pub fn platform_target(platform: &str) -> &'static str {
    match platform {
        // 64 位 ARM（armv8-a / aarch64）
        "tg5040" | "zero28" | "m17" | "gkdpixel" | "miyoomini" | "rg35xxplus" | "my282"
        | "magicmini" | "my355" | "rgb30" => "aarch64-unknown-linux-gnu",
        // 32 位 ARM（armv7-a 硬浮点）
        "rg35xx" | "trimuismart" => "armv7-unknown-linux-gnueabihf",
        // 未知平台——由 validate_platform_target 先行拦截（此处不兜底）
        _ => unreachable!("未知平台应在 validate_platform_target 处报错"),
    }
}

/// 校验平台是否有对应 target 映射（未知平台报错）。
///
/// # 参数
///
/// - `platform`：平台名
///
/// # 返回值
///
/// - `Ok(())`：平台已支持
/// - `Err(String)`：未知平台——错误信息列出已支持平台
fn validate_platform_target(platform: &str) -> Result<(), String> {
    const SUPPORTED: [&str; 12] = [
        "tg5040",
        "zero28",
        "m17",
        "gkdpixel",
        "miyoomini",
        "rg35xxplus",
        "my282",
        "magicmini",
        "my355",
        "rgb30",
        "rg35xx",
        "trimuismart",
    ];
    if SUPPORTED.contains(&platform) {
        Ok(())
    } else {
        Err(format!(
            "未知平台: {platform}（已支持: {}）",
            SUPPORTED.join(", ")
        ))
    }
}

/// 构造 build 命令（可测纯函数）。
///
/// 返回 4 条 cargo build 命令的 `(程序, 参数)` 列表：
/// minui、minarch、clock、minput（均 `--features <platform>/<device>`，
/// 通用工具但需实例化平台）。平台 lib（`platform-<platform>`）不单独编译
/// ——show/keymon 已拆为独立 crate（平台自治，由平台子 xtask 编译），
/// 平台 lib 作为 minui/minarch 的依赖被连带编译（编译 minui 时经
/// `--features platform-tg5040/<device>` 拉入）。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`，同时也是上层依赖 key（platform-<code>）与平台 crate 名后缀）
/// - `device`：设备参数（如 `smart`/`brick`）
///
/// # 返回值
///
/// `(程序, 参数)` 列表。
fn build_commands(platform: &str, device: &str) -> Vec<(&'static str, Vec<String>)> {
    let target = platform_target(platform);
    let mut cmds = Vec::new();

    for pkg in ["minui", "minarch", "clock", "minput"] {
        let feature_flag = format!("platform-{platform}/{device}");
        // 每个平台都有确定的交叉 target——恒带 --target
        let args = vec![
            "build".to_string(),
            "-p".to_string(),
            pkg.to_string(),
            "--features".to_string(),
            feature_flag,
            "--target".to_string(),
            target.to_string(),
            "--release".to_string(),
        ];
        cmds.push(("cargo", args));
    }
    cmds
}

/// 执行 `build` 步骤：编译 minui/minarch/clock/minput（平台 feature + target）。
///
/// 编译经**工具链容器**执行（见 `toolchain/Dockerfile`）——宿主机不直接跑面向
/// aarch64 的 cargo；命令为
/// `podman run --rm -v <root>:/workspace -w /workspace minui-toolchain
/// cargo build ...`，产物经挂载落回宿主 `target/<triple>/release/`。
/// 引擎经 `MINUI_CONTAINER_ENGINE` 配置（默认 podman）。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`）
/// - `device`：设备参数（如 `smart`/`brick`，决定编译 feature）
///
/// # 返回值
///
/// - `Ok(())`：四个 crate 编译成功
/// - `Err(String)`：未知平台、编译失败——错误信息含 stderr
pub fn run(platform: &str, device: &str) -> Result<(), String> {
    validate_platform_target(platform)?;
    let engine = crate::utils::container_engine();
    let prefix = crate::utils::container_prefix();
    for (_, args) in build_commands(platform, device) {
        let mut full: Vec<&str> = prefix.iter().map(String::as_str).collect();
        full.push("cargo");
        full.extend(args.iter().map(String::as_str));
        crate::utils::run_command(&engine, &full)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_maps_aarch64_platforms() {
        for p in [
            "tg5040",
            "zero28",
            "m17",
            "gkdpixel",
            "miyoomini",
            "rg35xxplus",
            "my282",
            "magicmini",
            "my355",
            "rgb30",
        ] {
            assert_eq!(
                platform_target(p),
                "aarch64-unknown-linux-gnu",
                "{p} 应为 aarch64"
            );
        }
    }

    #[test]
    fn target_maps_armv7_platforms() {
        assert_eq!(platform_target("rg35xx"), "armv7-unknown-linux-gnueabihf");
        assert_eq!(
            platform_target("trimuismart"),
            "armv7-unknown-linux-gnueabihf"
        );
    }

    #[test]
    fn build_commands_include_target_and_features() {
        let cmds = build_commands("tg5040", "brick");
        assert_eq!(
            cmds.len(),
            4,
            "通用二进制 4 条（minui/minarch/clock/minput）"
        );
        // minui：--features platform-tg5040/brick + --target aarch64 + --release
        let (program, args) = &cmds[0];
        assert_eq!(program, &"cargo");
        let joined = args.join(" ");
        assert!(joined.contains("build -p minui"), "{joined}");
        assert!(
            joined.contains("--features platform-tg5040/brick"),
            "{joined}"
        );
        assert!(
            joined.contains("--target aarch64-unknown-linux-gnu"),
            "{joined}"
        );
        assert!(joined.contains("--release"), "{joined}");
        // minarch：与 minui 同模式
        let (_, args) = &cmds[1];
        let joined = args.join(" ");
        assert!(joined.contains("build -p minarch"), "{joined}");
        assert!(
            joined.contains("--features platform-tg5040/brick"),
            "{joined}"
        );
        // clock：--features platform-tg5040/brick（与 minui 相同的透传）+ --target + --release
        let (_, args) = &cmds[2];
        let joined = args.join(" ");
        assert!(joined.contains("build -p clock"), "{joined}");
        assert!(
            joined.contains("--features platform-tg5040/brick"),
            "{joined}"
        );
        assert!(
            joined.contains("--target aarch64-unknown-linux-gnu"),
            "{joined}"
        );
        assert!(joined.contains("--release"), "{joined}");
        // minput：--features platform-tg5040/brick（与 clock 相同的透传）+ --target + --release
        let (_, args) = &cmds[3];
        let joined = args.join(" ");
        assert!(joined.contains("build -p minput"), "{joined}");
        assert!(
            joined.contains("--features platform-tg5040/brick"),
            "{joined}"
        );
        assert!(
            joined.contains("--target aarch64-unknown-linux-gnu"),
            "{joined}"
        );
        assert!(joined.contains("--release"), "{joined}");
        // 全部命令不得含 platform crate 的独立编译（show/keymon 平台自治）
        for (_, args) in &cmds {
            let joined = args.join(" ");
            assert!(
                !joined.contains("-p platform-tg5040"),
                "不应独立编译平台 crate: {joined}"
            );
        }
    }

    #[test]
    fn validate_rejects_unknown_platform() {
        let err = validate_platform_target("nonexistent").unwrap_err();
        assert!(err.contains("未知平台"));
        assert!(err.contains("tg5040"));
        // macos 不是受支持平台——报错信息不得列出
        assert!(!err.contains("macos"), "macos 不应在已支持列表: {err}");
    }

    #[test]
    fn validate_rejects_macos() {
        let err = validate_platform_target("macos").unwrap_err();
        assert!(err.contains("未知平台"), "macos 应被拒: {err}");
        // "已支持" 列表段不得含 macos（macos 不在 12 平台中）
        let supported = err.split("已支持:").nth(1).unwrap_or("");
        assert!(
            !supported.contains("macos"),
            "已支持列表不应含 macos: {err}"
        );
    }
}
