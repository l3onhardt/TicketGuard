#!/usr/bin/env bash
# 重建沙箱用的自带工具链和依赖源码：bash tools/codex/make-toolchain.sh
#
# Codex 跑在受限沙箱里，看不到开发机用户目录下的 rustup / cargo。这个脚本把
# 1.77.2 工具链和全部依赖源码复制到仓库内的 .toolchain/（git 忽略），
# 沙箱通过 tools/codex/cargo.cmd 使用它们，完全离线。
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

export PATH="${USERPROFILE:-$HOME}/.cargo/bin:$PATH"
version=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
[ -n "$version" ] || { echo "rust-toolchain.toml 里找不到 channel"; exit 1; }

rm -rf .toolchain
mkdir -p .toolchain

echo "== 复制工具链 $version"
cp -r "${USERPROFILE:-$HOME}/.rustup/toolchains/$version" .toolchain/rust

echo "== 打包依赖源码（cargo vendor）"
mkdir -p .toolchain/vendor .toolchain/cargo-home
cargo vendor --versioned-dirs .toolchain/vendor >/dev/null

# vendor 目录要用绝对路径写进配置（沙箱里 cd 的位置不确定）
root=$(pwd -W 2>/dev/null || pwd)
cat > .toolchain/cargo-home/config.toml <<EOF
# 给沙箱里的 cargo 用：依赖一律从 .toolchain/vendor 取，不联网。
# 由 tools/codex/make-toolchain.sh 生成。
source.crates-io.replace-with = "vendored-sources"

[source.vendored-sources]
directory = "$root/.toolchain/vendor"
EOF

echo "== 自检（用沙箱的 wrapper 构建一次）"
cmd //c "$(cygpath -w tools/codex/cargo.cmd)" build --release | tail -2
echo "完成：.toolchain/ 已就绪"
