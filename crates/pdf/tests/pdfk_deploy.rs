//! PDF-k —— pdfium 部署位验证（#210 真实环境验证收口；feature
//! `engine-pdfium`）。
//!
//! **为什么是独立测试文件**：cargo 的每个集成测试文件 = 独立测试
//! 二进制 = **独立进程**。pdfium 绑定是进程级单例（pdfium-render
//! 绑定表为全局 OnceCell，二次绑定报 `PdfiumLibraryBindings-
//! AlreadyInitialized`），「exe 同目录候选是否被定位序真实选中」
//! 只能在**该进程的首个绑定**里验证——本文件只含这一个绑定测试，
//! 不与 pdfc_engine 等其他测试二进制的绑定路径竞争（那些二进制
//! 各自独立进程，互不影响）。
//!
//! **验证形态（INSTALL.md「方式 B：放 exe 同目录」的自动化背书）**：
//!
//! 1. 从取件位（env `MDC_PDFIUM_LIB` → 候选清单开发位）定位库源，
//!    复制到 `current_exe()` 同目录（= 候选 0 部署位）；
//! 2. 清除 `MDC_PDFIUM_LIB`（防 CI 以 env 提供库时定位序第一步
//!    短路——`remove_var` 只作用于本测试进程，不影响 CI 为其他
//!    测试二进制设置的 env）；
//! 3. `PdfiumEngine::open`（定位序绑定）必须成功——结合形状锁
//!    （`engine_pdfium::tests::exe_dir_is_first_candidate`：候选 0
//!    = exe 同目录）与本测试内联复核，绑定来源唯一可能是刚放置
//!    的部署位库（候选 0 在最前且已存在——按序尝试首个成功）。
//!
//! **环境合同（与 pdfc_engine 同款——不静默 skip）**：库源或
//! fixtures 缺失 = 本文件明确 panic 并打印全部已尝试位置。门禁
//! 环境必须可诊断，静默跳过 = 测试面造假。

// feature 门（默认构建零引擎依赖——本文件整体退化为空目标，跨环境
// 可构建纪律保持）。
#![cfg(feature = "engine-pdfium")]

use std::path::{Path, PathBuf};

use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::{PdfiumEngine, PDFIUM_LIB_ENV};

/// fixtures 定位环境变量（与 pdfc_engine 同名同序）。
const FIXTURES_ENV: &str = "MDC_PDF_TEST_FIXTURES";

/// small.pdf 的页数基线（pdfc_engine 锁 1 同源：spike 探针 fixture）。
const SMALL_PDF_PAGES: u32 = 3;

/// fixtures 目录定位（env → crate 本地 → spike 位——与 pdfc_engine
/// 的 `fixture_candidates` 同序简化版：crate 本地 fixtures/ 入库后
/// CI 与 fresh clone 恒在，env/spike 为特殊环境覆盖）。
fn fixtures_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut tried: Vec<String> = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var(FIXTURES_ENV) {
        candidates.push(PathBuf::from(dir));
    }
    candidates.push(manifest.join("fixtures"));
    candidates.push(manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/fixtures"));
    for dir in &candidates {
        tried.push(dir.display().to_string());
        if dir.join("small.pdf").is_file() {
            return dir.clone();
        }
    }
    panic!(
        "环境合同：fixtures/small.pdf 缺失（不静默 skip）。已尝试：{}",
        tried.join("；")
    );
}

/// 库源定位（env → 候选清单除部署位外的取件位）。部署位（候选 0，
/// exe 同目录）**排除**——它是本测试的放置目标而非源；且排除后
/// 源与目标必不同路径（exe 在 target/，源在仓内）。
fn library_source(candidates: &[PathBuf]) -> PathBuf {
    let mut tried: Vec<String> = Vec::new();
    if let Ok(path) = std::env::var(PDFIUM_LIB_ENV) {
        let path = PathBuf::from(path);
        tried.push(format!("{PDFIUM_LIB_ENV}={}", path.display()));
        if path.is_file() {
            return path;
        }
    }
    // 候选 0 = exe 同目录部署位（放置目标，跳过）；1.. = 开发取件位。
    for candidate in candidates.iter().skip(1) {
        tried.push(candidate.display().to_string());
        if candidate.is_file() {
            return candidate.clone();
        }
    }
    panic!(
        "环境合同：pdfium 库源缺失（不静默 skip）。已尝试：{}",
        tried.join("；")
    );
}

/// #210 部署位验证（进程级首绑定）：库放到 exe 同目录 + 无 env →
/// 定位序必须选中候选 0 绑定成功并打开夹具。
#[test]
fn exe_dir_placement_is_selected_by_resolution_order() {
    let candidates = PdfiumEngine::library_candidates();

    // ---- 形状内联复核（与单测 exe_dir_is_first_candidate 同口径——
    // 进程级自证：本二进制的候选 0 = 本二进制 exe 同目录）----
    let exe_dir = std::env::current_exe()
        .expect("测试环境 current_exe 可得")
        .parent()
        .expect("exe 路径有父目录")
        .to_path_buf();
    assert!(
        !candidates.is_empty(),
        "候选清单非空（exe 位 + 三个取件位）"
    );
    assert_eq!(
        candidates[0],
        exe_dir.join(
            candidates[0]
                .file_name()
                .expect("候选 0 有文件名（平台库名）")
        ),
        "候选 0 = exe 同目录 + 平台库名"
    );

    // ---- 1) 库源 → 复制到部署位（候选 0）----
    let source = library_source(&candidates);
    let deploy_at = &candidates[0];
    std::fs::copy(&source, deploy_at).unwrap_or_else(|e| {
        panic!(
            "复制库到部署位失败：{} → {}（{e}）",
            source.display(),
            deploy_at.display()
        )
    });

    // ---- 2) 清 env（防 CI 以 MDC_PDFIUM_LIB 提供库时定位序短路----
    // 本测试进程内生效：定位序第一步 env 不存在，必须走候选清单）。
    // SAFETY（无）：edition 2021 下 remove_var 为安全 API；本文件仅
    // 一个测试，无并发 env 竞争。
    std::env::remove_var(PDFIUM_LIB_ENV);

    // ---- 3) 定位序 open：候选 0（部署位）已放库且最前 → 必选中 ----
    let fixture = fixtures_dir().join("small.pdf");
    let bytes = std::fs::read(&fixture)
        .unwrap_or_else(|e| panic!("读夹具失败 {}（{e}）", fixture.display()));
    let engine =
        PdfiumEngine::open(bytes).expect("定位序绑定 + 打开必须成功（部署位库已放置且为候选 0）");
    assert_eq!(
        engine.page_count(),
        SMALL_PDF_PAGES,
        "small.pdf 页数基线（pdfc_engine 锁 1 同源）"
    );
}
