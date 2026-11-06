//! #267 样式继承树——真实 docx 语料有效属性验证（验收 5）。
//!
//! 与 `style_corpus.rs`（S1 全量扫描 + 桶级统计）互补：本文件**钉定
//! 6 件真实文档**，逐字段断言「沿 basedOn 链解析出的最终字号/字体/
//! 缩进」（`effective_props`——#267 typed 严格面）。每件样本先断言
//! **自身层无该属性**（继承是唯一来源），再断言有效值——防止把
//! 直接属性误报为继承验证。
//!
//! 语料全部为**仓内 tracked 文件**（`test-artifacts/health-check/
//! corpus/`，Apache POI test-data + docx4j 样本——GitHub 公开仓收集
//! 2026-10-31，随仓库检出）。因此本门**不设静默 SKIP**：文件缺失 =
//! checkout 破损，直接失败（#262 教训——语料门静默空转 = 假绿；
//! tracked 夹具缺席没有任何正当理由）。
//!
//! 样本实况（styles.xml 一手核验，非转述）：
//!
//! | 文件 | 链 | 验证的继承形态 |
//! |---|---|---|
//! | poi `heading123.docx` | Heading3→Normal（+docDefaults） | 字号：Heading3 无自身 sz → docDefaults 24 经空链可见；Heading1 自身 32 |
//! | poi `Styles.docx` | berschrift3→Standard→docDefaults | 字号：德文文档两级空链 → docDefaults 22；berschrift1 自身 28；Titel 52 |
//! | poi `57312.docx` | ppunktatekst→ppunkta→punkt1→punkt→Normalny（深 5） | 字号+缩进+jc 多跳：sz=24/ind=737 来自 ppunkta（隔 3 层），jc=both 来自 Normalny；字体层替换（docDefaults Calibri → Normalny Times New Roman） |
//! | poi `MultipleBodyBug.docx` | Heading5→Heading4→…→Heading1→Normal（深 6） | 字体跨层：Heading5 自身无字体 → 隔 3 层继承 Heading2 的 Arial（Heading1 的 Arial Black 被更近层覆盖）；缩进/outline 自有 |
//! | poi `59378.docx` | CommentSubject→CommentText→Normal | 字号+字体双继承：sz=20 与 TNR×4 都来自 2 跳祖先 CommentText（并压过 docDefaults sz=22） |
//! | docx4j `indentation_firstline.docx` | ListParagraph→Normal | 缩进：left=720 自有；字号：docDefaults 22 经 Normal 可见；Heading1 自身 28 |

use mdc_office_io::style::{read_docx_styles, StyleChainError};

/// 语料根（仓内 tracked——两目录与 external_corpus.rs 先例同构）。
const CORPUS_ROOT: &str = "test-artifacts/health-check/corpus";

/// 钉定样本数下限（验收 5：≥5 件——取 6 留余量）。
const MIN_SPECIMENS: usize = 5;

fn corpus_path(rel: &str) -> std::path::PathBuf {
    // 两级上溯 = 仓根（crates/office-io → 仓库）。
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(CORPUS_ROOT)
        .join(rel)
}

/// 读样式表（tracked 夹具缺失 = checkout 破损 → 硬失败，不静默跳过）。
fn load(rel: &str) -> mdc_office_io::style::StyleBundle {
    let path = corpus_path(rel);
    assert!(
        path.exists(),
        "tracked corpus fixture missing (broken checkout?): {}",
        path.display()
    );
    read_docx_styles(path.to_str().expect("utf-8 path"))
        .unwrap_or_else(|e| panic!("read styles of {rel}: {e}"))
}

#[test]
fn inheritance_specimens_effective_props() {
    // ---- ① heading123.docx：docDefaults 字号经空链可见 ----
    let b = load("poi-docx/heading123.docx");
    // Heading3 自身无 sz（继承是唯一来源）。
    assert!(
        b.tree.get("Heading3").unwrap().run.size.is_none(),
        "Heading3 must not define its own size"
    );
    let h3 = b.tree.effective_props("Heading3").unwrap();
    // docDefaults sz=24（12pt）铺底 → Normal（空）→ Heading3（空）可见。
    assert_eq!(h3.run.size, Some(24));
    assert_eq!(h3.para.outline_lvl, Some(2)); // 自身层
                                              // 对照：Heading1 自身 sz=32（16pt）。
    assert_eq!(b.tree.get("Heading1").unwrap().run.size, Some(32));
    let h1 = b.tree.effective_props("Heading1").unwrap();
    assert_eq!(h1.run.size, Some(32));
    assert_eq!(h1.para.outline_lvl, Some(0));
    // 链字段承载（验收 1）：next / link / is_default 真实值。
    assert_eq!(
        b.tree.get("Heading1").unwrap().next.as_deref(),
        Some("Normal")
    );
    assert_eq!(
        b.tree.get("Heading1").unwrap().link.as_deref(),
        Some("Heading1Char")
    );
    assert!(b.tree.get("Normal").unwrap().is_default);

    // ---- ② Styles.docx（德文 Word 文档）：两级空链 → docDefaults ----
    let b = load("poi-docx/Styles.docx");
    assert!(b.tree.get("Standard").unwrap().is_default);
    let h3 = b.tree.effective_props("berschrift3").unwrap();
    // berschrift3 无自身 sz，Standard 无自身 sz → docDefaults 22（11pt）可见。
    assert_eq!(h3.run.size, Some(22));
    assert_eq!(h3.para.outline_lvl, Some(2));
    // 自有字号层（同文件对照）：berschrift1=28 / Titel=52。
    assert_eq!(b.tree.get("berschrift1").unwrap().run.size, Some(28));
    assert_eq!(
        b.tree.effective_props("berschrift1").unwrap().run.size,
        Some(28)
    );
    assert_eq!(b.tree.effective_props("Titel").unwrap().run.size, Some(52));
    // 未知 id → typed 错误（真实文档上同样成立）。
    assert_eq!(
        b.tree.effective_props("NoSuchStyle"),
        Err(StyleChainError::UnknownStyle {
            id: "NoSuchStyle".into()
        })
    );

    // ---- ③ 57312.docx（波兰语文档）：深 5 链多跳继承 ----
    let b = load("poi-docx/57312.docx");
    assert!(b.tree.get("Normalny").unwrap().is_default);
    assert_eq!(
        b.tree.chain("ppunktatekst").unwrap().len(),
        5,
        "ppunktatekst → ppunkta → punkt1 → punkt → Normalny"
    );
    // 查询口自身全空（sz/fonts/ind 均无）——一切来自继承。
    let self_style = b.tree.get("ppunktatekst").unwrap();
    assert!(self_style.run.size.is_none());
    assert!(self_style.run.fonts.is_none());
    assert!(self_style.para.indent.is_none());
    let eff = b.tree.effective_props("ppunktatekst").unwrap();
    // 字号 + 缩进：来自 3 层之上的 ppunkta（sz=24、ind left=737）。
    assert_eq!(eff.run.size, Some(24));
    assert_eq!(eff.para.indent.map(|i| i.left), Some(Some(737)));
    // 对齐：来自链头 Normalny（jc=both）。
    assert_eq!(eff.para.alignment.as_deref(), Some("both"));
    // 字体层替换：docDefaults Calibri（四槽）被 Normalny 的
    // Times New Roman（仅 ascii/hAnsi）整体替换——层内合并语义。
    let fonts = eff.run.fonts.as_ref().expect("fonts via chain layer");
    assert_eq!(fonts.ascii.as_deref(), Some("Times New Roman"));
    assert_eq!(fonts.hi_ansi.as_deref(), Some("Times New Roman"));
    assert_eq!(fonts.east_asia, None);
    assert_eq!(fonts.cs, None);

    // ---- ④ MultipleBodyBug.docx：深 6 链 + 字体跨层继承 ----
    let b = load("poi-docx/MultipleBodyBug.docx");
    assert_eq!(
        b.tree.chain("Heading5").unwrap().len(),
        6,
        "Heading5 → Heading4 → Heading3 → Heading2 → Heading1 → Normal"
    );
    let self_style = b.tree.get("Heading5").unwrap();
    // Heading5 自身无字体（继承是唯一来源）；缩进/outline 自有。
    assert!(self_style.run.fonts.is_none());
    let eff = b.tree.effective_props("Heading5").unwrap();
    // 字体：隔 3 层继承 Heading2 的 Arial（更近层覆盖 Heading1 的
    // Arial Black 与 Normal 的 Arial——「最近层赢」语义）。
    let fonts = eff.run.fonts.as_ref().expect("fonts via Heading2 layer");
    assert_eq!(fonts.ascii.as_deref(), Some("Arial"));
    assert_eq!(fonts.hi_ansi.as_deref(), Some("Arial"));
    assert_eq!(fonts.cs, None);
    // 缩进 + outline：自身层。
    let ind = eff.para.indent.expect("Heading5 own indent");
    assert_eq!(ind.left, Some(2143));
    assert_eq!(
        ind.special,
        Some(mdc_office_io::docx::ParagraphIndentSpecial::Hanging(1009))
    );
    assert_eq!(eff.para.outline_lvl, Some(4));

    // ---- ⑤ 59378.docx：2 跳祖先双属性继承（压过 docDefaults）----
    let b = load("poi-docx/59378.docx");
    let self_style = b.tree.get("CommentSubject").unwrap();
    assert!(self_style.run.size.is_none());
    assert!(self_style.run.fonts.is_none());
    let eff = b.tree.effective_props("CommentSubject").unwrap();
    // sz=20 与 Times New Roman×4 均来自 CommentText（2 跳）。
    assert_eq!(eff.run.size, Some(20));
    let fonts = eff.run.fonts.as_ref().expect("fonts via CommentText layer");
    for slot in [&fonts.ascii, &fonts.hi_ansi, &fonts.east_asia, &fonts.cs] {
        assert_eq!(slot.as_deref(), Some("Times New Roman"));
    }
    // docDefaults sz=22 被 CommentText 的 20 覆盖——继承链优先于铺底。
    assert_eq!(eff.run.size, Some(20));
    // 链字段（验收 1）：next / link 真实值。
    assert_eq!(
        b.tree.get("CommentSubject").unwrap().next.as_deref(),
        Some("CommentText")
    );
    assert_eq!(
        b.tree.get("CommentText").unwrap().link.as_deref(),
        Some("CommentTextChar")
    );

    // ---- ⑥ indentation_firstline.docx（docx4j）：缩进自有 + 字号继承 ----
    let b = load("docx4j/indentation_firstline.docx");
    let lp = b.tree.effective_props("ListParagraph").unwrap();
    // 缩进：自身层（left=720）。
    assert_eq!(lp.para.indent.map(|i| i.left), Some(Some(720)));
    // 字号：ListParagraph/Normal 均无自身 sz → docDefaults 22 可见。
    assert_eq!(lp.run.size, Some(22));
    assert_eq!(
        b.tree.effective_props("Heading1").unwrap().run.size,
        Some(28)
    );

    // ---- 样本量下限（防未来删样本后测试空转——#262 纪律）----
    let specimens = [
        "poi-docx/heading123.docx",
        "poi-docx/Styles.docx",
        "poi-docx/57312.docx",
        "poi-docx/MultipleBodyBug.docx",
        "poi-docx/59378.docx",
        "docx4j/indentation_firstline.docx",
    ];
    assert!(
        specimens.len() >= MIN_SPECIMENS,
        "钉定样本数 {} 低于验收下限 {MIN_SPECIMENS}",
        specimens.len()
    );
    // typed 错误在正常语料上零出现（真实生态零环——与全量扫描结论一致）。
    for rel in specimens {
        let b = load(rel);
        for id in b.tree.style_ids() {
            assert!(
                !matches!(
                    b.tree.effective_props(id),
                    Err(StyleChainError::CycleDetected { .. })
                ),
                "unexpected cycle in real corpus {rel} at {id}"
            );
        }
    }
}

#[test]
fn inherited_props_visible_only_through_chain() {
    // 反向证伪：继承值不得泄漏为自身层属性（防止「把继承值错当
    // 直接值缓存回 StyleRef」的实现回归）。取样 57312 + 59378 的
    // 既有断言面，逐字段核对自身层与有效层的差集。
    let b = load("poi-docx/57312.docx");
    let leaf = b.tree.get("ppunktatekst").unwrap().clone();
    let eff = b.tree.effective_props("ppunktatekst").unwrap();
    assert!(leaf.run.size.is_none() && eff.run.size == Some(24));
    assert!(leaf.para.indent.is_none() && eff.para.indent.is_some());
    assert!(
        leaf.para.alignment.is_none() && eff.para.alignment.is_some(),
        "alignment must come from Normalny layer, not leaf's own"
    );

    let b = load("poi-docx/59378.docx");
    let cs = b.tree.get("CommentSubject").unwrap().clone();
    let eff = b.tree.effective_props("CommentSubject").unwrap();
    assert!(cs.run.size.is_none() && eff.run.size == Some(20));
    assert!(cs.run.fonts.is_none() && eff.run.fonts.is_some());
}
