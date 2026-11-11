//! 规则引擎。移植自参考实现 `src/ticketguard/rules/engine.py`。
//!
//! 规则 JSON 由业务人员维护，加载时做一次完整校验；运行时不检查类型。

use crate::entities::{lines, DataError};
use crate::regex::Pattern;
use crate::schema::{Basis, Finding, Severity, Ticket};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

const CHECKS: [&str; 8] = [
    "required_field",
    "role_required",
    "roles_disjoint",
    "before",
    "duration_max",
    "crew_count",
    "regex",
    "lines_subset",
];

/// 规则 JSON 里引擎要用到的字段。`title`、`severity` 只在校验时看，不在这里。
#[derive(Debug, Deserialize)]
struct RawRule {
    id: String,
    ticket_types: Vec<String>,
    check: String,
    params: Value,
    message: String,
    basis: Basis,
    status: String,
}

/// 一条校验通过、可以执行的规则。
#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub ticket_types: Vec<String>,
    pub check: String,
    pub params: Value,
    pub severity: Severity,
    pub message: String,
    pub basis: Basis,
    pub status: String,
    /// `regex` 算子的正则。其它算子为 None。
    pub pattern: Option<Pattern>,
}

fn violation_to_params(violation: &Value) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    if let Some(obj) = violation.as_object() {
        for (key, value) in obj {
            let text = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            out.insert(key.clone(), text);
        }
    }
    out
}

/// 规则可以引用的顶层字段。加载规则时用它校验路径，写错字段名在加载时就报错。
const TOP_FIELDS: [&str; 6] = ["number", "outage_scope", "plan_start", "plan_end", "crew_size", "raw_text"];
const TASK_FIELDS: [&str; 2] = ["地点", "任务"];
const PERSON_FIELDS: [&str; 4] = ["name", "role", "signed_by", "note"];

/// 路径是否合法：`fields.<任意>`、`work_tasks.<地点|任务>`、`people.<name|role|signed_by|note>` 或顶层字段。
fn path_is_known(path: &str) -> bool {
    match path.split_once('.') {
        Some(("fields", key)) => !key.is_empty(),
        Some(("work_tasks", key)) => TASK_FIELDS.contains(&key),
        Some(("people", key)) => PERSON_FIELDS.contains(&key),
        Some(_) => false,
        None => TOP_FIELDS.contains(&path),
    }
}

/// 检查一条规则的参数形状是否符合它的算子。返回 None 表示没问题，否则返回一句说明。
/// 加载规则时查一次；之后各算子直接取值，不再防御。
fn params_problem(check: &str, params: &Value) -> Option<String> {
    let is_strings = |value: &Value, min: usize| {
        value.as_array().map(|list| list.len() >= min && list.iter().all(Value::is_string)).unwrap_or(false)
    };
    let need_string = |key: &str| (!params[key].is_string()).then(|| format!("params.{key} 必须是字符串"));
    let need_strings = |key: &str, min: usize| {
        (!is_strings(&params[key], min)).then(|| format!("params.{key} 必须是字符串数组（至少 {min} 项）"))
    };
    let need_bool = |key: &str| (!params[key].is_boolean()).then(|| format!("params.{key} 必须是 true 或 false"));
    match check {
        "required_field" => {
            let valid = params["fields"]
                .as_object()
                .map(|fields| !fields.is_empty() && fields.values().all(Value::is_string))
                .unwrap_or(false);
            (!valid).then(|| "params.fields 必须是 {字段路径: 中文栏目名} 的对象，且不能为空".to_string())
        }
        "role_required" => need_strings("roles", 1),
        "roles_disjoint" => {
            let exactly_two = params["roles"].as_array().map(|roles| roles.len() == 2).unwrap_or(false);
            need_strings("roles", 2).or((!exactly_two).then(|| "params.roles 必须恰好是两个角色".to_string()))
        }
        "before" => need_strings("chain", 2).or(need_bool("strict")),
        "duration_max" => need_string("start")
            .or(need_string("end"))
            .or((!params["hours"].is_number()).then(|| "params.hours 必须是数字".to_string())),
        "regex" => need_string("field").or(need_string("pattern")).or(need_bool("must_match")),
        "lines_subset" => need_string("field").or(need_strings("within", 1)),
        _ => None,
    }
}

/// 一条规则的参数里出现的全部字段路径。
fn paths_in(check: &str, params: &Value) -> Vec<String> {
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };
    match check {
        "required_field" => params["fields"].as_object().map(|map| map.keys().cloned().collect()).unwrap_or_default(),
        "before" => strings(&params["chain"]),
        "duration_max" => {
            [&params["start"], &params["end"]].iter().filter_map(|v| v.as_str()).map(str::to_string).collect()
        }
        "regex" => params["field"].as_str().map(|path| vec![path.to_string()]).unwrap_or_default(),
        "lines_subset" => {
            let mut paths = strings(&params["within"]);
            paths.extend(params["field"].as_str().map(str::to_string));
            paths
        }
        _ => Vec::new(),
    }
}

/// 取字段路径的值。与参考实现 `values()` 一致：None 和空字符串都丢掉。
pub fn values(ticket: &Ticket, path: &str) -> Vec<Value> {
    let (prefix, key) = path.split_once('.').unwrap_or((path, ""));
    let found: Vec<Option<Value>> = match prefix {
        "fields" => vec![ticket.fields.get(key).cloned()],
        "work_tasks" => ticket
            .work_tasks
            .iter()
            .map(|task| match key {
                "地点" => Some(Value::String(task.place.clone())),
                "任务" => Some(Value::String(task.task.clone())),
                _ => None,
            })
            .collect(),
        "people" => ticket
            .people
            .iter()
            .map(|person| {
                match key {
                    "name" => Some(person.name.clone()),
                    "role" => Some(person.role.clone()),
                    "signed_by" => person.signed_by.clone(),
                    "note" => person.note.clone(),
                    _ => None,
                }
                .map(Value::String)
            })
            .collect(),
        // 顶层字段：与参考实现的 `getattr(ticket, path)` 对应。路径在加载规则时已校验。
        _ => vec![match path {
            "number" => ticket.number.clone().map(Value::String),
            "outage_scope" => ticket.outage_scope.clone().map(Value::String),
            "plan_start" => ticket.plan_start.clone().map(Value::String),
            "plan_end" => ticket.plan_end.clone().map(Value::String),
            "crew_size" => ticket.crew_size.map(|size| Value::Number(size.into())),
            "raw_text" => Some(Value::String(ticket.raw_text.clone())),
            _ => None,
        }],
    };
    found.into_iter().flatten().filter(|value| !value.is_null() && value.as_str() != Some("")).collect()
}

/// 取字符串值。数字（如 crew_size）转成十进制文本，这样“有没有填”的判断与参考实现一致。
fn string_values(ticket: &Ticket, path: &str) -> Vec<String> {
    values(ticket, path)
        .into_iter()
        .map(|value| match value {
            Value::String(text) => text,
            other => other.to_string(),
        })
        .collect()
}

/// 取字符串参数。形状已在加载规则时校验过（见 `params_problem`）。
fn text<'a>(params: &'a Value, key: &str) -> &'a str {
    params[key].as_str().expect("参数形状已在加载规则时校验")
}

/// 取字符串数组参数。形状已在加载规则时校验过。
fn texts<'a>(params: &'a Value, key: &str) -> Vec<&'a str> {
    params[key].as_array().expect("参数形状已在加载规则时校验").iter().filter_map(Value::as_str).collect()
}

fn check_required_field(ticket: &Ticket, params: &Value) -> Vec<Value> {
    let fields = params["fields"].as_object().expect("参数形状已在加载规则时校验");
    fields
        .iter()
        .filter(|(path, _)| string_values(ticket, path).is_empty())
        .map(|(_, label)| serde_json::json!({ "field": label }))
        .collect()
}

fn check_role_required(ticket: &Ticket, params: &Value) -> Vec<Value> {
    let present: Vec<&str> = ticket.people.iter().map(|person| person.role.as_str()).collect();
    texts(params, "roles")
        .into_iter()
        .filter(|role| !present.contains(role))
        .map(|role| serde_json::json!({ "role": role }))
        .collect()
}

fn check_roles_disjoint(ticket: &Ticket, params: &Value) -> Vec<Value> {
    let roles = texts(params, "roles");
    let names_of = |role: &str| -> Vec<&str> {
        ticket.people.iter().filter(|person| person.role == role).map(|person| person.name.as_str()).collect()
    };
    let first = names_of(roles[0]);
    let mut shared: Vec<&str> = names_of(roles[1]).into_iter().filter(|name| first.contains(name)).collect();
    shared.sort_unstable();
    shared.dedup();
    shared.into_iter().map(|name| serde_json::json!({ "name": name, "roles": roles.join("、") })).collect()
}

/// 路径的最后一段，用作消息里的栏目名（`fields.签发时间` → `签发时间`）。
fn last_segment(path: &str) -> &str {
    path.rsplit_once('.').map(|(_, tail)| tail).unwrap_or(path)
}

fn check_before(ticket: &Ticket, params: &Value) -> Vec<Value> {
    let strict = params["strict"].as_bool().expect("参数形状已在加载规则时校验");
    // 没填的时间从链条里跳过，只比较填了的相邻两项
    let chain: Vec<(&str, String)> = texts(params, "chain")
        .into_iter()
        .filter_map(|path| string_values(ticket, path).into_iter().next().map(|value| (path, value)))
        .collect();
    chain
        .windows(2)
        .filter(|pair| pair[0].1 > pair[1].1 || (strict && pair[0].1 == pair[1].1))
        .map(|pair| {
            serde_json::json!({
                "a": last_segment(pair[0].0),
                "b": last_segment(pair[1].0),
                "a_time": pair[0].1,
                "b_time": pair[1].1,
            })
        })
        .collect()
}

fn check_duration_max(ticket: &Ticket, params: &Value) -> Vec<Value> {
    let start = string_values(ticket, text(params, "start"));
    let end = string_values(ticket, text(params, "end"));
    let limit = params["hours"].as_f64().expect("参数形状已在加载规则时校验");
    let (Some(start), Some(end)) = (start.first(), end.first()) else { return Vec::new() };
    // 票面时间由解析器产出，一定是 ISO 格式；不是就说明解析器有问题，直接暴露
    let seconds = crate::time::parse_iso(end).expect("票面时间应为 ISO 8601")
        - crate::time::parse_iso(start).expect("票面时间应为 ISO 8601");
    let hours = seconds as f64 / 3600.0;
    if hours <= limit {
        return Vec::new();
    }
    vec![serde_json::json!({
        "actual_hours": crate::time::format_hours(hours),
        "hours": crate::time::format_hours(limit),
    })]
}

fn check_crew_count(ticket: &Ticket, _params: &Value) -> Vec<Value> {
    static GROUP_SIZE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"(\d+)\s*人").unwrap());
    let members: Vec<&crate::schema::Person> =
        ticket.people.iter().filter(|person| person.role == "工作班成员").collect();
    // 分组派工单写法：“第一组5人”。有这种标注时，总人数按各组人数相加。
    let groups: Vec<i64> = members
        .iter()
        .filter_map(|person| person.note.as_deref())
        .filter_map(|note| GROUP_SIZE.captures(note))
        .filter_map(|caps| caps[1].parse().ok())
        .collect();
    let expected = if groups.is_empty() { 1 + members.len() as i64 } else { groups.iter().sum() };
    match ticket.crew_size {
        Some(actual) if actual != expected => vec![serde_json::json!({
            "actual": actual,
            "expected": expected,
        })],
        _ => Vec::new(),
    }
}

fn check_regex(ticket: &Ticket, rule_pattern: Option<&Pattern>, params: &Value) -> Vec<Value> {
    let must_match = params["must_match"].as_bool().expect("参数形状已在加载规则时校验");
    let pattern = rule_pattern.expect("regex 算子的 pattern 已在加载规则时编译");
    string_values(ticket, text(params, "field"))
        .into_iter()
        .filter(|value| pattern.is_match(value) != must_match)
        .map(|value| serde_json::json!({ "value": value, "evidence": value }))
        .collect()
}

fn check_lines_subset(ticket: &Ticket, params: &Value) -> Vec<Value> {
    let within: Vec<String> = texts(params, "within")
        .into_iter()
        .flat_map(|path| string_values(ticket, path))
        .flat_map(|text| lines(&text))
        .collect();
    let source = string_values(ticket, text(params, "field"));
    let mut extra: Vec<String> = Vec::new();
    for value in &source {
        for line in lines(value) {
            if !within.contains(&line) && !extra.contains(&line) {
                extra.push(line);
            }
        }
    }
    if extra.is_empty() {
        return Vec::new();
    }
    extra.sort_unstable();
    vec![serde_json::json!({
        "lines": extra.join("、"),
        "evidence": source.join("；"),
    })]
}

impl Rule {
    fn run_check(&self, ticket: &Ticket) -> Vec<Value> {
        match self.check.as_str() {
            "required_field" => check_required_field(ticket, &self.params),
            "role_required" => check_role_required(ticket, &self.params),
            "roles_disjoint" => check_roles_disjoint(ticket, &self.params),
            "before" => check_before(ticket, &self.params),
            "duration_max" => check_duration_max(ticket, &self.params),
            "crew_count" => check_crew_count(ticket, &self.params),
            "regex" => check_regex(ticket, self.pattern.as_ref(), &self.params),
            "lines_subset" => check_lines_subset(ticket, &self.params),
            other => unreachable!("check 已在加载时校验：{other}"),
        }
    }
}

/// 校验并编译一条规则。`label` 用在报错信息里，形如“文件：规则 R-XXX-001”。
pub fn parse_rule(label: &str, entry: Value) -> Result<Rule, DataError> {
    for field in ["id", "title", "ticket_types", "check", "params", "severity", "message", "basis", "status"] {
        if entry.get(field).is_none() {
            return Err(DataError(format!("{label} 缺少字段：{field}")));
        }
    }
    let mut basis_keys: Vec<&str> =
        entry["basis"].as_object().map(|basis| basis.keys().map(String::as_str).collect()).unwrap_or_default();
    basis_keys.sort_unstable();
    if basis_keys != ["clause", "doc", "source", "text"] {
        return Err(DataError(format!("{label} 的 basis 必须恰好含 doc、clause、text、source")));
    }
    let check = entry["check"].as_str().unwrap_or("").to_string();
    if !CHECKS.contains(&check.as_str()) {
        return Err(DataError(format!("{label} 的 check 未注册：{check}")));
    }
    let severity = match entry["severity"].as_str().unwrap_or("") {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "info" => Severity::Info,
        _ => return Err(DataError(format!("{label} 的 severity 必须为 error、warning 或 info"))),
    };
    let status = entry["status"].as_str().unwrap_or("");
    if !["draft", "reviewed", "active"].contains(&status) {
        return Err(DataError(format!("{label} 的 status 必须为 draft、reviewed 或 active")));
    }
    if let Some(problem) = params_problem(&check, &entry["params"]) {
        return Err(DataError(format!("{label} 的参数不对：{problem}")));
    }
    for path in paths_in(&check, &entry["params"]) {
        if !path_is_known(&path) {
            return Err(DataError(format!("{label} 引用了不存在的字段：{path}")));
        }
    }
    let pattern = if check == "regex" {
        let source = entry["params"]["pattern"].as_str().unwrap_or("");
        Some(Pattern::compile(label, source).map_err(|err| DataError(err.0))?)
    } else {
        None
    };
    let rule: RawRule =
        serde_json::from_value(entry).map_err(|err| DataError(format!("{label} 字段类型不对（{err}）")))?;
    Ok(Rule {
        id: rule.id,
        ticket_types: rule.ticket_types,
        check: rule.check,
        params: rule.params,
        severity,
        message: rule.message,
        basis: rule.basis,
        status: rule.status,
        pattern,
    })
}

/// 加载 `data/rules/*.rules.json`。任何不符合规范的地方直接报错。
pub fn load_rules(directory: &Path) -> Result<Vec<Rule>, DataError> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(directory)
        .map_err(|err| DataError(format!("{}：无法读取目录（{err}）", directory.display())))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()).map(|name| name.ends_with(".rules.json")).unwrap_or(false)
        })
        .collect();
    paths.sort();
    let mut rules = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .map_err(|err| DataError(format!("{}：读取失败（{err}）", path.display())))?;
        let raw: Vec<Value> = serde_json::from_str(&text)
            .map_err(|err| DataError(format!("{}：JSON 解析失败（{err}）", path.display())))?;
        for entry in raw {
            let label = format!(
                "{}：规则 {}",
                path.display(),
                entry.get("id").and_then(Value::as_str).unwrap_or("（缺少 id）")
            );
            rules.push(parse_rule(&label, entry)?);
        }
    }
    Ok(rules)
}

/// 渲染规则消息：`{field}` 这类占位符换成违规项里的值。
fn render_message(template: &str, violation: &Value) -> String {
    let params = violation_to_params(violation);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        match rest[open + 1..].find('}') {
            Some(close) => {
                let key = &rest[open + 1..open + 1 + close];
                match params.get(key) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &rest[open + 1 + close + 1..];
            }
            None => {
                out.push('{');
                rest = &rest[open + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 对一张票跑全部规则。draft 状态的规则一律只报提示。
pub fn run_rules(ticket: &Ticket, rules: &[Rule]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for rule in rules {
        if !rule.ticket_types.contains(&ticket.ticket_type.as_str().to_string()) {
            continue;
        }
        let severity = if rule.status == "draft" { Severity::Info } else { rule.severity };
        for violation in rule.run_check(ticket) {
            let evidence = violation.get("evidence").and_then(Value::as_str).unwrap_or("").to_string();
            findings.push(Finding {
                rule_id: rule.id.clone(),
                severity,
                message: render_message(&rule.message, &violation),
                basis: rule.basis.clone(),
                evidence,
                confidence: 1.0,
                needs_review: severity != Severity::Error,
                scenario_id: None,
            });
        }
    }
    findings
}
