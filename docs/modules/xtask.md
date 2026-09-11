# xtask — MinUI 构建辅助工具（Rust 重写版）

`xtask` 是 MinUI（Rust 重写版）的构建辅助工具。它把 C 原版散落在顶层 `makefile`、各平台 `makefile.env`、`makefile.toolchain` 里的构建逻辑，收敛成一个可测试、跨平台、无 shell 注入风险的 Rust 程序。

**使用方式**：`cargo xtask <子命令>`——`xtask` 是定义在 `.cargo/config.toml` 的 cargo alias（等价 `cargo run -p xtask -- <子命令>`）。

**两个命令面**：

- **`toolchain <子命令>`**：构建流水线，对齐 C 原版 makefile 的 `setup`/`special`/`tidy`/`build`/`system`/`cores`/`package` 目标——从源码到发布 zip 的全链路（第 6-14 章）
- **`doc` / `test` / `lint` / `help`**：辅助命令——文档生成、测试、格式检查、使用说明（第 2 章）

**当前状态**：✅ 全部命令已实现（仅依赖 clap）。平台自治已完成——show/keymon 拆为平台独立 crate，由平台子 xtask（`tg5040-xtask`）编译装配；辅助命令 doc/test/lint 双形态（无参排除全部平台 / 带 `--platform --device` 连带该平台）。已知限制为**预期行为**：`lint` 会因代码库既有格式问题失败、`test` 的 render 字体测试失败——详见第 2 章。

## 内容地图

| # | 章节 | 回答的问题 |
|---|------|-----------|
| 1 | [为什么需要 xtask](#1-为什么需要-xtask) | C 原版 makefile 有什么问题？为什么用 Rust 写构建工具？ |
| 2 | [快速上手与辅助命令](#2-快速上手与辅助命令) | 需要什么环境？所有命令怎么用？doc/test/lint 怎么工作？ |
| 3 | [依赖关系](#3-依赖关系) | 为什么只依赖 clap？为什么不用 zip crate / 浏览器 crate？ |
| 4 | [核心概念](#4-核心概念) | 全局步骤 vs 平台步骤？build/ 是什么？为什么没有 .elf？ |
| 5 | [模块总览](#5-模块总览) | 源文件各自负责什么？ |
| 6 | [流水线全链路推导](#6-流水线全链路推导) | 从 `cargo xtask toolchain all` 到发布 zip 的完整数据流（深度章节） |
| 7 | [setup](#7-setup) | 如何准备干净的 build 目录？ |
| 8 | [build](#8-build) | 平台→target 怎么映射？feature 怎么传递？ |
| 9 | [system](#9-system) | 两族通用二进制怎么复制？为什么不做完整性检测？ |
| 10 | [platform](#10-platform) | 平台子 xtask 的边界在哪里？怎么调用？ |
| 11 | [special](#11-special) | BOOT 目录如何重命名？缺失目录为什么跳过？ |
| 12 | [tidy](#12-tidy) | 旧卡兼容文件怎么复制？ |
| 13 | [package](#13-package) | version.txt/commits.txt/zip 怎么生成？ |
| 14 | [all（聚合入口）](#14-all聚合入口) | 七步流水线为什么按这个顺序？ |
| 15 | [与原 C makefile 对比](#15-与原-c-makefile-对比) | 全维度对照表 |
| 16 | [工程实践：测试策略 + 否决方案清单](#16-工程实践测试策略--否决方案清单) | 可测性设计 + 所有被否决的方案 |
| 17 | [常见问题 FAQ](#17-常见问题-faq) | 新手高频疑问 |
| 18 | [公开 API](#18-公开-api) | 命令行 API 面 |

---

## 1. 为什么需要 xtask

### 1.1 C 原版的构建系统长什么样

MinUI 的 C 原版（`MinUI/` 目录）用 **makefile** 做构建。构建逻辑分散在三类文件里：

| 文件 | 内容 |
|------|------|
| `makefile` | 顶层流水线：`setup`（清空 build/、复制 skeleton）、`special`（BOOT 目录重命名）、`tidy`（旧卡兼容）、`build`（编译）、`system`（复制二进制）、`cores`（libretro 核心）、`package`（打包） |
| `makefile.env` | 各平台的 ARCH/编译器变量——每个平台一份，重复且难维护 |
| `makefile.toolchain` | docker 容器内的交叉编译封装 |

Rust 重写版的目标：把这份构建知识**收敛成一个 Rust 程序**，让"从源码到发布包"的全流程可以像普通代码一样被测试、被审查、被安全调用。

### 1.2 C 原版的四个痛点

**痛点 1：`system()` 字符串拼接有注入风险**

C 原版到处用 `system("killall -STOP keymon.elf")` 这类字符串拼接调用外部命令——命令参数一旦包含用户可控内容（文件名、路径），就可能被 shell 解析成其他命令。xtask 全部改用 `std::process::Command` 的**参数数组直传**，不经 shell（`src/utils.rs:60` 的 `run_command`）——这是 xtask 作为"构建编排器"最重要的安全基线。

**痛点 2：shell 脚本不可单测**

makefile 的每个目标都是 shell 命令序列——失败语义、边界条件（目录缺失怎么办？命令不存在怎么办？）都无法用单元测试锁定。xtask 把每个步骤拆成**可测纯函数**（如 `build_commands` 返回命令列表、`apply_special` 接受目录路径），用 `#[cfg(test)]` 单测覆盖（第 16 章有完整清单）。

**痛点 3：ARCH 硬编码在 makefile.env**

每个平台一份 makefile.env 声明架构变量——12 个平台跨 2 种 ARM 架构（aarch64/armv7），复制粘贴多、改一处漏一处。xtask 把平台→target 映射**内置为一张表**（`toolchain/build.rs:40` 的 `platform_target`），集中在 `build.rs` 一个文件（第 8 章详解）。

**痛点 4：docker toolchain 依赖**

C 原版交叉编译要起 docker 容器（makefile.toolchain），容器内装交叉工具链。
xtask 沿用"容器内编译"的思路但收敛为**工具链容器**：一个镜像装齐 Rust +
SDL2 + cores 交叉 gcc，真机编译经 `podman run` 执行（对齐 C 原版容器化
编译、避免宿主装 ARM 工具链的负担——代价：编译需容器引擎与镜像，见第 8 章）。

### 1.3 cargo xtask 模式科普

**xtask 模式**是 Rust 生态里"用 Rust 写构建脚本"的惯用做法：在 workspace 里放一个独立的 `xtask` crate（bin），通过 `.cargo/config.toml` 的 alias 注册成 `cargo xtask <命令>`，让 cargo 帮我们编译和运行它。

```
.cargo/config.toml（minui-rs/）
[alias]
xtask = "run --quiet --package xtask --"
# 等价于: cargo run -p xtask -- <子命令>
```

`--quiet` 抑制 cargo 编译 xtask 时的进度输出，保留 xtask 自身输出——命令输出干净、可管道化。alias 只加这一个键，不改 build/rustflags 等全局行为。

为什么用 Rust 写构建工具而不是 Make/CMake/Python 脚本？

| 维度 | Make/Shell | xtask（Rust） |
|------|-----------|--------------|
| 类型安全 | 字符串拼接，拼错运行时才发现 | 编译期检查参数与分支 |
| 错误处理 | `set -e` + 隐式退出码 | `Result<(), String>` 显式传播，错误信息带 stderr |
| 可测试性 | 无 | 纯函数可单测（`cargo test -p xtask` 有 40+ 用例） |
| 跨平台 | 各平台 shell 差异大 | 一套 Rust 代码（无 shell 脚本、无平台分支） |
| 依赖 | 系统工具 | cargo 统一管理（本工具仅 clap） |

**与 C 原版的流水线对应**（xtask 的 `toolchain` 子命令组）：

```
C 原版 makefile                 xtask 命令                章节
──────────────────────────────────────────────────────────
make setup                   toolchain setup           第 7 章
make build                   toolchain build           第 8 章
make system                  toolchain system          第 9 章
make cores（改名 platform）    toolchain platform        第 10 章
make special                 toolchain special         第 11 章
make tidy                    toolchain tidy            第 12 章
make package                 toolchain package         第 13 章
make PLATFORM=x 组合          toolchain all             第 14 章
make help                    cargo xtask help          第 2 章
```

注意两个改名：`cores` → `platform`（新职责是"调用平台子 xtask 完成平台侧全流程"，而非复制 libretro 核心——见第 10 章）；新增 `doc`/`test`/`lint` 三个 C 原版没有的辅助命令（Rust 项目标配的质量门禁）。

---

## 2. 快速上手与辅助命令

### 2.1 环境依赖清单

真机编译（toolchain build/platform 的编译部分）统一经**工具链容器**执行
（`toolchain/Dockerfile`，见第 8 章）——宿主机只需容器引擎，**不需要**
ARM linker/SDL2/GNU make/交叉 gcc（都在容器内）。

| 依赖 | 用于 | 缺失时行为 |
|------|------|-----------|
| Rust（rustup 安装） | cargo 本身 | 一切命令不可用 |
| 容器引擎（podman/docker） | build/platform 的编译（经工具链容器） | 编译报错——需装引擎并构建镜像（见 8.2） |
| `git` | `setup` 写 hash.txt、`package` 写 version.txt/commits.txt | 两个步骤**硬性报错**（C 原版同样依赖 git） |
| 系统 `zip` 命令 | `package` 打包 3 个 zip | `package` **硬性报错**（发布环境须自备） |

**doc/test/lint 双形态**：无参数时只检查通用层（自动排除 `platforms/` 下全部平台 crate，扫描 Cargo.toml——新增平台零维护）；带 `--platform <p> --device <d>`（成对必填）时连带检查该平台，此时需宿主能编译该平台代码（平台代码依赖 SDL2——宿主本机架构检查需要 SDL2 开发库；真机交叉编译始终经容器）。

### 2.2 全部命令调用示例

```sh
# ── toolchain 流水线 ─────────────────────────────────────────────
cargo xtask toolchain all --platform tg5040 --device smart   # 完整流水线
cargo xtask toolchain all --platform tg5040 --device brick

# 单步执行
cargo xtask toolchain setup                        # 清空 build/ + 复制 skeleton
cargo xtask toolchain build --platform tg5040 --device smart   # 编译通用二进制（minui/minarch/clock/minput）
cargo xtask toolchain system --platform tg5040     # 复制两族二进制（minui/minarch → bin、clock/minput → Tools pak）
cargo xtask toolchain platform --platform tg5040 --device smart   # 调平台子 xtask（tg5040-xtask）
cargo xtask toolchain special                      # BOOT 目录重命名
cargo xtask toolchain tidy --platform tg5040       # 旧卡兼容
cargo xtask toolchain package --platform tg5040 --device smart   # 打包发布（命名含平台/设备段）

# ── 辅助命令（双形态：无参排除全部平台 / 带参连带该平台）──────────
cargo xtask doc      # 生成通用层文档（排除全部平台 crate）
cargo xtask doc --platform tg5040 --device smart   # 连带生成该平台文档
cargo xtask clean    # 清理 build/
cargo xtask clean --all    # 清理 build/ + target/（编译产物缓存）
cargo xtask test     # 运行通用层单元测试（排除全部平台 crate）
cargo xtask test --platform tg5040 --device smart  # 连带测试该平台
cargo xtask lint     # fmt + clippy（通用层）
cargo xtask lint --platform tg5040 --device smart  # 连带检查该平台
cargo xtask help     # 详细使用说明
```

**tg5040 发布流程速查**（从环境准备到发布包的完整步骤）：

```sh
# ① 环境准备（一次性）
# 容器引擎（默认 podman；macOS: brew install podman && podman machine init && podman machine start）
podman build -t minui-toolchain:latest toolchain/   # 构建工具链镜像（apt + rustup 下载需数分钟）
# 引擎可配置：export MINUI_CONTAINER_ENGINE=docker

# ② 完整流水线
cargo xtask toolchain all --platform tg5040 --device smart
# 平台侧（show/keymon 编译、cores make、装配）由 platform 步骤的平台子 xtask 完成：
cargo xtask toolchain setup
cargo xtask toolchain build --platform tg5040 --device smart
cargo xtask toolchain system --platform tg5040
cargo xtask toolchain platform --platform tg5040 --device smart
cargo xtask toolchain special
cargo xtask toolchain tidy --platform tg5040
cargo xtask toolchain package

# ③ 产物
# releases/MinUI-<platform>-<device>-YYYYMMDD-N-base.zip 与 -extras.zip（发布包）
# build/latest.txt（本次发布名）
```

### 2.3 `cargo xtask help` 输出节选

`help` 命令输出**项目级详细说明**（`src/help.rs:34` 的 `help_text()`），与 clap 的 `--help` 短帮助互补——`--help` 列出参数，`help` 讲清每一步做什么、与 C makefile 如何对应：

```
MinUI xtask — 构建辅助工具（Rust 重写版）

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
...（toolchain 八步骤 + 参数 + 调用示例，完整输出见 src/help.rs）
```

**为什么自定义 `help` 而不是用 clap 内置？**clap 的 `--help` 只列参数名和一句话描述——承载不了"每一步做什么、与 C makefile 如何对应、version.txt 如何生成"这类项目级说明。所以禁用 clap 内置 help 子命令（`disable_help_subcommand = true`，`src/main.rs:36`），实现自定义 `help`。

### 2.4 辅助命令原理（各一段）

doc/test/lint 三者共享**双形态范围语义**（`src/utils.rs:217` 的 `aux_scope_args`）：无参数时自动排除 `platforms/` 下全部平台相关 crate（`utils::platform_crate_names` 递归扫描 `platforms/**/Cargo.toml` 收集包名——平台 lib/show/keymon/平台子 xtask 全含，**新增平台零维护**），只查通用层；带 `--platform <p> --device <d>`（clap 双向 `requires` 成对必填，无默认）时排除**其他**平台、保留该平台 + 通用层，并加 `--features <p>/<d>`（device feature 编译期二选一，平台 lib 无默认——`compile_error!` 强制显式指定）。

#### doc —— 聚合首页从哪来？

`cargo doc` 在纯 workspace（无根 package）下**不生成根 index.html**——只有各 crate 的 index.html + 一个 `crates.js`（含 `window.ALL_CRATES = [...]` crate 列表）。xtask 的 `doc` 命令（`src/doc.rs:31` 的 `run()`）三步生成完整文档：

```
① 清理 target/doc/ 下非共享资源目录（保留 search.index/src/static.files/trait.impl）
② cargo doc --workspace --no-deps [范围参数]   ← 无参排除全部平台 / 带参连带该平台
③ 读 crates.js 的 ALL_CRATES → 自建聚合首页 target/doc/index.html → 跨平台打开
```

实现细节：

- **清理**（`clean_doc_dir`，`src/doc.rs:72`）：rustdoc 生成的共享资源目录（`search.index`/`src`/`static.files`/`trait.impl`）删除 crate 文档目录时必须保留——它们由 rustdoc 重建，不属于某个 crate
- **自建首页**（`write_index_page`，`src/doc.rs:106`）：把 `crates.js` 的 `ALL_CRATES` 列表渲染成 `<a href="common/index.html">common</a>` 形式的聚合页——crate 列表不硬编码，未来新增 crate 自动出现在首页
- **crate 名解析**（`list_crate_names`，`src/utils.rs:160`）：解析 `window.ALL_CRATES = [...]`（容错空格/换行，格式异常报错并提示手动打开）

**为什么必须先清理？**`crates.js` 是 rustdoc **每次运行的读-合并-写增量产物**——增量构建（源码无变化）不重跑 rustdoc，文件保持上一次内容。若历史上有过不带 `--no-deps` 的构建，crates.js 会残留依赖 crate 名——聚合首页将列出不存在的链接。清理 + 全量重建后，crates.js 恰好等于本次文档化的 crate 列表。

**打开浏览器为什么用 `spawn()` 不用 `output()`？**浏览器进程独立于 xtask 运行，等待其退出没有意义。`spawn()` 只检查启动是否成功、不等待不检查退出码——浏览器进程独立运行（`src/utils.rs:202` 的 `open_browser`，macOS `open`/Linux `xdg-open`）。手写系统命令而非引入 webbrowser/opener crate——仅为打开浏览器引入依赖不划算（第 3 章）。

#### test —— 平台范围怎么定？

`test` 命令（`src/test.rs`）按双形态执行：无参数 `cargo test --workspace --exclude <全部平台 crate>`（只测通用层）；带 `--platform <p> --device <d>` 时 `cargo test --workspace --features <p>/<d>`（排除其他平台，连带测试该平台——platform lib/show/keymon/平台子 xtask 全纳入）。

**已知限制**：render crate 的 8 个测试依赖字体路径，在部分环境失败——由后续 `fix-render-font-test` 变更修复。

#### lint —— 为什么"严格模式"会失败？

`lint`（`src/lint.rs`）执行 `cargo fmt --all --check`（fmt 始终全 workspace，格式化与平台无关）+ `cargo clippy --workspace --all-targets [范围参数] -- -D warnings`（范围语义同 test/doc）——`-D warnings` 把警告升级为错误。当前代码库存在既有格式 diff 与 clippy 警告，**lint 命令会失败**——这是**预期行为**（lint 的意义就是暴露问题，而非装作没问题），由后续 `fix-workspace-fmt` 变更清理。平台 crate 的接口约束由 Rust `pub(crate)` 可见性承担，lint 不维护平台引用白名单检查。

#### clean —— 为什么不用 `rm -rf`？

C 原版 `make clean` 就是一行 `rm -rf ./build`（`MinUI/makefile:88-89`）；Rust 版（`src/clean.rs:47` 的 `run()`）改用 `std::fs::remove_dir_all`——**免 shell**（参数数组直传，没有注入面）。目录不存在视为成功（对齐 `rm -rf` 宽松语义），删除结果打印到 stdout 便于审计，不做交互确认（保持脚本化可用，如 CI 里 `clean && all`）。

`--all` 连带删除 `target/`——Rust 重写版新增的清理项（C 版没有编译产物目录）。默认形态**不**删 target：`target/` 是编译缓存，删除后下次构建要全量重编；只有显式 `--all` 才清理，避免误伤。

---

## 3. 依赖关系

### 3.1 Cargo.toml 全文

```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true

[[bin]]
name = "xtask"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
```

**xtask 的依赖面 = 只有 clap**。`cargo tree -p xtask` 的依赖树不包含 common/render/minui/minarch/platform-tg5040 任何项目 crate——xtask 是**独立于主项目**的工具。

### 3.2 三个"弃用"决策（为什么没有依赖 X）

**弃用 cargo_metadata**：初版曾依赖它查询 workspace 元数据——但新架构没有任何消费方：平台清单靠扫描 `platforms/` 目录、命令执行靠 `std::process::Command`。无消费方的依赖是死重，移除后 `cargo tree -p xtask` 依赖面更小更易审计。

**弃用 zip crate**：打包调用系统 `zip` 命令而非 Rust zip crate——与 C 原版行为完全一致（`.DS_Store` 排除、符号链接、zip 内部结构可人工核对），`zip` 是 C 原版打包链的既有依赖。代价：发布环境无 zip 命令时 package 步骤报错（环境问题，发布环境须自备）。

**弃用 webbrowser/opener crate**：仅为"打开浏览器"引入 41M/32M 下载的传递依赖树（log/url/obj2c 等）不划算——`utils::open_browser` 手写系统命令（macOS `open`/Linux `xdg-open`），`spawn()` 免等待（第 2.4 节）。

---

## 4. 核心概念

在深入流水线之前，先建立四个心智模型——每个概念都有对应的深度章节，这里只给结论和直觉。

### 4.1 全局步骤 vs 平台步骤（`--platform` 作用域）

toolchain 的 8 个子命令分两类——**是否处理平台特定内容**：

| 类型 | 子命令 | `--platform` | 说明 |
|------|--------|-------------|------|
| **全局步骤** | `setup` / `special` | 不要求 | 处理全平台共有的 skeleton 内容——与 C 原版一致（C 的 setup/special 同样不依赖 PLATFORM） |
| **平台步骤** | `tidy` / `build` / `system` / `platform` / `package` / `all` | **必填** | 处理特定平台的内容（编译、复制、打包分支；package 的发布名含平台/设备段） |

**为什么 `tidy` 要 `--platform`？** C 原版是"按 PLATFORMS 列表条件执行"（一次构建多个平台）；Rust 版是**单平台流水线**（一次一个平台），`tidy` 必须知道当前平台才能应用对应分支（tg5040 → tg3040？还是 rg35xxplus → rg40xxcube？）。`setup`/`special` 强加平台参数反而是误导；`package` 是例外——Rust 版每 (platform, device) 独立打包，发布名必须含平台/设备段（见 §13）。

### 4.2 `--device` 参数语义

`--device` **必填无默认**，仅对**存在设备区分的平台**有意义——当前只有 tg5040（smart/brick）。消费方：

```
--device smart/brick
   ├── toolchain build    → 决定编译 feature（--features tg5040/smart）
   ├── toolchain platform → 透传给平台子 xtask（决定 show 分辨率/安装图来源）
   └── doc/test/lint      → 与 --platform 成对必填，连带检查该平台
```

其他平台忽略该参数（后续平台接入时再收敛）。**辅助命令的 `--platform` 与 `--device` 必须成对出现**——不带则排除全部平台只查通用层；带了就必须两者都带（clap 双向 `requires`，无默认值，命令必须明确）。

### 4.3 build/ 是"组装台"

整个流水线围绕 `<项目根>/build/` 目录工作：`setup` 清空重建它，之后的每个步骤都往里面**写入内容**，最后的 `package` 把它组装成发布 zip。三条内容源汇入 build/：

```
skeleton/（源头骨架）── setup ──▶ build/
cargo 产物 ──────────── system ─▶ build/SYSTEM/<p>/bin/（通用二进制）
平台子 xtask ────────── platform ─▶ build/（show/keymon/.so/install 等，平台侧全流程）
```

build/ 的逐步骤演化是第 6 章深度章节的主题——**理解 xtask 就是理解 build/ 在每个步骤后变成了什么**。

### 4.4 无 `.elf` 后缀的命名约定

C 原版产物叫 `minui.elf`/`keymon.elf`（skeleton 的 launch.sh 引用同名）。Rust 版**统一去后缀**（`minui`/`minarch`/`keymon`/`clock`/`minput`/`show`）：

- cargo 产物本无 `.elf` 后缀——skeleton 引用必须与产物一致
- `minui-rs/skeleton/**`（22 处）与 `platforms/tg5040/install/boot.sh`（1 处）的 `.elf` 引用全部去除
- 与 C 原版的分叉是**刻意的**——Rust 版产物（含平台自治 bin show）天然无后缀兼容

### 4.5 退出码语义

xtask 的退出码分三档（`src/main.rs:64` 的 `main()`）：

```
0 —— 成功
1 —— 命令执行失败（错误信息打印到 stderr，前缀"错误: "）
2 —— clap 参数解析错误（clap 自行处理，xtask 不捕获）
```

```rust
// src/main.rs:64 —— 程序入口：解析 CLI、分发执行、统一错误处理
// 错误信息打印到 stderr 并以退出码 1 结束（对齐 C 原版 makefile
// 非零退出码语义）；clap 解析错误由 clap 自行处理（退出码 2）。
fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}
```

**为什么返回 `Result<(), String>` 而不是 `process::exit(1)`？**`exit` 不可单元测试；`Result` 返回给 `main` 集中处理——每个命令的 `run()` 都可被测试直接调用并断言错误信息。

---

## 5. 模块总览

### 5.1 文件布局

```
xtask/
├── Cargo.toml            ← 依赖声明（仅 clap derive）
└── src/
    ├── main.rs           ← clap 入口 + 顶层分发（Commands → doc/lint/test/help/toolchain）
    ├── utils.rs          ← 共享工具：路径解析/命令执行/平台扫描（platform_crate_names/aux_scope_args）/
    │                       浏览器打开/递归复制
    ├── doc.rs            ← cargo doc（双形态范围）+ 自建聚合首页 + 打开浏览器
    ├── lint.rs           ← cargo fmt --check + cargo clippy 严格检查（双形态范围）
    ├── test.rs           ← cargo test（双形态范围：无参排除全部平台 / 带参连带该平台）
    ├── clean.rs          ← 清理 build/（--all 连带 target/）
    ├── help.rs           ← 详细使用说明（与 clap --help 互补）
    └── toolchain/
        ├── mod.rs        ← ToolchainCommands 枚举 + 分发（8 个子命令）
        ├── all.rs        ← 聚合入口：setup → build → system → platform → special → tidy → package
        ├── setup.rs      ← 清空 build/ + 复制 skeleton + 删 .keep/*.meta + 写 hash.txt
        ├── special.rs    ← BOOT 目录重命名与派生（缺失跳过）
        ├── tidy.rs       ← 旧卡兼容复制（按平台分支）
        ├── build.rs      ← 平台→target 映射 + cargo build + feature 传递
        ├── system.rs     ← 复制两族通用二进制（纯复制，不检测；show/keymon 归平台子 xtask）
        ├── platform.rs   ← 调用平台子 xtask（cargo run -p <p>-xtask -- <device>）
        └── package.rs    ← version.txt/commits.txt + PAYLOAD 组装 + zip 打包
```

### 5.2 模块职责与 C 对应

| 文件 | 职责 | 对应 C 原版 |
|------|------|------------|
| `main.rs` | CLI 入口与顶层分发 | `make` 的顶层目标选择 |
| `utils.rs` | 跨命令共享工具（含平台自动扫描） | `makefile` 的公共变量/函数 |
| `doc.rs`/`lint.rs`/`test.rs`/`help.rs` | 辅助命令 | 无直接对应（Rust 项目新增） |
| `toolchain/mod.rs` | 子命令组枚举与分发 | `make PLATFORM=x` 的目标路由 |
| `toolchain/all.rs` | 聚合编排 | `make all` 的组合语义 |
| `toolchain/setup.rs` | 准备 build 目录 | `make setup`（makefile:91-109） |
| `toolchain/special.rs` | BOOT 重命名 | `make special`（makefile:114-129） |
| `toolchain/tidy.rs` | 旧卡兼容 | `make tidy`（makefile:131-141） |
| `toolchain/build.rs` | 通用二进制交叉编译 | `make build`（makefile.toolchain） |
| `toolchain/system.rs` | 通用二进制复制 | `make system`（makefile:49-56） |
| `toolchain/platform.rs` | 调平台子 xtask | `make cores` + `makefile.copy` + 平台 make |
| `toolchain/package.rs` | 打包发布 | `make package`（makefile:143-165） |

### 5.3 模块设计原则：一个文件一个命令

每个模块职责单一、一个文件对应一个命令（或一组强相关参数）——与 C makefile 目标一一对应，便于独立测试与后续变更逐个演化。**`all.rs` 是唯一涉及"多步骤编排"的文件**——其他步骤模块互不调用，全部经 `toolchain/mod.rs` 的 `run()` 分发（`src/toolchain/mod.rs:97`）。

---

## 6. 流水线全链路推导

**本章是理解 xtask 的核心。** 目标：从 `cargo xtask toolchain all --platform tg5040` 到设备上的发布 zip，把 build/ 目录的每一次变化、每一个文件的旅程、每一个可能的失败点讲清楚——读完本章，流水线里不应该有任何黑盒。

### 6.1 build/ 逐步骤演化

`build/` 是流水线的**组装台**——每个步骤都在它上面写入内容。以下按 `all` 的执行顺序展示每个步骤之后 build/ 的关键变化（以 tg5040 为例）：

```
┌────────────────────────────────────────────────────────────────────┐
│ 初始状态: build/ 不存在（或为上次构建的残留）                          │
├────────────────────────────────────────────────────────────────────┤
│ [setup]   清空 build/ → 递归复制 skeleton/ → 删 .keep/*.meta        │
│           → 写 build/hash.txt（git 短 hash）                        │
│   build/ = skeleton 完整副本 + hash.txt                             │
├────────────────────────────────────────────────────────────────────┤
│ [build]   cargo build -p minui/minarch/clock/minput                 │
│           （--features tg5040/smart + --target aarch64-... + --release）│
│   不动 build/——产物在 target/aarch64-unknown-linux-gnu/release/     │
│   （platform lib 作为 minui/minarch 的依赖被连带编译；show/keymon   │
│     不在此——平台自治，由 platform 步骤的平台子 xtask 编译）          │
├────────────────────────────────────────────────────────────────────┤
│ [system]  复制两族通用二进制（纯复制，不检测）                        │
│           minui/minarch → SYSTEM/tg5040/bin/                       │
│           clock/minput → EXTRAS/Tools/tg5040/{Clock,Input}.pak/     │
├────────────────────────────────────────────────────────────────────┤
│ [platform] cargo run -q -p tg5040-xtask -- smart（平台子 xtask）     │
│   ├ 编译 show/keymon（平台自治 bin，--features smart）               │
│   ├ make PLATFORM=tg5040（cores——编译 libretro.so，全流程必跑）      │
│   ├ 复制 show → SYSTEM/tg5040/bin/ 与 BOOT/common/tg5040/           │
│   │         keymon → SYSTEM/tg5040/bin/                             │
│   │         stock .so → SYSTEM/tg5040/cores/                        │
│   │         extras .so → EXTRAS/Emus/tg5040/*.pak/                  │
│   └ 复制 install/：boot.sh → BOOT/common/tg5040.sh                  │
│                 update.sh → SYSTEM/tg5040/bin/install.sh            │
│                 安装图（按 device 选源）→ BOOT/common/tg5040/        │
├────────────────────────────────────────────────────────────────────┤
│ [special]  BOOT/common → BOOT/.tmp_update                          │
│            BOOT/trimui → BASE/trimui                               │
│            .tmp_update 复制到 BASE/trimui/app/                      │
│   build/BOOT/ = .tmp_update（原 common）                            │
│   build/BASE/ = trimui（+ 未来的 miyoo/magicx）                     │
├────────────────────────────────────────────────────────────────────┤
│ [tidy]    SYSTEM/tg5040/bin/install.sh                              │
│           → SYSTEM/tg3040/paks/MinUI.pak/launch.sh（旧卡兼容）       │
├────────────────────────────────────────────────────────────────────┤
│ [package] SYSTEM → PAYLOAD/.system（重命名）                        │
│           BOOT/.tmp_update → 复制到 PAYLOAD/                        │
│           zip PAYLOAD → PAYLOAD/MinUI.zip → build/BASE/MinUI.zip    │
│           BASE → releases/<name>-base.zip；EXTRAS → extras.zip      │
│           写 build/latest.txt                                       │
└────────────────────────────────────────────────────────────────────┘
```

**关键认知**：

1. **build/ 的内容是逐步累积的**——前一步的产物是后一步的输入（tidy 复制 platform 生成的 install.sh；package 消费 setup 复制的 skeleton + system 放入的二进制 + platform 放入的脚本）
2. **三步"注入"、三步"变形"、一步"消费"**：setup/system/platform 注入内容；special/tidy/package 变形与消费
3. **`package` 是唯一的"消费端"**——它把 build/ 整个组装成发布产物，此后 build/ 使命完成

### 6.2 二进制旅程：minarch 从源码到设备

以 `minarch` 为例，追踪一个二进制从源码到设备 SD 卡的完整旅程（对照 `platforms/tg5040/README.md` 第 6.2 节的设备文件去向表）：

```
minarch 源码（crates/minarch/）
    │
    ▼ [build] cargo build -p minarch --features tg5040/smart
    │         --target aarch64-unknown-linux-gnu --release
target/aarch64-unknown-linux-gnu/release/minarch     ← cargo 产物（无 .elf）
    │
    ▼ [system] copy_binaries 复制（minui/minarch → SYSTEM/bin，clock/minput → Tools pak）
build/SYSTEM/tg5040/bin/minarch
    │
    ▼ [special] BOOT/trimui → BASE/trimui（minarch 所在 SYSTEM 目录不动）
    ▼ [package] SYSTEM → PAYLOAD/.system（整体重命名）
build/PAYLOAD/.system/tg5040/bin/minarch
    │
    ▼ [package] zip -r MinUI.zip .system .tmp_update
releases/<name>-base.zip（内含 MinUI.zip → 用户解压 → 设备更新）
    │
    ▼ 设备安装流程（boot.sh → unzip MinUI.zip 到 SD 卡）
/mnt/SDCARD/.system/tg5040/bin/minarch     ← 设备上的最终位置
```

**每个环节都遵守无 `.elf` 后缀约定**——cargo 产物无后缀、system 复制同名、skeleton 的 launch.sh 引用同名，命名一致性贯穿整条链。

### 6.3 skeleton 变形与三条内容源

**skeleton/ 是发布内容的"源头骨架"**——`setup` 完整复制它到 build/，后续步骤再变形。Rust 版 skeleton 目前只有 `BOOT/common` + `BOOT/trimui` 两个设备家族目录（无 miyoo/magicx），所以 `special` 的实际执行路径是：

```
build/BOOT/common ──rename──▶ build/BOOT/.tmp_update
build/BOOT/trimui ──rename──▶ build/BASE/trimui
.tmp_update ──copy──▶ build/BASE/trimui/app/.tmp_update
```

**三条内容源汇入 build/**（第 4.3 节的完整图）：

```
┌──────────────────────────────────────────────────────────────┐
│                     三条内容源                                │
│  skeleton/（skeleton 目录）                                    │
│    └──▶ [setup] 递归复制 → build/（源头骨架）                   │
│  cargo 产物（target/<triple>/release/）                        │
│    └──▶ [system] 复制 → SYSTEM/<p>/bin/ + EXTRAS/Tools/<p>/*.pak │
│  platforms/<p>/{install/,cores/,show/,keymon/,xtask/}（平台自治）   │
│    └──▶ [platform] 平台子 xtask → build/（show/keymon/.so/install）│
│                                                              │
│  build/ = 骨架 + 二进制 + 平台特有文件 → [package] 组装成 zip   │
└──────────────────────────────────────────────────────────────┘
```

**为什么平台特有内容必须由平台子 xtask 注入？**每个平台的 install/ 目录、安装图、libretro 核心清单、show/keymon 编译都不同——xtask 若硬编码这些，每加一个平台就要改 xtask。所以 platform 步骤只负责 `cargo run -q -p <p>-xtask -- <device>`（命名约定 `<platform>-xtask`），平台特有逻辑（编译 show/keymon、cores make、装配复制）归平台子 xtask（`platforms/tg5040/xtask/`）。

**package 后的 build/ 完整结构**（tg5040 示例——流水线终点）：

```
build/（流水线终点，对照设备 SD 卡结构）
├── hash.txt                    ← setup 写入（git 短 hash）
├── latest.txt                  ← package 写入（发布名）
├── BASE/                       ← special 移入的设备家族目录 + MinUI.zip
│   ├── trimui/
│   ├── MinUI.zip               ← package 移入（PAYLOAD 打包产物）
│   └── （未来的 miyoo/magicx 及派生）
├── EXTRAS/                     ← skeleton 自带（额外工具包）
├── SYSTEM/ → 已被移走（package 重命名为 PAYLOAD/.system）
└── PAYLOAD/                    ← package 组装（发布包内部结构）
    ├── .system/                ← 原 build/SYSTEM（含 tg5040/bin/{minui,minarch,clock,minput,show,keymon,install.sh} 与 cores/）
    └── .tmp_update/            ← 原 build/BOOT/.tmp_update（更新检测，含 tg5040/ 的 show 与安装图）
```

### 6.4 失败点地图

流水线每个步骤可能失败的地方——**理解失败语义是使用流水线的另一半知识**：

| 步骤 | 硬报错（非零退出） | 宽容（跳过/继续） |
|------|-------------------|------------------|
| `setup` | git 缺失（无法写 hash.txt） | build/ 不存在（视为空） |
| `build` | 未知平台（列出已支持平台）；编译失败 | — |
| `system` | 任一源编译产物缺失（提示先 build） | 不承担完整性检测（唯一验收点在 package 的全量校验） |
| `platform` | 平台子 xtask 执行失败（cargo run 报错/平台子 xtask 内部任一环节失败） | — |
| `special` | 移动/复制失败 | **源目录缺失时跳过该操作**（C 原版假定目录存在，Rust 版未来扩展自然覆盖） |
| `tidy` | **源文件缺失**（install.sh 由 platform 步骤生成——提示流水线顺序） | **无分支平台视为成功**（zero28 等） |
| `package` | 完整性校验缺失（列出清单）；git 缺失；zip 命令缺失；读写失败 | — |

**依赖顺序约束**（为什么 all 的顺序不能乱）：

```
setup ──▶ build ──▶ system ──▶ platform ──▶ special ──▶ tidy ──▶ package
  │          │          │           │             │          │         │
  │          │          │           │             │          │         └─ 消费前面所有步骤的产物
  │          │          │           │             │          └─ 依赖 platform 的 install.sh
  │          │          │           │             └─ 依赖 setup 复制的 skeleton
  │          │          │           └─ 依赖 setup 复制的骨架目录
  │          │          └─ 依赖 build 的 cargo 产物
  │          └─ 依赖 setup 的 workspace（cargo 自管理）
  └─ 起点：清空重建
```

**平台自治后 `all` 可完整跑通**：system 复制两族通用二进制（show/keymon 是平台自治 bin，由 platform 步骤的平台子 xtask 编译装配），发布包完整性由末尾 package 步骤全量校验。完整跑通需环境具备容器引擎 + 工具链镜像（编译）+ git + zip + 网络（cores 克隆上游）；缺任一环境时报错属环境问题（见 6.4 失败点地图）。

---

## 7. setup

**职责**：准备干净的 build 目录。对应 C 原版 `make setup`（makefile:91-109）。

### 7.1 四步流程

```
① 清空 build/（remove_dir_all，不存在视为成功）
② 递归复制 skeleton/ → build/（copy_dir_recursive——std 无内置递归复制，手写遍历）
③ 删除 build/ 下所有 .keep 与 *.meta 文件
   （skeleton 用 .keep 保持空目录、*.meta 标记作者文件——发布前必须删除）
④ 写 build/hash.txt：git rev-parse --short HEAD 的 stdout
```

### 7.2 与 C 原版的差异（三处）

| C 原版 make setup | Rust 版 | 差异理由 |
|------------------|---------|---------|
| 先做 `tty -s` 交互终端检查 | **不做** | Rust 工具无需终端检测 |
| 复制 readmes（为 Linux fmt 格式化机制服务） | **不做** | Rust 版无此流程——skeleton 的 README.txt 是成品 |
| hash.txt 写 `workspace/` | 写 **`build/`** | Rust 无 workspace/ 目录 |

### 7.3 为什么 git 缺失必须报错

```rust
// src/toolchain/setup.rs:100 —— 获取 git 短 hash（git rev-parse --short HEAD 的 stdout）
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
```

`hash.txt` 是发布包溯源信息（用户能知道自己的包基于哪个 commit）——git 缺失时无法生成，**硬性报错**（C 原版同样依赖 git）。注意返回值**含末尾换行**（`echo` 语义一致，C 原版 `echo $(BUILD_HASH)` 同）。

---

## 8. build

**职责**：编译通用二进制（minui/minarch/clock/minput）。对应 C 原版 `make build`（经 makefile.toolchain 的 docker toolchain 交叉编译）——Rust 版经**工具链容器**执行 `cargo build`（8.3 节）。show/keymon 是平台自治 bin（独立 crate），由 platform 步骤的平台子 xtask 编译，**本步骤不涉及**。

### 8.1 平台→target 映射表（xtask 内置）

```rust
// src/toolchain/build.rs:40 —— 平台→target 映射（xtask 内置）
pub fn platform_target(platform: &str) -> &'static str {
    match platform {
        // 64 位 ARM（armv8-a / aarch64）
        "tg5040" | "zero28" | "m17" | "gkdpixel" | "miyoomini" | "rg35xxplus" | "my282"
        | "magicmini" | "my355" | "rgb30" => "aarch64-unknown-linux-gnu",
        // 32 位 ARM（armv7-a 硬浮点）
        "rg35xx" | "trimuismart" => "armv7-unknown-linux-gnueabihf",
        // 未知平台——由 validate_platform_target 先行拦截（此处不兜底）
        _ => unreachable!("未知平台应在 validate_platform_target 处报错"),
    }
}
```

12 个平台跨 2 种架构（映射依据 = C 原版各平台 makefile.env 的 ARCH 定义，探索核验；macos 是 C 原版 dummy 开发辅助平台而非发布目标，Rust 版不登记）：

| 架构 | target triple | 平台 |
|------|--------------|------|
| 64 位 ARM | `aarch64-unknown-linux-gnu` | tg5040/zero28/m17/gkdpixel/miyoomini/rg35xxplus/my282/magicmini/my355/rgb30（10 个） |
| 32 位 ARM | `armv7-unknown-linux-gnueabihf` | rg35xx/trimuismart（2 个） |

每个平台都有确定的交叉 target（无 host 平台）——`platform_target` 恒返回 triple，产物恒在 `target/<triple>/release/`（第 9.3 节）。

**为什么映射表内置在 xtask 而非平台自声明？**否决了"平台自身在 Cargo.toml metadata 声明 target"——xtask 需解析各平台 Cargo.toml，增加复杂度；映射是**构建编排知识**，归 xtask 集中管理。未知平台报错并列出已支持平台（`validate_platform_target`，`src/toolchain/build.rs:63`）。

### 8.2 build 命令构造（feature 传递差异）

```rust
// src/toolchain/build.rs:109 —— 构造 build 命令（可测纯函数）
fn build_commands(platform: &str, device: &str) -> Vec<(&'static str, Vec<String>)> {
    let target = platform_target(platform);
    let mut cmds = Vec::new();

    for pkg in ["minui", "minarch", "clock", "minput"] {
        let feature_flag = format!("{platform}/{device}");
        // 每个平台都有确定的交叉 target——恒带 --target
        let args = vec![
            "build".to_string(),
            "-p".to_string(),
            pkg.to_string(),
            "--features".to_string(),
            feature_flag,
            "--target".to_string(),
            target.to_string(),
            "--release".to_string(),
        ];
        cmds.push(("cargo", args));
    }
    cmds
}
```

以 `--platform tg5040 --device brick` 为例，生成 4 条命令（容器内执行）：

```
podman run --rm -v <workspace>:/workspace -w /workspace minui-toolchain:latest \
    cargo build -p minui   --features tg5040/brick --target aarch64-unknown-linux-gnu --release
podman run --rm -v <workspace>:/workspace -w /workspace minui-toolchain:latest \
    cargo build -p minarch --features tg5040/brick --target aarch64-unknown-linux-gnu --release
podman run --rm -v <workspace>:/workspace -w /workspace minui-toolchain:latest \
    cargo build -p clock   --features tg5040/brick --target aarch64-unknown-linux-gnu --release
podman run --rm -v <workspace>:/workspace -w /workspace minui-toolchain:latest \
    cargo build -p minput  --features tg5040/brick --target aarch64-unknown-linux-gnu --release
```

**feature 传递是新手最容易踩的坑**：

- 四个 crate 均用 **`tg5040/<device>`**：minui 等 crate 的 Cargo.toml 里 `tg5040` feature 启用 `dep:platform-tg5040`——`/device` 语法是 Cargo 的 **feature 透传**（把 `brick` 传给依赖 crate platform-tg5040）。平台 lib 作为依赖被连带编译，**无需单独 `-p platform-tg5040`**——show/keymon 已拆为独立 crate（平台自治），不在此编译

**为什么固定 `--release`？** 产物是发布包——编译 debug 无意义。**为什么编译必然经容器？** 真机目标是 ARM Linux——容器本机即目标架构（arm64 容器），cargo build 直接产出 AArch64；宿主机无需 ARM linker/SDL2（见 8.3）。

### 8.3 工具链容器（编译执行载体）

真机编译统一经 `toolchain/Dockerfile` 定义的**工具链容器**执行——宿主机只需
容器引擎，不需要 ARM linker/SDL2/GNU make/交叉 gcc（都在容器内）。

**容器内容**（每层带注释，见 `toolchain/Dockerfile`）：

| 层 | 内容 | 用途 |
|----|------|------|
| 1 | build-essential/make/git/curl/pkg-config | cores 编译与 rustup 安装 |
| 2 | libsdl2-dev | sdl2-sys 经 pkg-config 链接（容器内本机查找，零交叉配置） |
| 3 | gcc-aarch64-linux-gnu/g++-aarch64-linux-gnu | cores 补丁的 `$(CROSS_COMPILE)gcc` 交叉前缀 |
| 3.5 | libzstd-dev | pcsx_rearmed 链接 libzstd 需要（LZMA 压缩支持；C 原版环境自带，容器需显式安装——装 zstd 源码编译的方案被否决） |
| 4 | rustup minimal + aarch64 target | Rust 本机编译（容器本机即 aarch64） |
| 5 | `ENV CROSS_COMPILE=/usr/bin/aarch64-linux-gnu-` | 对齐 C 原版 setup-env.sh 机制 |

**为什么不直接用 cross？** cross 在 arm64 Mac（Apple Silicon）上有未修复 bug
（issue #1716：无视 Cross.toml 自定义镜像、硬走 x86_64 模拟必失败）；且 cross
只代理 cargo——cores 的 make 仍需单独容器。自定义容器一个镜像同时覆盖 Rust
与 cores 两类编译。

**命令构造**（`utils.rs` 的 `container_prefix()` + `container_engine()`）：
引擎经 `MINUI_CONTAINER_ENGINE` 环境变量配置（默认 podman）；前缀 =
`run --rm -v <workspace 根>:/workspace -w /workspace minui-toolchain:latest`，
调用方追加容器内命令（cargo/make）。产物经挂载落回宿主——`target/<triple>/release/`
与 `platforms/tg5040/cores/output/` 路径语义不变，system/装配/package 步骤零改动。

**平台子 xtask 同样经容器**：show/keymon 的 cargo build、cores 的 make 都包
容器前缀（`platforms/tg5040/xtask/src/main.rs`）——cores 的 make
在容器内以 `UNION_PLATFORM=tg5040 make` 执行（PLATFORM 经**环境变量**传递而非命令行参数——避免 MAKEOVERRIDES 泄漏覆盖子 make 内部赋值，详见 cores 文档）；容器环境提供
CROSS_COMPILE 交叉前缀；克隆/编译进度透传宿主可见。

---

## 9. system

**职责**：复制通用二进制进发布包（要求 `--platform`）。对应 C 原版 `make system` 的"复制二进制"部分（makefile:49-56）——makefile.copy 复制平台特有文件的部分已归入 `platform` 步骤（平台子 xtask）。

### 9.1 复制两族二进制（纯复制语义）

```rust
// src/toolchain/system.rs —— 复制清单
/// 进 `SYSTEM/<platform>/bin/` 的通用二进制（系统主程序，对齐 C 原版
/// makefile:52-53 的 minui.elf/minarch.elf）。
const SYSTEM_BINARIES: [&str; 2] = ["minui", "minarch"];

/// 进 `EXTRAS/Tools/<platform>/<pak>/` 的工具二进制（对齐 C 原版
/// makefile:55-56 的 clock.elf → Clock.pak、minput.elf → Input.pak）。
const TOOLS_PAKS: [(&str, &str); 2] = [("clock", "Clock.pak"), ("minput", "Input.pak")];
```

复制落点（对齐 C 原版发布包结构）：

1. **minui/minarch** → `build/SYSTEM/<platform>/bin/`（系统主程序）
2. **clock/minput** → `build/EXTRAS/Tools/<platform>/{Clock,Input}.pak/`——工具进 **Tools pak 而非 SYSTEM/bin**（C 原版 makefile:55-56 同款）：pak 的 `launch.sh` 以 `./clock` 相对引用，EXTRAS 可独立分发（老用户不必重下 base 包）

**为什么 show/keymon 不在此步骤？** 平台自治后 show/keymon 是独立 crate，由平台子 xtask 编译装配（`platform` 步骤）——xtask 的 system 只管通用二进制，"xtask 管平台产物"是职责越界。

### 9.2 不承担完整性检测

system 是**纯复制**步骤：源产物缺失（build 步骤未执行）硬性报错提示先 build；但复制动作结束后还有 `platform` 步骤（show/keymon/cores/install 继续往发布包复制），因此**发布包完整性的唯一验收点是流水线末尾的 `package` 步骤**（`check_release_integrity` 全量校验）——中途不做局部断言，避免「system 已验收」的错觉与清单重复维护（曾经存在过的 `check_required`/`copy_implemented` 局部检测已随此边界删除）。

```rust
// src/toolchain/system.rs —— 复制核心（源缺失硬性报错）
fn copy_binaries(target_dir: &Path, dst_dir: &Path, names: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dst_dir).map_err(|e| format!("创建目录失败（{dst_dir:?}）: {e}"))?;
    for bin in names {
        let src = target_dir.join(bin);
        if !src.is_file() {
            return Err(format!("编译产物缺失: {src:?}（请先执行 build 步骤）"));
        }
        std::fs::copy(&src, dst_dir.join(bin))
            .map_err(|e| format!("复制失败（{src:?} → {dst_dir:?}）: {e}"))?;
    }
    Ok(())
}
```

复制是**增量落盘**：目标目录中 setup 已放好的 skeleton 文件（`setterm`/`shutdown` 等）原样保留（测试 `copy_binaries_preserves_existing_skeleton_files` 锁定）。

### 9.3 产物路径推导

```rust
// src/toolchain/system.rs —— cargo 产物目录：恒为 target/<triple>/release/
fn cargo_target_dir(platform: &str) -> std::path::PathBuf {
    let target = platform_target(platform);
    utils::project_root().join("target").join(target).join("release")
}
```

产物位置由平台映射表的 target 决定——每个平台都有确定的交叉 target，产物恒在 `target/<triple>/release/`（无 host 产物目录）：

```
tg5040 等 aarch64 平台 → target/aarch64-unknown-linux-gnu/release/
rg35xx/trimuismart     → target/armv7-unknown-linux-gnueabihf/release/
```

**为什么 system 与 build 解耦？** 两个步骤可独立执行（先 build 后 system，或只跑 system 复用已有产物）——`all` 串行调用两者，但单步命令各自完整。

---

## 10. platform

**职责**：调用平台子 xtask 完成平台侧全流程（要求 `--platform` 与 `--device`）。由 C 原版 `cores` 步骤改名而来。

### 10.1 职责边界

platform 步骤的职责**限定为一行**：执行 `cargo run --quiet -p <platform>-xtask -- <device>`（命名约定：平台子 xtask 包名为 `<platform>-xtask`，如 `tg5040-xtask`）。平台侧全部工作——show/keymon 编译、libretro.so 编译与复制、install/ 等资源复制——**全部归平台子 xtask 负责**——xtask 不得包含任何平台特有逻辑。

```
C 原版分工（方案 B）                Rust 版分工
─────────────────────────────────────────────────────
顶层 makefile build/system/package  xtask（通用二进制编译/复制/最终打包）
platform/makefile.copy + 平台 make   platforms/<p>/xtask（平台子 xtask，平台自治全流程）
```

**为什么平台自治载体是平台子 xtask？** 平台侧工作包括编译（show/keymon 的 cargo build、cores 的 make）与装配（复制产物/资源）——sh 脚本无法稳健定位 cargo 产物、无类型/测试；平台子 xtask 是 Rust host 工具，与父 xtask 同构（可测、跨平台、命名约定统一）。父 xtask 对平台的唯一触点是 `<platform>-xtask` 包名约定 + device 透传（平台子 xtask 内部见 `platforms/tg5040/xtask/`）。

### 10.2 命令构造（cargo run 调用平台子 xtask）

```rust
// src/toolchain/platform.rs:38 —— 构造 platform 步骤命令（可测纯函数）
fn platform_command(platform: &str, device: &str) -> (String, Vec<String>) {
    let pkg = format!("{platform}-xtask");
    let args = vec![
        "run".to_string(),
        "--quiet".to_string(),
        "-p".to_string(),
        pkg,
        "--".to_string(),
        device.to_string(),
    ];
    ("cargo".to_string(), args)
}
```

**为什么不依赖 `.sh` 脚本？** 平台子 xtask 是 Rust host 工具（父 workspace 成员），经 `cargo run` 调用——无 `.sh` 关联执行器依赖，无 shell 适配分支（项目不做 Windows 开发环境适配）。**本步骤不修改平台子 xtask 内部逻辑**——平台自治逻辑归平台（边界收缩）。

---

## 11. special

**职责**：处理 BOOT 目录重命名与派生（全局步骤，无需 `--platform`）。对应 C 原版 `make special`（makefile:114-129）。

### 11.1 四步逻辑

```rust
// src/toolchain/special.rs:50 —— special 重命名逻辑（可测纯函数，接受 build 目录路径）
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
```

四步职责：

1. **`BOOT/common` → `BOOT/.tmp_update`**：更新检测目录改名（设备启动链读取 `.tmp_update`）
2. **`BOOT/{miyoo,trimui,magicx}` → `BASE/`**：设备家族目录移入发布包基础目录
3. **`.tmp_update` 复制到各设备 app 目录**：每个设备家族的 app 目录内置更新检测（magicx 无 app 层级，复制到根）
4. **`BASE/miyoo` 派生 `miyoo354/355/285`**：Miyoo 家族三兄弟共享同一系统，目录复制派生

### 11.2 缺失跳过语义（与 C 原版的关键差异）

**C 原版假定所有目录存在**（无条件 `mv`/`cp`——skeleton 里什么都有）。Rust 版每个操作前检查源目录存在，**缺失则跳过该操作**（`rename_if_exists`，`src/toolchain/special.rs:104`）：

```
C 原版：mv BOOT/miyoo BASE/miyoo       → 目录不存在时 mv 报错，流水线中断
Rust 版：if BOOT/miyoo 存在才移动       → 缺失跳过，不报错
```

**为什么 Rust 版这样设计？** 当前 Rust skeleton 只有 `BOOT/common` + `BOOT/trimui`（无 miyoo/magicx）——若照抄 C 的无条件移动，流水线会因 miyoo 缺失而中断。存在才处理 = 未来 skeleton 加入 miyoo/magicx 时逻辑**自然覆盖，无需改代码**。

**当前实际执行路径**（Rust skeleton）：common → `.tmp_update`、trimui → `BASE/trimui`、`.tmp_update` 复制到 `BASE/trimui/app/`——其余操作全部跳过。

### 11.3 与 package 的先后关系

C 原版 special 在 cores 之后、package 之前；Rust 版 `all` 顺序 `setup → build → system → platform → special → tidy → package` 保持同一语义——special 在 platform 之后执行（第 14 章推导）。

---

## 12. tidy

**职责**：兼容旧卡——复制新平台 install.sh 到旧平台位置（要求 `--platform`）。对应 C 原版 `make tidy`（makefile:131-141）。

### 12.1 两个平台分支

```rust
// src/toolchain/tidy.rs:50 —— tidy 复制逻辑（可测纯函数，接受 build 目录路径）
fn apply_tidy(build: &Path, platform: &str) -> Result<(), String> {
    match platform {
        // C 原版 makefile:138-141：tg5040 → tg3040 旧卡兼容
        "tg5040" => {
            let src = build
                .join("SYSTEM")
                .join("tg5040")
                .join("bin")
                .join("install.sh");
            let dst_dir = build
                .join("SYSTEM")
                .join("tg3040")
                .join("paks")
                .join("MinUI.pak");
            copy_install(&src, &dst_dir, "launch.sh")
        }
        // C 原版 makefile:134-137：rg35xxplus → rg40xxcube 旧卡兼容
        "rg35xxplus" => {
            let src = build
                .join("SYSTEM")
                .join("rg35xxplus")
                .join("bin")
                .join("install.sh");
            let dst_dir = build.join("SYSTEM").join("rg40xxcube").join("bin");
            copy_install(&src, &dst_dir, "install.sh")
        }
        // 其他平台无 tidy 分支——C 原版同样只处理两个平台
        _ => Ok(()),
    }
}
```

| 平台 | 源 | 目标 | 改名 |
|------|-----|------|------|
| tg5040 | `SYSTEM/tg5040/bin/install.sh` | `SYSTEM/tg3040/paks/MinUI.pak/` | `launch.sh` |
| rg35xxplus | `SYSTEM/rg35xxplus/bin/install.sh` | `SYSTEM/rg40xxcube/bin/` | `install.sh` |

**"旧卡兼容"是什么意思？** 用户旧 SD 卡上的系统是旧平台名（tg3040）——新平台（tg5040）的安装脚本被复制到旧平台位置，使旧卡插入新设备时能正常更新（无需重新分区）。

### 12.2 源缺失报错（依赖顺序的体现）

`copy_install` 对源文件**硬性检查**（`src/toolchain/tidy.rs:93`）：

```
源文件缺失: .../SYSTEM/tg5040/bin/install.sh
（install.sh 由 platform 步骤的平台子 xtask 生成，请确认流水线顺序）
```

install.sh 不是 skeleton 自带的——它由 **platform 步骤**（平台子 xtask 装配 install/）写入 build/。所以 tidy 依赖 platform 先行——`all` 的顺序保证这一点；单独跑 `tidy` 而没跑 `platform` 时，报错信息**提示流水线顺序**而非含糊的"文件不存在"。**无 tidy 分支的平台（如 zero28）视为成功**——与 C 原版一致（C 也只处理两个平台）。

---

## 13. package

**职责**：打包发布——**要求 `--platform` 与 `--device`**（发布名含平台与设备段区分产物），生成 version.txt/commits.txt、发布前完整性校验、组装 PAYLOAD、打包 3 个 zip。对应 C 原版 `make package`（makefile:143-165）。**发布包完整性的唯一验收点在此步骤**（全流程所有复制动作结束后做全量校验）。

### 13.1 九步流程（校验先行）

```
⓪ 发布前完整性校验（check_release_integrity）——缺失即报错列出清单，不产 zip
① 写 build/SYSTEM/version.txt   （MinUI-<platform>-<device>-YYYYMMDD-N
<git短hash>）
② 写 build/SYSTEM/commits.txt   （主仓库 git 信息）
③ 删除 build/ 下所有 .DS_Store
④ 组装 PAYLOAD：SYSTEM → PAYLOAD/.system（重命名）+ BOOT/.tmp_update 复制进来
⑤ zip PAYLOAD → MinUI.zip（.system + .tmp_update）→ 移到 build/BASE/
⑥ cd BASE → releases/<name>-base.zip
⑦ cd EXTRAS → releases/<name>-extras.zip
⑧ 写 build/latest.txt（发布名）
```

### 13.2 发布前完整性校验（第 ⓪ 步）

`check_release_integrity` 在 PAYLOAD 组装前按平台检查关键装配产物（缺失即报错，**绝不静默产出残缺 zip**）：

1. `SYSTEM/<platform>/bin/` 下 **2 个通用二进制**（minui/minarch——对齐 system.rs 的 SYSTEM_BINARIES）
2. 同一 bin/ 下**平台自治装配**（show/keymon/install.sh——平台子 xtask 产物）
3. **Tools pak 工具**：`EXTRAS/Tools/<platform>/{Clock,Input}.pak/` 下的 clock/minput（对齐 system.rs 的 TOOLS_PAKS）
4. **stock 核心**：`SYSTEM/<platform>/cores/` 下 6 个 `*_libretro.so`（fceumm/gambatte/gpsp/picodrive/snes9x2005_plus/pcsx_rearmed——对齐平台子 xtask `copy_stock_cores` 复制表）
5. **BOOT 派生产物**：`BOOT/.tmp_update` 目录存在（special 步骤产出）

两个防漂移守护测试钉住清单一致性：`STOCK_CORES` 必须与 tg5040 平台子 xtask 的 stock 表一致；Tools 清单必须与 system.rs 的 `TOOLS_PAKS` 一致（装配表与校验表同源，改一处漏另一处即编译/测试失败）。

```
$ cargo xtask toolchain package --platform tg5040 --device smart   # 直接跑（未装配）
错误: 发布包完整性检查失败——缺失产物: SYSTEM/tg5040/bin/minui, ...（请先执行 toolchain all 全流程）
```

### 13.3 version.txt 与发布名生成（含日期算法讲解）

```rust
// src/toolchain/package.rs —— 发布名：MinUI-<platform>-<device>-YYYYMMDD-N
fn release_name(releases: &Path, platform: &str, device: &str) -> Result<String, String> {
    let date = utc_date();
    let prefix = format!("MinUI-{platform}-{device}-{date}");
    // N = 当日同 (platform, device) 前缀的 -base.zip 计数（不同设备独立计数）
    let count = /* …过滤 starts_with(prefix-) && ends_with(-base.zip) 计数… */;
    Ok(format!("{prefix}-{count}"))
}
```

- **为什么发布名必须含平台与设备段？** Rust 版每 (platform, device) 独立打包——show 分辨率与安装图按 device 编译期确定、boot.sh 已移除运行时设备检测，产物内容不同（与原版 `MinUI-YYYYMMDD-N` 单包运行时检测通吃多设备不同）
- **`N`** = `releases/` 中**当日同 (platform, device)** `-base.zip` 的计数（首个为 0；smart 与 brick **独立计数**——测试 `release_name_includes_platform_and_device` 锁定）
- **`YYYYMMDD`** = UTC 日期（对齐 C 原版 `TZ=GMT date`——发布名全球统一，不受时区影响）
- **version.txt 内容** = `MinUI-<platform>-<device>-YYYYMMDD-N
<短hash>`——第一行给用户看版本，第二行给开发者溯源

**`utc_date()` 的 civil_from_days 算法**——**为什么手写日期算法而不是引入 chrono crate？** 零依赖原则（第 3 章）：只为"当前日期"引入大型时间库不划算。civil_from_days 是 Howard Hinnant 的经典算法（把"1970 年起的秒数"换算成年月日），约 15 行纯算术、无外部依赖：

```rust
// src/toolchain/package.rs —— 当前 UTC 日期（YYYYMMDD，对齐 C 原版 TZ=GMT date +%Y%m%d）
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
```

### 13.4 commits.txt 与 PAYLOAD 组装

**commits.txt**（对齐 C 原版 commits.sh 的主仓库段落）：一行 `MINUI <短hash> <repo名>`——repo 名来自 `git config --get remote.origin.url`，去掉 github 前缀/`.git` 后缀（无 remote 时回退 `local`）。cores/src 的 libretro 源码仓库信息在可用时附加（`commits_content`）。

**PAYLOAD 组装**是发布包的"内部结构"：

```
build/PAYLOAD/
├── .system/       ← build/SYSTEM 整体重命名（设备系统目录）
└── .tmp_update/   ← build/BOOT/.tmp_update 复制（更新检测目录）

zip -r MinUI.zip .system .tmp_update   → build/BASE/MinUI.zip
```

**base.zip / extras.zip 用系统 `zip` 命令**（`zip_dir`）：`cd` 进目录后 `zip -r ../../releases/<name>-base.zip <清单>`——清单 = C 原版 makefile:163 的目录列表（Bios/Roms/Saves/各平台目录…），**按 build 中实际存在的条目动态过滤**（`existing_entries`）。**为什么用系统 zip 不用 zip crate？** 第 3 章已述——与 C 原版行为完全一致，产物可人工核对。

**git/zip 缺失均硬性报错**（发布环境依赖，C 原版同样依赖）。

---

## 14. all（聚合入口）

**职责**：完整流水线。对应 C 原版 `make PLATFORM=x`（common = build+system+cores）与 `make`（setup/special/package）的组合。

### 14.1 编排顺序推导

```rust
// src/toolchain/all.rs:37 —— 执行完整流水线：setup → build → system → platform → special → tidy → package
pub fn run(platform: &str, device: &str) -> Result<(), String> {
    setup::run()?;
    build::run(platform, device)?;
    system::run(platform)?;
    platform::run(platform, device)?;
    special::run()?;
    tidy::run(platform)?;
    package::run(platform, device)?;
    Ok(())
}
```

顺序为什么是 `setup → build → system → platform → special → tidy → package`？它是对 C 原版两条命令的组合：

```
C 原版:
  make PLATFORM=x = common = build + system + cores（平台相关步骤）
  make            = setup + 所有平台(common) + special + package（全局步骤）

组合推导:
  全局步骤顺序: setup ... special ... package
  平台步骤插入: setup → [build → system → cores(platform)] → special → tidy → package
```

**任一失败即中止**——`?` 操作符传播错误，`all` 返回非零退出码。**本模块是唯一涉及多步骤编排的文件**（模块设计原则，第 5.3 节）。

### 14.2 当前状态：平台自治后可完整跑通

平台自治完成后，system 复制两族通用二进制——show/keymon 由 platform 步骤的平台子 xtask 编译装配，发布包完整性由 package 校验。完整跑通需完整环境：容器引擎 + 工具链镜像（编译统一经容器，宿主机无需 ARM 工具链/SDL2）+ GNU make 与网络（cores 克隆编译）+ git/zip（package）——缺任一环境时报错属环境问题。想要单步调试流水线时，直接跑各步骤命令（`setup` → `build` → `system` → `platform` → `special` → `tidy` → `package`）。

---

## 15. 与原 C makefile 对比

| 维度 | 原版 C makefile | Rust xtask |
|------|----------------|-----------|
| **目标映射** | `setup`/`special`/`tidy`/`build`/`system`/`cores`/`package` | `toolchain setup`/`special`/`tidy`/`build`/`system`/`platform`/`package`（cores 改名 platform，职责改为调平台子 xtask） |
| **聚合入口** | `make all` / `make PLATFORM=x` | `toolchain all --platform <name>`（单平台） |
| **命令执行** | `system("...")` 字符串拼接 | `std::process::Command` 参数数组直传（**免 shell 注入**） |
| **交叉编译** | makefile.toolchain（docker 容器） | 工具链容器（`podman run` + `minui-toolchain` 镜像，宿主免交叉工具链） |
| **架构声明** | 每平台一份 makefile.env（复制粘贴） | 内置映射表 `platform_target()`（一处维护） |
| **多平台构建** | `make all` 一次构建全部平台（PLATFORMS 列表） | 单平台流水线（`--platform` 必填）——多平台支持留待未来变更 |
| **可测试性** | shell 不可单测 | 纯函数可单测（40+ 用例，`cargo test -p xtask`） |
| **二进制命名** | `minui.elf`/`keymon.elf`（skeleton 引用同名） | 无 `.elf` 后缀（skeleton 22 处引用已去除——刻意的命名分叉） |
| **失败语义** | 隐式（命令失败即中断，无上下文） | 显式 `Result<(), String>`——错误信息带程序名/参数/stderr |
| **帮助命令** | `make help` | `cargo xtask help`（项目级详细说明） |
| **新增命令** | 无 | `doc`/`test`/`lint`（Rust 项目质量门禁） |
| **特殊差异** | special 假定目录存在（无条件 mv/cp） | special 缺失跳过（存在才处理） |
| **hash 位置** | `workspace/hash.txt` | `build/hash.txt` |
| **zip 打包** | 系统 zip 命令 | 系统 zip 命令（**保持一致**——C 产物可人工核对） |

---

## 16. 工程实践：测试策略 + 否决方案清单

### 16.1 可测性设计：纯函数抽离清单

xtask 把每个步骤的"命令构造"与"执行"分离——**构造逻辑是纯函数**（输入参数 → 返回命令列表，不执行），可脱离真实环境单测：

| 纯函数 | 位置 | 测试覆盖 |
|--------|------|---------|
| `build_commands` | `src/toolchain/build.rs` | 4 条命令的 feature 传递/target/release、无平台 crate 独立编译 |
| `platform_command` | `src/toolchain/platform.rs` | cargo run -p <p>-xtask/device 透传/命名约定 |
| `apply_special` | `src/toolchain/special.rs` | 仅 common+trimui/完整家族/目录缺失 |
| `apply_tidy` | `src/toolchain/tidy.rs` | tg5040 分支/rg35xxplus 分支/无分支平台 |
| `copy_binaries` | `src/toolchain/system.rs` | 复制成功/产物缺失硬报错/保留既有 skeleton 文件 |
| `lint_commands` | `src/lint.rs` | fmt 先行/clippy 范围参数（双形态） |
| `test_args` | `src/test.rs` | 无参排除全部平台/带参连带该平台（双形态） |
| `aux_scope_args` | `src/utils.rs` | 平台排除集合/features 注入 |
| `platform_crate_names` | `src/utils.rs` | 递归扫描 platforms/ 包名 |
| `release_name` | `src/toolchain/package.rs` | 当日同 (platform, device) base.zip 计数（其他设备/日期/非 base 不计） |
| `utc_date` | `src/toolchain/package.rs` | 8 位数字/全数字 |
| `truncate` | `src/utils.rs` | 超长截断/中文边界 |
| `copy_dir_recursive` | `src/utils.rs` | 树复制/覆盖/源缺失 |
| `list_crate_names` | `src/utils.rs` | 解析 crates.js/空格换行/格式错误 |

运行：`cargo test -p xtask`（纯逻辑层，不依赖 SDL/交叉工具链）。

**为什么不写流水线集成测试？** 完整流水线依赖真实环境（git/zip/交叉编译器/平台子 xtask 的 cores make），测试代价高且环境敏感——`all::run` 的测试只断言"在不完整环境必然返回 Err"（错误传播而非具体步骤）。设计取舍：**纯函数单测覆盖逻辑正确性，命令行为靠 `run_command` 的 stderr 传播 + 人工验收**——这正是"命令构造与执行分离"设计（第 16.1 节）的意义。

### 16.2 否决方案清单（三归档变更汇总）

遇到相似想法时先查此清单（防 AI 后续生成作出错误决策）：

| 方案 | 来源变更 | 否决理由 |
|------|---------|---------|
| 保留 `cargo_metadata` 依赖 | 设计决策记录 | 新架构无消费方——平台清单靠扫描目录，死重依赖 |
| zip 用 Rust `zip` crate | 设计决策记录 | 打包细节需重写 C 行为；系统 zip 与 C 产物一致性可人工核对 |
| 引入 webbrowser/opener crate | 设计决策记录 | 41M/32M 传递依赖仅为打开浏览器；手写 3 行系统命令 |
| target 映射写死单一 aarch64 | 设计决策记录 | 12 平台跨 2 种架构（32 位 armv7 被遗漏） |
| 平台自身声明 target（Cargo.toml metadata） | 设计决策记录 | xtask 需解析各平台 Cargo.toml，增加复杂度 |
| host 编译为主（不做交叉编译） | 设计决策记录 | 真实目标产物优先——开发机验证让位于真机正确性 |
| 12 个子命令全部平铺顶层 | 设计决策记录 | 顶层噪音大，无法体现流水线分组语义 |
| 所有子命令统一必填 `--platform` | 设计决策记录 | 与 C 原版语义不一致，全局步骤强加平台参数是误导 |
| anyhow/thiserror 错误类型 | 设计决策记录 | 引入依赖；`String` + 上下文拼接已满足工具类错误报告 |
| 命令内部 `process::exit(1)` | 设计决策记录 | exit 不可单测；Result 返回到 main 集中处理更可测 |
| panic 宏直接中止的临时实现 | 设计决策记录 | 运行时直接 panic，无法展示命令存在性；验收体验差 |
| 只建文件不建函数签名（空骨架文件） | 设计决策记录 | 架构骨架无法编译验证，模块职责没有代码锚点 |
| 全量不排除平台 crate（test/lint/doc 无参全跑） | 曾被采纳，后被平台自治推翻 | 平台 lib 无默认设备 feature（compile_error 强制）——无参全跑必失败；改为双形态（无参排除全部平台/带参连带该平台） |
| lint 宽容模式（无 `-D warnings`） | 设计决策记录 | 警告被容忍就失去 lint 价值 |
| doc 用 `cargo doc --open` | 设计决策记录 | 打开第一个成员而非聚合首页 |
| workspace 加虚拟根 package | 设计决策记录 | 改变 workspace 结构（触碰 workspace-structure spec） |
| doc 加 `--no-open` 开关 | 设计决策记录 | YAGNI，需要时再加 |
| 平台排除列表硬编码 | 设计决策记录 | 平台增多要手动维护 exclude——改扫描 `platforms/**/Cargo.toml` 自动收集 |

---

## 17. 常见问题 FAQ

### 17.1 `toolchain all` 能跑通吗？

能——平台自治完成后 system 复制两族通用二进制，show/keymon 由 platform 步骤的平台子 xtask 编译装配，发布包完整性由 package 步骤全量校验。完整跑通需完整环境（容器引擎 + 工具链镜像 + git/zip + 网络（cores 克隆上游）），缺任一环境时报错属环境问题（第 14.2 节）。

### 17.2 编译为什么经容器、宿主机不用装 ARM linker？

真机目标是 ARM Linux——**工具链容器**（arm64 Linux，本机架构即目标架构）内
`cargo build` 直接产出 AArch64 产物，宿主机无需 rustup target/ARM linker/
SDL2（第 8.3 节）。容器引擎缺失或镜像未构建时报错（提示构建命令）——**环境
问题，不是 bug**。错误信息经 `run_command` 带 stderr 输出，可诊断。

### 17.3 为什么 clock/minput 不进 SYSTEM/bin 而进 Tools pak？

平台自治后 `SYSTEM_BINARIES` 只有 minui/minarch（系统主程序，对齐 C 原版 makefile:52-53）；clock/minput 进 `EXTRAS/Tools/<platform>/{Clock,Input}.pak/`（对齐 C 原版 makefile:55-56）——pak 的 `launch.sh` 以 `./clock` 相对引用，EXTRAS 可独立分发，老用户更新系统不必重下工具（详见第 9.1 节）。

### 17.4 为什么没有 `.elf` 后缀？

cargo 产物本无 `.elf` 后缀——skeleton 引用必须与产物一致。与 C 原版的命名分叉是刻意的：Rust 版产物（含平台自治 bin show/keymon）天然无后缀兼容（第 4.4 节）。

### 17.5 为什么 doc 命令要先清理 target/doc？

`crates.js` 是 rustdoc 每次运行的**读-合并-写增量产物**——增量构建不重跑 rustdoc 时，文件保持上次内容，会残留依赖 crate 名或旧 crate 的文档目录。清理 + 全量重建后，crates.js 恰好等于本次文档化的 crate 列表，聚合首页不会有死链接（第 2.4 节）。

### 17.6 为什么用系统 zip 不用 zip crate？

与 C 原版行为完全一致（`.DS_Store` 排除、符号链接、zip 内部结构可人工核对）——`zip` 是 C 原版打包链的既有依赖。代价是发布环境无 zip 命令时 package 步骤报错（环境问题，发布环境须自备）（第 3.2 节）。

### 17.7 为什么 `utc_date` 手写日期算法？

零依赖原则——仅为"当前日期"引入 chrono 大型时间库不划算。civil_from_days 是 15 行纯算术算法（Howard Hinnant 经典算法），把"1970 年起的秒数"换算成年月日，无外部依赖、可单测（第 13.2 节）。

### 17.8 辅助命令的平台范围怎么定？

doc/test/lint 是**双形态**（第 2.4 节）：无参数时自动排除 `platforms/` 下全部平台相关 crate（扫描 Cargo.toml，新增平台零维护），只检查通用层——这是"默认查通用层"的显式约定；带 `--platform <p> --device <d>`（成对必填无默认）时连带检查该平台（需该平台工具链，如 SDL2）。平台相关检查必须显式点名才执行，命令不隐含默认值。

---

## 18. 公开 API

### 18.1 命令行 API 面

**顶层命令**（`cargo xtask <命令>`）：

| 命令 | 参数 | 说明 |
|------|------|------|
| `toolchain` | 子命令组（见下） | 构建流水线 |
| `doc` | `--platform`/`--device` 可选（成对） | 生成 rustdoc 文档并自动打开聚合首页（无参排除全部平台） |
| `clean` | `--all` 可选 | 删除 build/（`--all` 连带删除 target/） |
| `test` | `--platform`/`--device` 可选（成对） | 运行 workspace 单元测试（无参排除全部平台） |
| `lint` | `--platform`/`--device` 可选（成对） | cargo fmt --check + cargo clippy 严格检查 |
| `help` | 无 | 详细使用说明 |

**toolchain 子命令**（`cargo xtask toolchain <子命令>`）：

| 子命令 | `--platform` | `--device` | 说明 |
|--------|-------------|-----------|------|
| `all` | 必填 | 必填 | 完整流水线 |
| `setup` | 不要求 | 不要求 | 准备干净的 build 目录 |
| `special` | 不要求 | 不要求 | BOOT 目录重命名 |
| `tidy` | 必填 | 不要求 | 旧卡兼容 |
| `build` | 必填 | 必填 | 交叉编译通用二进制（minui/minarch/clock/minput） |
| `system` | 必填 | 不要求 | 复制两族通用二进制（minui/minarch → bin、clock/minput → Tools pak；纯复制不检测） |
| `platform` | 必填 | 必填 | 调用平台子 xtask（show/keymon/cores/资源全流程） |
| `package` | 必填 | 必填 | 打包发布（发布名含平台/设备段，先完整性校验） |

---

## 设计决策记录

章节 16 的否决清单记录"方案级否决"，本节收拢**流水线形态的演进决策**（xtask 从早期到 build-release-pipeline 定型的变更史）与各步骤职责边界的决策理由。

### 决策：发布包完整性校验收口到 package（唯一验收点）

- **结论**：全流程 14 项关键产物（2 通用 + show/keymon/install.sh + Tools clock/minput + 6 stock cores + .tmp_update）只在流水线末尾 `package` 步骤校验一次（`check_release_integrity`），缺失即中止不产 zip。
- **为什么**：早期 system 步骤自带 `check_required`/`copy_implemented` 局部检测——复制动作结束后还有 platform/special 继续注入产物，"中途验收"产生「system 已验收」的错觉，且装配表与校验表各维护一份易漂移。收口后唯一验收点 + 防漂移测试（STOCK_CORES/TOOLS_PAKS 与装配表一致性断言）。
- **被否决**：校验下沉到每个装配步骤（清单分散、步骤间依赖隐式）；先复制后校验（可能已污染 build/ 再报错）。
- **产生的问题**：package 前必须跑完整流水线（错误信息提示 `toolchain all`）；单步调试 package 需先手工凑齐产物。

### 决策：发布命名含平台与设备段（MinUI-<platform>-<device>-YYYYMMDD-N）

- **结论**：发布名三段式；N 按当日同 (platform, device) 前缀的 -base.zip 独立计数；version.txt 首行同步。
- **为什么**：Rust 版每 (platform, device) 独立打包——show 分辨率/安装图按 device 编译期确定、boot.sh 无运行时设备检测，产物内容不同；原版 `MinUI-YYYYMMDD-N` 单包运行时检测无需区分。
- **被否决**：沿用原版命名（两个 device 产物同名冲突）；`MinUI-<date>-<N>-<device>` 后缀式（解析/排序不便）；device 只进 version.txt 不进文件名（releases/ 目录人眼无法区分）。
- **产生的问题**：与原版命名体系不兼容（升级检测逻辑需按新名）；历史 releases/ 含旧命名文件时计数按前缀过滤不受影响。

### 决策：工具链容器 = 自建单镜像（不用 cross、不装宿主机工具链）

- **结论**：`toolchain/Dockerfile` 自建镜像（build-essential + SDL2 + 交叉 gcc + libzstd-dev + rustup aarch64），宿主机只需容器引擎（默认 podman，`MINUI_CONTAINER_ENGINE` 可切 docker）。
- **为什么**：真机目标是 ARM Linux——容器本机即目标架构，cargo/make 都在容器内执行，宿主机零交叉工具链负担；cores 的 make 需要容器（cross 只代理 cargo 覆盖不了）。
- **被否决**：cross（Apple Silicon 上 issue #1716 无视自定义镜像硬走 x86_64 模拟）；x86_64 + Buildroot 模拟；宿主 zig cc；整个 xtask 容器化（交互繁琐）；长驻容器会话（状态泄漏）。
- **产生的问题**：首次需构建镜像（apt + rustup 下载数分钟）；cores 克隆上游需容器内网络；libzstd-dev 需显式安装（装 zstd 源码编译被否决——apt 包一行解决）。

### 决策：cores 的 PLATFORM 经环境变量传递（UNION_PLATFORM）

- **结论**：cores make 以 `UNION_PLATFORM=tg5040 make` 执行（对齐 C 原版 setup-env.sh 的 `export UNION_PLATFORM`），不走命令行参数。
- **为什么**：命令行变量经 MAKEOVERRIDES 泄漏覆盖子 make 内部赋值（pcsx_rearmed/picodrive 等子 makefile 的 PLATFORM 被污染导致链接错误）。
- **被否决**：`make PLATFORM=tg5040` 命令行传参（早期做法，实测破坏子 make）。
- **产生的问题**：手动容器调试时须记得带环境变量（`cores/README` 的构建方法已统一示范）。

### 决策：system 纯复制不检测（见 §9.2）/ setup-special-package 全局步骤语义保持

- **结论**：见 §9.2 与 §4.1——C 原版"全局步骤不依赖 PLATFORM"的语义对 setup/special 保留；package 因发布命名需求升级为平台步骤（决策二）。
- **演进记录**：xtask 早期曾把 package 归"全局步骤、不要求 --platform"（scaffold 决策 3 的推论），build-release-pipeline 决策 10 后修正为必填——§18 API 表与 §4.1 已同步。
