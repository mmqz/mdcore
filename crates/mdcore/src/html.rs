//! HTML <-> Block Tree —— Text Stream 族边界编解码器（主档 §7 三层关系 / §18 转换策略）。
//!
//! DM（BlockTree）是唯一语义真理（§9）：HTML 与 Markdown 一样只是边界表示，
//! 本模块不引入第二个模型；`BlockKind` / `MarkSpan` / `Attrs` 零改动。
//!
//! # 支持子集（块级）
//!
//! | HTML | DM | 备注 |
//! |---|---|---|
//! | `p` | Paragraph | 行内子集见下 |
//! | `h1`..`h6` | Heading(attrs.level) | 其余 h* 按未知容器降级 |
//! | `ul`/`ol`/`li`（含嵌套） | ListItem(attrs.marker/depth) | 无 List 容器块型——与 markdown.rs 同构：li 直接嵌套；ol marker 由 `start` 属性推得（默认 `1.`，逐项递增），ul 统一 `-` |
//! | `blockquote`（含嵌套） | Blockquote | 子内容按块递归 |
//! | `pre`>``code`` | CodeBlock(attrs.lang) | 内容**逐字保留**（仅实体解码 + 按 HTML5 规则剥一个前导换行）；lang 取 `class="language-x"` / `lang-x` |
//! | `math-block` | Math | #261：LaTeX 显示公式源码**逐字保留**（仅实体解码，不剥换行——机器格式无 HTML5 前导换行惯例）；内部行内标签丢弃留痕（pre 同构） |
//! | `table`（thead/tbody/tr/th/td） | Table | 内容 = 规范管道行文本（表头行 + `---` 分隔行 + 体行），与 markdown.rs 的 Table 文本同构；单元格内 `\|` 转义 |
//! | `hr` | ThematicBreak | |
//!
//! # 支持子集（行内）
//!
//! `strong`/`b`→mark `strong`，`em`/`i`→`em`，`code`→`code`，`del`/`s`/`strike`→`del`，
//! `u`→`u`，`math`→`math`（#261：LaTeX 行内公式，内容逐字；野生 MathML
//! `<math>` 元素同词汇导入——文本内容扁平化入 mark，结构丢弃）。以上均为
//! op.rs `SetMark` 的自由 kind 字符串，无新模型。`br`→文本内换行 `\n`
//! （导出时还原为 `<br>`）。其余行内标签（span/a/font/未知标签）**透明**：文字保留、
//! 样式/链接丢弃；带属性时记一条 Dropped Loss（裸标签静默，避免刷屏）。
//!
//! # 降级规则（导入）
//!
//! - **未知/分节容器**（div/section/article/main/…）→ 透明穿轴：子内容按块解析，
//!   记 `DemotedToParagraph`。
//! - **嵌入/脚本类**（script/style/iframe/object/video/audio/canvas/svg/template/…）→
//!   整体跳过，记 `Dropped`（主档 §18：PDF/Canvas 类不可转换内容"作为嵌入对象挂载，
//!   不强行转换"——v0 无嵌入块型，先丢弃并留痕）。
//! - **表单控件**（button/select/textarea/input/meta/link）→ 跳过，记 `Dropped`。
//! - **html/body/head 外壳** → 静默透明；注释/DOCTYPE 由 tokenizer 跳过。
//! - **退化**（如无单元格的 `<table>`、超深嵌套 >256 层）→ 原文落为段落，记 `RawBlock`。
//! - 不规范输入不 panic：所有循环 EOF 有界；未闭合标签就地恢复。
//!
//! # 已知有损点（§18 Loss Report 覆盖之外的登记）
//!
//! 1. 行内空白按 HTML 语义折叠（连续空白→单空格、块缘裁剪）——canonical 内容
//!    （无连续空白）round-trip 恒等，非规范文本是规范化而非保真。
//! 2. 表格单元格行内 mark 不可承载（Table 文本 = 管道行），导入记 Dropped；
//!    对齐信息（`:---:`）同样不保留。
//! 3. li 内多段落折叠进条目文本（空格连接）；blockquote/table/pre 等 li 子块保留。
//! 4. 同层混排 ul/ol 相邻条目共享一个列表标签（按首项 marker 决定）；`*/+` marker
//!    差异 HTML 不可表达。
//! 5. 导出仅认识 strong/em/code/math/del/u 六种 mark；其余 kind 不导出（DM→MD 同样
//!    不写 mark，方向一致）。math 导出为专用 `<math>` 标签（标签即锚点，
//!    textContent 即 LaTeX 源——消费方 `katex.render(el.textContent, el)` 直接可得）。
//! 6. 同 kind 重叠/交叉的 mark 导出时按活跃集归一化（可能合并相邻 span）。
//!
//! # API
//!
//! - [`import_html`]：树 + 结构化 Loss Report（位置 / 原结构 / 处置 / 说明）。
//! - [`html_to_tree`]：只要树的便捷入口（与 markdown_to_tree 同构）。
//! - [`tree_to_html`]：语义 HTML 导出（fragment，无 html/head；全文本转义
//!   `& < > " '`；属性最小化：仅 code 的 class 与 ol 的 start）。

use crate::tree::{Attrs, BlockKind, BlockTree, MarkSpan, NodeId};

// ---------------------------------------------------------------------------
// 公共类型：§18 结构化 Loss Report
// ---------------------------------------------------------------------------

/// 无法映射结构的处置方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LossDisposition {
    /// 结构降级：内容按段落/透明容器尽力保留。
    DemotedToParagraph,
    /// 内容整体丢弃（脚本/嵌入对象/表单控件——DM 无对应块型）。
    Dropped,
    /// 结构不可恢复：原文（可见文本）落为普通段落。
    RawBlock,
}

impl LossDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            LossDisposition::DemotedToParagraph => "demoted_to_paragraph",
            LossDisposition::Dropped => "dropped",
            LossDisposition::RawBlock => "raw_block",
        }
    }
}

/// 单条有损记录：让 AI 与 UI 理解转换损失（主档 §18）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// 输入 HTML 中该结构起始处的 char offset。
    pub offset: usize,
    /// 原结构（如 `<div class="toc">`，属性值截断）。
    pub original: String,
    pub disposition: LossDisposition,
    /// 人/AI 可读的处置说明。
    pub note: String,
}

/// `import_html` 的返回：块树 + 有损报告。
#[derive(Clone, Debug)]
pub struct HtmlImport {
    pub tree: BlockTree,
    pub losses: Vec<Loss>,
}

// ---------------------------------------------------------------------------
// 对外 API
// ---------------------------------------------------------------------------

/// HTML → BlockTree（丢弃 Loss 的便捷入口；与 `markdown_to_tree` 同构）。
pub fn html_to_tree(html: &str) -> BlockTree {
    import_html(html).tree
}

/// HTML → BlockTree + 结构化 Loss Report。
pub fn import_html(html: &str) -> HtmlImport {
    let toks = tokenize(html);
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        losses: Vec::new(),
        depth: 0,
    };
    let nodes = p.parse_blocks(&[], 0);
    let mut tree = BlockTree::new();
    let root = tree.root();
    materialize(nodes, &mut tree, root);
    HtmlImport {
        tree,
        losses: p.losses,
    }
}

/// BlockTree → 语义 HTML fragment（不含 `<html>`/`<head>`；全文本转义）。
pub fn tree_to_html(tree: &BlockTree) -> String {
    let mut out = String::new();
    render_blocks(tree, tree.top_level(), &mut out);
    out
}

// ---------------------------------------------------------------------------
// 解析：tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// 原始文本（未实体解码；start = 输入 char offset）。
    Text {
        raw: String,
        start: usize,
    },
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        self_close: bool,
        start: usize,
    },
    Close {
        name: String,
        start: usize,
    },
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// script/style 为 RAWTEXT：内容（可能含 `<`/`</x>`）整段收进一个 Text。
fn raw_text_element(name: &str) -> bool {
    name == "script" || name == "style"
}

fn tokenize(input: &str) -> Vec<Tok> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0usize; // byte offset
    let mut coff = 0usize; // char offset
    let mut tstart: Option<usize> = None; // 当前文本 run 的 byte 起点

    let flush = |toks: &mut Vec<Tok>, tstart: &mut Option<usize>, end: usize, coff: usize| {
        if let Some(s) = *tstart {
            if end > s {
                toks.push(Tok::Text {
                    raw: input[s..end].to_string(),
                    start: coff - input[s..end].chars().count(),
                });
            }
            *tstart = None;
        }
    };

    while i < len {
        let markup = bytes[i] == b'<'
            && i + 1 < len
            && (bytes[i + 1].is_ascii_alphabetic()
                || bytes[i + 1] == b'/'
                || bytes[i + 1] == b'!'
                || bytes[i + 1] == b'?');
        if !markup {
            if tstart.is_none() {
                tstart = Some(i);
            }
            // coff 不变量：coff == input[..i] 的 char 数——正文文本同样推进，
            // 否则 flush 的 `coff - run 字符数` 会下溢（文本 char 从未入账）。
            let ch = input[i..].chars().next().unwrap();
            coff += 1;
            i += ch.len_utf8();
            continue;
        }
        flush(&mut toks, &mut tstart, i, coff);
        if input[i..].starts_with("<!--") {
            let skip = match input[i + 4..].find("-->") {
                Some(p) => i + 4 + p + 3,
                None => len,
            };
            coff += input[i..skip].chars().count();
            i = skip;
        } else if input[i..].starts_with("<!") || input[i..].starts_with("<?") {
            let skip = match bytes[i..].iter().position(|&b| b == b'>') {
                Some(p) => i + p + 1,
                None => len,
            };
            coff += input[i..skip].chars().count();
            i = skip;
        } else if input[i..].starts_with("</") {
            let rest = &input[i + 2..];
            let nb = rest
                .bytes()
                .position(|b| !(b.is_ascii_alphanumeric() || b == b'-' || b == b':'))
                .unwrap_or(rest.len());
            let name = rest[..nb].to_ascii_lowercase();
            let end = match bytes[i..].iter().position(|&b| b == b'>') {
                Some(p) => i + p + 1,
                None => len,
            };
            toks.push(Tok::Close { name, start: coff });
            coff += input[i..end].chars().count();
            i = end;
        } else {
            // open tag
            let rest = &input[i + 1..];
            let nb = rest
                .bytes()
                .position(|b| !(b.is_ascii_alphanumeric() || b == b'-' || b == b':'))
                .unwrap_or(rest.len());
            let name = rest[..nb].to_ascii_lowercase();
            let mut j = i + 1 + nb;
            let mut attrs: Vec<(String, String)> = Vec::new();
            let mut self_close = false;
            while j < len {
                while j < len && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= len {
                    break;
                }
                if bytes[j] == b'>' {
                    j += 1;
                    break;
                }
                if bytes[j] == b'/' {
                    self_close = true;
                    j += 1;
                    continue;
                }
                let astart = j;
                while j < len
                    && !bytes[j].is_ascii_whitespace()
                    && bytes[j] != b'='
                    && bytes[j] != b'>'
                    && bytes[j] != b'/'
                {
                    j += 1;
                }
                let aname = input[astart..j].to_ascii_lowercase();
                let mut aval = String::new();
                let mut k = j;
                while k < len && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < len && bytes[k] == b'=' {
                    k += 1;
                    while k < len && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if k < len && (bytes[k] == b'"' || bytes[k] == b'\'') {
                        let q = bytes[k];
                        k += 1;
                        let vs = k;
                        while k < len && bytes[k] != q {
                            k += 1;
                        }
                        aval = decode_entities(&input[vs..k.min(len)]);
                        if k < len {
                            k += 1;
                        }
                    } else {
                        let vs = k;
                        while k < len && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                            k += 1;
                        }
                        aval = decode_entities(&input[vs..k]);
                    }
                    j = k;
                }
                if !aname.is_empty() {
                    attrs.push((aname, aval));
                }
            }
            let tag_end = j;
            toks.push(Tok::Open {
                name: name.clone(),
                attrs,
                self_close,
                start: coff,
            });
            coff += input[i..tag_end].chars().count();
            i = tag_end;
            if raw_text_element(&name) {
                // RAWTEXT：到对应的 </script|style> 为止整段是文本（不解码实体）。
                let pat = format!("</{}", name);
                let close = find_ci(&bytes[i..], pat.as_bytes()).map(|p| i + p);
                match close {
                    Some(abs) => {
                        if abs > i {
                            toks.push(Tok::Text {
                                raw: input[i..abs].to_string(),
                                start: coff,
                            });
                            coff += input[i..abs].chars().count();
                        }
                        let end = match bytes[abs..].iter().position(|&b| b == b'>') {
                            Some(p) => abs + p + 1,
                            None => len,
                        };
                        toks.push(Tok::Close { name, start: coff });
                        coff += input[abs..end].chars().count();
                        i = end;
                    }
                    None => {
                        if len > i {
                            toks.push(Tok::Text {
                                raw: input[i..len].to_string(),
                                start: coff,
                            });
                            coff += input[i..len].chars().count();
                        }
                        toks.push(Tok::Close { name, start: coff });
                        i = len;
                    }
                }
            }
        }
    }
    flush(&mut toks, &mut tstart, len, coff);
    toks
}

fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&k| hay[k..k + needle.len()].eq_ignore_ascii_case(needle))
}

// ---------------------------------------------------------------------------
// 解析：实体解码 / 空白折叠
// ---------------------------------------------------------------------------

/// 最小实体表：amp/lt/gt/quot/apos/nbsp + 数字（十进制/十六进制）。
/// 未知实体原样保留（不吞内容）。
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let limit = chars.len().min(i + 12);
            if let Some(semi) = (i + 1..limit).find(|&j| chars[j] == ';') {
                let name: String = chars[i + 1..semi].iter().collect();
                let rep = match name.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    other => {
                        let num = if let Some(hex) = other
                            .strip_prefix("#x")
                            .or_else(|| other.strip_prefix("#X"))
                        {
                            u32::from_str_radix(hex, 16).ok()
                        } else {
                            other.strip_prefix('#').and_then(|d| d.parse::<u32>().ok())
                        };
                        num.and_then(char::from_u32)
                    }
                };
                if let Some(c) = rep {
                    out.push(c);
                    i = semi + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// HTML 行内空白语义：连续 [ \t\r\n] 折叠为单个空格。
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws {
                out.push(' ');
                in_ws = false;
            }
            out.push(c);
        }
    }
    if in_ws {
        out.push(' ');
    }
    out
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 解析：块递归下降
// ---------------------------------------------------------------------------

/// 结构化解析的块标签。
const HANDLED: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "blockquote",
    "pre",
    "math-block",
    "table",
    "hr",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
];

/// 嵌入/脚本/元数据：整体跳过（Dropped）。
const EMBED_SKIP: &[&str] = &[
    "script", "style", "iframe", "object", "video", "audio", "canvas", "svg", "math", "template",
    "noscript", "title",
];

/// 文档外壳：静默透明。
const SCAFFOLD: &[&str] = &["html", "body", "head"];

/// void 且无内容价值：跳过（Dropped）。
const VOID_DROP: &[&str] = &["meta", "link", "input"];

/// 表单控件：整体跳过（Dropped）。
const FORM_SKIP: &[&str] = &[
    "button", "select", "option", "textarea", "datalist", "output",
];

/// 未知/分节容器：透明穿轴（DemotedToParagraph）。
const TRANSPARENT: &[&str] = &[
    "div",
    "main",
    "section",
    "article",
    "aside",
    "nav",
    "header",
    "footer",
    "figure",
    "figcaption",
    "center",
    "address",
    "dialog",
    "details",
    "summary",
    "dl",
    "dt",
    "dd",
    "form",
    "fieldset",
    "legend",
    "hgroup",
    "search",
    "menu",
];

/// 行内 mark 标签 → kind。
fn mark_kind_of(tag: &str) -> Option<&'static str> {
    match tag {
        "strong" | "b" => Some("strong"),
        "em" | "i" => Some("em"),
        "code" => Some("code"),
        "del" | "s" | "strike" => Some("del"),
        "u" => Some("u"),
        // #261：LaTeX 行内公式（专用标签词汇——与导出 `<math>` 同构；
        // 野生 MathML 同名元素经此扁平化为 math mark，文本保留结构丢弃）。
        "math" => Some("math"),
        _ => None,
    }
}

/// 行内上下文可停止的块级边界（parse_inline 遇到即返回）。
fn is_inline_stop(name: &str) -> bool {
    HANDLED.contains(&name)
        || EMBED_SKIP.contains(&name)
        || SCAFFOLD.contains(&name)
        || VOID_DROP.contains(&name)
        || FORM_SKIP.contains(&name)
        || TRANSPARENT.contains(&name)
}

const MAX_DEPTH: usize = 256;

/// 解析中间产物（物化为 BlockTree 前的 DOM 形状）。
struct BNode {
    kind: BlockKind,
    attrs: Attrs,
    text: String,
    marks: Vec<MarkSpan>,
    children: Vec<BNode>,
}

impl BNode {
    fn leaf(kind: BlockKind, text: String, marks: Vec<MarkSpan>, attrs: Attrs) -> Self {
        BNode {
            kind,
            attrs,
            text,
            marks,
            children: Vec::new(),
        }
    }
}

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    losses: Vec<Loss>,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn loss(
        &mut self,
        offset: usize,
        original: String,
        d: LossDisposition,
        note: impl Into<String>,
    ) {
        self.losses.push(Loss {
            offset,
            original,
            disposition: d,
            note: note.into(),
        });
    }

    fn tok_start(&self) -> usize {
        match self.toks.get(self.pos) {
            Some(Tok::Text { start, .. })
            | Some(Tok::Open { start, .. })
            | Some(Tok::Close { start, .. }) => *start,
            None => 0,
        }
    }

    /// 描述当前 Open 标签（Loss.original 用）。
    fn describe_open(&self) -> String {
        if let Some(Tok::Open { name, attrs, .. }) = self.toks.get(self.pos) {
            if let Some((k, v)) = attrs.first() {
                let vv: String = v.chars().take(32).collect();
                return format!("<{} {}=\"{}\">", name, k, vv);
            }
            return format!("<{}>", name);
        }
        String::new()
    }

    /// 跳过当前元素（Open 已在 pos）：按同名标签配平；void 元素不配平。
    fn skip_element(&mut self) {
        let name = match self.toks.get(self.pos) {
            Some(Tok::Open {
                name, self_close, ..
            }) => {
                let n = name.clone();
                self.pos += 1;
                if *self_close || VOID.contains(&n.as_str()) {
                    return;
                }
                n
            }
            _ => return,
        };
        let mut depth = 1usize;
        while let Some(t) = self.toks.get(self.pos) {
            match t {
                Tok::Open {
                    name: n,
                    self_close,
                    ..
                } => {
                    self.pos += 1;
                    if n == &name && !*self_close && !VOID.contains(&n.as_str()) {
                        depth += 1;
                    }
                }
                Tok::Close { name: n, .. } => {
                    self.pos += 1;
                    if n == &name {
                        depth -= 1;
                        if depth == 0 {
                            return;
                        }
                    }
                }
                Tok::Text { .. } => self.pos += 1,
            }
        }
    }

    // ---- 块级 ----

    fn parse_blocks(&mut self, stop: &[&str], list_depth: usize) -> Vec<BNode> {
        let mut out = Vec::new();
        loop {
            match self.toks.get(self.pos) {
                None => break,
                Some(Tok::Close { name, .. }) => {
                    let name = name.clone();
                    self.pos += 1;
                    if stop.contains(&name.as_str()) {
                        break;
                    }
                    // 游离闭合标签：跳过（恢复，不 panic）
                }
                Some(Tok::Open { name, .. }) => {
                    let name = name.clone();
                    if HANDLED.contains(&name.as_str()) {
                        out.extend(self.parse_block_element(&name, list_depth));
                    } else if EMBED_SKIP.contains(&name.as_str()) {
                        let off = self.tok_start();
                        let desc = self.describe_open();
                        self.skip_element();
                        self.loss(
                            off,
                            desc,
                            LossDisposition::Dropped,
                            "嵌入/脚本类元素：DM v0 无对应块型，整体丢弃",
                        );
                    } else if SCAFFOLD.contains(&name.as_str()) {
                        self.pos += 1;
                    } else if VOID_DROP.contains(&name.as_str()) {
                        let off = self.tok_start();
                        let desc = self.describe_open();
                        self.pos += 1;
                        self.loss(
                            off,
                            desc,
                            LossDisposition::Dropped,
                            "void 元数据/控件标签：无内容价值，跳过",
                        );
                    } else if FORM_SKIP.contains(&name.as_str()) {
                        let off = self.tok_start();
                        let desc = self.describe_open();
                        self.skip_element();
                        self.loss(
                            off,
                            desc,
                            LossDisposition::Dropped,
                            "表单控件：非文档内容，整体丢弃",
                        );
                    } else if TRANSPARENT.contains(&name.as_str()) {
                        let off = self.tok_start();
                        let desc = self.describe_open();
                        self.pos += 1;
                        self.loss(
                            off,
                            desc,
                            LossDisposition::DemotedToParagraph,
                            "未知/分节容器：透明穿轴，子内容按块解析保留",
                        );
                    } else {
                        // 行内标签 / 未知标签出现在块层：收集为段落
                        let (text, marks) = self.parse_inline();
                        if !text.is_empty() {
                            out.push(BNode::leaf(BlockKind::Paragraph, text, marks, Attrs::new()));
                        }
                    }
                }
                Some(Tok::Text { .. }) => {
                    let (text, marks) = self.parse_inline();
                    if !text.is_empty() {
                        out.push(BNode::leaf(BlockKind::Paragraph, text, marks, Attrs::new()));
                    }
                }
            }
        }
        out
    }

    fn parse_block_element(&mut self, name: &str, list_depth: usize) -> Vec<BNode> {
        if self.depth >= MAX_DEPTH {
            // 超深嵌套防护：跳过该元素并记 RawBlock，绝不递归爆栈。
            let off = self.tok_start();
            let desc = self.describe_open();
            self.skip_element();
            self.loss(
                off,
                desc,
                LossDisposition::RawBlock,
                format!("嵌套深度超过 {MAX_DEPTH} 层，跳过该子树"),
            );
            return Vec::new();
        }
        match name {
            "p" => {
                self.pos += 1;
                let (text, marks) = self.parse_inline();
                self.consume_close_if("p");
                vec![BNode::leaf(BlockKind::Paragraph, text, marks, Attrs::new())]
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level: usize = name[1..].parse().unwrap_or(1);
                self.pos += 1;
                let (text, marks) = self.parse_inline();
                self.consume_close_if(name);
                let mut attrs = Attrs::new();
                attrs.insert("level".into(), level.to_string());
                vec![BNode::leaf(BlockKind::Heading, text, marks, attrs)]
            }
            "hr" => {
                self.pos += 1;
                vec![BNode::leaf(
                    BlockKind::ThematicBreak,
                    String::new(),
                    Vec::new(),
                    Attrs::new(),
                )]
            }
            "ul" | "ol" => self.parse_list(name, list_depth),
            "blockquote" => {
                self.pos += 1;
                self.depth += 1;
                let children = self.parse_blocks(&["blockquote"], list_depth);
                self.depth -= 1;
                vec![BNode {
                    kind: BlockKind::Blockquote,
                    attrs: Attrs::new(),
                    text: String::new(),
                    marks: Vec::new(),
                    children,
                }]
            }
            "pre" => vec![self.parse_pre()],
            "math-block" => vec![self.parse_math_block()],
            "table" => self.parse_table(),
            // 游离的 li/tr/td 等：透明解包，内容并入当前层
            _ => {
                let off = self.tok_start();
                let desc = self.describe_open();
                self.pos += 1;
                self.depth += 1;
                let kids = self.parse_blocks(&[name], list_depth);
                self.depth -= 1;
                self.loss(
                    off,
                    desc,
                    LossDisposition::DemotedToParagraph,
                    "游离的表格/列表子元素（缺少父容器）：内容解包保留",
                );
                kids
            }
        }
    }

    fn consume_close_if(&mut self, name: &str) {
        if let Some(Tok::Close { name: n, .. }) = self.toks.get(self.pos) {
            if n == name {
                self.pos += 1;
            }
        }
    }

    fn parse_list(&mut self, tag: &str, list_depth: usize) -> Vec<BNode> {
        let start_num = if tag == "ol" {
            if let Some(Tok::Open { attrs, .. }) = self.toks.get(self.pos) {
                attr_of(attrs, "start")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .filter(|&n| n >= 1)
                    .unwrap_or(1)
            } else {
                1
            }
        } else {
            1
        };
        self.pos += 1; // consume <ul|ol>
        let mut items: Vec<BNode> = Vec::new();
        loop {
            match self.toks.get(self.pos) {
                None => break,
                Some(Tok::Close { name, .. }) => {
                    let name = name.clone();
                    self.pos += 1;
                    if name == tag {
                        break;
                    }
                }
                Some(Tok::Open { name, .. }) if name == "li" => {
                    items.push(self.parse_li(list_depth, tag == "ol", items.len(), start_num));
                }
                Some(Tok::Open { .. }) => {
                    let off = self.tok_start();
                    let desc = self.describe_open();
                    self.skip_element();
                    self.loss(
                        off,
                        desc,
                        LossDisposition::Dropped,
                        "列表直接子元素非 <li>：无法挂载，丢弃",
                    );
                }
                Some(Tok::Text { raw, .. }) => {
                    let t = collapse_ws(&decode_entities(raw));
                    let off = self.tok_start();
                    self.pos += 1;
                    if !t.trim().is_empty() {
                        self.loss(
                            off,
                            format!("<{tag}>"),
                            LossDisposition::Dropped,
                            "游离于 <li> 之外的文本：丢弃",
                        );
                    }
                }
            }
        }
        items
    }

    fn parse_li(
        &mut self,
        list_depth: usize,
        ordered: bool,
        index: usize,
        start_num: u32,
    ) -> BNode {
        let li_off = self.tok_start();
        self.pos += 1; // consume <li>
        self.depth += 1;
        let blocks = self.parse_blocks(&["li"], list_depth + 1);
        self.depth -= 1;
        // 折叠：Paragraph/Heading 并入条目文本；其余块型保留为子块
        let mut text = String::new();
        let mut blen = 0usize;
        let mut marks: Vec<MarkSpan> = Vec::new();
        let mut children = Vec::new();
        for b in blocks {
            match b.kind {
                BlockKind::Paragraph => {
                    fold_inline(&mut text, &mut blen, &mut marks, b.text, b.marks)
                }
                BlockKind::Heading => {
                    self.loss(
                        li_off,
                        "heading".into(),
                        LossDisposition::DemotedToParagraph,
                        "<li> 内的标题块：文本并入条目（DM/MD 无此结构）",
                    );
                    fold_inline(&mut text, &mut blen, &mut marks, b.text, b.marks);
                }
                _ => children.push(b),
            }
        }
        let mut attrs = Attrs::new();
        if ordered {
            attrs.insert("marker".into(), format!("{}.", start_num + index as u32));
        } else {
            attrs.insert("marker".into(), "-".into());
        }
        // depth 与 markdown.rs 对齐：1 起始（顶层 li = 1，嵌套逐层 +1；
        // markdown 侧 = 缩进/2 + 1，见 markdown.rs list_item_of）。
        attrs.insert("depth".into(), (list_depth + 1).to_string());
        BNode {
            kind: BlockKind::ListItem,
            attrs,
            text,
            marks,
            children,
        }
    }

    fn parse_pre(&mut self) -> BNode {
        self.pos += 1; // consume <pre>
        let mut lang = String::new();
        // <pre> 与 <code> 之间的空白跳过；<code> 可选
        if let Some(Tok::Text { raw, .. }) = self.toks.get(self.pos) {
            if collapse_ws(raw).trim().is_empty() {
                self.pos += 1;
            }
        }
        if let Some(Tok::Open { name, attrs, .. }) = self.toks.get(self.pos) {
            if name == "code" {
                lang = lang_from_class(attrs);
                self.pos += 1;
            }
        }
        // 逐字内容：实体解码、空白**不折叠**；按 HTML5 剥一个前导换行
        let mut buf = String::new();
        let mut stripped = false;
        let mut logged_markup = false;
        loop {
            match self.toks.get(self.pos) {
                None => break,
                Some(Tok::Close { .. }) => {
                    let is_pre = matches!(self.toks.get(self.pos), Some(Tok::Close { name, .. }) if name == "pre");
                    self.pos += 1;
                    if is_pre {
                        break;
                    }
                }
                Some(Tok::Open { .. }) => {
                    if !logged_markup {
                        logged_markup = true;
                        let off = self.tok_start();
                        self.loss(
                            off,
                            "pre 内行内标签".into(),
                            LossDisposition::Dropped,
                            "<pre> 内的行内标签（高亮 span 等）：丢弃，文本保留",
                        );
                    }
                    self.pos += 1;
                }
                Some(Tok::Text { raw, .. }) => {
                    let mut t = decode_entities(raw);
                    if !stripped {
                        if let Some(rest) = t.strip_prefix('\n') {
                            t = rest.to_string();
                        }
                        stripped = true;
                    }
                    buf.push_str(&t);
                    self.pos += 1;
                }
            }
        }
        let mut attrs = Attrs::new();
        if !lang.is_empty() {
            attrs.insert("lang".into(), lang);
        }
        BNode::leaf(BlockKind::CodeBlock, buf, Vec::new(), attrs)
    }

    /// `<math-block>` → Math（#261）。内容逐字（仅实体解码）；与 pre 的
    /// 差异：不剥前导换行（机器格式无 HTML5 手写换行惯例，剥则往返丢字）。
    /// 内部行内标签丢弃留痕（pre 同构）；无 attrs / 无 marks（源码即内容）。
    fn parse_math_block(&mut self) -> BNode {
        self.pos += 1; // consume <math-block>
        let mut buf = String::new();
        let mut logged_markup = false;
        loop {
            match self.toks.get(self.pos) {
                None => break,
                Some(Tok::Close { .. }) => {
                    let is_end = matches!(self.toks.get(self.pos), Some(Tok::Close { name, .. }) if name == "math-block");
                    self.pos += 1;
                    if is_end {
                        break;
                    }
                }
                Some(Tok::Open { .. }) => {
                    if !logged_markup {
                        logged_markup = true;
                        let off = self.tok_start();
                        self.loss(
                            off,
                            "<math-block> 内行内标签".into(),
                            LossDisposition::Dropped,
                            "<math-block> 内的行内标签：丢弃，文本保留（pre 同构）",
                        );
                    }
                    self.pos += 1;
                }
                Some(Tok::Text { raw, .. }) => {
                    buf.push_str(&decode_entities(raw));
                    self.pos += 1;
                }
            }
        }
        BNode::leaf(BlockKind::Math, buf, Vec::new(), Attrs::new())
    }

    fn parse_table(&mut self) -> Vec<BNode> {
        self.pos += 1; // consume <table>
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut any_cell_marks = false;
        let mut raw_text = String::new();
        loop {
            match self.toks.get(self.pos) {
                None => break,
                Some(Tok::Close { name, .. }) => {
                    let name = name.clone();
                    self.pos += 1;
                    if name == "table" {
                        break;
                    }
                }
                Some(Tok::Open { name, .. }) if name == "tr" => {
                    self.pos += 1;
                    let mut row = Vec::new();
                    loop {
                        match self.toks.get(self.pos) {
                            None => break,
                            Some(Tok::Close { name, .. }) => {
                                let n = name.clone();
                                self.pos += 1;
                                if n == "tr" {
                                    break;
                                }
                            }
                            Some(Tok::Open { name, .. }) if name == "th" || name == "td" => {
                                self.pos += 1;
                                let (t, m) = self.parse_inline();
                                if !m.is_empty() {
                                    any_cell_marks = true;
                                }
                                self.consume_close_if(name);
                                row.push(t);
                            }
                            Some(Tok::Open { name, .. }) if name == "table" => {
                                let off = self.tok_start();
                                self.skip_element();
                                self.loss(
                                    off,
                                    "<table>".into(),
                                    LossDisposition::Dropped,
                                    "嵌套表格：DM v0 不支持，丢弃",
                                );
                            }
                            Some(Tok::Open { .. }) => self.pos += 1,
                            Some(Tok::Text { .. }) => self.pos += 1,
                        }
                    }
                    rows.push(row);
                }
                Some(Tok::Open { name, .. })
                    if name == "thead" || name == "tbody" || name == "tfoot" =>
                {
                    self.pos += 1;
                }
                Some(Tok::Open { name, .. }) if name == "caption" => {
                    let off = self.tok_start();
                    let desc = self.describe_open();
                    self.skip_element();
                    self.loss(
                        off,
                        desc,
                        LossDisposition::Dropped,
                        "表格标题 caption：丢弃",
                    );
                }
                Some(Tok::Open { name, .. }) if name == "colgroup" => {
                    self.skip_element();
                }
                Some(Tok::Open { name, .. }) if name == "col" => {
                    self.pos += 1;
                }
                Some(Tok::Open { name, .. }) if name == "th" || name == "td" => {
                    // 游离单元格：每个自成一行
                    self.pos += 1;
                    let (t, m) = self.parse_inline();
                    if !m.is_empty() {
                        any_cell_marks = true;
                    }
                    self.consume_close_if(name);
                    rows.push(vec![t]);
                }
                Some(Tok::Open { .. }) => {
                    let off = self.tok_start();
                    let desc = self.describe_open();
                    self.skip_element();
                    self.loss(
                        off,
                        desc,
                        LossDisposition::Dropped,
                        "表格内的非结构元素：丢弃",
                    );
                }
                Some(Tok::Text { raw, .. }) => {
                    let t = collapse_ws(&decode_entities(raw));
                    if !t.trim().is_empty() {
                        raw_text.push_str(&t);
                    }
                    self.pos += 1;
                }
            }
        }
        let col = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if col == 0 {
            let off = self.tok_start();
            self.loss(
                off,
                "<table>".into(),
                LossDisposition::RawBlock,
                "表格无可解析单元格：可见文本降级为段落",
            );
            let mut out = Vec::new();
            if !raw_text.is_empty() {
                out.push(BNode::leaf(
                    BlockKind::Paragraph,
                    raw_text,
                    Vec::new(),
                    Attrs::new(),
                ));
            }
            return out;
        }
        for r in rows.iter_mut() {
            r.resize(col, String::new());
        }
        let header = rows.remove(0);
        let text = canonical_table_text(&header, &rows);
        if any_cell_marks {
            let off = self.tok_start();
            self.loss(
                off,
                "<td>".into(),
                LossDisposition::Dropped,
                "表格单元格内的行内 mark：管道行文本不可承载，丢弃（样式存于文本）",
            );
        }
        vec![BNode::leaf(
            BlockKind::Table,
            text,
            Vec::new(),
            Attrs::new(),
        )]
    }

    // ---- 行内 ----

    /// 行内解析：返回（折叠+裁剪后的文本, mark 区间）。
    /// 停止条件：块级边界 Open（不消费）/ 无法配对的 Close（不消费）/ EOF。
    fn parse_inline(&mut self) -> (String, Vec<MarkSpan>) {
        let mut buf = String::new();
        let mut blen = 0usize; // buf 的 char 长度（同步维护）
        let mut marks: Vec<MarkSpan> = Vec::new();
        let mut open_marks: Vec<(String, &'static str, usize)> = Vec::new(); // (tag, kind, start)
        let mut unknown_open: Vec<String> = Vec::new();
        let mut last_nl = false;
        loop {
            match self.toks.get(self.pos) {
                None => break,
                Some(Tok::Text { raw, .. }) => {
                    self.pos += 1;
                    let mut t = collapse_ws(&decode_entities(raw));
                    if last_nl {
                        let trimmed = t.trim_start_matches(' ');
                        blen -= t.chars().count() - trimmed.chars().count();
                        t = trimmed.to_string();
                    }
                    if t.is_empty() {
                        continue;
                    }
                    buf.push_str(&t);
                    blen += t.chars().count();
                    last_nl = false;
                }
                Some(Tok::Open {
                    name,
                    attrs,
                    self_close,
                    ..
                }) => {
                    let name = name.clone();
                    let sc = *self_close;
                    match name.as_str() {
                        "br" => {
                            self.pos += 1;
                            while buf.ends_with(' ') {
                                buf.pop();
                                blen -= 1;
                            }
                            buf.push('\n');
                            blen += 1;
                            last_nl = true;
                        }
                        "img" => {
                            let off = self.tok_start();
                            let alt = self
                                .toks
                                .get(self.pos)
                                .map(|t| match t {
                                    Tok::Open { attrs, .. } => {
                                        attr_of(attrs, "alt").unwrap_or("").to_string()
                                    }
                                    _ => String::new(),
                                })
                                .unwrap_or_default();
                            self.pos += 1;
                            self.loss(
                                off,
                                "<img>".into(),
                                LossDisposition::Dropped,
                                format!("图片：DM v0 无图片块型，丢弃（alt: {:?}）", alt),
                            );
                        }
                        "wbr" | "source" | "track" | "param" | "area" | "base" | "col" => {
                            self.pos += 1;
                        }
                        _ if mark_kind_of(&name).is_some() => {
                            if sc {
                                self.pos += 1;
                            } else {
                                let kind = mark_kind_of(&name).unwrap();
                                open_marks.push((name.clone(), kind, blen));
                                self.pos += 1;
                            }
                        }
                        _ if is_inline_stop(&name) => break, // 块级边界（不消费）
                        _ => {
                            // 透明行内标签（span/a/未知…）：文字保留，样式/链接丢弃
                            if !attrs.is_empty() {
                                let off = self.tok_start();
                                let desc = self.describe_open();
                                self.loss(
                                    off,
                                    desc,
                                    LossDisposition::Dropped,
                                    "行内样式/链接标签：文本保留，属性丢弃",
                                );
                            }
                            self.pos += 1;
                            if !sc {
                                unknown_open.push(name);
                            }
                        }
                    }
                }
                Some(Tok::Close { name, .. }) => {
                    let name = name.clone();
                    if unknown_open.last() == Some(&name) {
                        unknown_open.pop();
                        self.pos += 1;
                    } else if let Some(kind) = mark_kind_of(&name) {
                        match open_marks.iter().rposition(|(_, k, _)| *k == kind) {
                            Some(idx) => {
                                let (_, _, s) = open_marks.remove(idx);
                                if blen > s {
                                    marks.push(MarkSpan {
                                        start: s,
                                        end: blen,
                                        kind: kind.to_string(),
                                    });
                                }
                                self.pos += 1;
                            }
                            None => break, // 游离闭合 → 块边界
                        }
                    } else {
                        break;
                    }
                }
            }
        }
        // 未闭合 mark 就地收口（恢复，不 panic）
        for (_, kind, s) in open_marks.into_iter().rev() {
            if blen > s {
                marks.push(MarkSpan {
                    start: s,
                    end: blen,
                    kind: kind.to_string(),
                });
            }
        }
        let (text, mut marks) = trim_inline(&buf, marks);
        marks.sort_by(|a, b| (a.start, a.end, &a.kind).cmp(&(b.start, b.end, &b.kind)));
        (text, marks)
    }
}

fn attr_of<'x>(attrs: &'x [(String, String)], k: &str) -> Option<&'x str> {
    attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
}

/// `class="language-rust"` / `"lang-rust"` → `rust`。
fn lang_from_class(attrs: &[(String, String)]) -> String {
    let class = match attr_of(attrs, "class") {
        Some(c) => c,
        None => return String::new(),
    };
    for tok in class.split_whitespace() {
        if let Some(l) = tok.strip_prefix("language-") {
            return l.to_string();
        }
        if let Some(l) = tok.strip_prefix("lang-") {
            return l.to_string();
        }
    }
    String::new()
}

/// 段落折叠进 li 条目文本：空格连接 + mark 平移。
fn fold_inline(
    text: &mut String,
    blen: &mut usize,
    marks: &mut Vec<MarkSpan>,
    add: String,
    add_marks: Vec<MarkSpan>,
) {
    if add.is_empty() {
        return;
    }
    if !text.is_empty() && !text.ends_with(' ') && !text.ends_with('\n') {
        text.push(' ');
        *blen += 1;
    }
    for m in add_marks {
        marks.push(MarkSpan {
            start: m.start + *blen,
            end: m.end + *blen,
            kind: m.kind,
        });
    }
    text.push_str(&add);
    *blen += add.chars().count();
}

/// 块缘裁剪（首尾空白），mark 区间同步平移/裁剪。
fn trim_inline(buf: &str, marks: Vec<MarkSpan>) -> (String, Vec<MarkSpan>) {
    let chars: Vec<char> = buf.chars().collect();
    let start = chars
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or(chars.len());
    let end = chars
        .iter()
        .rposition(|c| !c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    let text: String = chars[start..end].iter().collect();
    let total = end - start;
    let out = marks
        .into_iter()
        .filter_map(|m| {
            let s = m.start.saturating_sub(start).min(total);
            let e = m.end.saturating_sub(start).min(total);
            if e > s {
                Some(MarkSpan {
                    start: s,
                    end: e,
                    kind: m.kind,
                })
            } else {
                None
            }
        })
        .collect();
    (text, out)
}

// ---------------------------------------------------------------------------
// 表格：DM 规范管道行文本
// ---------------------------------------------------------------------------

/// 单元格内的 `|` 转义为 `\|`（树内文本即规范形；导出侧对称反转义）。
fn escape_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '|' {
            out.push('\\');
        }
        if c == '\n' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn unescape_cell(s: &str) -> String {
    s.replace("\\|", "|")
}

fn canonical_table_text(header: &[String], body: &[Vec<String>]) -> String {
    let col = header.len();
    let line = |cells: &[String]| -> String {
        let inner: Vec<String> = cells
            .iter()
            .take(col)
            .map(|c| format!(" {} ", escape_cell(c)))
            .collect();
        format!("|{}|", inner.join("|"))
    };
    let mut lines = vec![line(header)];
    // 分隔行不带内边距（对齐 markdown.rs 常规形态：`|---|---|`），
    // 保证同一表格在两条导入路径下文本逐字节一致。
    let sep: Vec<String> = (0..col).map(|_| "---".to_string()).collect();
    lines.push(format!("|{}|", sep.join("|")));
    for r in body {
        lines.push(line(r));
    }
    lines.join("\n")
}

/// 分隔行判定：每格仅含 `:- ` 且至少一个 `-`。
fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            !t.is_empty()
                && t.chars().all(|ch| ch == ':' || ch == '-' || ch == ' ')
                && t.contains('-')
        })
}

/// 单行管道文本 → 单元格（处理 `\|` 转义）。
fn split_row(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    if t.trim().is_empty() {
        return None;
    }
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '\\' => {
                cur.push('\\');
            }
            '|' => {
                cells.push(unescape_cell(&cur));
                cur = String::new();
            }
            _ => cur.push(c),
        }
    }
    cells.push(unescape_cell(&cur));
    Some(cells.into_iter().map(|c| c.trim().to_string()).collect())
}

/// Table 节点文本 → (表头, 体行)。无法解析 → None（导出侧降级为段落）。
fn parse_table_text(text: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in text.split('\n') {
        if let Some(cells) = split_row(line) {
            if !is_separator_row(&cells) {
                rows.push(cells);
            }
        }
    }
    if rows.is_empty() {
        return None;
    }
    let header = rows.remove(0);
    let col = header
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if col == 0 {
        return None;
    }
    let mut header = header;
    header.resize(col, String::new());
    for r in rows.iter_mut() {
        r.resize(col, String::new());
    }
    Some((header, rows))
}

// ---------------------------------------------------------------------------
// 物化：BNode → BlockTree（新分配 BlockId）
// ---------------------------------------------------------------------------

fn materialize(nodes: Vec<BNode>, tree: &mut BlockTree, parent: NodeId) {
    for b in nodes {
        let id = tree.create_node(b.kind, &b.text, b.attrs, b.marks);
        let len = tree.node(parent).children.len();
        let _ = tree.insert_child(parent, len, id);
        materialize(b.children, tree, id);
    }
}

// ---------------------------------------------------------------------------
// 导出：BlockTree → HTML
// ---------------------------------------------------------------------------

/// 导出认识的行内 mark kind（序即嵌套序；其余 kind 不导出——DM→MD 同样不写 mark）。
const INLINE_KINDS: [&str; 6] = ["strong", "em", "code", "math", "del", "u"];

fn kind_rank(k: &str) -> usize {
    INLINE_KINDS
        .iter()
        .position(|&x| x == k)
        .unwrap_or(usize::MAX)
}

/// 行内导出：转义 + mark 标签化（活跃集归一化，保证标签合法嵌套）。
// 0..=len 边界扫描：位置 len 是“收尾收口点”（无字符，仅闭标签），i 是位置
// 索引而非 chars 元素迭代——clippy 的迭代器改写不适配边界语义，保持显式。
#[allow(clippy::needless_range_loop)]
fn inline_html(text: &str, marks: &[MarkSpan]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let known: Vec<&MarkSpan> = marks
        .iter()
        .filter(|m| INLINE_KINDS.contains(&m.kind.as_str()))
        .collect();
    let mut open: Vec<String> = Vec::new();
    let mut out = String::new();
    for i in 0..=len {
        // 位置 i 的期望活跃 kind 集（去重、按序）
        let mut desired: Vec<&str> = if i < len {
            known
                .iter()
                .filter(|m| m.start <= i && i < m.end)
                .map(|m| m.kind.as_str())
                .collect()
        } else {
            Vec::new()
        };
        desired.sort_by_key(|k| kind_rank(k));
        desired.dedup();
        // 收口：栈顶不在期望集 → 闭合
        while let Some(top) = open.last() {
            if desired.contains(&top.as_str()) {
                break;
            }
            let top = open.pop().unwrap();
            out.push_str(&format!("</{}>", top));
        }
        // 栈不是期望集的前缀（交叉 mark）→ 全收口重开
        let prefix_ok =
            open.len() <= desired.len() && open.iter().zip(desired.iter()).all(|(a, b)| a == b);
        if !prefix_ok {
            while let Some(top) = open.pop() {
                out.push_str(&format!("</{}>", top));
            }
        }
        for k in desired.iter().skip(open.len()) {
            out.push_str(&format!("<{}>", k));
            open.push((*k).to_string());
        }
        if i < len {
            match chars[i] {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                '\n' => out.push_str("<br>"),
                c => out.push(c),
            }
        }
    }
    out
}

fn render_blocks(tree: &BlockTree, ids: &[NodeId], out: &mut String) {
    let mut i = 0;
    while i < ids.len() {
        if tree.node(ids[i]).kind == BlockKind::ListItem {
            let mut j = i;
            while j < ids.len() && tree.node(ids[j]).kind == BlockKind::ListItem {
                j += 1;
            }
            render_list(tree, &ids[i..j], out);
            i = j;
        } else {
            render_block(tree, ids[i], out);
            i += 1;
        }
    }
}

fn render_block(tree: &BlockTree, id: NodeId, out: &mut String) {
    let node = tree.node(id);
    match node.kind {
        BlockKind::Document => render_blocks(tree, &node.children, out),
        BlockKind::Paragraph => {
            out.push_str("<p>");
            out.push_str(&inline_html(&node.text.to_string(), &node.marks));
            out.push_str("</p>\n");
        }
        BlockKind::Heading => {
            let level: usize = node
                .attrs
                .get("level")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1)
                .clamp(1, 6);
            out.push_str(&format!("<h{}>", level));
            out.push_str(&inline_html(&node.text.to_string(), &node.marks));
            out.push_str(&format!("</h{}>\n", level));
        }
        BlockKind::CodeBlock => {
            let lang = node.attrs.get("lang").cloned().unwrap_or_default();
            if lang.is_empty() {
                out.push_str("<pre><code>");
            } else {
                out.push_str(&format!(
                    "<pre><code class=\"language-{}\">",
                    escape_html(&lang)
                ));
            }
            out.push_str(&escape_html(&node.text.to_string()));
            out.push_str("</code></pre>\n");
        }
        // #261：LaTeX 显示公式——专用标签（标签即锚点，textContent 即源码，
        // 消费方 katex.render(el.textContent, el) 直接可得；行内面对应 <math>）。
        BlockKind::Math => {
            out.push_str("<math-block>");
            out.push_str(&escape_html(&node.text.to_string()));
            out.push_str("</math-block>\n");
        }
        BlockKind::ThematicBreak => out.push_str("<hr>\n"),
        BlockKind::Table => {
            let text = node.text.to_string();
            match parse_table_text(&text) {
                Some((header, body)) => {
                    out.push_str("<table>\n<thead><tr>");
                    for c in &header {
                        out.push_str(&format!("<th>{}</th>", inline_html(c, &[])));
                    }
                    out.push_str("</tr></thead>\n");
                    if !body.is_empty() {
                        out.push_str("<tbody>\n");
                        for r in &body {
                            out.push_str("<tr>");
                            for c in r {
                                out.push_str(&format!("<td>{}</td>", inline_html(c, &[])));
                            }
                            out.push_str("</tr>\n");
                        }
                        out.push_str("</tbody>\n");
                    }
                    out.push_str("</table>\n");
                }
                None => {
                    // 退化：Table 文本不是规范管道行 → 按段落导出（登记于模块文档）
                    out.push_str("<p>");
                    out.push_str(&escape_html(&text));
                    out.push_str("</p>\n");
                }
            }
        }
        BlockKind::Blockquote => {
            out.push_str("<blockquote>\n");
            render_blocks(tree, &node.children, out);
            out.push_str("</blockquote>\n");
        }
        BlockKind::ListItem => render_list(tree, &[id], out), // 单项兜底
    }
}

/// 连续 ListItem 兄弟 → `<ul>`/`<ol>`；ordered 由首项 marker 决定（已知有损点 4）。
fn render_list(tree: &BlockTree, items: &[NodeId], out: &mut String) {
    let first = tree.node(items[0]);
    let marker = first
        .attrs
        .get("marker")
        .cloned()
        .unwrap_or_else(|| "-".into());
    let digits: String = marker.chars().take_while(|c| c.is_ascii_digit()).collect();
    let ordered = !digits.is_empty();
    if ordered {
        let start: u32 = digits.parse().unwrap_or(1);
        if start > 1 {
            out.push_str(&format!("<ol start=\"{}\">\n", start));
        } else {
            out.push_str("<ol>\n");
        }
    } else {
        out.push_str("<ul>\n");
    }
    for &id in items {
        let node = tree.node(id);
        out.push_str("<li>");
        out.push_str(&inline_html(&node.text.to_string(), &node.marks));
        render_blocks(tree, &node.children, out);
        out.push_str("</li>\n");
    }
    if ordered {
        out.push_str("</ol>\n");
    } else {
        out.push_str("</ul>\n");
    }
}
