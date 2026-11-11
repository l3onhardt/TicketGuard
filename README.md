# 票审 TicketGuard

配电“两票”**离线智能预审**：对照典型票库和安规条款，自动找出工作票里的疑似问题，并给出条款依据和置信度，由审票人决定是否打回。

- 完全离线，在普通办公电脑上运行（i5 四核、8GB 内存、无 GPU，目标机器很可能是 Windows 7）
- 单个 exe，把票拖到图标上就出报告；不用装任何东西
- “规则优先，模型兜底”：能写成规则的用规则判断，只有语义模糊的部分才交给本地小模型
- AI 只做预审建议，不自动打回，也不自动认定违章

## 快速开始

```bash
cargo build --release          # 构建 target/release/tg.exe

# 预审一张票：输入票面文本 .txt，报告（HTML + JSON）写到 reports/
target/release/tg.exe check 某张票.txt

# 也可以把 .txt 直接拖到 tg.exe 的图标上：报告生成在票的旁边，并自动用浏览器打开
target/release/tg.exe help     # 其它命令：eval、synth、templates、scenarios
```

开发（Rust 工具链由 `rust-toolchain.toml` 钉在 1.77.2，首次构建会自动下载）：

```bash
cargo test                                             # 全部测试
cargo fmt && cargo clippy --all-targets -- -D warnings  # 提交前跑，必须零警告
```

## 文档

先读 [AGENTS.md](AGENTS.md)，再按编号读 [docs/](docs/)，从 [01-需求与定位](docs/01-需求与定位.md) 开始。
进度见 [08-开发计划](docs/08-开发计划.md)。

## 当前状态

Rust 版已跑通，与最初的 Python 实现输出逐字节一致（266 张合成票、评测报告、21 个模板、全部变异算子）：

- 典票结构化：21 个场景模板（草稿，待人工修订）
- 规则：12 条（草稿，待业务核对，目前只报“提示”）
- 合成集评测：266 张样本，检出 99.0%，换说法零误报
- 单票预审：拖一张票就出 HTML 报告和打回意见草稿

未做：PDF 输入（等内网实测，ADR-0003）、本地模型（M4.3 起）、docx 读取的 Rust 移植。
Python 实现已归档到 `archive/`，不参与构建。
