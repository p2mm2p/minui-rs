//! 时间字段编辑与回卷校验（纯逻辑）
//!
//! 本模块提供 clock 的日期时间字段编辑与回卷校验纯函数：
//! `adjust`（字段增减 + 光标移动）与 `validate`（越界回卷）。
//! 无副作用、无平台依赖，可独立单元测试。
//!
//! 对应原版 C `clock.c` 的 `validate()`（:98-138）与按键 switch
//! （:150-235）。Rust 版将内联在 main 里的逻辑提升为模块级纯函数，
//! 使闰年/回卷/AMPM 等边界可被测试覆盖。
//!

/// 可编辑字段枚举（对应 C `clock.c:12-20` 的 `CURSOR_*`）
///
/// 顺序即光标循环顺序：年 → 月 → 日 → 时 → 分 → 秒 → AMPM（12 小时制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// 年（钳制 1970..2100）
    Year,
    /// 月（回卷 1..12）
    Month,
    /// 日（按当月天数回卷，含闰年）
    Day,
    /// 时（回卷 0..23；12 小时制显示时映射 1..12）
    Hour,
    /// 分（回卷 0..59）
    Minute,
    /// 秒（回卷 0..59）
    Second,
    /// AM/PM（仅 12 小时制；切换使小时 ±12）
    Ampm,
}

/// 当前可编辑字段数（24 小时制 6 个，12 小时制 7 个含 AMPM）
///
/// # 参数
///
/// - `show_24hour`：是否 24 小时制
///
/// # 返回值
///
/// 字段总数（6 或 7）
pub fn option_count(show_24hour: bool) -> u32 {
    if show_24hour { 6 } else { 7 }
}

/// 将光标索引转换为字段枚举
///
/// # 参数
///
/// - `cursor`：光标索引（0..option_count）
///
/// # 返回值
///
/// 对应字段（`cursor` 越界时返回 `Field::Year` 兜底）
pub fn field_at(cursor: u32) -> Field {
    match cursor {
        0 => Field::Year,
        1 => Field::Month,
        2 => Field::Day,
        3 => Field::Hour,
        4 => Field::Minute,
        5 => Field::Second,
        _ => Field::Ampm,
    }
}

/// 字段增减（UP/DOWN）与光标移动（LEFT/RIGHT）
///
/// `delta`：+1（UP）/ -1（DOWN）。对 AMPM 字段切换使小时 ±12；
/// 对其他字段直接 ±1（越界由 `validate` 回卷）。
///
/// # 参数
///
/// - `field`：当前编辑字段
/// - `delta`：增减量（+1 或 -1）
/// - `values`：当前 6 个字段值 `(年, 月, 日, 时, 分, 秒)`
///
/// # 返回值
///
/// 调整后的 6 个字段值
pub fn adjust(
    field: Field,
    delta: i32,
    values: (i32, i32, i32, i32, i32, i32),
) -> (i32, i32, i32, i32, i32, i32) {
    let (y, m, d, h, min, s) = values;
    match field {
        Field::Year => (y + delta, m, d, h, min, s),
        Field::Month => (y, m + delta, d, h, min, s),
        Field::Day => (y, m, d + delta, h, min, s),
        Field::Hour => (y, m, d, h + delta, min, s),
        Field::Minute => (y, m, d, h, min + delta, s),
        Field::Second => (y, m, d, h, min, s + delta),
        Field::Ampm => (y, m, d, h + delta * 12, min, s),
    }
}

/// 光标循环移动
///
/// `delta`：+1（RIGHT）/ -1（LEFT）。结果回卷到 `0..option_count`。
///
/// # 参数
///
/// - `cursor`：当前光标索引
/// - `delta`：移动量（+1 或 -1）
/// - `show_24hour`：是否 24 小时制（决定字段总数）
///
/// # 返回值
///
/// 移动后的光标索引（已回卷）
pub fn move_cursor(cursor: u32, delta: i32, show_24hour: bool) -> u32 {
    let count = option_count(show_24hour);
    let c = cursor as i32 + delta;
    (((c % count as i32) + count as i32) % count as i32) as u32
}

/// 判断是否为闰年
///
/// 对应原版 `clock.c:100-101` 的闰年判断（4 整除、100 除外、400 再入）。
///
/// # 参数
///
/// - `year`：年份（如 2024）
///
/// # 返回值
///
/// 是否为闰年
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 获取指定月份的天数
///
/// # 参数
///
/// - `year`：年份（闰年判断用）
/// - `month`：月份（1..12）
///
/// # 返回值
///
/// 当月天数（28/29/30/31）
pub fn days_in_month(year: i32, month: i32) -> u32 {
    match month {
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 越界回卷校验
///
/// 对应原版 `clock.c` 的 `validate()`（:98-138）：
/// - 月回卷到 1..12（超 12 减 12，小于 1 加 12）
/// - 日按当月天数回卷（超天数减天数，小于 1 加天数；2 月按闰年）
/// - 时回卷到 0..23、分/秒回卷到 0..59（超 59 减 60，小于 0 加 60）
/// - 年钳制在 1970..2100
///
/// # 参数
///
/// - `values`：6 个字段值 `(年, 月, 日, 时, 分, 秒)`
///
/// # 返回值
///
/// 回卷后的 6 个字段值
pub fn validate(values: (i32, i32, i32, i32, i32, i32)) -> (i32, i32, i32, i32, i32, i32) {
    let (mut y, mut m, mut d, mut h, mut min, mut s) = values;

    // 年钳制（C :108-109——clamp 等价于 if/else 钳制）
    y = y.clamp(1970, 2100);

    // 月回卷（C :105-106）
    if m > 12 {
        m -= 12;
    } else if m < 1 {
        m += 12;
    }

    // 日按当月天数回卷（C :111-129）
    let dim = days_in_month(y, m) as i32;
    if d > dim {
        d -= dim;
    } else if d < 1 {
        d += dim;
    }

    // 时回卷（C :132-133）
    if h > 23 {
        h -= 24;
    } else if h < 0 {
        h += 24;
    }

    // 分/秒回卷（C :134-137）
    if min > 59 {
        min -= 60;
    } else if min < 0 {
        min += 60;
    }
    if s > 59 {
        s -= 60;
    } else if s < 0 {
        s += 60;
    }

    (y, m, d, h, min, s)
}

/// 12 小时制显示转换
///
/// 内部小时 0..23 → 显示小时 1..12：
/// - 0 → 12
/// - 13..23 → 减 12（13 → 1）
/// - 1..12 → 不变
///
/// # 参数
///
/// - `hour_24`：内部小时（0..23）
///
/// # 返回值
///
/// 12 小时制显示值（1..12）
pub fn display_hour(hour_24: i32) -> i32 {
    match hour_24 {
        0 => 12,
        13..=23 => hour_24 - 12,
        _ => hour_24,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate：闰年 / 月天数 / 回卷边界 ──

    #[test]
    fn leap_year_2024_feb_has_29_days() {
        assert!(is_leap_year(2024));
        assert_eq!(days_in_month(2024, 2), 29);
    }

    #[test]
    fn non_leap_year_2023_feb_has_28_days() {
        assert!(!is_leap_year(2023));
        assert_eq!(days_in_month(2023, 2), 28);
    }

    #[test]
    fn century_year_1900_not_leap_2000_leap() {
        assert!(!is_leap_year(1900), "整百非 400 倍数不是闰年");
        assert!(is_leap_year(2000), "400 倍数是闰年");
    }

    #[test]
    fn month_wraps_above_12() {
        // 12 月按 UP → 13 → 回卷为 1（对应 spec「月越界回卷」）
        assert_eq!(validate((2024, 13, 1, 0, 0, 0)).1, 1);
    }

    #[test]
    fn month_wraps_below_1() {
        // 1 月按 DOWN → 0 → 回卷为 12
        assert_eq!(validate((2024, 0, 1, 0, 0, 0)).1, 12);
    }

    #[test]
    fn day_wraps_by_leap_feb() {
        // 2024-02-29 按 UP → 30 → 回卷为 1（2 月有 29 天，spec「日按闰年回卷」）
        assert_eq!(validate((2024, 2, 30, 0, 0, 0)).2, 1);
    }

    #[test]
    fn day_wraps_by_non_leap_feb() {
        // 2023-02-28 按 UP → 29 → 回卷为 1（2023 非闰年，2 月只有 28 天，
        // spec「非闰年 2 月回卷」）
        assert_eq!(validate((2023, 2, 29, 0, 0, 0)).2, 1);
    }

    #[test]
    fn day_wraps_by_30_day_month() {
        // 4 月 30 日按 UP → 31 → 回卷为 1（4 月只有 30 天）
        assert_eq!(validate((2024, 4, 31, 0, 0, 0)).2, 1);
    }

    #[test]
    fn day_wraps_by_31_day_month() {
        // 1 月 31 日按 UP → 32 → 回卷为 1
        assert_eq!(validate((2024, 1, 32, 0, 0, 0)).2, 1);
    }

    #[test]
    fn day_wraps_below_1() {
        // 1 日按 DOWN → 0 → 回卷为当月天数（1 月 = 31）
        assert_eq!(validate((2024, 1, 0, 0, 0, 0)).2, 31);
    }

    #[test]
    fn hour_wraps_above_23() {
        // 23 时按 UP → 24 → 回卷为 0
        assert_eq!(validate((2024, 1, 1, 24, 0, 0)).3, 0);
    }

    #[test]
    fn hour_wraps_below_0() {
        // 0 时按 DOWN → -1 → 回卷为 23
        assert_eq!(validate((2024, 1, 1, -1, 0, 0)).3, 23);
    }

    #[test]
    fn minute_wraps_above_59() {
        assert_eq!(validate((2024, 1, 1, 0, 60, 0)).4, 0);
    }

    #[test]
    fn second_wraps_below_0() {
        assert_eq!(validate((2024, 1, 1, 0, 0, -1)).5, 59);
    }

    #[test]
    fn year_clamps_above_2100() {
        assert_eq!(validate((2101, 1, 1, 0, 0, 0)).0, 2100);
    }

    #[test]
    fn year_clamps_below_1970() {
        assert_eq!(validate((1969, 1, 1, 0, 0, 0)).0, 1970);
    }

    // ── adjust：字段增减 + AMPM ±12 ──

    #[test]
    fn adjust_year_plus_one() {
        assert_eq!(adjust(Field::Year, 1, (2024, 1, 1, 0, 0, 0)).0, 2025);
    }

    #[test]
    fn adjust_hour_minus_one() {
        assert_eq!(adjust(Field::Hour, -1, (2024, 1, 1, 9, 0, 0)).3, 8);
    }

    #[test]
    fn adjust_ampm_toggles_am_to_pm() {
        // 9 AM 编辑 AMPM → +12 → 21（PM），spec「AMPM 切换使小时 ±12」
        assert_eq!(adjust(Field::Ampm, 1, (2024, 1, 1, 9, 0, 0)).3, 21);
    }

    #[test]
    fn adjust_ampm_toggles_pm_to_am() {
        // 21（PM）编辑 AMPM → -12 → 9（AM）
        assert_eq!(adjust(Field::Ampm, -1, (2024, 1, 1, 21, 0, 0)).3, 9);
    }

    // ── move_cursor：光标循环 ──

    #[test]
    fn cursor_wraps_from_last_to_first_24h() {
        // 24 小时制 6 字段：秒(5) 按 RIGHT → 0（年），spec「光标循环移动」
        assert_eq!(move_cursor(5, 1, true), 0);
    }

    #[test]
    fn cursor_wraps_from_first_to_last_24h() {
        assert_eq!(move_cursor(0, -1, true), 5);
    }

    #[test]
    fn cursor_wraps_from_last_to_first_12h() {
        // 12 小时制 7 字段：AMPM(6) 按 RIGHT → 0（年）
        assert_eq!(move_cursor(6, 1, false), 0);
    }

    #[test]
    fn cursor_wraps_ampm_removed_in_24h() {
        // 12 小时制 AMPM(6) 切到 24 小时制后字段数变 6，光标钳制
        assert_eq!(option_count(true), 6);
        assert_eq!(option_count(false), 7);
    }

    #[test]
    fn field_at_maps_indices() {
        assert_eq!(field_at(0), Field::Year);
        assert_eq!(field_at(3), Field::Hour);
        assert_eq!(field_at(6), Field::Ampm);
    }

    // ── display_hour：12 小时制显示转换 ──

    #[test]
    fn display_hour_zero_becomes_twelve() {
        // 0 时显示为 12，spec「12 小时制小时显示转换」
        assert_eq!(display_hour(0), 12);
    }

    #[test]
    fn display_hour_13_becomes_1() {
        assert_eq!(display_hour(13), 1);
    }

    #[test]
    fn display_hour_9_stays_9() {
        assert_eq!(display_hour(9), 9);
    }

    #[test]
    fn display_hour_23_becomes_11() {
        assert_eq!(display_hour(23), 11);
    }

    #[test]
    fn display_hour_12_stays_12() {
        assert_eq!(display_hour(12), 12);
    }
}
