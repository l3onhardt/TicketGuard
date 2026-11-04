#!/usr/bin/env bash
# 派发任务给 Codex 并跑门禁：bash tools/codex/dispatch.sh T-001
# 返工：bash tools/codex/dispatch.sh T-001 .codex-runs/T-001/review.md （带上评审意见，续接原会话）
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

id="$1"; review="${2:-}"
card=$(ls docs/tasks/"$id"-*.md)
run=".codex-runs/$id"; mkdir -p "$run"
effort=$(sed -n 's/^effort:[[:space:]]*//p' "$card" | head -1)

if [ -z "$review" ]; then
  git rev-parse --verify -q HEAD >/dev/null || { echo "仓库还没有任何提交，先提交基线再派发"; exit 1; }
  [ -z "$(git status --porcelain)" ] || { echo "工作区不干净，先提交或清理再派发"; exit 1; }
  git rev-parse HEAD > "$run/base"
  { cat tools/codex/prompt_template.md; printf '\n---- 任务卡 %s ----\n' "$card"; cat "$card"; } > "$run/prompt.md"
  codex exec -C . -s workspace-write -m gpt-6.1-sol -c model_reasoning_effort="\"${effort:-high}\"" \
    --output-schema tools/codex/report.schema.json -o "$run/report.json" --json - \
    < "$run/prompt.md" > "$run/events.jsonl"
else
  thread=$(grep -o '"thread_id":"[^"]*"' "$run/events.jsonl" | head -1 | cut -d'"' -f4)
  [ -n "$thread" ] || { echo "events.jsonl 里找不到 thread_id，无法续接"; exit 1; }
  codex exec resume "$thread" -c model_reasoning_effort="\"${effort:-high}\"" \
    --output-schema tools/codex/report.schema.json -o "$run/report.json" --json - \
    < "$review" >> "$run/events.jsonl"
fi

echo "== Codex 汇报"; cat "$run/report.json"; echo
py -I tools/gate.py "$id"
