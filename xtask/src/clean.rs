//! `clean` 命令模块
//!
//! 删除构建产物目录：默认删除 `build/`，`--all` 时连带删除 `<workspace 根>/target`。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `clean` 目标（makefile:88-89）：
//! `rm -rf ./build`。Rust 版用 `std::fs::remove_dir_all` 实现——
//! 目录不存在视为成功（对齐 `rm -rf` 宽松语义）。不做平台分支
//! （项目不做开发环境适配）。
//!
//! `target/` 是 Rust 重写版新增的清理项（C 版无编译产物目录），`--all` 时才删除。
//!
//! 删除成功后打印被清理目录路径，便于审计；不做交互确认（保持脚本化
//! 可用，CI 中 `clean && all` 无需人工介入）。
//!

use std::path::Path;

use crate::utils;

/// 执行 `clean` 命令：删除 build/（`--all` 时连带删除 target/）。
///
/// 实际路径来自 `utils::build_dir()` 与 `project_root().join("target")`
/// （编译期固定指向 workspace），删除行为见 `run_with_paths`。
///
/// # 参数
///
/// - `all`：是否连带删除 `<workspace 根>/target` 编译产物目录
///
/// # 返回值
///
/// - `Ok(())`：所有目标目录已删除（或不存在）
/// - `Err(String)`：删除失败——错误信息含路径
pub(crate) fn run(all: bool) -> Result<(), String> {
    run_with_paths(
        all,
        &utils::build_dir(),
        &utils::project_root().join("target"),
    )
}

/// 按给定路径执行清理（`run` 的路径参数化版本，供测试用临时目录验证）。
///
/// # 参数
///
/// - `all`：是否连带删除 `target` 目录
/// - `build`：build 目录路径
/// - `target`：target 目录路径
///
/// # 返回值
///
/// - `Ok(())`：所有目标目录已删除（或不存在）
/// - `Err(String)`：任一目录删除失败——错误信息含路径，立即中止不再
///   继续删除后续目录
fn run_with_paths(all: bool, build: &Path, target: &Path) -> Result<(), String> {
    remove_and_report(build, "build")?;
    if all {
        remove_and_report(target, "target")?;
    }
    Ok(())
}

/// 删除单个目录并打印结果。
///
/// 目录存在则整体删除并打印 `已清理: <路径>`；不存在则打印
/// `<名称> 目录不存在,跳过`（不视为错误）。删除失败返回含路径的错误。
///
/// # 参数
///
/// - `dir`：要删除的目录路径
/// - `name`：目录的展示名（如 `build`/`target`，用于不存在的提示语）
///
/// # 返回值
///
/// - `Ok(())`：目录已删除或不存在
/// - `Err(String)`：删除失败——错误信息含路径
fn remove_and_report(dir: &Path, name: &str) -> Result<(), String> {
    if dir.is_dir() {
        utils::remove_dir_if_exists(dir)?;
        println!("已清理: {}", dir.to_string_lossy());
    } else {
        println!("{name} 目录不存在,跳过");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造临时目录树：root/{build, target}（target 可选）。
    fn setup(
        root_name: &str,
        with_target: bool,
    ) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(root_name);
        let _ = std::fs::remove_dir_all(&root);
        let build = root.join("build");
        let target = root.join("target");
        std::fs::create_dir_all(build.join("sub")).unwrap();
        if with_target {
            std::fs::create_dir_all(&target).unwrap();
        }
        (root, build, target)
    }

    #[test]
    fn run_false_removes_build_only() {
        let (root, build, target) = setup("xtask-clean-test-build-only", true);
        run_with_paths(false, &build, &target).expect("删除应成功");
        assert!(!build.exists(), "build 应被删除");
        assert!(target.exists(), "未加 --all 时 target 不应被删除");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn run_all_removes_build_and_target() {
        let (root, build, target) = setup("xtask-clean-test-all", true);
        run_with_paths(true, &build, &target).expect("删除应成功");
        assert!(!build.exists(), "build 应被删除");
        assert!(!target.exists(), "target 应被删除");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn run_missing_dirs_are_ok() {
        let (root, build, target) = setup("xtask-clean-test-missing", false);
        let _ = std::fs::remove_dir_all(&build);
        run_with_paths(true, &build, &target).expect("目录不存在应视为成功");
        let _ = std::fs::remove_dir_all(&root);
    }
}
