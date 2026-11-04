---
id: T-002
title: docx 解析保留 Wingdings 勾选框符号
milestone: M1.2（前置修复）
effort: medium
budget_src: 20
budget_tests: 20
allow:
  - src/ticketguard/ingest/office.py
  - tests/test_t002_extra.py
protect:
  - tests/test_t002_sym.py
---

## 目标

典票库 docx 里的勾选框（☑ / □）不是普通文字，而是 `<w:sym w:font="Wingdings 2" w:char="0052"/>` 这样的符号元素。现在的 `_wtext` 只收集 `<w:t>`，导致“是否办理分组工作派工单：☑是 □否”变成了“是否办理分组工作派工单：是 否”，勾的是哪一项就丢了。修复 `_wtext`，让这类符号按文档顺序输出成对应的 Unicode 字符。

## 接口

`_wtext(el)` 签名不变。公开接口 `read_docx` / `read_docx_notes` 不变。

## 规则（由规划者确定，不要改）

1. 在元素内**按文档顺序**拼接 `<w:t>` 的文字和 `<w:sym>` 的映射字符，`w:sym` 不能挪到末尾或开头。
2. 映射表（字体名精确匹配；`w:char` 按十六进制解析，若 ≥ 0xF000 则先减去 0xF000）：

| 字体 | char | 输出 |
|---|---|---|
| Wingdings 2 | 0x52 | ☑ |
| Wingdings 2 | 0xA3 | □ |
| Wingdings | 0xA8 | □ |
| Wingdings | 0xFE | ☑ |

3. 不在映射表中的符号，输出空串（与修复前的行为一致）。

## 不做

- 不加其他字体或字符的映射，不做“通用 Wingdings 码表”。
- 不改 `read_xlsx`、`blocks_to_text`。
- 不重新生成 `data/interim`（规划者会做）。

## 验收

- `tests/test_t002_sym.py`（受保护）全部通过，原有测试不退化。

## 规模参考

参考实现：一个模块级映射字典 + 改写 `_wtext` 共约 8 行。
