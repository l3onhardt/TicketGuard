# ADR-0002 决策层接口对齐 System One 协议

- 状态：已采纳
- 日期：2026-11-05

## 背景

语义判断需要一个“有限选项 + 校准概率”的判断模型。这类模型（TypeSafe Jev 及其开源替代品）有一个事实上的通用协议：

- 输入：一段 `state`，加上若干具名的 `questions`。题型有 `noul`（是 / 否）、`choice`（单选）、`score`（有序打分）。
- 输出：每道题一个概率分布。

## 决定

`DecisionBackend.decide(state, questions) -> {name: Answer(distribution)}`，字段的语义和这个协议一致。各题互相独立；有依赖关系的题分两次调用。

## 好处

- 规则基线、llama.cpp 和蒸馏模型三种后端可以无缝替换。
- 可以直接借鉴社区的评测和校准方法。

## 注意

只是对齐协议，**不接入** TypeSafe 的云服务，也不引入社区项目的代码。
