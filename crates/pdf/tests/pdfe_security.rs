//! PDF-e — S2/S6 打开时安全扫描语义锁（feature `engine-lopdf`；
//! SECURITY_POLICY S2.4 open_scan 产品面 + S6 表单探针 + S3 打开面
//! 限额接线）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**；敌意 fixtures
//! 全部 lopdf 现场构造——零仓外依赖，净克隆可复现）：
//! 1. `scan_benign_document_zero_warnings`——良性文档零告警 + 无表单
//!    + 非加密 + 报告确定性（同文档两扫逐位相等）；
//! 2. `scan_detects_all_eight_families`——八族全检测（单文档多族混合
//!    + 不可达敌意对象同样可见——对象图全扫而非可达性走查）；
//! 3. `scan_family_details_and_dedup`——族详情形状（对象号定位）+
//!    同 dict 同族合并（/S /URI 与 /URI 键双触发 = 一条 hit）；
//! 4. `scan_form_probe_acroform_fields`——S6 表单探针（字段枚举 +
//!    层级全名 `.` 连接 + /Tx 现值可读 + 字段计数；**表单非危险族**
//!    ——warnings 恒空：被动可读面告警语义与 S2 危险族分立）；
//! 5. `scan_encrypted_flag_and_detection_boundary`——加密旗标 + 检测
//!    边界诚实性（仍加密：lopdf 未解密装载不解析对象图——检测面
//!    不可用、旗标 true；曾加密[空口令自动解密]：明文对象图可见——
//!    检测照常）；
//! 6. `writer_from_bytes_carries_assessment`——写器装载后扫描的产品
//!    面出口（assessment() 与 open_scan 同结果）；
//! 7. `open_scan_rejects_oversized_input`——S3-L7 输入字节双入口
//!    （open_scan + from_bytes_with_limits）超限 typed 拒 + 边界放行；
//! 8. `open_scan_rejects_over_budget`——S3-L4 打开预算双入口（注入
//!    0 预算 = 确定性触发）；
//! 9. `session_open_with_scanned_assessment_end_to_end`——会话端到端
//!    （扫描评估喂 PdfSession::open：告警不阻断打开 + 评估可查询 +
//!    非加密态写命令正常——S2.4 只读告警语义的产品面闭环）；
//! 10. `scan_depth_guard_bounds_recursion`——深度护栏判别式（**内存面
//!     直扫**：护栏外 /AA 不可见、护栏内可见——短路验证发现文件面
//!     恒真[lopdf 解析器嵌套预算 MAX_NESTING_DEPTH=100 先行丢弃超深
//!     对象，探针实证]，重构为 scan_document 内存面真判别 + 文件面
//!     分层在档断言）；
//! 11. `scan_form_walk_terminates_on_cycles`——字段树环防（/Kids
//!     自引用终止 + 计数不重复）。

#![cfg(feature = "engine-lopdf")]

use mdc_pdf::command::CommandError;
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageGeometry, Rotation};
use mdc_pdf::security::{
    DangerousFamily, Limits, OpenAssessment, REASON_OPEN_BUDGET, REASON_OPEN_FILE_BYTES,
};
use mdc_pdf::security_scan::{open_scan, scan_document};
use mdc_pdf::writer_lopdf::{lopdf, LopdfWriter};

use lopdf::{dictionary, Object, Stream, StringFormat};

// ---------------------------------------------------------------------------
// fixture 构造（lopdf 现场构造——spike build_form_pdf 同款惯例）
// ---------------------------------------------------------------------------

/// 存活到 Vec<u8>（save_to 内存序列化）。
fn save(doc: &mut lopdf::Document) -> Vec<u8> {
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("fixture 序列化");
    bytes
}

/// 最小良性文档（1 页 Letter）+ 页增补键 + catalog 增补键（敌意载荷
/// 注入点——敌意对象挂页 /Annots 或 catalog 动作面）。
fn doc_with(
    page_extra: Vec<(&'static str, Object)>,
    catalog_extra: Vec<(&'static str, Object)>,
) -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut page = dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    };
    for (key, value) in page_extra {
        page.set(key, value);
    }
    let page_id = doc.add_object(Object::Dictionary(page));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let mut catalog = dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
    };
    for (key, value) in catalog_extra {
        catalog.set(key, value);
    }
    let catalog_id = doc.add_object(Object::Dictionary(catalog));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    save(&mut doc)
}

/// 良性对照文档。
fn benign_pdf() -> Vec<u8> {
    doc_with(vec![], vec![])
}

fn name(value: &str) -> Object {
    Object::Name(value.as_bytes().to_vec())
}

fn text_string(value: &str) -> Object {
    Object::String(value.as_bytes().to_vec(), StringFormat::Literal)
}

/// 断言告警族集合（顺序无关——报告序是族声明序 × 详情序，锁以集合
/// 断言免双维护）。
fn families(assessment: &OpenAssessment) -> Vec<DangerousFamily> {
    let mut out: Vec<DangerousFamily> = assessment.warnings.iter().map(|h| h.family).collect();
    out.sort_by_key(|family| format!("{family:?}"));
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// 1. 良性文档零告警 + 确定性
// ---------------------------------------------------------------------------

#[test]
fn scan_benign_document_zero_warnings() {
    let bytes = benign_pdf();
    let assessment = open_scan(&bytes, &Limits::default()).expect("良性文档扫描");
    assert!(
        assessment.warnings.is_empty(),
        "良性文档零告警（误报 = 告警面失效）：{:?}",
        assessment.warnings
    );
    assert!(!assessment.encrypted, "非加密");
    assert!(
        assessment.form_probe.is_none(),
        "无 /AcroForm → form_probe = None"
    );
    // 报告确定性：同文档两扫逐位相等（对象图 BTreeMap 序 + 显式排序
    // 双保险——报告可作可复现证据）。
    let again = open_scan(&bytes, &Limits::default()).expect("复扫");
    assert_eq!(assessment, again, "同文档两扫报告逐位相等（确定性）");
}

// ---------------------------------------------------------------------------
// 2. 八族全检测（多族混合 + 不可达对象可见）
// ---------------------------------------------------------------------------

#[test]
fn scan_detects_all_eight_families() {
    // 不可达敌意对象（不挂任何页/catalog——对象图全扫语义：检测不
    // 依赖解析器可达性判定，敌意载荷可藏于未引用对象）。
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    }));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        // ① /OpenAction（catalog 打开动作——/GoTo 目的地形态，无执行面）。
        "OpenAction" => dictionary! {
            "Type" => name("Action"),
            "S" => name("GoTo"),
            "D" => vec![Object::Integer(0), name("Fit")],
        },
        // ⑦ /Names → /JavaScript 文档级 JS 名字树（⑧ NamesJavaScript 族）。
        "Names" => dictionary! {
            "JavaScript" => dictionary! {
                "Names" => vec![text_string("init"), Object::Reference((999, 0))],
            },
            // ⑤ /EmbeddedFiles 名字树标记。
            "EmbeddedFiles" => dictionary! {
                "Names" => vec![text_string("payload.txt")],
            },
        },
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    // ② 页 /AA 附加动作。
    let page_dict = match doc.get_object_mut(page_id) {
        Ok(Object::Dictionary(dict)) => dict,
        other => panic!("页对象形态：{other:?}"),
    };
    page_dict.set(
        "AA",
        dictionary! {
            "O" => dictionary! { "Type" => name("Action"), "S" => name("URI") },
        },
    );
    // ③ Launch 动作（不可达对象）。
    doc.add_object(Object::Dictionary(dictionary! {
        "Type" => name("Action"),
        "S" => name("Launch"),
        "F" => text_string("/usr/bin/curl"),
        "Win" => dictionary! { "F" => text_string("calc.exe") },
    }));
    // ④ URI 动作（不可达对象——载荷键 /URI 双探测面）。
    doc.add_object(Object::Dictionary(dictionary! {
        "Type" => name("Action"),
        "S" => name("URI"),
        "URI" => text_string("https://evil.example/exfil"),
    }));
    // ⑥ RichMedia 注释（不可达对象）。
    doc.add_object(Object::Dictionary(dictionary! {
        "Type" => name("Annot"),
        "Subtype" => name("RichMedia"),
        "Rect" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(10), Object::Integer(10)],
        "RichMediaSettings" => dictionary! { "Activation" => dictionary! {} },
    }));
    // ⑦ JavaScript 动作（不可达对象——/S /JavaScript + /JS 载荷）。
    doc.add_object(Object::Dictionary(dictionary! {
        "Type" => name("Action"),
        "S" => name("JavaScript"),
        "JS" => text_string("app.alert(1)"),
    }));
    // ⑤' /Type /EmbeddedFile 流本体（不可达对象——嵌入文件实体面）。
    doc.add_object(Stream::new(
        dictionary! { "Type" => name("EmbeddedFile"), "Length" => Object::Integer(3) },
        b"abc".to_vec(),
    ));
    let bytes = save(&mut doc);

    let assessment = open_scan(&bytes, &Limits::default()).expect("敌意混合文档扫描");
    let expect = vec![
        DangerousFamily::AdditionalActions,
        DangerousFamily::EmbeddedFiles,
        DangerousFamily::JavaScript,
        DangerousFamily::LaunchAction,
        DangerousFamily::NamesJavaScript,
        DangerousFamily::OpenAction,
        DangerousFamily::RichMedia,
        DangerousFamily::UriAction,
    ];
    assert_eq!(
        families(&assessment),
        expect,
        "八族全检出（对齐 SECURITY_POLICY S2.2/S2.3 全清单）"
    );
    // 每族至少一条（详情含对象定位面）。
    assert!(
        assessment
            .warnings
            .iter()
            .all(|hit| hit.detail.contains("obj ") || hit.detail.contains("树")),
        "每条命中携带定位详情：{:?}",
        assessment.warnings
    );
}

// ---------------------------------------------------------------------------
// 3. 族详情形状 + 同 dict 同族合并
// ---------------------------------------------------------------------------

#[test]
fn scan_family_details_and_dedup() {
    // 单一 URI 动作（/S /URI 与 /URI 键**双触发**——一 dict 每族至多
    // 一条 hit，不因多触发条件重复告警）。
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    }));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let uri_action_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => name("Action"),
        "S" => name("URI"),
        "URI" => text_string("https://example.com/"),
    }));
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "OpenAction" => Object::Reference(uri_action_id),
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let bytes = save(&mut doc);

    let assessment = open_scan(&bytes, &Limits::default()).expect("扫描");
    // 双族：/OpenAction（catalog 键）+ /UriAction（动作对象）——同一
    // 载荷的族语义分立告警；每族恰一条（去重合并）。
    assert_eq!(
        families(&assessment),
        vec![DangerousFamily::OpenAction, DangerousFamily::UriAction,]
    );
    let uri_hits: Vec<&str> = assessment
        .warnings
        .iter()
        .filter(|hit| hit.family == DangerousFamily::UriAction)
        .map(|hit| hit.detail.as_str())
        .collect();
    assert_eq!(
        uri_hits,
        vec![format!(
            "obj {} {}: /URI 动作",
            uri_action_id.0, uri_action_id.1
        )],
        "详情形状 = 对象号定位（可诊断面）——同 dict 双触发合并为一条"
    );
}

// ---------------------------------------------------------------------------
// 4. S6 表单探针
// ---------------------------------------------------------------------------

#[test]
fn scan_form_probe_acroform_fields() {
    // build_form_pdf 同款惯例扩展：/Tx 带值文本域 + 层级字段（parent.
    // kid）+ 无值按钮域——三形态枚举。
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    }));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let text_field_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => name("Annot"),
        "Subtype" => name("Widget"),
        "FT" => name("Tx"),
        "T" => text_string("probe_text"),
        "V" => text_string("hello form"),
    }));
    // 层级字段：parent（无 /FT 的命名节点）→ kid（/Tx 带值）。
    let kid_id = doc.add_object(Object::Dictionary(dictionary! {
        "FT" => name("Tx"),
        "T" => text_string("kid"),
        "V" => text_string("nested"),
    }));
    let parent_id = doc.add_object(Object::Dictionary(dictionary! {
        "T" => text_string("parent"),
        "Kids" => vec![Object::Reference(kid_id)],
    }));
    let toggle_id = doc.add_object(Object::Dictionary(dictionary! {
        "FT" => name("Btn"),
        "T" => text_string("toggle"),
    }));
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => dictionary! {
            "Fields" => vec![
                Object::Reference(text_field_id),
                Object::Reference(parent_id),
                Object::Reference(toggle_id),
            ],
        },
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let bytes = save(&mut doc);

    let assessment = open_scan(&bytes, &Limits::default()).expect("表单文档扫描");
    // 表单 = 被动可读面，**不是**危险对象族——warnings 恒空（S6 与
    // S2 分立：字段枚举进报告元数据并告警，永不初始化交互 formfill）。
    assert!(
        assessment.warnings.is_empty(),
        "表单字段不产生 S2 危险族告警（S6 语义分立）：{:?}",
        assessment.warnings
    );
    let probe = assessment
        .form_probe
        .as_ref()
        .expect("/AcroForm 在位 → form_probe = Some");
    // 字段判定 = /FT 或 /T 在位（容器节点[T 无 FT]也是字段——PDF 全名
    // 语义的中间节点）：probe_text + parent + parent.kid + toggle = 4。
    assert_eq!(
        probe.field_count, 4,
        "字段总数（含命名容器节点 parent——/T 在位即字段）"
    );
    // 枚举序 = 全名字典序（确定性报告）；层级全名 `.` 连接；/Tx 现值
    // 可读；Btn 无值 = None。
    let names: Vec<(&str, Option<&str>)> = probe
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.value.as_deref()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("parent", None),
            ("parent.kid", Some("nested")),
            ("probe_text", Some("hello form")),
            ("toggle", None),
        ],
        "S6 探针形状：全名连接 + /Tx 现值可读 + 无值 None（/Tx 字段名值可读进评估——探针只读面）"
    );
}

// ---------------------------------------------------------------------------
// 5. 加密旗标 + 检测边界（诚实性）
// ---------------------------------------------------------------------------

#[test]
fn scan_encrypted_flag_and_detection_boundary() {
    // 加密双形态的检测边界（lopdf 装载语义，诚实性在档）：
    // - **仍加密**（非空口令）：lopdf 以未解密形态装载——对象图**不解析**
    //   （仅 /Encrypt 字典可见），S2 检测面不可用（检测需要明文对象
    //   图）；encrypted 旗标 = true（会话层据此拒写，S5）。
    // - **曾加密**（空口令可解）：装载时自动解密——对象图完整可见，
    //   S2 检测照常（is_encrypted=false / was_encrypted=true）。
    use lopdf::{EncryptionVersion, Permissions};
    let build = |user_password: &str| {
        let mut doc = lopdf::Document::with_version("1.7");
        doc.trailer.set(
            "ID",
            Object::Array(vec![
                text_string("mdc-pdf-pdfe-scan-id"),
                text_string("mdc-pdf-pdfe-scan-id"),
            ]),
        );
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ],
        }));
        doc.set_object(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => Object::Name(b"Pages".to_vec()),
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => Object::Integer(1),
            }),
        );
        let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Catalog".to_vec()),
            "Pages" => Object::Reference(pages_id),
            "OpenAction" => dictionary! { "S" => name("GoTo") },
        }));
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let state: lopdf::EncryptionState = EncryptionVersion::V1 {
            document: &doc,
            owner_password: "owner-secret",
            user_password,
            permissions: Permissions::all(),
        }
        .try_into()
        .expect("加密状态构造");
        doc.encrypt(&state).expect("对象加密");
        save(&mut doc)
    };

    // ① 仍加密（非空口令）：旗标 true + 检测面不可用（对象图不解析）。
    let bytes = build("user-secret");
    let assessment = open_scan(&bytes, &Limits::default()).expect("加密档扫描（S5.1 只读支持）");
    assert!(assessment.encrypted, "仍加密旗标（会话层据此拒写——S5）");
    assert!(
        assessment.warnings.is_empty(),
        "仍加密态：lopdf 未解密装载不解析对象图——S2 检测面不可用（诚实边界：无假阳/假阴，检测需明文对象图）"
    );

    // ② 曾加密（空口令可解）：装载自动解密 → 对象图可见 → 检测照常。
    let bytes = build("");
    let assessment = open_scan(&bytes, &Limits::default()).expect("曾加密档扫描");
    assert!(
        !assessment.encrypted,
        "空口令自动解密（was_encrypted 形态）"
    );
    assert_eq!(
        families(&assessment),
        vec![DangerousFamily::OpenAction],
        "曾加密档：明文对象图可见 → /OpenAction 照常检出（检测面对可解密档有效）"
    );
}

// ---------------------------------------------------------------------------
// 6. 写器 assessment 出口
// ---------------------------------------------------------------------------

#[test]
fn writer_from_bytes_carries_assessment() {
    // 产品面出口：from_bytes 装载后扫描随写器携带（宿主经 assessment()
    // 取出喂 PdfSession::open——与 open_scan 独立路径同结果）。
    let bytes = doc_with(
        vec![],
        vec![(
            "OpenAction",
            Object::Dictionary(dictionary! { "S" => name("GoTo") }),
        )],
    );
    let scanned = open_scan(&bytes, &Limits::default()).expect("open_scan 路径");
    let writer =
        LopdfWriter::from_bytes(bytes).expect("写器构造（含敌意动作仍打开——只检测不拒绝）");
    assert_eq!(
        writer.assessment(),
        &scanned,
        "from_bytes 装载后扫描与 open_scan 同结果（同一扫描单一实现）"
    );
    assert!(
        matches!(
            families(writer.assessment()).as_slice(),
            [DangerousFamily::OpenAction]
        ),
        "写器携带的评估含 OpenAction 告警"
    );
}

// ---------------------------------------------------------------------------
// 7. S3-L7 输入字节双入口
// ---------------------------------------------------------------------------

#[test]
fn open_scan_rejects_oversized_input() {
    let bytes = benign_pdf();
    // 注入小上限（限额注入形态——默认 1 GiB 的实跑不可行，注入后
    // 端到端语义与默认面同构：同一条检查路径）。
    let tight = Limits {
        open_file_max_bytes: bytes.len() as u64,
        ..Limits::default()
    };
    assert!(
        open_scan(&bytes, &tight).is_ok(),
        "恰等上限放行（硬拒绝族边界）"
    );
    let over = Limits {
        open_file_max_bytes: bytes.len() as u64 - 1,
        ..Limits::default()
    };
    // 双入口同款拒绝（S5 双防线风格的打开面泛化）。
    assert_eq!(
        open_scan(&bytes, &over),
        Err(CommandError::SecurityDeny {
            reason: REASON_OPEN_FILE_BYTES
        }),
        "open_scan 入口：超限 typed 拒（先于解析）"
    );
    assert!(
        matches!(
            LopdfWriter::from_bytes_with_limits(bytes.clone(), over),
            Err(CommandError::SecurityDeny {
                reason: REASON_OPEN_FILE_BYTES
            })
        ),
        "from_bytes 入口：超限 typed 拒（与 open_scan 同款）"
    );
}

// ---------------------------------------------------------------------------
// 8. S3-L4 打开预算双入口
// ---------------------------------------------------------------------------

#[test]
fn open_scan_rejects_over_budget() {
    // 0 预算 = 确定性触发（Duration 全精度比较——任何真实装载耗时
    // > 0ns 必拒；默认 3s 预算的正控由全部既有装载锁背书）。
    let zero = Limits {
        open_budget_ms: 0,
        ..Limits::default()
    };
    let bytes = benign_pdf();
    assert_eq!(
        open_scan(&bytes, &zero),
        Err(CommandError::SecurityDeny {
            reason: REASON_OPEN_BUDGET
        }),
        "open_scan 入口：装载耗时超 0 预算 → typed 拒"
    );
    assert!(
        matches!(
            LopdfWriter::from_bytes_with_limits(bytes, zero),
            Err(CommandError::SecurityDeny {
                reason: REASON_OPEN_BUDGET
            })
        ),
        "from_bytes 入口：预算拒在装载+扫描之后（整段计时）"
    );
}

// ---------------------------------------------------------------------------
// 9. 会话端到端（评估流转产品面闭环）
// ---------------------------------------------------------------------------

#[test]
fn session_open_with_scanned_assessment_end_to_end() {
    // S2.4 产品面闭环：敌意文档 → open_scan → 评估喂 PdfSession::open
    // → 打开成功（**告警不阻断打开**）+ 评估可查询 + 非加密态写命令
    // 正常（检测与执行面分立——v0 无动作解释器）。
    let bytes = doc_with(
        vec![(
            "AA",
            Object::Dictionary(dictionary! { "O" => dictionary! { "S" => name("URI") } }),
        )],
        vec![(
            "OpenAction",
            Object::Dictionary(dictionary! { "S" => name("GoTo") }),
        )],
    );
    let assessment = open_scan(&bytes, &Limits::default()).expect("敌意文档扫描（只读不阻断）");
    let mut session = PdfSession::open(
        DocumentMeta {
            id: PdfDocumentId(9),
            file_name: "hostile.pdf".into(),
            page_count: PageCount(1),
        },
        assessment,
        vec![PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg0,
        }],
        OutlineTree::new(),
    )
    .expect("S2.4：检测告警不阻断打开（用户自行决定去留）");
    assert_eq!(
        families(session.assessment()),
        vec![
            DangerousFamily::AdditionalActions,
            DangerousFamily::OpenAction,
            DangerousFamily::UriAction,
        ],
        "会话评估可查询（open 参数原样流转——单一评估对象端到端）"
    );
    // 非加密态：写命令不受 S5 拒（S2 告警 ≠ S5 加密态——两策略分立）。
    use mdc_pdf::annotation::{AnnotationKind, Origin};
    use mdc_pdf::command::{Command, Source};
    use mdc_pdf::page::PageIndex;
    let add = session.execute(Command::AnnotateAdd {
        draft: mdc_pdf::annotation::AnnotationDraft {
            page: PageIndex(0),
            kind: AnnotationKind::Note {
                rect: mdc_pdf::annotation::Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
            },
            author: "reader".into(),
            origin: Origin::Human,
        },
        source: Source::View,
    });
    assert!(
        add.is_ok(),
        "S2 告警不影响写命令（非加密态）——检测与执行面分立"
    );
}

// ---------------------------------------------------------------------------
// 10. 深度护栏（判别式——内存面直扫；文件面由 lopdf 解析器先行截断）
// ---------------------------------------------------------------------------

#[test]
fn scan_depth_guard_bounds_recursion() {
    // 病态深嵌套（数组链底部藏 /AA）。**面区分（短路验证发现的锁
    // 空转，本锁重构为真判别）**：走 save/load 文件往返时，lopdf 0.45
    // 解析器自身的嵌套预算（reader::MAX_NESTING_DEPTH = 100）会静默
    // 丢弃超深对象（诊断探针实证：100 层链序列化后 /AA 仍在字节里，
    // load 后对象易号消失——99 层完好）。「护栏外不可见」若经文件面
    // 断言则恒真（护栏不可观测 = 锁空转）；故判别式断言对**内存构造**
    // 的 Document 直扫（`scan_document` 公开面——对象可见性只由扫描
    // 护栏决定），文件面的解析器截断作为独立上游防线一并入档断言。
    let build = |depth: usize| {
        let mut chain = Object::Dictionary(dictionary! { "AA" => dictionary! {} });
        for _ in 0..depth {
            chain = Object::Array(vec![chain]);
        }
        let mut doc = lopdf::Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ],
        }));
        doc.set_object(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => Object::Name(b"Pages".to_vec()),
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => Object::Integer(1),
            }),
        );
        doc.add_object(chain);
        let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Catalog".to_vec()),
            "Pages" => Object::Reference(pages_id),
            // 护栏内对照：浅位 /OpenAction 照常检出。
            "OpenAction" => Object::Dictionary(dictionary! { "S" => name("GoTo") }),
        }));
        doc.trailer.set("Root", Object::Reference(catalog_id));
        doc
    };

    // (a) 内存面判别式：深链（200 层 > object_depth_max 128）护栏外
    // ——/AA 不检测（扫描栈不爆：递归深度有界）；护栏内 /OpenAction
    // 照常（对照）。
    let deep = scan_document(&build(200), &Limits::default());
    assert_eq!(
        families(&deep),
        vec![DangerousFamily::OpenAction],
        "护栏外（200 层深的 /AA）不检测——深度有界的检测边界；护栏内 /OpenAction 照常（对照）"
    );

    // 判别式正控：同载荷提至护栏内（10 层）→ /AA 可见（证明「不可见」
    // 是护栏语义而非载荷形态问题）。
    let shallow = scan_document(&build(10), &Limits::default());
    assert_eq!(
        families(&shallow),
        vec![
            DangerousFamily::AdditionalActions,
            DangerousFamily::OpenAction,
        ],
        "正控：护栏内（10 层）的 /AA 照常检出"
    );

    // (b) 文件面分层在档：同一深链经 save/load——lopdf 解析器嵌套预算
    // （MAX_NESTING_DEPTH = 100 < 128 护栏）先行丢弃超深对象，/AA 同样
    // 不可见（上游防线独立成立；扫描护栏在文件面不可观测，只对内存面
    // /未来解析器变化生效）。
    let mut bytes = Vec::new();
    build(200)
        .save_to(&mut bytes)
        .expect("深链序列化（字节面 /AA 在场）");
    let file_face = open_scan(&bytes, &Limits::default()).expect("文件面扫描（不爆栈）");
    assert_eq!(
        families(&file_face),
        vec![DangerousFamily::OpenAction],
        "文件面：解析器嵌套预算先行截断（100 < 128）——两层防线各司其职，护栏空转面在档"
    );
}

// ---------------------------------------------------------------------------
// 11. 字段树环防
// ---------------------------------------------------------------------------

#[test]
fn scan_form_walk_terminates_on_cycles() {
    // /Kids 自引用字段（敌意环——seen-set 环防终止；无环防 = 无限
    // 递归/爆栈）。计数恰 1（环边不产生第二字段）。
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    }));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    // 自引用字段：field /Kids [field]——环。
    let field_id = doc.new_object_id();
    doc.set_object(
        field_id,
        Object::Dictionary(dictionary! {
            "FT" => name("Tx"),
            "T" => text_string("ouroboros"),
            "Kids" => vec![Object::Reference(field_id)],
        }),
    );
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => dictionary! {
            "Fields" => vec![Object::Reference(field_id)],
        },
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let bytes = save(&mut doc);

    let assessment =
        open_scan(&bytes, &Limits::default()).expect("环字段扫描终止（seen-set 环防）");
    let probe = assessment.form_probe.as_ref().expect("表单在位");
    assert_eq!(probe.field_count, 1, "环边不重复计数（节点至多游走一次）");
    assert_eq!(probe.fields.len(), 1, "枚举恰一项：{:?}", probe.fields);
    assert_eq!(probe.fields[0].name, "ouroboros");
}
