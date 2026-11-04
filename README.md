# 票审 TicketGuard

配电“两票”**离线智能预审**：对照典型票库和安规条款，自动找出工作票里的疑似问题，并给出条款依据和置信度，由审票人决定是否打回。

- 完全离线，在普通办公电脑上运行（i5 四核、8GB 内存、无 GPU）
- “规则优先，模型兜底”：能写成规则的用规则判断，只有语义模糊的部分才交给本地小模型（类 Jev 的判断模型）
- AI 只做预审建议，不自动打回，也不自动认定违章

## 快速开始

```bash
py -I tools/tg.py ingest data/raw data/interim   # 把典票库 docx / xlsx 解析成文本
py -I -m unittest discover -s tests -v           # 运行测试
```

## 文档

先读 [AGENTS.md](AGENTS.md)，再按编号读 [docs/](docs/)，从 [01-需求与定位](docs/01-需求与定位.md) 开始。
进度见 [08-开发计划](docs/08-开发计划.md)。

## 当前状态

M0（工作区与文档）已完成，下一步是 M1（典票库结构化）。
