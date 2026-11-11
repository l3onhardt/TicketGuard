//! 票面解析器（配电第一种工作票）。移植自参考实现 `src/ticketguard/ingest/ticket_parser.py`。
//!
//! 参考实现里的 `next(...)` / `[0]` 在字段缺失时会抛异常；这里保持同样立场：
//! 缺栏目直接返回错误，不静默跳过。

use crate::schema::{Person, SafetyMeasure, Ticket, TicketType};
use once_cell::sync::Lazy;
use regex::Regex;
use std::fmt;

#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

const REQUIRED: [&str; 5] = [
    "工作负责人（监护人）：",
    "工作地点或设备（注明",
    "停电设备或线路名称：",
    "调度或设备运维单位负责的安全措施",
    "工作班组自行装设的接地线",
];

const CATEGORIES: [(&str, &str); 5] = [
    ("应拉", "应拉开关刀闸熔断器"),
    ("应投切", "应投切电源及二次回路"),
    ("接地", "接地"),
    ("标志牌", "标志牌"),
    ("保留或邻近", "保留带电设备"),
];

/// 签名栏：标签、角色名、对应的时间字段名。
const SIGNATURES: [(&str, &str, &str); 4] = [
    ("工作票签发人签名：", "签发人", "签发时间"),
    ("工作票会签人签名：", "会签人", "会签时间"),
    ("工作票接收人签名：", "接收人", "接收时间"),
    ("工作许可人签名：", "许可人", "许可时间"),
];

static RE_DATETIME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d{4})\s*年\s*(\d{1,2})\s*月\s*(\d{1,2})\s*日\s*(\d{1,2})\s*时\s*(\d{1,2})\s*分").unwrap()
});
static RE_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?:\d+[、.．]|（\d+）|\(\d+\))").unwrap());
static RE_CREW: Lazy<Regex> = Lazy::new(|| Regex::new(r"总人数共\s*(\d+)\s*人").unwrap());
static RE_DISPATCH: Lazy<Regex> = Lazy::new(|| Regex::new(r"[☑√]\s*(是|否)").unwrap());
static RE_MEMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^([^（(]+)[（(]([^）)]*)[）)]$").unwrap());
static RE_SIGNED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*([^\s/（(：:]+)(?:\s*[（(]([^）)]*?)代签[）)])?").unwrap());
static RE_MONITOR: Lazy<Regex> = Lazy::new(|| Regex::new(r"指定[\s/]*([^\s/]+?)[\s/]*为专责监护人").unwrap());

/// “  ” 与普通空格都算空白，与参考实现的 `_SPACE` 一致。
fn is_space(ch: char) -> bool {
    ch == ' ' || ch == '\u{3000}'
}

fn trim_spaces(text: &str) -> &str {
    text.trim_matches(is_space)
}

fn lstrip_spaces(text: &str) -> &str {
    text.trim_start_matches(is_space)
}

/// 取某一行里标签之后的部分；没有这一行时返回空串（与参考实现的 `_after` 一致）。
fn after<'a>(lines: &[&'a str], label: &str) -> &'a str {
    lines.iter().find_map(|line| after_in(line, label)).unwrap_or("")
}

/// 同上，但只看一行。
fn after_in<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    line.find(label).map(|index| &line[index + label.len()..])
}

/// 日期时间 → ISO 8601 字符串。解析不出来返回 None。
pub fn parse_datetime(text: &str) -> Option<String> {
    let caps = RE_DATETIME.captures(text)?;
    let year: i32 = caps[1].parse().ok()?;
    let month: u32 = caps[2].parse().ok()?;
    let day: u32 = caps[3].parse().ok()?;
    let hour: u32 = caps[4].parse().ok()?;
    let minute: u32 = caps[5].parse().ok()?;
    Some(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}"))
}

/// 从票面文本里切出票面块（“编号：”行到表格结束）。
pub fn ticket_block(text: &str) -> Result<String, ParseError> {
    let lines: Vec<&str> = crate::text::splitlines(text);
    let start = lines
        .iter()
        .position(|line| lstrip_spaces(line).starts_with("编号："))
        .ok_or_else(|| ParseError("票面缺少编号：行".to_string()))?;
    let table = lines[start + 1..]
        .iter()
        .position(|line| trim_spaces(line) == "[TABLE]")
        .map(|offset| start + 1 + offset)
        .ok_or_else(|| ParseError("编号：行后缺少 [TABLE]".to_string()))?;
    let end = lines[table + 1..]
        .iter()
        .position(|line| trim_spaces(line) == "[/TABLE]")
        .map(|offset| table + 1 + offset)
        .ok_or_else(|| ParseError("票面缺少 [/TABLE]".to_string()))?;
    Ok(lines[start..=end].join("\n"))
}

fn parse_header(ticket: &mut Ticket, lines: &[&str]) -> Result<(), ParseError> {
    let header = lines
        .iter()
        .find(|line| line.contains(REQUIRED[0]))
        .ok_or_else(|| ParseError(format!("票面缺少栏目：{}", REQUIRED[0])))?;
    let name = after_in(header, REQUIRED[0]).unwrap_or("").split('/').next().unwrap_or("").trim_matches(is_space);
    if !name.is_empty() {
        ticket.people.push(Person {
            name: name.to_string(),
            role: "工作负责人".to_string(),
            signed_by: None,
            note: None,
        });
    }
    let phone = after_in(header, "电话：").unwrap_or("").split('/').next().unwrap_or("").trim_matches(is_space);
    ticket.fields.insert(
        "电话".to_string(),
        if phone.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(phone.to_string()) },
    );
    let unit = after_in(header, "单位和班组：").unwrap_or("").split(" / ").next().unwrap_or("").trim_matches(is_space);
    ticket.fields.insert(
        "单位班组".to_string(),
        if unit.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(unit.to_string()) },
    );
    ticket.crew_size = RE_CREW.captures(header).and_then(|caps| caps[1].parse::<i64>().ok());
    ticket.plan_start = parse_datetime(after_in(header, "自").unwrap_or(""));
    ticket.plan_end = parse_datetime(after_in(header, "至").unwrap_or(""));
    let dispatch = RE_DISPATCH
        .captures(after(lines, "是否办理分组工作派工单"))
        .map(|caps| serde_json::Value::String(caps[1].to_string()))
        .unwrap_or(serde_json::Value::Null);
    ticket.fields.insert("分组派工单".to_string(), dispatch);

    let members = after(lines, "工作班人员（不包括工作负责人）：")
        .trim_matches(|ch| is_space(ch) || matches!(ch, '。' | '.' | '；' | ';'));
    for member in members.split(['、', '，', ',', '；', ';', '/', ' ', '\u{3000}']) {
        if member.is_empty() {
            continue;
        }
        let (name, note) = match RE_MEMBER.captures(member) {
            Some(caps) => (caps[1].to_string(), Some(caps[2].to_string())),
            None => (member.to_string(), None),
        };
        ticket.people.push(Person { name, role: "工作班成员".to_string(), signed_by: None, note });
    }
    Ok(())
}

fn parse_signatures(ticket: &mut Ticket, lines: &[&str]) {
    for (label, role, time_field) in SIGNATURES {
        let rest = after(lines, label);
        if let Some(caps) = RE_SIGNED.captures(rest) {
            let signed_by =
                caps.get(2).map(|m| m.as_str().chars().filter(|ch| !ch.is_whitespace()).collect::<String>());
            ticket.people.push(Person { name: caps[1].to_string(), role: role.to_string(), signed_by, note: None });
        }
        ticket.fields.insert(
            time_field.to_string(),
            match parse_datetime(rest) {
                Some(value) => serde_json::Value::String(value),
                None => serde_json::Value::Null,
            },
        );
    }
    if let Some(caps) = RE_MONITOR.captures(&ticket.raw_text) {
        ticket.people.push(Person {
            name: caps[1].to_string(),
            role: "专责监护人".to_string(),
            signed_by: None,
            note: None,
        });
    }
    let briefing = lines.iter().find(|line| line.starts_with("安全交代 |")).copied().unwrap_or("");
    ticket.fields.insert(
        "安全交代时间".to_string(),
        match parse_datetime(briefing) {
            Some(value) => serde_json::Value::String(value),
            None => serde_json::Value::Null,
        },
    );
}

fn parse_measures(ticket: &mut Ticket, lines: &[&str]) -> Result<(), ParseError> {
    let start = lines
        .iter()
        .position(|line| line.starts_with(REQUIRED[3]))
        .ok_or_else(|| ParseError(format!("票面缺少栏目：{}", REQUIRED[3])))?;
    let end = lines
        .iter()
        .position(|line| line.starts_with(REQUIRED[4]))
        .ok_or_else(|| ParseError(format!("票面缺少栏目：{}", REQUIRED[4])))?;
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for line in &lines[start..end] {
        // Python: line.partition("|")[2] —— 第一个竖线之后的全部内容
        let rest = line.split_once('|').map(|(_, rest)| rest).unwrap_or("");
        let (label, content) = match rest.split_once('：') {
            Some((label, content)) => (label, content),
            None => (rest, ""),
        };
        let category = CATEGORIES
            .iter()
            .find(|(key, _)| label.contains(key))
            .map(|(_, name)| *name)
            .ok_or_else(|| ParseError(format!("安全措施栏目无法识别：{label}")))?;
        for segment in content.split(" / ") {
            // Python: re.split(r"(?<=[；;。！!])(?={_NUMBER})", segment)
            for item in split_numbered(segment) {
                let item = RE_NUMBER
                    .find(item)
                    .map(|m| &item[m.end()..])
                    .unwrap_or(item)
                    .trim_matches(|ch| is_space(ch) || matches!(ch, '；' | ';' | '。' | '.'));
                if item.is_empty() || item == "无" {
                    continue;
                }
                let order = match counts.iter_mut().find(|(name, _)| *name == category) {
                    Some((_, count)) => {
                        *count += 1;
                        *count
                    }
                    None => {
                        counts.push((category, 1));
                        1
                    }
                };
                ticket.measures.push(SafetyMeasure {
                    category: category.to_string(),
                    text: item.to_string(),
                    order: Some(order),
                });
            }
        }
    }
    let last = lines.get(end).ok_or_else(|| ParseError(format!("票面缺少栏目：{}", REQUIRED[4])))?;
    for (order, cell) in last.split('|').skip(2).enumerate() {
        let value = cell.replace(" / ", "");
        let value = trim_spaces(&value);
        if value.is_empty() || value == "无" || value == "/" {
            continue;
        }
        ticket.measures.push(SafetyMeasure {
            category: "班组自装接地".to_string(),
            text: value.to_string(),
            order: Some(order + 1),
        });
    }
    Ok(())
}

/// 按“上一项以句号或分号结尾、下一项以序号开头”切分。
/// 等价于参考实现的 `re.split(r"(?<=[；;。！!])(?={_NUMBER})", segment)`。
fn split_numbered(segment: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (offset, ch) in segment.char_indices() {
        if !matches!(ch, '；' | ';' | '。' | '！' | '!') {
            continue;
        }
        let next = offset + ch.len_utf8();
        if next < segment.len() && RE_NUMBER.is_match(&segment[next..]) {
            parts.push(&segment[start..next]);
            start = next;
        }
    }
    parts.push(&segment[start..]);
    parts
}

fn parse_work(ticket: &mut Ticket, lines: &[&str]) -> Result<(), ParseError> {
    let start = lines
        .iter()
        .position(|line| line.starts_with(REQUIRED[1]))
        .ok_or_else(|| ParseError(format!("票面缺少栏目：{}", REQUIRED[1])))?;
    let end = lines
        .iter()
        .position(|line| line.contains(REQUIRED[2]))
        .ok_or_else(|| ParseError(format!("票面缺少栏目：{}", REQUIRED[2])))?;
    for line in &lines[start + 1..end] {
        let mut cells = line.split('|').map(trim_spaces);
        let place = cells.next().unwrap_or("").to_string();
        let task = cells.next().unwrap_or("").to_string();
        if place.is_empty() && task.is_empty() {
            continue;
        }
        ticket.work_tasks.push(crate::schema::WorkTask { place, task });
    }
    let scope = after(lines, REQUIRED[2]).trim_matches(|ch| is_space(ch) || matches!(ch, '/' | '。' | '.'));
    ticket.outage_scope = if scope.is_empty() { None } else { Some(scope.to_string()) };
    let remark = lines.iter().find(|line| line.starts_with("备注")).copied().unwrap_or("");
    let value =
        remark.split_once('：').map(|(_, rest)| rest).unwrap_or("").trim_matches(|ch| is_space(ch) || ch == '/');
    ticket.fields.insert(
        "备注".to_string(),
        if value.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(value.to_string()) },
    );
    Ok(())
}

/// 解析配电第一种工作票的票面文本。
pub fn parse_dist1(text: &str) -> Result<Ticket, ParseError> {
    let block = ticket_block(text)?;
    for label in REQUIRED {
        if !block.contains(label) {
            return Err(ParseError(format!("票面缺少栏目：{label}")));
        }
    }
    let lines: Vec<&str> = crate::text::splitlines(&block).into_iter().map(trim_spaces).collect();
    let number = lines[0].split_once("编号：").map(|(_, rest)| trim_spaces(rest)).unwrap_or("");
    let mut ticket = Ticket {
        ticket_type: TicketType::Dist1,
        number: if number.is_empty() { None } else { Some(number.to_string()) },
        raw_text: block.clone(),
        work_tasks: Vec::new(),
        outage_scope: None,
        plan_start: None,
        plan_end: None,
        people: Vec::new(),
        crew_size: None,
        measures: Vec::new(),
        fields: serde_json::Map::new(),
    };
    parse_header(&mut ticket, &lines)?;
    parse_signatures(&mut ticket, &lines);
    parse_work(&mut ticket, &lines)?;
    parse_measures(&mut ticket, &lines)?;
    Ok(ticket)
}
