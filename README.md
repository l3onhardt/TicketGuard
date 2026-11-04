# 票审 TicketGuard

配电“两票”**离线智能预审**：对照典型票库和安规条款，自动找出工作票里的疑似问题，并给出条款依据和置信度，由审票人决定是否打回。

- 完全离线，在普通办公电脑上运行（i5 四核、8GB 内存、无 GPU）
- “规则优先，模型兜底”：能写成规则的用规则判断，只有语义模糊的部分才交给本地小模型（类 Jev 的判断模型）
- AI 只做预审建议，不自动打回，也不自动认定违章

## 快速开始

```bash
# 1. 解析典票库（只需做一次；典票库原件放在 data/raw，不入库）
py -I tools/tg.py ingest data/raw data/interim

# 2. 预审一张票：输入票面文本 .txt 或 .docx，报告写到 reports/（双击 .html 打开）
py -I tools/tg.py check 某张票.txt

# 3. 开发用：生成合成样本并评测检出率
py -I tools/tg.py synth "data/interim/<典票库文本>.txt"
py -I tools/tg.py eval  "data/interim/<典票库文本>.txt"

py -I -m unittest discover -s tests -v           # 运行测试
```

## 文档

先读 [AGENTS.md](AGENTS.md)，再按编号读 [docs/](docs/)，从 [01-需求与定位](docs/01-需求与定位.md) 开始。
进度见 [08-开发计划](docs/08-开发计划.md)。

## 当前状态

规则基线可用（只覆盖配电第一种工作票）：

- 典票结构化：21 个场景模板（草稿，待人工修订）
- 规则：12 条（草稿，待业务核对，目前只报“提示”）
- 合成集评测：266 张样本，检出 99.0%（95% CI 96.6%–99.7%），换说法零误报
- 单票预审：`check` 命令，输出 JSON / HTML 报告和打回意见草稿

未做：PDF 输入（等内网实测，ADR-0003）、本地模型（M4.3 起，需要模型文件）、打包成 exe（M6）。
