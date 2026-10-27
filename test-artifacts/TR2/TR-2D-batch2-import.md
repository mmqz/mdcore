# TR-2D 第二批：Word/Excel 生成件 → 引擎导入（本地执行）

> 环境：Windows + Office 16 COM（**真 Word/Excel 生成**）+ Playwright
> 被测：mdcore @ `51da472`
> 方法：用真 Word/Excel 构造含复杂特性的文件 → `__importDocx` / `__importXlsx` → 对账

---

## 生成的三份文件

| 文件 | 字节 | 由谁生成 | 含特性 |
|---|---|---|---|
| `word-complex.docx` | 25,767 | **真 Word COM** | 页眉页脚、批注、修订痕迹、PAGE 域、书签、多级标题、表格 |
| `word-merged.docx` | 16,825 | 真 Word COM | **纵向合并 + 横向合并**的 4×3 表格 |
| `excel-complex.xlsx` | 10,746 | **真 Excel COM** | 双工作表、SUM 公式、合并、超链接 |

---

## 发现（5 条，按严重度）

### P1-1 · 标题自动编号被重复叠加

Word 里创建的三级标题文本是 `1.1 表格`。导入后变成：

```
11.1 表格          ← 编号重复
```

**根因推测**：Word 标题样式自带 numbering（`1.1`），引擎又按层级自动补了「1」，两者拼接。
**影响**：所有用 Word 自动编号的招标文件，导入后标题全错。

### P1-2 · 标题样式识别失败（`headings: 0`）

```
stats: {"headings": 0, "paragraphs": 6, ...}
```

文档里有 `标题 1` / `标题 2` / `标题 3` 三段（Word 确认过），引擎报告 **0 个标题**，全按普通段落处理。
**影响**：导入后无法生成目录（G1 依赖标题层级）。

### P2-1 · 页眉页脚静默丢弃

```
"headers_skipped": 3, "footers_skipped": 3, "warnings": []
```

6 个部件全部跳过，**`warnings` 为空**——用户不知道丢了。
标书场景：页眉里的项目编号、页脚页码是评审项，静默丢失会被判格式不合规。

**注意**：这与 `capability-map` D3 的判断一致——引擎 `read_docx` 有 `headers/footers` 块级读面，**但导入链没消费**。

### P2-2 · 合并单元格降级为空格拼接

原始表格（Word 确认）：纵向合并 R1C1-R2C1，横向合并 R1C2-R1C3。

导入后：

```
| R1C1 R2C1 | R1C2 R1C3 |  |
|  | R2C2 | R2C3 |
```

合并区域被拼成一个单元格（`R1C1 R2C1`），但 `merged_ranges: 0` —— **合并结构完全丢失**，再导出就变成普通表格。

### P2-3 · Excel 公式压平 + 超链接丢弃

```
"formulas_flattened": 2, "hyperlinks_dropped": 1
```

`=SUM(B2:B4)` → 静态 `100`；超链接只剩文字 `招标公告`，URL 丢失。
另有一个空行 `|  |  |  |` 混进表格。

---

## 做对的部分（不能只报坏消息）

| 项 | 结果 |
|---|---|
| 三份文件全部导入成功 | ✅ 零崩溃、零 pageerror |
| 段落/列表/表格文字 | ✅ **零丢失** |
| Excel 多工作表顺序 | ✅ 正确 |
| 统计诚实性 | ✅ `headers_skipped`/`formulas_flattened`/`hyperlinks_dropped` 全部如实报告，**没有伪装** |
| 表格结构 | ✅ 行数正确（4×3 → 4 行 3 列） |

**诚实统计是本轮最值得肯定的地方**——虽然丢了东西，但每一处都登记了。
问题在于「登记了但没有警告给用户」，且部分**登记为 0 的项实际有丢失**（`merged_ranges: 0` 其实是合并不支持）。

---

## 建议修复优先级

| # | 问题 | 级别 | 估时 | 依赖 |
|---|---|---|---|---|
| 1 | 标题样式识别（`headings: 0`） | P1 | 1-2 天 | 无，`read_docx` 已有 style ID |
| 2 | 自动编号叠加 | P1 | 1 天 | #1 |
| 3 | 页眉页脚进导入链 | P2 | 1 天 | 引擎读面已有 |
| 4 | 合并单元格 | P2 | 2 天 | 需表格模型支持 |
| 5 | 公式保留/超链接 | P2 | 1 天 | — |

**#1 是根因**：标题识别失败导致层级信息丢失，后面目录、编号都建立在它上面。

---

## 证据文件

| 文件 | 内容 |
|---|---|
| `w2/word-complex.docx` | 真 Word 生成的复杂文档 |
| `w2/word-merged.docx` | 真 Word 生成的合并单元格表格 |
| `w2/excel-complex.xlsx` | 真 Excel 生成的复杂表格 |
| `w2/gen4.txt` | Word COM 生成过程日志（含 comments/revisions/fields 计数确认） |
| `w2-import-raw.json` | 导入结果原始 JSON |

复现：`python _w2_detail.py`（内含自动起服务逻辑）