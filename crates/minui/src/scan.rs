//! # 文件系统扫描
//!
//! 对应原 C 代码 `minui.c` 中的扫描和路径函数。

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use common::types::*;
use common::utils::*;

// ============================================================================
// 常量
// ============================================================================

pub const AUTO_RESUME_SLOT: i32 = 9;
pub const DEFAULT_SLOT: i32 = 8;
pub const MAX_RECENTS: usize = 24;

// ============================================================================
// 路径构造
// ============================================================================

pub fn roms_path(sdcard: &str) -> String { format!("{}/Roms", sdcard) }
pub fn faux_recent_path(sdcard: &str) -> String { format!("{}/Recently Played", sdcard) }
pub fn collections_path(sdcard: &str) -> String { format!("{}/Collections", sdcard) }
pub fn paks_path(sdcard: &str, platform: &str) -> String { format!("{}/.system/{}/paks", sdcard, platform) }
pub fn shared_userdata_path(sdcard: &str) -> String { format!("{}/.userdata/shared", sdcard) }
pub fn recent_file_path(sdcard: &str) -> String { format!("{}/.minui/recent.txt", shared_userdata_path(sdcard)) }

// ============================================================================
// 模拟器可用性
// ============================================================================

pub fn has_emu(emu_name: &str, sdcard_path: &str, platform_tag: &str, paks: &str) -> bool {
    let pak1 = format!("{}/Emus/{}.pak/launch.sh", paks, emu_name);
    if path_exists(&pak1) { return true; }
    let pak2 = format!("{}/Emus/{}/{}.pak/launch.sh", sdcard_path, platform_tag, emu_name);
    path_exists(&pak2)
}

pub fn find_cue(dir_path: &str) -> Option<String> {
    let dir_name = Path::new(dir_path).file_name().and_then(|n| n.to_str())?;
    let cue_path = format!("{}/{}.cue", dir_path, dir_name);
    if path_exists(&cue_path) { Some(cue_path) } else { None }
}

pub fn find_m3u(rom_path: &str) -> Option<String> {
    let rom_parent = Path::new(rom_path).parent()?.to_str()?;
    let parent_name = Path::new(rom_parent).file_name().and_then(|n| n.to_str())?;
    let grandparent = Path::new(rom_parent).parent()?.to_str()?;
    let m3u_path = format!("{}/{}.m3u", grandparent, parent_name);
    if path_exists(&m3u_path) { Some(m3u_path) } else { None }
}

pub fn get_first_disc(m3u_path: &str) -> Option<String> {
    let base_path = Path::new(m3u_path).parent()?.to_str()?;
    let content = std::fs::read_to_string(m3u_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let disc_path = format!("{}/{}", base_path, line);
        if path_exists(&disc_path) { return Some(disc_path); }
    }
    None
}

pub fn is_console_dir(path: &str, sdcard: &str) -> bool {
    parent_dir(path).map_or(false, |p| p == roms_path(sdcard))
}

pub fn extract_collate_prefix(path: &str) -> String {
    path.rfind('(').map(|p| path[..=p].to_string()).unwrap_or_else(|| path.to_string())
}

pub fn scan_dir_has_visible(path: &str) -> bool {
    fs::read_dir(path).map(|entries| {
        entries.filter_map(|e| e.ok()).any(|e| !hide(&e.file_name().to_string_lossy()))
    }).unwrap_or(false)
}

pub fn has_roms(dir_name: &str, sdcard_path: &str, platform_tag: &str, paks: &str) -> bool {
    let emu_name = get_emu_name(dir_name, &roms_path(sdcard_path));
    if !has_emu(&emu_name, sdcard_path, platform_tag, paks) { return false; }
    let rom_dir = format!("{}/{}", roms_path(sdcard_path), dir_name);
    scan_dir_has_visible(&rom_dir)
}

// ============================================================================
// Entry 构造
// ============================================================================

fn create_entry(path: &str, entry_type: EntryType) -> Entry {
    let name = get_display_name(path);
    Entry {
        path: path.to_string(),
        name,
        unique: None,
        entry_type,
        alpha: 0,
    }
}

fn create_entry_with_platform(path: &str, entry_type: EntryType, platform_tag: &str) -> Entry {
    let name = get_display_name_with_platform(path, platform_tag);
    Entry {
        path: path.to_string(),
        name,
        unique: None,
        entry_type,
        alpha: 0,
    }
}

// ============================================================================
// 目录扫描
// ============================================================================

pub fn scan_dir(path: &str, is_collection: bool) -> Vec<Entry> {
    let mut entries = Vec::new();
    let dir_iter = match fs::read_dir(path) {
        Ok(iter) => iter,
        Err(_) => return entries,
    };
    for entry in dir_iter.filter_map(|e| e.ok()) {
        let name_str = entry.file_name().to_string_lossy().to_string();
        if hide(&name_str) { continue; }
        let full_path_str = entry.path().to_string_lossy().to_string();
        let entry_type = if entry.path().is_dir() {
            if is_pak(&name_str) { EntryType::Pak } else { EntryType::Dir }
        } else if is_collection {
            EntryType::Dir
        } else {
            EntryType::Rom
        };
        entries.push(create_entry(&full_path_str, entry_type));
    }
    entries
}

pub fn get_entries(path: &str, sdcard: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    if is_console_dir(path, sdcard) {
        let collated_prefix = extract_collate_prefix(path);
        let roms = roms_path(sdcard);
        if let Ok(dir_iter) = fs::read_dir(&roms) {
            for entry in dir_iter.filter_map(|e| e.ok()) {
                let name_str = entry.file_name().to_string_lossy().to_string();
                if hide(&name_str) { continue; }
                let full_path_str = entry.path().to_string_lossy().to_string();
                if !entry.path().is_dir() { continue; }
                if !prefix_match(&collated_prefix, &full_path_str) { continue; }
                entries.append(&mut scan_dir(&full_path_str, false));
            }
        }
    } else {
        entries = scan_dir(path, false);
    }
    entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    entries
}

// ============================================================================
// 特殊目录扫描
// ============================================================================

pub fn get_root(
    sdcard: &str,
    platform_tag: &str,
    paks: &str,
    has_any_recent: bool,
    _has_any_collections: bool,
    simple_mode: bool,
) -> Vec<Entry> {
    let mut root: Vec<Entry> = Vec::new();
    let roms = roms_path(sdcard);

    if has_any_recent {
        root.push(create_entry(&faux_recent_path(sdcard), EntryType::Dir));
    }

    let mut entries: Vec<Entry> = Vec::new();
    let mut emus: Vec<Entry> = Vec::new();

    if let Ok(dir_iter) = fs::read_dir(&roms) {
        for entry in dir_iter.filter_map(|e| e.ok()) {
            let name_str = entry.file_name().to_string_lossy().to_string();
            if hide(&name_str) { continue; }
            let full_path_str = entry.path().to_string_lossy().to_string();
            if !entry.path().is_dir() { continue; }
            if has_roms(&name_str, sdcard, platform_tag, paks) {
                emus.push(create_entry(&full_path_str, EntryType::Dir));
            }
        }
    }

    emus.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    let mut prev_name: Option<String> = None;
    for entry in emus {
        let should_keep = match &prev_name {
            Some(prev) => entry.name != *prev,
            None => true,
        };
        if should_keep {
            prev_name = Some(entry.name.clone());
            entries.push(entry);
        }
    }

    let map_path = format!("{}/map.txt", roms);
    if !entries.is_empty() && path_exists(&map_path) {
        apply_name_map(&mut entries, &map_path);
    }

    let collections = collections_path(sdcard);
    let has_collections = path_exists(&collections) && scan_dir_has_visible(&collections);

    if has_collections {
        if !entries.is_empty() {
            root.push(create_entry(&collections, EntryType::Dir));
        } else {
            let mut coll_entries = scan_dir(&collections, true);
            coll_entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
            entries.append(&mut coll_entries);
        }
    }

    root.append(&mut entries);

    if !simple_mode {
        let tools_path = format!("{}/Tools/{}", sdcard, platform_tag);
        if path_exists(&tools_path) {
            root.push(create_entry_with_platform(&tools_path, EntryType::Dir, platform_tag));
        }
    }

    root
}

pub fn get_recents_from_list(recents: &[Recent], sdcard: &str) -> Vec<Entry> {
    recents.iter().filter(|r| r.available).map(|r| {
        let sd_path = format!("{}{}", sdcard, r.path);
        let entry_type = if is_pak(&sd_path) { EntryType::Pak } else { EntryType::Rom };
        let mut entry = create_entry(&sd_path, entry_type);
        if let Some(ref alias) = r.alias { entry.name = alias.clone(); }
        entry
    }).collect()
}

pub fn get_collection(collection_path: &str, sdcard: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let content = match std::fs::read_to_string(collection_path) {
        Ok(c) => c, Err(_) => return entries,
    };
    for line in content.lines() {
        let line = normalize_newline(line);
        let line = trim_trailing_newlines(&line).trim();
        if line.is_empty() { continue; }
        let sd_path = format!("{}/{}", sdcard, line);
        if path_exists(&sd_path) {
            let entry_type = if is_pak(&sd_path) { EntryType::Pak } else { EntryType::Rom };
            entries.push(create_entry(&sd_path, entry_type));
        }
    }
    entries
}

pub fn get_discs(m3u_path: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let base_path = match Path::new(m3u_path).parent().and_then(|p| p.to_str()) {
        Some(p) => p, None => return entries,
    };
    let content = match std::fs::read_to_string(m3u_path) {
        Ok(c) => c, Err(_) => return entries,
    };
    let mut disc = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let disc_path = format!("{}/{}", base_path, line);
        if path_exists(&disc_path) {
            disc += 1;
            let mut entry = create_entry(&disc_path, EntryType::Rom);
            entry.name = format!("Disc {}", disc);
            entries.push(entry);
        }
    }
    entries
}

// ============================================================================
// 路径分派
// ============================================================================

pub fn get_entries_for_path(
    path: &str, sdcard: &str, platform_tag: &str, paks: &str,
    recents: &[Recent], simple_mode: bool,
) -> Vec<Entry> {
    let faux = faux_recent_path(sdcard);
    let col = collections_path(sdcard);

    if exact_match(path, sdcard) {
        let has_recents = !recents.is_empty();
        let has_cols = path_exists(&col) && scan_dir_has_visible(&col);
        return get_root(sdcard, platform_tag, paks, has_recents, has_cols, simple_mode);
    }
    if exact_match(path, &faux) {
        return get_recents_from_list(recents, sdcard);
    }
    if !exact_match(path, &col) && prefix_match(&col, path) && suffix_match(".txt", path) {
        return get_collection(path, sdcard);
    }
    if is_m3u(path) {
        return get_discs(path);
    }
    get_entries(path, sdcard)
}

// ============================================================================
// Directory 构造
// ============================================================================

pub fn make_directory(
    path: &str, entries: Vec<Entry>, selected: usize,
    sdcard: &str, platform_tag: &str,
) -> Directory {
    let name = get_display_name(path);
    let mut entries = entries;
    apply_directory_maps(&mut entries, path, sdcard);

    let faux = faux_recent_path(sdcard);
    let col = collections_path(sdcard);
    let is_faux_recent = path == faux;
    let is_collection = prefix_match(&col, path) && !exact_match(path, &col);
    let skip_index = is_faux_recent || is_collection;

    let mut alphas: Vec<usize> = Vec::new();
    let mut alpha_index: isize = -1;

    entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));

    for i in 0..entries.len() {
        if !skip_index {
            let first_char = entries[i].name.chars().next().unwrap_or(' ').to_ascii_lowercase();
            let letter_index = if first_char.is_ascii_alphabetic() {
                (first_char as u8 - b'a') as isize + 1
            } else { 0 };
            if letter_index != alpha_index {
                alpha_index = letter_index;
                alphas.push(i);
            }
            entries[i].alpha = alphas.len().saturating_sub(1);
        }
    }

    fix_duplicate_names(&mut entries, path, sdcard, platform_tag);

    let total = entries.len();
    let visible_count = 6;
    let selected = selected.min(total.saturating_sub(1));
    let start = if total <= visible_count { 0 } else {
        selected.saturating_sub(visible_count / 2).min(total - visible_count)
    };
    let end = if total <= visible_count { total } else { start + visible_count };

    Directory { path: path.to_string(), name, entries, alphas, selected, start, end }
}

fn make_unique_name(path: &str, _platform_tag: &str) -> String {
    let display = get_display_name(path);
    let tag = extract_emu_tag_from_console_path(path);
    format!("{} ({})", display, tag)
}

fn extract_emu_tag_from_console_path(path: &str) -> String {
    if let Some(paren_start) = path.rfind('(') {
        let after = &path[paren_start + 1..];
        if let Some(paren_end) = after.rfind(')') {
            return after[..paren_end].to_string();
        }
    }
    file_name(path).unwrap_or("?").to_string()
}

fn fix_duplicate_names(
    entries: &mut [Entry], _dir_path: &str, _sdcard: &str, platform_tag: &str,
) {
    let len = entries.len();
    let mut updates: Vec<(usize, Option<String>)> = Vec::new();
    for i in 0..len.saturating_sub(1) {
        if entries[i].name == entries[i + 1].name {
            let same_filename = file_name(&entries[i].path) == file_name(&entries[i + 1].path);
            let (u1, u2) = if same_filename {
                (Some(make_unique_name(&entries[i].path, platform_tag)),
                 Some(make_unique_name(&entries[i + 1].path, platform_tag)))
            } else {
                (file_name(&entries[i].path).map(String::from),
                 file_name(&entries[i + 1].path).map(String::from))
            };
            updates.push((i, u1));
            updates.push((i + 1, u2));
        }
    }
    for (idx, unique) in updates {
        if idx < entries.len() { entries[idx].unique = unique; }
    }
}

fn apply_directory_maps(entries: &mut Vec<Entry>, dir_path: &str, _sdcard: &str) {
    let map_path = format!("{}/map.txt", dir_path);
    if path_exists(&map_path) {
        apply_name_map(entries, &map_path);
        entries.retain(|e| !hide(&e.name));
        entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    }
}

fn apply_name_map(entries: &mut [Entry], map_path: &str) {
    let content = match std::fs::read_to_string(map_path) {
        Ok(c) => c, Err(_) => return,
    };
    let mut map: HashMap<String, String> = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Some(tab_pos) = line.find('\t') {
            let key = line[..tab_pos].to_string();
            let value = line[tab_pos + 1..].to_string();
            map.insert(key, value);
        }
    }
    let mut needs_resort = false;
    for entry in entries.iter_mut() {
        let filename = file_name(&entry.path).unwrap_or("");
        if let Some(alias) = map.get(filename) {
            entry.name = alias.clone();
            needs_resort = true;
        }
    }
    if needs_resort {
        entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    }
}

// ============================================================================
// 最近游戏管理
// ============================================================================

pub fn load_recents(
    sdcard: &str, platform_tag: &str, paks: &str,
) -> (Vec<Recent>, bool) {
    let mut recents: Vec<Recent> = Vec::new();
    let mut parent_paths: Vec<String> = Vec::new();

    let change_disc_path = common::paths::CHANGE_DISC_PATH;
    if path_exists(change_disc_path) {
        if let Ok(sd_path) = std::fs::read_to_string(change_disc_path) {
            let sd_path = sd_path.trim();
            if path_exists(sd_path) {
                let disc_path = sd_path.strip_prefix(sdcard).unwrap_or(sd_path).to_string();
                let emu_name = extract_emu_tag_from_console_path(sd_path);
                let available = has_emu(&emu_name, sdcard, platform_tag, paks);
                if available {
                    recents.push(Recent { path: disc_path.clone(), alias: None, available });
                }
                if let Some(parent) = parent_dir(&disc_path) {
                    parent_paths.push(parent.to_string());
                }
            }
        }
        let _ = std::fs::remove_file(change_disc_path);
    }

    let recent_file = recent_file_path(sdcard);
    let mut has_any = !recents.is_empty();

    if let Ok(content) = std::fs::read_to_string(&recent_file) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let (path, alias) = if let Some(tab_pos) = line.find('\t') {
                (&line[..tab_pos], Some(&line[tab_pos + 1..]))
            } else { (line, None) };

            let sd_path = format!("{}/{}", sdcard, path);
            if !path_exists(&sd_path) { continue; }
            if recents.len() >= MAX_RECENTS { break; }

            if let Some(_m3u) = find_m3u(&sd_path) {
                let disc_parent = match parent_dir(&sd_path) {
                    Some(p) => p.to_string(), None => continue,
                };
                if parent_paths.iter().any(|p| prefix_match(p, &disc_parent)) { continue; }
                parent_paths.push(disc_parent);
            }

            let emu_name = get_emu_name(&sd_path, &roms_path(sdcard));
            let available = has_emu(&emu_name, sdcard, platform_tag, paks);
            if available { has_any = true; }
            recents.push(Recent { path: path.to_string(), alias: alias.map(String::from), available });
        }
    }
    (recents, has_any)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn setup_test_dirs(name: &str) -> String {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = format!("/tmp/minui_scan_test_{}_{}", name, id);
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(format!("{}/Roms/Game Boy (GB)", base)).unwrap();
        fs::create_dir_all(format!("{}/Roms/Game Boy Color (GBC)", base)).unwrap();
        fs::create_dir_all(format!("{}/Roms/Super Nintendo (SFC)", base)).unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus", base)).unwrap();
        fs::create_dir_all(format!("{}/Emus/test", base)).unwrap();
        fs::create_dir_all(format!("{}/Tools/test", base)).unwrap();
        fs::create_dir_all(format!("{}/.userdata/shared/.minui", base)).unwrap();

        fs::write(format!("{}/Roms/Game Boy (GB)/Zelda.gb", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/Game Boy (GB)/Mario (World).gb", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/Game Boy Color (GBC)/Pokemon.gbc", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/Super Nintendo (SFC)/Zelda.sfc", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/Game Boy (GB)/.hidden.gb", base), "dummy").unwrap();

        fs::create_dir_all(format!("{}/.system/test/paks/Emus/GB.pak", base)).unwrap();
        fs::write(format!("{}/.system/test/paks/Emus/GB.pak/launch.sh", base), "#!/bin/sh\necho ok").unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus/GBC.pak", base)).unwrap();
        fs::write(format!("{}/.system/test/paks/Emus/GBC.pak/launch.sh", base), "#!/bin/sh\necho ok").unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus/SFC.pak", base)).unwrap();
        fs::write(format!("{}/.system/test/paks/Emus/SFC.pak/launch.sh", base), "#!/bin/sh\necho ok").unwrap();
        fs::create_dir_all(format!("{}/Tools/test/Clock.pak", base)).unwrap();
        fs::write(format!("{}/Tools/test/Clock.pak/launch.sh", base), "#!/bin/sh\necho clock").unwrap();
        base
    }

    #[test]
    fn test_scan_dir_basic() {
        let base = setup_test_dirs("unnamed");
        let gb_path = format!("{}/Roms/Game Boy (GB)", base);
        let entries = scan_dir(&gb_path, false);
        assert_eq!(entries.len(), 2);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.iter().any(|n| n.contains("Zelda")));
        assert!(names.iter().any(|n| n.contains("Mario")));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_get_root() {
        let base = setup_test_dirs("unnamed");
        let paks = paks_path(&base, "test");
        let recent_file = recent_file_path(&base);
        fs::create_dir_all(Path::new(&recent_file).parent().unwrap()).unwrap();
        fs::write(&recent_file, "").unwrap();
        let root = get_root(&base, "test", &paks, false, false, false);
        assert!(root.len() >= 4);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_get_root_simple_mode() {
        let base = setup_test_dirs("unnamed");
        let paks = paks_path(&base, "test");
        let root = get_root(&base, "test", &paks, false, false, true);
        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.iter().any(|n| n.contains("Tools")));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_find_m3u() {
        let base = setup_test_dirs("unnamed");
        fs::create_dir_all(format!("{}/Roms/PS/Final Fantasy VII", base)).unwrap();
        fs::write(format!("{}/Roms/PS/Final Fantasy VII/Disc 1.cue", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/PS/Final Fantasy VII.m3u", base), "Final Fantasy VII/Disc 1.cue\n").unwrap();
        let disc_path = format!("{}/Roms/PS/Final Fantasy VII/Disc 1.cue", base);
        let m3u = find_m3u(&disc_path);
        assert!(m3u.is_some());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_get_discs() {
        let base = setup_test_dirs("unnamed");
        fs::create_dir_all(format!("{}/Roms/PS/Final Fantasy VII", base)).unwrap();
        fs::write(format!("{}/Roms/PS/Final Fantasy VII/Disc 1.cue", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/PS/Final Fantasy VII/Disc 2.cue", base), "dummy").unwrap();
        fs::write(format!("{}/Roms/PS/Final Fantasy VII.m3u", base),
            "Final Fantasy VII/Disc 1.cue\nFinal Fantasy VII/Disc 2.cue\n").unwrap();
        let m3u_path = format!("{}/Roms/PS/Final Fantasy VII.m3u", base);
        let discs = get_discs(&m3u_path);
        assert_eq!(discs.len(), 2);
        assert_eq!(discs[0].name, "Disc 1");
        assert_eq!(discs[1].name, "Disc 2");
        let _ = fs::remove_dir_all(&base);
    }
}
