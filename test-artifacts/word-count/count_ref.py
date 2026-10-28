#!/usr/bin/env python3
"""N-3 中文字数统计——独立参考实现（issue #29 验收双实现路径之一）。

与 Rust 侧 `mdcore::word_count` 实现同一份口径表（见
`crates/mdcore/src/word_count.rs` 模块文档），但走**完全不同的实现路径**：
Python 正则字符类 + 显式码位区间 + 逐字符扫描，不经块树/字符分类共享代码。
两侧对同一文本统计，误差 < 0.1% 为验收线（tests/n3_word_count.rs 断言）。

口径表（与 Rust 模块文档逐条对应）：
  cjk_chars      汉字数 = CJK 统一表意 URO U+4E00..U+9FFF + 扩展 A
                 U+3400..U+4DBF（恰两段；CJK 标点/全角形式/扩展 B+ 不计）
  western_words  西文词数 = 连续 [A-Za-z0-9] 序列（下划线/撇号/连字符为分隔符）
  total_words    总字数 = cjk_chars + western_words（标书评审口径）
  total_chars    总字符数 = 全部非空白字符（str.isspace 判定）
  paragraphs     段落数（引用内段落计；列表项/代码块/标题互斥分计不计）
  headings       标题数
  tables         表格数（连续管道行合并一块）

块级解析镜像 markdown.rs v0 行文法（canonical 输入面：fence/标题/主题线/
表格/引用行/列表行/单行段落）。行内投影镜像 inline.rs 消费面子集（成对
`**`/`*`/`` ` ``/`~~`/`<u>`` 剥定界符；未闭合按字面量——本脚本接受面为
canonical 成对形，随机生成夹具不产未闭合/嵌套形，那类边界由 Rust 侧
单元矩阵钉定）。表格单元格：剥前导/尾随 `|`、未转义 `|` 分隔、`\\|`
反转义、trim；首个数据行后紧跟的 GFM 分隔行（`:?-+:?`）剔除。

用法：
  python3 count_ref.py FILE.md            # 统计到 stdout（JSON）
  python3 count_ref.py FILE.md --out F    # 同时落盘 JSON
"""

import json
import re
import sys

# ---- 口径常量（显式码位区间——与 Rust is_cjk_ideograph 双实现互证） ----
CJK_RE = re.compile("[\u3400-\u4DBF\u4E00-\u9FFF]")
WORD_RE = re.compile(r"[A-Za-z0-9]+")

# ---- 行内标记剥除（成对形；先长后短防 `**` 被 `*` 截吃） ----
INLINE_PATTERNS = [
    (re.compile(r"\*\*(.+?)\*\*"), r"\1"),   # strong
    (re.compile(r"\*(.+?)\*"), r"\1"),       # em
    (re.compile(r"~~(.+?)~~"), r"\1"),       # del（GFM）
    (re.compile(r"`([^`]+)`"), r"\1"),       # code span（内容保留）
    (re.compile(r"<u>(.+?)</u>"), r"\1"),    # 下划线标签对
]

HEADING_RE = re.compile(r"^(#{1,6}) (.*)$")
BULLET_RE = re.compile(r"^[-*+] (.*)$")
ORDERED_RE = re.compile(r"^(\d+[.)]) (.*)$")
THEMATIC = ("---", "***", "___")


def strip_inline(text: str) -> str:
    for pat, repl in INLINE_PATTERNS:
        text = pat.sub(repl, text)
    return text


def split_pipe_row(line: str):
    """单管道行 → 单元格序列（与 Rust split_pipe_row 同规）。"""
    s = line[1:] if line.startswith("|") else line
    s = s[:-1] if s.endswith("|") else s
    cells, cur, esc = [], [], False
    for ch in s:
        if esc:
            if ch == "|":
                cur.append("|")
            else:
                cur.append("\\")
                cur.append(ch)
            esc = False
        elif ch == "\\":
            esc = True
        elif ch == "|":
            cells.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    if esc:
        cur.append("\\")
    cells.append("".join(cur).strip())
    return cells


def is_separator_row(cells) -> bool:
    def one(c: str) -> bool:
        t = c.strip()
        if t.startswith(":"):
            t = t[1:]
        if t.endswith(":"):
            t = t[:-1]
        return len(t) > 0 and set(t) == {"-"}

    return len(cells) > 0 and all(one(c) for c in cells)


class Counter:
    def __init__(self):
        self.cjk_chars = 0
        self.western_words = 0
        self.total_chars = 0
        self.paragraphs = 0
        self.headings = 0
        self.tables = 0

    def add_text(self, text: str, inline: bool = True):
        """计一段净文本（inline=True 先做行内标记剥除）。"""
        t = strip_inline(text) if inline else text
        self.cjk_chars += len(CJK_RE.findall(t))
        self.western_words += len(WORD_RE.findall(t))
        self.total_chars += sum(1 for ch in t if not ch.isspace())

    def add_table(self, text: str):
        self.tables += 1
        rows_emitted = 0
        for line in text.split("\n"):
            line = line[:-1] if line.endswith("\r") else line
            if line.strip() == "":
                continue
            cells = split_pipe_row(line)
            if rows_emitted == 1 and is_separator_row(cells):
                continue
            rows_emitted += 1
            for cell in cells:
                self.add_text(cell)


def count_markdown(md: str) -> dict:
    md = md.replace("\r\n", "\n").replace("\r", "\n")
    c = Counter()
    lines = md.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            j = i + 1
            while j < len(lines) and not lines[j].startswith("```"):
                j += 1
            inner = "\n".join(lines[i + 1 : min(j, len(lines))])
            c.add_text(inner, inline=False)  # 代码块字面计入
            i = j + 1
            continue
        if line.strip() == "":
            i += 1
            continue
        m = HEADING_RE.match(line)
        if m:
            c.headings += 1
            c.add_text(m.group(2))
            i += 1
            continue
        if line.strip() in THEMATIC:
            i += 1
            continue
        if line.startswith("|"):
            j = i
            rows = []
            while j < len(lines) and lines[j].startswith("|"):
                rows.append(lines[j])
                j += 1
            c.add_table("\n".join(rows))
            i = j
            continue
        if line.startswith("> "):
            c.paragraphs += 1  # 引用行 = 引用容器内 Paragraph
            c.add_text(line[2:])
            i += 1
            continue
        m = BULLET_RE.match(line)
        if m:
            c.add_text(m.group(1))  # 列表项文本计入、不计段落
            i += 1
            continue
        m = ORDERED_RE.match(line)
        if m:
            c.add_text(m.group(2))
            i += 1
            continue
        c.paragraphs += 1
        c.add_text(line)
        i += 1
    return {
        "cjk_chars": c.cjk_chars,
        "western_words": c.western_words,
        "total_words": c.cjk_chars + c.western_words,
        "total_chars": c.total_chars,
        "paragraphs": c.paragraphs,
        "headings": c.headings,
        "tables": c.tables,
    }


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    path = argv[1]
    out_path = None
    if "--out" in argv:
        out_path = argv[argv.index("--out") + 1]
    with open(path, "r", encoding="utf-8") as f:
        md = f.read()
    result = count_markdown(md)
    text = json.dumps(result, ensure_ascii=False, indent=2)
    if out_path:
        with open(out_path, "w", encoding="utf-8") as f:
            f.write(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
