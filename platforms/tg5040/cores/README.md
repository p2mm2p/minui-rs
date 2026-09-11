# tg5040 cores — 平台核心声明与补丁

`platforms/tg5040/cores/` 是 tg5040 平台的 libretro 核心**声明层**（机制复用通用层 `cores/`）：CORES 清单（13 个：6 stock + 7 extras）、平台补丁与产物去向。

## 模块定位与职责

- **为什么打补丁**：上游核心的 Makefile 不认 tg5040/UNION——平台补丁插入 platform 分支（共享补丁在通用层）
- **stock vs extras**：stock 6 核随系统（`SYSTEM/tg5040/cores/`）；extras 7 核按需分发（`EXTRAS/Emus/tg5040/*.pak/`）
- **构建**：`UNION_PLATFORM=tg5040 make`（容器内执行；PLATFORM 走环境变量）
- **清理**：`make nuke` 只删 src/ 与 output/（保留声明文件）

## 详细文档

完整文档（13 核清单、补丁情况表、产物映射、构建方法、验证方式）见 [docs/modules/cores.md](../../../docs/modules/cores.md)。
