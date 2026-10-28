# 域指令文本语义判据（issue #40 裁决）

> 裁决日期：2026-10-28 · 裁决人：引擎 owner · 状态：**既定语义，非缺陷**
> 关联：ai-bid-research `docs/engine-gap-requests-m0.5.md` §19 / B3-3 全量差分 4 块 UNCLASSIFIED → **改判 EXPECTED_DIFF（规则 `FIELD_CODE_NOT_TEXT`）**

## 1. 结论

**`w:instrText`（域指令文本）恒不入正文——包括「无 `separate`、无缓存结果」的退化域。**

标书侧 question 的前设——「`""` 邻接的**字面 w:t 正文**被表现型域剥离吞除」——经 XML 现场复核**不成立**：争议文本位于 `w:instrText`（域指令面），全文 `w:t`（字面正文面）零出现。引擎行为与四个独立实现面共识一致，属渲染保真的既定语义，不是缺陷。

## 2. 差异现场与 XML 真实形态

songjiang 件（`A-telecom-shanghai-songjiang-光终端放装-招标文件.docx`，生成器指纹 `WPS Office_12.1.0.29161`）4 块差异（段509/975/1211/1339），XML 现场统一形态：

```xml
<w:r><w:t>…通信工程建设项目施工</w:t></w:r>          <!-- 字面正文（保留） -->
<w:r><w:fldChar w:fldCharType="begin"/></w:r>          <!-- 无 w:dirty -->
<w:r><w:instrText xml:space="preserve">&quot;&quot;施工/服务</w:instrText></w:r>
<w:r><w:fldChar w:fldCharType="end"/></w:r>            <!-- 无 separate / 无缓存结果 -->
<w:r><w:t>框架协议》</w:t></w:r>                       <!-- 字面正文（保留） -->
```

关键事实（分面统计定谳）：

| 文本串 | `w:t` 字面面 | `w:instrText` 指令面 |
|---|---|---|
| `施工/服务` | **0** | 3（三个退化域指令） |
| `不高于` | **0** | 1（跨 run 累积指令：`""` + `不高于` + `[]的价格`） |

全文域普查：7 域 = 2 个 HYPERLINK（TOC 内链，带 separate+缓存结果——内容型，引擎**保留**其结果面）+ 5 个退化域（指令即上述正文串，无 separate 无缓存结果）。

## 3. 四独立实现面证据（2026-10-28 本机实测）

| 实现面 | 版本 | `施工/服务` | `不高于` | 行为 |
|---|---|---|---|---|
| LibreOffice 渲染（docx→PDF 提取） | 25.2.3.2 | 0 命中 / 168 页 | 0 命中 | 退化域渲染为**空**；协议名渲染为「施工框架协议」 |
| LibreOffice 导入（docx→ODT 解剖） | 25.2.3.2 | — | — | 导入为 `field:UNHANDLED` + **空结果区**（separator 与 end 之间无内容） |
| pandoc（docx→plain） | 3.1.11.1 | 0 命中 | 0 命中 | 指令文本不进输出 |
| python-docx（paragraph.text） | — | 0 命中 | 0 命中 | 仅读 `w:t` 面（与引擎同形） |
| **mdcore 引擎（本仓）** | — | 0 命中 | 0 命中 | **与上述四方一致** |

复核命令（判据可独立复验）：

```bash
soffice --headless --convert-to pdf <file>.docx && python3 -c "import fitz; t=''.join(p.get_text() for p in fitz.open('<file>.pdf')); print(t.count('施工/服务'), t.count('不高于'))"
pandoc <file>.docx -t plain | grep -c '施工/服务' || true
```

## 4. 语料旁证

- 全语料（7 docx）`""` 前缀指令退化域共 5 个（全部在 songjiang），**零个**带 separate+缓存结果——不存在任何渲染器曾把这些指令显示为文本的快照证据；
- `fldChar` 无 `w:dirty` 标记——域为已结算态（WPS 保存时认为渲染终态即空）；
- 旧实现 bid_parse 拼接 instrText 属**过度收集**：该件差分共 123 条 OLD_BUG，本 4 块是其中唯一涉及 instrText 面的争议，其余差异已由 #39（嵌套表格）等收口。

## 5. 语义判据（规则表）

| 面 | 元素 | 语义 | 引擎行为 |
|---|---|---|---|
| 字面正文面 | `w:t` | 作者可见文本 | 逐字保留（含 `""` 引号等任意字符——**永不按内容/引号形态剥离**） |
| 域指令面 | `w:instrText` | 域代码（非文本源，模块文档既有登记） | 恒不入正文（含退化域、跨 run 累积形） |
| 域结果面 | `separate`→`end` 之间 runs | 渲染期缓存快照 | 表现型域（PAGE/DATE/PAGEREF…）剥离 + 计 `fields_dropped`；内容型域（HYPERLINK/REF/TOC/QUOTE…）保留 |
| 指令面丢弃计数 | — | 指令从未可见 → 非损失面 | **不计** `fields_dropped`（该计数器只计缓存结果剥离） |

## 6. 标书侧差分改判指引

4 块（段509/975/1211/1339）改判 **EXPECTED_DIFF**，规则名建议 `FIELD_CODE_NOT_TEXT`：

```
old  = 旧实现拼接 instrText（过度收集——渲染不可见）
new  = 引擎指令面恒不入正文（渲染保真）
rule = FIELD_CODE_NOT_TEXT（old 多出的串恰为 document.xml instrText 面内容）
```

验收对照（issue #40 验收测试 2 形态）：`docs/b3-3-acceptance-report.md` §5 残留裁决表 #2-5 按此归零。

## 7. 边界与不变量（测试钉定）

钉定测试 = `crates/office-io/tests/tr31_degenerate_field.rs`（真语料探针 `--ignored` 形态内含）：

1. 退化域指令文本不入正文（单 run + 跨 run 累积两形态）；
2. 字面 `w:t` 含 `""` 引号逐字保留（剥离按**域结果区位置**判定，非内容）；
3. 指令面丢弃不进 `fields_dropped`；
4. 内容型域（HYPERLINK/TOC）缓存结果保留面**零变化**（tr14 既有钉定维持）。

若未来渲染器证据（Word/WPS 实机）推翻 §3 共识（即退化域指令在某主流渲染器上真实显示），本判据重开裁决——届时按「内容型域指令面补发射」处理，钉定测试同步翻转。
