//! `tidy` 步骤模块
//!
//! 兼容旧卡：复制新平台 install.sh 到旧平台位置（要求 --platform）。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `tidy` 目标（makefile:131-141）：
//! 按 PLATFORMS 列表条件复制——tg5040 → `SYSTEM/tg3040/paks/MinUI.pak/launch.sh`
//! （`SYSTEM/tg5040/bin/install.sh` 的副本，使旧卡能正常更新）、
//! rg35xxplus → rg40xxcube。
//!
//! 差异：C 原版按 `PLATFORMS` 列表（一次构建多个平台）条件执行；Rust 版
//! 单平台流水线按 `--platform` 分支（scaffold 已定：tidy 要求 --platform）。
//! 源文件缺失报错（install.sh 由 platform 步骤的平台子 xtask 生成，流水线
//! 顺序保证其在 tidy 前就位）。
//!

use std::path::Path;

use crate::utils;

/// 执行 `tidy` 步骤：按平台复制旧卡兼容文件。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`）
///
/// # 返回值
///
/// - `Ok(())`：兼容文件复制完成（当前平台无 tidy 分支也视为成功）
/// - `Err(String)`：源文件缺失或复制失败
pub fn run(platform: &str) -> Result<(), String> {
    let build = utils::build_dir();
    apply_tidy(&build, platform)
}

/// tidy 复制逻辑（可测纯函数，接受 build 目录路径）。
///
/// # 参数
///
/// - `build`：build 目录路径
/// - `platform`：目标平台（`tg5040`/`rg35xxplus` 有分支，其余无）
///
/// # 返回值
///
/// - `Ok(())`：兼容文件复制完成（无分支平台视为成功）
/// - `Err(String)`：源文件缺失或复制失败
fn apply_tidy(build: &Path, platform: &str) -> Result<(), String> {
    match platform {
        // C 原版 makefile:138-141：tg5040 → tg3040 旧卡兼容
        "tg5040" => {
            let src = build
                .join("SYSTEM")
                .join("tg5040")
                .join("bin")
                .join("install.sh");
            let dst_dir = build
                .join("SYSTEM")
                .join("tg3040")
                .join("paks")
                .join("MinUI.pak");
            copy_install(&src, &dst_dir, "launch.sh")
        }
        // C 原版 makefile:134-137：rg35xxplus → rg40xxcube 旧卡兼容
        "rg35xxplus" => {
            let src = build
                .join("SYSTEM")
                .join("rg35xxplus")
                .join("bin")
                .join("install.sh");
            let dst_dir = build.join("SYSTEM").join("rg40xxcube").join("bin");
            copy_install(&src, &dst_dir, "install.sh")
        }
        // 其他平台无 tidy 分支——C 原版同样只处理两个平台
        _ => Ok(()),
    }
}

/// 复制 install.sh 到目标目录（改名 dst_name）。
///
/// # 参数
///
/// - `src`：源 install.sh 路径
/// - `dst_dir`：目标目录（不存在则创建）
/// - `dst_name`：目标文件名（launch.sh 或 install.sh）
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：源缺失（错误信息提示平台步骤顺序）或复制失败
fn copy_install(src: &Path, dst_dir: &Path, dst_name: &str) -> Result<(), String> {
    if !src.is_file() {
        return Err(format!(
            "源文件缺失: {src:?}（install.sh 由 platform 步骤的平台子 xtask 生成，请确认流水线顺序）"
        ));
    }
    std::fs::create_dir_all(dst_dir).map_err(|e| format!("创建目录失败（{dst_dir:?}）: {e}"))?;
    let dst = dst_dir.join(dst_name);
    std::fs::copy(src, &dst).map_err(|e| format!("复制失败（{src:?} → {dst:?}）: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 每个测试唯一目录名——cargo test 并行执行，共享路径会互相踩踏
    fn make_build(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-tidy-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("SYSTEM")).unwrap();
        dir
    }

    #[test]
    fn tidy_tg5040_copies_install_to_tg3040_launch() {
        let build = make_build("tg5040");
        std::fs::create_dir_all(build.join("SYSTEM").join("tg5040").join("bin")).unwrap();
        std::fs::write(
            build
                .join("SYSTEM")
                .join("tg5040")
                .join("bin")
                .join("install.sh"),
            "#!/bin/sh\nupdate",
        )
        .unwrap();

        apply_tidy(&build, "tg5040").expect("tidy 应成功");

        let dst = build
            .join("SYSTEM")
            .join("tg3040")
            .join("paks")
            .join("MinUI.pak")
            .join("launch.sh");
        assert!(dst.is_file(), "tg3040/launch.sh 应存在");
        assert_eq!(
            std::fs::read_to_string(&dst).unwrap(),
            "#!/bin/sh\nupdate",
            "内容应与 install.sh 相同"
        );
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn tidy_tg5040_fails_when_source_missing() {
        let build = make_build("tg5040-missing");
        let err = apply_tidy(&build, "tg5040").unwrap_err();
        assert!(err.contains("源文件缺失"), "应报源缺失: {err}");
        assert!(err.contains("平台子 xtask"), "应提示平台步骤: {err}");
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn tidy_rg35xxplus_copies_install_to_rg40xxcube() {
        let build = make_build("rg35xxplus");
        std::fs::create_dir_all(build.join("SYSTEM").join("rg35xxplus").join("bin")).unwrap();
        std::fs::write(
            build
                .join("SYSTEM")
                .join("rg35xxplus")
                .join("bin")
                .join("install.sh"),
            "#!/bin/sh\nupdate",
        )
        .unwrap();

        apply_tidy(&build, "rg35xxplus").expect("tidy 应成功");

        let dst = build
            .join("SYSTEM")
            .join("rg40xxcube")
            .join("bin")
            .join("install.sh");
        assert!(dst.is_file(), "rg40xxcube/install.sh 应存在");
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn tidy_unknown_platform_is_ok() {
        let build = make_build("unknown");
        assert!(apply_tidy(&build, "zero28").is_ok(), "无分支平台视为成功");
        let _ = std::fs::remove_dir_all(&build);
    }
}
