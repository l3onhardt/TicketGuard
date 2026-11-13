use crate::Error;
use std::io::Read;

const MAX_HEAD: usize = 16384;
const MAX_BODY: usize = 4194304;

/// 浏览器发来的一个请求。
#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub host: String,
    pub body: String,
}

fn incomplete() -> Error {
    Error("请求不完整".to_string())
}

fn read(stream: &mut impl Read, buffer: &mut [u8]) -> Result<usize, Error> {
    match stream.read(buffer) {
        Ok(0) => Err(incomplete()),
        Ok(count) => Ok(count),
        Err(err) => Err(Error(format!("读取请求失败（{err}）"))),
    }
}

fn content_length(value: &str) -> Result<usize, Error> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(incomplete());
    }
    match value.parse::<usize>() {
        Ok(size) if size <= MAX_BODY => Ok(size),
        _ => Err(Error("票面内容太大（超过 4 MB）".to_string())),
    }
}

fn parse_head(head: &[u8]) -> Result<(Request, usize), Error> {
    let head = std::str::from_utf8(head).map_err(|_| incomplete())?;
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().expect("split 至少返回一段").split(' ');
    let (Some(method), Some(address), Some(protocol), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(incomplete());
    };
    if method.is_empty() || address.is_empty() || protocol.is_empty() {
        return Err(incomplete());
    }
    let path = address.split_once('?').map_or(address, |(path, _)| path);
    let mut request =
        Request { method: method.to_string(), path: path.to_string(), host: String::new(), body: String::new() };
    let mut length = 0;
    for (name, value) in lines.filter_map(|line| line.split_once(':')) {
        if name.eq_ignore_ascii_case("Host") {
            request.host = value.trim().to_string();
        } else if name.eq_ignore_ascii_case("Content-Length") {
            length = content_length(value.trim())?;
        }
    }
    Ok((request, length))
}

/// 从连接上读一个请求。
pub fn read_request(stream: &mut impl std::io::Read) -> Result<Request, Error> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    let head_end = loop {
        let start = bytes.len().saturating_sub(3);
        let limit = chunk.len().min(MAX_HEAD + 1 - bytes.len());
        let count = read(stream, &mut chunk[..limit])?;
        bytes.extend_from_slice(&chunk[..count]);
        let end = bytes[start..].windows(4).position(|window| window == b"\r\n\r\n").map(|index| start + index + 4);
        if end.unwrap_or(bytes.len()) > MAX_HEAD {
            return Err(Error("请求头太长".to_string()));
        }
        if let Some(end) = end {
            break end;
        }
    };
    let (mut request, length) = parse_head(&bytes[..head_end - 4])?;
    let mut body = bytes.split_off(head_end);
    body.truncate(length);
    while body.len() < length {
        let limit = chunk.len().min(length - body.len());
        let count = read(stream, &mut chunk[..limit])?;
        body.extend_from_slice(&chunk[..count]);
    }
    request.body = String::from_utf8(body).map_err(|_| Error("请求正文不是 UTF-8".to_string()))?;
    Ok(request)
}

/// 拼一个完整的响应（状态行、头、正文）。
pub fn response(status: u16, content_type: &str, body: &str) -> Vec<u8> {
    let reason = if status == 200 { "OK" } else { "Error" };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// 核对 Host 和口令，返回口令后面的那一段地址；对不上返回 None。
pub fn route<'a>(request: &'a Request, port: u16, token: &str) -> Option<&'a str> {
    if request.host != format!("127.0.0.1:{port}") {
        return None;
    }
    request.path.strip_prefix(format!("/{token}/").as_str())
}

/// 把页面传来的名字洗成能当文件名用的主文件名。
pub fn safe_name(name: &str) -> String {
    let name: String =
        name.chars().map(|ch| if ch.is_control() || "\\/:\"*?<>|".contains(ch) { '_' } else { ch }).collect();
    let name = match name.rsplit_once('.') {
        Some((base, ext)) if ext.eq_ignore_ascii_case("txt") => base,
        _ => &name,
    };
    let name = name.trim_matches(|ch: char| ch == '.' || ch.is_whitespace());
    if name.is_empty() {
        "粘贴的票面".to_string()
    } else {
        name.chars().take(60).collect()
    }
}

/// 从一份报告（`build_report` 的结果）里取出主界面要显示的几个数。
pub fn summary(report: &serde_json::Value) -> serde_json::Value {
    let ticket = report["ticket"].as_object().expect("报告票面为对象");
    let number =
        ticket.get("number").and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).unwrap_or("未填写");
    let level = report["verdict"]["level"].as_str().expect("报告结论等级为字符串");
    let headline = report["verdict"]["headline"].as_str().expect("报告结论为字符串");
    let findings = report["findings"].as_array().expect("报告发现为数组");
    let (mut issues, mut notes, mut pending, mut reworded) = (0, 0, 0, 0);
    for finding in findings {
        let severity = finding["severity"].as_str().expect("发现严重度为字符串");
        let rule_id = finding["rule_id"].as_str().expect("发现规则编号为字符串");
        if severity == "info" {
            notes += 1;
        } else {
            issues += 1;
        }
        pending += usize::from(rule_id == "CMP-MISSING");
        reworded += usize::from(rule_id == "CMP-REWORDED");
    }
    serde_json::json!({
        "number": number, "level": level, "headline": headline,
        "issues": issues, "notes": notes, "pending": pending, "reworded": reworded
    })
}
