use std::collections::hash_map::RandomState;
use std::ffi::OsStr;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::{ErrorKind, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use ticketguard::review::ModelUse;
use ticketguard::ui::{read_request, response, route, safe_name, summary};
use ticketguard::Error;

use crate::{
    apply_mode, locate, open_in_browser, read_ticket, review_ticket, start_mode, write_report, Args, Mode, OPT_OUT,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HTML: &str = "text/html; charset=utf-8";

fn report_dir(args: &Args) -> Result<PathBuf, Error> {
    if let Some(dir) = args.options.get(OPT_OUT) {
        return Ok(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().map_err(|err| Error(format!("找不到程序自己的位置（{err}）")))?;
    Ok(exe.with_file_name("报告"))
}

/// 运行主界面，直到用户退出。
pub fn serve(args: &Args) -> Result<(), Error> {
    let mut session = Session { out_dir: report_dir(args)?, model: None, reports: Vec::new() };
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|err| Error(format!("本机没有可用的端口（{err}）")))?;
    let port = listener.local_addr().map_err(|err| Error(format!("本机没有可用的端口（{err}）")))?.port();
    let random = || RandomState::new().build_hasher().finish();
    let token = format!("{:016x}{:016x}", random(), random());
    let address = format!("http://127.0.0.1:{port}/{token}/");
    println!(
        "票审 TicketGuard {VERSION}\n\n\
         主界面已经在浏览器里打开。没有自动打开的话，把下面这行地址复制到浏览器的地址栏：\n\
         {address}\n\n\
         用的时候请不要关掉这个窗口。用完了直接关掉它，或者点页面右上角的“退出票审”。"
    );
    open_in_browser(OsStr::new(&address));

    // 浏览器可能预开空连接或中途断开，连接错误不必提示用户。
    loop {
        let Ok((mut stream, _)) = listener.accept() else { continue };
        if stream.set_read_timeout(Some(Duration::from_secs(2))).is_err()
            || stream.set_write_timeout(Some(Duration::from_secs(10))).is_err()
        {
            continue;
        }
        let Ok(request) = read_request(&mut stream) else { continue };
        let (reply, quit) = match route(&request, port, &token) {
            Some("quit") if request.method == "POST" => (json_response(Ok(json!({"ok": true}))), true),
            Some(path) => (session.reply(&request.method, path, &request.body, args), false),
            None => (not_found(), false),
        };
        if stream.write_all(&reply).is_ok() && quit {
            return Ok(());
        }
    }
}

fn json_response(result: Result<Value, Error>) -> Vec<u8> {
    let value = match result {
        Ok(value) => value,
        Err(err) => json!({"ok": false, "error": err.0}),
    };
    response(200, "application/json; charset=utf-8", &value.to_string())
}

fn not_found() -> Vec<u8> {
    response(404, "text/plain; charset=utf-8", "没有这个页面")
}

fn examples() -> Result<Value, Error> {
    let dir = locate("示例票面");
    let failed = |err| Error(format!("{}：读取目录失败（{err}）", dir.display()));
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(json!([])),
        Err(err) => return Err(failed(err)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(failed)?;
        let path = entry.path();
        let is_text = path.extension().and_then(OsStr::to_str).is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        if is_text && entry.file_type().map_err(failed)?.is_file() {
            files.push(entry.file_name());
        }
    }
    files.sort();
    let mut list = Vec::new();
    for name in files {
        let text = read_ticket(&dir.join(&name))?;
        list.push(json!({"name": name.to_string_lossy(), "text": text}));
    }
    Ok(Value::Array(list))
}

fn accuracy() -> Result<Value, Error> {
    let path = locate("data/accuracy.json");
    let text = fs::read_to_string(&path).map_err(|err| Error(format!("{}：读取失败（{err}）", path.display())))?;
    serde_json::from_str(&text).map_err(|err| Error(format!("{}：不是合法的 JSON（{err}）", path.display())))
}

struct Session {
    out_dir: PathBuf,
    model: Option<Mode>,
    reports: Vec<PathBuf>,
}

impl Session {
    fn reply(&mut self, method: &str, path: &str, body: &str, args: &Args) -> Vec<u8> {
        match (method, path) {
            ("GET", "") => response(200, HTML, include_str!("ui.html")),
            ("GET", "status") => json_response(Ok(self.status())),
            ("GET", "examples") => json_response(examples()),
            ("GET", "accuracy") => json_response(accuracy()),
            ("POST", "check") => json_response(self.check(body, args)),
            ("GET", path) if path.starts_with("report/") => {
                self.report(path).map_or_else(not_found, |html| response(200, HTML, &html))
            }
            _ => not_found(),
        }
    }

    fn status(&self) -> Value {
        let (model, note) = match &self.model {
            Some(Mode::Ready(..)) => ("ready", ""),
            Some(Mode::Failed(reason)) => ("failed", reason.as_str()),
            None if locate("models").is_dir() => ("available", ""),
            None => ("missing", "这个文件夹里没有 models，只能用规则模式"),
            Some(Mode::Rule) => unreachable!("模型状态只保存模型模式的启动结果"),
        };
        json!({"version": VERSION, "model": model, "model_note": note, "out_dir": self.out_dir.display().to_string()})
    }

    fn report(&self, path: &str) -> Option<String> {
        let index = path.strip_prefix("report/")?;
        if !index.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let path = self.reports.get(index.parse::<usize>().ok()?)?;
        fs::read_to_string(path).ok()
    }

    fn check(&mut self, body: &str, args: &Args) -> Result<Value, Error> {
        let format_error = || Error("请求的格式不对".to_string());
        let data: Value = serde_json::from_str(body).map_err(|_| format_error())?;
        let (Some(name), Some(text), Some(stage @ ("rule" | "full"))) =
            (data["name"].as_str(), data["text"].as_str(), data["stage"].as_str())
        else {
            return Err(format_error());
        };
        let (mut review, templates) = review_ticket(text, args)?;
        if stage == "full" {
            if review.findings.iter().any(|finding| finding.rule_id == "CMP-MISSING") {
                if self.model.is_none() {
                    self.model = Some(start_mode("model")?);
                }
                apply_mode(&mut review, &templates, self.model.as_ref().expect("已保存模型模式的启动结果"));
            } else {
                review.model = ModelUse::Done { asked: 0 };
            }
        }
        let name = safe_name(name);
        let (html, report) = write_report(&review, &templates, &name, &format!("{name}.txt"), &self.out_dir)?;
        let counts = summary(&report);
        let (model, model_note) = match &review.model {
            ModelUse::Off => ("off", ""),
            ModelUse::Done { .. } => ("done", ""),
            ModelUse::Failed(reason) => ("failed", reason.as_str()),
        };
        let result = json!({
            "ok": true, "name": name,
            "number": counts["number"], "level": counts["level"], "headline": counts["headline"],
            "issues": counts["issues"], "notes": counts["notes"],
            "pending": counts["pending"], "reworded": counts["reworded"],
            "model": model, "model_note": model_note,
            "report": format!("report/{}", self.reports.len()), "saved": html.display().to_string()
        });
        self.reports.push(html);
        Ok(result)
    }
}
