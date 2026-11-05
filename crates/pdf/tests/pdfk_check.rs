//! PDF-k —— pdfium 部署自检全链（#68 方向一；feature `engine-pdfium`）。
//!
//! **为什么是独立测试文件**：与 `tests/pdfk_deploy.rs` 同款纪律——
//! cargo 的每个集成测试文件 = 独立测试二进制 = **独立进程**。pdfium
//! 绑定是进程级单例（pdfium-render 绑定表为全局 OnceCell），自检走查
//! `run_check()` 的**真实绑定**只能发生在该进程的首个绑定里——本文件
//! 只含这一个绑定测试，不与其他测试二进制的绑定路径竞争。
//!
//! **验证面**（与单测分工——单测只覆盖零副作用面）：
//!
//! 1. `run_check()` 在库可定位环境必须报告**已绑定**（真实走查全链：
//!    探针收集 → 逐层绑定尝试 → 终态）；
//! 2. 胜出层自洽：终态来源层在探针序列中有对应 `bind = Some(Ok(()))`
//!    标记；路径形态（Some = 具体路径且文件存在 / None = 系统层）；
//! 3. 渲染面：`render_report` 输出含「已绑定」/ 来源 / 退出码 0 行；
//! 4. 副作用合同：走查绑定 = crate 进程单例就位（`PdfiumEngine::
//!    shared_handle` 复用成功——「自检进程与应用进程绑定行为面一致」
//!    的实证）。
//!
//! **环境合同（与 pdfk_deploy 同款——不静默 skip）**：库源（env →
//! 候选清单）缺失 = 本文件明确 panic 并打印全部已尝试位置。门禁环境
//! 必须可诊断，静默跳过 = 测试面造假。

// feature 门（默认构建零引擎依赖——本文件整体退化为空目标，跨环境
// 可构建纪律保持）。
#![cfg(feature = "engine-pdfium")]

use std::path::{Path, PathBuf};

use mdc_pdf::deploy_check::{render_report, run_check, BindOutcome, LayerKind};
use mdc_pdf::engine_pdfium::{PdfiumEngine, PDFIUM_LIB_ENV};

/// 库源可定位性预检（env → 全部候选清单）。注意：这是**预检**——
/// 若预检通过但 run_check 仍失败，那是真失败（如库损坏/架构不匹配），
/// 本测试会带着完整渲染报告红掉——正是自检工具要暴露的形态。
fn ensure_library_locatable(candidates: &[PathBuf]) {
    let mut tried: Vec<String> = Vec::new();
    if let Ok(path) = std::env::var(PDFIUM_LIB_ENV) {
        tried.push(format!(
            "{PDFIUM_LIB_ENV}={path}（存在：{}）",
            Path::new(&path).is_file()
        ));
        if Path::new(&path).is_file() {
            return;
        }
    }
    for candidate in candidates {
        tried.push(format!(
            "{}（存在：{}）",
            candidate.display(),
            candidate.is_file()
        ));
        if candidate.is_file() {
            return;
        }
    }
    panic!(
        "环境合同：pdfium 库源缺失（不静默 skip）。已尝试：{}",
        tried.join("；")
    );
}

/// 自检全链：库可定位环境 → run_check 必须已绑定 + 终态/探针/渲染/
/// 副作用四面自洽。
#[test]
fn check_reports_bound_when_library_locatable() {
    let candidates = PdfiumEngine::library_candidates();
    ensure_library_locatable(&candidates);

    let report = run_check();

    // ---- 1) 终态：必须已绑定 ----
    let (source, path) = match &report.outcome {
        BindOutcome::Bound { source, path } => (source.clone(), path.clone()),
        BindOutcome::Failed { .. } => {
            panic!(
                "run_check 未绑定（库源已预检在位——真失败形态）：\n{}",
                render_report(&report)
            )
        }
    };
    assert!(!source.is_empty(), "胜出层来源不得为空");
    assert!(report.outcome.is_bound(), "is_bound 退出码合同面");

    // ---- 2) 胜出层与探针序列自洽 ----
    let winners: Vec<_> = report
        .probes
        .iter()
        .filter(|p| p.bind == Some(Ok(())))
        .collect();
    assert_eq!(
        winners.len(),
        1,
        "恰一层胜出（生产定位序短路——余层不得再尝试）"
    );
    let winner = winners[0];
    assert_eq!(winner.title, source, "终态来源 = 探针中的胜出层标题");
    match (&winner.kind, &path) {
        // 系统层胜出：无显式路径（装载器解析）。
        (LayerKind::System, None) => {}
        // env/候选层胜出：胜出层期望路径 = 终态路径，且文件真实在位
        // （绑定成功的路径必存在——绑定本身已装载该文件）。
        (_, Some(bound_path)) => {
            assert_eq!(
                winner.expected.as_deref(),
                Some(bound_path.as_path()),
                "终态路径 = 胜出层期望路径"
            );
            assert!(bound_path.is_file(), "绑定路径文件在位");
        }
        _ => panic!(
            "胜出层与路径形态不匹配（kind={:?} path={:?}）",
            winner.kind, path
        ),
    }
    // 胜出层之后的层：未尝试（短路可观测）。
    let winner_idx = report
        .probes
        .iter()
        .position(|p| p.bind == Some(Ok(())))
        .expect("上面已断言恰一胜出层");
    for probe in &report.probes[winner_idx + 1..] {
        assert!(
            probe.bind.is_none(),
            "胜出层之后的层必须未尝试（{}）",
            probe.title
        );
    }

    // ---- 3) 渲染面 ----
    let text = render_report(&report);
    assert!(text.contains("绑定结果: 已绑定"));
    assert!(text.contains(&source), "渲染含来源层");
    assert!(text.contains("退出码: 0（已绑定）"));
    if let Some(path) = &path {
        assert!(text.contains(&path.display().to_string()), "渲染含绑定路径");
    }

    // ---- 4) 副作用合同：走查绑定 = crate 进程单例就位 ----
    // （自检进程与应用进程的绑定行为面一致——run_check 成功后，
    // shared_handle 不得再走定位序，直接复用单例。）
    assert!(
        mdc_pdf::engine_pdfium::PdfiumEngine::shared_handle().is_ok(),
        "run_check 绑定后 crate 单例必须就位（shared_handle 复用成功）"
    );
}
