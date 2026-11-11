# 移植期对拍工具

Rust 实现是照 Python 参考实现（`src/ticketguard/`）移植的。这套脚本在移植期用来证明两边一致；
**Python 参考实现删除时，本目录一并删除。**

在仓库根目录运行（需要开发机上装有 Python 和 Rust）：

```bash
py -I tools/parity/compare.py                       # 266 张合成票，Rust 与参考实现逐字节比较报告
py -I tools/parity/compare.py target/release/tg.exe 30   # 只比前 30 张
py -I tools/parity/synth_parity.py                  # 9 个变异算子能生成的全部样本，两边集合相等
py -I tools/parity/make_fixtures.py                 # 重新导出 rust/tests/fixtures 下的基准结果
```

- `compare.py`：验收门槛是“0 处不一致”。有任何差异都要先查清原因，不允许放宽比较。
- `synth_parity.py`：随机数序列两边不同，所以比的是“每个场景每种算子能生成的全部不同票面”集合。
- `make_fixtures.py`：只用编造的票面（`tests/fixtures_d1.py`），产物可入库；改了规则、词表或
  有意改变行为时重新运行，并逐条审阅 diff。

`make_fixtures.py` 的输入是 `tests/fixtures_d1.py` 里的两张编造票面；该文件目前也服务于
Python 测试。参考实现删除后，编造票面的正本改为 `rust/tests/fixtures/tickets/*.txt`
（由 make_fixtures 导出，是同一份内容）。
