//! 核心数据结构。字段含义与约束见 docs/03-数据规范.md。
//!
//! 移植自参考实现 `src/ticketguard/schema.py`。序列化结果与参考实现的
//! `dataclasses.asdict()` 一致：字段顺序相同，空值写成 null 而不是省略。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 票种。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TicketType {
    #[serde(rename = "配电第一种工作票")]
    Dist1,
    #[serde(rename = "配电第二种工作票")]
    Dist2,
    #[serde(rename = "紧急抢修工作票")]
    Emergency,
    #[serde(rename = "配电带电作业工作票")]
    Live,
    #[serde(rename = "一级动火工作票")]
    Hot1,
    #[serde(rename = "二级动火工作票")]
    Hot2,
    #[serde(rename = "书面布置记录票")]
    Written,
    #[serde(rename = "操作票")]
    Operation,
    #[default]
    #[serde(rename = "未识别")]
    Unknown,
}

impl TicketType {
    pub fn as_str(self) -> &'static str {
        match self {
            TicketType::Dist1 => "配电第一种工作票",
            TicketType::Dist2 => "配电第二种工作票",
            TicketType::Emergency => "紧急抢修工作票",
            TicketType::Live => "配电带电作业工作票",
            TicketType::Hot1 => "一级动火工作票",
            TicketType::Hot2 => "二级动火工作票",
            TicketType::Written => "书面布置记录票",
            TicketType::Operation => "操作票",
            TicketType::Unknown => "未识别",
        }
    }
}

impl fmt::Display for TicketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 发现等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 明确违反条款，建议打回
    Error,
    /// 疑似问题，需人工确认
    Warning,
    /// 规范性提示
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 票面上出现的一个人。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    /// 工作负责人 / 签发人 / 会签人 / 接收人 / 许可人 / 专责监护人 / 工作班成员
    pub role: String,
    /// 代签人：“李某（张三代签）”解析为 name=李某, signed_by=张三
    #[serde(default)]
    pub signed_by: Option<String>,
    /// 身份标注：“赵七（厂家技术人员）”里的“厂家技术人员”
    #[serde(default)]
    pub note: Option<String>,
}

impl Person {
    pub fn new(name: &str, role: &str) -> Self {
        Person { name: name.to_string(), role: role.to_string(), signed_by: None, note: None }
    }
}

/// 一条安全措施。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SafetyMeasure {
    /// 应拉开关刀闸熔断器 / 应投切电源及二次回路 / 接地 / 标志牌 / 保留带电设备 / 班组自装接地
    pub category: String,
    pub text: String,
    #[serde(default)]
    pub order: Option<usize>,
}

/// 工作地点与任务的对应项。参考实现是 `{"地点": ..., "任务": ...}`。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkTask {
    #[serde(rename = "地点")]
    pub place: String,
    #[serde(rename = "任务")]
    pub task: String,
}

/// 一张工作票的结构化表示。原文必须保留在 raw_text，用于证据回溯。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ticket {
    pub ticket_type: TicketType,
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub raw_text: String,
    #[serde(default)]
    pub work_tasks: Vec<WorkTask>,
    #[serde(default)]
    pub outage_scope: Option<String>,
    /// ISO 8601
    #[serde(default)]
    pub plan_start: Option<String>,
    #[serde(default)]
    pub plan_end: Option<String>,
    #[serde(default)]
    pub people: Vec<Person>,
    #[serde(default)]
    pub crew_size: Option<i64>,
    #[serde(default)]
    pub measures: Vec<SafetyMeasure>,
    /// 票种特有字段。参考实现是 `dict[str, Any]`，这里用有序映射保持同一顺序。
    #[serde(default)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// 判定依据：必须能回溯到具体文件和条款。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Basis {
    /// 如 “安规（配电）释义”
    pub doc: String,
    /// 如 “21.3.5.3”
    pub clause: String,
    /// 条款原文（节选）
    pub text: String,
    /// 出处，如 “典票库 脚注#24”
    pub source: String,
}

/// 一条审查发现。AI 永远只输出“疑似”，最终结论由人确认。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub basis: Basis,
    /// 票面原文片段
    pub evidence: String,
    /// 硬规则为 1.0；场景比对为匹配度
    pub confidence: f64,
    /// severity 不是 error 时必须人工复核
    pub needs_review: bool,
    /// 对照的典票场景
    pub scenario_id: Option<String>,
}
