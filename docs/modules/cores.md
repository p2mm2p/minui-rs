# cores — libretro 核心构建系统（通用层）

`cores/` 是 MinUI 的 **libretro 核心构建系统**——负责把上游开源模拟器核心（如 gpsp、mgba）从源码编译成 tg5040 设备可用的 `_libretro.so` 动态库。构建机制保持原版 **makefile + git patch** 流程（config.yaml：`libretro_*.so` 编译及补丁 patch 保持原状采用 cmake/make 体系），不重写。

## 为什么分层（边界原则）

核心构建资产按**归属**分两层：

```
┌─ cores/（本目录，通用层）──────────────────────────────┐
│  makefile        构建机制：clone → patch → build 流程    │
│  patches/        核心级共享补丁（所有平台通用）           │
├─ platforms/<p>/cores/（平台层）────────────────────────┤
│  makefile        CORES 清单：编译哪些核心、仓库参数       │
│  patches/        平台补丁（平台特有修补）                │
└────────────────────────────────────────────────────────┘
```

**归属原则**：机制与跨平台资产归通用层，平台声明与平台补丁归平台。平台"编译哪些核心、用什么补丁"是平台知识——不得提取到通用层（提取会导致"平台自己负责自己"的规范被破坏）。

**与原版结构的对比**：C 时代每个平台自包含（`workspace/tg5040/cores/` 里机制、清单、补丁混在一处，机制被复制多份）；新结构把机制抽到通用层共享一份，平台只声明差异——改动机制一处生效，平台迁移只写清单与补丁。

## 构建机制（clone → patch → build）

`cores/makefile` 的核心是一个 `TEMPLATE` 宏——对 CORES 清单里的每个核心展开同一套流程：

```
src/<core>/                    ← ① git clone 核心源码（指定 REPO / HASH 版本）
src/<core>/.patched            ← ② 打平台补丁（标记 .patched）
src/<core>/.patched-all        ← ③ 打共享补丁（标记 .patched-all）
output/<core>_libretro.so      ← ④ make 编译产物
```

四个目标供开发者使用（`make <core>` / `make clone-<core>` / `make patch-<core>` / `make clean-<core>`），顶层 `make` 编译全部（CORES 清单由平台声明提供）、`make clean` 清理、`make nuke` 连源码一起删。

## 为什么要打补丁

两类补丁，动机完全不同：

| 补丁类型 | 位置 | 目的 |
|---------|------|------|
| **平台补丁** | `platforms/<p>/cores/patches/<core>.patch` | **让核心认识这台设备**——libretro 核心的官方 Makefile 不认识 `platform=tg5040`，补丁加上该平台的编译分支（CC/CXX/AR、TARGET 等）。没有它核心根本编译不出来 |
| **共享补丁** | `cores/patches/<core>/*.patch` | **修核心的行为问题**——与平台无关，任何平台编译这个核心都需要 |

**共享补丁情况**（当前 2 个核心）：

| 核心 | 补丁 | 修什么 |
|------|------|--------|
| gambatte | `001-export-dmg-grid-color-on-change.patch` | 调色板颜色变化时导出 DMG 网格颜色到 `/tmp/dmg_grid_color`——供 MinUI 显示系统读取（原版核心不导出） |
| pokemini | `0001-fix-resume-audio.patch` | 音频恢复延迟修复（`defer_frames` 缓冲）——修核心的 resume 行为问题 |

## 补丁流程（两个标记）

同一个核心可能同时有平台补丁和共享补丁——用两个标记文件区分应用状态：

```
src/<core>/.patched      平台补丁已应用（patches/<core>.patch，CWD = 平台 cores 目录）
src/<core>/.patched-all  共享补丁已应用（../../../cores/patches/<core>/*.patch，通配全部）

补丁应用顺序：平台补丁 → 共享补丁（make 依赖链保证）
标记文件存在即跳过（幂等——重复 make 不会重复打补丁）
```

## 产物与去向

编译产物是 `output/<core>_libretro.so`（在平台 cores 目录下）。产物按核心类型复制到设备不同位置：

| 核心类型 | 去向 | 设备位置 | 说明 |
|---------|------|---------|------|
| **stock 核心**（6 个：fceumm/gambatte/gpsp/picodrive/snes9x2005_plus/pcsx_rearmed） | `SYSTEM/<p>/cores/` | `.system/<p>/cores/` | 随 MinUI 系统自带，模拟器 pak 的 launch.sh 从这里加载 |
| **extras 核心**（7 个：fake08/mgba/mednafen_pce_fast/pokemini/race/mednafen_supafaust/mednafen_vb） | `EXTRAS/Emus/<p>/*.pak/` | 用户从 extras.zip 按需复制 | 每个核心对应一个 .pak（有的一个核服务多个 pak，见平台文档） |

复制动作由各平台的打包器（`platforms/<p>/cores/` 的配套脚本或未来 Rust 化的 `package` bin）执行——**通用层只管编译，不管落位**（"落位到哪"是平台知识）。

## 版本钉住（HASH）

平台 makefile **为每个核心定义 `*_HASH`**——钉住补丁（平台 + 共享）对应的
**上游版本**（克隆后 `git checkout $HASH`；模板机制：定义了 HASH 的核心做
全量克隆，未定义则 `--depth 1` 浅克隆拉最新——全钉后不再有未定义核心）。

**为什么钉版本**：补丁（平台/共享）针对特定上游代码编写，上游演进后补丁
可能无法干净应用（`git apply` 失败）或编译不过（API 漂移）。钉住补丁对应
版本保证**编译可重现、补丁可应用**。**全部核心一律钉住**——不因"补丁对
当前上游仍可应用"而省略：当前可应用不代表未来可应用，上游一次 Makefile
结构调整就会让补丁失效。本项目重写重现 C 原版行为——补丁是 C 原版行为的
一部分，不做"上游新版是否已修复"的判断（补丁语义审查属于后续项目更新
范畴）。

**锚定方法**：`*_HASH` 取补丁基线文件（补丁 diff 的 src blob）在各自上游
历史中**最后一次作为文件内容存在的 commit**——即补丁作者打补丁时针对的
上游代码状态。用 `git log --find-object=<补丁 src blob> -- <文件>` 反查。

**当前钉住的版本**（tg5040 平台，13 个全部，见 `platforms/tg5040/cores/makefile`）：

| 核心 | 锚点 commit | 基线文件 |
|------|------------|---------|
| fceumm | `5218d39`（2023-05-27） | Makefile.libretro |
| gambatte | `90701d6`（2023-05-27） | Makefile.libretro |
| gpsp | `f7a6a43`（2025-09-07） | Makefile |
| mgba | `c9ff0b5f5`（2023-05-27） | Makefile.libretro |
| pcsx_rearmed | `70ffca24`（2023-07-07） | Makefile.libretro |
| picodrive | `02ff0254`（2023-05-26） | Makefile.libretro（基线 3a770905 编不过——mcd.c 缺全局 Pico_mcd，02ff0254 修复） |
| pokemini | `182e8b7`（2023-05-27） | Makefile.libretro |
| race | `5c4a96d`（2024-10-18） | Makefile |
| snes9x2005_plus | `edeb733`（2023-03-03） | Makefile |
| mednafen_pce_fast | `ced21d6`（2024-06-28） | Makefile |
| mednafen_vb | `c52a2a5`（2023-05-27） | Makefile |
| mednafen_supafaust | `fc01024`（2023-06-18） | Makefile |
| fake-08 | `611a8cf`（2025-12-30） | libretro.cpp + Makefile（`UpdateAndDraw` 仍启用的最后主分支 commit） |

**如何更新**：新版本补丁验证能干净应用（`git apply --check`）且编译通过后，
把 `*_HASH` 更新为新版本 commit。

**PLATFORM 传递（对齐 C 原版机制）**：cores make 的 PLATFORM 经
**`UNION_PLATFORM` 环境变量**传入（xtask 执行
`UNION_PLATFORM=tg5040 make`，对齐 C 原版 `support/setup-env.sh` 的
`export UNION_PLATFORM=tg5040`），makefile 顶部 `PLATFORM=$(UNION_PLATFORM)`
继承。**不用命令行 `make PLATFORM=tg5040`**——命令行变量会经
MAKEOVERRIDES 泄漏给子 make，覆盖 `Makefile.libretro` 内部
`PLATFORM = libretro`，导致链接缺 libretro 前端对象（undefined reference
到 `lprintf`/`rf*`）。环境变量不进 MAKEOVERRIDES，子 make 内部赋值正常
生效——pcsx_rearmed/picodrive 无需任何 FLAGS 覆盖。

## 构建方法

```sh
# 在平台 cores 目录下运行（make 的 CWD 约定 = 平台 cores 目录）
# 注意：真实构建经工具链容器执行（见下「环境要求」）——
# 以下命令展示的是容器内等价操作
cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make      # 编译全部 13 个核心
cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make gpsp  # 只编译一个
cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make clean
```

**CWD 约定**：`make` 必须在平台 cores 目录运行——makefile 内的相对路径（include 通用机制、平台补丁、共享补丁）都按此解析。不要从其他目录调用。

**环境要求（重要）**：本构建系统使用 `$(eval)` + `$(call)` 模板机制，
**需要 GNU Make ≥ 4.x**，且构建在**工具链容器**内执行（对齐 C 原版在
docker 工具链容器中构建 cores 的做法）：

```sh
# 经工具链容器编译（xtask 自动构造 podman run；无需在宿主装 GNU make）
cargo xtask toolchain platform --platform tg5040 --device smart

# 手动容器构建（等价于平台子 xtask 的 cores 步骤）
podman run --rm -v <minui-rs 根>:/workspace -w /workspace \
  minui-toolchain:latest \
  bash -c "cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make"
```

容器环境设 `CROSS_COMPILE=/usr/bin/aarch64-linux-gnu-`（apt 装的
gcc-aarch64-linux-gnu，对齐 C 原版 setup-env.sh 机制）——平台补丁的
`$(CROSS_COMPILE)gcc` 解析到真实交叉编译器。宿主机无需 GNU make/交叉 gcc。

## 新增一个核心的步骤

1. 平台 `cores/makefile` 的 `CORES` 清单加核心名
2. 需要时在平台 `cores/patches/` 放 `<core>.patch`（加 tg5040 编译分支）
3. 核心行为有问题 → 在通用层 `cores/patches/<core>/` 放共享补丁（所有平台受益）
4. `make <core>` 验证产物

## 验证方式

无 Rust 代码——config.yaml 的 cargo 检查（`cargo test --doc`/clippy）不适用。本系统的验证：

- **路径解析**：`make -n PLATFORM=<p>` 干跑（不实际编译）——include 与补丁通配无路径错误
- **补丁可应用性**：对克隆的源码 `git apply --check <patch>` 验证补丁能干净应用

---

# 平台声明层：tg5040 cores


本目录是 tg5040 平台的 **libretro 核心构建声明**（通用层机制见 `cores/README.md`）：声明编译哪些核心、各核心的仓库参数、平台特有的补丁。构建时 `make` 在这里运行，`include` 通用层机制。

```
platforms/tg5040/cores/
├── makefile       CORES 清单（13 个核心）+ 各核心仓库/版本参数
└── patches/       13 个平台补丁（<core>.patch）
```

## 为什么要打补丁（平台视角）

libretro 核心（如 gpsp、mgba）是**上游开源项目**——它们的官方 Makefile 只认识自家测试过的平台（`platform=linux`/`miyoo`/`rg350` 等），**不认识 tg5040**。没有补丁时 `make platform=tg5040` 会报"未知平台"。

**平台补丁的作用 = 给核心的 Makefile 加上 tg5040 编译分支**：

```makefile
# gpsp.patch 的内容（摘录）——在官方 Makefile 的 platform 分支表里插入：
+# TRIMUI SMART PRO
+else ifeq ($(platform), tg5040)
+	TARGET := $(TARGET_NAME)_libretro.so
+	CC = $(CROSS_COMPILE)gcc
+	CXX = $(CROSS_COMPILE)g++
+	AR = $(CROSS_COMPILE)ar
+	SHARED := -shared -Wl,--version-script=link.T
```

补丁是"唯一接入方式"——除非上游主动支持 tg5040（目前没有），否则每个核心都需要这份补丁才能编译出设备可用的 `.so`。

## 平台补丁情况（13 个）

| 补丁 | 修补对象 | 内容主题 |
|------|---------|---------|
| `gpsp.patch` | gpsp 的 `Makefile` | 加 tg5040 平台分支 |
| `race.patch` | race 的 `Makefile` | 加 tg5040 平台分支 |
| `mednafen_pce_fast.patch` | beetle-pce-fast 的 `Makefile` | 加 tg5040 平台分支 |
| `mednafen_vb.patch` | beetle-vb 的 `Makefile` | 加 tg5040 平台分支 |
| `mednafen_supafaust.patch` | supafaust 的 `Makefile` | 加 tg5040 平台分支 |
| `snes9x2005_plus.patch` | snes9x2005 的 `Makefile` | 加 tg5040 平台分支 |
| `fceumm.patch` | libretro-fceumm 的 `Makefile.libretro` | 加 tg5040 平台分支 |
| `gambatte.patch` | gambatte 的 `Makefile.libretro` | 加 tg5040 平台分支 |
| `mgba.patch` | mGBA 的 `Makefile.libretro` | 加 tg5040 平台分支 |
| `pcsx_rearmed.patch` | pcsx_rearmed 的 `Makefile.libretro` | 加 tg5040 平台分支 |
| `picodrive.patch` | picodrive 的 `Makefile.libretro` | 加 tg5040 平台分支 |
| `pokemini.patch` | PokeMini 的 `Makefile.libretro` | 加 tg5040 平台分支 |
| `fake-08.patch` | fake-08 的 `platform/libretro/Makefile` **+ `libretro.cpp`** | 加 tg5040 平台分支 + 源码行为修补（唯一的双文件补丁） |

**与共享补丁的关系**：同名核心（gambatte/pokemini）在通用层 `cores/patches/` 还有共享补丁（修核心行为）——两者都打：先平台补丁（`makefile` 认识 tg5040）再共享补丁（行为修正），由 `.patched`/`.patched-all` 两个标记分别管理（详见 `cores/README.md`「补丁流程」）。

## 与原版补丁做法的对比

| 维度 | C 原版（workspace/tg5040/cores/） | 现在（platforms/tg5040/cores/） |
|------|----------------------------------|--------------------------------|
| 补丁位置 | 与构建机制、共享补丁混在同一自包含目录 | `patches/` 独立子目录，机制在通用层（`cores/`） |
| 平台接入 | 机制被复制进每个平台（workspace/<p>/cores/ 各一份 makefile） | 机制共享一份（`include ../../../cores/makefile`）——改动机制一处生效 |
| 补丁应用 | 平台补丁与共享补丁路径混在一起（`../../all/cores/patches/`） | 路径分离清晰：平台 `patches/` + 共享 `../../../cores/patches/`，`.patched`/`.patched-all` 两标记分别管理 |
| 新增核心 | 在自包含 makefile 加 CORES + 补丁 | 同样加 CORES + 补丁，但机制不需要碰 |

补丁**内容**做法不变（git apply 同一流程）——变化的是组织方式。

## CORES 清单（编译哪些核心）

`makefile` 的 `CORES` 变量声明 13 个核心（6 个 stock + 7 个 extras），每个核心可带仓库/版本/构建参数：

```makefile
CORES = fceumm gambatte gpsp pcsx_rearmed picodrive snes9x2005_plus
CORES+= mednafen_pce_fast mednafen_vb fake-08 mednafen_supafaust mgba pokemini race
```

| 核心 | 仓库 | 特殊参数 |
|------|------|---------|
| fceumm | `libretro/libretro-fceumm` | — |
| gambatte | `libretro/gambatte-libretro` | — |
| gpsp | 默认（libretro 组织） | — |
| pcsx_rearmed | 默认 | `MAKEFILE = Makefile.libretro` |
| picodrive | `irixxxx/picodrive` | `MAKEFILE = Makefile.libretro` |
| snes9x2005_plus | `libretro/snes9x2005` | `FLAGS = USE_BLARGG_APU=1` |
| mednafen_pce_fast | `libretro/beetle-pce-fast-libretro` | — |
| mednafen_vb | `libretro/beetle-vb-libretro` | — |
| fake-08 | `jtothebell/fake-08` | `CORE = fake08_libretro.so`、`BUILD_PATH = fake-08/platform/libretro` |
| mednafen_supafaust | `libretro/supafaust` | — |
| mgba | 默认 | — |
| pokemini | `libretro/PokeMini` | `MAKEFILE = Makefile.libretro` |
| race | 默认 | — |

（完整参数见 `makefile` 的 `*_REPO`/`*_HASH`/`*_CORE`/`*_MAKEFILE`/`*_BUILD_PATH`/`*_FLAGS` 变量；`*_HASH` 可把核心钉在指定提交——如 gpsp 在某个提交后存档不兼容，需要钉版本。）

## 产物与去向

编译产物：`output/<core>_libretro.so`（本目录下）。打包时复制到：

| 核心 | 类型 | 复制到 | 设备位置 |
|------|------|--------|---------|
| fceumm / gambatte / gpsp / picodrive / snes9x2005_plus / pcsx_rearmed | stock | `SYSTEM/tg5040/cores/` | `.system/tg5040/cores/` |
| fake-08 | extras | `EXTRAS/Emus/tg5040/P8.pak/` | 用户按需复制 |
| mgba | extras | `EXTRAS/Emus/tg5040/MGBA.pak/` + `SGB.pak/` | 用户按需复制（**一核双 pak**） |
| mednafen_pce_fast | extras | `EXTRAS/Emus/tg5040/PCE.pak/` | 用户按需复制 |
| pokemini | extras | `EXTRAS/Emus/tg5040/PKM.pak/` | 用户按需复制 |
| race | extras | `EXTRAS/Emus/tg5040/NGP.pak/` + `NGPC.pak/` | 用户按需复制（**一核双 pak**） |
| mednafen_supafaust | extras | `EXTRAS/Emus/tg5040/SUPA.pak/` | 用户按需复制 |
| mednafen_vb | extras | `EXTRAS/Emus/tg5040/VB.pak/` | 用户按需复制 |

**一核多 pak**：mgba 服务 GBA 与 SGB（Game Boy 超级增强）两个模拟器 pak；race 服务 NGP 与 NGPC——编译一次，复制到多个 pak 目录（每个 pak 有自己的 launch.sh 和默认配置，但加载同一个 `.so`）。

**stock vs extras 的区别**：stock 核心随系统包（MinUI.zip 的 `.system/`），开箱即用；extras 核心在 extras.zip 里，用户想玩对应机种时把 pak 目录复制到 SD 卡 `EXTRAS/Emus/tg5040/` 即可（skeleton 已预置空 pak 目录结构）。

## 构建方法

```sh
cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make        # 全部 13 个（PLATFORM 经环境变量传递）
cd platforms/tg5040/cores && UNION_PLATFORM=tg5040 make mgba   # 单个
cd platforms/tg5040/cores && make clean PLATFORM=tg5040  # 清理产物
cd platforms/tg5040/cores && make nuke PLATFORM=tg5040   # 连克隆的源码一起删
```

**CWD 约定**：make 必须在本目录运行（相对路径按此解析：`include ../../../cores/makefile` 引用通用机制、`patches/` 为本目录补丁、共享补丁在 `../../../cores/patches/`）。

## 验证方式

无 Rust 代码——config.yaml 的 cargo 检查不适用。验证：

- **路径解析**：`make -n PLATFORM=tg5040` 干跑——include 与补丁通配无路径错误
- **补丁可应用性**：克隆源码后 `git apply --check patches/<core>.patch`
