//! T-012 验收测试：模型复核的核心逻辑（设置、提问、读回答、把意见写回预审结果）。
//!
//! 这里没有真模型：`ask` 是测试里的假模型，按提问内容给出固定的把握。
//! 提问的写法必须和 ADR-0008 实测时用的完全一样——阈值是在那套写法上定的，
//! 所以 `request_is_exactly_what_was_measured` 把请求正文逐字钉住。
//!
//! 受保护文件：实现方不得修改。

mod common;

use common::{fixtures, repo_root, snapshot, ticket, Snapshot};
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};
use ticketguard::harness::{http, load_harness, plain, read_answer, request_body, second_opinion, selfcheck, Harness};
use ticketguard::review::{review_text, ModelUse, Review};
use ticketguard::schema::Severity;
use ticketguard::Error;

fn harness() -> Harness {
    load_harness(&fixtures().join("harness.json")).expect("设置快照能加载")
}

fn review_of(name: &str, data: &Snapshot) -> Review {
    review_text(&ticket(name), &data.rules, &data.templates, &data.lexicon).expect("预审成功")
}

/// 模型程序的应答（只保留用得到的部分）：“是”的把握为 p。
fn reply(p: f64) -> String {
    json!({"choices": [{"logprobs": {"content": [{"token": "是", "top_logprobs": [
        {"token": "是", "logprob": p.ln()},
        {"token": "否", "logprob": (1.0 - p).ln()},
        {"token": "不", "logprob": -20.0},
    ]}]}}]})
    .to_string()
}

/// 请求正文里最后一条 user 消息（本题）。
fn last_question(body: &str) -> String {
    let value: Value = serde_json::from_str(body).expect("请求正文是合法 JSON");
    value["messages"].as_array().expect("messages 是数组").last().expect("至少一条消息")["content"]
        .as_str()
        .expect("content 是字符串")
        .to_string()
}

/// 假模型：本题的“票面”一行含 `word` 就给 0.9，否则给 0.1。记下每次的提问。
fn fake<'a>(word: &'a str, asked: &'a mut Vec<String>) -> impl FnMut(&str) -> Result<String, Error> + 'a {
    move |body: &str| {
        let question = last_question(body);
        let said = question.lines().find(|line| line.starts_with("票面：")).expect("提问里有“票面：”一行").to_string();
        asked.push(question);
        Ok(reply(if said.contains(word) { 0.9 } else { 0.1 }))
    }
}

fn find<'a>(review: &'a Review, rule_id: &str, category: &str) -> &'a ticketguard::schema::Finding {
    review
        .findings
        .iter()
        .find(|finding| finding.rule_id == rule_id && finding.title.contains(category))
        .unwrap_or_else(|| panic!("没有找到 {rule_id}（{category}）"))
}

// ---------- 设置 ----------

/// (说明, 把好的设置改坏的办法, 报错里应当点出的字段)
type Case<'a> = (&'a str, &'a dyn Fn(&mut Value), &'a str);

#[test]
fn settings_load_and_the_shipped_file_is_valid() {
    let harness = harness();
    assert_eq!(harness.version, "h1");
    assert_eq!(harness.server, "llama-b12110/llama-server.exe");
    assert_eq!(harness.model, "Qwen3.5-2B-Q4_K_M.gguf");
    assert_eq!(harness.model_bytes, 1280835840);
    assert_eq!((harness.threads, harness.context), (4, 4096));
    assert_eq!(harness.threshold, 0.6);
    assert_eq!(harness.shots.len(), 9);
    assert_eq!(harness.shots[0].typical, "断开10kV东风线01开关操作电源空开");
    assert_eq!(harness.shots[0].ticket, "将10kV红旗线12开关的操作电源空开断开");
    assert_eq!(harness.shots[0].answer, "是");
    assert_eq!(harness.selfcheck.len(), 2);
    // 随产品发的那一份也要能加载
    load_harness(&repo_root().join("data/harness.json")).expect("data/harness.json 能加载");
}

#[test]
fn bad_settings_are_rejected_with_the_file_name() {
    let dir = std::env::temp_dir().join(format!("tg-t012-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good: Value = serde_json::from_str(&common::read("harness.json")).unwrap();
    let cases: [Case; 6] = [
        ("阈值太低", &|v| v["threshold"] = json!(0.5), "threshold"),
        ("阈值超过 1", &|v| v["threshold"] = json!(1.2), "threshold"),
        ("问题里缺占位", &|v| v["question"] = json!("典票：{典票}\n是不是同一类措施？"), "question"),
        ("答案不是是或否", &|v| v["shots"][0]["答案"] = json!("对"), "答案"),
        ("自检题没有否", &|v| v["selfcheck"] = json!([v["selfcheck"][0]]), "selfcheck"),
        ("缺字段", &|v| drop(v.as_object_mut().unwrap().remove("system")), "system"),
    ];
    for (name, change, word) in cases {
        let mut value = good.clone();
        change(&mut value);
        let path = dir.join("harness.json");
        std::fs::write(&path, value.to_string()).unwrap();
        let err = load_harness(&path).expect_err(name).to_string();
        assert!(err.contains("harness.json"), "{name}：报错要带文件名，实际是：{err}");
        assert!(err.contains(word), "{name}：报错要点出 {word}，实际是：{err}");
    }
    let missing = load_harness(&dir.join("没有这个文件.json")).expect_err("文件不存在").to_string();
    assert!(missing.contains("没有这个文件.json") && missing.contains("读取失败"), "{missing}");
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---------- 提问 ----------

#[test]
fn names_and_numbers_are_wiped_before_asking() {
    assert_eq!(
        plain("在10kV四镇线77号杆小号侧装设接地线1组。（共1组接地线）"),
        "在某线某号杆小号侧装设接地线1组。（共1组接地线）"
    );
    assert_eq!(plain("断开110kV凤岭站10kV中马线906开关操作电源空开"), "断开某站某线开关操作电源空开");
    assert_eq!(plain("10kV测试线测试01开关2刀闸"), "某线测试开关刀闸");
    assert_eq!(plain("邻近带电设备工作须保持足够的安全距离"), "邻近带电设备工作须保持足够的安全距离");
}

#[test]
fn request_is_exactly_what_was_measured() {
    let harness = harness();
    let body = request_body(
        &harness,
        "在10kV测试线测试01开关所在杆塔下方明显可视位置悬挂“禁止合闸，线路有人工作！”标志牌。",
        "在10kV测试线测试01开关所在杆塔下方明显可视位置挂上“禁止合闸，线路有人工作！”标志牌。",
    );
    let value: Value = serde_json::from_str(&body).expect("请求正文是合法 JSON");
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["messages", "max_tokens", "temperature", "logprobs", "top_logprobs", "chat_template_kwargs", "cache_prompt"]
    );
    assert_eq!(value["max_tokens"], 1);
    assert_eq!(value["temperature"], 0);
    assert_eq!(value["logprobs"], true);
    assert_eq!(value["top_logprobs"], 20);
    assert_eq!(value["chat_template_kwargs"], json!({"enable_thinking": false}));
    assert_eq!(value["cache_prompt"], true);

    let messages = value["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1 + 2 * 9 + 1, "系统提示 + 9 道例题（一问一答）+ 本题");
    assert_eq!(messages[0], json!({"role": "system", "content": harness.system}));
    assert_eq!(
        messages[1],
        json!({"role": "user", "content": "典票：断开某线开关操作电源空开\n票面：将某线开关的操作电源空开断开\n是不是同一类措施？"}),
        "例题也要抹掉线路名和编号"
    );
    assert_eq!(messages[2], json!({"role": "assistant", "content": "是"}));
    assert_eq!(messages[4], json!({"role": "assistant", "content": "否"}));
    assert_eq!(
        messages[19],
        json!({"role": "user", "content": "典票：在某线测试开关所在杆塔下方明显可视位置悬挂“禁止合闸，线路有人工作！”标志牌。\n票面：在某线测试开关所在杆塔下方明显可视位置挂上“禁止合闸，线路有人工作！”标志牌。\n是不是同一类措施？"})
    );
    assert_eq!(request_body(&harness, "甲", "乙"), request_body(&harness, "甲", "乙"), "同样的提问每次都一样");
}

// ---------- 读回答 ----------

#[test]
fn answer_is_the_share_of_yes() {
    assert!((read_answer(&reply(0.9)).unwrap() - 0.9).abs() < 1e-9);
    assert!((read_answer(&reply(0.25)).unwrap() - 0.25).abs() < 1e-9);
    // 只出现“是”，没有“否”：把握就是 1
    let only_yes =
        json!({"choices": [{"logprobs": {"content": [{"top_logprobs": [{"token": "是", "logprob": -0.1}]}]}}]});
    assert_eq!(read_answer(&only_yes.to_string()).unwrap(), 1.0);
    // 按两个字的概率归一：0.3 / (0.3 + 0.3)
    let half = json!({"choices": [{"logprobs": {"content": [{"top_logprobs": [
        {"token": "是", "logprob": 0.3f64.ln()}, {"token": "否", "logprob": 0.3f64.ln()}, {"token": "嗯", "logprob": 0.4f64.ln()},
    ]}]}}]});
    assert!((read_answer(&half.to_string()).unwrap() - 0.5).abs() < 1e-9);
}

#[test]
fn unusable_answers_are_errors_not_guesses() {
    let chatty = json!({"choices": [{"logprobs": {"content": [{"top_logprobs": [
        {"token": "根据", "logprob": -0.2}, {"token": "是", "logprob": -3.0}, {"token": "否", "logprob": -3.0},
    ]}]}}]});
    let err = read_answer(&chatty.to_string()).expect_err("两个字加起来不到一半").to_string();
    assert!(err.contains("没有回答“是”或“否”"), "{err}");
    let err = read_answer("{\"error\": {\"message\": \"Loading model\"}}").expect_err("没有概率").to_string();
    assert!(err.contains("top_logprobs"), "{err}");
    let err = read_answer("<html>").expect_err("不是 JSON").to_string();
    assert!(err.contains("不是合法 JSON"), "{err}");
}

// ---------- 和模型程序通信 ----------

/// 假的连接：读的时候给出事先备好的应答，写进来的请求记下来。
struct Wire {
    response: Cursor<Vec<u8>>,
    request: Vec<u8>,
}

impl Wire {
    fn new(response: &str) -> Self {
        Wire { response: Cursor::new(response.as_bytes().to_vec()), request: Vec::new() }
    }
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.response.read(buffer)
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.request.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn http_request_and_response() {
    let mut wire = Wire::new(
        "HTTP/1.1 200 OK\r\nServer: llama.cpp\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\nContent-Length: 15\r\n\r\n{\"答\":\"是\"}",
    );
    let (status, body) = http(&mut wire, "POST", "/v1/chat/completions", "{\"问\":\"甲\"}").unwrap();
    assert_eq!((status, body.as_str()), (200, "{\"答\":\"是\"}"));
    assert_eq!(
        String::from_utf8(wire.request).unwrap(),
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"问\":\"甲\"}",
        "Content-Length 按字节数算（中文一个字 3 个字节）"
    );

    // 模型还在加载：状态码照实返回，由调用方决定等不等
    let mut loading = Wire::new("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 2\r\n\r\n{}");
    assert_eq!(http(&mut loading, "GET", "/health", "").unwrap(), (503, "{}".to_string()));
    assert!(String::from_utf8(loading.request).unwrap().starts_with("GET /health HTTP/1.1\r\n"));
}

#[test]
fn http_refuses_what_it_cannot_read() {
    for (name, response) in [
        ("应答不完整", "HTTP/1.1 200 OK\r\nContent-Length: 2"),
        ("空应答", ""),
        ("分块应答", "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n"),
        ("状态行不对", "你好\r\n\r\n{}"),
    ] {
        let err = http(&mut Wire::new(response), "GET", "/health", "").expect_err(name).to_string();
        assert!(err.starts_with("和模型程序通信失败（"), "{name}：{err}");
    }
}

// ---------- 自检 ----------

#[test]
fn selfcheck_passes_only_when_every_probe_is_answered_right() {
    let harness = harness();
    let mut seen = Vec::new();
    // 第一道自检题（答案“是”）的票面里有“合闸压板”，第二道（答案“否”）没有
    selfcheck(&harness, &mut fake("合闸压板", &mut seen)).expect("答对了就通过");
    assert_eq!(seen.len(), 2, "每道自检题问一次");

    let err = selfcheck(&harness, &mut |_: &str| Ok(reply(0.9))).expect_err("全答“是”").to_string();
    assert!(err.contains("模型自检没有通过") && err.contains("第 2 道") && err.contains("“否”"), "{err}");
    let err = selfcheck(&harness, &mut |_: &str| Ok(reply(0.1))).expect_err("全答“否”").to_string();
    assert!(err.contains("第 1 道") && err.contains("“是”"), "{err}");
    // 刚好在阈值上算“是”
    let err = selfcheck(&harness, &mut |_: &str| Ok(reply(0.6))).expect_err("0.6 算“是”").to_string();
    assert!(err.contains("第 2 道"), "{err}");
    let err = selfcheck(&harness, &mut |_: &str| Err(Error("连不上".to_string()))).expect_err("问不出来").to_string();
    assert_eq!(err, "连不上");
}

// ---------- 模型复核 ----------

// d1_a_model：在 d1_a 上改了两处——
//   标志牌栏  “悬挂”写成“挂上”（词表里没有）   → 规则报缺少，模型应当认出只是写法不同
//   应投切栏  删掉“退出…合闸压板”那一条         → 规则报缺少，这一栏剩下的那条不是它
// d1_a_drop_ground：接地栏整栏是“无”             → 规则报缺少，栏里没有措施，不用问模型

#[test]
fn rule_mode_leaves_no_trace_of_the_model() {
    let data = snapshot();
    let review = review_of("d1_a_model", &data);
    assert_eq!(review.model, ModelUse::Off);
    assert_eq!(review.findings.iter().filter(|finding| finding.rule_id == "CMP-MISSING").count(), 2);
    assert!(review.findings.iter().all(|finding| finding.second.is_none()));
    let value = serde_json::to_value(&review.findings[0]).unwrap();
    assert!(value.get("second").is_some(), "second 字段照常序列化（没有就是 null）");
}

#[test]
fn second_opinion_demotes_rewording_and_keeps_real_gaps() {
    let data = snapshot();
    let harness = harness();
    let mut review = review_of("d1_a_model", &data);
    let before = review.findings.len();
    let mut asked = Vec::new();
    second_opinion(&mut review, &data.templates, &harness, &mut fake("挂上", &mut asked)).expect("复核成功");

    assert_eq!(review.model, ModelUse::Done { asked: 2 });
    assert_eq!(review.findings.len(), before, "模型只改等级和说明，不增删发现");
    assert_eq!(
        asked,
        [
            "典票：退出某线测试开关合闸压板\n票面：断开某线测试开关操作电源空开\n是不是同一类措施？",
            "典票：在某线测试开关所在杆塔下方明显可视位置悬挂“禁止合闸，线路有人工作！”标志牌\n票面：在某线测试开关所在杆塔下方明显可视位置挂上“禁止合闸，线路有人工作！”标志牌\n是不是同一类措施？",
        ],
        "按发现的顺序、栏内措施的顺序提问；空栏不问（解析票面时已去掉句末的句号）"
    );

    // 写法不同：降为提示，换编号、标题和说明
    let reworded = find(&review, "CMP-REWORDED", "标志牌");
    assert_eq!(reworded.severity, Severity::Info);
    assert_eq!(reworded.title, "“标志牌”栏有一条措施的写法和典票不同");
    assert_eq!(
        reworded.message,
        "对照典票场景 1「单台公变更换」，票面“标志牌”栏里没有找到字面一致的这类措施；模型认为票面上有一条和它是同一类措施，只是写法不同，请确认。"
    );
    let second = reworded.second.as_ref().expect("记下模型的意见");
    assert_eq!(second.measure, "在10kV测试线测试01开关所在杆塔下方明显可视位置挂上“禁止合闸，线路有人工作！”标志牌");
    assert!((second.p - 0.9).abs() < 1e-9 && second.same);
    assert!(reworded.needs_review && reworded.scenario_id.is_some() && !reworded.typical_text.is_empty());
    assert_eq!(reworded.evidence, "D1-S01-M05", "措施编号不变");

    // 真缺项：等级、标题、说明都不变，只多一条模型意见
    let gap = find(&review, "CMP-MISSING", "应投切电源及二次回路");
    assert_eq!(gap.severity, Severity::Warning);
    assert_eq!(gap.title, "“应投切电源及二次回路”栏可能缺少一类措施");
    assert_eq!(gap.message, "对照典票场景 1「单台公变更换」，票面“应投切电源及二次回路”栏中没有找到这类措施。");
    let second = gap.second.as_ref().expect("问过的都记下意见");
    assert_eq!(second.measure, "断开10kV测试线测试01开关操作电源空开");
    assert!((second.p - 0.1).abs() < 1e-9 && !second.same);

    // 仍然按严重度排序：疑点在前，提示在后
    let levels: Vec<Severity> = review.findings.iter().map(|finding| finding.severity).collect();
    let mut sorted = levels.clone();
    sorted.sort_by_key(|level| *level == Severity::Info);
    assert_eq!(levels, sorted);
}

#[test]
fn an_empty_column_is_not_asked_about() {
    let data = snapshot();
    let mut review = review_of("d1_a_drop_ground", &data);
    second_opinion(&mut review, &data.templates, &harness(), &mut |_: &str| panic!("不该提问")).unwrap();
    assert_eq!(review.model, ModelUse::Done { asked: 0 });
    let empty = find(&review, "CMP-MISSING", "接地");
    assert_eq!(empty.severity, Severity::Warning);
    assert!(empty.second.is_none(), "没问过就没有意见");
}

#[test]
fn several_measures_in_a_column_the_closest_one_wins() {
    // d1_a 的“应拉开关刀闸熔断器”栏有两条措施。把模板要求的“开关2刀闸”那条改写成词表外的说法，
    // 规则报缺少；模型对两条各问一次，取把握大的那条。
    let data = snapshot();
    let text = ticket("d1_a").replace("2、10kV测试线测试01开关2刀闸。", "2、10kV测试线测试01开关2隔离刀。");
    let mut review = review_text(&text, &data.rules, &data.templates, &data.lexicon).expect("预审成功");
    assert_eq!(review.findings.iter().filter(|finding| finding.rule_id == "CMP-MISSING").count(), 1);
    let mut asked = Vec::new();
    second_opinion(&mut review, &data.templates, &harness(), &mut fake("隔离刀", &mut asked)).unwrap();
    assert_eq!(asked.len(), 2);
    assert_eq!(review.model, ModelUse::Done { asked: 2 });
    let second = find(&review, "CMP-REWORDED", "应拉开关刀闸熔断器").second.clone().unwrap();
    assert_eq!(second.measure, "10kV测试线测试01开关2隔离刀");
    assert!(second.same);
}

#[test]
fn threshold_is_inclusive_and_nothing_to_ask_is_still_done() {
    let data = snapshot();
    let harness = harness();
    let mut at = review_of("d1_a_model", &data);
    second_opinion(&mut at, &data.templates, &harness, &mut |_: &str| Ok(reply(0.6))).unwrap();
    assert_eq!(at.findings.iter().filter(|finding| finding.rule_id == "CMP-REWORDED").count(), 2, "0.6 达到阈值");
    let mut below = review_of("d1_a_model", &data);
    second_opinion(&mut below, &data.templates, &harness, &mut |_: &str| Ok(reply(0.59))).unwrap();
    assert_eq!(below.findings.iter().filter(|finding| finding.rule_id == "CMP-REWORDED").count(), 0);

    // 没有疑似缺项的票：一次都不问，但复核算做过了
    let mut clean = review_of("d1_b", &data);
    second_opinion(&mut clean, &data.templates, &harness, &mut |_: &str| panic!("不该提问")).unwrap();
    assert_eq!(clean.model, ModelUse::Done { asked: 0 });
    // 没找到场景的票：没有做措施比对，也就没什么可问
    let mut unrelated = review_of("d1_b_unrelated", &data);
    second_opinion(&mut unrelated, &data.templates, &harness, &mut |_: &str| panic!("不该提问")).unwrap();
    assert_eq!(unrelated.model, ModelUse::Done { asked: 0 });
}

#[test]
fn a_failed_question_leaves_the_review_untouched() {
    let data = snapshot();
    let harness = harness();
    let mut review = review_of("d1_a_model", &data);
    let before = serde_json::to_value(&review.findings).unwrap();
    let mut count = 0;
    // 第一问答成“写法不同”，第二问失败：前面那条也不能改
    let mut flaky = |_: &str| {
        count += 1;
        if count == 1 {
            Ok(reply(0.9))
        } else {
            Err(Error("和模型程序通信失败（连接被重置）".to_string()))
        }
    };
    let err = second_opinion(&mut review, &data.templates, &harness, &mut flaky).expect_err("第二问失败").to_string();
    assert_eq!(err, "和模型程序通信失败（连接被重置）");
    assert_eq!(serde_json::to_value(&review.findings).unwrap(), before);
    assert_eq!(review.model, ModelUse::Off);

    let mut garbled = review_of("d1_a_model", &data);
    let err = second_opinion(&mut garbled, &data.templates, &harness, &mut |_: &str| Ok("{}".to_string()))
        .expect_err("应答读不出把握")
        .to_string();
    assert!(err.contains("top_logprobs"), "{err}");
    assert_eq!(garbled.model, ModelUse::Off);
}
