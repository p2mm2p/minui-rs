//! `system` 步骤模块
//!
//! 复制通用二进制进发布包（要求 --platform）。show/keymon 是平台自治 bin
//! （独立 crate），由平台子 xtask 编译装配——不在此步骤职责内。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `system` 目标中"复制二进制"部分
//! （makefile:49-56）；makefile.copy 复制平台特有文件的部分已归入
//! `platform` 步骤（平台子 xtask 负责）。
//!
//! 复制落点对齐 C 原版（发布包结构对照见 build-release-pipeline 决策 9）：
//! 1. **minui/minarch** → `build/SYSTEM/<platform>/bin/`（系统主程序）
//! 2. **clock/minput** → `build/EXTRAS/Tools/<platform>/{Clock,Input}.pak/`
//!    （工具 pak——C 原版 makefile:55-56 把 clock.elf/minput.elf 复制进
//!    Tools pak 而非 SYSTEM/bin：pak 的 launch.sh 以 `./clock` 相对引用，
//!    EXTRAS 可独立分发）
//!
//! 职责边界：**纯复制，不承担完整性检测**——源产物缺失（build 步骤未
//! 执行）硬性报错；复制动作结束后还有 `platform` 步骤（show/keymon/cores/
//! install 继续往发布包复制），因此**发布包完整性的唯一验收点是流水线
//! 末尾的 `package` 步骤**（`package.rs` 的 `check_release_integrity`
//! 全量校验）——中途不做局部断言，避免「system 已验收」的错觉与清单
//! 重复维护。
//!

use std::path::Path;

use crate::toolchain::build::platform_target;
use crate::utils;

/// 进 `SYSTEM/<platform>/bin/` 的通用二进制（系统主程序，对齐 C 原版
/// makefile:52-53 的 minui.elf/minarch.elf）。
const SYSTEM_BINARIES: [&str; 2] = ["minui", "minarch"];

/// 进 `EXTRAS/Tools/<platform>/<pak>/` 的工具二进制（对齐 C 原版
/// makefile:55-56 的 clock.elf → Clock.pak、minput.elf → Input.pak）。
const TOOLS_PAKS: [(&str, &str); 2] = [("clock", "Clock.pak"), ("minput", "Input.pak")];

/// 执行 `system` 步骤：复制通用二进制到 bin/ 与 Tools pak。
///
/// 不承担发布包完整性检测——那是 `package` 步骤的职责（全流程所有
/// 复制动作结束后的唯一验收点）。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`）
///
/// # 返回值
///
/// - `Ok(())`：二进制已复制
/// - `Err(String)`：源产物缺失（提示先 build）或复制失败
pub fn run(platform: &str) -> Result<(), String> {
    let build = utils::build_dir();
    let target_dir = cargo_target_dir(platform);

    // 1. minui/minarch → SYSTEM/<platform>/bin/
    let bin_dir = build.join("SYSTEM").join(platform).join("bin");
    copy_binaries(&target_dir, &bin_dir, &SYSTEM_BINARIES)?;

    // 2. clock/minput → EXTRAS/Tools/<platform>/<pak>/
    let tools_root = build.join("EXTRAS").join("Tools").join(platform);
    for (bin, pak) in TOOLS_PAKS {
        copy_binaries(&target_dir, &tools_root.join(pak), &[bin])?;
    }
    Ok(())
}

/// 复制一组二进制到目标目录（源产物缺失硬性报错）。
///
/// # 参数
///
/// - `target_dir`：cargo 产物目录（`target/<triple>/release/`）
/// - `dst_dir`：目标目录（如 `build/SYSTEM/<platform>/bin/` 或 Tools pak 目录）
/// - `names`：要复制的二进制名列表
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：产物缺失或复制失败
fn copy_binaries(target_dir: &Path, dst_dir: &Path, names: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dst_dir).map_err(|e| format!("创建目录失败（{dst_dir:?}）: {e}"))?;
    for bin in names {
        let src = target_dir.join(bin);
        if !src.is_file() {
            return Err(format!("编译产物缺失: {src:?}（请先执行 build 步骤）"));
        }
        std::fs::copy(&src, dst_dir.join(bin))
            .map_err(|e| format!("复制失败（{src:?} → {dst_dir:?}）: {e}"))?;
    }
    Ok(())
}

/// 返回 cargo 产物目录（`target/<triple>/release/`）。
///
/// 每个平台都有确定的交叉 target——产物恒在 `target/<triple>/release/`
/// （无 host 产物目录）。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`）
///
/// # 返回值
///
/// cargo 产物目录路径。
fn cargo_target_dir(platform: &str) -> std::path::PathBuf {
    let target = platform_target(platform);
    utils::project_root()
        .join("target")
        .join(target)
        .join("release")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bin_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-system-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cargo_target_dir_uses_triple_for_arm() {
        let dir = cargo_target_dir("tg5040");
        assert!(
            dir.ends_with("target/aarch64-unknown-linux-gnu/release"),
            "{dir:?}"
        );
    }

    #[test]
    fn copy_binaries_copies_given_names() {
        let src = make_bin_dir("copy-src");
        let dst = make_bin_dir("copy-dst");
        for b in ["minui", "minarch"] {
            std::fs::write(src.join(b), format!("bin-{b}")).unwrap();
        }
        copy_binaries(&src, &dst, &["minui", "minarch"]).expect("复制应成功");
        assert_eq!(
            std::fs::read_to_string(dst.join("minui")).unwrap(),
            "bin-minui"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("minarch")).unwrap(),
            "bin-minarch"
        );
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn copy_binaries_fails_when_product_missing() {
        let src = make_bin_dir("prod-missing");
        let dst = make_bin_dir("prod-missing-dst");
        let err = copy_binaries(&src, &dst, &["minui"]).unwrap_err();
        assert!(err.contains("编译产物缺失"), "应报产物缺失: {err}");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn copy_binaries_preserves_existing_skeleton_files() {
        // 纯复制语义：dst 模拟 setup 后 skeleton bin/（setterm/shutdown），
        // 复制后原有文件保留
        let src = make_bin_dir("preserve-src");
        let dst = make_bin_dir("preserve-dst");
        for b in SYSTEM_BINARIES {
            std::fs::write(src.join(b), format!("bin-{b}")).unwrap();
        }
        std::fs::write(dst.join("setterm"), "x").unwrap();
        std::fs::write(dst.join("shutdown"), "x").unwrap();

        copy_binaries(&src, &dst, &SYSTEM_BINARIES).expect("复制应成功");
        for b in SYSTEM_BINARIES {
            assert!(dst.join(b).is_file(), "{b} 应被复制");
        }
        assert!(dst.join("setterm").is_file(), "skeleton 文件应保留");
        assert!(dst.join("shutdown").is_file(), "skeleton 文件应保留");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn tools_place_in_clock_and_input_paks() {
        // 落点对齐（spec「system 装配职责」）：clock → Clock.pak、minput →
        // Input.pak（C 原版 makefile:55-56），不进 SYSTEM/bin
        let src = make_bin_dir("tools-src");
        let build = make_bin_dir("tools-build");
        for b in SYSTEM_BINARIES.into_iter().chain(
            TOOLS_PAKS.into_iter().map(|(bin, _)| bin),
        ) {
            std::fs::write(src.join(b), format!("bin-{b}")).unwrap();
        }

        let bin_dir = build.join("SYSTEM").join("tg5040").join("bin");
        copy_binaries(&src, &bin_dir, &SYSTEM_BINARIES).expect("bin 复制应成功");

        let tools_root = build.join("EXTRAS").join("Tools").join("tg5040");
        for (bin, pak) in TOOLS_PAKS {
            copy_binaries(&src, &tools_root.join(pak), &[bin]).expect("pak 复制应成功");
            assert!(
                tools_root.join(pak).join(bin).is_file(),
                "{bin} 应复制到 {pak}/"
            );
            assert!(
                !bin_dir.join(bin).exists(),
                "{bin} 不应在 SYSTEM/bin（C 原版 bin 无 clock/minput）"
            );
        }
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&build);
    }
}
