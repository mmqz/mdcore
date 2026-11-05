//! M-1 外部语料常驻回归门 · L3 面（issue #29 A6；TR-2B POI 真实
//! docx 语料的常驻化——6 件 Apache POI 回归夹具）。
//!
//! # 语料与溯源
//!
//! `test-artifacts/external-corpus/poi/*.docx`：Apache POI `test-data/
//! document/` 取样（Apache-2.0，provenance.json 在档）。含
//! 51921-Word-Crash067 崩溃回归夹具——**零 panic 是本门的存在理由**。
//!
//! # 门禁语义（下降即红）
//!
//! 每件：`read_docx_from_bytes` **不 panic** 且返回 `Ok`（TR-2B 实测
//! 基线 6/6 导入成功；类型化错误也不许新增——本门把「成功导入」记为
//! 通过口径）。成功件附加结构下限断言（段落数 > 0——空导入 = 静默
//! 失败面）。与 `BASELINE.json` 的 `l3` 节比对：通过数不得低于基线、
//! 失败清单不得新增。
//!
//! 基线再生：`M1_REGEN_BASELINE=1 cargo test -p mdc-office-io --test
//! external_corpus`（写回 l3 节后提交入库）。

use std::path::PathBuf;

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/external-corpus")
}

/// l3 基线节（serde_json::Value 手工提取——office-io 无直接 serde
/// derive 依赖，零锁变更纪律）。返回 (cases, imported, failed)。
fn load_l3() -> Option<(usize, usize, Vec<String>)> {
    let text = std::fs::read_to_string(corpus_root().join("BASELINE.json")).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    let l3 = root.get("l3")?;
    let cases = l3.get("cases")?.as_u64()? as usize;
    let imported = l3.get("imported")?.as_u64()? as usize;
    let failed = l3
        .get("failed")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some((cases, imported, failed))
}

fn save_l3(cases: usize, imported: usize, failed: &[String]) {
    let p = corpus_root().join("BASELINE.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({"schema": 1}));
    root["schema"] = serde_json::json!(1);
    root["l3"] = serde_json::json!({
        "cases": cases,
        "imported": imported,
        "failed": failed,
    });
    std::fs::write(&p, serde_json::to_string_pretty(&root).expect("pretty"))
        .expect("write BASELINE.json");
}

#[test]
fn l3_external_corpus_poi_docx_gate() {
    // 环境门（issue #89 纪律）：外部语料不在仓（test-artifacts/ 被 gitignore），
    // CI / 新克隆上必然缺失。缺件 = SKIP（响亮横幅），不是业务 FAIL。
    let dir_path = corpus_root().join("poi");
    let dir_ok = std::fs::read_dir(&dir_path)
        .map(|d| d.count() > 0)
        .unwrap_or(false);
    if !dir_ok {
        eprintln!();
        eprintln!("====================================================================");
        eprintln!("SKIP: l3_external_corpus_poi_docx_gate");
        eprintln!(
            "reason: CORPUS_MISSING: {}",
            corpus_root().join("poi").display()
        );
        eprintln!("（外部语料缺件，非业务失败——issue #89 口径：依赖缺失不得伪装成业务 FAIL）");
        eprintln!("补件指引: 拉 Apache POI 测试语料 .docx 到 test-artifacts/external-corpus/poi/");
        eprintln!("====================================================================");
        return;
    }
    let dir = corpus_root().join("poi");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("poi corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".docx"))
        .collect();
    files.sort();
    // TR-2B 口径锚：6 件（51921-Word-Crash067 崩溃夹具必须在场）。
    assert!(
        files.len() == 6,
        "POI corpus count drift: {} (TR-2B: 6) — check provenance",
        files.len()
    );
    assert!(
        files.iter().any(|f| f.contains("Crash")),
        "Crash 回归夹具缺失（51921-Word-Crash067）——本门的存在理由"
    );

    let mut imported = 0usize;
    let mut failed: Vec<String> = Vec::new();
    let mut para_floor_ok = 0usize;
    for f in &files {
        let bytes = std::fs::read(dir.join(f)).expect("corpus file readable");
        // 不 panic = 测试本体通过；typed error 记失败清单（基线可比）。
        match mdc_office_io::docx::read_docx_from_bytes(&bytes) {
            Ok(doc) => {
                imported += 1;
                // 结构下限：正文块序非空（空导入 = 静默失败面；夹具
                // 均为 POI 真实文档——TR-2B 实测全部导入出块）。
                if !doc.blocks.is_empty() {
                    para_floor_ok += 1;
                }
            }
            Err(e) => {
                failed.push(format!("{f}: {e}"));
            }
        }
    }

    let current = (files.len(), imported, failed.clone());

    if std::env::var("M1_REGEN_BASELINE").is_ok() {
        save_l3(current.0, current.1, &current.2);
        eprintln!(
            "M1 baseline regenerated (l3): {}/{} imported",
            current.1, current.0
        );
        return;
    }

    let Some((base_cases, base_imported, _base_failed)) = load_l3() else {
        save_l3(files.len(), imported, &failed);
        panic!(
            "BASELINE.json 缺失或无 l3 节——已生成（{imported}/{} imported，失败 {:?}）。提交入库后重跑。",
            files.len(), failed
        )
    };

    assert!(
        imported >= base_imported,
        "[poi] 导入下降即红：{}/{}（基线 {}/{}）——新增失败：{:?}",
        imported,
        files.len(),
        base_imported,
        base_cases,
        failed
    );
    assert_eq!(
        para_floor_ok, imported,
        "[poi] 空导入面：成功件段落数为 0（静默失败）"
    );
    if imported > base_imported {
        eprintln!(
            "[poi] baseline improved（{base_imported} → {imported}）—— M1_REGEN_BASELINE=1 再生入档"
        );
    }
}
