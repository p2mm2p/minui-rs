# clock — 日期时间设置工具

`clock` 是 MinUI 的"时钟设置"小工具——一个独立的二进制，让你在掌机上调整系统日期和时间。它是发布包完整性要求的 6 个二进制之一（minui/minarch/keymon/clock/minput/show），当设备时间不准时（比如刚换电池、跨时区旅行），通过它校正。

## 模块定位与职责

```
用户在设备上运行 clock（独立二进制）
          │
          ├── 显示当前日期时间（YYYY/MM/DD HH:MM:SS）
          │
          ├── 方向键编辑字段
          │   ├── UP/DOWN    ← 当前字段 ±1（AM/PM 切换 ±12 小时）
          │   ├── LEFT/RIGHT ← 光标循环移动（年→月→日→时→分→秒→[AM/PM]）
          │   └── SELECT     ← 切换 12/24 小时制并持久化
          │
          ├── A 键保存 → 写入系统 RTC（date + hwclock 命令）
          └── B 键取消 → 不保存直接退出
```

**核心职责**：提供一个极简的日期时间编辑器。没有菜单、没有主题、没有多余功能——只有 7 个可编辑的数字字段和几个按键。这符合 MinUI 的"极简"哲学：工具就该一步到位。

**为什么需要独立进程？** 与原版 C 一致，clock 是独立二进制，由用户在需要时调用（如从外部 shell 或设置入口），不常驻内存。它和 minui/minarch 一样通过 `Platform` trait 访问硬件，但本身不参与启动器或游戏流程。

## 内部结构（子组件）

```
crates/clock/src/
├── main.rs      ← 入口：装配层（启动序列、主循环、绘制编排、平台接线）
├── validate.rs  ← 纯逻辑：时间字段编辑与回卷校验（可单测）
└── prefs.rs     ← 纯逻辑：12/24 小时制偏好读写（可单测）
```

**当前状态**：三个模块全部实现并带单元测试（36 个）。`main.rs` 完成装配（启动序列 → 主循环 → 退出序列），依赖 `validate`（字段编辑）/`prefs`（偏好持久化）两个纯逻辑模块。

## 运行逻辑（模块内部流程）

**启动失败语义**：初始化失败（`init_video`/`init_input` panic、图集/字体 `.expect`）即进程非零退出、不进入主循环——平台初始化在掌机上是"要么成功要么重启"（Platform trait 不返回 Result 的既定契约）；日期/时间编辑中的用户操作错误（如非法字段）不 panic，走回卷校验静默拒绝。

### 主循环（简化）

```
1. main() 创建平台实例: let mut platform = Tg5040::new();
2. run(&mut platform) 装配:
   platform.set_cpu_speed(Menu)    ← CPU 降频省电
   platform.init_video()           ← SDL 视频初始化
   platform.init_input()           ← 输入初始化
   load_atlas / load_font          ← 资源加载（图集 + 字体）
   read_show_24hour()              ← 读取 12/24 小时偏好（标记文件）
   localtime(now)                  ← 读取当前系统时间初始化字段
3. 主循环:
   while !quit:
     let input = platform.poll_input()       ← 读取按键
     if UP/DOWN:    adjust(field, ±1)        ← 字段增减（纯函数）
     if LEFT/RIGHT: move_cursor(±1)          ← 光标循环（纯函数）
     if SELECT:     toggle 12/24h + 持久化    ← 偏好切换
     if A:          save_changes = true; quit
     if B:          quit
     if dirty:                                ← 脏帧渲染
       validate(...)                          ← 回卷校验（纯函数）
       blit_hardware_group(...)               ← 顶部状态栏
       render_text(...)                       ← 居中日期时间
       blit_pill(Underline)                   ← 光标下划线
       blit_button_group(...)                 ← 底部按键提示
       platform.flip(&screen, wait_vsync)
     else:                                    ← 不脏帧时限幅（60fps）
       sleep(FRAME_BUDGET - elapsed)
4. 退出:
   platform.quit_input() / quit_video()
   if save_changes: platform.set_date_time(...)  ← 写入 RTC
```

**关键设计**：字段编辑（`validate::adjust`）、回卷校验（`validate::validate`）、光标循环（`validate::move_cursor`）、12 小时显示转换（`validate::display_hour`）、偏好读写（`prefs::read_show_24hour`/`write_show_24hour`）**全部是纯函数**——不依赖平台、不碰 SDL，可在桌面上直接 `cargo test` 单测。装配层（main.rs）只做"收集数据 + 调用纯函数 + 驱动渲染"。

### `<P: Platform>` 泛型参数

clock 与 minui/minarch 一样，函数签名使用 `<P: Platform>`。这个 `P` 是 Rust 的**泛型参数**——编译时的"占位符"，最终被替换为具体平台类型（如 `Tg5040`）。

**通俗理解**：clock 的代码只对着"插座标准"写——我需要读按键（`poll_input`）、需要显示画面（`flip`）、需要写时间（`set_date_time`）。至于插在墙上的具体是什么设备，编译时才决定。所以 clock 是**通用工具**：代码本身不绑定任何平台，但编译时必须通过 feature 透传实例化一个具体平台（与 minui 相同，见下文「平台编译选择」）。

### 回卷校验（validate）

这是 clock 最"聪明"的部分。当你把日期从 1 月 31 日按 UP 加到 32 日，它不会显示 2 月 32 日——而是回卷成 2 月 1 日：

```
validate((2024, 1, 32, 0, 0, 0))  →  (2024, 2, 1, 0, 0, 0)
  ↑ 月 1 → 2（32 日超过 1 月的 31 天，减去 31 得 1）
```

它处理所有边界：
- **闰年**：2024 年 2 月有 29 天（`is_leap_year` 判断 4 整除、100 除外、400 再入）
- **月回卷**：12 月按 UP → 1 月；1 月按 DOWN → 12 月
- **日按当月天数回卷**：4/6/9/11 月 30 天，2 月 28/29 天，其余 31 天
- **时/分/秒回卷**：23 时按 UP → 0 时；0 分按 DOWN → 59 分
- **年钳制**：1970..2100（超出钳到边界）

这些边界在原版 C 里是内联在 main 里的 `validate()` 函数（`clock.c:98-138`），Rust 版提升为模块级纯函数——每个边界都有单元测试覆盖。

### 12/24 小时制

SELECT 键切换显示制式，并用一个**标记文件**持久化偏好：

```
.userdata/<platform>/show_24hour   ← 文件存在 = 24 小时制，缺失 = 12 小时制
```

文件存在性表达偏好是原版 C 的设计（`clock.c:57,230-234` 的 `exists()`/`touch`/`rm`），Rust 版完全兼容——用户从原版 MinUI 升级时，这个文件已存在，clock 启动即识别为 24 小时制，偏好无缝保留。

12 小时制下有两个特殊行为：
- **显示转换**：内部小时 0 → 显示 12，13 → 1，23 → 11（`display_hour` 纯函数）
- **AM/PM 字段**：编辑它使小时 ±12（9 AM 变 21 PM），光标多一个 AMPM 字段

## 与原 C clock 的对比

| 维度 | 原 C（~300 行） | Rust |
|------|-----------------|------|
| 回卷校验 | `validate()` 内联在 main，全局变量 | `validate::validate` 纯函数，单元测试覆盖全部边界 |
| 字段编辑 | main 里 switch 直接改全局变量 | `validate::adjust`/`move_cursor` 纯函数 |
| 数字渲染 | 预渲染 digits surface（SDL 时代性能技巧） | 逐字符 `render_text`（dirty 重绘下开销可忽略） |
| 偏好持久化 | `system("touch")`/`system("rm")` | `prefs::write_show_24hour` 用 `std::fs` 直接读写 |
| 本地时间 | `localtime()`（libc） | `libc::localtime_r`（同语义，FFI 封装） |
| 写 RTC | `system("date -s ...; hwclock --utc -w")` | 平台层 `set_date_time`（`std::process::Command` 免 shell 转义，无注入风险） |

## 关键技术拓展

### 为什么逐字符渲染而不是预渲染数字条？

原版 C 在启动时把 12 个字符（`0-9 / :`）预先渲染到一张 surface（`clock.c:31-51`），之后 `blitNumber` 按固定宽度 `i*SCALE1(10)` 裁剪——这是 SDL 时代的性能技巧，避免每帧重绘文字。

Rust 版直接逐字符调用 `render::text::render_text`。为什么可以？因为 clock 是**只在 dirty 时重绘**（有按键才重绘，否则 `GFX_sync` 空转），fontdue 单字符栅格化在这种低频场景下开销完全可忽略。预渲染反而引入"字符条宽度固定 10px"的硬编码布局，不如逐字符按实际 advance_width 排布灵活。这也符合"render 零新增"的决策——数字条不是需要新抽象的原语，只是"文字渲染 + 居中算术"的组合。

### 为什么偏好放 clock 自己而不是 common？

`show_24hour` 标记文件在**整个 workspace 只有 clock 使用**（原版 minui.c/api.c 都不读它）——不是跨模块共享状态。按"common 是跨模块共享家"的定位，放进 common 是过度设计。所以偏好读写放在 clock 自己的 `prefs.rs`，路径参数化可单测。

### 为什么 clock 依赖 libc？

Rust 标准库**没有本地时间函数**（只有 UTC 的 `SystemTime`）。原版 clock 用 `localtime()` 读系统本地时间（含时区），Rust 版用 `libc::localtime_r` 达成同样效果——这与平台 crate（tg5040）的 libc 用法一致，clock 作为通用工具直接依赖 libc 是合理的（它本来就是 libc 目标平台的工具）。

## 公开 API 说明

clock 是一个二进制 crate（`main.rs`），不对外暴露库 API。其模块结构仅供内部组织代码使用。

主要入口：

```rust
// main.rs
fn main() {
    #[cfg(feature = "platform-tg5040")]
    {
        let mut platform = tg5040::Tg5040::new();
        run(&mut platform);          // 实际装配（启动序列 → 主循环 → 退出）
    }
}
```

纯逻辑模块（`validate.rs`/`prefs.rs`）的函数是 `pub`，但仅在同一 crate 内（`mod validate`）可见——它们不构成对外 API，而是为了单元测试可达。

### 平台编译选择

与 minui 相同，clock 通过 Cargo feature flag 选择目标平台（设备 feature 必选——`tg5040` 后必须跟 `/smart` 或 `/brick`，见 workspace-structure spec「平台 feature 系统规范」）：

```sh
cargo build -p clock --features platform-tg5040/smart --release
```

每个 feature 对应一个 `platform-*` crate。编译时只有一个平台实现被链接进来。clock 的 `Cargo.toml` 声明 optional 平台依赖 + `platform-tg5040` feature（`["dep:platform-tg5040"]`），与 `crates/minui/Cargo.toml` 完全同模式。

## 关键代码解析

### 光标定位（cursor_geometry）

光标下划线（`Asset::Underline` 药丸）的位置是 clock 最微妙的布局逻辑，对应原版 `clock.c:297-304`：

```
年字段:  x = ox（内容区起点），宽 40
其他:    x = ox + 50 + (cursor-1) * 30，宽 20
AM/PM:   宽度 = "AM"/"PM" 文字宽度 + 2
```

`ox` 是居中后的内容区起点，`50` 是 "YYYY/" 前缀的占位宽度，`30` 是每个字段的步进（两个数字 + 间距）。这些值都是原版 C 的 `SCALE1()` 缩放常量，Rust 版对应 `YEAR_CURSOR_W`/`FIELD_CURSOR_W`/`DATE_PREFIX_W`/`FIELD_STEP` 常量——**布局常量不重复定义**，复用 `common::video` 的 `PILL_SIZE`/`BUTTON_SIZE` 等。

### 本地时间读取（localtime）

```rust
fn localtime(secs: i64) -> LocalTime {
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let t = secs as libc::time_t;
        libc::localtime_r(&t, &mut tm);
        // tm_year + 1900, tm_mon + 1, ...
    }
}
```

`unsafe` 是因为 FFI 调用 libc——`localtime_r` 是线程安全的（不像 `localtime` 用静态缓冲区），且返回的 `tm` 结构体由调用方持有，无悬垂风险。这与平台 crate 中 `shm_open`/`ioctl` 等 FFI 的 `unsafe` 用法一致。

