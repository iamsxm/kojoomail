#!/usr/bin/env bash
# Windows 本地构建；PowerShell 可直接运行 node scripts/build-local.mjs。
set -euo pipefail
cd "$(dirname "$0")/.."
exec node scripts/build-local.mjs x86_64-pc-windows-msvc
