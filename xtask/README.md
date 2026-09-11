# xtask — MinUI 构建辅助工具

`xtask` 是 MinUI（Rust 重写版）的构建辅助 crate。它把 C 原版散落在顶层 `makefile`、各平台 `makefile.env`、`makefile.toolchain` 里的构建逻辑，收敛成一个可测试、免 shell 注入风险的 Rust 程序。

**使用方式**：`cargo xtask <命令>`——`xtask` 是定义在 `.cargo/config.toml` 的 cargo alias（等价 `cargo run -p xtask -- <命令>`）。

## 模块定位与职责

- **`toolchain` 流水线**：`setup → build → system → platform → special → tidy → package` + `all` 聚合入口——从源码到发布 zip 的全链路；编译统一经工具链容器（`toolchain/Dockerfile`，宿主机只需容器引擎）
- **辅助命令**：`doc`（rustdoc + 聚合首页）/ `test` / `lint`（fmt + clippy 严格模式，双形态范围）/ `clean`（build/，`--all` 连带 target/）/ `help`
- **零第三方依赖面**：仅 clap——不依赖任何项目 crate，独立编译可测

## 快速上手

```sh
cargo xtask toolchain all --platform tg5040 --device smart   # 完整流水线 → 发布 zip
cargo xtask help                    # 详细使用说明
cargo xtask test                    # 通用层测试（纯逻辑，无需交叉工具链）
```

**环境依赖**：容器引擎（podman/docker，编译用）、git（setup/package 溯源）、系统 `zip`（package 打包）。发布产物：`releases/MinUI-<platform>-<device>-YYYYMMDD-N-{base,extras}.zip`。

## 详细文档

完整文档（模块布局、流水线逐步推导、发布包语义、否决方案清单、设计决策记录、FAQ、命令行 API 面）见 [docs/modules/xtask.md](../docs/modules/xtask.md)。
