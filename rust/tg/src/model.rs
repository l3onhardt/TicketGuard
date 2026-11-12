use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use ticketguard::harness::{http, Harness};
use ticketguard::Error;

/// 一个正在运行的模型程序。离开作用域时结束它。
pub struct Model {
    child: std::process::Child,
    port: u16,
}

impl Model {
    /// 核对文件 → 启动模型程序（只听本机）→ 等它就绪 → 自检。`dir` 是模型目录。
    pub fn start(harness: &Harness, dir: &Path) -> Result<Model, Error> {
        let server = dir.join(&harness.server);
        if !server.is_file() {
            return Err(Error(format!("找不到模型程序 {}", server.display())));
        }
        let weights = dir.join(&harness.model);
        let size =
            std::fs::metadata(&weights).map_err(|_| Error(format!("找不到模型文件 {}", weights.display())))?.len();
        if size != harness.model_bytes {
            return Err(Error(format!(
                "模型文件 {} 的大小不对（应为 {} 字节，实际 {size} 字节），可能没有拷贝完整",
                weights.display(),
                harness.model_bytes
            )));
        }
        let port = (|| {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
            Ok::<_, std::io::Error>(listener.local_addr()?.port())
        })()
        .map_err(|err| Error(format!("本机没有可用的端口（{err}）")))?;
        let child = Command::new(server)
            .arg("-m")
            .arg(weights)
            .args([
                "-t",
                &harness.threads.to_string(),
                "-c",
                &harness.context.to_string(),
                "-np",
                "1",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--offline",
                "--no-webui",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| Error(format!("模型程序启动失败（{err}）")))?;
        let mut model = Model { child, port };
        model.wait_ready()?;
        ticketguard::harness::selfcheck(harness, &mut |body| model.ask(body))?;
        Ok(model)
    }

    fn wait_ready(&mut self) -> Result<(), Error> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) {
            if self.child.try_wait().map_err(|err| Error(format!("模型程序启动失败（{err}）")))?.is_some() {
                return Err(Error("模型程序启动后自己退出了（模型模式需要 Windows 10 以上的 64 位系统）".to_string()));
            }
            if let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)) {
                if stream.set_read_timeout(Some(Duration::from_millis(300))).is_ok()
                    && stream.set_write_timeout(Some(Duration::from_millis(300))).is_ok()
                    && matches!(http(&mut stream, "GET", "/health", ""), Ok((200, _)))
                {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        Err(Error("模型加载超过 180 秒还没有就绪".to_string()))
    }

    /// 提问一次：发请求正文，拿回应答正文。
    pub fn ask(&self, body: &str) -> Result<String, Error> {
        let error = |err| Error(format!("和模型程序通信失败（{err}）"));
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).map_err(error)?;
        stream.set_read_timeout(Some(Duration::from_secs(120))).map_err(error)?;
        let (status, body) = http(&mut stream, "POST", "/v1/chat/completions", body)?;
        if status != 200 {
            return Err(Error(format!("模型程序拒绝了提问（状态码 {status}）")));
        }
        Ok(body)
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
