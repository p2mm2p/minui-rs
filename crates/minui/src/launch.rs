//! 游戏启动
//!
//! 负责构造启动命令并写入 `/tmp/next`，触发外部 shell 脚本启动模拟器
//! （minarch）。对应原版 C `openRom()`/`openPak()`/`queueNext()`/
//! `readyResumePath()`/`autoResume()`/`saveLast()` 逻辑（minui.c:946-1136/
//! :989-1064/:1212-1221）。
//!
//! ## 依赖方向
//!
//! 本模块只依赖 `common`、`disc` 与 `recents`（`add_recent`），
//! **不依赖 browser**——所有入口使用字符串参数（不收 `Entry`/
//! `EntryType`，目录判断用 `is_dir: bool` 表达）。
//!
//! ## 续玩语义（resume 参数）
//!
//! `open_rom` 的 `resume: bool` 是 C `should_resume` 全局的显式化——
//! "本次启动是否续玩存档"。C 的"目录级续玩通道"（对 cue/m3u 目录按
//! RESUME 后残留标志、让目录内下一个被 A 的 ROM 继承续玩意图）的
//! 跨帧存储（`resume_pending`）归 browser+main 变更的 MenuState；
//! 本模块只提供显式参数接口。
//!

#![cfg_attr(not(feature = "tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow

use common::paths::{
    AUTO_RESUME_SLOT, RESUME_SLOT_PATH, get_auto_resume_path, get_faux_recent_path, get_roms_path,
    get_shared_userdata_path,
};
use common::utils::{
    exists, get_emu_name, get_emu_path, get_file, prefix_match, put_file, put_int, remove_file,
    suffix_match,
};

use crate::disc;
use crate::recents::{Recent, add_recent};

/// 启动命令文件路径（shell 脚本读取并执行）
///
/// C 的 `queueNext` 直接写字符串（defines.h 无此宏）；`/tmp/next` 是
/// minui → shell 脚本的进程通信协议，脚本侧读取。
const NEXT_PATH: &str = "/tmp/next";

/// 上次浏览位置记录文件路径
///
/// 对应 C `defines.h:29` 的 `LAST_PATH`。**minui 独有**（grep 全
/// workspace 仅 `save_last`（本模块）与 `menu::load_last`（menu 模块）
/// 两个消费方）——故不放入 common::paths，遵循"单一使用者的常量谁
/// 使用谁定义"；本常量供 menu 侧读取（`use crate::launch::LAST_PATH`），
/// 故为 `pub`。
pub(crate) const LAST_PATH: &str = "/tmp/last.txt";

/// 转义 shell 单引号（`'` → `'\''`）
///
/// 对应原 C `escapeSingleQuotes()`（minui.c:978-985，基于
/// `replaceString`）。命令以单引号包裹路径时，路径内的单引号必须
/// 转义（`'foo'bar` → `'foo'\''bar'`）。
fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// 写入 /tmp/next 启动命令，返回"是否退出主循环"信号
///
/// 对应原 C `queueNext()`（minui.c:946-950）：写入命令并置 `quit=1`。
/// 调用方（main 主循环）收到 `true` 后退出循环、交出控制权给
/// shell 脚本/minarch。
///
/// # 参数
///
/// - `cmd`:启动命令（如 `"'/mnt/SDCARD/Emus/GB.pak/launch.sh' '/mnt/SDCARD/Roms/x.gb'"`）
///
/// # 返回值
///
/// 恒为 `true`（退出信号）。
pub(crate) fn queue_next(cmd: &str) -> bool {
    let _ = put_file(NEXT_PATH, cmd);
    true
}

/// 计算当前条目是否可续玩
///
/// 对应原 C `readyResumePath()`/`readyResume()`（minui.c:989-1028）。
/// 规则：
/// 1. 路径必须在 Roms 下（C :995）
/// 2. `is_dir=true`（目录条目）时：目录含同名 `.cue` 或 `.m3u` 才继续
///    （C :998-1005，candidate 路径 = `{dir}/{dir_name}.cue` 或 `.m3u`）
/// 3. 归一化为 m3u（C :1007-1013）
/// 4. slot 路径 = `{shared}/.minui/{emu}/{rom_file}.txt` 存在即 `true`（C :1022-1024）
///
/// # 参数
///
/// - `path`:条目路径（ROM 或目录，带 SDCARD 前缀）
/// - `is_dir`:是否为目录条目（对应 C 的 `type==ENTRY_DIR`）
/// - `sdcard_path`:SD 卡根路径（slot 路径派生需要）
///
/// # 返回值
///
/// `true`——存在对应存档 slot，可按 RESUME 续玩。
pub(crate) fn ready_resume(path: &str, is_dir: bool, sdcard_path: &str) -> bool {
    let roms_path = get_roms_path(sdcard_path);
    if !prefix_match(&roms_path, path) {
        return false; // C :995
    }

    let mut target = path.to_string();
    if is_dir {
        // C :998-1005——目录条目先检测 cue/m3u（candidate = {dir}/{dir_name}.cue / .m3u）
        let trimmed = target.trim_end_matches('/');
        let Some(dir_name) = trimmed.rsplit_once('/').map(|(_, d)| d) else {
            return false;
        };
        let cue = format!("{trimmed}/{dir_name}.cue");
        let m3u = format!("{trimmed}/{dir_name}.m3u");
        if exists(&cue) {
            target = cue;
        } else if exists(&m3u) {
            target = m3u;
        } else {
            return false;
        }
    }

    // C :1007-1013——归一化为 m3u（多碟游戏的 slot 挂在 m3u 上）
    if !suffix_match(".m3u", &target)
        && let Some(m3u) = disc::find_m3u(&target)
    {
        target = m3u;
    }

    // C :1015-1024——slot 路径存在性
    let emu = get_emu_name(&target, &roms_path);
    let rom_file = target.rsplit_once('/').map(|(_, f)| f).unwrap_or(&target);
    let slot_path = format!(
        "{}/.minui/{emu}/{rom_file}.txt",
        get_shared_userdata_path(sdcard_path)
    );
    exists(&slot_path)
}

/// 打开 ROM（构造启动命令 + 记录最近游玩）
///
/// 对应原 C `openRom()`（minui.c:1078-1137）。流程：
/// 1. `disc::find_m3u` 检测多碟（C :1084-1088）——记录用 m3u 路径
/// 2. 启动目标是 m3u 时用 `disc::get_first_disc` 定位实际碟片（C :1090-1092）
/// 3. `resume=true`：读取存档槽位 → 写 `RESUME_SLOT_PATH` → 多碟时按
///    存档记录的碟片切换启动目标（C :1097-1122）；`resume=false`：
///    `put_int(RESUME_SLOT_PATH, 8)` 隐藏默认槽位（C :1124）
/// 4. `get_emu_path` 构造模拟器启动脚本路径（C :1126-1127）
/// 5. `add_recent` 记录最近游玩（C :1131）
/// 6. `queue_next` 写入命令 `'{emu_path}' '{rom_path}'`（C :1134-1136）
///
/// 注意：C 的 `saveLast` 在 openRom 内调用，但其参数依赖"当前是否在
/// Collections/Recently Played"等 main 层上下文——Rust 版由调用方
/// （main 分发处）在调用后自行 `save_last`。
///
/// # 参数
///
/// - `path`:ROM 完整路径（带 SDCARD 前缀）
/// - `resume`:本次启动是否续玩存档（C `should_resume` 的显式化）
/// - `recent_alias`:记录到最近列表的可选别名（C 的 `recent_alias` 全局）
/// - `recents`:最近列表（`add_recent` 副作用）
/// - `sdcard_path`/`platform`/`paks_path`:平台原语
///
/// # 返回值
///
/// 退出信号（恒 `true`——启动后主循环退出）。
pub(crate) fn open_rom(
    path: &str,
    resume: bool,
    recent_alias: Option<&str>,
    recents: &mut Vec<Recent>,
    sdcard_path: &str,
    platform: &str,
    paks_path: &str,
) -> bool {
    let roms_path = get_roms_path(sdcard_path);
    let mut sd_path = path.to_string();

    // ── 1-2. m3u 检测与碟片定位（C :1084-1092）──
    let m3u = disc::find_m3u(&sd_path);
    let recent_path = m3u.clone().unwrap_or_else(|| sd_path.clone());
    if m3u.is_some()
        && suffix_match(".m3u", &sd_path)
        && let Some(first) = disc::get_first_disc(&recent_path)
    {
        sd_path = first;
    }

    let emu_name = get_emu_name(&sd_path, &roms_path);

    // ── 3. 续玩/默认槽位（C :1097-1124）──
    if resume {
        // 读取存档槽位——slot 路径基于 m3u 归一化目标重建
        // （对应 C 的全局 slot_path，readyResumePath 基于条目构造）
        let slot_target = m3u.as_deref().unwrap_or(&sd_path);
        let slot_emu = get_emu_name(slot_target, &roms_path);
        let slot_file = slot_target
            .rsplit_once('/')
            .map(|(_, f)| f)
            .unwrap_or(slot_target);
        let shared = get_shared_userdata_path(sdcard_path);
        let slot_path = format!("{shared}/.minui/{slot_emu}/{slot_file}.txt");

        if let Some(slot) = get_file(&slot_path) {
            let slot = slot.trim().to_string();
            let _ = put_file(RESUME_SLOT_PATH, &slot);

            // 多碟状态切换（C :1103-1122）——按存档记录的碟片切换启动目标
            if m3u.is_some() {
                let m3u_file = m3u
                    .as_deref()
                    .unwrap()
                    .rsplit_once('/')
                    .map(|(_, f)| f)
                    .unwrap_or("");
                let disc_state_path = format!("{shared}/.minui/{slot_emu}/{m3u_file}.{slot}.txt");
                if exists(&disc_state_path)
                    && let Some(disc_path) = get_file(&disc_state_path)
                {
                    let disc_path = disc_path.trim();
                    if disc_path.starts_with('/') {
                        // 绝对路径直接使用（C :1115）
                        sd_path = disc_path.to_string();
                    } else {
                        // 相对路径拼到 m3u 目录（C :1116-1120）
                        let m3u_dir = m3u
                            .as_deref()
                            .unwrap()
                            .rsplit_once('/')
                            .map(|(p, _)| p)
                            .unwrap_or("");
                        sd_path = format!("{m3u_dir}/{disc_path}");
                    }
                }
            }
        }
    } else {
        // C :1124——隐藏默认槽位（8 是 resume 隐藏状态）
        let _ = put_int(RESUME_SLOT_PATH, 8);
    }

    // ── 4-6. 模拟器路径 + 记录 + 命令（C :1126-1136）──
    let emu_path = get_emu_path(&emu_name, sdcard_path, platform, paks_path);
    add_recent(
        recents,
        &recent_path,
        recent_alias,
        sdcard_path,
        platform,
        paks_path,
    );

    let cmd = format!(
        "'{}' '{}'",
        escape_single_quotes(&emu_path),
        escape_single_quotes(&sd_path)
    );
    queue_next(&cmd)
}

/// 打开 .pak 工具包
///
/// 对应原 C `openPak()`（minui.c:1066-1077）：路径在 Roms 下才记录
/// 最近游玩（C :1069-1071）；命令为 `'{path}/launch.sh'`（.pak 目录
/// 自带 launch.sh，直接执行）。
///
/// # 参数
///
/// - `path`:`.pak` 目录完整路径（带 SDCARD 前缀）
/// - `recents`:最近列表
/// - `sdcard_path`/`platform`/`paks_path`:平台原语（`add_recent` 需要）
///
/// # 返回值
///
/// 退出信号（恒 `true`）。
pub(crate) fn open_pak(
    path: &str,
    recents: &mut Vec<Recent>,
    sdcard_path: &str,
    platform: &str,
    paks_path: &str,
) -> bool {
    let roms_path = get_roms_path(sdcard_path);
    if prefix_match(&roms_path, path) {
        add_recent(recents, path, None, sdcard_path, platform, paks_path);
    }
    let cmd = format!("'{}/launch.sh'", escape_single_quotes(path));
    queue_next(&cmd)
}

/// 自动续玩（main 启动时调用）
///
/// 对应原 C `autoResume()`（minui.c:1033-1064）。`AUTO_RESUME_PATH`
/// 存在时：读取 → **删除标记** → 校验 ROM 与模拟器仍存在 → 写
/// `RESUME_SLOT_PATH`（`AUTO_RESUME_SLOT=9`）→ `queue_next` → `true`。
/// main 收到 `true` 直接退出（跳过 UI 初始化——开机即续玩）。
///
/// # 参数
///
/// - `sdcard_path`/`platform`/`paks_path`:平台原语
///
/// # 返回值
///
/// `true`——已排队启动（main 直接退出）；`false`——正常进入 UI。
pub(crate) fn auto_resume(sdcard_path: &str, platform: &str, paks_path: &str) -> bool {
    let auto_path = get_auto_resume_path(sdcard_path);
    if !exists(&auto_path) {
        return false;
    }
    let Some(content) = get_file(&auto_path) else {
        return false;
    };
    let path = content.trim().to_string();
    let _ = remove_file(&auto_path); // C :1040

    // ROM 仍存在？（C :1044-1046）
    let sd_path = format!("{sdcard_path}{path}");
    if !exists(&sd_path) {
        return false;
    }

    // 模拟器仍存在？（C :1048-1055）
    let roms_path = get_roms_path(sdcard_path);
    let emu_name = get_emu_name(&sd_path, &roms_path);
    let emu_path = get_emu_path(&emu_name, sdcard_path, platform, paks_path);
    if !exists(&emu_path) {
        return false;
    }

    // C :1060-1062——写默认槽位 + 启动
    let _ = put_int(RESUME_SLOT_PATH, AUTO_RESUME_SLOT);
    let cmd = format!(
        "'{}' '{}'",
        escape_single_quotes(&emu_path),
        escape_single_quotes(&sd_path)
    );
    queue_next(&cmd)
}

/// 保存"上次浏览位置"到 /tmp/last.txt
///
/// 对应原 C `saveLast()`（minui.c:1212-1221）。`top_is_recents=true`
/// 时保存 Recently Played 伪目录路径（最近游玩恒在列表顶部，无需记
/// 具体条目——C 注释 minui.c:1215-1218）；否则保存传入路径。
///
/// # 参数
///
/// - `top_is_recents`:当前是否在 Recently Played 伪目录（对应 C 读
///   `top->path` 的 FAUX_RECENT_PATH 特判——Rust 由调用方传入）
/// - `path`:要保存的路径（带 SDCARD 前缀）
/// - `sdcard_path`:SD 卡根路径（构造 Recently Played 路径需要）
pub(crate) fn save_last(top_is_recents: bool, path: &str, sdcard_path: &str) {
    let content = if top_is_recents {
        get_faux_recent_path(sdcard_path)
    } else {
        path.to_string()
    };
    let _ = put_file(LAST_PATH, &content);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 生成测试 SD 卡根目录（正斜杠路径）
    fn temp_root(name: &str) -> String {
        let base = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        format!("{base}/minui_launch_{}_{name}", std::process::id())
    }

    const PLATFORM: &str = "tg5040";

    fn paks(root: &str) -> String {
        format!("{root}/.system/{PLATFORM}/paks")
    }

    /// 构造 SD 卡 + 安装模拟器
    fn setup(root: &str, emu: &str) {
        let _ = fs::create_dir_all("/tmp"); // Windows 无 /tmp 的兜底
        fs::create_dir_all(format!("{root}/.userdata/shared/.minui")).unwrap();
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        let pak = format!("{root}/Emus/{PLATFORM}/{emu}.pak/launch.sh");
        fs::create_dir_all(std::path::Path::new(&pak).parent().unwrap()).unwrap();
        fs::write(&pak, "#!/bin/sh\n").unwrap();
    }

    fn cleanup_tmp_files() {
        let _ = fs::remove_file(NEXT_PATH);
        let _ = fs::remove_file(LAST_PATH);
        let _ = fs::remove_file(RESUME_SLOT_PATH);
    }

    /// /tmp 协议文件（NEXT_PATH/LAST_PATH/RESUME_SLOT_PATH）是**全局共享**
    /// 资源——cargo test 默认并行执行，涉及它们的测试必须串行。
    /// 纯 std 方案（静态 Mutex），避免引入测试专用 crate。

    // ── queue_next ──────────────────────────────

    #[test]
    fn queue_next_writes_and_signals() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("queue_next");
        setup(&root, "GB");
        cleanup_tmp_files();

        let result = queue_next("'emu' 'rom'");
        assert!(result, "返回退出信号");
        assert_eq!(
            fs::read_to_string(NEXT_PATH).unwrap(),
            "'emu' 'rom'",
            "/tmp/next 内容"
        );

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    // ── ready_resume ────────────────────────────

    fn make_slot(root: &str, emu: &str, rom_file: &str) {
        let slot_dir = format!("{root}/.userdata/shared/.minui/{emu}");
        fs::create_dir_all(&slot_dir).unwrap();
        fs::write(format!("{slot_dir}/{rom_file}.txt"), "0").unwrap();
    }

    #[test]
    fn ready_resume_rom_with_slot() {
        let root = temp_root("rr_rom");
        setup(&root, "SFC");
        let rom = format!("{root}/Roms/SFC/Game.sfc");
        fs::write(&rom, "x").unwrap();
        make_slot(&root, "SFC", "Game.sfc");

        assert!(ready_resume(&rom, false, &root));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ready_resume_rom_without_slot() {
        let root = temp_root("rr_rom_no_slot");
        setup(&root, "SFC");
        let rom = format!("{root}/Roms/SFC/Game.sfc");
        fs::write(&rom, "x").unwrap();

        assert!(!ready_resume(&rom, false, &root));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ready_resume_dir_with_cue() {
        let root = temp_root("rr_dir_cue");
        setup(&root, "PS");
        let game_dir = format!("{root}/Roms/PS/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.cue"),
            "FILE \"x.bin\" BINARY\n",
        )
        .unwrap();
        fs::write(format!("{game_dir}/Final Fantasy VII.bin"), "x").unwrap();
        make_slot(&root, "PS", "Final Fantasy VII.cue");

        assert!(ready_resume(&game_dir, true, &root));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ready_resume_dir_with_m3u() {
        let root = temp_root("rr_dir_m3u");
        setup(&root, "SFC");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Final Fantasy VII.m3u"), "Disc 1.sfc\n").unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();
        make_slot(&root, "SFC", "Final Fantasy VII.m3u");

        assert!(ready_resume(&game_dir, true, &root));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ready_resume_dir_without_cue_m3u() {
        let root = temp_root("rr_dir_none");
        setup(&root, "SFC");
        let game_dir = format!("{root}/Roms/SFC/Plain");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Plain.sfc"), "x").unwrap();

        assert!(!ready_resume(&game_dir, true, &root));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ready_resume_non_roms_path() {
        let root = temp_root("rr_non_roms");
        setup(&root, "GB");

        assert!(!ready_resume("/tmp/some/file", false, &root));

        fs::remove_dir_all(&root).unwrap();
    }

    // ── open_rom ────────────────────────────────

    #[test]
    fn open_rom_single_disc_no_resume() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("open_rom_single");
        setup(&root, "SFC");
        cleanup_tmp_files();
        let rom = format!("{root}/Roms/SFC/Game.sfc");
        fs::write(&rom, "x").unwrap();
        let mut recents: Vec<Recent> = Vec::new();

        open_rom(
            &rom,
            false,
            Some("Game"),
            &mut recents,
            &root,
            PLATFORM,
            &paks(&root),
        );

        // 命令 = '{emu_path}' '{rom_path}'
        let expected = format!("'{root}/Emus/{PLATFORM}/SFC.pak/launch.sh' '{rom}'");
        assert_eq!(fs::read_to_string(NEXT_PATH).unwrap(), expected);
        // 默认槽位 8
        assert_eq!(fs::read_to_string(RESUME_SLOT_PATH).unwrap().trim(), "8");
        // 记录最近游玩
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].path, "/Roms/SFC/Game.sfc");
        assert_eq!(recents[0].alias.as_deref(), Some("Game"));

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_rom_multi_disc_records_m3u() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("open_rom_multi");
        setup(&root, "SFC");
        cleanup_tmp_files();
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.m3u"),
            "Disc 1.sfc\nDisc 2.sfc\n",
        )
        .unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();
        fs::write(format!("{game_dir}/Disc 2.sfc"), "x").unwrap();
        let disc1 = format!("{game_dir}/Disc 1.sfc");
        let mut recents: Vec<Recent> = Vec::new();

        open_rom(
            &disc1,
            false,
            None,
            &mut recents,
            &root,
            PLATFORM,
            &paks(&root),
        );

        // 记录用 m3u 路径（C :1088 recent_path = m3u_path）
        assert_eq!(
            recents[0].path,
            "/Roms/SFC/Final Fantasy VII/Final Fantasy VII.m3u"
        );
        // 启动目标是第一碟
        let expected = format!("'{root}/Emus/{PLATFORM}/SFC.pak/launch.sh' '{disc1}'");
        assert_eq!(fs::read_to_string(NEXT_PATH).unwrap(), expected);

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_rom_resume_writes_slot() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("open_rom_resume");
        setup(&root, "SFC");
        cleanup_tmp_files();
        let rom = format!("{root}/Roms/SFC/Game.sfc");
        fs::write(&rom, "x").unwrap();
        make_slot(&root, "SFC", "Game.sfc");
        // 槽位文件内容 = "3"
        fs::write(
            format!("{root}/.userdata/shared/.minui/SFC/Game.sfc.txt"),
            "3",
        )
        .unwrap();
        let mut recents: Vec<Recent> = Vec::new();

        open_rom(
            &rom,
            true,
            None,
            &mut recents,
            &root,
            PLATFORM,
            &paks(&root),
        );

        // RESUME_SLOT_PATH 写入槽位内容（C :1099-1101）
        assert_eq!(fs::read_to_string(RESUME_SLOT_PATH).unwrap().trim(), "3");

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    // ── open_pak ────────────────────────────────

    #[test]
    fn open_pak_in_roms_adds_recent() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("open_pak_in");
        setup(&root, "GB");
        cleanup_tmp_files();
        let pak = format!("{root}/Roms/Game Boy (GB)/Pokemon Red.pak");
        let _ = fs::create_dir_all(&pak);
        let mut recents: Vec<Recent> = Vec::new();

        open_pak(&pak, &mut recents, &root, PLATFORM, &paks(&root));

        let expected = format!("'{pak}/launch.sh'");
        assert_eq!(fs::read_to_string(NEXT_PATH).unwrap(), expected);
        assert_eq!(recents.len(), 1, "Roms 下的 pak 记录最近游玩");

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_pak_outside_roms_no_recent() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("open_pak_out");
        setup(&root, "GB");
        cleanup_tmp_files();
        let pak = format!("{root}/Tools/{PLATFORM}/sometool.pak");
        let _ = fs::create_dir_all(&pak);
        let mut recents: Vec<Recent> = Vec::new();

        open_pak(&pak, &mut recents, &root, PLATFORM, &paks(&root));

        assert!(recents.is_empty(), "非 Roms 下的 pak 不记录");

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    // ── auto_resume ─────────────────────────────

    #[test]
    fn auto_resume_consumes_marker() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("auto_resume");
        setup(&root, "SFC");
        cleanup_tmp_files();
        let rom = format!("{root}/Roms/SFC/Game.sfc");
        fs::write(&rom, "x").unwrap();
        // minarch 写的标记：无前缀路径（minarch.c:3117 写入格式）
        fs::write(
            format!("{root}/.userdata/shared/.minui/auto_resume.txt"),
            "/Roms/SFC/Game.sfc",
        )
        .unwrap();

        let result = auto_resume(&root, PLATFORM, &paks(&root));
        assert!(result);
        // 标记被删除
        assert!(!exists(&format!(
            "{root}/.userdata/shared/.minui/auto_resume.txt"
        )));
        // 默认槽位 9（AUTO_RESUME_SLOT）
        assert_eq!(fs::read_to_string(RESUME_SLOT_PATH).unwrap().trim(), "9");

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn auto_resume_no_marker() {
        let root = temp_root("auto_resume_none");
        setup(&root, "SFC");

        assert!(!auto_resume(&root, PLATFORM, &paks(&root)));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn auto_resume_missing_rom() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("auto_resume_missing");
        setup(&root, "SFC");
        cleanup_tmp_files();
        fs::write(
            format!("{root}/.userdata/shared/.minui/auto_resume.txt"),
            "/Roms/SFC/Gone.sfc",
        )
        .unwrap();

        assert!(!auto_resume(&root, PLATFORM, &paks(&root)));
        // 标记已消费（C :1040 读取即删除）
        assert!(!exists(&format!(
            "{root}/.userdata/shared/.minui/auto_resume.txt"
        )));

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    // ── save_last ───────────────────────────────

    #[test]
    fn save_last_recents_special_case() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("save_last_recents");
        setup(&root, "GB");
        cleanup_tmp_files();

        save_last(true, &format!("{root}/Roms/SFC/Game.sfc"), &root);

        let saved = fs::read_to_string(LAST_PATH).unwrap();
        assert_eq!(saved, format!("{root}/Recently Played"), "特判写伪目录路径");

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn save_last_regular_path() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("save_last_regular");
        setup(&root, "GB");
        cleanup_tmp_files();
        let rom = format!("{root}/Roms/SFC/Game.sfc");

        save_last(false, &rom, &root);

        let saved = fs::read_to_string(LAST_PATH).unwrap();
        assert_eq!(saved, rom);

        cleanup_tmp_files();
        fs::remove_dir_all(&root).unwrap();
    }
}
