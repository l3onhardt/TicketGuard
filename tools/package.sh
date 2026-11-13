#!/usr/bin/env bash
# 组装交付文件夹：bash tools/package.sh [--with-model]
#   默认只出规则版（几 MB）；--with-model 把模型程序和模型文件也放进去（约 1.4 GB）。
# 产物在 dist/票审/（git 忽略）。示例报告由刚构建的 tg.exe 现场生成，不会和程序对不上。
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

out="dist/票审"
llama="llama-b12110"
model=$(sed -n 's/.*"model": *"\([^"]*\)".*/\1/p' data/harness.json)

cargo build --release
# 每次重新组装，免得上一次的文件混进来
if [ -d "$out" ]; then
  mv "$out" "dist/票审.旧.$(date +%Y%m%d-%H%M%S)"
fi
mkdir -p "$out/data" "$out/示例报告"
cp target/release/tg.exe "$out/票审.exe"
cp -r data/rules data/templates "$out/data/"
cp data/harness.json data/accuracy.json "$out/data/"
cp -r tools/package/示例票面 "$out/"
# 记事本（尤其是旧系统的）只认 CRLF 和带 BOM 的 UTF-8
for name in 使用说明 反馈表; do
  { printf '\xef\xbb\xbf'; sed 's/$/\r/' "tools/package/$name.txt"; } > "$out/$name.txt"
done
if [ "${1:-}" = "--with-model" ]; then
  mkdir -p "$out/models/$llama"
  # 只带模型程序运行需要的文件（不带命令行聊天、训练、评测等其它工具）
  for f in llama-server.exe llama-server-impl.dll llama-common.dll llama.dll mtmd.dll ggml.dll ggml-base.dll libomp.dll; do
    cp "models/$llama/$f" "$out/models/$llama/"
  done
  cp models/$llama/ggml-cpu-*.dll models/$llama/LICENSE* "$out/models/$llama/"
  # 模型程序依赖微软 VC++ 运行库，目标电脑不一定装了，按微软允许的方式随程序放一份
  for f in msvcp140.dll vcruntime140.dll vcruntime140_1.dll; do
    cp "/c/Windows/System32/$f" "$out/models/$llama/"
  done
  cp "models/$model" "$out/models/"
fi

# 示例报告：五张编造的票，规则模式各一份；带了模型就再出模型模式各一份
export TG_NO_OPEN=1
for ticket in "$out"/示例票面/*.txt; do
  "$out/票审.exe" check "$ticket" --mode rule --out "$out/示例报告/规则模式" > /dev/null
done
if [ -d "$out/models" ]; then
  for ticket in "$out"/示例票面/*.txt; do
    "$out/票审.exe" check "$ticket" --mode model --out "$out/示例报告/模型模式" > /dev/null
  done
fi
find "$out/示例报告" -name '*.json' -delete

du -sh "$out"
ls "$out" "$out/示例报告"/*
