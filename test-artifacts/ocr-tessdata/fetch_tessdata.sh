#!/usr/bin/env bash
# OCR 语言包取件（版本 pin + SHA256 验证——pdfium 取件脚本同纪律）
set -euo pipefail
cd "$(dirname "$0")"
PIN_URL="https://github.com/tesseract-ocr/tessdata_fast/raw/main/chi_sim.traineddata"
SHA256_PIN="a5fcb6f0db1e1d6d8522f39db4e848f05984669172e584e8d76b6b3141e1f730"
echo "[fetch] ${PIN_URL}"
curl -fL "${PIN_URL}" -o chi_sim.traineddata
SHA256_GOT="$(sha256sum chi_sim.traineddata | cut -d' ' -f1)"
echo "[sha256] got=${SHA256_GOT}"
# 注：上游 fast 构件滚动更新——SHA 以取件当日实测为准回填下方后提交。
if [ -n "${SHA256_PIN:-}" ] && [ "${SHA256_GOT}" != "${SHA256_PIN}" ]; then
  echo "FAIL: sha256 与 pin 不符（期望 ${SHA256_PIN}）" >&2; exit 1
fi
# 系统 eng/osd 复制（恒定面——tesseract 发行包自带）。
for lang in eng osd; do
  SRC="/usr/share/tesseract-ocr/5/tessdata/${lang}.traineddata"
  [ -f "${lang}.traineddata" ] || cp "${SRC}" .
done
echo "[done] $(ls *.traineddata)"
