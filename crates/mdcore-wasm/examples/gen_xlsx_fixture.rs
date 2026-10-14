//! web e2e 夹具生成器（t17）：样例 XLSX → `web/tests/fixtures/
//! import_sample.xlsx`（xlsx_import.py 与 mdcore-wasm 原生漂移守卫测试
//! 共用）。产线 = office-io `WorkbookWriter`（单一真源
//! `build_sample_xlsx`，与原生测试同源——零双份漂移；写→读全环 =
//! C2→C1 栈复证）。
//!
//! 用法（仓库根）：`cargo run -p mdcore-wasm --example gen_xlsx_fixture`
//!（可选显式路径参数；默认 web/tests/fixtures/import_sample.xlsx）。

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "web/tests/fixtures/import_sample.xlsx".to_string());
    let bytes = mdcore_wasm::xlsx_import::build_sample_xlsx();
    let path = std::path::Path::new(&out);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("create fixture dir {}: {e}", dir.display()));
    }
    std::fs::write(path, &bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    println!(
        "fixture written: {} ({} bytes)",
        path.display(),
        bytes.len()
    );
}
