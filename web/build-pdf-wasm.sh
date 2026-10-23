#!/usr/bin/env bash
# 阶段5 §八.4 — mdc-pdf 第二 wasm 包构建（web/pkg_pdf/，与 mdcore-wasm
# 的 web/pkg/ 并列——PDF 视图的模型面：pdfOpenScan + WasmPdfSession）。
#
# 产物 web/pkg_pdf/（pdf_wasm.js + pdf_wasm_bg.wasm + .d.ts——glue 文件
# 名由 wasm-bindgen --out-name pdf_wasm 决定，桥契约 = pdf_wasm.js）。
#
# 前置（主代理并行实施中）：
#   - workspace 成员 crate mdc-pdf-wasm（crates/mdc-pdf-wasm/）
#   - wasm-bindgen CLI 版本与 Cargo.lock 精确一致（同 build-wasm.sh 纪律）
#
# crate 未落地时本脚本**不失败**（exit 0 + 说明）——pdf_view.ts 的懒加
# 载会进 display-only 降级态（探针 __pdfState().wasm = false，e2e SKIP
# 门），构建管线不受阻。
set -euo pipefail

cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"

WASM_CRATE="mdc-pdf-wasm"

# crate 在场性检查（workspace members 元数据——缺席即降级退出）
if ! cargo metadata --no-deps --format-version 1 2>/dev/null \
    | grep -q "\"name\":\"${WASM_CRATE}\""; then
  echo "skip: ${WASM_CRATE} crate absent (parallel wasm work pending)"
  echo "      pdf_view runs display-only until it lands (web/pkg_pdf/ expected)"
  exit 0
fi

echo "[1/2] cargo build -p ${WASM_CRATE} --target wasm32-unknown-unknown --release"
cargo build -p "${WASM_CRATE}" --target wasm32-unknown-unknown --release

# cdylib 工件定位：[lib] name 未设 → 工件名 = crate 名下划线化
# （mdc_pdf_wasm.wasm）；glue 文件名由下方 --out-name pdf_wasm 决定。
TARGET_DIR="target/wasm32-unknown-unknown/release"
WASM_FILE="${TARGET_DIR}/mdc_pdf_wasm.wasm"
if [ ! -f "${WASM_FILE}" ]; then
  echo "error: cdylib artifact not found under ${TARGET_DIR}/ (expected pdf_wasm.wasm)"
  echo "       check [lib] name in crates/mdc-pdf-wasm/Cargo.toml"
  exit 1
fi

echo "[2/2] wasm-bindgen --target web --out-dir web/pkg_pdf"
wasm-bindgen --target web --out-name pdf_wasm --out-dir web/pkg_pdf "${WASM_FILE}"

if [ ! -f web/pkg_pdf/pdf_wasm.js ]; then
  echo "error: web/pkg_pdf/pdf_wasm.js not produced — glue name mismatch"
  echo "       (bridge contract expects pdf_wasm.js; fix [lib] name and rebuild)"
  ls web/pkg_pdf/ || true
  exit 1
fi

echo "ok -> web/pkg_pdf/ (rebuild web dist with: cd web && npm run build)"
