CORES = fceumm gambatte gpsp pcsx_rearmed picodrive snes9x2005_plus
CORES+= mednafen_pce_fast mednafen_vb fake-08 mednafen_supafaust mgba pokemini race

###############################

# optional core vars
# *_REPO=
# *_HASH=
# *_CORE=
# *_FLAGS=
# *_MAKEFILE=
# *_BUILD_PATH=

# 版本钉住：13 个 core 全部钉住补丁对应的上游版本（机制见
# ../../../cores/makefile 模板：克隆后 git checkout $HASH）。
#
# 锚定方法：平台/共享补丁的基线文件（补丁 diff 的 src blob）在各自上游
# 历史中最后一次作为文件内容存在的 commit——即补丁作者打补丁时针对的
# 上游代码状态（git log --find-object=<blob> 反查）。所有补丁都钉住，
# 不因"补丁对当前上游仍可应用"而省略——上游演进后补丁可能失效，
# 钉住保证编译可重现、补丁始终可应用。
#
# 日期为锚点 commit 的提交日期；除 fceumm/fake-08 外均为本次
# 从"能应用的最近版本"纠正回"补丁原本版本"（原钉为编译通过优先）。
fceumm_HASH = 5218d39            # 2023-05-27（补丁基线 Makefile.libretro）
gambatte_HASH = 90701d6          # 2023-05-27（补丁基线 Makefile.libretro）
gpsp_HASH = f7a6a43              # 2025-09-07（补丁基线 Makefile）
mgba_HASH = c9ff0b5f5            # 2023-05-27（补丁基线 Makefile.libretro）
pcsx_rearmed_HASH = 70ffca24     # 2023-07-07（补丁基线 Makefile.libretro）
# picodrive：补丁基线 Makefile.libretro 最后存在于 3a770905(2023-05-02)，
# 但该 commit 的 mcd.c 尚无全局 Pico_mcd（libretro 链接 undefined reference，
# 上游 02ff0254 修复）；钉在 02ff0254——补丁仍可应用（Makefile.libretro
# 自 2023-01 后未变）且代码可编译的最近 commit。
picodrive_HASH = 02ff0254        # 2023-05-26（Pico_mcd 链接修复）
pokemini_HASH = 182e8b7          # 2023-05-27（补丁基线 Makefile.libretro）
race_HASH = 5c4a96d              # 2024-10-18（补丁基线 Makefile）
snes9x2005_plus_HASH = edeb733   # 2023-03-03（补丁基线 Makefile）
mednafen_pce_fast_HASH = ced21d6 # 2024-06-28（补丁基线 Makefile）
mednafen_vb_HASH = c52a2a5       # 2023-05-27（补丁基线 Makefile）
mednafen_supafaust_HASH = fc01024 # 2023-06-18（补丁基线 Makefile）
# fake-08：平台补丁在 retro_load_game 调用 Vm::UpdateAndDraw（延迟刷新），
# 上游 0be1ae4（game-loop-refactor）后该方法被注释；钉在方法仍启用的
# 主分支最后 commit（补丁基线 libretro.cpp + Makefile）。
fake-08_HASH = 611a8cf           # 2025-12-30（UpdateAndDraw 启用的最后主分支）

mednafen_pce_fast_REPO = https://github.com/libretro/beetle-pce-fast-libretro
mednafen_vb_REPO = https://github.com/libretro/beetle-vb-libretro

fake-08_REPO = https://github.com/jtothebell/fake-08
fake-08_CORE = fake08_libretro.so
fake-08_BUILD_PATH = fake-08/platform/libretro

fceumm_REPO = https://github.com/libretro/libretro-fceumm

gambatte_REPO = https://github.com/libretro/gambatte-libretro

mednafen_supafaust_REPO = https://github.com/libretro/supafaust

# pcsx_rearmed/picodrive：Makefile.libretro 内部 `PLATFORM = libretro`
# 决定是否把 libretro 前端对象（libretro.o 等）加入链接。此处无需
# FLAGS 覆盖——父 make 的 PLATFORM 来自 UNION_PLATFORM 环境变量
# （xtask 调用：`UNION_PLATFORM=tg5040 make`，对齐 C 原版 setup-env.sh），
# 环境变量不进入 MAKEOVERRIDES，不会泄漏给子 make 覆盖内部赋值。
pcsx_rearmed_MAKEFILE = Makefile.libretro

picodrive_REPO = https://github.com/irixxxx/picodrive
picodrive_MAKEFILE = Makefile.libretro

pokemini_REPO = https://github.com/libretro/PokeMini
pokemini_MAKEFILE = Makefile.libretro

snes9x2005_plus_REPO = https://github.com/libretro/snes9x2005
snes9x2005_plus_FLAGS = USE_BLARGG_APU=1

###############################

include ../../../cores/makefile