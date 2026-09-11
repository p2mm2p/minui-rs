/// 测试共享辅助（cargo 集成测试惯例：`tests/common/` 子目录不会被当作测试目标）
///
/// 提供 mock 核心（`tests/fixtures/mock_core.c`）的主机端编译辅助，
/// 供 `tests/core_loading.rs` 加载真实的 libretro 核心动态库做集成测试。
///
/// ## 编译机制
///
/// 直接调用系统 `cc -shared -fPIC`（`std::process::Command`），不引入构建依赖：
/// `cc` crate 只产静态库（`shared_flag` 已弃用为 no-op），而 libloading
/// 需要动态库——见 design.md 决策 6。产物写入 `CARGO_TARGET_TMPDIR`，
/// 不参与 `cargo build --features tg5040/smart` 的设备构建。
use std::path::PathBuf;

/// 主机端 mock 核心动态库的文件名（macOS 为 `.dylib`，Linux 为 `.so`）
///
/// 不加 `lib` 前缀：按绝对路径 dlopen 不要求该前缀，且让文件名保持
/// 设备上 `<core>_libretro.so` 的形态，`derive_core_name` 截断结果
/// 与真实核心一致（`mock_core_xxx` → `mock_core`）。
fn mock_core_file_name(id: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("mock_core_{id}.dylib")
    } else {
        format!("mock_core_{id}.so")
    }
}

/// 用系统 cc 编译 `tests/fixtures/mock_core.c` 为动态库并返回其路径
///
/// # 参数
///
/// - `id`: 构建标识（每个测试传入唯一值，避免并行测试输出冲突）
/// - `defines`: 额外的编译宏（如 `MOCK_NO_RETRO_RUN`，用于模拟缺失符号）
///
/// # 返回值
///
/// 编译产出的动态库绝对路径（位于 `CARGO_TARGET_TMPDIR`）。
///
/// # Panics
///
/// 系统 cc 不存在或 mock 核心编译失败时 panic（夹具是测试的前置条件，
/// 失败即测试失败）。
pub fn build_mock_core(id: &str, defines: &[&str]) -> PathBuf {
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let out_path = out_dir.join(mock_core_file_name(id));

    let mut cmd = std::process::Command::new("cc");
    cmd.args(["-shared", "-fPIC"])
        .arg("-I")
        .arg("tests/fixtures")
        .arg("tests/fixtures/mock_core.c")
        .arg("-o")
        .arg(&out_path);
    for define in defines {
        cmd.arg(format!("-D{define}"));
    }

    let status = cmd
        .status()
        .expect("无法启动系统 cc，mock 核心夹具编译失败");
    assert!(status.success(), "mock 核心编译失败（exit {status:?}）");

    out_path
}
