# minarch — MinUI 游戏内前端

`minarch` 是 MinUI 的"游戏内界面"：当你从启动器（minui）选中一个 ROM 后，minarch 被 shell 脚本拉起，负责加载 libretro 模拟核心（`.so`）、运行游戏循环、提供游戏内菜单（即时存档、画面设置、换碟），以及退出后回到 minui。

**核心职责**：作为 libretro 核心和掌机硬件之间的桥梁——动态加载 `.so`、实现 libretro 回调（environment/video/audio/input）、管理存档（savestate 快照 + SRAM 电池存档）、游戏内菜单、HDMI 输出适配。

## 模块定位与职责

- **进程模型**：与 minui 相互独立，经 `/tmp` 文件协议通信（`/tmp/next` 启动 + resume_slot/change_disc/auto_resume 三协议，详见[模块文档](../../docs/modules/minarch.md)）
- **装配架构**：12+ 个纯逻辑模块（libretro/game/environment/savestate/sram/config/audio/vibration/menu/controls/hdmi/core）+ 装配层（`main.rs` 薄胶水 + `assembly.rs` 可测逻辑）——纯逻辑全部可脱离 SDL 测试

## 内部结构（子组件）

```
crates/minarch/src/
├── lib.rs / main.rs / assembly.rs   ← crate 根 + 装配层（bin 入口 + 可测纯逻辑）
├── libretro.rs / core.rs            ← libretro ABI 地基 + 会话与视频管线
├── game.rs / environment.rs         ← 游戏文件组织（zip/m3u）+ 30 case 环境分发
├── savestate.rs / sram.rs           ← 存档快照 + 电池存档持久化
├── config.rs / menu.rs / controls.rs← 配置注册表、游戏内菜单、按键映射
├── audio.rs / vibration.rs / hdmi.rs← 音频引擎、振动状态机、HDMI 检测
```

## 快速了解

- **lib + bin 双目标**：集成测试链接 lib（minui 是 bin-only，这是两 crate 的结构差异）
- **测试体系**：mock 核心（假 `.so`）+ vendored `libretro.h` + 编译期 const 断言，14 个集成测试目标 213 个运行时测试
- **与 C 的差异**：`<P: Platform>` 泛型替代宏平台层、Result 替代静默崩溃、软件缩放替代 GPU 缩放

## 详细文档

完整文档（运行逻辑、C 语义对照、装配契约、设计决策记录、逐模块解析、测试详解）见 [docs/modules/minarch.md](../../docs/modules/minarch.md)。
