//! 票审 TicketGuard 命令行入口（Rust 版）。
//!
//! 与参考实现 `tools/tg.py` 的子命令一一对应。移植期只保留运行路径需要的命令：
//! `check`（预审一张票）、`eval`（合成集评测）、`synth`（生成合成样本）、
//! `scenarios`（按场景切分典票正文）。典票 docx 的读取暂不在 Rust 里做。

mod model;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ticketguard::entities::load_lexicon;
use ticketguard::evaluate::evaluate;
use ticketguard::harness::{load_harness, second_opinion, Harness};
use ticketguard::ingest::{parse_dist1, split_scenarios};
use ticketguard::report::{build_report, render_html};
use ticketguard::review::{review_text, ModelUse};
use ticketguard::rules::load_rules;
use ticketguard::synth::generate;
use ticketguard::templates::{build_template, load_templates};
use ticketguard::Error;

fn usage() -> String {
    "用法：tg <命令> [参数]\n\
     \n\
     命令：\n\
       check <票面文件> [--out 目录] [--templates 目录] [--rules 目录] [--lexicon 文件] [--mode rule|model|auto]\n\
           预审一张票（票面文本 .txt），写出 JSON 和 HTML 报告\n\
           --mode：rule 只用规则；model 规则之后再请本机模型复核；auto（默认）有模型就用\n\
       eval <典票正文.txt> [样本.jsonl] [--out 报告.json] [--templates 目录] [--rules 目录] [--lexicon 文件]\n\
           在合成集上评测检出率与场景命中\n\
       synth <典票正文.txt> [输出.jsonl] [--per N] [--seed N]\n\
           生成带期望标签的合成票面样本\n\
       templates <典票正文.txt> [输出目录] [--lexicon 文件]\n\
           从典票正文生成第一种票的场景模板初稿\n\
       scenarios <典票正文.txt> [--section 名称]\n\
           按“场景 N：”切分并打印场景清单\n"
        .to_string()
}

struct Args {
    positional: Vec<String>,
    options: HashMap<String, String>,
}

impl Args {
    fn option(&self, name: &str, default: &str) -> String {
        self.options.get(name).cloned().unwrap_or_else(|| default.to_string())
    }

    /// 数据文件的路径：命令行给了就用给的，否则去找默认位置。
    fn data(&self, name: &str, default: &str) -> PathBuf {
        match self.options.get(name) {
            Some(given) => PathBuf::from(given),
            None => locate(default),
        }
    }
}

fn parse_args(argv: &[String]) -> Result<(String, Args), Error> {
    let mut command = None;
    let mut args = Args { positional: Vec::new(), options: HashMap::new() };
    let mut index = 0;
    while index < argv.len() {
        let token = &argv[index];
        if command.is_none() && !token.starts_with("--") {
            command = Some(token.clone());
        } else if let Some(name) = token.strip_prefix("--") {
            let value = argv.get(index + 1).ok_or_else(|| Error(format!("选项 --{name} 后面要跟一个值")))?;
            args.options.insert(name.to_string(), value.clone());
            index += 1;
        } else {
            args.positional.push(token.clone());
        }
        index += 1;
    }
    let command = command.ok_or_else(|| Error(usage()))?;
    Ok((command, args))
}

/// 选项名不带 `--` 前缀。
const OPT_LEXICON: &str = "lexicon";
const OPT_RULES: &str = "rules";
const OPT_TEMPLATES: &str = "templates";
const OPT_OUT: &str = "out";

fn count_suffix(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().ends_with(suffix))
        .unwrap_or(false)
}

/// 去掉最后的扩展名，保留中间的点（文件名常含“2026.7.1”）。
fn stem(path: &Path) -> String {
    path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("ticket").to_string()
}

fn write_file(path: &Path, content: &str) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| Error(format!("{}：无法创建目录（{err}）", parent.display())))?;
        }
    }
    std::fs::write(path, content).map_err(|err| Error(format!("{}：写入失败（{err}）", path.display())))
}

/// 找数据文件：先按当前目录，找不到再从 exe 所在目录逐级往上找。
/// 这样把票拖到 exe 上（此时当前目录不确定）也能找到 exe 旁边的 data/。
fn locate(relative: &str) -> PathBuf {
    let direct = PathBuf::from(relative);
    if direct.exists() {
        return direct;
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1) {
            let candidate = dir.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    direct
}

/// 读票面文本。记事本另存时常带 BOM，或者存成了 ANSI（GBK），这里给出能照着做的提示。
fn read_ticket(path: &Path) -> Result<String, Error> {
    let bytes = std::fs::read(path).map_err(|err| Error(format!("{}：读取失败（{err}）", path.display())))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        Error(format!("{}：文件不是 UTF-8 编码。请用记事本打开，“另存为”时把编码选成 UTF-8 再试。", path.display()))
    })?;
    Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
}

enum Mode {
    Rule,
    Ready(model::Model, Box<Harness>),
    Failed(String),
}

fn prepare_mode(args: &Args) -> Result<Mode, Error> {
    let mode = match args.options.get("mode") {
        Some(mode) => mode.clone(),
        None => std::env::var("TG_MODE").unwrap_or_else(|_| "auto".to_string()),
    };
    match mode.as_str() {
        "rule" => return Ok(Mode::Rule),
        "auto" | "model" => {}
        _ => return Err(Error(format!("--mode 只能是 rule、model 或 auto，不能是 {mode}"))),
    }
    let dir = locate("models");
    if mode == "auto" && !dir.is_dir() {
        return Ok(Mode::Rule);
    }
    let harness = load_harness(&locate("data/harness.json"))?;
    println!("正在启动模型（第一次要等一会儿）…");
    Ok(match model::Model::start(&harness, &dir) {
        Ok(model) => Mode::Ready(model, Box::new(harness)),
        Err(err) => {
            println!("模型没有用上，这次只用规则：{err}");
            Mode::Failed(err.0)
        }
    })
}

/// 预审一张票，写出 JSON 和 HTML 报告，返回 HTML 的路径。
fn check_ticket(path: &Path, out_dir: &Path, args: &Args, mode: &Mode) -> Result<PathBuf, Error> {
    if !count_suffix(path, ".txt") {
        return Err(Error(format!("{}：目前只支持票面文本（.txt）。PDF 和 Word 的读取还没有做。", path.display())));
    }
    let text = read_ticket(path)?;
    let lexicon = load_lexicon(&args.data(OPT_LEXICON, "data/rules/lexicon.json"))?;
    let rules = load_rules(&args.data(OPT_RULES, "data/rules"))?;
    let templates = load_templates(&args.data(OPT_TEMPLATES, "data/templates/D1"))?;
    let mut review = review_text(&text, &rules, &templates, &lexicon)?;
    match mode {
        Mode::Ready(model, harness) => {
            if let Err(err) = second_opinion(&mut review, &templates, harness, &mut |body| model.ask(body)) {
                println!("模型复核没有做成，这张票只用了规则：{err}");
                review.model = ModelUse::Failed(err.0);
            }
        }
        Mode::Failed(reason) => review.model = ModelUse::Failed(reason.clone()),
        Mode::Rule => {}
    }
    let report = build_report(&review, &templates, path.file_name().and_then(|name| name.to_str()).unwrap_or(""));
    let name = stem(path);
    write_file(&out_dir.join(format!("{name}.预审.json")), &format!("{}\n", pretty(&report)))?;
    let html = out_dir.join(format!("{name}.预审.html"));
    write_file(&html, &render_html(&report))?;

    let scenario = &report["scenario"];
    let number = match report["ticket"]["number"].as_str() {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => "未填写".to_string(),
    };
    println!("工作票 {number}：{}", report["verdict"]["headline"].as_str().expect("报告结论为字符串"));
    if scenario["matched"] == true {
        println!(
            "对照{}（相似度：{}）",
            scenario["label"].as_str().expect("场景名称为字符串"),
            scenario["similarity"].as_str().expect("相似度为字符串")
        );
    } else {
        println!("没有找到对应的典票场景");
    }
    for finding in report["findings"].as_array().expect("发现为数组") {
        println!(
            "  [{}] {}：{}",
            finding["label"].as_str().expect("标签为字符串"),
            finding["title"].as_str().expect("标题为字符串"),
            finding["message"].as_str().expect("消息为字符串")
        );
    }
    println!("报告 → {}", html.display());
    Ok(html)
}

fn cmd_check(args: &Args) -> Result<(), Error> {
    let source = args.positional.first().ok_or_else(|| Error("check 需要票面文件：tg check <票面文件>".to_string()))?;
    let out_dir = PathBuf::from(args.option(OPT_OUT, "reports"));
    let mode = prepare_mode(args)?;
    check_ticket(Path::new(source), &out_dir, args, &mode).map(|_| ())
}

/// 用系统默认浏览器打开报告。不经过命令解释器，路径里有空格或特殊字符也安全。
/// 环境变量 TG_NO_OPEN 存在时不打开（自动化测试用）。
fn open_in_browser(path: &Path) {
    if std::env::var_os("TG_NO_OPEN").is_some() {
        return;
    }
    let opened = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(path).status();
    if opened.map(|status| !status.success()).unwrap_or(true) {
        println!("没能自动打开报告，请手动双击：{}", path.display());
    }
}

fn wait_for_enter() {
    println!("\n按回车键关闭窗口…");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

/// 拖放模式：把一张或几张票拖到 exe 图标上。报告写在票的旁边，并自动打开。
fn run_dropped(files: &[String]) -> ExitCode {
    let args = Args { positional: Vec::new(), options: HashMap::new() };
    let mode = match prepare_mode(&args) {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("{err}");
            wait_for_enter();
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for file in files {
        let path = Path::new(file);
        let out_dir = path.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or(Path::new("."));
        match check_ticket(path, out_dir, &args, &mode) {
            Ok(html) => open_in_browser(&html),
            Err(err) => {
                failed = true;
                println!("这张票没能预审：{err}");
            }
        }
        println!();
    }
    drop(mode);
    // 出错时停住，让人看清原因；成功时报告已经在浏览器里打开，窗口直接关掉
    if failed {
        wait_for_enter();
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn cmd_eval(args: &Args) -> Result<(), Error> {
    let source = args
        .positional
        .first()
        .ok_or_else(|| Error("eval 需要典票正文：tg eval <典票正文.txt> [样本.jsonl]".to_string()))?;
    let samples_path = args.positional.get(1).map(String::as_str).unwrap_or("data/synthetic/d1.jsonl");
    let text = std::fs::read_to_string(source).map_err(|err| Error(format!("{source}：读取失败（{err}）")))?;
    let chunks = split_scenarios(&text);
    let mut originals: HashMap<String, String> = HashMap::new();
    for chunk in &chunks {
        if chunk.section == "配电第一种工作票典型场景" {
            originals.insert(format!("D1-S{:02}", chunk.number.parse::<i64>().unwrap_or(0)), chunk.text.clone());
        }
    }
    let raw =
        std::fs::read_to_string(samples_path).map_err(|err| Error(format!("{samples_path}：读取失败（{err}）")))?;
    let samples: Vec<serde_json::Value> = ticketguard::text::splitlines(&raw)
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|err| Error(format!("{samples_path}：样本不是合法 JSON（{err}）")))
        })
        .collect::<Result<_, _>>()?;
    let lexicon = load_lexicon(&args.data(OPT_LEXICON, "data/rules/lexicon.json"))?;
    let rules = load_rules(&args.data(OPT_RULES, "data/rules"))?;
    let templates = load_templates(&args.data(OPT_TEMPLATES, "data/templates/D1"))?;
    let report = evaluate(&samples, &originals, &rules, &templates, &lexicon)?;
    let out = args.option(OPT_OUT, "reports/eval-rust.json");
    write_file(Path::new(&out), &format!("{}\n", pretty(&report)))?;
    for (name, stats) in report["by_operator"].as_object().cloned().unwrap_or_default() {
        println!(
            "{name}：样本 {}，场景命中 {}，检出 {}/{}，意外发现 {}",
            stats["samples"], stats["top1"], stats["detected"], stats["expected"], stats["unexpected"]
        );
    }
    println!(
        "合计：样本 {}，检出率 {}（95% 置信区间 {}），意外发现 {} → {out}",
        report["samples"], report["recall"], report["recall_ci95"], report["unexpected"],
    );
    Ok(())
}

fn cmd_synth(args: &Args) -> Result<(), Error> {
    let source = args
        .positional
        .first()
        .ok_or_else(|| Error("synth 需要典票正文：tg synth <典票正文.txt> [输出.jsonl]".to_string()))?;
    let out = args.positional.get(1).map(String::as_str).unwrap_or("data/synthetic/d1.jsonl");
    let per: usize = args.option("per", "5").parse().map_err(|_| Error("--per 需要整数".to_string()))?;
    let seed: u64 = args.option("seed", "0").parse().map_err(|_| Error("--seed 需要整数".to_string()))?;
    let text = std::fs::read_to_string(source).map_err(|err| Error(format!("{source}：读取失败（{err}）")))?;
    let chunks: Vec<_> =
        split_scenarios(&text).into_iter().filter(|chunk| chunk.section == "配电第一种工作票典型场景").collect();
    let samples = generate(&chunks, per, seed);
    let body: String = samples.iter().map(compact).collect::<Vec<_>>().join("\n") + "\n";
    write_file(Path::new(out), &body)?;
    println!("已生成 {} 个样本 → {out}", samples.len());
    Ok(())
}

fn cmd_templates(args: &Args) -> Result<(), Error> {
    let source = args
        .positional
        .first()
        .ok_or_else(|| Error("templates 需要典票正文：tg templates <典票正文.txt> [输出目录]".to_string()))?;
    let out = PathBuf::from(args.positional.get(1).map(String::as_str).unwrap_or("data/templates/D1"));
    let lexicon = load_lexicon(&args.data(OPT_LEXICON, "data/rules/lexicon.json"))?;
    let text = std::fs::read_to_string(source).map_err(|err| Error(format!("{source}：读取失败（{err}）")))?;
    let mut count = 0;
    for chunk in split_scenarios(&text) {
        if chunk.section != "配电第一种工作票典型场景" {
            continue;
        }
        let ticket = parse_dist1(&chunk.text)
            .map_err(|err| Error(format!("场景 {}「{}」解析失败：{}", chunk.number, chunk.title, err.0)))?;
        let template = build_template(&chunk, &ticket, &lexicon);
        let name = template["id"].as_str().unwrap_or("").to_string();
        write_file(&out.join(format!("{name}.json")), &pretty(&template))?;
        count += 1;
    }
    println!("已生成 {count} 个模板 → {}", out.display());
    Ok(())
}

fn cmd_scenarios(args: &Args) -> Result<(), Error> {
    let source = args
        .positional
        .first()
        .ok_or_else(|| Error("scenarios 需要典票正文：tg scenarios <典票正文.txt> [--section 名称]".to_string()))?;
    let filter = args.options.get("section").cloned();
    let text = std::fs::read_to_string(source).map_err(|err| Error(format!("{source}：读取失败（{err}）")))?;
    let chunks = split_scenarios(&text);
    let mut shown = 0;
    let mut counts: Vec<(String, usize)> = Vec::new();
    for chunk in &chunks {
        if !chunk.number.is_empty() {
            match counts.iter_mut().find(|(name, _)| *name == chunk.section) {
                Some((_, count)) => *count += 1,
                None => counts.push((chunk.section.clone(), 1)),
            }
            if let Some(filter) = &filter {
                if &chunk.section != filter {
                    continue;
                }
            }
            println!("{}\t{}\t{}", chunk.section, chunk.number, chunk.title);
            shown += 1;
        }
    }
    println!("---");
    for (name, count) in &counts {
        println!("{name}：{count} 个场景");
    }
    println!("共打印 {shown} 个场景块");
    Ok(())
}

/// 紧凑 JSON（样本文件是 JSONL）。
fn compact(value: &serde_json::Value) -> String {
    serde_json::to_string(value).expect("JSON 序列化不会失败")
}

/// 与 Python `json.dumps(..., ensure_ascii=False, indent=2)` 一致。
fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON 序列化不会失败")
}

const COMMANDS: [&str; 5] = ["check", "eval", "synth", "templates", "scenarios"];

fn run(argv: &[String]) -> Result<(), Error> {
    let (command, args) = parse_args(argv)?;
    match command.as_str() {
        "check" => cmd_check(&args),
        "eval" => cmd_eval(&args),
        "synth" => cmd_synth(&args),
        "templates" => cmd_templates(&args),
        "scenarios" => cmd_scenarios(&args),
        "help" | "--help" | "-h" => {
            print!("{}", usage());
            Ok(())
        }
        other => Err(Error(format!("未知命令：{other}\n\n{}", usage()))),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // 双击 exe：没有参数。告诉用户怎么用，并停住窗口。
    if argv.is_empty() {
        println!("票审 TicketGuard：配电工作票离线智能预审\n");
        println!("用法：把工作票的票面文本（.txt）拖到本程序的图标上，报告会生成在票的旁边并自动打开。");
        println!("本工具只做预审建议，所列均为“疑似问题”，是否接收或打回由审票人决定。\n");
        println!("命令行用法请运行：tg help");
        wait_for_enter();
        return ExitCode::SUCCESS;
    }
    // 拖放：参数全是存在的文件，而且第一个不是命令名
    if !COMMANDS.contains(&argv[0].as_str()) && argv.iter().all(|arg| Path::new(arg).is_file()) {
        return run_dropped(&argv);
    }
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
