# 架构

MinUI Rust 重写的整体架构：进程模型、模块职责与依赖方向、关键技术解析、workspace 契约与跨模块设计决策。

## 1. 项目运行逻辑

```
开机
  │
  ▼
minui（启动器）
  ├── 浏览 SD 卡：Roms/  Collections/  Recently Played/
  ├── 选中 ROM → 写入 /tmp/next → shell 脚本 → 启动 minarch
  └── 按 MENU → 进入睡眠 / 显示版本信息
        │
        ▼
minarch（游戏内前端）
  ├── 装配层（main.rs + assembly.rs）启动序列：
  │    参数解析 → 平台初始化 → Core::open（dlopen）→ Game::open（zip/m3u）
  │    → 三级 cfg 加载（system/default/user）→ 回调桥注册 → 核心加载 → 音频初始化
  ├── 主循环（支持线程模式：`minarch_thread_video` 开启时 `core.run()`
  │    移入独立核心线程，方案 A 优雅退出；单线程时同步执行）
  │    （快进限速 → pending flip → 音频 drain → 电源状态机 → HDMI 检测）
  ├── 按 MENU → 游戏内菜单（存档、设置、换碟；进入时写 SRAM/RTC、降频）
  ├── 睡眠/唤醒（存档 + auto_resume 标记 + vibration 挂起/恢复）
  └── 退出 → 写档 → 返回 minui（通过 shell 脚本重新拉起）
```

**minui 和 minarch 是两个独立进程**，通过 shell 脚本和 `/tmp` 文件通信（`/tmp/next` 启动 + resume_slot/change_disc/auto_resume 三协议）。这部分沿用原版设计，Rust 重写不做改变。

## 2. 模块职责与设计目的

```
minui-rs/
├── crates/
│   ├── common/         Platform trait + 基础类型 + 文件工具（零依赖）
│   ├── render/         所有渲染函数（文字、药丸按钮、电池图标、缩放器）
│   ├── minui/          启动器（bin）
│   ├── minarch/        游戏内前端（lib + bin：lib 供集成测试链接，bin 为薄入口）
│   ├── clock/          日期时间设置工具（bin）
│   ├── minput/         按键诊断工具（bin）
├── platforms/
│   └── tg5040/         TrimUI Smart Pro / Brick 平台实现（SDL2）
│       ├── src/        平台 lib（纯 lib——settings/input/power 等）
│       ├── keymon/     keymon 系统级按键守护（独立 crate，平台自治 bin）
│       ├── show/       show 启动画面工具（独立 crate，平台自治 bin）
│       └── xtask/      tg5040 平台子 xtask（平台自治打包编排，父 xtask 调用）
├── xtask/              构建辅助（toolchain 流水线 + doc/test/lint/help）
├── toolchain/          工具链容器定义（Dockerfile——真机编译的容器环境）
├── skeleton/           SD 卡目录骨架（完全复制原版）
├── cores/              libretro 核心构建（通用机制，make 保持原样）
│                       └── platforms/<p>/cores/ 平台核心声明与补丁
├── fonts/              用户替换字体（CJK 支持）
└── i18n/               翻译文件
```

**依赖方向（严格遵守单向）：**

```
                common（零依赖）
                  ↑           ↑
                  │           └────────────┐
                  │                        │
              render                platforms/tg5040
           (fontdue + png)       (sdl2 + sdl2-sys + libc)
                  ↑                        ↑
                  └───────────┬────────────┘
                              │
      minui         minarch           clock          minput
    (无第三方)  (libloading+flate2)    (libc)      (无第三方)
      └───────────────┴───────────────┴───────────────┘
       四者均直接依赖 common + render + platform；platform 为
       可选依赖，经 `--features platform-tg5040/<device>` 透传

platforms/tg5040（平台 lib，纯 lib——无 bin）
  └── sdl2 + sdl2-sys + libc（show/keymon 拆为独立 crate：show 依赖平台 lib/common/png，
      keymon 依赖平台 lib/libc；均不依赖 render）
```

**设计原则：**
- **common 零第三方依赖** — Platform trait 是纯接口，不感知 SDL
- **render 只管渲染** — 所有"画东西"的函数集中一处，按语义分子模块
- **SDL 锁在平台实现内** — `platforms/tg5040` 内部用 SDL2，对外只暴露 `Platform` trait
- **minui ↔ minarch 互不调用** — 保持原版 shell 脚本通信
- **clock/minput 是通用工具** — 不内置平台逻辑，经 feature 透传实例化平台
- **show/keymon 是平台自治 bin** — 独立 crate，由平台子 xtask 编译装配；show 自实现图片解码，不依赖 render；是否打包由平台决定

## 3. 关键技术解析

### 3.1 Platform trait — 硬件抽象

`common::platform::Platform` 是所有硬件访问的唯一通道。上层代码（minui、minarch）通过泛型参数 `<P: Platform>` 获得硬件能力，编译时单态化，零运行时开销。

```rust
// minui 中使用（伪代码）
fn run<P: Platform>(platform: &mut P) {
    let mut screen = platform.init_video().unwrap();
    platform.poll_input();
    // ... 渲染循环 ...
}
```

**为什么这样设计？** 原版 C 使用 `#define` 宏在编译时替换平台函数名（如 `#define GFX_clear PLAT_clearVideo`），导致代码中到处是宏展开后的间接调用，难以追踪数据流。Rust trait 将接口与实现编译期绑定，类型系统确保调用正确性。

**trait 面**：27 方法 + 24 关联常量（16 设备常量 + 8 按键能力常量 HAS_*）——详见模块文档 common 页。

### 3.2 common 零依赖

`common` 不依赖任何第三方 crate，所有功能基于 Rust 标准库：文件 I/O 通过 `std::fs` 实现、字符串处理用原生 `String`/`&str`、无 `unsafe` 代码。

**为什么这样设计？** 对比原版 C：`api.c` 约 1,700 行，依赖 SDL、SDL_ttf、SDL_image，任何改动都可能触发级联编译。Rust 版将依赖层级严格分离——common 的改动不会触发 render 或平台的重新编译。

## 4. 与原 C 源码的对比

| 维度 | 原版 C | Rust 版 |
|------|--------|---------|
| **内存安全** | 手动 malloc/free，全局可变状态（`pad`、`gfx`、`top`、`stack`） | 所有权系统 + 借用检查消除悬垂指针和 UAF |
| **模块化** | 宏转发（`#define`），平台代码和通用代码混合编译 | trait 分离接口和实现，crate 边界明确 |
| **错误处理** | 无错误传播（`LOG_error` + 继续执行） | `Result<T, E>` 强制调用方处理异常路径 |
| **并发** | pthread 直接操作全局状态 | Send/Sync trait 在编译期检查数据竞争 |
| **构建系统** | makefile + per-platform 变量 | Cargo workspace + feature flag |
| **代码行数** | ~12,000 行 C | 预估 ~8,000-10,000 行 Rust |

## 5. workspace 契约

### 5.1 依赖声明纪律

- **全部依赖集中声明于根 `[workspace.dependencies]`**——内部 path 依赖与第三方依赖皆然；`path` 相对根解析，版本与 features 唯一来源。成员清单只写 `workspace = true`（外加"本 crate 为何用它"的注释），根表即**全项目依赖清单**（可审计）
- 成员**不得**再写版本号 / `path` / `features`——写 `path` 会被 Cargo **静默忽略**（无警告、无提示），残留的 `path` 是一句不会被检出的谎言
- 两条继承约束（Cargo 语言语义，非本仓库约定）：**不使用 `package` rename**——平台依赖 key 一律 = 包名 `platform-<code>`（rename 只能声明于根表，而此处无必要）；`optional` **不可**继承，必须留在成员——`platform-tg5040 = { workspace = true, optional = true }` 是唯一合法形态
- 根表的 `features` 对**所有**继承者生效（feature 叠加，成员无法退出）——单一 crate 专用的 feature 也一并写在根表（`unsafe_textures` / `use-pkgconfig` / `derive`）；未来若某成员需要不同组合，须在该成员改用显式声明脱离继承，并在根表注明原因
- workspace `resolver = "2"`、`edition = "2024"`

### 5.2 平台 feature 系统（平台代码 → 包名 `platform-<code>`）

**平台代码 = `PLATFORM` 常量值 = `.system/{code}` 目录名 = `platforms/{code}` 目录名 = xtask 的 `--platform` 参数**——平台代码是目录与运行期标识的锚点；**依赖 key / feature 名恒为包名 `platform-<code>`**（由平台代码加前缀派生，不使用 rename）：

- 平台 crate（如 `platform-tg5040`）的设备 feature（`smart`/`brick`）**互斥且必选、无默认**——未指定设备或同时指定两个时编译期报错（`compile_error!` 断言）
- 上层 crate 以 `--features platform-<code>/<device>` 透传（依赖 key = 包名，无 rename；path 声明于根 `[workspace.dependencies]`，成员只写 `platform-tg5040 = { workspace = true, optional = true }`）
- 条件全部**正向** `#[cfg(feature = "...")]`——禁止 `#[cfg(not(...))]` 负向逻辑（负向在叠加 feature 与第三设备场景下不可读/不可扩展）
- 新增设备须同步更新平台 crate 顶部两条 compile_error 断言（any/all）

### 5.3 接口面（两类 pub 面）

平台 crate 对外只暴露两类接口，由 Rust `pub(crate)` 可见性在编译期强制（无运行时检查）：

1. `Tg5040` 类型本身——仅装配层实例化
2. `settings::SettingsHandle`——跨进程系统设置

硬件实现模块（power/input）一律 `pub(crate)`；设备语义键（`BTN_SLEEP`/`BTN_MOD_*`）与能力常量（`HAS_*`）归 `Platform` trait 关联常量（历史演进见下方决策记录）。

## 6. 跨模块设计决策记录

### 决策：workspace 分层与 common 零依赖

- **结论**：六 crate（common/render/minui/minarch/clock/minput）+ 平台 + xtask 的 workspace；common 零第三方依赖。
- **为什么**：C 版 api.h/api.c 混合类型/渲染/平台/工具，SDL 耦合 + 全局状态 + 宏转发不可追踪；分层让修改 common 不触发上层重编译、纯逻辑无硬件可测。
- **被否决**：`dyn Platform` 运行时多态（虚表开销，静态分发零开销）；按依赖而非语义拆分（渲染就是渲染）。
- **产生的问题**：上层 crate 的依赖面需纪律维持（minui 无第三方、minarch 仅 libloading+flate2）。

### 决策：类型归属规则与布局常量单一来源

- **结论**：类型定义在语义域所在模块（`VsyncMode`/`FRAME_BUDGET_MS` → video、`CpuSpeed` → power、`AudioFrame` → audio）；实例由调用方持有。全部 UI 布局常量唯一定义在 `common::video`（`PILL_SIZE` 等），业务 crate 禁止重复定义。
- **为什么**：布局常量两处定义编译器不保证相等（C 的 `#include defines.h` 解决单一来源，Rust 用 `use`）；平台差异布局值经 trait 方法覆盖（见 common 决策记录）。
- **被否决**：平台 crate 本地常量；业务 crate 各自定义。
- **产生的问题**：新增布局值须先查 common::video（config.yaml 规则，xtask/平台移植清单均有对应检查项）。

### 决策：平台 feature 系统形态（编译错误取代静默回落）

- **结论**：正向 feature + `compile_error!` 双断言（互斥 + 必选），无默认。
- **为什么**：Cargo feature 是叠加的——负向 `not(brick)` 逻辑（早期版本）在叠加场景恰好规避重复定义，但不可读、第三设备无法表达；编译错误取代"无 feature 静默回落 smart"的隐含默认，设备选择必须显式。
- **被否决**：`not(brick)` 负向逻辑；`default = ["smart"]` 隐含默认（静默回落）；build.rs 运行时校验（compile_error! 更早更直接）。
- **产生的问题**：每设备独立二进制与安装包（发行模型变化，见 tg5040 决策记录）；所有构建命令须显式带 device。

### 决策：平台接口面收敛（白名单机制 → pub(crate)）

- **结论**：两类 pub 接口 + 硬件实现 pub(crate)；早期 xtask lint 的平台引用白名单（`PLATFORM_REF_WHITELIST`）随平台自治删除。
- **为什么**：白名单是运行时/CI 检查（漏检风险、清单维护），Rust 可见性在编译期强制更可靠；show/keymon 拆独立 crate 后平台 lib 的 pub 面进一步收敛。
- **被否决**：保留 lint 白名单；白名单豁免 show/keymon（豁免即机制失效）。
- **产生的问题**：语义键/能力常量从平台 crate 移入 trait（消费方跨 crate 需要 pub——trait 关联常量成为唯一通道）。

### 决策：cfg 路径族/协议常量的归属分层

- **结论**：原语（`SDCARD_PATH`/`PLATFORM`）在 trait 关联常量；派生路径在 `common::paths` 函数（13 个）；跨进程 /tmp 协议常量（`RESUME_SLOT_PATH`/`CHANGE_DISC_PATH`/`AUTO_RESUME_SLOT`）在 `common::paths` 常量区块；单进程常量（minui 的 `LAST_PATH`）留定义方。
- **为什么**：Rust 关联常量不能引用其他关联常量（E0401）——派生必须函数化；"谁使用谁定义 + 跨进程共享才进 common"。
- **被否决**：常量全做函数（无派生逻辑的伪抽象）；单建 consts 模块（3 个常量不配新模块）。
- **产生的问题**：跨 crate 共享路径时先确认消费方数量（双模块引用才进 common 原则）。

### 决策：依赖声明全部集中到根 `[workspace.dependencies]`

- **结论**：内部 path 依赖 + 全部第三方依赖（8 个唯一 crate）集中声明于根 `[workspace.dependencies]`（含平台自治 bin show/keymon）；平台依赖 key 一律用包名 `platform-<code>`（**不用 rename**，`--features platform-tg5040/<device>` 直白可读）；成员清单只剩 `workspace = true` 与"本 crate 为何用它"的注释。
- **为什么**：`common` 曾在 7 处、以三种相对深度声明（`../common` / `../../crates/common` / `../../../crates/common`），平台依赖的 path 与 rename 写法各重复 4 份，第三方版本串散落 12 处——crate 搬迁或升版要逐文件改。集中后根表即全项目依赖清单：一处看全版本、features 与原因注释，"可审计"从"逐 crate 可见"变为"一处可见全部"；成员清单退化为纯结构，依赖方向仍一眼可见（key 名不变）。
- **被否决**（两轮）：`workspace.dependencies` 保持全空（原 §5.1 措辞——三种拼写、4 份 rename、12 处版本串就是它的代价）；只集中 path 依赖、第三方逐 crate 声明（两套纪律并存，成员里"写版本还是写继承"要靠记忆——该中间态已实测通过，随后被推翻）；show/keymon 保留就地 `path = ".."`（会成为全项目唯一的非继承声明）；把平台依赖 key rename 成平台代码（`tg5040 = { package = "platform-tg5040", … }`——多一层间接，`--features tg5040/<device>` 不如包名直白，且徒增"rename 只能写在根表"这类约束）。
- **产生的问题**：① 成员侧再写 `path` 会被 Cargo **静默忽略**（无警告），靠纪律维持；② 根表 `features` 对全部继承者生效——单一 crate 专用的 feature（`unsafe_textures` / `use-pkgconfig`）也被全局化，成员无法退出；③ `optional` 不可继承，仍由成员声明。
