# cores — libretro 核心构建系统（通用层）

`cores/` 是 MinUI libretro 核心的**通用构建机制层**：clone → patch → build 的共享逻辑与共享补丁。平台特有内容（CORES 清单、平台补丁、stock/extras 划分）归 `platforms/<p>/cores/`（平台声明层）。

## 模块定位与职责

- **两层归属**：机制 + 共享补丁在通用层；清单 + 平台补丁在平台层（平台 makefile `include ../../../cores/makefile` 复用机制）
- **构建机制**：clone 上游（版本钉住 `*_HASH`）→ 打补丁（`.patched`/`.patched-all` 标记）→ make 产出 `*_libretro.so`
- **产物去向**：stock 6 核 → `SYSTEM/<p>/cores/`；extras 7 核 → `EXTRAS/Emus/<p>/*.pak/`
- **PLATFORM 传递**：经 `UNION_PLATFORM=<p> make` 环境变量（命令行传参会经 MAKEOVERRIDES 泄漏破坏子 make）
- **新增核心**：通用层步骤清单见详细文档（makefile 声明 + HASH 钉住 + 补丁 + 测试）

## 详细文档

完整文档（分层设计、补丁流程、版本钉住反查法、新增核心步骤、tg5040 平台 13 核清单与补丁情况）见 [docs/modules/cores.md](../docs/modules/cores.md)。
