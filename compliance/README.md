# compliance/ — 产品面合规常驻门禁

本目录是**随码走的合规工具**（COMPLIANCE.md §5：工具随产品代码首推入 main）。
结论性文档与发布形态套件在 `compliance-baseline` 分支（COMPLIANCE.md +
compliance_release/ 三件套），两处对账关系见下。

## 内容

| 文件 | 作用 |
|---|---|
| `scan.sh` | 产品面参考对象特征词常驻门禁。CI 与每次提交前运行；命中即 FAIL。 |
| `denylist/sources.txt` | 门禁字集：四源（Typora/Delta/Zed/Obsidian）品牌词与特有标识符。 |

## 运行

```bash
./compliance/scan.sh          # 产品面（crates/*/src + web/src + 清单）
./compliance/scan.sh --all    # 扩展（附加测试面与构建脚本）
```

PASS 判据：`0 reference-object terms`（exit 0）。任何命中都必须人工分诊：
确认为参考对象痕迹 → 删除或改名；确认为合法独立用词 → 在本目录 README
登记书面豁免（词 + 理由 + 日期）后再放行。

## 与合规体系的对账关系

```text
分析仓 typora-re-analysis（私有，允许域）
  ├─ compliance_rounds/      三层审计底稿（标识符/字面量/结构-行为）——只存分析仓
  ├─ compliance_fullcross/   四源全量交叉审计（结构/行为级，周期性）
  └─ compliance_behavior/    行为级审计（24 项）
        │ 审计结论（词表特征）重建 ↓（底稿本身不出分析仓）
产品仓本目录（词法级常驻门）
  └─ scan.sh + denylist/     每次提交强制执行
产品仓 compliance-baseline 分支（结论声明层）
  ├─ COMPLIANCE.md           姿态/审计终态/常驻门禁/分支政策
  └─ compliance_release/     THIRD-PARTY-NOTICES / ASSET_CHECK / TRADEMARK_RULES
```

## 重建声明

本目录工具由分析仓四源审计**结论**重建（2026-10-10，main 首推时），非底稿
拷贝。开发环境 R22-c 建立的原版工具合入时，以 diff 为准替换，diff 即差异
清单；两版字集语义冲突时以更严格者为准。

## 已登记豁免

（暂无。首次豁免登记时删除本行并按下表格式追加。）

| 词 | 理由 | 登记日期 |
|---|---|---|
