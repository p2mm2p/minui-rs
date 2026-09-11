# 快速上手

从零开始：环境准备 → 编译 → 打包发布。完整命令语义与流水线推导见 [xtask 模块文档](modules/xtask.md)。

## 1. 环境依赖

| 依赖 | 用途 | 安装 |
|------|------|------|
| Rust（rustup） | cargo 本身 | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| 容器引擎 | 真机（ARM）编译 | macOS：`brew install podman && podman machine init && podman machine start`（默认 podman，`MINUI_CONTAINER_ENGINE=docker` 可切） |
| git | 版本/溯源（hash.txt/version.txt） | 系统自带 |
| 系统 `zip` | package 打包 | macOS 自带 |

## 2. 工具链容器（一次性准备）

面向真机（ARM Linux）的**全部编译**——`toolchain build`（minui/minarch/clock/minput）、平台子 xtask 的 show/keymon 与 cores（libretro.so）——统一经工具链容器执行（见 `toolchain/Dockerfile`）。宿主机只需容器引擎，无需安装 ARM linker、SDL2 交叉库或 cores 的交叉 gcc。

```sh
podman build -t minui-toolchain:latest toolchain/   # apt + rustup 下载需数分钟
```

## 3. 编译与测试（宿主机直跑）

```sh
# 安装 Rust（通过 rustup）

# 编译（设备 feature 必选——平台 crate 无默认设备，smart/brick 必须显式指定）
# 注意：面向真机（ARM Linux）的编译经工具链容器执行（见上）；以下仅宿主本机架构调试用
cargo build -p minui --features tg5040/smart --release

# 宿主直跑的检查命令（不需容器，通用层代码）
cargo xtask test            # 单元测试（排除平台 crate）
cargo xtask lint            # fmt + clippy（排除平台 crate）
cargo xtask doc             # rustdoc（排除平台 crate）
cargo xtask test --platform tg5040 --device smart   # 连带平台（需宿主能编译平台）
```

**平台 feature 系统**：平台 crate（如 `platform-tg5040`）的设备 feature（`smart`/`brick`）互斥且必选、无默认——未指定设备或同时指定两个时编译期报错（`compile_error!` 断言）。上层 crate 以 `--features <平台代码>/<device>` 透传（详见 [架构页](architecture.md) workspace 契约）。

## 4. 构建与打包（toolchain 流水线）

```sh
# 完整流水线（setup → build → system → platform → special → tidy → package）
cargo xtask toolchain all --platform tg5040 --device smart
# 也可单独执行各步骤：
cargo xtask toolchain setup                        # 清空 build/ + 复制 skeleton
cargo xtask toolchain build --platform tg5040 --device smart   # 编译 4 个通用二进制（容器内）
cargo xtask toolchain system --platform tg5040     # 复制两族二进制（minui/minarch → bin、clock/minput → Tools pak）
cargo xtask toolchain platform --platform tg5040 --device smart   # 平台子 xtask（show/keymon/cores/装配）
cargo xtask toolchain special                      # BOOT 目录重命名
cargo xtask toolchain tidy --platform tg5040       # 旧卡兼容
cargo xtask toolchain package --platform tg5040 --device smart   # 打包发布（含完整性校验）
```

### 目录语义：build/ 是「装配暂存区」，不是编译产物

```
minui-rs/
├── target/    ← cargo 编译产物（纯缓存，可随时 cargo clean 整删）
├── build/     ← 装配暂存区（setup 每次清空并从 skeleton/ 重建，可随时 xtask clean）
│               内容 = 将来发布 zip 的母版：SYSTEM/（各平台系统区）、
│               EXTRAS/（扩展层）、BOOT/（更新器，special 后成 .tmp_update）、BASE/
└── releases/  ← 最终发布 zip（package 产出，唯一需要留存的产物）
```

`build/` 对齐 C 原版的 `./build`——setup 步骤每次 `rm -rf build` 再复制 `skeleton/`，然后各步骤往里填二进制、cores、install 脚本与跨平台家族派生。**它不是"某次编译的缓存"**，而是有状态、跨步骤累积的装配现场；`cargo clean` 不会也不应删除它（清理用 `cargo xtask clean`）。

### 发布包：产物、命名与消费场景

`package` 步骤产出三个 zip（划分对齐 C 原版，边界 = 消费端场景而非目录便利）：

| 包 | 内容 | 给谁 |
|---|---|---|
| `MinUI.zip`（放 build/BASE/ 内） | 仅 `.system` + `.tmp_update` | **设备端升级**——boot.sh 检测到 SD 卡根有它即 unzip 覆盖系统区；绝不能带 Bios/Roms/Saves（用户数据） |
| `releases/MinUI-<platform>-<device>-<YYYYMMDD>-<N>-base.zip` | SD 卡根视图（Bios/Roms/Saves + 家族 app + 内含 MinUI.zip + README.txt） | **新用户整卡初始化**——解压到 FAT32 卡即开机 |
| `releases/MinUI-<platform>-<device>-<YYYYMMDD>-<N>-extras.zip` | EXTRAS 扩展层（Emus/Tools/Bios） | **按需功能扩展**——老用户不必重下 base |

命名 `MinUI-<platform>-<device>-<YYYYMMDD>-<N>`（如 `MinUI-tg5040-smart-20260907-0`）：Rust 版每 (platform, device) 独立打包（show 分辨率与安装图按设备编译期确定、boot.sh 无运行时设备检测），命名须含平台/设备段区分产物；N 按同 (platform, device) 前缀当日 `-base.zip` 计数（不同设备独立计数）。版本信息写入 `build/SYSTEM/version.txt`（发布名 + git 短 hash）与 `commits.txt`；`build/latest.txt` 记录最新发布名。

### 发布前校验与内容卫生

`package` 步骤在打包前做**发布完整性校验**：任一关键产物缺失（minui/minarch、show/keymon/install.sh、Tools pak 的 clock/minput、6 个 stock 核心、BOOT/.tmp_update 派生）即报错列出清单，不会静默产出残缺 zip——因此发布前请先跑完整流水线。发布 zip 的内容卫生由流水线保证：setup 删 `.keep`/`*.meta`、package 删 `.DS_Store`、skeleton 目录名与 C 原版逐字节一致。

## 5. cores 中间产物清理

libretro 核心的克隆源码（`platforms/<p>/cores/src/`，13 个上游仓库）与编译产物（`platforms/<p>/cores/output/`，`*_libretro.so`）是**中间产物**（.gitignore 已忽略），可随时清理、下次构建时重新克隆/编译：

```sh
# makefile 层（在平台 cores 目录内；nuke 无需 PLATFORM/UNION_PLATFORM）
cd platforms/tg5040/cores && make nuke

# xtask 层（推荐——容器内执行，不必进目录）
cargo run -p tg5040-xtask -- smart cores-nuke   # 清理 src/ 与 output/
```

清理只删 src/output，保留 makefile/patches/README 声明文件。

## 6. git 依赖

xtask 的 setup（`build/hash.txt`）与 package（version.txt/commits.txt）依赖 `git rev-parse`，hash 语义指向 **minui-rs 自身仓库**（Rust 重写代码的版本，而非 C 原版 `MinUI/` 仓库）。请确保在 minui-rs git 仓库内执行流水线；仓库外执行会报「获取 git hash 失败」。
