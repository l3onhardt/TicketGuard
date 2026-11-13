//! T-021 验收测试：主界面背后的页面服务里不碰网络的部分（ADR-0009）。
//!
//! 读请求、拼响应、核对口令和 Host、清洗文件名。页面服务只听本机，
//! 但这台电脑上别的程序和浏览器里别的网页也能连本机，所以口令和 Host 的核对是安全边界。
//!
//! 受保护文件：实现方不得修改。

use ticketguard::ui::{read_request, response, route, safe_name, Request};

fn parse(raw: &str) -> Result<Request, String> {
    read_request(&mut raw.as_bytes()).map_err(|err| err.0)
}

/// 一次只给一个字节的连接：请求头和正文被拆成很多次才读完也要能拼起来。
struct Drip<'a>(&'a [u8]);

impl std::io::Read for Drip<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.0.split_first() {
            Some((first, rest)) if !buffer.is_empty() => {
                buffer[0] = *first;
                self.0 = rest;
                Ok(1)
            }
            _ => Ok(0),
        }
    }
}

struct Broken;

impl std::io::Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "超时"))
    }
}

#[test]
fn reads_a_get_request() {
    let request = parse("GET /abc/status?x=1 HTTP/1.1\r\nhOsT:  127.0.0.1:5000 \r\nAccept: */*\r\n\r\n").expect("能读");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/abc/status");
    assert_eq!(request.host, "127.0.0.1:5000");
    assert_eq!(request.body, "");
}

#[test]
fn reads_a_post_body_by_content_length() {
    let body = "{\"text\":\"断开10kV某线01开关\"}";
    let raw = format!("POST /abc/check HTTP/1.1\r\nHost: 127.0.0.1:5000\r\ncontent-length: {}\r\n\r\n{body}多出来的", body.len());
    assert_eq!(parse(&raw).expect("能读").body, body);
    let request = read_request(&mut Drip(raw.as_bytes())).expect("一次一个字节也能读");
    assert_eq!((request.method.as_str(), request.body.as_str()), ("POST", body));
}

#[test]
fn rejects_broken_requests() {
    assert_eq!(parse("").unwrap_err(), "请求不完整");
    assert_eq!(parse("GET /abc/ HTTP/1.1\r\nHost: x\r\n").unwrap_err(), "请求不完整");
    assert_eq!(parse("GET\r\n\r\n").unwrap_err(), "请求不完整");
    assert_eq!(parse("POST /a HTTP/1.1\r\nContent-Length: 10\r\n\r\n短").unwrap_err(), "请求不完整");
    assert_eq!(parse("POST /a HTTP/1.1\r\nContent-Length: -1\r\n\r\n").unwrap_err(), "请求不完整");
    assert_eq!(parse("POST /a HTTP/1.1\r\nContent-Length: 4194305\r\n\r\n").unwrap_err(), "票面内容太大（超过 4 MB）");
    assert_eq!(parse(&format!("GET /a HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(17000))).unwrap_err(), "请求头太长");
    let raw = b"POST /a HTTP/1.1\r\nContent-Length: 2\r\n\r\n\xff\xfe";
    assert_eq!(read_request(&mut &raw[..]).map_err(|err| err.0).unwrap_err(), "请求正文不是 UTF-8");
    assert!(read_request(&mut Broken).map_err(|err| err.0).unwrap_err().starts_with("读取请求失败（"));
}

#[test]
fn response_has_length_and_no_store() {
    let bytes = response(200, "application/json; charset=utf-8", "{\"ok\":true,\"名\":\"票\"}");
    let text = String::from_utf8(bytes).expect("响应是 UTF-8");
    let (head, body) = text.split_once("\r\n\r\n").expect("头和正文之间有空行");
    assert_eq!(body, "{\"ok\":true,\"名\":\"票\"}");
    let lines: Vec<&str> = head.split("\r\n").collect();
    assert_eq!(lines[0], "HTTP/1.1 200 OK");
    assert!(lines.contains(&"Content-Type: application/json; charset=utf-8"));
    assert!(lines.contains(&format!("Content-Length: {}", body.len()).as_str()), "长度按字节算");
    assert!(lines.contains(&"Cache-Control: no-store"));
    assert!(lines.contains(&"X-Content-Type-Options: nosniff"));
    assert!(lines.contains(&"Connection: close"));
    let missing = String::from_utf8(response(404, "text/plain; charset=utf-8", "没有这个页面")).expect("UTF-8");
    assert!(missing.starts_with("HTTP/1.1 404 Error\r\n"));
}

fn request(host: &str, path: &str) -> Request {
    Request { method: "GET".to_string(), path: path.to_string(), host: host.to_string(), body: String::new() }
}

#[test]
fn route_needs_the_right_host_and_token() {
    assert_eq!(route(&request("127.0.0.1:5000", "/abc/"), 5000, "abc"), Some(""));
    assert_eq!(route(&request("127.0.0.1:5000", "/abc/report/3"), 5000, "abc"), Some("report/3"));
    // 口令不对、口令后面没有斜杠、口令只是前缀
    assert_eq!(route(&request("127.0.0.1:5000", "/abd/status"), 5000, "abc"), None);
    assert_eq!(route(&request("127.0.0.1:5000", "/abc"), 5000, "abc"), None);
    assert_eq!(route(&request("127.0.0.1:5000", "/abcd/status"), 5000, "abc"), None);
    assert_eq!(route(&request("127.0.0.1:5000", "/"), 5000, "abc"), None);
    // Host 必须正好是本机地址加这次的端口：别的网页借域名绕进来时 Host 是它自己的域名
    assert_eq!(route(&request("evil.example:5000", "/abc/status"), 5000, "abc"), None);
    assert_eq!(route(&request("localhost:5000", "/abc/status"), 5000, "abc"), None);
    assert_eq!(route(&request("127.0.0.1:5001", "/abc/status"), 5000, "abc"), None);
    assert_eq!(route(&request("127.0.0.1", "/abc/status"), 5000, "abc"), None);
    assert_eq!(route(&request("", "/abc/status"), 5000, "abc"), None);
}

#[test]
fn safe_name_cannot_leave_the_report_folder() {
    assert_eq!(safe_name("示例1-漏了接地措施.txt"), "示例1-漏了接地措施");
    assert_eq!(safe_name("2026.7.1 某线停电.TXT"), "2026.7.1 某线停电");
    assert_eq!(safe_name("粘贴的票面"), "粘贴的票面");
    assert_eq!(safe_name("..\\..\\Windows\\system32\\票.txt"), "_.._Windows_system32_票");
    assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
    assert_eq!(safe_name("C:\\票?.txt"), "C__票_");
    assert_eq!(safe_name("a\u{0}b\nc\t<d>|\"e\"*"), "a_b_c__d___e__");
    for empty in ["", "   ", "..", ".txt", " . "] {
        assert_eq!(safe_name(empty), "粘贴的票面", "{empty:?}");
    }
    let long = safe_name(&"票".repeat(200));
    assert_eq!(long.chars().count(), 60);
    for name in ["..\\x", "a/b", "con:1", "  .hidden.  "] {
        let cleaned = safe_name(name);
        assert!(!cleaned.contains(['\\', '/', ':']) && !cleaned.starts_with('.') && !cleaned.ends_with('.'), "{cleaned}");
    }
}
