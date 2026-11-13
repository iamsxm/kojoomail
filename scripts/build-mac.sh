#!/usr/bin/env bash
# 在对应架构的 Mac 上构建，与 Actions 使用相同的 MCP sidecar 和更新签名流程。
# 默认使用 rustc 的 host target；Apple Silicon / Intel 发行包分别生成。
set -euo pipefail
cd "$(dirname "$0")/.."
exec node scripts/build-local.mjs "$@"
