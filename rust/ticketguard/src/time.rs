//! 时间小工具：ISO 8601 → 秒，以及与 Python `:g` 等价的小时数格式。
//!
//! 只用整数运算，不依赖系统时区和第三方库。

/// 把 `YYYY-MM-DDTHH:MM` 解析成自 1970-01-01 起的秒数。格式不对返回 None。
pub fn parse_iso(text: &str) -> Option<i64> {
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60)
}

/// 公历日期 → 自 1970-01-01 起的天数（Howard Hinnant 的算法）。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// 与 Python `f"{hours:g}"` 等价：整数不带小数点，否则最多 6 位有效数字、去掉末尾的 0。
pub fn format_hours(hours: f64) -> String {
    if hours == hours.trunc() && hours.abs() < 1e15 {
        return format!("{}", hours as i64);
    }
    let text = format!("{hours:.6}");
    let digits = hours.abs().log10().floor() as i32 + 1;
    let decimals = (6 - digits).clamp(0, 6) as usize;
    let text = if decimals < 6 { format!("{hours:.decimals$}") } else { text };
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_difference_is_exact() {
        let start = parse_iso("2024-04-01T08:30").unwrap();
        let end = parse_iso("2024-04-02T08:30").unwrap();
        assert_eq!(end - start, 86_400);
    }

    #[test]
    fn leap_day_counts() {
        let start = parse_iso("2024-02-28T00:00").unwrap();
        let end = parse_iso("2024-03-01T00:00").unwrap();
        assert_eq!(end - start, 2 * 86_400);
    }

    #[test]
    fn hours_format_matches_python_g() {
        assert_eq!(format_hours(24.0), "24");
        assert_eq!(format_hours(25.5), "25.5");
        assert_eq!(format_hours(120.25), "120.25");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_iso("2024-13-01T00:00"), None);
        assert_eq!(parse_iso("not a date"), None);
    }
}
