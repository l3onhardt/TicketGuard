//! 典票场景模板：生成与读取。移植自参考实现 `src/ticketguard/templates.py`。
//!
//! 生成：从一个典票场景（切好的正文 + 解析出的票面）抽出必备措施，见 ADR-0004。
//! 读取：运行时从 `data/templates/<票种>/*.json` 加载。

use crate::entities::{abstract_text, keywords, DataError, Lexicon};
use crate::ingest::scenarios::ScenarioChunk;
use crate::schema::Ticket;
use crate::text::splitlines;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;

static RE_OPTIONAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"则无需|则不用|无需填写|不用写").unwrap());
static RE_SUFFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"[（(](有调整|新增)[）)]\s*$").unwrap());

#[derive(Debug, Clone, Deserialize)]
pub struct RequiredMeasure {
    pub id: String,
    pub category: String,
    /// 占位符化后的写法，比对和签名用
    pub pattern: String,
    /// 典票上的原文（未占位符化）。给审票人看的，例如“在10kV四镇线四镇街公变支12号杆小号侧装设接地线1组”
    #[serde(default)]
    pub source_text: String,
    pub keywords: Vec<String>,
    pub must: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub id: String,
    pub ticket_type: String,
    pub title: String,
    pub work_task: String,
    pub required_measures: Vec<RequiredMeasure>,
}

/// `a` 的关键词是否全部在 `b` 里。
fn is_subset(a: &[String], b: &[String]) -> bool {
    a.iter().all(|word| b.contains(word))
}

/// 模板的必备措施 = 典票措施签名去重：同一类别里，签名被另一条完全包含的删掉。
pub fn required_measures(ticket: &Ticket, lexicon: &Lexicon, prefix: &str) -> Vec<Value> {
    // (类别, 关键词, 措施原文)
    let mut signatures: Vec<(String, Vec<String>, String)> = Vec::new();
    for measure in &ticket.measures {
        let words = keywords(&measure.text, lexicon);
        let covered = signatures
            .iter()
            .any(|(category, existing, _)| *category == measure.category && is_subset(&words, existing));
        if words.is_empty() || covered {
            continue;
        }
        // 新签名严格包含旧签名时，旧的那条已经隐含在新的里了
        signatures.retain(|(category, existing, _)| {
            !(*category == measure.category && is_subset(existing, &words) && !is_subset(&words, existing))
        });
        signatures.push((measure.category.clone(), words, measure.text.clone()));
    }
    signatures
        .into_iter()
        .enumerate()
        .map(|(index, (category, words, text))| {
            let mut map = Map::new();
            map.insert("id".to_string(), json!(format!("{prefix}-M{:02}", index + 1)));
            map.insert("category".to_string(), json!(category));
            map.insert("pattern".to_string(), json!(abstract_text(&text)));
            map.insert("source_text".to_string(), json!(text));
            map.insert("keywords".to_string(), json!(words));
            map.insert("must".to_string(), json!(!RE_OPTIONAL.is_match(&text)));
            Value::Object(map)
        })
        .collect()
}

/// 从一个典票场景生成模板初稿（status 固定为 draft，等业务人员核对）。
pub fn build_template(chunk: &ScenarioChunk, ticket: &Ticket, lexicon: &Lexicon) -> Value {
    let template_id = format!("D1-S{:02}", chunk.number.parse::<i64>().unwrap_or(0));
    let suffix = RE_SUFFIX.captures(&chunk.title);
    let title = match &suffix {
        Some(caps) => &chunk.title[..caps.get(0).expect("分组 0 一定存在").start()],
        None => chunk.title.as_str(),
    };
    let mut task = ticket.work_tasks.first().map(|item| item.task.clone()).unwrap_or_default();
    let lines = splitlines(&chunk.text);
    for (index, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("编号：") {
            break;
        }
        if let Some((_, rest)) = line.split_once("工作任务：") {
            task = rest.trim().to_string();
            if task.is_empty() {
                task = lines.get(index + 1).map(|next| next.trim().to_string()).unwrap_or_default();
            }
            break;
        }
    }
    let mut source = Map::new();
    source.insert("doc".to_string(), json!("2026.9典票库"));
    source.insert("section".to_string(), json!(chunk.section));
    source.insert("scenario".to_string(), json!(chunk.number));
    source.insert("status".to_string(), json!(suffix.as_ref().map(|caps| caps[1].to_string()).unwrap_or_default()));
    let mut map = Map::new();
    map.insert("id".to_string(), json!(template_id));
    map.insert("ticket_type".to_string(), json!(ticket.ticket_type.as_str()));
    map.insert("title".to_string(), json!(title.trim()));
    map.insert("source".to_string(), Value::Object(source));
    map.insert("work_task".to_string(), json!(abstract_text(&task)));
    map.insert("required_measures".to_string(), Value::Array(required_measures(ticket, lexicon, &template_id)));
    map.insert("status".to_string(), json!("draft"));
    Value::Object(map)
}

/// 读取一个目录下的全部模板，按文件名排序。缺字段直接报错。
pub fn load_templates(directory: &Path) -> Result<Vec<Template>, DataError> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(directory)
        .map_err(|err| DataError(format!("{}：无法读取目录（{err}）", directory.display())))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    paths.sort();
    let mut templates = Vec::with_capacity(paths.len());
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .map_err(|err| DataError(format!("{}：读取失败（{err}）", path.display())))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| DataError(format!("{}：JSON 解析失败（{err}）", path.display())))?;
        let missing: Vec<&str> = ["id", "ticket_type", "title", "work_task", "required_measures"]
            .into_iter()
            .filter(|field| value.get(field).is_none())
            .collect();
        if !missing.is_empty() {
            return Err(DataError(format!("{}：缺少字段 {}", path.display(), missing.join(", "))));
        }
        let template: Template = serde_json::from_value(value)
            .map_err(|err| DataError(format!("{}：字段类型不对（{err}）", path.display())))?;
        templates.push(template);
    }
    if templates.is_empty() {
        return Err(DataError(format!("{}：目录里没有模板文件（*.json）", directory.display())));
    }
    Ok(templates)
}
