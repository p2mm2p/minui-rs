//! `special` 步骤模块
//!
//! 处理 BOOT 目录重命名与派生（全局步骤，无需 --platform）。
//!
//! 对应 C 原版 `MinUI/makefile` 的 `special` 目标（makefile:114-129）：
//! `BOOT/common` → `BOOT/.tmp_update`、`BOOT/{miyoo,trimui,magicx}` →
//! `BASE/`、复制 `.tmp_update` 到各设备 app 目录、派生 `BASE/miyoo354/355/285`。
//!
//! 差异：C 原版假定所有目录存在（无条件 mv/cp）；Rust 版 skeleton 只有
//! `BOOT/common` + `BOOT/trimui`（无 miyoo/magicx）——**源目录不存在时
//! 跳过该操作**（不报错），未来 skeleton 扩展时逻辑自然覆盖（见
//!

use std::path::Path;

use crate::utils;

/// 执行 `special` 步骤：按 C 原版逻辑重命名/复制，缺失目录跳过。
///
/// # 返回值
///
/// - `Ok(())`：全部存在的目录处理完成（缺失目录跳过不报错）
/// - `Err(String)`：重命名/复制失败
pub fn run() -> Result<(), String> {
    let build = utils::build_dir();
    apply_special(&build)
}

/// special 重命名逻辑（可测纯函数，接受 build 目录路径）。
///
/// 步骤（每步源不存在则跳过）：
/// 1. `BOOT/common` → `BOOT/.tmp_update`
/// 2. `BOOT/{miyoo,trimui,magicx}` → `BASE/`（逐个移动）
/// 3. `.tmp_update` 复制到 `BASE/{miyoo,trimui,magicx}/app|/`（目标存在时）
/// 4. `BASE/miyoo` 派生 `BASE/miyoo354`/`miyoo355`/`miyoo285`
///
/// # 参数
///
/// - `build`：build 目录路径
///
/// # 返回值
///
/// - `Ok(())`：处理完成
/// - `Err(String)`：移动/复制失败——错误信息含路径
fn apply_special(build: &Path) -> Result<(), String> {
    let boot = build.join("BOOT");
    let base = build.join("BASE");

    // 1. BOOT/common → BOOT/.tmp_update
    rename_if_exists(&boot.join("common"), &boot.join(".tmp_update"))?;

    // 2. BOOT/{miyoo,trimui,magicx} → BASE/
    for name in ["miyoo", "trimui", "magicx"] {
        rename_if_exists(&boot.join(name), &base.join(name))?;
    }

    // 3. .tmp_update 复制到各设备 app 目录
    let tmp_update = boot.join(".tmp_update");
    if tmp_update.is_dir() {
        // C 原版：cp -R .tmp_update BASE/miyoo/app/、BASE/trimui/app/、BASE/magicx/
        for name in ["miyoo", "trimui", "magicx"] {
            let base_app = base.join(name).join("app");
            if base_app.is_dir() {
                copy_tmp_update_into(&tmp_update, &base_app)?;
            }
        }
        // C 原版：cp -R .tmp_update BASE/magicx/（magicx 无 app 层级，复制到根）
        let base_magicx = base.join("magicx");
        if base_magicx.is_dir() && !base_magicx.join("app").is_dir() {
            copy_tmp_update_into(&tmp_update, &base_magicx)?;
        }
    }

    // 4. BASE/miyoo 派生 miyoo354/355/285
    let miyoo = base.join("miyoo");
    if miyoo.is_dir() {
        for name in ["miyoo354", "miyoo355", "miyoo285"] {
            let dst = base.join(name);
            if !dst.exists() {
                utils::copy_dir_recursive(&miyoo, &dst)?;
            }
        }
    }

    Ok(())
}

/// 移动目录（源不存在则跳过）。
///
/// # 参数
///
/// - `src`：源目录
/// - `dst`：目标路径
///
/// # 返回值
///
/// - `Ok(())`：移动完成或源不存在
/// - `Err(String)`：移动失败
fn rename_if_exists(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::rename(src, dst).map_err(|e| format!("移动失败（{src:?} → {dst:?}）: {e}"))?;
    }
    Ok(())
}

/// 复制 `.tmp_update` 目录内容到目标 app 目录（`cp -R .tmp_update/. app/`）。
///
/// # 参数
///
/// - `tmp_update`：`.tmp_update` 源目录
/// - `app_dir`：目标 app 目录
///
/// # 返回值
///
/// - `Ok(())`：复制完成
/// - `Err(String)`：复制失败
fn copy_tmp_update_into(tmp_update: &Path, app_dir: &Path) -> Result<(), String> {
    let dst = app_dir.join(".tmp_update");
    utils::copy_dir_recursive(tmp_update, &dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // 每个测试唯一目录名——cargo test 并行执行，共享路径会互相踩踏
    fn make_build(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-special-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("BOOT")).unwrap();
        std::fs::create_dir_all(dir.join("BASE")).unwrap();
        dir
    }

    #[test]
    fn special_handles_only_common_and_trimui() {
        // 模拟 Rust skeleton：BOOT/common + BOOT/trimui（无 miyoo/magicx）
        let build = make_build("only-common-trimui");
        std::fs::create_dir_all(build.join("BOOT").join("common")).unwrap();
        std::fs::create_dir_all(build.join("BOOT").join("trimui").join("app")).unwrap();
        std::fs::write(build.join("BOOT").join("common").join("updater"), "x").unwrap();

        apply_special(&build).expect("special 应成功（缺失目录跳过）");

        // common → .tmp_update
        assert!(!build.join("BOOT").join("common").exists());
        assert!(
            build
                .join("BOOT")
                .join(".tmp_update")
                .join("updater")
                .exists()
        );
        // trimui → BASE/trimui
        assert!(build.join("BASE").join("trimui").join("app").is_dir());
        // .tmp_update 复制到 trimui/app/
        assert!(
            build
                .join("BASE")
                .join("trimui")
                .join("app")
                .join(".tmp_update")
                .join("updater")
                .exists()
        );
        // miyoo/magicx 缺失——跳过，不报错
        assert!(!build.join("BASE").join("miyoo").exists());
        assert!(!build.join("BASE").join("magicx").exists());
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn special_handles_full_family_when_present() {
        // 完整家族（未来 skeleton 扩展场景）：miyoo/trimui/magicx 都在
        let build = make_build("full-family");
        for name in ["common", "miyoo", "trimui", "magicx"] {
            std::fs::create_dir_all(build.join("BOOT").join(name)).unwrap();
        }
        std::fs::create_dir_all(build.join("BOOT").join("trimui").join("app")).unwrap();
        std::fs::create_dir_all(build.join("BOOT").join("miyoo").join("app")).unwrap();
        std::fs::write(build.join("BOOT").join("common").join("updater"), "x").unwrap();

        apply_special(&build).expect("special 应成功");

        assert!(build.join("BOOT").join(".tmp_update").is_dir());
        assert!(build.join("BASE").join("miyoo").is_dir());
        assert!(build.join("BASE").join("trimui").is_dir());
        assert!(build.join("BASE").join("magicx").is_dir());
        // 派生 miyoo354/355/285
        for name in ["miyoo354", "miyoo355", "miyoo285"] {
            assert!(build.join("BASE").join(name).is_dir(), "{name} 应被派生");
        }
        let _ = std::fs::remove_dir_all(&build);
    }

    #[test]
    fn special_ok_when_build_missing() {
        let build = std::env::temp_dir().join("xtask-special-test-missing");
        let _ = std::fs::remove_dir_all(&build);
        // build 不存在 → 所有 rename_if_exists 跳过 → 成功
        assert!(apply_special(&build).is_ok());
    }
}
