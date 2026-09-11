# minui — MinUI 启动器

`minui` 是 MinUI 的"主界面"——开机后看到的第一个画面。它负责浏览 SD 卡上的游戏 ROM、显示最近游玩列表、展示版本信息，以及最重要的：选中游戏后启动 minarch 来运行它。

## 模块定位与职责

```
开机 → minui（本 crate）
          │
          ├── 浏览 SD 卡目录
          │   ├── Roms/            ← 按游戏机分类的 ROM 文件
          │   ├── Collections/     ← 用户自建的收藏合集
          │   └── Recently Played/ ← 最近 24 个游玩记录
          │
          ├── 选中 ROM → 写入 /tmp/next
          │                → shell 脚本启动 minarch
          │
          ├── 按 MENU → 版本信息 / 进入睡眠
          │
          └── 从 minarch 返回 → 刷新最近游戏列表
```

**核心职责**：提供极简的文件浏览体验。"极简"意味着——没有设置菜单、没有封面图滚动、没有主题皮肤。只有文字列表，上下选，A 确认。**设计哲学**（沿用 Shaun Inman 的理念）：开机即玩——你不应该在启动器里花时间，你应该在游戏里花时间。

**进程模型**：minui 与 minarch 是两个独立进程，经 `/tmp` 文件协议通信（启动见「launch 模块」，其余三协议 resume_slot/change_disc/auto_resume 见 minarch 文档的协议全家表——minui 是其中 resume_slot/auto_resume 的写方、change_disc 的读方）。

## 内部结构（子组件）

```
crates/minui/src/
├── main.rs      ← 装配层：启动序列、主循环、帧控制（唯一含平台 cfg 的文件）
├── menu.rs      ← 菜单导航状态机：目录栈、滚动窗口、restore、load_last
├── ui.rs        ← 列表/按钮组/缩略图渲染（纯像素，可测）
├── browser.rs   ← 文件浏览器：Directory/Entry、五形态分发、索引（map.txt/unique/alphas）
├── disc.rs      ← 游戏文件组织判断：m3u 多碟 / cue 音轨 / 模拟器检测
├── launch.rs    ← 启动游戏：/tmp/next 写入、resume 槽位、auto_resume
├── recents.rs   ← 最近游戏：24 条上限、持久化 recent.txt
└── version.rs   ← 版本信息页（每帧直接渲染）
```

**依赖分层**（单向无环）：`common ← disc ← recents/launch ← browser ← menu/ui ← main`。`menu` 只依赖 browser/launch/recents；`ui` 只依赖 browser/version 与 render；`launch` 全字符串参数化（不收 `Entry`）；平台特化只出现在 `main.rs`。

## 运行逻辑

### 启动序列（main.rs）

```
1. auto_resume() 检查自动续玩标记 → 存在且 ROM/模拟器可用 → 直接启动退出（不进主循环）
2. simple_mode = .userdata/shared/enable-simple-mode 是否存在
3. SettingsHandle::init()（系统设置：亮度/音量/静音）
4. platform.init_video() → VideoBuffer::new(SCREEN_WIDTH, HEIGHT)
5. 加载图集 assets@{scale}x.png 与字体
6. platform.init_input()；PowerState::new()；无电源键且非 simple_mode 时 disable_sleep()
7. Menu::new(...)（打开根目录 + load_last 恢复上次位置）
8. platform.set_cpu_speed(CpuSpeed::Menu)
```

### 主循环（每帧）

```
poll_input → power::update（Sleep → faux_sleep / PowerOff → 关机链）
→ 在线状态变化置脏 → 输入处理（版本页切换 / scroll / alpha_jump / entry_open / close）
→ 脏帧渲染（ui::render_* + blit_hardware_group + 版本页）
→ flip（帧耗时 < 17ms 才 wait_vsync；不脏帧时限幅到 60fps）
→ HDMI 变化检测（保存位置 + 退出重启语义）
```

**主循环结构**（装配层真实流程）：

```
loop {
    let now = platform.now_ms();
    let input = platform.poll_input();            // ① 输入快照

    // ② 电源状态机（检测者）——Sleep/PowerOff 由本层执行动作序列
    let (action, dirty, show_setting) = power::update(...);
    match action {
        Some(Sleep)    => { before_sleep(); faux_sleep(); after_sleep(); }
        Some(PowerOff) => { before_sleep(); /* 渲染关机消息 */ power_off(); }
        None => {}
    }
    // ③ 在线状态变化 → dirty（状态栏刷新）

    // ④ 输入处理（版本页 or 列表导航）
    if menu.show_version { tapped_menu/B → 退出版本页 }
    else {
        UP/DOWN/LEFT/RIGHT（just_repeated）→ scroll()
        L1/R1 → alpha_jump()；A → entry_open；B && stack>1 → close_directory()
        X && can_resume → should_resume + entry_open
    }
    // ⑤ 脏帧渲染（列表/状态栏/缩略图）→ flip(wait_vsync = 帧耗时 < 17ms)
    // ⑥ 不脏帧 → 帧限幅（FRAME_BUDGET - elapsed）
    // ⑦ HDMI 变化检测 → save_last + 退出重启语义
}
```

**主循环按键行为场景**（对应 spec 验收语义）：
- MENU **短按**（<250ms 释放，`tapped_menu`）→ 版本页；版本页中再短按或 B → 返回列表
- MENU **长按**（≥250ms）→ 不触发版本页（进入睡眠判定路径——`ignore_menu`）
- UP/DOWN 重复触发（just_repeated，300ms 首复 + 100ms 间隔）→ `scroll` 顶部/底部停止；首次按下（just_pressed）→ 允许回绕
- L1/R1 → `alpha_jump`（已在边界组无操作）
- X（`BTN_RESUME`，仅 `can_resume` 时）→ `should_resume=true` + entry_open——从列表直接续玩当前条目
- A → entry_open（Rom 启动 / Pak 启动 / Dir 下钻或 auto_launch）；B 且栈深 >1 → close_directory（保存 restore 后出栈）

帧控制：`FRAME_BUDGET = 17`（60fps，对应 C api.c:204）；帧耗时 < 17ms 才 `flip(wait_vsync=true)`，超时跳过 vsync 防落后。

## 与原 C minui 的对比

| 维度 | 原 C（~1,700 行） | Rust |
|------|-------------------|------|
| 文件浏览 | `opendir`/`readdir` 单层扫描 + 全局链表 | `std::fs::read_dir` 单层扫描 + `Vec<Entry>` |
| 最近游戏 | 固定大小数组 + 手动序列化 | `VecDeque<Entry>` + `put_file`/`get_file` |
| 按键处理 | 全局 `pad` 变量 + `PAD_justPressed` 宏 | `InputState` struct，无全局变量 |
| 界面渲染 | 直接调用 `GFX_*`，参数散落 | `render` crate 统一 API |
| 进程通信 | `system()` + `/tmp/next` | 保持不变（已验证的稳定设计） |

### 逐模块 C 语义对照（汇总）

| Rust 模块 | 对应 C（minui.c） | 语义要点 |
|-----------|------------------|---------|
| `disc.rs` | 检测函数族 :475-516/:837-861 | `hasM3u`→`find_m3u`（bool+输出 → Option 合并）、`hasCue`、`hasEmu`（双候选）、`getFirstDisc` |
| `recents.rs` | Recent 区 :377-418/:441-599 | `hasRecents`→Option 语义改进、CHANGE_DISC 注入 :523-538、多碟去重 :563-582、saveRecents 规范化 :595 |
| `launch.rs` | 启动区 :946-1136/:989-1064/:1212-1221 | `queueNext`/`openRom`/`openPak`/`readyResume`/`autoResume`/`saveLast`——resume 显式参数化 |
| `browser.rs` | 目录区 :105-373/:214-322/:600-942 | 数据结构、`Directory_new` 分发、getRoot/getEntries/getRecents/getCollection/getDiscs、`Directory_index` |
| `menu.rs` | 菜单区 :1138-1296 | openDirectory/closeDirectory/Entry_open/loadLast——状态机化 + 滚动/alpha 纯函数 |
| `ui.rs` | 渲染区 :1486-1675 | render_list（pill 高亮/unique/截断）、按钮组文案 :1581-1670、空目录 :1645-1648 |
| `main.rs` | main :1286-1704 + api.c:204-218 | 启动序列、主循环、帧控制（FRAME_BUDGET=17） |
| `version.rs` | show_version :1523-1586 | parse 语义 + 每帧渲染（不缓存合成面板） |

## 模块详解

### disc — 游戏文件组织判断

提供 4 个"这个游戏文件/目录是什么性质"的判断函数（对应 C `hasM3u`/`hasCue`/`hasEmu`/`getFirstDisc`，minui.c:475-516/:837-861）。**只承载文件组织判断**，不含目录扫描（browser）与碟片条目构造（get_discs）。只依赖 common——是 minui 业务模块的底层：

| 函数 | 语义 | 边界 |
|------|------|------|
| `find_m3u(rom_path)` | ROM 是否多碟游戏的一个碟片：父目录存在 `{父目录}/{父目录名}.m3u` | 扩展名**大小写敏感**（C 字面拼接语义——只有 `.M3U` 不算命中） |
| `find_cue(dir_path)` | 目录是否音轨游戏：同名 `.cue` 存在 | 同上 |
| `has_emu(emu_name, sdcard, platform, paks)` | 模拟器 `.pak` 是否安装：优先 `{sdcard}/Emus/{platform}/{emu}.pak/launch.sh`，回退 `{paks}/Emus/{emu}.pak/launch.sh`，任一存在即 true | 双候选回退（与 `get_emu_path` 同族但语义不同：判断 vs 取路径） |
| `get_first_disc(m3u_path)` | 读 m3u 取第一个**存在**的碟片路径 | 空 m3u / 首行不存在 → None |

**命名约定**（项目先例）：`has_` = 纯 bool 判断；`find_` = 返回 `Option` 的"查找"（C 的 `hasM3u` 虽是 bool，但所有调用方都消费输出路径——`find_` 合并"判断+取路径"）；`get_first_disc` 与 C 名直译一致。

真实 API 与示例：

```rust
use minui::disc::{find_m3u, find_cue, get_first_disc};

// 多碟游戏：Roms/SFC/Final Fantasy VII/ 下有同名 m3u 与 Disc 1.sfc
assert_eq!(
    find_m3u("/SDCARD/Roms/SFC/Final Fantasy VII/Disc 1.sfc"),
    Some("/SDCARD/Roms/SFC/Final Fantasy VII/Final Fantasy VII.m3u".into())
);
// 大小写敏感：只有 .M3U 大写文件 → 不算命中（C 字面拼接语义）
assert_eq!(find_m3u("/SDCARD/Roms/SFC/Disc 1.sfc"), None);

// 第一个存在的碟片（m3u 内容 Disc 1.sfc\nDisc 2.sfc，两文件都在）
assert_eq!(
    get_first_disc("/SDCARD/Roms/SFC/Final Fantasy VII/Final Fantasy VII.m3u"),
    Some("/SDCARD/Roms/SFC/Final Fantasy VII/Disc 1.sfc".into())
);
```

**为什么 m3u 逻辑不与 minarch 共享？** minarch 的 `Game_open` 内联了相同路径构造但语义不同（打开游戏时检测多碟设置 `m3u_path` vs 本模块判断"是否多碟的一个碟片"）——提取共享函数会把两种语义压成一个通用名，宁可冗余也要简单。

### recents — 最近游戏管理

对应 C `Recent` 区（minui.c:377-418/:441-599）。只依赖 common + disc。

```rust
pub struct Recent {
    pub path: String,               // 无 SDCARD 前缀的相对路径（持久化格式）
    pub alias: Option<String>,      // 可选显示别名（recent.txt 的 \t 第二列）
    pub available_when_load: bool,  // 加载时刻模拟器是否仍安装（瞬时快照，不持久化）
}
pub const MAX_RECENTS: usize = 24;  // C 上限（必须是菜单行数的倍数）
```

**`find_recents` 的 Option 语义**（核心设计）：
- `None` = 没有任何**可用**（`available_when_load=true`）的最近游戏 → browser 的 get_root **不建** Recently Played 伪目录
- `Some(vec)` = 有可用条目；vec 含**全部**条目（含不可用——加载保留、显示时过滤，过滤发生在 browser 的 Recent→Entry 转换）
- `available_when_load` 是加载时刻快照（每次 `find_recents` 重新计算；模拟器重装后条目自动恢复显示）

**数据流**：

```
recent.txt（每行 path 或 path\talias）
  │ find_recents(sdcard, platform, paks)
  ▼
① CHANGE_DISC_PATH 存在？→ 注入顶部 + 删除文件
② 逐行解析 → available_when_load = has_emu(当前时刻快照)
③ 多碟去重（find_m3u 命中且父目录已登记 → 跳过）
④ 回写规范化（清理不存在条目）
  ▼
Option<Vec<Recent>>：None = 无可用（browser 不建伪目录）
  │                  Some = 全量（含不可用，显示过滤归 browser）
  ▼
entry_open / 游戏启动 → add_recent（bump + 截断 24 + 立即持久化）
```

**加载副作用**（忠实 C）：
1. `CHANGE_DISC_PATH` 存在 → 读取换碟请求 → 条目注入列表顶部 → **删除该文件**（minarch 换碟后 minui 更新 recents）
2. 多碟去重：条目是多碟游戏的一个碟片（`disc::find_m3u` 命中）→ 父目录已登记则跳过
3. 加载后回写规范化文件（清理已不存在的条目）

**`add_recent`**：path 去 SDCARD 前缀后比较；不存在 → 插入顶部截断到 24；已存在不在顶部 → bump 到顶。持久化格式：每行 `path` 或 `path\talias`，`\n` 结尾。

真实 API 与示例：

```rust
use minui::recents::{find_recents, add_recent, Recent, MAX_RECENTS};

// 无可玩条目 → None（不建 Recently Played 伪目录的关键信号）
let none = find_recents("/SDCARD", "tg5040", "/SDCARD/.system/tg5040/paks");
// Some(vec) 时 vec 含全部条目——available_when_load=false 的保留（显示时过滤）

// add_recent：去重 bump + 截断
let mut recents: Vec<Recent> = vec![];
add_recent(&mut recents, "Roms/SFC/Game.sfc", None, "/SDCARD", "tg5040", "...");
assert_eq!(recents[0].path, "Roms/SFC/Game.sfc");  // 无 SDCARD 前缀
// 再次添加同路径 → bump 到顶不重复；列表满 24 条时最旧被弹出
```

### launch — 游戏启动

对应 C `queueNext`/`openRom`/`openPak`/`readyResume`/`autoResume`/`saveLast`（minui.c:946-1136/:989-1064/:1212-1221）。只依赖 common + disc + recents（`add_recent`），**不收 `Entry`**——全字符串参数（目录判断用 `is_dir: bool`）。

**续玩机制（resume 语义）**：
- `open_rom(path, resume, recent_alias, recents, ...)`：`resume=true` 时读取 slot 文件内容写入 `RESUME_SLOT_PATH`（`/tmp/resume_slot.txt`）；`resume=false` 时写入 `8`（隐藏默认槽位）。多碟（m3u 命中）：add_recent 记录 m3u 路径、启动目标用 `get_first_disc` 定位实际碟片
- `ready_resume(path, is_dir, ...)`：判断条目是否可续玩——ROM 查 slot 文件（`{shared}/.minui/{emu}/{rom_file}.txt`）；目录查同名 `.cue`/`.m3u`；**非 Roms 路径直接 false**（C 的 ROMS_PATH prefix 检查）
- `auto_resume()`：`AUTO_RESUME_PATH` 存在 → 读取 + **删除** → 校验 ROM/模拟器仍存在 → `RESUME_SLOT_PATH` 写 `9`（AUTO_RESUME_SLOT）→ `queue_next` → true（main 据此直接退出进入游戏）
- `save_last(top_is_recents, path, ...)`：保存浏览位置到 `/tmp/last.txt`；Recently Played 顶部时存固定伪目录值（不记具体条目）
- `queue_next(cmd)`：写入 `/tmp/next`（空格分隔引号包裹命令），返回 true = 退出主循环信号

真实 API 与示例（全字符串参数——不收 Entry）：

```rust
use minui::launch::{open_rom, open_pak, ready_resume, auto_resume, save_last, queue_next};

// 启动 ROM（resume=false 时 RESUME_SLOT_PATH 写 8 隐藏默认槽）
open_rom("Roms/SFC/Game.sfc", false, None, &mut recents, "/SDCARD", "tg5040", paks_path);
// → /tmp/next = '{emu}' '{rom}'；recents 增加条目

// 续玩判断：ROM 查 slot 文件；目录查同名 cue/m3u；非 Roms 路径恒 false
assert!(ready_resume("Roms/SFC/Game.sfc", false, "/SDCARD"));
assert!(!ready_resume("/tmp/x.sfc", false, "/SDCARD"));

// 自动续玩（main 启动时调用）：消费标记 → true = 直接退出进游戏
let will_launch = auto_resume("/SDCARD", "tg5040", paks_path);

// 保存浏览位置（menu 关闭目录时）
save_last(false, "Roms/SFC", "/SDCARD");
```

**续玩时序图**（resume 全链路）：

```
minui（本模块）                         /tmp 文件                   minarch
  menu 中 X（can_resume）
    → should_resume = true
    → A → entry_open
        └─ open_rom(path, resume=true)
             ├─ 读 slot 记忆?── no ──> RESUME_SLOT_PATH ← 写 "3"（或非续玩写 8）
             └─ queue_next('emu' 'rom') → /tmp/next
  shell 脚本拉起 minarch ──────────────────────────────────────> 启动读 RESUME_SLOT_PATH
                                                                   （读+删）→ 按槽位读档
  睡眠（minarch before_sleep）──写 auto_resume.txt（ROM 相对路径）──>
  minarch 退出/重启 ──────────────────────────────────────────────>
  minui 再次启动
    → auto_resume() 读 auto_resume.txt（读+删）
        校验 ROM/模拟器仍存在 → RESUME_SLOT_PATH 写 9 → queue_next → true → 直接退出
```

**常量归属**：`LAST_PATH`（/tmp/last.txt）是 minui 独有（全 workspace 仅 `save_last` 与 `menu::load_last` 消费）→ 定义在 `launch.rs` 并提为 `pub`，menu 单向引用；跨进程三常量在 `common::paths`（minarch 也读写）。

### browser — 文件浏览与索引

对应 C minui.c 的目录/条目区（:105-373 数据结构、:214-322 索引、:600-942 构造器）。**本模块是 minui 最大的纯逻辑层**——"路径 → 条目列表"的数据装配，全部 `std::fs::read_dir` 单层扫描（无递归，见「关键技术」）。

**数据结构**（字段对齐 C）：

```rust
pub enum EntryType { Dir, Pak, Rom }
pub struct Entry {
    pub path: String, pub name: String,
    pub unique: Option<String>,   // 重名唯一标识（"Pokemon Red (GB)"）
    pub alpha: usize,             // 所属字母组在 alphas 中的下标（L1/R1 跳字母）
    pub entry_type: EntryType,
}
pub struct Directory {
    pub path: String, pub name: String, pub entries: Vec<Entry>,
    pub selected: usize,          // 光标
    pub start: usize, pub end: usize,  // 滚动窗口页首/页尾
    pub alphas: Vec<usize>,       // 字母组起始条目下标（C IntArray 的 Rust 化）
}
```

**数据装配管线**：

```
路径字符串（SDCARD/Roms/...）
  │ Directory::new(path, selected, sdcard, platform, paks)
  ▼
五形态分发 ── root / recents / collection / m3u / 普通目录
  │
  ▼
形态构造器（get_root / get_recents / get_collection / get_discs / get_entries）
  │  单层 read_dir 扫描（console 目录按 collated 前缀合并多兄弟目录）
  ▼
directory_index（构造后统一执行）
  ├─ map.txt 别名重映射 + hide 过滤（替换后 resort）
  ├─ unique 后缀（重名消歧）
  └─ alphas 字母索引（skip_index 形态跳过）
  ▼
Directory { entries 排序后列表 + selected/start/end + alphas }
  │
  ▼
menu 导航（栈 + 滚动窗口）+ ui 渲染
```

**`Directory::new` 五形态分发**（按序判断）：根目录（path == SDCARD）→ 最近游玩（faux recent 路径）→ 合集（Collections 下 `.txt`）→ 碟片（`.m3u`）→ 普通目录（含 console 合并）。平台原语（sdcard/platform/paks）由调用方从 trait 常量显式传入——browser 零平台依赖。

**根目录构造顺序**（C getRoot 语义，顺序不可打乱）：
1. `find_recents` 有可用 → 插 Recently Played 伪目录
2. 遍历 Roms 单层：hide 过滤 + `has_roms` 通过 → Dir 条目；排序后相邻重名去重
3. `{Roms}/map.txt` 别名重映射并重排序（无 hide 过滤——C "we don't support hidden remaps here"）
4. `has_collections`：有系统条目 → Collections 伪目录；无 → Collections 下目录并入
5. `exists(Tools)` 且非 simple_mode → Tools 条目

真实 API 与示例：

```rust
use minui::browser::{Directory, EntryType};

// 五形态分发（root/recents/collection/m3u/普通）——平台原语显式传入
let dir = Directory::new("/SDCARD", 0, "/SDCARD", "tg5040", paks_path);
// 根目录：entries 依 get_root 顺序（Recently Played 伪目录 / console / Collections / Tools）

// 普通目录：单层扫描；.pak 目录 → Pak，文件 → Rom
let sfc = Directory::new("/SDCARD/Roms/SFC", 0, "/SDCARD", "tg5040", paks_path);
assert_eq!(sfc.entries[0].entry_type, EntryType::Rom);

// m3u 目录：碟片条目 "Disc 1"/"Disc 2"
let discs = Directory::new("/SDCARD/Roms/SFC/FF7/FF7.m3u", 0, "/SDCARD", "tg5040", paks_path);
```

**console 目录合并**：`is_console_dir`（父目录 == Roms）时把 path 截断到**最后一个 `(`**（保留括号）作 collated 前缀，遍历 Roms 下全部子目录按前缀合并——保留 `(` 避免 "Game Boy" 合并掉 "Game Boy Color"（C 语义）。合并结果统一按显示名 ASCII 大小写不敏感排序。

**索引逻辑**（`directory_index`，构造后执行）：
- **map.txt 重映射**：每行 `filename\talias`；命中替换 name；hide 命中过滤；替换后重排序。与 C 的有意偏差：C 在过滤后还会再遍历一遍重新应用 alias（实现细节残留），Rust 一次完成（输出等价）
- **unique 后缀**：相邻 name 相等 → 文件名相等者用 `get_unique_name`（`name (emu_tag)`），否则用对方文件名
- **alphas**：`get_index_char(name)`（首字符 a-z → 1-26，否则 0）变化时记组起始；Recently Played 与合集不建字母索引（C skip_index）

**索引实例**（Directory::new 构造后自动执行，对应 spec 验收场景）：

```
场景 1：map.txt 别名重映射 + 隐藏
  map.txt 内容:  Game.sfc	Final Fantasy      Secret.sfc	.hidden
  → Game.sfc 的 name 变 "Final Fantasy"；Secret.sfc 被过滤（hide 命中）

场景 2：重名 unique 后缀
  同目录两个 "Pokemon"（A.gb / B.gb）→ unique = "A.gb"/"B.gb"（文件名）
  不同目录同名文件（GB 与 GBC 各一）→ unique = "Pokemon (GB)"/"Pokemon (GBC)"（emu 标签）

场景 3：字母索引
  显示名 Zelda/Mario/007 → alphas = [0, 1, 2]（Z→26、M→13、0→0 组）
  → L1/R1 跳字母经 entries[i].alpha 定位 alphas[alpha±1]

场景 4：Recently Played 与合集不建字母索引（skip_index）
```

**判断函数**：`has_roms`（模拟器已装 && 目录有非隐藏条目）/ `has_collections`（目录存在 && 有非隐藏条目）——仅 get_root 内部使用。

### menu — 菜单导航状态机

对应 C 菜单区（openDirectory/closeDirectory/Entry_open/loadLast，minui.c:1138-1296）。只依赖 browser/launch/recents（+common），**不依赖 render/平台**——可测逻辑全部纯函数。

```rust
pub struct Menu {
    stack: Vec<Directory>,   // C top + stack（栈顶即 top）
    rows: usize,             // 每页行数（Platform::main_row_count() 参数化）
    can_resume: bool,        // 当前条目可续玩（X 键 RESUME 门控）
    should_resume: bool,     // C should_resume 的显式化（entry_open 一次性消费）
    show_version: bool,
    // restore 状态（C :432-435 五个全局）：回退/重新进入时恢复位置
    restore_depth: usize, restore_relative: usize,
    restore_selected: usize, restore_start: usize, restore_end: usize,
}
```

真实 API 与示例：

```rust
use minui::menu::{Menu, MenuInput, scroll, alpha_jump, ScrollDir};

// 滚动：顶部 from_press=false 停止 / true 回绕到底（窗口整页对齐）
let (sel, start, end) = scroll(0, 0, 8, 20, 8, ScrollDir::Up, false);
assert_eq!((sel, start, end), (0, 0, 8));          // 顶部停止
let (sel, start, end) = scroll(0, 0, 8, 20, 8, ScrollDir::Up, true);
assert_eq!((sel, start, end), (19, 12, 20));       // 回绕 + 窗口对齐

// alpha 跳转：alphas=[0,4,9]，当前组 1 → 下一组（L1/R1）
assert_eq!(alpha_jump(&[0, 4, 9], 1, 20, 8, true), Some((9, 9, 17)));
assert_eq!(alpha_jump(&[0, 4, 9], 0, 20, 8, false), None);  // 已最前组

// 菜单驱动（装配层逐帧注入按键）
let mut menu = Menu::new(root, rows, sdcard_path, platform, paks_path, simple_mode);
let action = menu.entry_open(0, &mut recents, ...);  // Rom → launch::open_rom 副作用
```

**滚动窗口纯函数**：`scroll(selected, start, end, total, rows, dir, from_press) -> (usize, usize, usize)`——`from_press=false`（重复触发）到顶/底停止；`true`（首次按下）越界回绕并整页对齐窗口；Left/Right 整页移动越界夹到首尾。**alpha 跳转**：`alpha_jump(alphas, current_alpha, total, rows, next) -> Option<...>`——已在边界组返回 `None`（C 的守卫"什么都不做"，`Option` 让调用方不错误重置光标）。

**load_last**：读 `launch::LAST_PATH` → 路径逐级分解匹配（exact/collated 前缀/Collections 文件名后缀三分支）→ 恢复 selected/start/end → 逐级下钻。**auto-launch 目录不显示内容**（C :1269）——上次停在 `.pak`/cue/m3u 目录时只把光标停在该条目，不进入（否则打开即启动）。

**entry_open 分发**：Rom → `launch::open_rom`（`recent_alias` = 条目显示名；`should_resume` 一次性消费）；Pak → `open_pak`；Dir → `open_directory`（`auto_launch` 时目录含同名 cue/m3u 直接启动不入栈）。Collections 下启动时 `save_last` 用合集路径。

### ui — 列表渲染

对应 C 渲染区（minui.c:1486-1675）。只依赖 browser/version/render/common::video。全部函数直接操作 `VideoBuffer`（可脱离 SDL 测试）。

```rust
pub fn render_list(screen, atlas, font, entries, selected, start, end, thumb: Option<u32>, ow, padding, scale);
pub fn render_buttons(screen, atlas, font, version_page, can_resume, stack_depth, simple_mode, total, scale);
pub fn render_thumbnail(screen, entry, scale) -> Option<u32>;  // {父目录}/.res/{文件名}.png
pub fn render_empty(screen, font, scale);                      // "Empty folder"
```

**渲染顺序语义**（与 C 一致）：`trim_sorting_meta` 去排序前缀 → `truncate_text` 按可用宽度截断 → 选中行白 pill + 黑字；**重名条目先画深灰 unique 再画白 name 覆盖其前半**——渲染顺序使 name 后露出的 unique 后缀呈现深灰（C :1614-1631 的技巧）。

**按钮组文案**（render_buttons 内部决策，对应 C :1581-1670）：版本页 `POWER`/`SLEEP`；列表页 `can_resume` 时 `X`/`RESUME` 否则 `POWER`/`SLEEP|INFO`（`BTN_SLEEP==BTN_POWER` 且 simple_mode 决定）；底部 `total>0` 时 `stack>1` → `B`/`BACK`+`A`/`OPEN`、`stack==1` → 仅 `A`/`OPEN`；`total==0` 时 `stack>1` → 仅 `B`/`BACK`。

**列表渲染管线**（每行）：

```
render_list 对 [start, end) 窗口逐行:
  name = trim_sorting_meta(entry.name)          // 去 "01 - " 前缀
  available = 可用宽度（缩略图存在时扣列宽 ow）
  text = truncate_text(name, available)         // 超长截断
  ├─ 选中行：白 pill 背景 + 黑字
  └─ 普通行：深灰字；重名条目 unique 后缀深灰渲染
             （先画深灰 unique、再画白 name 覆盖前半——顺序技巧）
  └─ 缩略图列（存在时）：父目录 .res/{文件名}.png
```

### version — 版本信息页

对应 C `show_version`（minui.c:1523-1586）。零内部依赖。`parse_version`：release = 第一行、commit = 第二行（无第二行返回空串——C 在无第二行时崩溃，Rust 安全化改进）。**渲染策略**：进入版本页时每帧直接渲染、不缓存合成面板——C 缓存 SDL surface 是懒构建成本问题，Rust 的 fontdue 渲染 4 行文本开销可忽略（设计决策，简单为主）。

### main — 装配层

见「运行逻辑」。装配层的 cfg 分支集中于文件顶部平台特化区：语义键经 trait 关联常量（`P::BTN_SLEEP`/`BTN_MOD_*`）→ 构造 `ModKeys`、`sleep_btn` 参数；`BTN_RESUME = BTN_X` 是 minui 本地常量（minui 单消费者功能键，不进 trait）；settings 经 `SettingsHandle`。业务模块（menu/ui）零平台引用（值全参数传入）。

## 关键技术

### 为什么用 std::fs::read_dir 而不是 walkdir？

浏览目录是**单层遍历**——只扫当前层，子目录作为 `Dir` 条目由用户逐层进入（对应 C `openDirectory`）。**这段历史本身是个教训**：早期版本误引入 `walkdir` 递归遍历（错误设计决策），发现后从规范、依赖、文档三处消除——walkdir 的递归语义会把深层目录错误平铺为 ROM 列表，破坏逐层导航与 console 合并。为单层遍历引入依赖纯属浪费，`std::fs::read_dir` + `DirEntry::file_type()` 就是 `readdir` + `d_type` 的直接对应。

### /tmp/next 协议

minui 与 minarch 不通过函数调用通信——独立进程。`/tmp/next` 内容为空格分隔的引号包裹命令（`'emu_path' 'rom_path'`），minui 写入、shell 脚本读取后传给 minarch。协议从 C 原版沿用，5 年 20+ 台掌机验证：简单、可靠、可调试（`cat /tmp/next` 即看）。

### 与 minarch 的 /tmp 协议全家（minui 视角）

| 文件 | minui 角色 | 语义 |
|------|-----------|------|
| `/tmp/next` | **写**（launch::queue_next） | 启动命令 `'emu' 'rom'`——shell 脚本拉起 minarch |
| `/tmp/resume_slot.txt` | **写**（open_rom：续玩写槽位、非续玩写 8、auto_resume 写 9） | minarch 读+删，按槽位自动读档 |
| `/tmp/change_disc.txt` | **读+删**（recents::find_recents：换碟请求注入列表顶部） | minarch 换碟后写——minui 更新最近游玩 |
| `.userdata/shared/.minui/auto_resume.txt` | **读+删**（launch::auto_resume：校验后直接续玩启动） | minarch 睡眠/HDMI 重启前写 |

### 排序规则

ROM 按文件名排序，但先经 `common::utils::trim_sorting_meta()` 去前缀数字（`"01 - Pokemon Red.gb"` → `"Pokemon Red.gb"`）——用户可用数字控制排序，显示不见编号。排序比较用 `to_ascii_lowercase`（对齐 C `strcasecmp`——不用 Unicode 折叠，避免引入 C 没有的行为）。

## 设计决策记录

minui 无早期决策沉淀（README 一直偏薄），以下为按主题收敛的完整决策记录。

### 决策：模块拆分为 8 文件（vs 全塞 main）

- **结论**：main（装配）+ menu（导航状态机）+ ui（渲染）+ browser/disc/launch/recents/version（数据与逻辑）八文件；依赖单向 `common ← disc ← recents/launch ← browser ← menu/ui ← main`。
- **为什么**：全塞 main 约 600 行、渲染/导航/循环混杂、不可测；menu/ui 独立后可测逻辑（滚动窗口、渲染）全部纯函数化。
- **被否决**：全塞 main.rs；只拆 ui.rs（菜单逻辑仍不可测）。
- **产生的问题**：模块间边界需纪律维持——launch 不收 Entry、menu 不碰 render，靠代码评审与文档约束。

### 决策：Menu 结构体收拢 C 全局

- **结论**：C 的 `top`+`stack`/`can_resume`/`should_resume`/`show_version`/restore 五全局收为 `Menu` 字段；`should_resume` 一次性消费（entry_open 后复位）。
- **为什么**：Rust 无跨模块全局可变状态；restore 状态显式归属菜单实例使回退逻辑可测。
- **被否决**：保留全局 static（不可测、跨测试污染）。
- **产生的问题**：调用方须持有 `Menu` 并逐帧驱动（main 装配职责）。

### 决策：recents 现查现存，不持有全局

- **结论**：`find_recents` 每次全量加载（文件是唯一真相）；entry_open 触发启动时临时加载 → `add_recent` 内部持久化 → 丢弃。
- **为什么**：`available_when_load` 本就是加载时刻快照；C 的会话内存全局语义等价（C 的 addRecent 同样立即持久化）。
- **被否决**：Menu 持有 `Vec<Recent>` 跨会话维护——与文件形成双源真相，且 change_disc 注入/去重在 find_recents 内处理，外部维护会重复实现。
- **产生的问题**：每次需要 recents 都读盘（24 条文本文件，开销可忽略）。

### 决策：find_recents 返回 Option + available_when_load 瞬时快照

- **结论**：`None` = 无可用条目（不建伪目录）；`Some` 含全部（不可用保留）；过滤发生在 browser 的 Recent→Entry 转换。
- **为什么**：C `hasRecents` 的"加载 + 返回 has>0"合一的改进——纯函数 + Option 表达消灭全局数组副作用；快照语义使模拟器重装后条目自动恢复。
- **被否决**：返回 `Vec` + 调用方自行判断空（丢失"有但全不可用"与"完全没有"的区别）。
- **产生的问题**：browser 转换层须实现过滤（契约写入 spec 防跨模块越界）。

### 决策：launch 全字符串化 + resume 显式参数化

- **结论**：launch 函数不收 `Entry`/`EntryType`；`open_rom(path, resume: bool, ...)`——resume 显式参数（C `should_resume` 全局的显式化）。
- **为什么**：零 browser 依赖保持依赖单向；C 的 DIR 条目残留 should_resume（打开目录后下一个 openRom 继承续玩意图）是隐式状态 bug 源，显式参数让调用方（menu）自己管理"本次是否续玩"。
- **被否决**：launch 收 Entry（依赖反转）；resume 隐式全局（继承 C 的坑）。
- **产生的问题**：menu 需在 open_directory 时维护续玩意图（restore/resume_pending 逻辑归 menu）。

### 决策：/tmp 常量归属（跨进程入 common，minui 独有留本地）

- **结论**：`RESUME_SLOT_PATH`/`CHANGE_DISC_PATH`/`AUTO_RESUME_SLOT` 入 `common::paths` 常量区块（minarch 实证读写——grep 证据）；`LAST_PATH` 留 `launch.rs`（全 workspace 仅 minui 两个消费方）。
- **为什么**："谁使用谁定义"+ 跨进程共享才进 common；minarch 将来直接 `use common::paths`。
- **被否决**：/tmp 常量全做派生函数（无派生逻辑的函数是伪抽象）；单建 consts 模块（3 个常量不配新模块）。
- **产生的问题**：`LAST_PATH` 提为 `pub` 供 menu 侧 `use crate::launch::LAST_PATH`（单向引用不重复定义）。

### 决策：walkdir 移除（错误设计决策修正史）

- **结论**：浏览器单层遍历用 `std::fs::read_dir`；`walkdir` 从规范/依赖/文档三处消除。
- **为什么**：C 原版 `addEntries`/`getRoot`/`getEntries` 全是 `opendir`/`readdir` 单层扫描；walkdir 递归会把深层目录错误平铺，是重写时引入的错误设计决策。
- **被否决**：保留 walkdir 加 `max_depth(1)`（为单层遍历引入依赖纯属浪费）；保留 walkdir 递归实现（行为与 C 不一致）。
- **产生的问题**：无（纯修正，测试锁定单层语义）。

### 决策：滚动窗口/alpha 跳转纯函数化

- **结论**：C :1367-1427 的 UP/DOWN/LEFT/RIGHT 边界逻辑提取为 `scroll` 纯函数（`from_press` 参数区分首次/重复）；L1/R1 为 `alpha_jump`（返回 `Option`）。
- **为什么**：边界规则（顶部停止/回绕/整页移动/夹取）逐条可测；`from_press` 还原 C `justPressed` vs `justRepeated` 的行为差异。
- **被否决**：滚动逻辑内联在主循环（不可测）；alpha_jump 收 entries+selected（依赖字母组信息即可，更纯）。
- **产生的问题**：main 需把 `from_press` 语义（just_pressed vs just_repeated）映射正确——误用会得到 C 不同的边界行为。

### 决策：帧控制自实现（FRAME_BUDGET = 17）

- **结论**：帧耗时 < 17ms 才 `flip(wait_vsync=true)`；不脏帧时限幅补帧。
- **为什么**：对应 C api.c:204-218 的 GFX_startFrame/flip/sync——vsync 语义：耗时 < 17ms 才等 vsync，超时跳过防落后（与平台 PRESENTVSYNC 的既有固定 vsync 配合）。
- **被否决**：每帧都 wait_vsync（帧超时后 vsync 等待叠加导致帧率塌陷）。
- **产生的问题**：帧计时须用 `now_ms()` 平台时钟（FFI 封装）。

### 决策：render_text 负字形 bug 伴随修复

- **结论**：ui 测试触发 render 层真实 bug——fontdue 的 `ymin` 对 descender（y/g/p）为负，`as u32` 巨大数下溢；render_text 目标坐标改有符号运算 + clamp 0（详见 render 文档）。
- **为什么**：真机渲染任何含 y/g/p 的 ROM 名都会 panic——不能回避。
- **被否决**：测试数据回避 descender；ui 层 clamp（治标不治本，bug 属 render 层）。
- **产生的问题**：无（修复 + 回归测试）。

## 测试

全部模块内嵌 `#[cfg(test)] mod tests`（bin-only crate 无集成测试——minarch 才有 lib+bin 拆分）。模式：纯函数/结构体方法直接单测；涉及文件系统与 `/tmp` 的用临时目录模拟 SD 卡结构（`temp_root` + `setup_sdcard` + `install_emu` 辅助）。特别关注：大小写敏感文件系统语义的测试以 `#[cfg(target_os = "linux")]` 门控（macOS APFS/NTFS 大小写不敏感，无法构造"仅大写扩展名"对照）。

### 测试基础设施

| 辅助 | 用途 |
|------|------|
| `temp_root(name)` | 独立临时 SD 卡根目录（测试结束 `remove_dir_all`） |
| `setup_sdcard(...)` | 建目录结构（Roms/{GB,SFC}/…、Collections、Tools） |
| `install_emu(...)` | 安装模拟器（构造 `.pak/launch.sh`，供 `has_emu`/`has_roms` 判定） |
| `#[cfg(target_os = "linux")]` | 大小写敏感文件系统语义测试门控（disc 扩展名场景） |

覆盖矩阵（各模块测试要点）：

| 模块 | 单测覆盖 |
|------|---------|
| `disc` | 四函数命中/未命中/边界（m3u 大小写、双候选、空 m3u） |
| `recents` | find_recents Option 语义、CHANGE_DISC 注入与删除、多碟去重、add_recent bump/截断/持久化格式 |
| `launch` | queue_next 写入、resume 槽位写入、ready_resume 三分支、auto_resume 消费删标记、save_last 特判 |
| `browser` | 五形态分发、get_root 顺序（伪目录/has_roms 过滤/map 重映射/Collections 提升/Tools 简洁模式）、console 合并 `(` 截断、unique/alphas、get_discs 命名 |
| `menu` | scroll 全部边界（停/回绕/整页/夹取）、alpha_jump Option 语义、load_last 恢复、entry_open 分发 |
| `ui` | 渲染函数存在性与纯像素输出（无 SDL 依赖） |
| `version` | parse_version 多行/单行/空内容 |

```sh
cargo test -p minui    # 全部（无需平台 feature——纯逻辑层）
```

### recent.txt 文件示例

```
Roms/Game Boy (GB)/Pokemon Red.gb
Roms/SFC/Final Fantasy VII/Final Fantasy VII.m3u	最终幻想 7    ← 	 别名列
Roms/PS/Metal Gear Solid.cue
```

- 路径一律**无 SDCARD 前缀**（平台无关——换平台卡通用）
- 多碟游戏记录 **m3u 路径**（启动时经 get_first_disc 定位实际碟片）
- 别名经 `	` 第二列（map.txt 之外的显示覆盖——browser 的 Recent→Entry 转换消费）

## 边界与已知限制

- **recents 文件格式兼容**：`recent.txt` 与 C 原版格式一致（`path\talias`），用户从原版 MinUI 升级时最近游戏保留
- **slot 8 语义**：`RESUME_SLOT_PATH` 写 `8` 表示"隐藏默认槽"（非 0-7 手动槽、非 9 自动槽）——minarch 读到缺失/8 时按 9 处理（自动档）
- **`Entry::alpha` 初始 0**：索引构建后覆盖；Recently Played 等 skip_index 形态的条目 alpha 保持 0（不参与字母跳转）
- **UI 无边界裁剪**：渲染函数不做逐像素边界检查——布局逻辑保证合法坐标（render 层既有约定）
- **真机验证项**：音量/亮度调节经 keymon 通道（minui 无设置 UI）；PLUS/MINUS 的 SDL 可达性疑点影响 minui 的按键映射兜底（见 platform-tg5040 文档 8.6）

---

> 本文档由 minui crate 各模块的源码注释与需求契约整理而成——模块头注释保持职责级自足（零外部文档指针），本页承担全部行为契约与决策的详细叙述。
