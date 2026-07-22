## 第一步完成 —— 数据结构和平台trait的定义

```
minui-rs/
├── Cargo.toml          ← 12 个平台 feature flags，编译时选择
└── src/
    ├── lib.rs          ← crate 根，模块声明 + 重导出
    ├── types.rs        ← 核心数据结构 (320 行)
    ├── platform.rs     ← Platform trait + TestPlatform (340 行)
    ├── state.rs        ← MinUi 状态机 (290 行)
    └── paths.rs        ← SD 卡路径常量 (160 行)
```

### 各文件与原 C 代码的映射

| Rust 文件 | 对应原 C | 核心内容 |
|-----------|---------|---------|
| `types.rs` | `minui.c:16-208` + `defines.h:36-206` | `Entry`, `Directory`, `Recent`, `Button`, `PadContext`, `Color` 等所有数据结构 |
| `platform.rs` | `platform.h` + `api.h` 全部 `PLAT_*` 声明 | `Platform` trait：13 组关联常量 + 30+ 个方法，覆盖视频、输入、音频、电源、覆盖层 |
| `state.rs` | `minui.c:422-436`（全部 static 变量） | `MinUi` 结构体：导航栈、最近游戏、恢复状态、UI 标志 |
| `paths.rs` | `defines.h:13-33`（全部 `#define` 路径） | `roms_path()`、`recent_path()`、`slot_path()` 等派生函数 |

### Rust 版相对于 C 版的关键改进

1. **所有权替代手动内存管理** —— 不再有 `malloc`/`free`，`Entry_free()` 直接变成 `drop()`
2. **`Option<T>` 替代 NULL** —— `unique: Option<String>` 而非 `char* unique` 的 NULL 检查
3. **`Vec<T>` 替代手写 Array** —— 不再需要 `Array_new`/`Array_push`/`Array_pop`
4. **trait 替代 `#include` 平台头文件** —— 编译时选择平台，运行时多态
5. **所有全局状态在 `MinUi` 结构体中** —— 可构造、可测试、可序列化
6. **`TestPlatform`** —— 能在电脑上运行和测试，不需要实际硬件

准备好进入第二步了吗？第二步是实现**文件系统扫描** —— `add_entries`、`get_root`、`get_recents`、`get_collections`、`get_discs`，以及字符串工具函数（`hide`、`getDisplayName`、`getEmuName` 等）。

---