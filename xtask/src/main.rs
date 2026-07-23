//! # xtask — MinUI 构建工具
//!
//! 使用方式: `cargo xtask <command> --platform <name>`
//!
//! ## 命令
//!
//! - `list-platforms` — 列出支持的平台
//! - `build --platform <name>` — 交叉编译 + 打包 MinUI.zip
//! - `build-debug --platform <name>` — 同上，debug 构建

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ============================================================================
// 平台定义
// ============================================================================

struct Platform {
    tag: &'static str,
    name: &'static str,
    rust_target: &'static str,
    cargo_feature: &'static str,
    pkg: &'static str,
}

const PLATFORMS: &[Platform] = &[
    Platform { tag: "rg35xxplus", name: "Anbernic RG35XX Plus/H/SP/40XX/CubeXX/34XX",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-rg35xxplus",
        pkg: "platform-rg35xxplus" },
    Platform { tag: "rg35xx", name: "Anbernic RG35XX (original)",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-rg35xx",
        pkg: "platform-rg35xx" },
    Platform { tag: "miyoomini", name: "Miyoo Mini / Mini Plus",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-miyoomini",
        pkg: "platform-miyoomini" },
    Platform { tag: "trimuismart", name: "Trimui Smart",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-trimuismart",
        pkg: "platform-trimuismart" },
    Platform { tag: "tg5040", name: "Trimui Smart Pro / Brick",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-tg5040",
        pkg: "platform-tg5040" },
    Platform { tag: "rgb30", name: "Powkiddy RGB30",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-rgb30",
        pkg: "platform-rgb30" },
    Platform { tag: "m17", name: "M17",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-m17",
        pkg: "platform-m17" },
    Platform { tag: "my282", name: "Miyoo A30",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-my282",
        pkg: "platform-my282" },
    Platform { tag: "my355", name: "Miyoo Flip",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-my355",
        pkg: "platform-my355" },
    Platform { tag: "magicmini", name: "MagicX XU Mini M",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-magicmini",
        pkg: "platform-magicmini" },
    Platform { tag: "zero28", name: "MagicX Mini Zero 28",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-zero28",
        pkg: "platform-zero28" },
    Platform { tag: "gkdpixel", name: "GKD Pixel",
        rust_target: "armv7-unknown-linux-gnueabihf", cargo_feature: "platform-gkdpixel",
        pkg: "platform-gkdpixel" },
];

// ============================================================================
// 路径
// ============================================================================

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn binary_path(pkg: &str, target: &str, release: bool) -> PathBuf {
    let profile = if release { "release" } else { "debug" };
    project_root().join("target").join(target).join(profile).join(pkg)
}

// ============================================================================
// 工具
// ============================================================================

fn run_cmd(cmd: &mut Command, desc: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{}: {}", desc, e))?;
    if !status.success() { return Err(format!("{} failed (exit {:?})", desc, status.code())); }
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {:?}: {}", dst, e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("read_dir {:?}: {}", src, e))? {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| format!("copy {:?}: {}", src_path, e))?;
        }
    }
    Ok(())
}

fn cargo_build(pkg: &str, target: &str, feature: &str, release: bool) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&project_root());
    cmd.args(["build", "-p", pkg, "--target", target, "--features", feature]);
    if release { cmd.arg("--release"); }
    run_cmd(&mut cmd, &format!("build {}", pkg))
}

fn cargo_build_bin(pkg: &str, bin: &str, target: &str, feature: &str, release: bool) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&project_root());
    cmd.args(["build", "-p", pkg, "--bin", bin, "--target", target, "--features", feature]);
    if release { cmd.arg("--release"); }
    run_cmd(&mut cmd, &format!("build {}/{}", pkg, bin))
}

// ============================================================================
// 命令：编译 + 打包
// ============================================================================

fn cmd_build(platform_tag: &str, release: bool) -> Result<(), String> {
    let p = PLATFORMS.iter().find(|x| x.tag == platform_tag)
        .ok_or_else(|| format!("Unknown platform: {}. Use 'list-platforms'.", platform_tag))?;

    let project = project_root();
    let sk = project.join("skeleton");

    // ── 1. 编译 ──
    println!("╔════════════════════════════════════╗");
    println!("║  Building MinUI — {:16} ║", p.tag);
    println!("╚════════════════════════════════════╝\n");

    println!("[1/2] Compiling for {}...", p.rust_target);
    cargo_build("minui-launcher", p.rust_target, p.cargo_feature, release)?;
    cargo_build("minarch", p.rust_target, p.cargo_feature, release)?;
    cargo_build_bin(p.pkg, "keymon", p.rust_target, p.cargo_feature, release)?;

    // ── 2. 打包 ──
    println!("\n[2/2] Packaging...");

    // 在 target 下创建临时 staging 目录（不碰 skeleton）
    let staging = project.join("target").join("minui-package").join(p.tag);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| format!("clean staging: {}", e))?;
    }

    // 复制 skeleton → staging
    for dir in &["BASE", "BOOT", "EXTRAS"] {
        let src = sk.join(dir);
        if src.exists() { copy_dir(&src, &staging.join(dir))?; }
    }
    let sys_platform = format!("SYSTEM/{}", p.tag);
    if sk.join(&sys_platform).exists() { copy_dir(&sk.join(&sys_platform), &staging.join(&sys_platform))?; }
    if sk.join("SYSTEM/res").exists() { copy_dir(&sk.join("SYSTEM/res"), &staging.join("SYSTEM/res"))?; }

    // 放入编译产物
    let bin_dir = staging.join("SYSTEM").join(p.tag).join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir bin: {}", e))?;
    fs::copy(binary_path("minui-launcher", p.rust_target, release), bin_dir.join("minui"))
        .map_err(|e| format!("copy minui: {}", e))?;
    fs::copy(binary_path("minarch", p.rust_target, release), bin_dir.join("minarch"))
        .map_err(|e| format!("copy minarch: {}", e))?;
    fs::copy(binary_path("keymon", p.rust_target, release), bin_dir.join("keymon"))
        .map_err(|e| format!("copy keymon: {}", e))?;

    // zip
    let zip_path = project.join(format!("MinUI-{}.zip", p.tag));
    let mut cmd = Command::new("zip");
    cmd.current_dir(&staging).arg("-r").arg(&zip_path).arg(".");
    run_cmd(&mut cmd, "zip")?;

    let size = fs::metadata(&zip_path).map(|m| m.len() as f64 / 1048576.0).unwrap_or(0.0);
    println!("\n✅ MinUI-{}.zip  ({:.1} MB)", p.tag, size);
    println!("   Note: libretro cores NOT included. Put them in:");
    println!("   skeleton/SYSTEM/{}/cores/", p.tag);
    Ok(())
}

fn cmd_list_platforms() {
    println!("Supported platforms:\n");
    for p in PLATFORMS {
        println!("  {:16}  {}", p.tag, p.name);
    }
    println!("\nUsage: cargo xtask build --platform <tag>");
}

// ============================================================================
// main
// ============================================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { return print_usage(); }

    let result = match args[1].as_str() {
        "list-platforms" => { cmd_list_platforms(); Ok(()) }
        "build" => cmd_build(&parse_platform(&args)?, true),
        "build-debug" => cmd_build(&parse_platform(&args)?, false),
        _ => { println!("Unknown: {}", args[1]); print_usage(); Ok(()) }
    };

    if let Err(e) = result { eprintln!("\n❌ {}", e); std::process::exit(1); }
}

fn parse_platform(args: &[String]) -> Result<String, String> {
    args.iter().position(|a| a == "--platform")
        .and_then(|i| args.get(i + 1).cloned())
        .ok_or_else(|| "Usage: cargo xtask build --platform <name>".into())
}

fn print_usage() {
    println!("MinUI xtask\n");
    println!("Usage: cargo xtask build --platform <name>\n");
    println!("Commands:");
    println!("  list-platforms              List supported platforms");
    println!("  build --platform <name>     Cross-compile + package MinUI.zip");
    println!("  build-debug --platform <name>  Same, debug build\n");
    println!("Example: cargo xtask build --platform rg35xxplus");
}
