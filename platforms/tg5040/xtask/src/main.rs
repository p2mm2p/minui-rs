//! tg5040 平台子 xtask——平台自治编排入口（替代原 package.sh）
//!
//! 承担 tg5040 平台侧的全部工作（父 xtask 只做通用二进制的编译/检测/
//! 职责边界」）：
//!
//! 1. **编译 show/keymon**：`cargo build -p platform-tg5040-show --features <device>`
//!    与 `-p platform-tg5040-keymon --features <device>`（device 决定 show 的
//!    分辨率——smart 1280×720 / brick 1024×768；keymon 逻辑不随 device 分叉
//!    但编译需选一个设备 feature）
//! 2. **编译 libretro.so**：在 `cores/` 执行 make（PLATFORM 经
//!    `UNION_PLATFORM=tg5040` 环境变量传递，对齐 C 原版）
//!    （克隆/编译进度透传，全流程必跑——无 libretro.so 发布包不可运行）
//! 3. **复制产物进发布包**（build 目录）：show → `SYSTEM/tg5040/bin/` 与
//!    `BOOT/common/tg5040/`、keymon → `SYSTEM/tg5040/bin/`、stock `.so` →
//!    `SYSTEM/tg5040/cores/`、extras `.so` → `EXTRAS/Emus/tg5040/<pak>/`
//! 4. **复制 install 资源**（原 package.sh 职责）：boot.sh/update.sh/安装图
//!    （安装图按 device 选源）
//! 5. **清理 cores 中间产物**（`cores-nuke` 子命令）：容器内执行
//!    `make nuke` 删除 `cores/src/`（上游克隆）与 `cores/output/`（编译
//!    产物），保留 makefile/patches/README 声明文件
//!
//! ## 调用契约
//!
//! 父 xtask 执行 `cargo run --quiet -p tg5040-xtask -- <device>`。build 目录
//! 与 target 依约定自定位（不传参）：
//! - build 目录：workspace 根的 `build/`（父 xtask setup 建立）
//! - target 目录：workspace 根的 `target/<triple>/release/`（tg5040 固定
//!   `aarch64-unknown-linux-gnu`）
//!

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};

/// 设备参数（smart/brick 二选一，对应平台 crate 的 device feature）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Smart,
    Brick,
}

impl Device {
    /// 解析设备参数（非法值返回 None——打印用法并非零退出）。
    fn parse(s: &str) -> Option<Device> {
        match s {
            "smart" => Some(Device::Smart),
            "brick" => Some(Device::Brick),
            _ => None,
        }
    }

    /// feature 名（传给 cargo build 的 --features）。
    fn feature(self) -> &'static str {
        match self {
            Device::Smart => "smart",
            Device::Brick => "brick",
        }
    }
}

/// tg5040 交叉编译 target（平台固定，不需要外部传入——你不能打包为其他架构）。
const TARGET: &str = "aarch64-unknown-linux-gnu";

/// 返回 workspace 根目录（`minui-rs/`）。
///
/// 原理：`env!("CARGO_MANIFEST_DIR")` 是本 crate 所在目录
/// （`<根>/platforms/tg5040/xtask`），上溯三级即 workspace 根
/// （xtask → tg5040 → platforms → 根）。
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR 上溯三级应为 workspace 根")
        .to_path_buf()
}

/// 平台目录（`<根>/platforms/tg5040`）。
fn platform_dir() -> PathBuf {
    project_root().join("platforms").join("tg5040")
}

/// 返回 build 目录（`<根>/build`）——父 xtask setup 复制 skeleton 到此。
fn build_dir() -> PathBuf {
    project_root().join("build")
}

/// 返回 show/keymon 的 cargo 产物目录（`<根>/target/<triple>/release/`）。
fn target_release_dir() -> PathBuf {
    project_root().join("target").join(TARGET).join("release")
}

/// 平台侧待编译的自治 bin crate 清单（show + keymon）。
const AUTONOMOUS_BINS: [&str; 2] = ["platform-tg5040-show", "platform-tg5040-keymon"];

/// 构造 `cargo build` 命令参数（可测纯函数）。
///
/// 产物形态：`cargo build -p <pkg> --features <device> --target aarch64-... --release`。
///
/// # 参数
///
/// - `pkg`：crate 名（如 `platform-tg5040-show`）
/// - `device`：设备参数（决定 `--features`）
///
/// # 返回值
///
/// cargo build 的参数列表。
fn build_command_args(pkg: &str, device: Device) -> Vec<String> {
    vec![
        "build".to_string(),
        "-p".to_string(),
        pkg.to_string(),
        "--features".to_string(),
        device.feature().to_string(),
        "--target".to_string(),
        TARGET.to_string(),
        "--release".to_string(),
    ]
}

/// 工具链容器镜像名（与 `toolchain/Dockerfile` 一致）。
const TOOLCHAIN_IMAGE: &str = "minui-toolchain:latest";

/// 返回容器引擎名（`MINUI_CONTAINER_ENGINE` 环境变量，默认 podman）。
fn container_engine() -> String {
    std::env::var("MINUI_CONTAINER_ENGINE")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "podman".to_string())
}

/// 构造工具链容器命令前缀（`podman run --rm -v <root>:/workspace -w /workspace <镜像>`）。
///
/// 编译（show/keymon 的 cargo build、cores 的 make）经工具链容器执行——
/// 挂载 workspace 根到容器 `/workspace`，产物经挂载落回宿主（target/ 与
/// cores/output/），后续装配复制仍读宿主路径。
fn container_prefix() -> Vec<String> {
    let mut args = vec!["run".to_string(), "--rm".to_string()];
    let root = project_root();
    let root_str = root.to_string_lossy().to_string();
    args.push("-v".to_string());
    args.push(format!("{root_str}:/workspace"));
    args.push("-w".to_string());
    args.push("/workspace".to_string());
    args.push(TOOLCHAIN_IMAGE.to_string());
    args
}

/// 执行 `cargo build`（stdout/stderr 透传——克隆/编译进度可见）。
///
/// 编译在工具链容器内执行（宿主机不直接跑面向 aarch64 的 cargo）：
/// `podman run ... minui-toolchain cargo build <args>`；`--target
/// aarch64-unknown-linux-gnu` 显式保留（容器本机即 aarch64，产物一致，
/// 落回宿主 `target/<triple>/release/`）。
///
/// # 参数
///
/// - `pkg`：crate 名（如 `platform-tg5040-show`）
/// - `device`：设备参数（决定 `--features`）
///
/// # 返回值
///
/// - `Ok(())`：编译成功
/// - `Err(String)`：容器引擎缺失或编译失败——错误信息含 stderr
fn cargo_build(pkg: &str, device: Device) -> Result<(), String> {
    println!(
        "==> 编译 {pkg}（--features {}，工具链容器内）",
        device.feature()
    );
    let mut cmd = Command::new(container_engine());
    cmd.args(container_prefix())
        .arg("cargo")
        .args(build_command_args(pkg, device));
    let status = cmd.status().map_err(|e| {
        format!(
            "容器引擎启动失败（{}，需 podman/docker）: {e}",
            container_engine()
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo build {pkg} 失败（退出码 {:?}）",
            status.code()
        ))
    }
}

/// 编译 show/keymon（平台自治 bin）。
///
/// # 参数
///
/// - `device`：设备参数
///
/// # 返回值
///
/// - `Ok(())`：两个 crate 编译成功
/// - `Err(String)`：编译失败
fn build_autonomous_bins(device: Device) -> Result<(), String> {
    for pkg in AUTONOMOUS_BINS {
        cargo_build(pkg, device)?;
    }
    Ok(())
}

/// 容器内 cores make 命令参数（对齐 C 原版调用方式）。
///
/// 容器 CWD 为 `/workspace`（挂载的 workspace 根），先 cd 到平台 cores
/// 目录再 make——平台 cores 目录经挂载即宿主 `platforms/tg5040/cores`。
///
/// **PLATFORM 经 UNION_PLATFORM 环境变量传递而非命令行参数**（对齐 C 原版
/// `support/setup-env.sh` 的 `export UNION_PLATFORM=tg5040` + `make`）：
/// make 顶层 `PLATFORM=$(UNION_PLATFORM)` 继承。若走命令行
/// `make PLATFORM=tg5040`，GNU make 会把命令行变量经 MAKEOVERRIDES
/// 泄漏给子 make，覆盖 Makefile.libretro 内部 `PLATFORM = libretro`，
/// 导致链接缺 libretro 前端（undefined reference）。环境变量不会泄漏，
/// 与 C 原版机制一致——子 make 内部赋值正常生效。
fn cores_make_command_args() -> Vec<String> {
    vec![
        "bash".to_string(),
        "-c".to_string(),
        "cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make".to_string(),
    ]
}

/// 容器内 cores 清理（`make nuke`）命令参数。
///
/// nuke 目标只清理 `src/`（上游克隆）与 `output/`（编译产物），不需要
/// PLATFORM——cores/makefile 已放行无 PLATFORM 的 nuke（见
/// `cores/makefile` 头部的 MAKECMDGOALS 判断），故无需 UNION_PLATFORM。
///
/// # 返回值
///
/// 命令参数列表（`bash -c "cd platforms/tg5040/cores && make nuke"`）。
fn cores_make_nuke_args() -> Vec<String> {
    vec![
        "bash".to_string(),
        "-c".to_string(),
        "cd platforms/tg5040/cores && make nuke".to_string(),
    ]
}

/// 编译 libretro.so（在工具链容器内执行 cores make，PLATFORM 经
/// `UNION_PLATFORM` 环境变量传递）。
///
/// 容器内执行 `cores_make_command_args()` 的命令——克隆（src/）与产物
/// （output/）落回宿主并跨调用保留。stdout/stderr 继承（透传 make 的
/// 逐核心克隆/编译进度）；容器引擎缺失或失败报错中止。
///
/// # 返回值
///
/// - `Ok(())`：cores 编译成功
/// - `Err(String)`：容器引擎缺失或 make 失败
fn build_cores() -> Result<(), String> {
    println!("==> 编译 libretro.so（工具链容器内 make，UNION_PLATFORM=tg5040）");
    let mut cmd = Command::new(container_engine());
    cmd.args(container_prefix()).args(cores_make_command_args());
    let status = cmd.status().map_err(|e| {
        format!(
            "容器引擎启动失败（{}，需 podman/docker）: {e}",
            container_engine()
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "make 编译 cores 失败（退出码 {:?}）",
            status.code()
        ))
    }
}

/// 清理 cores 中间产物（`src/` 克隆与 `output/` 编译产物，容器内 make nuke）。
///
/// 与 cores 编译同走工具链容器（复用 container_prefix），清理逻辑保持
/// 在 makefile 内（未来扩展清理范围时 xtask 自动跟随，宿主/容器不分叉）。
/// 只删 src/output，不触碰 cores 目录下的 makefile/patches/README 声明。
///
/// # 返回值
///
/// - `Ok(())`：清理完成
/// - `Err(String)`：容器引擎缺失或 make 失败
fn nuke_cores() -> Result<(), String> {
    println!("==> 清理 cores 中间产物（工具链容器内 make nuke）");
    let mut cmd = Command::new(container_engine());
    cmd.args(container_prefix()).args(cores_make_nuke_args());
    let status = cmd.status().map_err(|e| {
        format!(
            "容器引擎启动失败（{}，需 podman/docker）: {e}",
            container_engine()
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "make nuke 清理 cores 失败（退出码 {:?}）",
            status.code()
        ))
    }
}

/// 复制单个文件（创建目标目录）。
fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败（{parent:?}）: {e}"))?;
    }
    std::fs::copy(src, dst).map_err(|e| format!("复制失败（{src:?} → {dst:?}）: {e}"))?;
    Ok(())
}

/// 复制 show/keymon 二进制进发布包。
///
/// - `show` → `build/SYSTEM/tg5040/bin/` 与 `build/BOOT/common/tg5040/`
///   （boot.sh 的 `./show ./$ACTION.png` 需 show 与安装图同目录）
/// - `keymon` → `build/SYSTEM/tg5040/bin/`
///
/// 源产物缺失报错（完整性检测——替代原父 xtask system 的 show/keymon 检测）。
///
/// # 参数
///
/// - `build`：build 目录
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：产物缺失或复制失败
fn copy_autonomous_bins(build: &Path) -> Result<(), String> {
    copy_autonomous_bins_from(build, &target_release_dir())
}

/// 按给定产物目录复制 show/keymon（`copy_autonomous_bins` 的参数化版本，
/// 供测试用临时产物目录验证落点）。
fn copy_autonomous_bins_from(build: &Path, release: &Path) -> Result<(), String> {
    let system_bin = build.join("SYSTEM").join("tg5040").join("bin");
    let boot_common = build.join("BOOT").join("common").join("tg5040");

    for bin in ["show", "keymon"] {
        let src = release.join(bin);
        if !src.is_file() {
            return Err(format!(
                "编译产物缺失: {src:?}（请先执行 build 步骤编译 {bin}）"
            ));
        }
        copy_file(&src, &system_bin.join(bin))?;
        println!("已复制 {bin} → {}", system_bin.join(bin).display());
    }
    // show 需额外复制到 BOOT/common/tg5040/（boot.sh CWD 与安装图同目录）
    copy_file(&release.join("show"), &boot_common.join("show"))?;
    println!("已复制 show → {}", boot_common.join("show").display());
    Ok(())
}

/// 复制 stock 核心（6 个）到 `SYSTEM/tg5040/cores/`。
fn copy_stock_cores(build: &Path, cores_output: &Path) -> Result<(), String> {
    const STOCK: [&str; 6] = [
        "fceumm",
        "gambatte",
        "gpsp",
        "picodrive",
        "snes9x2005_plus",
        "pcsx_rearmed",
    ];
    let dst = build.join("SYSTEM").join("tg5040").join("cores");
    for core in STOCK {
        let so = format!("{core}_libretro.so");
        copy_file(&cores_output.join(&so), &dst.join(&so))?;
        println!("已复制 stock 核心 {so}");
    }
    Ok(())
}

/// extras 核心复制表（.so → 目标 pak，一核可多 pak）。
///
/// 对齐 C 原版 `MinUI/makefile` 的 `cores` 目标（tg5040 无 gkdpixel
/// 特例，全部复制）。
const EXTRAS_CORES: [(&str, &[&str]); 7] = [
    ("fake08_libretro.so", &["P8.pak"]),
    ("mgba_libretro.so", &["MGBA.pak", "SGB.pak"]),
    ("mednafen_pce_fast_libretro.so", &["PCE.pak"]),
    ("pokemini_libretro.so", &["PKM.pak"]),
    ("race_libretro.so", &["NGP.pak", "NGPC.pak"]),
    ("mednafen_supafaust_libretro.so", &["SUPA.pak"]),
    ("mednafen_vb_libretro.so", &["VB.pak"]),
];

/// 复制 extras 核心到 `EXTRAS/Emus/tg5040/<pak>/`。
fn copy_extras_cores(build: &Path, cores_output: &Path) -> Result<(), String> {
    let emus_dir = build.join("EXTRAS").join("Emus").join("tg5040");
    for (so, paks) in EXTRAS_CORES {
        for pak in paks {
            copy_file(&cores_output.join(so), &emus_dir.join(pak).join(so))?;
            println!("已复制 extras 核心 {so} → {pak}");
        }
    }
    Ok(())
}

/// 复制 cores 编译产物（stock + extras）进发布包。
fn copy_cores(build: &Path) -> Result<(), String> {
    let cores_output = platform_dir().join("cores").join("output");
    copy_stock_cores(build, &cores_output)?;
    copy_extras_cores(build, &cores_output)
}

/// 复制 install 资源（原 makefile.copy 职责）。
///
/// - `boot.sh` → `BOOT/common/tg5040.sh`
/// - `update.sh` → `SYSTEM/tg5040/bin/install.sh`
/// - 安装图按 device 选源（smart → `install/*.png`；brick →
///   `install/brick/*.png` 提升到 `BOOT/common/tg5040/`）
/// - `unzip` → `BOOT/common/tg5040/unzip`（boot.sh 的 `./unzip -o` 依赖——
///   aarch64 unzip 工具，来源见 `install/README.md`；special 后成为
///   `.tmp_update/tg5040/unzip`，与原版包结构一致）
///
/// # 参数
///
/// - `build`：build 目录
/// - `device`：设备参数（决定安装图来源）
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：复制失败
fn copy_install(build: &Path, device: Device) -> Result<(), String> {
    let install_dir = platform_dir().join("install");
    let boot_common = build.join("BOOT").join("common");

    // boot.sh → BOOT/common/tg5040.sh（bootloader 调用，成为 .tmp_update/tg5040.sh）
    copy_file(&install_dir.join("boot.sh"), &boot_common.join("tg5040.sh"))?;

    // update.sh → SYSTEM/tg5040/bin/install.sh（系统更新脚本）
    let system_bin = build.join("SYSTEM").join("tg5040").join("bin");
    copy_file(
        &install_dir.join("update.sh"),
        &system_bin.join("install.sh"),
    )?;

    // unzip → BOOT/common/tg5040/unzip（boot.sh 的 ./unzip -o 解压 MinUI.zip）
    let boot_tg5040 = boot_common.join("tg5040");
    copy_file(&install_dir.join("unzip"), &boot_tg5040.join("unzip"))?;
    println!("已复制 unzip → {}", boot_tg5040.join("unzip").display());

    // 安装图按 device 选源，放到 BOOT/common/tg5040/（boot.sh 的 ./$ACTION.png 同目录）
    let img_src = match device {
        Device::Smart => install_dir.clone(),
        Device::Brick => install_dir.join("brick"),
    };
    for img in ["installing.png", "updating.png"] {
        copy_file(&img_src.join(img), &boot_tg5040.join(img))?;
        println!(
            "已复制安装图 {img}（来源: {}）",
            img_src.join(img).display()
        );
    }
    Ok(())
}

/// 复制系统数据到 `SYSTEM/tg5040/dat/`（原 makefile.copy 职责）。
///
/// `runtrimui.sh` → `SYSTEM/tg5040/dat/runtrimui.sh`：源是 skeleton 的
/// `BOOT/trimui/app/runtrimui.sh`（TrimUI 官方启动器入口）。系统内的
/// `install/update.sh` 引用 `.system/tg5040/dat/runtrimui.sh` 做旧固件
/// 迁移（替换设备 `/usr/trimui/bin/runtrimui.sh`）——缺它迁移静默跳过。
///
/// # 参数
///
/// - `build`：build 目录
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：源缺失或复制失败
fn copy_dat(build: &Path) -> Result<(), String> {
    let src = build.join("BOOT").join("trimui").join("app").join("runtrimui.sh");
    let dst = build.join("SYSTEM").join("tg5040").join("dat").join("runtrimui.sh");
    copy_file(&src, &dst)?;
    println!("已复制 runtrimui.sh → {}", dst.display());
    Ok(())
}

/// 平台侧全流程:编译 show/keymon → 编译 cores → 复制全部产物。
///
/// # 参数
///
/// - `device`：设备参数
///
/// # 返回值
///
/// - `Ok(())`：全流程成功
/// - `Err(String)`：任一步骤失败——错误信息含具体步骤
fn run_all(device: Device) -> Result<(), String> {
    build_autonomous_bins(device)?;
    build_cores()?;
    let build = build_dir();
    copy_autonomous_bins(&build)?;
    copy_cores(&build)?;
    copy_install(&build, device)?;
    copy_dat(&build)?;
    println!("tg5040 平台打包完成 → {}", build.display());
    Ok(())
}

/// CLI 子命令（默认全流程）。
#[derive(Parser, Debug)]
#[command(name = "tg5040-xtask")]
#[command(about = "tg5040 平台自治打包（编译 show/keymon + cores + 装配复制）")]
struct Cli {
    /// 设备参数（smart 或 brick）
    device: String,
    /// 子命令（缺省执行全流程）
    #[command(subcommand)]
    command: Option<SubCommand>,
}

#[derive(Subcommand, Debug)]
enum SubCommand {
    /// 只编译 show/keymon
    Build,
    /// 只编译 libretro.so（cores make）
    Cores,
    /// 只复制产物与资源（show/keymon/.so/install 进 build/）
    Copy,
    /// 清理 cores 中间产物（src/ 克隆与 output/ 编译产物，make nuke）
    CoresNuke,
}

/// 只复制产物与资源（show/keymon/.so/install 进 build/）。
///
/// # 参数
///
/// - `device`：设备参数（决定安装图来源）
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：复制失败
fn copy_all(device: Device) -> Result<(), String> {
    let build = build_dir();
    copy_autonomous_bins(&build)?;
    copy_cores(&build)?;
    copy_install(&build, device)?;
    copy_dat(&build)
}

/// 程序入口:解析 CLI、执行、统一错误处理。
fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(device) = Device::parse(&cli.device) else {
        eprintln!("用法: tg5040-xtask <smart|brick> [build|cores|copy|cores-nuke]");
        eprintln!("  device 必须是 smart 或 brick");
        return ExitCode::FAILURE;
    };
    let result = match cli.command {
        None => run_all(device),
        Some(SubCommand::Build) => build_autonomous_bins(device),
        Some(SubCommand::Cores) => build_cores(),
        Some(SubCommand::Copy) => copy_all(device),
        Some(SubCommand::CoresNuke) => nuke_cores(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_parse_accepts_smart_and_brick() {
        assert_eq!(Device::parse("smart"), Some(Device::Smart));
        assert_eq!(Device::parse("brick"), Some(Device::Brick));
        assert_eq!(Device::parse("invalid"), None);
        assert_eq!(Device::parse(""), None);
    }

    #[test]
    fn device_feature_matches_name() {
        assert_eq!(Device::Smart.feature(), "smart");
        assert_eq!(Device::Brick.feature(), "brick");
    }

    #[test]
    fn project_root_resolves_to_workspace_root() {
        let root = project_root();
        assert!(root.ends_with("minui-rs"), "root = {root:?}");
        assert!(root.join("Cargo.toml").is_file(), "根应有 Cargo.toml");
    }

    #[test]
    fn platform_dir_points_to_tg5040() {
        assert!(platform_dir().ends_with("platforms/tg5040"));
    }

    #[test]
    fn build_dir_is_root_plus_build() {
        assert_eq!(build_dir(), project_root().join("build"));
    }

    #[test]
    fn target_release_dir_uses_fixed_triple() {
        let dir = target_release_dir();
        assert!(
            dir.ends_with("target/aarch64-unknown-linux-gnu/release"),
            "{dir:?}"
        );
    }

    #[test]
    fn copy_file_creates_parent_dirs() {
        let root =
            std::env::temp_dir().join(format!("tg5040-xtask-copyfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("src.txt");
        let dst = root.join("nested").join("dst.txt");
        std::fs::write(&src, "x").unwrap();
        copy_file(&src, &dst).expect("复制应成功");
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "x");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_autonomous_bins_fails_when_product_missing() {
        // 完整性检测:产物目录无 show/keymon → 报错（替代原 xtask system 检测）
        let root = std::env::temp_dir().join(format!("tg5040-xtask-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let build = root.join("build");
        let release = root.join("release");
        let err = copy_autonomous_bins_from(&build, &release).unwrap_err();
        assert!(err.contains("编译产物缺失"), "产物缺失应报错: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_autonomous_bins_places_show_twice_and_keymon_once() {
        // spec「show 复制到两处」「keymon 复制到 bin」
        let root = std::env::temp_dir().join(format!("tg5040-xtask-bin-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        let release = root.join("release");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join("show"), "show-bin").unwrap();
        std::fs::write(release.join("keymon"), "keymon-bin").unwrap();

        copy_autonomous_bins_from(&build, &release).expect("复制应成功");

        let system_bin = build.join("SYSTEM").join("tg5040").join("bin");
        let boot_tg5040 = build.join("BOOT").join("common").join("tg5040");
        assert_eq!(
            std::fs::read_to_string(system_bin.join("show")).unwrap(),
            "show-bin"
        );
        assert_eq!(
            std::fs::read_to_string(boot_tg5040.join("show")).unwrap(),
            "show-bin",
            "show 应复制到 BOOT/common/tg5040/"
        );
        assert_eq!(
            std::fs::read_to_string(system_bin.join("keymon")).unwrap(),
            "keymon-bin"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_stock_cores_places_six_sos() {
        // spec「stock 核心复制到 SYSTEM/cores」
        let root = std::env::temp_dir().join(format!("tg5040-xtask-stock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        let output = root.join("output");
        std::fs::create_dir_all(&output).unwrap();
        for core in [
            "fceumm",
            "gambatte",
            "gpsp",
            "picodrive",
            "snes9x2005_plus",
            "pcsx_rearmed",
        ] {
            std::fs::write(output.join(format!("{core}_libretro.so")), core).unwrap();
        }
        copy_stock_cores(&build, &output).expect("stock 复制应成功");
        let cores_dir = build.join("SYSTEM").join("tg5040").join("cores");
        for core in [
            "fceumm",
            "gambatte",
            "gpsp",
            "picodrive",
            "snes9x2005_plus",
            "pcsx_rearmed",
        ] {
            let so = format!("{core}_libretro.so");
            assert!(
                cores_dir.join(&so).is_file(),
                "{so} 应复制到 SYSTEM/tg5040/cores/"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_extras_cores_follows_mapping_table() {
        // spec「extras 核心按映射表复制」（含一核多 pak）
        let root = std::env::temp_dir().join(format!("tg5040-xtask-extras-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        let output = root.join("output");
        std::fs::create_dir_all(&output).unwrap();
        for (so, _) in EXTRAS_CORES {
            std::fs::write(output.join(so), so).unwrap();
        }
        copy_extras_cores(&build, &output).expect("extras 复制应成功");
        let emus = build.join("EXTRAS").join("Emus").join("tg5040");
        // 逐条断言映射表落点
        let expectations = [
            ("fake08_libretro.so", "P8.pak"),
            ("mgba_libretro.so", "MGBA.pak"),
            ("mgba_libretro.so", "SGB.pak"),
            ("mednafen_pce_fast_libretro.so", "PCE.pak"),
            ("pokemini_libretro.so", "PKM.pak"),
            ("race_libretro.so", "NGP.pak"),
            ("race_libretro.so", "NGPC.pak"),
            ("mednafen_supafaust_libretro.so", "SUPA.pak"),
            ("mednafen_vb_libretro.so", "VB.pak"),
        ];
        for (so, pak) in expectations {
            assert!(emus.join(pak).join(so).is_file(), "{so} 应复制到 {pak}/");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extras_cores_table_has_expected_paks() {
        // 验证复制表数据与 C 原版对齐（spec「extras 核心按映射表复制」）
        let paks_of = |so: &str| {
            EXTRAS_CORES
                .iter()
                .find(|(s, _)| *s == so)
                .map(|(_, p)| p)
                .expect("表中应有该核心")
        };
        assert_eq!(*paks_of("fake08_libretro.so"), ["P8.pak"]);
        assert_eq!(*paks_of("mgba_libretro.so"), ["MGBA.pak", "SGB.pak"]);
        assert_eq!(*paks_of("race_libretro.so"), ["NGP.pak", "NGPC.pak"]);
        assert_eq!(EXTRAS_CORES.len(), 7, "应有 7 个 extras 核心条目");
    }

    #[test]
    fn build_command_args_include_features_and_target() {
        // build 命令构造（spec「平台子 xtask 编译 show 与 keymon」）
        let args = build_command_args("platform-tg5040-show", Device::Brick);
        let joined = args.join(" ");
        assert!(joined.contains("build -p platform-tg5040-show"), "{joined}");
        assert!(joined.contains("--features brick"), "{joined}");
        assert!(
            joined.contains("--target aarch64-unknown-linux-gnu"),
            "{joined}"
        );
        assert!(joined.contains("--release"), "{joined}");

        let args = build_command_args("platform-tg5040-keymon", Device::Smart);
        let joined = args.join(" ");
        assert!(
            joined.contains("build -p platform-tg5040-keymon"),
            "{joined}"
        );
        assert!(joined.contains("--features smart"), "{joined}");
    }

    #[test]
    fn autonomous_bins_cover_show_and_keymon() {
        assert_eq!(
            AUTONOMOUS_BINS,
            ["platform-tg5040-show", "platform-tg5040-keymon"]
        );
    }

    #[test]
    fn cores_make_args_use_union_platform_env() {
        // cores 命令构造（spec「平台子 xtask 编译 libretro.so」——经容器）
        let args = cores_make_command_args();
        let joined = args.join(" ");
        assert!(joined.starts_with("bash -c "), "{joined}");
        assert!(
            joined.contains("cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make"),
            "{joined}"
        );
        // 治本：PLATFORM 经环境变量而非命令行参数——命令行 PLATFORM 会经
        // MAKEOVERRIDES 泄漏给子 make，覆盖 Makefile.libretro 内部
        // `PLATFORM = libretro`（对齐 C 原版 setup-env.sh 的机制）
        assert!(
            !joined.contains("make PLATFORM="),
            "PLATFORM 不得作命令行参数: {joined}"
        );
    }

    #[test]
    fn cores_make_nuke_args_need_no_union_platform() {
        // cores-nuke 命令构造（spec「平台子 xtask 暴露 cores 清理子命令」）：
        // nuke 只清理 src/output，不需要 PLATFORM——命令不含 UNION_PLATFORM
        //（cores/makefile 已放行无 PLATFORM 的 nuke）
        let args = cores_make_nuke_args();
        let joined = args.join(" ");
        assert!(joined.starts_with("bash -c "), "{joined}");
        assert!(
            joined.contains("cd platforms/tg5040/cores && make nuke"),
            "{joined}"
        );
        assert!(
            !joined.contains("UNION_PLATFORM"),
            "nuke 无需 UNION_PLATFORM: {joined}"
        );
    }

    #[test]
    fn cli_parses_cores_nuke_subcommand() {
        // CLI 解析（spec「xtask 清理入口」）：cores-nuke 是合法子命令
        use clap::Parser;
        let cli = Cli::try_parse_from(["tg5040-xtask", "smart", "cores-nuke"])
            .expect("cores-nuke 应可解析");
        assert!(matches!(cli.command, Some(SubCommand::CoresNuke)));
    }

    #[test]
    fn container_prefix_mounts_workspace_root() {
        // 容器前缀（spec「工具链容器编译」）：挂载 workspace 根、固定镜像
        let args = container_prefix();
        let joined = args.join(" ");
        assert!(joined.starts_with("run --rm "), "{joined}");
        assert!(
            joined.contains(&format!("{}:/workspace", project_root().display())),
            "应挂载 workspace 根: {joined}"
        );
        assert!(joined.contains("-w /workspace"), "{joined}");
        assert!(joined.contains(TOOLCHAIN_IMAGE), "{joined}");
        assert_eq!(args.len(), 7);
    }

    #[test]
    fn container_engine_configurable_default_podman() {
        // 引擎可配置（MINUI_CONTAINER_ENGINE，默认 podman）
        let old = std::env::var("MINUI_CONTAINER_ENGINE").ok();
        unsafe {
            std::env::set_var("MINUI_CONTAINER_ENGINE", "");
            assert_eq!(container_engine(), "podman");
            std::env::set_var("MINUI_CONTAINER_ENGINE", "docker");
            assert_eq!(container_engine(), "docker");
            match old {
                Some(v) => std::env::set_var("MINUI_CONTAINER_ENGINE", v),
                None => std::env::set_var("MINUI_CONTAINER_ENGINE", ""),
            }
        }
    }

    #[test]
    fn copy_install_smart_uses_root_images() {
        // spec「安装图按 device 选择」：smart → install/*.png（根目录）
        let root =
            std::env::temp_dir().join(format!("tg5040-xtask-install-smart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        copy_install(&build, Device::Smart).expect("install 复制应成功");
        let install = platform_dir().join("install");
        let boot_tg5040 = build.join("BOOT").join("common").join("tg5040");
        // boot.sh → BOOT/common/tg5040.sh
        assert!(
            same_content(
                &install.join("boot.sh"),
                &build.join("BOOT").join("common").join("tg5040.sh")
            ),
            "boot.sh 应复制为 tg5040.sh"
        );
        // update.sh → SYSTEM/tg5040/bin/install.sh
        assert!(
            same_content(
                &install.join("update.sh"),
                &build
                    .join("SYSTEM")
                    .join("tg5040")
                    .join("bin")
                    .join("install.sh")
            ),
            "update.sh 应复制为 install.sh"
        );
        // smart: 安装图来自 install/ 根目录
        assert!(
            same_content(
                &install.join("installing.png"),
                &boot_tg5040.join("installing.png")
            ),
            "smart 安装图应来自 install/ 根目录"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_install_places_unzip_next_to_boot_sh() {
        // spec「平台子 xtask 装配完整性」：unzip → BOOT/common/tg5040/
        // （boot.sh 的 ./unzip 依赖；special 后成 .tmp_update/tg5040/unzip）
        let root =
            std::env::temp_dir().join(format!("tg5040-xtask-unzip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        copy_install(&build, Device::Smart).expect("install 复制应成功");
        let install = platform_dir().join("install");
        assert!(
            same_content(
                &install.join("unzip"),
                &build.join("BOOT").join("common").join("tg5040").join("unzip")
            ),
            "unzip 应复制到 BOOT/common/tg5040/"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_dat_places_runtrimui_in_system_dat() {
        // spec「平台子 xtask 装配完整性」：BOOT/trimui/app/runtrimui.sh →
        // SYSTEM/tg5040/dat/runtrimui.sh（update.sh 旧固件迁移依赖）
        let root = std::env::temp_dir().join(format!("tg5040-xtask-dat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        // 源 = build/BOOT/trimui/app/runtrimui.sh（setup 从 skeleton 复制）
        let src = build.join("BOOT").join("trimui").join("app").join("runtrimui.sh");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "#!/bin/sh\n").unwrap();

        copy_dat(&build).expect("dat 复制应成功");

        let dst = build.join("SYSTEM").join("tg5040").join("dat").join("runtrimui.sh");
        assert!(dst.is_file(), "runtrimui.sh 应复制到 SYSTEM/tg5040/dat/");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_install_brick_uses_brick_subdir_images() {
        // spec「安装图按 device 选择」：brick → install/brick/*.png（提升到包根）
        let root =
            std::env::temp_dir().join(format!("tg5040-xtask-install-brick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        copy_install(&build, Device::Brick).expect("install 复制应成功");
        let install = platform_dir().join("install");
        let boot_tg5040 = build.join("BOOT").join("common").join("tg5040");
        assert!(
            same_content(
                &install.join("brick").join("installing.png"),
                &boot_tg5040.join("installing.png")
            ),
            "brick 安装图应来自 install/brick/ 子目录"
        );
        assert!(
            same_content(
                &install.join("brick").join("updating.png"),
                &boot_tg5040.join("updating.png")
            ),
            "brick updating 图应来自 install/brick/ 子目录"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 比较两文件内容是否一致（测试辅助）。
    fn same_content(a: &Path, b: &Path) -> bool {
        std::fs::read(a).ok() == std::fs::read(b).ok()
    }
}
