"""取数接口契约。

取数方式（抓包、打印成 PDF、粘贴、系统接口）与解析解耦：各取数方式只负责产出
下面约定的“票面文本”，解析层只认这个格式。外部已有的取数方案把结果适配成
`TicketSource.to_ticket_text()` 的返回值即可接入，后续替换取数方式不影响下游。

票面文本约定：
  - 纯文本，UTF-8，一个栏目或一个表格单元格占一行；
  - 保留栏目标签原文（如“工作地点或设备”“工作班人员”），解析层按标签定位字段；
  - 表格用单独一行 "[TABLE]" / "[/TABLE]" 包裹，单元格内换行写成 " / "。
  （与 ingest.blocks_to_text 的输出保持一致。）
"""

from __future__ import annotations

from typing import Protocol


class TicketSource(Protocol):
    """一个取数来源。实现方只需把原始数据转成票面文本。"""

    def to_ticket_text(self) -> str:
        ...
