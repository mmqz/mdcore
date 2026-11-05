//! M-1 外部语料常驻回归门 · L2 面（issue #29 A6；TR-2B pandoc 语料
//! 的常驻化——4 件全特性 markdown → docx 导出面）。
//!
//! # 语料与溯源
//!
//! `test-artifacts/external-corpus/pandoc/`：jgm/pandoc `test/` 四件
//! （writer/tables/reader-more/lhs，GPL-2.0+——**仅输入数据**，许可
//! 纪律见 provenance.json）。全特性输入（pandoc 全特性游行/表格/
//! reader 边界/lhs 混排）。
//!
//! # 门禁语义（下降即红）
//!
//! 每件：mdcore 解析（不 panic）→ `export_docx_core`（BlockTree 到
//! DocxWriter 到 zip 字节）→ **zip 容器有效**（`[Content_Types].xml`
//! 与 `word/document.xml` 在场）且字节数 ≥ 基线下限。docx 字节数记
//! 基线信息（不做字节级门——导出面合法改进会变字节；门 = 导出成功
//! 与容器有效性）。与 `BASELINE.json` 的 `l2` 节比对：通过数不得
//! 低于基线。
//!
//! 基线再生：`M1_REGEN_BASELINE=1 cargo test -p mdcore-wasm --test
//! external_corpus`（写回 l2 节后提交入库）。

use std::path::PathBuf;

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/external-corpus")
}

/// l2 基线节（serde_json::Value 手工提取——mdcore-wasm 无直接 serde
/// derive 依赖，零锁变更纪律）。返回 (cases, passed, failed)。
fn load_l2() -> Option<(usize, usize, Vec<String>)> {
    let text = std::fs::read_to_string(corpus_root().join("BASELINE.json")).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    let l2 = root.get("l2")?;
    let cases = l2.get("cases")?.as_u64()? as usize;
    let passed = l2.get("passed")?.as_u64()? as usize;
    let failed = l2
        .get("failed")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some((cases, passed, failed))
}

fn save_l2(cases: usize, passed: usize, failed: &[String]) {
    let p = corpus_root().join("BASELINE.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({"schema": 1}));
    root["schema"] = serde_json::json!(1);
    root["l2"] = serde_json::json!({
        "cases": cases,
        "passed": passed,
        "failed": failed,
    });
    std::fs::write(&p, serde_json::to_string_pretty(&root).expect("pretty"))
        .expect("write BASELINE.json");
}

/// zip 容器有效性（TR-2B L2 判据同构：Content_Types + document.xml；
/// 与 src/docx_export.rs 测试面 zip_stored_part 同款手写 PK 头遍历——
/// 写器产 stored 条目，零三方依赖纪律）。
fn zip_entry_names(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut i = 0usize;
    while i + 30 <= bytes.len() {
        if &bytes[i..i + 4] != b"PK\x03\x04" {
            break; // 中央目录/EOCD 起点或非 zip——终止
        }
        let size = u32::from_le_bytes([bytes[i + 18], bytes[i + 19], bytes[i + 20], bytes[i + 21]])
            as usize;
        let name_len = u16::from_le_bytes([bytes[i + 26], bytes[i + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[i + 28], bytes[i + 29]]) as usize;
        let entry_name = String::from_utf8_lossy(&bytes[i + 30..i + 30 + name_len]).into_owned();
        names.push(entry_name);
        i = i + 30 + name_len + extra_len + size;
    }
    names
}

fn zip_valid(bytes: &[u8]) -> bool {
    let names = zip_entry_names(bytes);
    names.iter().any(|n| n == "[Content_Types].xml")
        && names.iter().any(|n| n == "word/document.xml")
}

#[test]
fn l2_external_corpus_pandoc_export_gate() {
    // 环境门（issue #89 纪律）：外部语料不在仓（test-artifacts/ 被 gitignore），
    // CI / 新克隆上必然缺失。缺件 = SKIP（响亮横幅），不是业务 FAIL。
    let dir_path = corpus_root().join("pandoc");
    let dir_ok = std::fs::read_dir(&dir_path)
        .map(|d| d.count() > 0)
        .unwrap_or(false);
    if !dir_ok {
        eprintln!();
        eprintln!("====================================================================");
        eprintln!("SKIP: l2_external_corpus_pandoc_export_gate");
        eprintln!(
            "reason: CORPUS_MISSING: {}",
            corpus_root().join("pandoc").display()
        );
        eprintln!("（外部语料缺件，非业务失败——issue #89 口径：依赖缺失不得伪装成业务 FAIL）");
        eprintln!("补件指引: 拉 pandoc 导出的 .markdown/.txt 语料到 test-artifacts/external-corpus/pandoc/");
        eprintln!("====================================================================");
        return;
    }
    let dir = corpus_root().join("pandoc");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("pandoc corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".markdown") || n.ends_with(".txt"))
        .collect();
    files.sort();
    // TR-2B 口径锚：4 件。
    assert_eq!(files.len(), 4, "pandoc corpus count drift: {}", files.len());

    let mut passed = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for f in &files {
        let md = std::fs::read_to_string(dir.join(f)).expect("corpus file readable");
        // 解析不 panic（崩溃 = 测试失败）；导出 + 容器校验。
        let core = mdcore::Core::from_markdown(&md);
        let bytes = match mdcore_wasm::docx_export::export_docx_core(&core.tree) {
            Ok(b) => b,
            Err(e) => {
                failed.push(format!("{f}: export {e:?}"));
                continue;
            }
        };
        if bytes.len() < 1024 || !zip_valid(&bytes) {
            failed.push(format!("{f}: zip invalid or too small ({}B)", bytes.len()));
            continue;
        }
        passed += 1;
    }

    if std::env::var("M1_REGEN_BASELINE").is_ok() {
        save_l2(files.len(), passed, &failed);
        eprintln!(
            "M1 baseline regenerated (l2): {passed}/{} passed",
            files.len()
        );
        return;
    }

    let Some((base_cases, base_passed, _base_failed)) = load_l2() else {
        save_l2(files.len(), passed, &failed);
        panic!(
            "BASELINE.json 缺失或无 l2 节——已生成（{passed}/{} passed，失败 {:?}）。提交入库后重跑。",
            files.len(), failed
        )
    };

    assert!(
        passed >= base_passed,
        "[pandoc] 导出下降即红：{}/{}（基线 {}/{}）——新增失败：{:?}",
        passed,
        files.len(),
        base_passed,
        base_cases,
        failed
    );
    if passed > base_passed {
        eprintln!(
            "[pandoc] baseline improved（{base_passed} → {passed}）—— M1_REGEN_BASELINE=1 再生入档"
        );
    }
}
