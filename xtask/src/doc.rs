//! `doc` 命令模块
//!
//! 生成 rustdoc 文档并自动打开首页，两种形态（命令必须明确，无默认值）：
//!
//! - `cargo xtask doc`：排除**全部**平台相关 crate（`platforms/` 下自动
//!   扫描——新增平台零维护），只生成通用层文档
//! - `cargo xtask doc --platform <p> --device <d>`：`--platform` 与
//!   `--device` **成对必填**（无默认），排除**其他**平台 crate、保留该
//!   平台 + 通用层，并加 `--features <p>/<d>` 连带生成该平台文档
//!   （device feature 是编译期二选一，平台 lib 无默认，必须显式指定）
//!
//! 流程：清理旧文档目录（防 crates.js 残留）→ `cargo doc --workspace
//! --no-deps [范围参数]` → 读 `target/doc/crates.js` 的 `ALL_CRATES`
//! 自建聚合首页 `target/doc/index.html`（纯 workspace 不生成根
//! index.html）→ 跨平台打开首页。
//!
//! 对应 C 原版：无直接对应（Rust 项目新增的辅助命令）。
//! 决策 3（聚合首页自建）、决策 4（手写打开浏览器）。
//!

use std::path::Path;

use crate::utils;

/// 执行 `doc` 命令：生成文档 → 自建聚合首页 → 自动打开。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`；None = 只生成通用层文档）
/// - `device`：设备参数（platform 为 Some 时必填，调用方保证成对）
///
/// # 返回值
///
/// - `Ok(())`：文档生成成功且首页已生成（打开失败也会返回 Err，见下）
/// - `Err(String)`：`cargo doc` 失败、crates.js 缺失、或首页打开失败——
///   打开失败的错误信息含路径提示，便于手动打开
pub(crate) fn run(platform: Option<&str>, device: &str) -> Result<(), String> {
    if let Some(p) = platform {
        utils::validate_platform(p)?;
    }
    let doc_dir = utils::project_root().join("target").join("doc");
    clean_doc_dir(&doc_dir)?;
    let mut args = vec![
        "doc".to_string(),
        "--workspace".to_string(),
        "--no-deps".to_string(),
    ];
    args.extend(utils::aux_scope_args(platform, device));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    utils::run_command("cargo", &args)?;
    let crates = utils::list_crate_names(&doc_dir)?;
    write_index_page(&doc_dir, &crates)?;
    let index = doc_dir.join("index.html");
    utils::open_browser(&index)
}

/// 清理 `target/doc/` 下的旧 crate 文档目录。
///
/// 必要性：`crates.js` 是 rustdoc **每次运行**的写入物——内容为
/// "本次文档化的 crate 列表"。增量构建（源码无变化）不触发 rustdoc
/// 重跑，文件保持**上一次运行**的内容。若历史上有过不带 `--no-deps`
/// 的构建（或 `-p <crate>` 单包构建），crates.js 与目录会残留
/// 依赖 crate（adler2 等）或旧 crate 的文档——聚合首页将列出不存在的
/// 链接。
///
/// 清理范围：删除 `doc_dir` 下所有**非共享资源**目录
/// （`search.index`/`src`/`static.files`/`trait.impl` 由 rustdoc 重建），
/// 让 cargo doc 全量重建——crates.js 与目录永远一致。
/// 代价：每次 doc 全量重建（workspace 5 个 crate，可接受）。
///
/// # 参数
///
/// - `doc_dir`：`target/doc/` 目录路径
///
/// # 返回值
///
/// - `Ok(())`：清理完成（目录不存在也视为成功）
/// - `Err(String)`：读取或删除失败
fn clean_doc_dir(doc_dir: &Path) -> Result<(), String> {
    // rustdoc 生成的共享资源目录名——删除 crate 文档时必须保留
    const SHARED: [&str; 4] = ["search.index", "src", "static.files", "trait.impl"];
    if !doc_dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(doc_dir).map_err(|e| format!("读取 target/doc 失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取 target/doc 条目失败: {e}"))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if !SHARED.contains(&name_str.as_str()) && entry.path().is_dir() {
            std::fs::remove_dir_all(entry.path())
                .map_err(|e| format!("清理旧文档目录失败（{}）: {e}", name_str))?;
        }
    }
    Ok(())
}

/// 生成聚合首页 `index.html`（列出全部 crate 的链接）。
///
/// 纯 workspace 的 `cargo doc` 不生成根 `index.html`（rustdoc 只生成
/// 各 crate 的 `index.html` + `crates.js`）。本函数生成一个简单聚合页，
/// 使浏览器打开 `target/doc/index.html` 即可看到全部 crate。
/// 已实测验证：cargo doc 增量构建不会覆盖手动写入的 index.html。
///
/// # 参数
///
/// - `doc_dir`：`target/doc/` 目录路径
/// - `crates`：crate 名列表（来自 `list_crate_names`）
///
/// # 返回值
///
/// - `Ok(())`：首页写入成功
/// - `Err(String)`：写入失败（如权限、doc_dir 不存在）
fn write_index_page(doc_dir: &Path, crates: &[String]) -> Result<(), String> {
    let links: String = crates
        .iter()
        .map(|c| format!("<li><a href=\"{c}/index.html\">{c}</a></li>"))
        .collect();
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh">
<head><meta charset="utf-8"><title>MinUI (Rust) — workspace 文档</title></head>
<body>
<h1>MinUI (Rust) — workspace 文档</h1>
<ul>
{links}
</ul>
</body>
</html>
"#
    );
    std::fs::write(doc_dir.join("index.html"), html).map_err(|e| format!("写入聚合首页失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_index_page_contains_all_crate_links() {
        let dir = std::env::temp_dir().join("xtask-doc-test-index");
        std::fs::create_dir_all(&dir).unwrap();
        let crates = vec![
            "common".to_string(),
            "render".to_string(),
            "xtask".to_string(),
        ];
        write_index_page(&dir, &crates).expect("应能写入首页");
        let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
        for c in &crates {
            assert!(
                html.contains(&format!("<a href=\"{c}/index.html\">{c}</a>")),
                "首页缺少 crate 链接: {c}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_index_page_fails_when_dir_missing() {
        let dir = std::env::temp_dir().join("xtask-doc-test-missing");
        let _ = std::fs::remove_dir_all(&dir);
        let err = write_index_page(&dir, &["common".to_string()]).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn clean_doc_dir_removes_crate_dirs_keeps_shared() {
        let dir = std::env::temp_dir().join("xtask-doc-test-clean");
        let _ = std::fs::remove_dir_all(&dir);
        // 模拟残留：依赖 crate 目录 + workspace crate 目录 + 共享资源目录
        std::fs::create_dir_all(dir.join("adler2")).unwrap();
        std::fs::create_dir_all(dir.join("common")).unwrap();
        std::fs::create_dir_all(dir.join("search.index")).unwrap();
        std::fs::create_dir_all(dir.join("static.files")).unwrap();
        clean_doc_dir(&dir).expect("清理应成功");
        assert!(!dir.join("adler2").exists(), "依赖 crate 目录应被清理");
        assert!(!dir.join("common").exists(), "workspace crate 目录应被清理");
        assert!(dir.join("search.index").exists(), "共享资源目录应保留");
        assert!(dir.join("static.files").exists(), "共享资源目录应保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_doc_dir_ok_when_missing() {
        let dir = std::env::temp_dir().join("xtask-doc-test-clean-missing");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(clean_doc_dir(&dir).is_ok(), "目录不存在视为成功");
    }
}
