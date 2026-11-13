# 票审 TicketGuard

配电“两票”**离线智能预审**：对照典型票库和安规条款，自动找出工作票里的疑似问题，并给出条款依据和置信度，由审票人决定是否打回。

- 完全离线，在普通办公电脑上运行（i5 四核、8GB 内存、无 GPU，目标机器很可能是 Windows 7）
- 单个 exe，把票拖到图标上就出报告；不用装任何东西
- 两种模式：**规则模式**只用规则，结果完全确定；**模型模式**在规则之后请本机小模型复核“疑似缺少措施”的事项，只能把疑点降为提示，不能增删发现（ADR-0008）
- AI 只做预审建议，不自动打回，也不自动认定违章

## 快速开始

```bash
cargo build --release          # 构建 target/release/tg.exe

# 预审一张票：输入票面文本 .txt，报告（HTML + JSON）写到 reports/
target/release/tg.exe check 某张票.txt

# 也可以把 .txt 直接拖到 tg.exe 的图标上：报告生成在票的旁边，并自动用浏览器打开

# 指定模式：rule 只用规则；model 规则之后请本机模型复核；auto（默认）旁边有 models/ 就用模型
target/release/tg.exe check 某张票.txt --mode model

# 组装给审票人试用的文件夹 dist/票审/（加 --with-model 带上模型，约 1.3 GB）
bash tools/package.sh --with-model
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
- 单票预审：拖一张票就出 HTML 报告和打回意见草稿；报告页带反馈区，一键复制成不含票面内容的文字
- 模型模式（harness h1，Qwen3.5-2B Q4）：在编造的题集上，规则的换说法误报救回两到四成，真缺项放走 0/336；设置全在 `data/harness.json`。Windows 7 上跑不了，只有规则模式
- 试用包：`tools/package.sh`，含两个模式的批处理、使用说明、反馈表、五张编造的示例票

未做：PDF 输入（等内网实测，ADR-0003）、docx 读取的 Rust 移植、模型模式评测进 `tg eval`。**还没有在真实票上试过**，下一步等试用反馈。
Python 实现已归档到 `archive/`，不参与构建。
