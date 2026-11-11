//! 规则引擎的行为测试。移植自参考实现的 `tests/test_t006_rules.py`（规划者编写的验收测试）。
//!
//! 每条规则都配一个应命中的样本和一个不应命中的样本（AGENTS.md §6）。
//! 这里用的是规则快照（`fixtures/rules`），另有一条测试保证现行规则库也能加载。

mod common;

use common::{fixtures, repo_root, snapshot, ticket};
use serde_json::{json, Value};
use ticketguard::ingest::parse_dist1;
use ticketguard::rules::engine::{parse_rule, values};
use ticketguard::rules::{load_rules, run_rules, Rule};
use ticketguard::schema::{Person, Severity, Ticket, TicketType, WorkTask};

fn rules() -> Vec<Rule> {
    snapshot().rules
}

fn rule(id: &str) -> Vec<Rule> {
    let found: Vec<Rule> = rules().into_iter().filter(|rule| rule.id == id).collect();
    assert_eq!(found.len(), 1, "规则快照里应当恰好有一条 {id}");
    found
}

fn ids(ticket: &Ticket, rules: &[Rule]) -> Vec<String> {
    run_rules(ticket, rules).into_iter().map(|finding| finding.rule_id).collect()
}

fn a() -> Ticket {
    parse_dist1(&ticket("d1_a")).expect("编造票面 A 能解析")
}

fn b() -> Ticket {
    parse_dist1(&ticket("d1_b")).expect("编造票面 B 能解析")
}

fn hot(kind: TicketType, start: Option<&str>, end: Option<&str>, signer: &str, leader: &str) -> Ticket {
    Ticket {
        ticket_type: kind,
        plan_start: start.map(str::to_string),
        plan_end: end.map(str::to_string),
        people: vec![Person::new(leader, "工作负责人"), Person::new(signer, "签发人")],
        ..Ticket::default()
    }
}

/// hit 样本必须报出该规则，miss 样本不得报出。
fn check(id: &str, hit: &Ticket, miss: &Ticket) {
    let rule = rule(id);
    assert!(ids(hit, &rule).contains(&id.to_string()), "{id} 应命中");
    assert!(!ids(miss, &rule).contains(&id.to_string()), "{id} 不应命中");
}

/// 读出规则快照里某条规则的原始 JSON，供“改坏一处”的校验测试用。
fn raw_rule(id: &str) -> Value {
    for name in ["ticket.rules.json", "location.rules.json", "special.rules.json"] {
        let text = std::fs::read_to_string(fixtures().join("rules").join(name)).unwrap();
        let list: Vec<Value> = serde_json::from_str(&text).unwrap();
        if let Some(found) = list.into_iter().find(|rule| rule["id"] == id) {
            return found;
        }
    }
    panic!("规则快照里没有 {id}");
}

fn load_error(mutate: impl FnOnce(&mut Value)) -> String {
    let mut entry = raw_rule("R-CREW-001");
    mutate(&mut entry);
    match parse_rule("x.rules.json：规则 R-CREW-001", entry) {
        Ok(_) => panic!("改坏的规则不应当通过校验"),
        Err(err) => err.0,
    }
}

#[test]
fn loads_all_rules() {
    let mut names: Vec<String> = rules().into_iter().map(|rule| rule.id).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "R-CREW-001",
            "R-HOT-001",
            "R-HOT-002",
            "R-HOT-003",
            "R-LIVE-001",
            "R-LOC-001",
            "R-LOC-002",
            "R-NEW-001",
            "R-REQ-001",
            "R-REQ-002",
            "R-TIME-001",
            "R-TIME-002",
        ]
    );
}

#[test]
fn current_rule_library_loads() {
    // 现行规则库（业务人员会改）：能加载、编号不重复、每条都有依据原文。
    let rules = load_rules(&repo_root().join("data/rules")).expect("data/rules 能加载");
    assert!(!rules.is_empty());
    let mut names: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count, "规则编号不得重复");
    for rule in &rules {
        assert!(!rule.basis.text.trim().is_empty(), "{} 缺少依据原文", rule.id);
        assert!(!rule.basis.doc.trim().is_empty(), "{} 缺少依据文件", rule.id);
    }
}

#[test]
fn rejects_broken_rules() {
    assert!(load_error(|r| {
        r.as_object_mut().unwrap().remove("message");
    })
    .contains("R-CREW-001"));
    assert!(load_error(|r| {
        r["basis"].as_object_mut().unwrap().remove("text");
    })
    .contains("basis"));
    assert!(load_error(|r| {
        r["basis"]["extra"] = json!("x");
    })
    .contains("basis"));
    assert!(load_error(|r| r["check"] = json!("eval")).contains("eval"));
    assert!(load_error(|r| r["status"] = json!("done")).contains("status"));
    assert!(load_error(|r| r["severity"] = json!("fatal")).contains("severity"));
}

#[test]
fn rejects_unknown_field_path_at_load_time() {
    // 规则里写错字段名，要在加载时就报错，而不是等到审票时才出问题。
    let mut entry = raw_rule("R-REQ-001");
    entry["params"]["fields"] = json!({ "plan_finish": "计划工作时间（至）" });
    let err = parse_rule("x.rules.json：规则 R-REQ-001", entry).expect_err("应当报错").0;
    assert!(err.contains("plan_finish"), "报错信息要指出写错的字段：{err}");

    let mut entry = raw_rule("R-NEW-001");
    entry["params"]["field"] = json!("people.nickname");
    assert!(parse_rule("x", entry).is_err());

    let mut entry = raw_rule("R-NEW-001");
    entry["params"]["pattern"] = json!("新员工|(未闭合");
    let err = parse_rule("x.rules.json：规则 R-NEW-001", entry).expect_err("应当报错").0;
    assert!(err.contains("正则"), "正则写错要在加载时报出来：{err}");
}

/// (规则编号, 把它改坏的方法, 报错里应当出现的参数名)
type BrokenParam = (&'static str, fn(&mut Value), &'static str);

#[test]
fn rejects_malformed_params_at_load_time() {
    // 参数的类型写错（最常见的是把 true 写成 "true"、把数字写成字符串），加载时就要报错，
    // 不能等到审票时才崩，也不能悄悄当成默认值。
    let cases: [BrokenParam; 7] = [
        ("R-TIME-001", |r| r["params"]["strict"] = json!("false"), "strict"),
        ("R-TIME-001", |r| r["params"]["chain"] = json!(["fields.签发时间"]), "chain"),
        ("R-HOT-001", |r| r["params"]["hours"] = json!("24"), "hours"),
        ("R-HOT-003", |r| r["params"]["roles"] = json!(["签发人"]), "roles"),
        ("R-NEW-001", |r| r["params"]["must_match"] = json!(0), "must_match"),
        ("R-REQ-001", |r| r["params"]["fields"] = json!(["number"]), "fields"),
        ("R-LOC-002", |r| r["params"]["within"] = json!("outage_scope"), "within"),
    ];
    for (id, mutate, word) in cases {
        let mut entry = raw_rule(id);
        mutate(&mut entry);
        let err = parse_rule(&format!("x.rules.json：规则 {id}"), entry).expect_err("应当报错").0;
        assert!(err.contains(word) && err.contains(id), "{id}：报错要指出是哪条规则的哪个参数：{err}");
    }
}

#[test]
fn only_rules_files_are_loaded() {
    let dir = std::env::temp_dir().join(format!("tg-rules-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lexicon.json"), r#"{"synonyms": {}, "keywords": []}"#).unwrap();
    let loaded = load_rules(&dir).expect("只有词表的目录能加载");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn value_paths() {
    let t = a();
    assert_eq!(values(&t, "fields.签发时间"), vec![json!("2099-01-09T11:30")]);
    assert_eq!(values(&t, "fields.会签时间"), Vec::<Value>::new());
    assert_eq!(values(&t, "people.note"), vec![json!("新员工")]);
    assert_eq!(values(&t, "work_tasks.地点"), vec![json!("35kV甲站10kV测试线乙支6号杆甲公变台架")]);
    assert_eq!(values(&t, "crew_size"), vec![json!(5)]);
    let empty_scope = Ticket { outage_scope: Some(String::new()), ..t };
    assert_eq!(values(&empty_scope, "outage_scope"), Vec::<Value>::new());
}

#[test]
fn draft_rule_is_info_with_message() {
    let findings = run_rules(&Ticket { crew_size: Some(6), ..a() }, &rule("R-CREW-001"));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Info);
    assert!(findings[0].needs_review);
    assert_eq!(findings[0].confidence, 1.0);
    assert_eq!(
        findings[0].message,
        "“工作负责人及工作班人员总人数”写的是 6 人，按工作负责人加工作班人员名单应为 5 人。"
    );
    assert_eq!(findings[0].basis.clause, "5.3.1.3");
}

#[test]
fn reviewed_rule_keeps_severity() {
    let mut reviewed = rule("R-CREW-001");
    reviewed[0].status = "reviewed".to_string();
    let findings = run_rules(&Ticket { crew_size: Some(6), ..a() }, &reviewed);
    assert_eq!(findings[0].severity, Severity::Error);
    assert!(!findings[0].needs_review);
}

#[test]
fn ticket_type_filter() {
    let mut both = rule("R-HOT-001");
    both.extend(rule("R-HOT-003"));
    let ticket = hot(TicketType::Dist1, Some("2099-01-01T08:00"), Some("2099-01-03T08:00"), "甲", "甲");
    assert!(ids(&ticket, &both).is_empty());
}

#[test]
fn crew_count() {
    check("R-CREW-001", &Ticket { crew_size: Some(6), ..a() }, &a());
    // 分组派工单写法：“第一组5人”“第二组5人”，总人数按各组相加
    let grouped = Ticket {
        crew_size: Some(10),
        people: vec![
            Person::new("张三", "工作负责人"),
            Person { note: Some("第一组5人".to_string()), ..Person::new("张三", "工作班成员") },
            Person { note: Some("第二组5人".to_string()), ..Person::new("李四", "工作班成员") },
        ],
        ..a()
    };
    assert!(ids(&grouped, &rule("R-CREW-001")).is_empty());
}

#[test]
fn required_field() {
    let hit = Ticket { plan_end: None, ..a() };
    let messages: Vec<String> = run_rules(&hit, &rule("R-REQ-001")).into_iter().map(|f| f.message).collect();
    assert_eq!(messages, ["票面“计划工作时间（至）”栏未填写或无法识别。"]);
    check("R-REQ-001", &hit, &a());
}

#[test]
fn role_required() {
    let mut hit = a();
    hit.people.retain(|person| person.role != "接收人");
    let messages: Vec<String> = run_rules(&hit, &rule("R-REQ-002")).into_iter().map(|f| f.message).collect();
    assert_eq!(messages, ["票面上没有找到接收人的签名。"]);
    check("R-REQ-002", &hit, &a());
}

#[test]
fn time_chain() {
    let mut hit = a();
    hit.fields.insert("接收时间".to_string(), json!("2099-01-09T09:00"));
    let messages: Vec<String> = run_rules(&hit, &rule("R-TIME-001")).into_iter().map(|f| f.message).collect();
    assert_eq!(messages, ["签发时间（2099-01-09T11:30）晚于接收时间（2099-01-09T09:00），时间顺序不对。"]);
    // 接收时间没填时，链条跳过它继续比后面的
    let mut miss = a();
    miss.fields.insert("接收时间".to_string(), Value::Null);
    check("R-TIME-001", &hit, &miss);
}

#[test]
fn plan_order() {
    // 计划开始和结束是同一时刻也算不对（strict）
    check("R-TIME-002", &Ticket { plan_end: Some("2099-01-10T08:30".to_string()), ..a() }, &a());
}

#[test]
fn new_staff_reminder() {
    let found: Vec<(String, String)> =
        run_rules(&a(), &rule("R-NEW-001")).into_iter().map(|f| (f.message, f.evidence)).collect();
    assert_eq!(
        found,
        [(
            "工作班人员中有“新员工”，请确认已完成安全教育培训，并在监护人员监护下参加指定的工作。".to_string(),
            "新员工".to_string()
        )]
    );
    check("R-NEW-001", &a(), &b());
}

#[test]
fn location_text() {
    let hit = Ticket {
        work_tasks: vec![WorkTask { place: "甲公变台架".to_string(), task: "更换".to_string() }],
        ..a()
    };
    check("R-LOC-001", &hit, &a());
}

#[test]
fn location_lines() {
    let hit = Ticket { outage_scope: Some("35kV甲站10kV示范线".to_string()), ..a() };
    let findings = run_rules(&hit, &rule("R-LOC-002"));
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("10kV测试线"));
    assert_eq!(findings[0].evidence, "35kV甲站10kV测试线乙支6号杆甲公变台架");
    check("R-LOC-002", &hit, &b());
}

#[test]
fn hot_work() {
    let h1 = TicketType::Hot1;
    check(
        "R-HOT-001",
        &hot(h1, Some("2099-01-01T08:00"), Some("2099-01-02T09:00"), "甲", "乙"),
        &hot(h1, Some("2099-01-01T08:00"), Some("2099-01-02T08:00"), "甲", "乙"),
    );
    let message =
        run_rules(&hot(h1, Some("2099-01-01T08:00"), Some("2099-01-02T08:30"), "甲", "乙"), &rule("R-HOT-001"))[0]
            .message
            .clone();
    assert_eq!(message, "一级动火工作票计划时间为 24.5 小时，超过有效期 24 小时。");
    let h2 = TicketType::Hot2;
    check(
        "R-HOT-002",
        &hot(h2, Some("2099-01-01T00:00"), Some("2099-01-06T01:00"), "甲", "乙"),
        &hot(h2, Some("2099-01-01T00:00"), Some("2099-01-06T00:00"), "甲", "乙"),
    );
    check("R-HOT-003", &hot(h1, None, None, "甲", "甲"), &hot(h1, None, None, "甲", "乙"));
}

#[test]
fn live_work_needs_monitor() {
    let hit =
        Ticket {
            ticket_type: TicketType::Live, people: vec![Person::new("甲", "工作负责人")], ..Ticket::default()
        };
    let miss = Ticket {
        ticket_type: TicketType::Live,
        people: vec![Person::new("甲", "工作负责人"), Person::new("乙", "专责监护人")],
        ..Ticket::default()
    };
    check("R-LIVE-001", &hit, &miss);
}

#[test]
fn every_rule_has_hit_and_miss_coverage() {
    // AGENTS §6：新增规则必须同时提供应命中、不应命中的样本。
    // 这里把上面覆盖到的规则编号列出来；规则快照里多出一条没列的，本测试就会失败。
    let covered = [
        "R-CREW-001",
        "R-HOT-001",
        "R-HOT-002",
        "R-HOT-003",
        "R-LIVE-001",
        "R-LOC-001",
        "R-LOC-002",
        "R-NEW-001",
        "R-REQ-001",
        "R-REQ-002",
        "R-TIME-001",
        "R-TIME-002",
    ];
    for rule in rules() {
        assert!(covered.contains(&rule.id.as_str()), "规则 {} 还没有“命中 / 不命中”测试", rule.id);
    }
}
