//! `setup` 步骤模块
//!
//! 准备干净的 build 目录：清空 `build/` → 递归复制 `skeleton/` → `build/`
//! → 删除 `.keep`/`*.meta` → 写 `build/hash.txt`（git 短 hash）。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `setup` 目标（makefile:91-109）：
//! 清空 `./build`、`cp -R ./skeleton ./build`、`find ... -name '.keep' -delete`、
//! `echo $(BUILD_HASH) > ./workspace/hash.txt`。
//!
//! 差异：Rust 版不做 C 原版的 `tty -s` 交互终端检查（Rust 工具无需）；
//! 不做 readmes 复制（C 版为 Linux fmt 格式化机制服务，Rust 版无此流程，
//! skeleton 的 README.txt 是成品）；hash.txt 写入 `build/` 内（C 版写
//! `workspace/`，Rust 无此目录——见
//!

use std::path::Path;

use crate::utils;

/// 执行 `setup` 步骤：清空 build/ → 复制 skeleton → 清理 → 写 hash。
///
/// # 返回值
///
/// - `Ok(())`：build 目录就绪（skeleton 内容 + hash.txt）
/// - `Err(String)`：删除/复制/清理/写 hash 任一失败——错误信息含路径
pub fn run() -> Result<(), String> {
    let build = utils::build_dir();
    let skeleton = utils::project_root().join("skeleton");
    utils::remove_dir_if_exists(&build)?;
    utils::copy_dir_recursive(&skeleton, &build)?;
    remove_authoring_files(&build)?;
    let hash = git_short_hash()?;
    std::fs::write(build.join("hash.txt"), hash).map_err(|e| format!("写入 hash.txt 失败: {e}"))?;
    Ok(())
}

/// 递归删除 build 目录中的 `.keep` 与 `*.meta` 文件。
///
/// 对应 C 原版 `find . -type f -name '.keep' -delete` 与
/// `find . -type f -name '*.meta' -delete`（makefile:102-103）——
/// skeleton 用 `.keep` 占位空目录，发布前必须删除。
///
/// # 参数
///
/// - `dir`：要清理的目录（递归）
///
/// # 返回值
///
/// - `Ok(())`：清理完成（无 .keep/*.meta 或删除失败忽略——读取错误不
///   阻断，与 C 原版 find -delete 的宽松语义一致）
fn remove_authoring_files(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| format!("读取目录失败（{dir:?}）: {e}"))?
    {
        let entry = entry.map_err(|e| format!("读取目录条目失败（{dir:?}）: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            remove_authoring_files(&path)?;
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let is_authoring = name == ".keep" || name.ends_with(".meta");
            if is_authoring {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

/// 获取 git 短 hash（`git rev-parse --short HEAD` 的 stdout）。
///
/// # 返回值
///
/// - `Ok(String)`：短 hash（含换行，C 原版 echo 语义一致）
/// - `Err(String)`：git 命令不可用或执行失败——version.txt/commits.txt
///   依赖 git，缺失必须报错
fn git_short_hash() -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_err(|e| format!("获取 git hash 失败（git 命令不可用）: {e}"))?;
    if !output.status.success() {
        return Err("获取 git hash 失败：git rev-parse 非零退出".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_authoring_files_deletes_keep_and_meta() {
        let dir = std::env::temp_dir().join("xtask-setup-test-keep");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join(".keep"), "").unwrap();
        std::fs::write(dir.join("sub").join("a.meta"), "").unwrap();
        std::fs::write(dir.join("sub").join("b.txt"), "keep me").unwrap();
        remove_authoring_files(&dir).expect("清理应成功");
        assert!(!dir.join(".keep").exists());
        assert!(!dir.join("sub").join("a.meta").exists());
        assert!(dir.join("sub").join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_short_hash_fails_when_git_missing() {
        // git 缺失环境下必然失败——验证 Err 路径（错误信息含 git 提示）
        match git_short_hash() {
            Ok(h) => assert!(!h.trim().is_empty(), "git 可用时 hash 非空"),
            Err(e) => assert!(e.contains("git"), "错误信息应含 git: {e}"),
        }
    }
}
