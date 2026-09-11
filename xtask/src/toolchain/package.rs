//! `package` 步骤模块
//!
//! 打包发布：生成 version.txt/commits.txt 并打包 zip（要求 --platform 与
//! --device——发布名含平台与设备段，见「发布命名」）。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `package` 目标（makefile:143-165）：
//! 写 version.txt（`MinUI-<platform>-<device>-YYYYMMDD-N\n<git短hash>`）、
//! commits.txt、组装 PAYLOAD/.system 与 MinUI.zip、生成 base/extras 发布 zip。
//!
//! 差异：version.txt 的日期用 UTC（对齐 C 原版 `TZ=GMT date`）；hash 经
//! git 命令获取（缺失报错）；zip 用系统命令（缺 zip 命令报错，发布
//! 环境须自备——见
//!
//! 发布命名（build-release-pipeline 决策 10）：Rust 版每 (platform, device)
//! 产物内容不同（show 分辨率/安装图按 device 编译期确定、boot.sh 已移除
//! 运行时设备检测），发布名须含平台与设备段区分——
//! `MinUI-<platform>-<device>-YYYYMMDD-<N>`（如 `MinUI-tg5040-smart-20260907-0`），
//! N 按同 (platform, device) 前缀当日计数（与原版 `MinUI-YYYYMMDD-N` 不同——
//! 原版单包运行时检测通吃多设备，无需区分）。
//!
//! 发布前完整性校验：PAYLOAD 组装前检查关键产物（minui/minarch、
//! show/keymon/install.sh、Tools pak 的 clock/minput、stock cores、
//! .tmp_update 派生）是否齐全，缺失即报错——避免静默产出残缺发布包（见
//! 「package 发布前完整性校验」Requirement）。
//!

use std::path::Path;

use crate::utils;

/// 进 `SYSTEM/<platform>/bin/` 的通用二进制（对齐 system.rs 的 SYSTEM_BINARIES）。
const REQUIRED_BINARIES: [&str; 2] = ["minui", "minarch"];

/// 平台自治装配的 bin 产物（show/keymon/install.sh，由平台子 xtask 复制）。
const REQUIRED_PLATFORM_BINS: [&str; 3] = ["show", "keymon", "install.sh"];

/// Tools pak 工具二进制（对齐 system.rs 的 TOOLS_PAKS——clock/minput 进
/// EXTRAS/Tools/<platform>/{Clock,Input}.pak/ 而非 SYSTEM/bin）。
const REQUIRED_TOOLS: [(&str, &str); 2] = [("clock", "Clock.pak"), ("minput", "Input.pak")];

/// stock 核心（6 个，对齐平台子 xtask `copy_stock_cores` 的复制表）。
const STOCK_CORES: [&str; 6] = [
    "fceumm",
    "gambatte",
    "gpsp",
    "picodrive",
    "snes9x2005_plus",
    "pcsx_rearmed",
];

/// 执行 `package` 步骤：version/commits + PAYLOAD 组装 + 3 个 zip。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`——决定完整性校验范围与发布名）
/// - `device`：设备参数（如 `smart`/`brick`——进入发布名区分产物）
///
/// # 返回值
///
/// - `Ok(())`：发布包生成完成
/// - `Err(String)`：git/zip 缺失、文件读写失败、完整性校验失败
pub fn run(platform: &str, device: &str) -> Result<(), String> {
    let build = utils::build_dir();
    let releases = utils::project_root().join("releases");
    std::fs::create_dir_all(&releases).map_err(|e| format!("创建 releases 目录失败: {e}"))?;

    // 0. 发布前完整性校验（PAYLOAD 组装前，缺失即报错列出清单）
    let missing = check_release_integrity(&build, &[platform]);
    if !missing.is_empty() {
        return Err(format!(
            "发布包完整性检查失败——缺失产物: {}（请先执行 toolchain all 全流程）",
            missing.join(", ")
        ));
    }

    // 1. version.txt（build/SYSTEM/version.txt）
    let hash = git_short_hash()?;
    let release_name = release_name(&releases, platform, device)?;
    let system_dir = build.join("SYSTEM");
    let version_content = format!("{release_name}\n{}", hash.trim());
    std::fs::write(system_dir.join("version.txt"), version_content)
        .map_err(|e| format!("写入 version.txt 失败: {e}"))?;

    // 2. commits.txt（主仓库 + cores/src git 信息）
    let commits = commits_content()?;
    std::fs::write(system_dir.join("commits.txt"), commits)
        .map_err(|e| format!("写入 commits.txt 失败: {e}"))?;

    // 3. 删除 .DS_Store（C 原版 makefile:154）
    remove_ds_store(&build)?;

    // 4. 组装 PAYLOAD/.system + .tmp_update
    let payload = build.join("PAYLOAD");
    let _ = std::fs::remove_dir_all(&payload);
    std::fs::create_dir_all(&payload).map_err(|e| format!("创建 PAYLOAD 失败: {e}"))?;
    std::fs::rename(build.join("SYSTEM"), payload.join(".system"))
        .map_err(|e| format!("移动 SYSTEM → PAYLOAD/.system 失败: {e}"))?;
    utils::copy_dir_recursive(
        &build.join("BOOT").join(".tmp_update"),
        &payload.join(".tmp_update"),
    )?;

    // 5. MinUI.zip（.system + .tmp_update）
    zip_dir(
        &payload,
        &payload.join("MinUI.zip"),
        &[".system", ".tmp_update"],
    )?;
    // 6. MinUI.zip → build/BASE
    std::fs::rename(
        payload.join("MinUI.zip"),
        build.join("BASE").join("MinUI.zip"),
    )
    .map_err(|e| format!("移动 MinUI.zip 失败: {e}"))?;

    // 7. base.zip / extras.zip（对齐 C 原版 makefile:163-164）
    zip_base(&build, &releases, &release_name)?;
    zip_extras(&build, &releases, &release_name)?;

    // 8. latest.txt
    std::fs::write(build.join("latest.txt"), &release_name)
        .map_err(|e| format!("写入 latest.txt 失败: {e}"))?;

    Ok(())
}

/// 发布前完整性校验：检查指定平台的关键装配产物是否齐全。
///
/// 校验清单（按平台逐一检查）：
/// 1. `SYSTEM/<platform>/bin/` 下 2 个通用二进制（minui/minarch）
/// 2. 平台自治装配：`SYSTEM/<platform>/bin/` 下 show/keymon/install.sh
/// 3. Tools pak 工具：`EXTRAS/Tools/<platform>/{Clock,Input}.pak/` 下
///    clock/minput（system 步骤装配；对齐 C 原版 makefile:55-56）
/// 4. stock 核心：`SYSTEM/<platform>/cores/<core>_libretro.so`（6 个）
/// 5. BOOT 派生产物：`BOOT/.tmp_update` 目录存在（special 步骤产出，
///    package 组装 PAYLOAD/.tmp_update 依赖它）
///
/// # 参数
///
/// - `build`：build 目录路径
/// - `platforms`：实际装配的平台列表（当前由调用方传单平台）
///
/// # 返回值
///
/// 缺失产物清单（相对 build 的路径字符串；空 = 齐全）。
fn check_release_integrity(build: &Path, platforms: &[&str]) -> Vec<String> {
    let mut missing = Vec::new();

    for platform in platforms {
        let bin_dir = build.join("SYSTEM").join(platform).join("bin");
        // 1. 通用二进制（minui/minarch）
        for bin in REQUIRED_BINARIES {
            if !bin_dir.join(bin).is_file() {
                missing.push(format!("SYSTEM/{platform}/bin/{bin}"));
            }
        }
        // 2. 平台自治装配（show/keymon/install.sh）
        for bin in REQUIRED_PLATFORM_BINS {
            if !bin_dir.join(bin).is_file() {
                missing.push(format!("SYSTEM/{platform}/bin/{bin}"));
            }
        }
        // 3. Tools pak 工具（clock/minput）
        let tools_root = build.join("EXTRAS").join("Tools").join(platform);
        for (bin, pak) in REQUIRED_TOOLS {
            if !tools_root.join(pak).join(bin).is_file() {
                missing.push(format!("EXTRAS/Tools/{platform}/{pak}/{bin}"));
            }
        }
        // 4. stock 核心
        let cores_dir = build.join("SYSTEM").join(platform).join("cores");
        for core in STOCK_CORES {
            let so = format!("{core}_libretro.so");
            if !cores_dir.join(&so).is_file() {
                missing.push(format!("SYSTEM/{platform}/cores/{so}"));
            }
        }
    }

    // 5. BOOT/.tmp_update 派生（special 步骤产出，全局结构）
    if !build.join("BOOT").join(".tmp_update").is_dir() {
        missing.push("BOOT/.tmp_update（special 步骤未执行？）".to_string());
    }

    missing
}

/// 生成发布名 `MinUI-<platform>-<device>-YYYYMMDD-N`。
///
/// Rust 版每 (platform, device) 独立打包（show 分辨率/安装图按 device
/// 编译期确定、boot.sh 无运行时设备检测），发布名须含平台与设备段——
/// 对齐 C 原版 `MinUI-YYYYMMDD-N` 的日期+当日计数语义，但前缀含
/// platform/device 使不同设备独立计数、可区分（见模块文档「发布命名」）。
///
/// # 参数
///
/// - `releases`：`releases/` 目录路径
/// - `platform`：目标平台（如 `tg5040`）
/// - `device`：设备参数（如 `smart`/`brick`）
///
/// # 返回值
///
/// 发布名（如 `MinUI-tg5040-smart-20260907-0`）。
fn release_name(releases: &Path, platform: &str, device: &str) -> Result<String, String> {
    let date = utc_date();
    let prefix = format!("MinUI-{platform}-{device}-{date}");
    let count = std::fs::read_dir(releases)
        .map_err(|e| format!("读取 releases 目录失败: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{prefix}-"))
                && e.file_name().to_string_lossy().ends_with("-base.zip")
        })
        .count();
    Ok(format!("{prefix}-{count}"))
}

/// 当前 UTC 日期（`YYYYMMDD`，对齐 C 原版 `TZ=GMT date +%Y%m%d`）。
///
/// # 返回值
///
/// 日期字符串（如 `20260810`）。
fn utc_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    // 1970-01-01 起的天数 → 年月日（civil_from_days 算法）
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// 获取 git 短 hash。
///
/// # 返回值
///
/// - `Ok(String)`：短 hash（去尾换行）
/// - `Err(String)`：git 命令不可用或失败
fn git_short_hash() -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_err(|e| format!("获取 git hash 失败（git 命令不可用）: {e}"))?;
    if !output.status.success() {
        return Err("获取 git hash 失败：git rev-parse 非零退出".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 生成 commits.txt 内容（主仓库 git 信息）。
///
/// 对齐 C 原版 `commits.sh` 的主仓库段落（name/hash/date/repo）。
/// cores/src 的 libretro 源码仓库信息因当前环境（无 git、无 cores/src）
/// 仅在可用时附加。
///
/// # 返回值
///
/// - `Ok(String)`：commits.txt 内容
/// - `Err(String)`：git 缺失
fn commits_content() -> Result<String, String> {
    let hash = git_short_hash()?;
    let repo = git_remote()?;
    Ok(format!("MINUI {hash} {repo}\n"))
}

/// 获取主仓库 remote URL（去掉 github 前缀/后缀，对齐 commits.sh）。
///
/// # 返回值
///
/// - `Ok(String)`：repo 名（如 `USER/REPO`），无 remote 时为 `local`
/// - `Err(String)`：git 缺失
fn git_remote() -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(|e| format!("获取 git remote 失败（git 命令不可用）: {e}"))?;
    if !output.status.success() {
        return Ok("local".to_string());
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // 对齐 commits.sh 的 sed：去 git@github.com:/https://github.com/ 前缀与 .git 后缀
    let repo = url
        .trim_start_matches("git@github.com:")
        .trim_start_matches("https://github.com/")
        .trim_end_matches(".git");
    Ok(repo.to_string())
}

/// 递归删除目录中的 `.DS_Store` 文件。
///
/// # 参数
///
/// - `dir`：目录路径（递归）
///
/// # 返回值
///
/// - `Ok(())`：完成（读取失败忽略，对齐 find -delete 宽松语义）
fn remove_ds_store(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| format!("读取目录失败（{dir:?}）: {e}"))?
    {
        let entry = entry.map_err(|e| format!("读取目录条目失败（{dir:?}）: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            remove_ds_store(&path)?;
        } else if path.file_name().is_some_and(|n| n == ".DS_Store") {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

/// 在目录内 zip 指定条目（系统 zip 命令）。
///
/// # 参数
///
/// - `dir`：工作目录（zip 在其中执行）
/// - `zip_path`：输出 zip 文件路径
/// - `entries`：要打包的条目名（相对 dir）
///
/// # 返回值
///
/// - `Ok(())`：zip 完成
/// - `Err(String)`：zip 命令缺失或失败
fn zip_dir(dir: &Path, zip_path: &Path, entries: &[impl AsRef<str>]) -> Result<(), String> {
    let mut args = vec!["-r".to_string(), zip_path.to_string_lossy().to_string()];
    args.extend(entries.iter().map(|e| e.as_ref().to_string()));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // zip 需要在目标目录内执行（相对条目）
    let output = std::process::Command::new("zip")
        .args(&args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("zip 命令不可用（发布环境需安装 zip）: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "zip 失败（{zip_path:?}）: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// 打包 base.zip（对齐 C 原版 makefile:163 的清单）。
///
/// # 参数
///
/// - `build`：build 目录
/// - `releases`：releases 目录
/// - `release_name`：发布名（如 `MinUI-20260810-0`）
///
/// # 返回值
///
/// - `Ok(())`：打包完成
/// - `Err(String)`：zip 失败
fn zip_base(build: &Path, releases: &Path, release_name: &str) -> Result<(), String> {
    // C 原版清单：Bios Roms Saves miyoo miyoo354 trimui rg35xx rg35xxplus gkdpixel
    // miyoo355 magicx miyoo285 em_ui.sh MinUI.zip README.txt——按实际存在过滤
    let base_dir = build.join("BASE");
    let entries = existing_entries(&base_dir);
    let zip_path = releases.join(format!("{release_name}-base.zip"));
    zip_dir(&base_dir, &zip_path, &entries)
}

/// 打包 extras.zip（对齐 C 原版 makefile:164 的清单）。
///
/// # 参数
///
/// - `build`：build 目录
/// - `releases`：releases 目录
/// - `release_name`：发布名
///
/// # 返回值
///
/// - `Ok(())`：打包完成
/// - `Err(String)`：zip 失败
fn zip_extras(build: &Path, releases: &Path, release_name: &str) -> Result<(), String> {
    let extras_dir = build.join("EXTRAS");
    let entries = existing_entries(&extras_dir);
    let zip_path = releases.join(format!("{release_name}-extras.zip"));
    zip_dir(&extras_dir, &zip_path, &entries)
}

/// 返回目录中实际存在的条目名（对齐 C 原版清单但过滤缺失目录）。
///
/// # 参数
///
/// - `dir`：目录路径
///
/// # 返回值
///
/// 条目名列表（仅存在者）。
fn existing_entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_date_is_8_digits() {
        let d = utc_date();
        assert_eq!(d.len(), 8, "日期应为 YYYYMMDD: {d}");
        assert!(d.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn release_name_includes_platform_and_device() {
        // 发布命名（决策 10）：MinUI-<platform>-<device>-YYYYMMDD-N，
        // N 按同 (platform, device) 前缀当日 base.zip 计数
        let dir = std::env::temp_dir().join("xtask-package-test-rel");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let date = utc_date();
        std::fs::write(
            dir.join(format!("MinUI-tg5040-smart-{date}-0-base.zip")),
            "",
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("MinUI-tg5040-smart-{date}-1-base.zip")),
            "",
        )
        .unwrap();
        // 其他 device 不计（smart 与 brick 独立计数）
        std::fs::write(
            dir.join(format!("MinUI-tg5040-brick-{date}-0-base.zip")),
            "",
        )
        .unwrap();
        // 其他日期不计
        std::fs::write(dir.join("MinUI-tg5040-smart-19990101-0-base.zip"), "").unwrap();
        // 非 base 不计
        std::fs::write(
            dir.join(format!("MinUI-tg5040-smart-{date}-0-extras.zip")),
            "",
        )
        .unwrap();
        let name = release_name(&dir, "tg5040", "smart").expect("应生成发布名");
        assert_eq!(
            name,
            format!("MinUI-tg5040-smart-{date}-2"),
            "应计数 2 个当日同设备 base.zip: {name}"
        );
        // brick 独立计数（不受 smart 影响）
        let brick_name = release_name(&dir, "tg5040", "brick").expect("应生成 brick 发布名");
        assert_eq!(
            brick_name,
            format!("MinUI-tg5040-brick-{date}-1"),
            "brick 应独立计数: {brick_name}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_entries_filters_missing() {
        let dir = std::env::temp_dir().join("xtask-package-test-entries");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("Bios")).unwrap();
        std::fs::write(dir.join("README.txt"), "x").unwrap();
        let entries = existing_entries(&dir);
        assert!(entries.contains(&"Bios".to_string()));
        assert!(entries.contains(&"README.txt".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_entries_empty_when_dir_missing() {
        let dir = std::env::temp_dir().join("xtask-package-test-missing");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(existing_entries(&dir).is_empty());
    }

    #[test]
    fn remove_ds_store_deletes_hidden_files() {
        let dir = std::env::temp_dir().join("xtask-package-test-ds");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join(".DS_Store"), "").unwrap();
        std::fs::write(dir.join("sub").join(".DS_Store"), "").unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        remove_ds_store(&dir).expect("清理应成功");
        assert!(!dir.join(".DS_Store").exists());
        assert!(!dir.join("sub").join(".DS_Store").exists());
        assert!(dir.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_hash_fails_when_git_missing() {
        match git_short_hash() {
            Ok(h) => assert!(!h.is_empty()),
            Err(e) => assert!(e.contains("git"), "错误应含 git: {e}"),
        }
    }

    /// 构造完整性校验用的临时 build 目录（按齐全态填充 tg5040 产物）。
    fn make_complete_build(tag: &str) -> std::path::PathBuf {
        let build = std::env::temp_dir().join(format!("xtask-package-int-{tag}"));
        let _ = std::fs::remove_dir_all(&build);
        // bin：2 通用（minui/minarch）+ show/keymon/install.sh
        let bin_dir = build.join("SYSTEM").join("tg5040").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        for bin in REQUIRED_BINARIES
            .iter()
            .chain(REQUIRED_PLATFORM_BINS.iter())
        {
            std::fs::write(bin_dir.join(bin), "x").unwrap();
        }
        // Tools pak：clock/minput
        let tools_root = build.join("EXTRAS").join("Tools").join("tg5040");
        for (bin, pak) in REQUIRED_TOOLS {
            let pak_dir = tools_root.join(pak);
            std::fs::create_dir_all(&pak_dir).unwrap();
            std::fs::write(pak_dir.join(bin), "x").unwrap();
        }
        // cores：6 stock
        let cores_dir = build.join("SYSTEM").join("tg5040").join("cores");
        std::fs::create_dir_all(&cores_dir).unwrap();
        for core in STOCK_CORES {
            std::fs::write(cores_dir.join(format!("{core}_libretro.so")), "x").unwrap();
        }
        // BOOT/.tmp_update 派生
        std::fs::create_dir_all(build.join("BOOT").join(".tmp_update")).unwrap();
        build
    }

    #[test]
    fn integrity_ok_when_all_present() {
        let build = make_complete_build("ok");
        let missing = check_release_integrity(&build, &["tg5040"]);
        assert!(missing.is_empty(), "齐全应无缺失: {missing:?}");
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn integrity_reports_all_missing_on_empty_build() {
        let build = std::env::temp_dir().join("xtask-package-int-empty");
        let _ = std::fs::remove_dir_all(&build);
        let missing = check_release_integrity(&build, &["tg5040"]);
        // 2 通用 + 3 平台 bin + 2 Tools + 6 cores + .tmp_update = 14 项
        assert_eq!(missing.len(), 14, "空 build 应报全部缺失: {missing:?}");
        assert!(missing.iter().any(|m| m.contains("bin/minui")));
        assert!(missing.iter().any(|m| m.contains("bin/show")));
        assert!(
            missing.iter().any(|m| m.contains("Clock.pak/clock")),
            "应报 Tools 缺失: {missing:?}"
        );
        assert!(missing.iter().any(|m| m.contains("cores/fceumm_libretro.so")));
        assert!(missing.iter().any(|m| m.contains("BOOT/.tmp_update")));
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn integrity_reports_partial_missing() {
        let build = make_complete_build("partial");
        // 删 minui、一个 Tools 工具与 .tmp_update
        std::fs::remove_file(
            build
                .join("SYSTEM")
                .join("tg5040")
                .join("bin")
                .join("minui"),
        )
        .unwrap();
        std::fs::remove_file(
            build
                .join("EXTRAS")
                .join("Tools")
                .join("tg5040")
                .join("Clock.pak")
                .join("clock"),
        )
        .unwrap();
        std::fs::remove_dir_all(build.join("BOOT").join(".tmp_update")).unwrap();
        let missing = check_release_integrity(&build, &["tg5040"]);
        assert_eq!(missing.len(), 3, "应报 3 项缺失: {missing:?}");
        assert!(missing.iter().any(|m| m.contains("bin/minui")));
        assert!(missing.iter().any(|m| m.contains("Clock.pak/clock")));
        assert!(missing.iter().any(|m| m.contains(".tmp_update")));
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn integrity_platform_loop_covers_each_platform() {
        // 多平台扩展点：platforms 数组逐个检查（fake 平台也应被查）
        let build = std::env::temp_dir().join("xtask-package-int-multi");
        let _ = std::fs::remove_dir_all(&build);
        std::fs::create_dir_all(&build).unwrap();
        let missing = check_release_integrity(&build, &["tg5040", "other"]);
        assert_eq!(
            missing.len(),
            27,
            "两平台应各报 13 项 + 共享 .tmp_update 1 项"
        );
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn stock_cores_match_platform_xtask_copy_table() {
        // 防漂移守护：本模块的 STOCK_CORES 必须与 tg5040 平台子 xtask
        // `copy_stock_cores` 的 STOCK 常量一致（平台子 xtask 是 stock/extras
        // 划分的权威——装配复制按它的表执行，完整性校验须查同一清单）。
        // xtask 不依赖平台 crate（spec 约束），此处以显式断言钉住。
        let expected = [
            "fceumm",
            "gambatte",
            "gpsp",
            "picodrive",
            "snes9x2005_plus",
            "pcsx_rearmed",
        ];
        assert_eq!(
            STOCK_CORES.as_slice(),
            &expected[..],
            "STOCK_CORES 与 tg5040 平台子 xtask copy_stock_cores 的 STOCK 不一致"
        );
    }

    #[test]
    fn tools_match_system_tools_paks() {
        // 防漂移守护：完整性校验的 Tools 清单必须与 system.rs 的
        // TOOLS_PAKS 装配表一致（clock → Clock.pak、minput → Input.pak）
        let expected = [("clock", "Clock.pak"), ("minput", "Input.pak")];
        assert_eq!(
            REQUIRED_TOOLS.as_slice(),
            &expected[..],
            "REQUIRED_TOOLS 与 system.rs TOOLS_PAKS 不一致"
        );
    }
}
