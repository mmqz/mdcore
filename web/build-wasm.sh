#!/usr/bin/env bash
# R9 — mdcore-wasm 构建脚本（可重现两步之一：Rust → WASM → JS glue）。
#
# 产物 web/pkg/（mdcore_wasm.js / mdcore_wasm_bg.wasm / .d.ts）。
# pkg/ 不入库（见 .gitignore）；wasm-bindgen CLI 版本必须与 Cargo.lock 的
# wasm-bindgen 精确一致（当前 0.2.129），否则运行期 link 失败。
#
# 用法： ./build-wasm.sh            （或 bash web/build-wasm.sh）
set -euo pipefail

cd "$(dirname "$0")/.."   # projects/prototype

export PATH="$HOME/.cargo/bin:$PATH"

echo "[1/2] cargo build -p mdcore-wasm --target wasm32-unknown-unknown --release"
cargo build -p mdcore-wasm --target wasm32-unknown-unknown --release

echo "[2/2] wasm-bindgen --target web --out-dir web/pkg"
wasm-bindgen --target web --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/mdcore_wasm.wasm

echo "ok -> web/pkg/"
