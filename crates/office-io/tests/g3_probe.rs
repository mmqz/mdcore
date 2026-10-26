// G3 探针（issue #6）：写面导出 docx 的 docProps 元数据面现状实测。
// 显式运行：cargo test -p mdc-office-io --test g3_probe -- --ignored --nocapture
// 产出物落 /tmp/g3/g3_probe_out.docx（不入库）。
use std::io::{Read, Write};

#[test]
#[ignore = "探针：显式跑（issue #6 G3 现状登记件）"]
fn g3_docx_docprops_inventory() {
    let mut w = mdc_office_io::docx::DocxWriter::new();
    w.heading(1, "暗标元数据探针").unwrap();
    w.paragraph_text("G3 现状核查：本文件由 office-io 写面生成。");
    let bytes = w.save_to_vec().unwrap();
    std::fs::create_dir_all("/tmp/g3").unwrap();
    std::fs::File::create("/tmp/g3/g3_probe_out.docx")
        .unwrap()
        .write_all(&bytes)
        .unwrap();
    println!("导出 {} 字节 -> /tmp/g3/g3_probe_out.docx", bytes.len());

    let f = std::fs::File::open("/tmp/g3/g3_probe_out.docx").unwrap();
    let mut ar = zip::ZipArchive::new(f).unwrap();
    println!("=== zip 清单:");
    let mut names = vec![];
    for i in 0..ar.len() {
        let n = ar.by_index(i).unwrap().name().to_string();
        names.push(n.clone());
        println!("  {n}");
    }
    for name in [
        "docProps/core.xml",
        "docProps/app.xml",
        "docProps/custom.xml",
    ] {
        if let Ok(mut e) = ar.by_name(name) {
            let mut s = String::new();
            e.read_to_string(&mut s).unwrap();
            println!("=== {name}:\n{s}");
        } else {
            println!("=== {name}: （不存在）");
        }
    }
    assert!(!names.is_empty());
}
