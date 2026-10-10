#!/usr/bin/env bash
# compliance/scan.sh — 产品面参考对象特征词常驻门禁
#
# 扫描产品面（编译进发布产物的源面）是否出现 denylist/sources.txt 中的
# 参考对象特征词。命中即 FAIL（exit 1），禁止合入，必须人工分诊。
#
# 用法：
#   ./compliance/scan.sh            # 产品面扫描（CI / 提交前）
#   ./compliance/scan.sh --all      # 扩展扫描（含测试面 crates/*/tests + web/tests）
#
# 口径对齐：与 gates.md 中"产品面品牌词 0"门同源；与分析仓全量审计
# （compliance_fullcross，周期性）互为补充——本脚本是词法级常驻门，
# 全量审计是结构/行为级周期门。
#
# 重建声明：由分析仓审计结论重建；开发环境 R22-c 原版合入时以 diff 为准替换。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DENYLIST="$ROOT/compliance/denylist/sources.txt"
MODE="${1:-product}"

# 产品面：编译进产物的源面 + 清单
# R24-c 增补：packaging/（分发模板——发布面 = 产品面，品牌词门禁恒覆盖）
PRODUCT_PATHS=(crates/*/src web/src web/index.html Cargo.toml crates/*/Cargo.toml web/package.json packaging)
# 扩展面：测试与构建脚本（--all 时附加）
EXTRA_PATHS=(crates/*/tests web/tests web/build.mjs web/build-wasm.sh)

if [[ "$MODE" == "--all" ]]; then
  PATHS=("${PRODUCT_PATHS[@]}" "${EXTRA_PATHS[@]}")
  SCOPE="product+tests"
else
  PATHS=("${PRODUCT_PATHS[@]}")
  SCOPE="product"
fi

# 逐词构建 grep 表达式：
#  - 以 _ 结尾的词（d1_/d2_）按前缀匹配 \b<w>
#  - 其余按词边界 -w 匹配
hits=0
fail=0
while IFS= read -r word; do
  [[ -z "$word" || "$word" == \#* ]] && continue
  hits=$((hits+1))
  if [[ "$word" == *_ ]]; then
    pattern_args=(-E -e "\b${word}")
  else
    pattern_args=(-w -e "$word")
  fi
  for p in "${PATHS[@]}"; do
    # shellcheck disable=SC2086
    matched=$(grep -rIn "${pattern_args[@]}" "$p" 2>/dev/null || true)
    if [[ -n "$matched" ]]; then
      echo "DENY-HIT [$word] in $p:"
      echo "$matched" | head -20
      fail=$((fail+1))
    fi
  done
done < "$DENYLIST"

echo "----"
echo "compliance/scan.sh: scope=$SCOPE, denylist terms=$hits, hit-paths=$fail"
if [[ $fail -gt 0 ]]; then
  echo "FAIL: reference-object terms detected in product surface. Triage required."
  exit 1
fi
echo "PASS: product surface clean (0 reference-object terms)."
