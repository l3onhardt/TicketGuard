//! 文本小工具：与 Python 字符串方法逐字符等价的实现。
//!
//! 参考实现大量使用 `str.splitlines()`，它识别的换行符比 Rust 的 `str::lines()` 多
//! （换页符、垂直制表符、U+2028 等）。典票正文是从 Word 转出来的，这些字符确实会出现，
//! 所以这里单独实现一份，保证两边切出来的行完全一致。

/// 与 Python `str.splitlines()` 等价（不保留换行符）。
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let is_break = matches!(
            ch,
            '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{1C}' | '\u{1D}' | '\u{1E}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        lines.push(&text[start..index]);
        let mut next = index + ch.len_utf8();
        // “\r\n” 算一个换行
        if ch == '\r' {
            if let Some(&(following, '\n')) = chars.peek() {
                chars.next();
                next = following + 1;
            }
        }
        start = next;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// 与 Python `str.splitlines(keepends=True)` 等价（保留换行符）。
pub fn splitlines_keepends(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let is_break = matches!(
            ch,
            '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{1C}' | '\u{1D}' | '\u{1E}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        let mut next = index + ch.len_utf8();
        if ch == '\r' {
            if let Some(&(following, '\n')) = chars.peek() {
                chars.next();
                next = following + 1;
            }
        }
        lines.push(&text[start..next]);
        start = next;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_splitlines() {
        assert_eq!(splitlines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n"), vec!["a"]);
        assert_eq!(splitlines("a\n\nb"), vec!["a", "", "b"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(splitlines("甲\u{0B}乙\u{2028}丙"), vec!["甲", "乙", "丙"]);
    }

    #[test]
    fn keepends_round_trips() {
        let text = "甲\r\n乙\n丙\u{0C}丁";
        assert_eq!(splitlines_keepends(text).concat(), text);
        assert_eq!(splitlines_keepends(text), vec!["甲\r\n", "乙\n", "丙\u{0C}", "丁"]);
    }
}
