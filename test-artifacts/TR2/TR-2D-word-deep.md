# TR-2D · Windows 真 Word 互操作深挖（本地执行）

> 环境：Windows + Microsoft Office 16（COM）+ Playwright/Chrome + PyMuPDF
> 被测：mdcore @ `51da472`（本地 checkout `tr2-local`），web dist 本地构建
> 日期：2026-10-26

---

## 结论摘要

| # | 用例 | 结果 | 级别 |
|---|---|---|---|
| W1 | 导出 docx 字体/字号 | ❌ 全部 10pt Times New Roman | P1 |
| W2 | 标题真样式 | ✅ `标题 1/2/3` + OutlineLevel 1/2/3 | — |
| W3 | TOC 域 | ❌ `TOC=0 fields=0` | P1 (G1) |
| W4 | **行内粗体/斜体** | ❌ **星号原样输出** | **P0** |
| W5 | 页边距 | ❌ 写死 3.5/3/3/3 cm | P1 (G2) |
| W6 | Word 打开 | ✅ 正常打开，1 页 77 字 | — |
| W7 | 表格结构 | ✅ 3 行 2 列 | — |
| W8 | 列表 | ✅ ListFormat 真列表 | — |

---

## 一、最关键发现：G2「已实装」对产品导出链无效

云端 #10 称：

> G2（`3f16779`）已供 `DocxWriter::default_typography` 与 `page_geometry`，LO 跨层实测精确生效……**但 web 导出链未消费**

本地实测**确认了后半句，并量化了后果**：

```
window 导出 API 探测:
  __exportDocx        ✅ 存在
  __exportDocxStyled  ❌ 不存在（TR-2C 未实现）

导出 docx 在真 Word 里:
  Heading1: size=10 bold=0 font=Times New Roman   ← G2 完全没生效
  Normal  : size=10        font=Times New Roman   ← G2 完全没生效
  margins : T3.5 B3 L3 R3 cm                      ← 写死值
```

**判定**：这不是「G2 做完了」，是「**引擎 API 有了，产品链没接**」。
从用户视角，字体/字号/行距/边距**一个都不能设**——和一个月前我测的结果完全一样。

---

## 二、P0 复现：行内标记原样进 Word

输入 markdown：

```markdown
本项目含**智能文档平台**与*需求解析*两项内容。
总计人民币 **壹佰贰拾万元整**。
```

真 Word 打开后：

```
[3]  style=正文 | 本项目含**智能文档平台**与*需求解析*两项内容。
[14] style=正文 | 总计人民币 **壹佰贰拾万元整**。
```

**星号原样输出，没有加粗、没有斜体。**

为什么是 P0：标书报价页写 `**壹佰贰拾万元整**`，评标专家看到的是带星号的文本——
格式不合规，属于废标风险。而且这是**用户完全无法察觉**的错误（编辑器里显示是正常的加粗）。

云端 #8 未发现这条，因为它用 LibreOffice 验证的是**导入**，没验证**导出的行内 marks**。

---

## 三、仍然正常的部分（不能因为找 bug 就忽略）

| 能力 | 实测 |
|---|---|
| 标题真样式 | `标题 1/2/3`，`OutlineLevel=1/2/3`——导航窗格可用、能生成目录 |
| 表格 | 真 `w:tbl`，`cell(1,1)=阶段` |
| 列表 | `ListType=4`，真 Word 列表（不是手打圆点） |
| Word 兼容性 | 直接打开无修复提示、无报错 |
| 导出确定性 | 26,594 字节，产物结构完整 |

**结构层是好的，排版层是空的。** 这个判断从我第一轮审计到现在没有变。

---

## 四、对云端结论的修正

| 云端说法 | 本地实测 |
|---|---|
| 「零 P0」(Linux+LO) | **P0 至少 1 条**（粗体丢失，Windows+Word） |
| 「G2 已实装」(#10) | 引擎 API 已实装，**产品导出链未消费**，用户视角等于没做 |
| 「导出面已验证无污染」(#9) | docx 段落文本确实干净，但**消失了 marks**——干净不等于正确 |

**建议**：`TEST-ROUND-REPORT.md` 的结论表必须分环境列出，且增加一行「产品可用性」维度——
「API 存在」和「用户能用」是两回事。

---

## 五、证据文件

| 文件 | 内容 |
|---|---|
| `tr2-export.docx` | 本次导出产物（26,594 字节） |
| `_tr2d_word.txt` | Word COM 逐段实测原始输出 |
| `_tr2d_export.py` | 导出脚本（Playwright，可复现） |
| `_tr2d_word.ps1` | Word COM 验证脚本 |

复现：
```powershell
# 1) 本地起服务
cd mdcore\web; python -m http.server 8123 --directory dist
# 2) python _tr2d_export.py   （注入 + 点导出按钮）
# 3) powershell -File _tr2d_word.ps1   （Word COM 逐段取证）
```