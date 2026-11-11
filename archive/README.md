# archive/：已归档的 Python 参考实现

`src/`、`tests/`、`tools/tg.py`、`tools/parity/` 是从最初的 Python 参考实现原样搬过来的（2026-11-12）。

- **产品代码是 `rust/`**（见 `docs/decisions/0006-全栈改用Rust.md`）。本目录不参与构建、测试和门禁，也不再加功能。
- 保留它的用处：查历史行为、对照旧报告版式。移植期的对拍工具（`tools/parity/`）绑在它上面，只在需要重跑对拍时才用。
- 运行方式（需要开发机装 Python，仓库根目录执行）：
  ```bash
  py -I archive/tools/tg.py check 某张票.txt
  py -I archive/tools/parity/compare.py
  ```
  对拍脚本里的路径常量仍指向 `src/`、`data/`，用之前要先改路径（或在仓库根目录临时建一个 `src` 软链接）。
- 归档里的 `ingest/office.py` 仍是唯一能把典票 docx / xlsx 转成文本的代码。Rust 版补上 docx 读取之前，这一步继续用它。
