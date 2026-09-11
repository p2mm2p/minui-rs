# minui — MinUI 启动器

`minui` 是 MinUI 的"主界面"——开机后看到的第一个画面。它负责浏览 SD 卡上的游戏 ROM、显示最近游玩列表、展示版本信息，以及选中游戏后启动 minarch 来运行它。

## 模块定位与职责

- **核心职责**：极简文件浏览体验——没有设置菜单、没有封面滚动、没有主题皮肤；上下选、A 确认、开机即玩
- **进程模型**：与 minarch 相互独立，经 `/tmp/next` 等文件协议通信（启动 + resume_slot/change_disc/auto_resume）
- **装配架构**：7 个纯逻辑模块（disc/recents/launch/browser/menu/ui/version）+ `main.rs` 装配层——依赖单向 `common ← disc ← recents/launch ← browser ← menu/ui ← main`，平台特化只在 main.rs

## 内部结构（子组件）

```
crates/minui/src/
├── main.rs      ← 装配层：启动序列、主循环、帧控制（唯一含平台 cfg 的文件）
├── menu.rs      ← 菜单导航状态机：目录栈、滚动窗口、restore、load_last
├── ui.rs        ← 列表/按钮组/缩略图渲染（纯像素，可测）
├── browser.rs   ← 文件浏览器：Directory/Entry、五形态分发、索引（map.txt/unique/alphas）
├── disc.rs      ← 游戏文件组织判断（m3u 多碟 / cue 音轨 / 模拟器检测）
├── launch.rs    ← 游戏启动：/tmp/next、resume 槽位、auto_resume
├── recents.rs   ← 最近游戏：24 条上限、持久化 recent.txt
└── version.rs   ← 版本信息页
```

## 快速了解

- **bin-only crate**：全部测试内嵌 `#[cfg(test)]`（与 minarch 的 lib+bin 拆分不同——minui 无 FFI 测试需求）
- **续玩体系**：resume_slot / auto_resume / /tmp/last.txt 三件套——睡死/重启后回到游戏
- **单层浏览**：`std::fs::read_dir` 单层扫描（曾有 walkdir 递归的错误设计被修正）

## 详细文档

完整文档（逐模块行为契约、C 语义对照、索引/续玩/协议细节、设计决策记录、测试矩阵）见 [docs/modules/minui.md](../../docs/modules/minui.md)。
