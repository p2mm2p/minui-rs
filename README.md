## 第一步完成 —— 数据结构和平台 trait 的定义

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

## 第二步完成 —— 文件系统扫描

### 新增文件

```
src/
├── utils.rs    (360 行) — 字符串工具 + 文件 I/O
└── scan.rs     (950 行) — 目录扫描 + Directory 构造
```

### `utils.rs` 包含的函数

| 函数 | 对应 C | 功能 |
|------|--------|------|
| `prefix_match` | `prefixMatch()` | 大小写不敏感前缀匹配 |
| `suffix_match` | `suffixMatch()` | 大小写不敏感后缀匹配 |
| `exact_match` | `exactMatch()` | 大小写敏感精确匹配 |
| `contains_string` | `containsString()` | 子串搜索 |
| `hide` | `hide()` | 隐藏文件判断（`.` 开头 / `.disabled` / `map.txt`） |
| `get_display_name` | `getDisplayName()` | 提取显示名（去扩展名、括号、空白） |
| `get_display_name_with_platform` | `getDisplayName` + 平台后缀处理 | 带平台标签的显示名 |
| `get_emu_name` | `getEmuName()` | 从 ROM 路径提取模拟器标签 |
| `get_emu_path` | `getEmuPath()` | 构建模拟器启动路径 |
| `normalize_newline` | `normalizeNewline()` | `\r\n` → `\n` |
| `trim_trailing_newlines` | `trimTrailingNewlines()` | 去除行尾换行 |
| `skip_sorting_meta` | `trimSortingMeta()` | 跳过 `001) ` 排序前缀 |
| `path_exists` / `file_exists` / `dir_exists` | `exists()` | 路径存在性检查 |
| `put_file` / `get_file` / `get_file_limited` | `putFile()` / `getFile()` | 文件读写 |
| `put_int` / `get_int` | `putInt()` / `getInt()` | 整数的文件读写 |
| `alloc_file` | `allocFile()` | 读取整个文件 |
| `file_name` / `parent_dir` | — | 路径解析辅助 |
| `is_pak` / `is_m3u` / `is_cue` | — | 文件类型判断 |

### `scan.rs` 包含的函数

| 函数 | 对应 C | 功能 |
|------|--------|------|
| `has_emu` | `hasEmu()` | 检查模拟器是否可用 |
| `find_cue` | `hasCue()` | 查找 PS1 CUE 文件 |
| `find_m3u` | `hasM3u()` | 查找多碟 M3U 文件 |
| `has_roms` | `hasRoms()` | 检查目录下是否有 ROM + 模拟器 |
| `scan_dir` | `addEntries()` | 扫描目录，构建 Entry 列表 |
| `get_entries` | `getEntries()` | 获取条目（含归类逻辑） |
| `get_root` | `getRoot()` | 构建根目录 |
| `get_recents_from_list` | `getRecents()` | 最近游戏 → Entry 列表 |
| `get_collection` | `getCollection()` | 读取收藏列表 |
| `get_discs` | `getDiscs()` | 读取多碟列表 |
| `get_first_disc` | `getFirstDisc()` | 获取第一张碟 |
| `make_directory` | `Directory_new()` + `Directory_index()` | 创建 Directory + 字母索引 |
| `load_recents` | `hasRecents()` | 加载最近游戏文件 |

### 测试覆盖

- **utils**: 25 个测试 — 覆盖所有匹配函数、显示名提取、文件 I/O
- **scan**: 10 个测试 — 覆盖目录扫描、根目录构建、M3U 解析、多碟、名称映射、字母索引
- **总计**: 36 个测试全部通过，零 warning，clippy 干净

### Rust vs C 的关键差异

| 原 C | Rust |
|------|------|
| `char*` + `strcpy`/`strrchr`/`sprintf` | `String` + `Path` + `format!()` |
| `strncasecmp` | `.eq_ignore_ascii_case()` |
| `opendir`/`readdir`/`closedir` | `std::fs::read_dir` + Iterator |
| `FILE*` + `fopen`/`fread`/`fclose` | `std::fs::read_to_string` / `std::fs::write` |
| `malloc`/`free` 手动管理 | 所有权自动管理 |
| 全局 `static` 变量 | 纯函数 + 参数传递 |

---