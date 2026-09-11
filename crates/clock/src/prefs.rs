//! 12/24 小时制偏好读写（纯逻辑）
//!
//! 本模块提供 clock 的显示制式偏好持久化：用 `show_24hour` 标记文件的
//! 存在性表达偏好（存在 = 24 小时制，缺失 = 12 小时制）。
//!
//! 对应原版 C `clock.c` 的 `exists(USERDATA_PATH "/show_24hour")`
//! （:57）、`system("touch ...")` / `system("rm ...")`（:230-234）。
//! Rust 版用 `std::fs` 直接读写，避免 shell 命令，且路径参数化可测。
//!
//! 文件格式与 C 原版完全兼容——用户从原版 MinUI 升级时，
//! `.userdata/<platform>/show_24hour` 已存在即识别为 24 小时制。
//!

/// 读取 12/24 小时偏好（标记文件存在 = 24 小时制）
///
/// # 参数
///
/// - `path`：`show_24hour` 标记文件路径（如
///   `SDCARD_PATH/.userdata/<platform>/show_24hour`）
///
/// # 返回值
///
/// `true` = 24 小时制（文件存在），`false` = 12 小时制（文件缺失）
pub fn read_show_24hour(path: &str) -> bool {
    std::path::Path::new(path).is_file()
}

/// 写入 12/24 小时偏好
///
/// `true` 创建标记文件（24 小时制），`false` 删除之（12 小时制）。
/// 父目录不存在时静默忽略（与原版 `system("touch")` 失败静默一致）。
///
/// # 参数
///
/// - `path`：`show_24hour` 标记文件路径
/// - `show_24hour`：是否 24 小时制
pub fn write_show_24hour(path: &str, show_24hour: bool) {
    if show_24hour {
        // 创建空标记文件（对应 C `touch`）
        let _ = std::fs::write(path, b"");
    } else {
        // 删除标记文件（对应 C `rm`）
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("clock-prefs-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_returns_false_when_file_missing() {
        let dir = temp_dir("missing");
        let path = dir.join("show_24hour");
        assert!(
            !read_show_24hour(path.to_str().unwrap()),
            "文件缺失应为 12 小时制"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_returns_true_when_file_exists() {
        let dir = temp_dir("exists");
        let path = dir.join("show_24hour");
        std::fs::write(&path, b"").unwrap();
        assert!(
            read_show_24hour(path.to_str().unwrap()),
            "文件存在应为 24 小时制"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_true_creates_file() {
        let dir = temp_dir("write-true");
        let path = dir.join("show_24hour");
        write_show_24hour(path.to_str().unwrap(), true);
        assert!(path.is_file(), "write(true) 应创建标记文件");
        assert!(read_show_24hour(path.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_false_removes_file() {
        let dir = temp_dir("write-false");
        let path = dir.join("show_24hour");
        std::fs::write(&path, b"").unwrap();
        write_show_24hour(path.to_str().unwrap(), false);
        assert!(!path.exists(), "write(false) 应删除标记文件");
        assert!(!read_show_24hour(path.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_false_when_file_missing_is_noop() {
        let dir = temp_dir("write-false-noop");
        let path = dir.join("show_24hour");
        write_show_24hour(path.to_str().unwrap(), false);
        assert!(!path.exists(), "删除不存在的文件应无副作用");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_write_then_read() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("show_24hour");
        write_show_24hour(path.to_str().unwrap(), true);
        assert!(read_show_24hour(path.to_str().unwrap()));
        write_show_24hour(path.to_str().unwrap(), false);
        assert!(!read_show_24hour(path.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
