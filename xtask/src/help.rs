//! `help` 命令模块
//!
//! 输出 xtask 的详细使用说明：各子命令职责、参数、
//! 与 C 原版 makefile 的对应关系、完整调用示例。
//!
//! 对应 C 原版：`make help`（minui-rs 新增的详细说明命令）。
//! clap 的 `--help` 提供短帮助，本命令提供项目级详细说明。
//!

/// 执行 `help` 命令：打印详细使用说明。
///
/// # 返回值
///
/// - `Ok(())`：打印成功（当前不会失败）
/// - `Err(String)`：当前不会返回错误
pub(crate) fn run() -> Result<(), String> {
    println!("{}", help_text());
    Ok(())
}

/// 生成详细使用说明文本。
///
/// 覆盖：顶层命令列表、toolchain 各步骤职责与 C 原版对应关系、
/// `--platform`/`--device` 参数语义、完整调用示例。
///
/// # 返回值
///
/// 使用说明字符串。
fn help_text() -> String {
    r#"MinUI xtask — 构建辅助工具（Rust 重写版）

用法: cargo xtask <子命令>

顶层命令:
  toolchain   构建流水线（对齐 C 原版 makefile 的 setup/special/tidy/build/system/cores/package）
  doc         生成 rustdoc 文档并自动打开聚合首页
             （无参数：排除全部平台 crate，只查通用层；
               --platform <p> --device <d> 成对必填，连带生成该平台文档；
               首页自建自 crates.js；打开失败时错误信息含路径）
  clean      清理构建产物（删除 build/，--all 时连带删除 target/）
             （对应 C 原版 make clean 的 rm -rf ./build；目录不存在视为
              成功；无交互确认，删除结果打印到 stdout）
  test        运行单元测试（无参数：排除全部平台 crate，只测通用层；
               --platform <p> --device <d> 成对必填，连带测试该平台——
               平台排除自动扫描 platforms/ 目录，新增平台零维护；
               render 字体测试失败为已知限制，见 fix-render-font-test）
  lint        格式化 + 静态检查（严格模式）
             （cargo fmt --all --check 全 workspace + cargo clippy
              --workspace --all-targets——无参数排除全部平台 crate，
              --platform <p> --device <d> 成对必填连带检查该平台；
              既有问题会导致失败——预期行为，见 fix-workspace-fmt）
  help        显示本使用说明

toolchain 子命令（除说明外均要求 --platform <name>）:
  all        完整流水线: setup → build → system → platform → special → tidy → package
             （对应 C 原版 make PLATFORM=x 的 common 与 make 的 setup/special/package 顺序）
             --platform <name> 必填; --device <device> 必填（无默认）
  setup      准备干净的 build 目录并复制 skeleton（全局步骤，无需 --platform）
             （清空 build/ → 复制 skeleton → 删 .keep/*.meta → 写 build/hash.txt，需 git）
  special    处理 BOOT 目录重命名（BOOT/common → .tmp_update、BOOT/trimui → BASE/ 等，
             全局步骤，无需 --platform；缺失目录跳过）
  tidy       兼容旧卡（复制新平台 install.sh 到旧平台位置，如 tg5040 → tg3040）
             --platform <name> 必填
  build      编译通用二进制（minui/minarch/clock/minput，交叉编译）
             --platform <name> 必填; --device <device> 必填（决定编译 feature）
             （平台→target 映射 xtask 内置：aarch64/armv7/macOS host；
               需要 rustup 安装对应 target + ARM linker——环境缺工具链报错属环境问题）
  system     复制通用二进制进发布包（纯复制，不承担完整性检测——完整性验收
              唯一归 package 步骤）
              minui/minarch → build/SYSTEM/<platform>/bin/；
              clock/minput → build/EXTRAS/Tools/<platform>/{Clock,Input}.pak/
              --platform <name> 必填
  platform   调用平台子 xtask 完成平台侧全流程（原 C cores 步骤——show/keymon 编译、
              libretro.so 编译与复制、install 等资源复制）
              --platform <name> 必填; --device <device> 必填（透传给平台子 xtask）
  package    打包发布（version.txt/commits.txt 需 git、3 个 zip 需系统 zip 命令；
              打包前校验发布包完整性）
              --platform <name> 必填; --device <device> 必填（发布名
              MinUI-<platform>-<device>-YYYYMMDD-N 含二者，区分设备产物）

--platform 参数:
  目标平台名（如 tg5040），对应 C 原版 make 的 PLATFORM 变量。
  toolchain 子命令: tidy/build/system/platform/all/package 必填；
  setup/special 为全局步骤无需平台。
  辅助命令（doc/test/lint）: --platform 可选——不带则排除全部平台只查
  通用层；带时必须同时带 --device（成对必填，无默认）连带检查该平台。

--device 参数:
  设备参数（smart/brick，必填无默认）。当前仅 tg5040 平台有设备区分（smart/brick）：
  toolchain 的 build/platform/all/package 用它决定编译 feature/透传平台子
  xtask/发布命名（Rust 版每设备独立打包——show 分辨率与安装图按 device 编译期确定，
  发布名须含 device 段区分）；
  doc/test/lint 辅助命令带 --platform 时必须同时带 --device（成对必填，无默认）。

调用示例:
  cargo xtask toolchain all --platform tg5040 --device smart
  cargo xtask toolchain build --platform tg5040 --device brick
  cargo xtask toolchain setup
  cargo xtask toolchain package --platform tg5040 --device smart
  cargo xtask clean
  cargo xtask clean --all
  cargo xtask doc                 # 排除全部平台，只查通用层
  cargo xtask doc --platform tg5040 --device smart   # 连带该平台
  cargo xtask lint
  cargo xtask lint --platform tg5040 --device brick
  cargo xtask test
  cargo xtask test --platform tg5040 --device smart
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_returns_ok() {
        assert!(run().is_ok());
    }

    #[test]
    fn help_text_contains_all_subcommands() {
        let text = help_text();
        for name in [
            "toolchain",
            "all",
            "setup",
            "special",
            "tidy",
            "build",
            "system",
            "platform",
            "package",
            "doc",
            "clean",
            "test",
            "lint",
            "help",
        ] {
            assert!(text.contains(name), "help 输出缺少子命令名: {name}");
        }
    }

    #[test]
    fn help_text_mentions_platform_and_device_args() {
        let text = help_text();
        assert!(text.contains("--platform"));
        assert!(text.contains("--device"));
    }
}
