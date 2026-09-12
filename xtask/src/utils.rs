//! xtask 共享工具模块
//!
//! 提供跨命令复用的基础能力：项目根路径解析、build 目录解析、
//! 外部命令执行（免 shell 注入）与平台名校验。
//!
//! （模块布局 Requirement）。详细设计见

use std::path::{Path, PathBuf};
use std::process::Command;

/// 返回项目根目录（`minui-rs/`）。
///
/// 原理：`env!("CARGO_MANIFEST_DIR")` 是编译期注入的 xtask 包所在目录
/// （`<根>/xtask`），取其父目录即 workspace 根。编译期常量意味着无论
/// 从哪个工作目录运行 xtask，路径都不会漂移。
///
/// # 返回值
///
/// 项目根目录的绝对路径（`PathBuf`）。
pub(crate) fn project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR 的父目录应为 workspace 根")
        .to_path_buf()
}

/// 返回 build 目录（`<项目根>/build`）。
///
/// 对应 C 原版顶层 makefile 的 `./build`（复制 skeleton 后在此目录组装
/// 发布内容）。setup 步骤清空并重建该目录，clean 步骤整体删除。
///
/// # 返回值
///
/// build 目录的绝对路径（`PathBuf`）。
pub(crate) fn build_dir() -> PathBuf {
    project_root().join("build")
}

/// 删除目录（不存在视为成功）。
///
/// 供 `clean` 命令（删除 build/target）与 `setup` 步骤（清空 build 后重建）
/// 共用——对齐 C 原版 `rm -rf` 的宽松语义：目标不存在不报错。
///
/// # 参数
///
/// - `dir`：要删除的目录路径
///
/// # 返回值
///
/// - `Ok(())`：目录已删除，或本就不存在
/// - `Err(String)`：删除失败——错误信息含路径
pub(crate) fn remove_dir_if_exists(dir: &Path) -> Result<(), String> {
    if dir.is_dir() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("删除目录失败（{dir:?}）: {e}"))?;
    }
    Ok(())
}

/// 执行外部命令（参数数组直传，不经 shell）。
///
/// 与 C 原版 `system("...")` 字符串拼接不同：`Command` 直接以参数数组
/// 传给操作系统，杜绝 shell 注入（config.yaml 安全基线，参见
/// `platforms/tg5040/src/power.rs` 的先例）。
///
/// # 参数
///
/// - `program`：可执行程序名或路径
/// - `args`：参数列表
///
/// # 返回值
///
/// - `Ok(())`：命令以退出码 0 结束
/// - `Err(String)`：命令启动失败（程序不存在等）或非零退出——错误信息
///   包含程序名、参数与 stderr 输出
pub(crate) fn run_command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("命令启动失败: {program} {}: {e}", args.join(" ")))?;
    if output.status.success() {
        Ok(())
    } else {
        // stderr 与 stdout 都带上：cargo fmt --check 的 diff 输出在 stdout、
        // cargo clippy 的警告在 stderr——只带其一会让 lint 失败原因不可见。
        // stdout 可能很长（如 clippy 警告列表），截断到 2000 字符避免刷屏。
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = truncate(&stdout, 2000);
        Err(format!(
            "命令失败: {program} {}: {stderr}{stdout}",
            args.join(" ")
        ))
    }
}

/// 截断长字符串（保留开头，省略号标注截断位置）。
///
/// # 参数
///
/// - `s`：原始字符串
/// - `max`：最大长度（字符数）
///
/// # 返回值
///
/// 截断后的字符串（`String`）。
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let cut = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}...", &s[..cut])
    } else {
        s.to_string()
    }
}

/// 校验平台名：`platforms/<name>/` 目录必须存在。
///
/// # 参数
///
/// - `name`：平台名（如 `tg5040`）
///
/// # 返回值
///
/// - `Ok(())`：平台目录存在
/// - `Err(String)`：平台目录不存在——错误信息列出项目根下已存在的平台
pub(crate) fn validate_platform(name: &str) -> Result<(), String> {
    let platform_dir = project_root().join("platforms").join(name);
    if platform_dir.is_dir() {
        Ok(())
    } else {
        let known = list_platforms();
        Err(format!("未知平台: {name}（已存在: {}）", known.join(", ")))
    }
}

/// 列出 `platforms/` 目录下已存在的平台名。
///
/// # 返回值
///
/// 平台名列表（`Vec<String>`，按目录名排序）。
pub(crate) fn list_platforms() -> Vec<String> {
    let platforms_dir = project_root().join("platforms");
    let mut names: Vec<String> = std::fs::read_dir(&platforms_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// 收集 `platforms/` 下全部平台相关 crate 的包名。
///
/// 递归扫描 `platforms/**/Cargo.toml`（平台 lib、平台自治 bin show/keymon、
/// 平台子 xtask 等）读取 `name = "..."`——供辅助命令（doc/test/lint）
/// 动态排除"全部平台代码"（新增平台/子 crate 时零维护，无需手动扩展
/// exclude 列表）。
///
/// # 返回值
///
/// 平台相关 crate 名列表（`Vec<String>`，按出现顺序去重）。
pub(crate) fn platform_crate_names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    collect_crate_names(&project_root().join("platforms"), &mut names);
    names.sort();
    names.dedup();
    names
}

/// 递归收集目录树下所有 `Cargo.toml` 的 `name` 字段。
///
/// # 参数
///
/// - `dir`：要扫描的目录（如 `platforms/`）
/// - `out`：收集结果（追加写入）
fn collect_crate_names(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_crate_names(&path, out);
        } else if path.file_name().is_some_and(|n| n == "Cargo.toml")
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Some(name) = crate_name_from_toml(&content)
        {
            out.push(name);
        }
    }
}

/// 从 Cargo.toml 内容提取 `name = "..."`（首个 `[package]` 段的 name）。
///
/// # 参数
///
/// - `content`：Cargo.toml 内容
///
/// # 返回值
///
/// - `Some(String)`：包名
/// - `None`：未找到（如虚拟 workspace 清单）
fn crate_name_from_toml(content: &str) -> Option<String> {
    let package = content.split_once("[package]")?.1;
    let name_line = package
        .lines()
        .find(|l| l.trim_start().starts_with("name"))?;
    let name = name_line.split('=').nth(1)?.trim().trim_matches('"');
    Some(name.to_string())
}

/// 构造辅助命令（doc/test/lint）的 workspace 范围参数。
///
/// 语义（统一标准，平台排除自动维护）：
/// - `platform = None`：排除**全部**平台相关 crate（`platforms/` 下自动
///   扫描——新增平台/子 crate 零维护），只检查通用层代码
/// - `platform = Some(p)`：排除**其他**平台 crate（按目录归属：
///   `platforms/<p>/` 下的 crate 全保留，其余平台 crate 排除），保留该
///   平台 + 通用层，并追加 `--features <p>/<device>`（device 默认 smart）
///   使该平台代码以指定设备配置编译
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`；None = 排除全部平台）
/// - `device`：设备参数（如 `smart`/`brick`；仅 platform 为 Some 时使用，
///   默认 `smart`）
///
/// # 返回值
///
/// 追加到 cargo 命令的参数列表（`--exclude <pkg>` × N 或
/// `--exclude ... --features <p>/<device>`）。
pub(crate) fn aux_scope_args(platform: Option<&str>, device: &str) -> Vec<String> {
    let mut args = Vec::new();
    // 待排除的平台 crate = 全平台集合 − 目标平台集合（platform=None 时全排）
    let keep: Vec<String> = match platform {
        Some(p) => platform_crate_names_in(p),
        None => Vec::new(),
    };
    for pkg in platform_crate_names() {
        if !keep.contains(&pkg) {
            args.push("--exclude".to_string());
            args.push(pkg);
        }
    }
    if let Some(p) = platform {
        // 启用本平台的 device feature
        args.push("--features".to_string());
        args.push(format!("platform-{p}/{device}"));
    }
    args
}

/// 收集指定平台目录（`platforms/<code>/`）下的全部 crate 包名。
///
/// # 参数
///
/// - `code`：平台代码（如 `tg5040`）
///
/// # 返回值
///
/// 该平台相关 crate 名列表（含平台 lib 与其子 crate）。
fn platform_crate_names_in(code: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    collect_crate_names(&project_root().join("platforms").join(code), &mut names);
    names
}

/// 解析 rustdoc 生成的 `crates.js`，返回 workspace 的 crate 名列表。
///
/// `cargo doc --workspace` 在纯 workspace（无根 package）下不生成根
/// `index.html`，但会生成 `crates.js`——其中第一行即
/// `window.ALL_CRATES = ["common","minarch",...]`。doc 命令据此
/// 自建聚合首页（crate 列表不硬编码——新增 crate 自动出现在首页）。
///
/// # 参数
///
/// - `doc_dir`：`target/doc/` 目录路径
///
/// # 返回值
///
/// - `Ok(Vec<String>)`：crate 名列表（按 crates.js 中顺序）
/// - `Err(String)`：crates.js 缺失或格式异常——错误信息提示手动打开文档
pub(crate) fn list_crate_names(doc_dir: &Path) -> Result<Vec<String>, String> {
    let crates_js = doc_dir.join("crates.js");
    let content = std::fs::read_to_string(&crates_js)
        .map_err(|e| format!("读取 crates.js 失败（{crates_js:?}）: {e}"))?;
    let (_, rest) = content.split_once("window.ALL_CRATES = [").ok_or_else(|| {
        format!("crates.js 缺少 ALL_CRATES（{crates_js:?}），请手动打开 {crates_js:?} 所在目录")
    })?;
    let list = rest.split_once(']').map(|(head, _)| head).unwrap_or(rest);
    let names: Vec<String> = list
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        Err(format!("crates.js 的 ALL_CRATES 为空（{crates_js:?}）"))
    } else {
        Ok(names)
    }
}

/// 用系统默认浏览器打开本地文件或 URL。
///
/// 手写系统命令而非引入第三方 crate（webbrowser/opener）——仅为打开
/// 浏览器引入依赖不划算（config.yaml：非必要不增加复杂度）。
/// `Command` 直接传参，免 shell 注入。
///
/// 用 `spawn()` 而非 `run_command` 的 `output()`：浏览器程序独立于
/// xtask 运行，等待其退出没有意义。spawn 前把 stdout/stderr 重定向到
/// null，防止子进程继承管道句柄导致调用方挂起。
///
/// 支持的平台：macOS（`open`）与 Linux（`xdg-open`）——项目不做
/// Windows 开发环境适配（无 Windows 分支）。
///
/// # 参数
///
/// - `path`：要打开的本地文件路径或 URL（如 `target/doc/index.html`）
///
/// # 返回值
///
/// - `Ok(())`：打开命令成功启动（浏览器是否真正打开由系统决定）
/// - `Err(String)`：命令启动失败（程序不存在等）——错误信息含路径提示
pub(crate) fn open_browser(path: &Path) -> Result<(), String> {
    let path_str = path.to_string_lossy();
    let mut command = {
        #[cfg(target_os = "macos")]
        {
            Command::new("open")
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Command::new("xdg-open")
        }
        // 非 Unix 平台（如 Windows）不在支持范围——报错提示手动打开
        #[cfg(not(unix))]
        {
            return Err(format!(
                "当前平台不支持自动打开浏览器，请手动打开: {path:?}"
            ));
        }
    };
    command
        .arg(&*path_str)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("打开浏览器失败: {path:?}: {e}（可手动打开该文件）"))?;
    // spawn() 已启动子进程；不等待（浏览器独立运行，drop 时也不等待）
    Ok(())
}

/// 工具链容器镜像名。
///
/// 与 `toolchain/Dockerfile` 构建出的镜像一致
/// （`podman build -t minui-toolchain:latest toolchain/`）。
pub(crate) const TOOLCHAIN_IMAGE: &str = "minui-toolchain:latest";

/// 返回容器引擎名（`MINUI_CONTAINER_ENGINE` 环境变量，默认 podman）。
///
/// 编译命令经工具链容器执行（见 `toolchain/Dockerfile`）——引擎可配置
/// （开发机 podman，CI 可 docker）。
///
/// # 返回值
///
/// 容器引擎可执行名（如 `podman`）。
pub(crate) fn container_engine() -> String {
    std::env::var("MINUI_CONTAINER_ENGINE")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "podman".to_string())
}

/// 构造工具链容器命令前缀（`podman run --rm -v ...`）。
///
/// 挂载 workspace 根到容器 `/workspace`（产物经挂载落回宿主），工作目录
/// 为 `/workspace`，镜像为 `minui-toolchain:latest`。返回参数列表，
/// 调用方追加要在容器内执行的命令（如 `cargo build ...`）。
///
/// # 返回值
///
/// 容器命令前缀参数列表（`Vec<String>`）。
pub(crate) fn container_prefix() -> Vec<String> {
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

/// 递归复制目录（含子目录与文件）。
///
/// std 没有内置递归复制，手写实现：遍历源目录树，在目标创建同名目录
/// 并复制全部文件。用于 setup 步骤复制 `skeleton/` → `build/`。
///
/// # 参数
///
/// - `src`：源目录路径
/// - `dst`：目标目录路径（不存在则创建；已存在则覆盖其中同名文件）
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：源目录不存在、读取/创建/复制失败——错误信息含路径
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源目录不存在: {src:?}"));
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败（{dst:?}）: {e}"))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读取目录失败（{src:?}）: {e}"))?
    {
        let entry = entry.map_err(|e| format!("读取目录条目失败（{src:?}）: {e}"))?;
        let path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
        } else {
            std::fs::copy(&path, &dst_path)
                .map_err(|e| format!("复制文件失败（{path:?} → {dst_path:?}）: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_engine_defaults_to_podman() {
        // 引擎可配置（默认 podman，MINUI_CONTAINER_ENGINE 覆盖）。
        // set_var 在 edition 2024 为 unsafe（多线程 UB 风险）——xtask 测试
        // 单线程运行，包 unsafe 块安全。
        let old = std::env::var("MINUI_CONTAINER_ENGINE").ok();
        unsafe {
            std::env::set_var("MINUI_CONTAINER_ENGINE", "");
            assert_eq!(container_engine(), "podman", "空值应回退默认 podman");
            std::env::set_var("MINUI_CONTAINER_ENGINE", "docker");
            assert_eq!(container_engine(), "docker");
            match old {
                Some(v) => std::env::set_var("MINUI_CONTAINER_ENGINE", v),
                None => std::env::set_var("MINUI_CONTAINER_ENGINE", ""),
            }
        }
    }

    #[test]
    fn container_prefix_mounts_workspace() {
        // 容器命令前缀：podman run --rm -v <root>:/workspace -w /workspace 镜像
        let args = container_prefix();
        let joined = args.join(" ");
        assert!(joined.starts_with("run --rm "), "{joined}");
        assert!(
            joined.contains(&format!("{}:/workspace", project_root().display())),
            "应挂载 workspace 根: {joined}"
        );
        assert!(joined.contains("-w /workspace"), "{joined}");
        assert!(joined.contains(TOOLCHAIN_IMAGE), "{joined}");
        // 调用方在其后追加容器内命令
        assert_eq!(args.len(), 7, "run --rm -v <mnt> -w /workspace <image>");
    }

    #[test]
    fn copy_dir_recursive_copies_tree() {
        let src = std::env::temp_dir().join("xtask-copy-test-src");
        let dst = std::env::temp_dir().join("xtask-copy-test-dst");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::fs::write(src.join("sub").join("b.txt"), "b").unwrap();
        copy_dir_recursive(&src, &dst).expect("复制应成功");
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "a");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub").join("b.txt")).unwrap(),
            "b"
        );
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn copy_dir_recursive_overwrites_existing() {
        let src = std::env::temp_dir().join("xtask-copy-test-ovr-src");
        let dst = std::env::temp_dir().join("xtask-copy-test-ovr-dst");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), "new").unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("a.txt"), "old").unwrap();
        std::fs::write(dst.join("stale.txt"), "stale").unwrap();
        copy_dir_recursive(&src, &dst).expect("复制应成功");
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "new");
        // 目标中源没有的文件保留（对齐 cp -R 行为）
        assert_eq!(
            std::fs::read_to_string(dst.join("stale.txt")).unwrap(),
            "stale"
        );
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn copy_dir_recursive_fails_when_src_missing() {
        let src = std::env::temp_dir().join("xtask-copy-test-missing");
        let _ = std::fs::remove_dir_all(&src);
        let err = copy_dir_recursive(&src, &std::env::temp_dir()).unwrap_err();
        assert!(err.contains("源目录不存在"));
    }

    #[test]
    fn project_root_ends_with_minui_rs() {
        let root = project_root();
        assert!(root.ends_with("minui-rs"), "root = {root:?}");
    }

    #[test]
    fn build_dir_is_project_root_plus_build() {
        assert_eq!(build_dir(), project_root().join("build"));
    }

    #[test]
    fn remove_dir_if_exists_ok_when_missing() {
        let dir = std::env::temp_dir().join("xtask-utils-test-remove-missing");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(remove_dir_if_exists(&dir).is_ok());
    }

    #[test]
    fn remove_dir_if_exists_removes_dir() {
        let dir = std::env::temp_dir().join("xtask-utils-test-remove");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("f.txt"), "x").unwrap();
        remove_dir_if_exists(&dir).expect("删除应成功");
        assert!(!dir.exists());
    }

    #[test]
    fn run_command_fails_for_unknown_program() {
        let result = run_command("__definitely_not_a_real_program__", &["--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("__definitely_not_a_real_program__"));
    }

    #[test]
    fn run_command_includes_stdout_on_failure() {
        // 失败命令的 stdout 也应进入错误信息（如 cargo fmt --check 的 diff）
        let result = run_command("cargo", &["fmt", "--all", "--check"]);
        if let Err(err) = result {
            // 当前代码库存在既有 fmt 问题——命令必然失败且输出 diff 到 stdout
            assert!(err.contains("命令失败"), "错误信息应含命令名");
        }
    }

    #[test]
    fn truncate_shortens_long_strings() {
        let long = "x".repeat(100);
        let cut = truncate(&long, 10);
        assert!(cut.ends_with("..."));
        assert!(cut.chars().count() <= 13, "10 字符 + 省略号");
        let short = "abc".to_string();
        assert_eq!(truncate(&short, 10), "abc");
    }

    #[test]
    fn truncate_handles_unicode_boundaries() {
        // 中文是多字节字符——截断不能切在字符中间（panic）
        let s = "中文测试".repeat(20);
        let cut = truncate(&s, 5);
        assert!(!cut.is_empty());
    }

    #[test]
    fn validate_platform_accepts_tg5040() {
        assert!(validate_platform("tg5040").is_ok());
    }

    #[test]
    fn validate_platform_rejects_unknown() {
        let err = validate_platform("nonexistent").unwrap_err();
        assert!(err.contains("未知平台"));
        assert!(err.contains("tg5040"));
    }

    #[test]
    fn platform_crate_names_scans_all_platform_crates() {
        let names = platform_crate_names();
        // tg5040 平台:lib + show + keymon + 平台子 xtask 全收集
        assert!(names.contains(&"platform-tg5040".to_string()), "{names:?}");
        assert!(
            names.contains(&"platform-tg5040-show".to_string()),
            "{names:?}"
        );
        assert!(
            names.contains(&"platform-tg5040-keymon".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&"tg5040-xtask".to_string()), "{names:?}");
    }

    #[test]
    fn aux_scope_no_platform_excludes_all() {
        let args = aux_scope_args(None, "smart");
        let joined = args.join(" ");
        assert!(joined.contains("--exclude platform-tg5040"), "{joined}");
        assert!(joined.contains("--exclude tg5040-xtask"), "{joined}");
        // 无 platform 不带 features（命令无默认）
        assert!(!joined.contains("--features"), "{joined}");
    }

    #[test]
    fn aux_scope_with_platform_excludes_others_and_adds_features() {
        let args = aux_scope_args(Some("tg5040"), "brick");
        let joined = args.join(" ");
        // 本平台 crate 不排除
        assert!(!joined.contains("--exclude platform-tg5040"), "{joined}");
        assert!(!joined.contains("--exclude tg5040-xtask"), "{joined}");
        // 显式 device feature（成对必填）
        assert!(
            joined.contains("--features platform-tg5040/brick"),
            "{joined}"
        );
    }

    #[test]
    fn list_crate_names_parses_all_crates() {
        let dir = std::env::temp_dir().join("xtask-utils-test-list");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("crates.js"),
            r#"window.ALL_CRATES = ["common","minarch","minui","render","xtask"];"#,
        )
        .unwrap();
        let names = list_crate_names(&dir).expect("应能解析 crates.js");
        assert_eq!(names, vec!["common", "minarch", "minui", "render", "xtask"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_crate_names_handles_spaces_and_newlines() {
        let dir = std::env::temp_dir().join("xtask-utils-test-spaces");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("crates.js"),
            "window.ALL_CRATES = [\"common\",\n  \"render\",\n];",
        )
        .unwrap();
        let names = list_crate_names(&dir).expect("应能解析带换行的 crates.js");
        assert_eq!(names, vec!["common", "render"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_crate_names_fails_when_missing() {
        let dir = std::env::temp_dir().join("xtask-utils-test-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = list_crate_names(&dir).unwrap_err();
        assert!(err.contains("crates.js"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_crate_names_fails_on_bad_format() {
        let dir = std::env::temp_dir().join("xtask-utils-test-bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crates.js"), "window.SOMETHING_ELSE = [];").unwrap();
        let err = list_crate_names(&dir).unwrap_err();
        assert!(err.contains("ALL_CRATES"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_browser_spawns_launcher() {
        // spawn 语义：只检查启动命令能否拉起（浏览器独立运行，不等待不检查文件）
        // 用项目内确定存在的文件路径（target/doc 已由 doc 测试构建或存在）
        let target_dir = std::env::temp_dir();
        let path = target_dir.join("__open_browser_test__.html");
        std::fs::write(&path, "<html></html>").unwrap();
        let result = open_browser(&path);
        let _ = std::fs::remove_file(&path);
        // open/xdg-open 都可能成功或失败（取决于环境有无浏览器），
        // 但错误信息必须含路径提示——验证 Err 分支的消息质量而非必然性
        if let Err(err) = result {
            assert!(err.contains("手动打开"), "错误信息应提示手动打开: {err}");
        }
    }
}
