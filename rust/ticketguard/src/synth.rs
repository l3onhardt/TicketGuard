//! 合成样本变异算子。移植自参考实现 `src/ticketguard/synth.py`。
//!
//! 变异算子是确定性（或由种子驱动）的，同一个场景每种算子最多产出 `per` 张不同的票。

use crate::ingest::scenarios::ScenarioChunk;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Map, Value};

const DT: &str = r"\d{4}\s*年\s*\d{1,2}\s*月\s*\d{1,2}\s*日\s*\d{1,2}\s*时\s*\d{1,2}\s*分";

static RE_DT: Lazy<Regex> = Lazy::new(|| Regex::new(DT).unwrap());
static RE_PLAN: Lazy<Regex> = Lazy::new(|| Regex::new(&format!(r"(?s)自\s*({DT})(.*?)至\s*({DT})")).unwrap());
static RE_LOCATION_STATION: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+kV[^，、；。|]*?站").unwrap());
static RE_TOTAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"(总人数共\s*)(\d+)").unwrap());
static RE_LINE_NAMES: Lazy<Regex> = Lazy::new(|| Regex::new(r"10kV([^\s，,、；;。:：/kV“”（）()|]{1,6}?线)").unwrap());
static RE_SIGNER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(工作票接收人签名：[\s　]*)[^\s　/（(]+").unwrap());
static RE_EMPTY_CELL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\s　/]*无[。.]?[\s　]*$").unwrap());

const CATEGORIES: [(&str, &str); 5] = [
    ("应拉", "应拉开关刀闸熔断器"),
    ("应投切", "应投切电源及二次回路"),
    ("接地", "接地"),
    ("标志牌", "标志牌"),
    ("保留或邻近", "保留带电设备"),
];

const PARAPHRASE_PAIRS: [(&str, &str); 4] =
    [("合闸压板", "合闸连接片"), ("令克", "跌落式熔断器"), ("刀闸", "隔离开关"), ("空开", "空气开关")];

pub type Mutation = Option<(String, Value)>;

/// 一次只做一处改动，并返回该样本期望被检出的规则。
pub type Operator = fn(&str, &mut Rng) -> Mutation;

/// 小型确定性随机数生成器（SplitMix64）。合成样本只需要“同一个种子结果相同”，
/// 不需要密码学强度，也不值得为它引入第三方依赖。
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 返回 0..n 之间的一个下标。
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// 在某一行上做一次替换。找不到标签或替换没有生效就返回 None。
fn mutate_line(text: &str, label: &str, pattern: &Regex, replacement: &str, rule_id: &str) -> Mutation {
    let mut rows: Vec<String> = crate::text::splitlines_keepends(text).into_iter().map(str::to_string).collect();
    for row in rows.iter_mut() {
        if row.contains(label) {
            let changed = pattern.replace(row, replacement).to_string();
            if changed == *row {
                return None;
            }
            *row = changed;
            return Some((rows.concat(), json!([{ "rule_id": rule_id }])));
        }
    }
    None
}

/// 把“总人数共 N 人”写大 1 或 2。
pub fn crew_off(text: &str, rng: &mut Rng) -> Mutation {
    let delta = [1, 2][rng.below(2)];
    let mut rows: Vec<String> = crate::text::splitlines_keepends(text).into_iter().map(str::to_string).collect();
    for row in rows.iter_mut() {
        if row.contains("总人数共") {
            let changed = RE_TOTAL
                .replace(row, |caps: &regex::Captures<'_>| {
                    format!("{}{}", &caps[1], caps[2].parse::<i64>().unwrap() + delta)
                })
                .to_string();
            if changed == *row {
                return None;
            }
            *row = changed;
            return Some((rows.concat(), json!([{ "rule_id": "R-CREW-001" }])));
        }
    }
    None
}

/// 接收时间改成 2000 年，早于签发时间。
pub fn time_swap(text: &str, _rng: &mut Rng) -> Mutation {
    mutate_line(text, "工作票接收人签名：", &RE_DT, "2000年01月01日00时00分", "R-TIME-001")
}

/// 计划起止时间对调。等价于参考实现的 `re.sub(..., r"自\3\2至\1", text, count=1)`。
pub fn plan_swap(text: &str, _rng: &mut Rng) -> Mutation {
    let caps = RE_PLAN.captures(text)?;
    let whole = caps.get(0).expect("分组 0 一定存在");
    let changed = format!("{}自{}{}至{}{}", &text[..whole.start()], &caps[3], &caps[2], &caps[1], &text[whole.end()..]);
    if changed == text {
        return None;
    }
    Some((changed, json!([{ "rule_id": "R-TIME-002" }])))
}

/// 停电范围改成虚构的站和线。
pub fn outage_wrong(text: &str, _rng: &mut Rng) -> Mutation {
    mutate_line(
        text,
        "停电设备或线路名称：",
        &Regex::new(r"(停电设备或线路名称：)[^\r\n]*").unwrap(),
        "${1}35kV虚构站10kV虚构线",
        "R-LOC-002",
    )
}

/// 工作地点删掉站名。
pub fn loc_strip(text: &str, _rng: &mut Rng) -> Mutation {
    let mut rows: Vec<String> = crate::text::splitlines_keepends(text).into_iter().map(str::to_string).collect();
    let index = rows.iter().position(|row| row.starts_with("工作地点或设备（注明"))?;
    let next = rows.get(index + 1)?.clone();
    if !next.contains('|') {
        return None;
    }
    let (location, task) = next.split_once('|')?;
    let changed = RE_LOCATION_STATION.replace_all(location, "").to_string();
    if changed == location {
        return None;
    }
    rows[index + 1] = format!("{changed}|{task}");
    Some((rows.concat(), json!([{ "rule_id": "R-LOC-001" }])))
}

/// 删掉接收人签名。
pub fn drop_signer(text: &str, _rng: &mut Rng) -> Mutation {
    let mut rows: Vec<String> = crate::text::splitlines_keepends(text).into_iter().map(str::to_string).collect();
    for row in rows.iter_mut() {
        if row.contains("工作票接收人签名：") {
            let changed = RE_SIGNER.replace(row, "${1}/").to_string();
            if changed == *row {
                return None;
            }
            *row = changed;
            return Some((rows.concat(), json!([{ "rule_id": "R-REQ-002" }])));
        }
    }
    None
}

/// 把某一栏安全措施整栏写成“无”。
pub fn drop_category(text: &str, rng: &mut Rng) -> Mutation {
    let mut rows: Vec<String> = crate::text::splitlines_keepends(text).into_iter().map(str::to_string).collect();
    let start = rows.iter().position(|row| row.starts_with("调度或设备运维单位负责的安全措施"))?;
    let mut candidates: Vec<(usize, String, String)> = Vec::new();
    for (index, row) in rows.iter().enumerate().skip(start).take(5) {
        let Some((prefix, rest)) = row.split_once('|') else { continue };
        let Some((label, content)) = rest.split_once('：') else { continue };
        if RE_EMPTY_CELL.is_match(content) {
            continue;
        }
        // 与参考实现一致：措施栏的标签认不出来属于票面版式问题，直接暴露，不悄悄跳过
        let (_, category) = CATEGORIES
            .iter()
            .find(|(key, _)| label.contains(key))
            .unwrap_or_else(|| panic!("安全措施栏目无法识别：{label}"));
        candidates.push((index, format!("{prefix}|{label}：无"), category.to_string()));
    }
    if candidates.is_empty() {
        return None;
    }
    let (index, changed, category) = candidates[rng.below(candidates.len())].clone();
    let ending = rows[index].len() - rows[index].trim_end_matches(['\r', '\n']).len();
    let row = rows[index].clone();
    rows[index] = format!("{changed}{}", &row[row.len() - ending..]);
    Some((rows.concat(), json!([{ "rule_id": "CMP-MISSING", "category": category }])))
}

/// 换一种说法写同一条措施（**合格票**，用来测误报）。
pub fn paraphrase(text: &str, rng: &mut Rng) -> Mutation {
    let choices: Vec<(&str, &str)> = PARAPHRASE_PAIRS.iter().copied().filter(|(old, _)| text.contains(old)).collect();
    if choices.is_empty() {
        return None;
    }
    let (old, new) = choices[rng.below(choices.len())];
    Some((text.replace(old, new), json!([])))
}

/// 把所有线路名换成虚构名（**合格票**，用来测误报）。
pub fn rename_lines(text: &str, _rng: &mut Rng) -> Mutation {
    let mut names: Vec<String> = RE_LINE_NAMES.captures_iter(text).map(|caps| caps[1].to_string()).collect();
    if names.is_empty() {
        return None;
    }
    names.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then_with(|| a.cmp(b)));
    names.dedup();
    let mut out = text.to_string();
    for (index, name) in names.iter().enumerate() {
        out = out.replace(name.as_str(), &format!("虚构{}号线", index + 1));
    }
    Some((out, json!([])))
}

/// 算子表。名字必须与参考实现一致（评测报告按算子名分组）。
pub const OPERATORS: [(&str, Operator); 9] = [
    ("crew_off", crew_off),
    ("time_swap", time_swap),
    ("plan_swap", plan_swap),
    ("outage_wrong", outage_wrong),
    ("loc_strip", loc_strip),
    ("drop_signer", drop_signer),
    ("drop_category", drop_category),
    ("paraphrase", paraphrase),
    ("rename_lines", rename_lines),
];

/// 生成合成样本。每行一个 JSON，字段与参考实现一致。
pub fn generate(chunks: &[ScenarioChunk], per: usize, seed: u64) -> Vec<Value> {
    let mut rng = Rng::new(seed);
    let mut samples = Vec::new();
    for chunk in chunks {
        let base = format!("D1-S{:02}", chunk.number.parse::<i64>().unwrap_or(0));
        for (name, operator) in OPERATORS {
            let mut seen: Vec<String> = Vec::new();
            for _ in 0..per {
                let Some((text, expected)) = operator(&chunk.text, &mut rng) else { break };
                if seen.contains(&text) {
                    continue;
                }
                let mut sample = Map::new();
                sample.insert("id".to_string(), json!(format!("syn-{base}-{name}-{}", seen.len())));
                sample.insert("base_scenario".to_string(), json!(base));
                sample.insert("mutations".to_string(), json!([{ "op": name }]));
                sample.insert("ticket_text".to_string(), json!(text.clone()));
                sample.insert("expected_findings".to_string(), expected);
                samples.push(Value::Object(sample));
                seen.push(text);
            }
        }
    }
    samples
}
