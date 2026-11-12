use crate::entities::{abstract_text, DataError};
use crate::review::{scenario_number, sort_findings, ModelUse, Review};
use crate::schema::{Second, Severity};
use crate::templates::Template;
use crate::Error;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Example {
    #[serde(rename = "典票")]
    pub typical: String,
    #[serde(rename = "票面")]
    pub ticket: String,
    #[serde(rename = "答案")]
    pub answer: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Harness {
    pub version: String,
    pub server: String,
    pub model: String,
    pub model_bytes: u64,
    pub threads: u32,
    pub context: u32,
    pub threshold: f64,
    pub system: String,
    pub question: String,
    pub shots: Vec<Example>,
    pub selfcheck: Vec<Example>,
}

pub fn load_harness(path: &Path) -> Result<Harness, DataError> {
    let error = |reason| DataError(format!("{}：{reason}", path.display()));
    let text = std::fs::read_to_string(path).map_err(|err| error(format!("读取失败（{err}）")))?;
    let harness: Harness = serde_json::from_str(&text).map_err(|err| error(format!("设置不合规范（{err}）")))?;
    if !(harness.threshold > 0.5 && harness.threshold <= 1.0) {
        return Err(error("threshold 要大于 0.5、不超过 1".to_string()));
    }
    if !harness.question.contains("{典票}") || !harness.question.contains("{票面}") {
        return Err(error("question 里要同时有 {典票} 和 {票面}".to_string()));
    }
    if harness.shots.iter().chain(&harness.selfcheck).any(|example| !matches!(example.answer.as_str(), "是" | "否")) {
        return Err(error("shots 和 selfcheck 的“答案”只能是“是”或“否”".to_string()));
    }
    for answer in ["是", "否"] {
        if !harness.selfcheck.iter().any(|example| example.answer == answer) {
            return Err(error(format!("selfcheck 里至少要有一道答案为“{answer}”的题")));
        }
    }
    Ok(harness)
}

pub fn plain(text: &str) -> String {
    abstract_text(text)
        .replace("{站}", "某站")
        .replace("{线路}", "某线")
        .replace("{杆号}", "某号杆")
        .replace("{编号}", "")
}

pub fn request_body(harness: &Harness, typical: &str, measure: &str) -> String {
    let question = |typical: &str, measure: &str| {
        harness.question.replace("{典票}", &plain(typical)).replace("{票面}", &plain(measure))
    };
    let mut messages = vec![json!({"role": "system", "content": harness.system})];
    for example in &harness.shots {
        messages.push(json!({"role": "user", "content": question(&example.typical, &example.ticket)}));
        messages.push(json!({"role": "assistant", "content": example.answer}));
    }
    messages.push(json!({"role": "user", "content": question(typical, measure)}));
    json!({
        "messages": messages,
        "max_tokens": 1,
        "temperature": 0,
        "logprobs": true,
        "top_logprobs": 20,
        "chat_template_kwargs": {"enable_thinking": false},
        "cache_prompt": true,
    })
    .to_string()
}

pub fn read_answer(response: &str) -> Result<f64, Error> {
    let value: Value =
        serde_json::from_str(response).map_err(|err| Error(format!("模型的应答不是合法 JSON（{err}）")))?;
    let missing = || Error("模型的应答里没有各个字的概率（top_logprobs）".to_string());
    let probabilities = value["choices"][0]["logprobs"]["content"][0]["top_logprobs"].as_array().ok_or_else(missing)?;
    let (mut yes, mut no) = (0.0, 0.0);
    for item in probabilities {
        match item["token"].as_str() {
            Some("是") => yes = item["logprob"].as_f64().ok_or_else(missing)?.exp(),
            Some("否") => no = item["logprob"].as_f64().ok_or_else(missing)?.exp(),
            _ => {}
        }
    }
    if yes + no < 0.5 {
        return Err(Error("模型没有回答“是”或“否”".to_string()));
    }
    Ok(yes / (yes + no))
}

pub fn http(stream: &mut (impl Read + Write), method: &str, path: &str, body: &str) -> Result<(u16, String), Error> {
    let error = |reason| Error(format!("和模型程序通信失败（{reason}）"));
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(|err| error(err.to_string()))?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|err| error(err.to_string()))?;
    let response = String::from_utf8(response).map_err(|_| error("应答不是 UTF-8".to_string()))?;
    let (head, body) = response.split_once("\r\n\r\n").ok_or_else(|| error("应答不完整".to_string()))?;
    if head.to_ascii_lowercase().contains("transfer-encoding") {
        return Err(error("不支持分块应答".to_string()));
    }
    let status = head
        .split("\r\n")
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| error("状态行不对".to_string()))?;
    Ok((status, body.to_string()))
}

pub fn selfcheck(harness: &Harness, ask: &mut impl FnMut(&str) -> Result<String, Error>) -> Result<(), Error> {
    for (index, example) in harness.selfcheck.iter().enumerate() {
        let response = ask(&request_body(harness, &example.typical, &example.ticket))?;
        let same = read_answer(&response)? >= harness.threshold;
        if same != (example.answer == "是") {
            return Err(Error(format!(
                "模型自检没有通过：第 {} 道自检题应当回答“{}”。模型文件或设置可能不对",
                index + 1,
                example.answer
            )));
        }
    }
    Ok(())
}

pub fn second_opinion(
    review: &mut Review,
    templates: &[Template],
    harness: &Harness,
    ask: &mut impl FnMut(&str) -> Result<String, Error>,
) -> Result<(), Error> {
    // 在副本上累计意见，避免中途失败时留下半截复核结果。
    let mut findings = review.findings.clone();
    let mut asked = 0;
    for finding in findings.iter_mut().filter(|finding| finding.rule_id == "CMP-MISSING") {
        let template = templates
            .iter()
            .find(|template| finding.scenario_id.as_deref() == Some(template.id.as_str()))
            .expect("缺项发现对应的模板一定存在");
        let required = template
            .required_measures
            .iter()
            .find(|measure| measure.id == finding.evidence)
            .expect("缺项发现的措施编号一定来自对应模板");
        let mut best: Option<Second> = None;
        for measure in review.ticket.measures.iter().filter(|measure| measure.category == required.category) {
            let response = ask(&request_body(harness, &finding.typical_text, &measure.text))?;
            let p = read_answer(&response)?;
            asked += 1;
            if best.as_ref().map_or(true, |second| p > second.p) {
                best = Some(Second { measure: measure.text.clone(), p, same: p >= harness.threshold });
            }
        }
        if let Some(second) = best {
            if second.same {
                finding.rule_id = "CMP-REWORDED".to_string();
                finding.severity = Severity::Info;
                finding.title = format!("“{}”栏有一条措施的写法和典票不同", required.category);
                finding.message = format!(
                    "对照典票场景 {}「{}」，票面“{}”栏里没有找到字面一致的这类措施；模型认为票面上有一条和它是同一类措施，只是写法不同，请确认。",
                    scenario_number(&template.id), template.title, required.category
                );
            }
            finding.second = Some(second);
        }
    }
    sort_findings(&mut findings);
    review.findings = findings;
    review.model = ModelUse::Done { asked };
    Ok(())
}
