//! 测试共用的小工具：读取 `rust/tests/fixtures/` 下的编造票面和基准结果。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use ticketguard::entities::{load_lexicon, Lexicon};
use ticketguard::ingest::{parse_dist1, ScenarioChunk};
use ticketguard::rules::{load_rules, Rule};
use ticketguard::templates::{build_template, Template};

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// 仓库根目录（本 crate 在 `<root>/rust/tests`）。
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(relative: &str) -> String {
    let path = fixtures().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}：读取失败（{err}）", path.display()))
}

pub fn json(relative: &str) -> serde_json::Value {
    serde_json::from_str(&read(relative)).unwrap_or_else(|err| panic!("{relative}：不是合法 JSON（{err}）"))
}

pub fn ticket(name: &str) -> String {
    read(&format!("tickets/{name}.txt"))
}

/// 规则、模板、同义词表的快照（不是 `data/` 下的现行版本，所以业务改规则不影响这些测试）。
/// 模板由两张编造票面现算，不落盘，避免多一份需要同步维护的夹具。
pub struct Snapshot {
    pub rules: Vec<Rule>,
    pub templates: Vec<Template>,
    pub lexicon: Lexicon,
}

pub fn snapshot() -> Snapshot {
    let lexicon = load_lexicon(&fixtures().join("rules/lexicon.json")).expect("词表快照能加载");
    let templates = [("d1_a", "单台公变更换（有调整）"), ("d1_b", "开闭所间隔检修")]
        .into_iter()
        .enumerate()
        .map(|(index, (name, title))| {
            let text = ticket(name);
            let chunk = ScenarioChunk {
                section: "配电第一种工作票典型场景".to_string(),
                number: (index + 1).to_string(),
                title: title.to_string(),
                start_line: 0,
                text: text.clone(),
            };
            let parsed = parse_dist1(&text).expect("编造票面能解析");
            let built = build_template(&chunk, &parsed, &lexicon);
            serde_json::from_value(built).expect("生成的模板能反序列化")
        })
        .collect();
    Snapshot { rules: load_rules(&fixtures().join("rules")).expect("规则快照能加载"), templates, lexicon }
}

/// 基准结果里列出的全部票面名。
pub fn ticket_names(golden: &serde_json::Value) -> Vec<String> {
    golden["parse"].as_object().expect("golden.parse 是对象").keys().cloned().collect()
}
