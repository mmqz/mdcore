//! N-3 字数统计验收测试（issue #29）。
//!
//! 验收线：**40 万字级混合文档**（确定性生成器，夹具入库
//! `test-artifacts/word-count/fixture_400k.md`）上，引擎 `Core::word_count`
//! vs 独立 Python 参考实现（`test-artifacts/word-count/count_ref.py`
//! ——正则/显式码位路径，与 Rust 零共享代码）全七指标相对误差 < 0.1%。
//!
//! 夹具钉定：生成器输出与入库夹具逐字节相等（防生成器漂移导致验收
//! 文本换血）；刷新流程见 `test-artifacts/word-count/README.md`
//! （`MDCORE_N3_REGEN=1`）。

use std::path::PathBuf;
use std::process::Command;

use mdcore::op::Op;
use mdcore::{ApplyMeta, Core, WordCount};

// ---------------------------------------------------------------------------
// 确定性生成器（LCG——零依赖，无 rand）
// ---------------------------------------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

const HEADINGS: &[&str] = &[
    "总体技术方案",
    "项目实施组织",
    "质量保证体系",
    "运维服务承诺",
    "风险管理策略",
    "安全合规要求",
    "进度计划安排",
    "验收标准说明",
    "培训服务方案",
    "售后保障措施",
];

const SENTENCES: &[&str] = &[
    "本项目采用微服务架构与容器化部署方案，确保系统具备良好的横向扩展能力。",
    "投标人应具备类似项目的实施经验，并提供近三年内的有效业绩证明材料。",
    "系统核心接口平均响应时间不超过三百毫秒，峰值并发承载能力不低于五千用户。",
    "数据备份策略采用全量加增量相结合的方式，恢复点目标与恢复时间目标均满足要求。",
    "平台需支持国产化软硬件环境适配，包括主流操作系统与数据库的兼容性验证。",
    "项目团队将按照敏捷迭代模式组织交付，每两周输出一次可演示的阶段性成果。",
    "所有关键数据传输过程均采用加密通道保护，防止敏感信息在链路上被窃听篡改。",
    "运维值班制度实行全天候响应，重大故障两小时内到场处置并提交书面报告。",
    "文档交付物包括需求规格说明书、设计概要、测试报告与用户操作手册四类。",
    "培训计划分为管理员进阶课程与普通用户基础课程，累计不少于四十课时。",
    "接口规范遵循统一风格定义，报文使用结构化格式进行数据交换与一致性校验。",
    "日志采集链路集中汇聚到检索平台，支持按关键字与时间范围的组合查询。",
];

const LATIN_TOKENS: &[&str] = &[
    "API", "JSON", "HTTP", "K8s", "Redis", "MySQL", "Kafka", "Nginx", "Docker", "SSD", "QPS",
    "SLA", "v2.4", "99.9%", "OAuth2", "IPv6",
];

const CODE_LINES: &[&str] = &[
    "fn main() {",
    "let x = 42;",
    "let y = x * 2;",
    "println!(\"{}\", y);",
    "}",
    "SELECT count(*) FROM orders;",
    "curl -s http://api.local/health",
    "tar -czf backup.tar.gz /data",
    "chmod +x deploy.sh",
    "# 端口范围 8080-8090",
    "git commit -m \"release v3\"",
    "echo \"部署完成 校验通过\"",
];

/// 生成 ~40 万字级混合文档（标题/段落/行内标记/表格/代码块/列表/引用/
/// 主题线全形态）。确定性：同种子同输出（`n3_generator_deterministic` 钉定）。
fn gen_fixture() -> String {
    let mut rng = Lcg(0x00C0_FFEE_0D0B_ACE5);
    let mut out = String::with_capacity(2 * 1024 * 1024);
    out.push_str("# 某某信息系统建设项目投标文件（技术部分）\n\n");
    out.push_str("> 本文档为确定生成的四十万字级字数统计验收夹具。\n\n");
    for i in 0..940usize {
        let h = HEADINGS[i % HEADINGS.len()];
        let tok = LATIN_TOKENS[(i as u64 % LATIN_TOKENS.len() as u64) as usize];
        out.push_str(&format!("## 第{}章 {}（{}）\n\n", i + 1, h, tok));
        let paras = 3 + rng.below(2);
        for _ in 0..paras {
            let sents = 2 + rng.below(2);
            let mut para = String::new();
            for k in 0..sents {
                let s = SENTENCES[rng.below(SENTENCES.len() as u64) as usize];
                match rng.below(6) {
                    0 => para.push_str(&format!("**{s}**")),
                    1 => para.push_str(&format!("*{s}*")),
                    2 => para.push_str(&format!("~~{s}~~")),
                    3 => para.push_str(&format!("<u>{s}</u>")),
                    _ => para.push_str(s),
                }
                if k + 1 < sents {
                    para.push('，');
                }
                if rng.below(4) == 0 {
                    let t = LATIN_TOKENS[rng.below(LATIN_TOKENS.len() as u64) as usize];
                    para.push_str(&format!("（{t}）"));
                }
            }
            para.push('。');
            out.push_str(&para);
            out.push_str("\n\n");
        }
        if i % 3 == 1 {
            out.push_str("| 指标项 | 目标值 | 说明 |\n| --- | :---: | ---: |\n");
            let rows = 3 + rng.below(3);
            for r in 0..rows {
                out.push_str(&format!(
                    "| **指标{}** | {} {} | 备注说明第{}条 |\n",
                    r + 1,
                    100 + r * 37,
                    LATIN_TOKENS[rng.below(LATIN_TOKENS.len() as u64) as usize],
                    r + 1
                ));
            }
            if i % 9 == 4 {
                out.push_str("| 管道 \\| 转义 | 1 | 专项验证 |\n");
            }
            out.push('\n');
        }
        if i % 4 == 2 {
            out.push_str("```rust\n");
            let n = 3 + rng.below(4);
            for _ in 0..n {
                out.push_str(CODE_LINES[rng.below(CODE_LINES.len() as u64) as usize]);
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        let items = 3 + rng.below(3);
        for k in 0..items as usize {
            let s = SENTENCES[rng.below(SENTENCES.len() as u64) as usize];
            if (i + k) % 2 == 0 {
                out.push_str(&format!("- {s}\n"));
            } else {
                out.push_str(&format!("{}. {}\n", k + 1, s));
            }
        }
        out.push('\n');
        if i % 5 == 3 {
            let s = SENTENCES[rng.below(SENTENCES.len() as u64) as usize];
            out.push_str(&format!("> {s}\n\n"));
        }
        if i % 7 == 6 {
            out.push_str("---\n\n");
        }
    }
    out
}

fn artifact_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/word-count")
}

const METRICS: [&str; 7] = [
    "cjk_chars",
    "western_words",
    "total_words",
    "total_chars",
    "paragraphs",
    "headings",
    "tables",
];

#[test]
fn n3_generator_deterministic() {
    assert_eq!(gen_fixture(), gen_fixture());
}

#[test]
fn n3_acceptance_400k_vs_python_reference() {
    let md = gen_fixture();
    let fixture = artifact_dir().join("fixture_400k.md");
    // 刷新通道（维护者显式触发；见 test-artifacts/word-count/README.md）。
    if std::env::var_os("MDCORE_N3_REGEN").is_some() {
        std::fs::write(&fixture, &md)
            .expect("MDCORE_N3_REGEN=1：写 test-artifacts/word-count/fixture_400k.md");
    }
    let committed = std::fs::read_to_string(&fixture).unwrap_or_else(|e| {
        panic!("读夹具 {fixture:?} 失败：{e}（验收夹具入库为验收文本的钉定面）")
    });
    // 行尾归一化：夹具入库 LF、Windows 检出 CRLF（.gitattributes 未标记），
    // 生成器产出 LF——直接比字节在 Windows 上恒定漂移。比较【逻辑内容】。
    let committed = committed.replace("\r\n", "\n");
    assert_eq!(
        md, committed,
        "生成器与入库夹具漂移：如为有意变更，以 MDCORE_N3_REGEN=1 重跑本测试刷新夹具，\
         并同步重算两侧证据 JSON（见 test-artifacts/word-count/README.md）"
    );

    let wc = Core::from_markdown(&md).word_count();
    // 量级 sanity：这确实是四十万字级文档（评审口径 = total_words）。
    assert!(
        (380_000..=420_000).contains(&wc.total_words),
        "量级 sanity 失败：{wc:?}"
    );

    // Rust 侧输出落 JSON（证据面 = test-artifacts/word-count/wc_rust.json）。
    let rust_json = serde_json::to_string_pretty(&wc).expect("WordCount 序列化");
    let out_dir = std::env::temp_dir().join("mdcore-n3-word-count");
    std::fs::create_dir_all(&out_dir).expect("建临时证据目录");
    std::fs::write(out_dir.join("wc_rust.json"), &rust_json).expect("落 rust 侧 JSON");

    // Python 独立参考实现（count_ref.py——正则/码位路径，零共享代码）。
    let script = artifact_dir().join("count_ref.py");
    let py_out = out_dir.join("wc_python.json");
    let run = |bin: &str| {
        Command::new(bin)
            .arg(&script)
            .arg(&fixture)
            .arg("--out")
            .arg(&py_out)
            .output()
    };
    let output = match run("python3") {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            run("python").expect("python3 与 python 均不可用——验收依赖 Python 参考实现")
        }
        Err(e) => panic!("启动 python3 失败：{e}"),
    };
    assert!(
        output.status.success(),
        "count_ref.py 执行失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rust: serde_json::Value = serde_json::from_str(&rust_json).expect("rust JSON 解析");
    let py_text = std::fs::read_to_string(&py_out).expect("读 python 侧 JSON");
    let py: serde_json::Value = serde_json::from_str(&py_text).expect("python JSON 解析");
    for key in METRICS {
        let a = rust[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{key} 缺于 rust JSON"));
        let b = py[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{key} 缺于 python JSON"));
        let denom = b.max(1) as f64;
        let rel = (a as f64 - b as f64).abs() / denom;
        assert!(
            rel < 0.001,
            "指标 {key}：rust={a} python={b} 相对误差 {rel:.8} ≥ 0.1%（口径漂移）"
        );
    }
}

// ---------------------------------------------------------------------------
// op 面活文档（非 markdown 解析路径）——计数跟随编辑前进
// ---------------------------------------------------------------------------

#[test]
fn n3_word_count_tracks_live_edits() {
    let mut core = Core::from_markdown("初始段落");
    let w0 = core.word_count();
    assert_eq!(w0.cjk_chars, 4);
    assert_eq!(w0.paragraphs, 1);

    // 追加一个带标记源文本的段落（ReplaceText 在既有段落尾续写）。
    let ops = vec![Op::ReplaceText {
        path: vec![0],
        start: 4,
        end: 4,
        insert: "，新增 **加粗** 与 hello".into(),
    }];
    core.apply_ops(ops, ApplyMeta::view("test-edit"))
        .expect("apply");
    let w1 = core.word_count();
    // 净文本 = "初始段落，新增 加粗 与 hello"：9 汉字（含“与”）+ hello。
    assert_eq!(w1.cjk_chars, 9); // 初始段落(4)+新增(2)+加粗(2)+与(1)
    assert_eq!(w1.western_words, 1); // hello
    assert_eq!(w1.total_chars, 15); // 9 汉字 + 1 逗号 + 5 hello

    // 结构性新增：标题块 + 表格块（op 面 InsertBlock）。
    let ops = vec![
        Op::InsertBlock {
            parent: vec![],
            index: 0,
            kind: mdcore::tree::BlockKind::Heading,
            attrs: [("level".to_string(), "2".to_string())]
                .into_iter()
                .collect(),
            text: "二级标题甲".into(),
            id: None,
        },
        Op::InsertBlock {
            parent: vec![],
            index: 2,
            kind: mdcore::tree::BlockKind::Table,
            attrs: Default::default(),
            text: "| 甲 | 乙 |\n| --- | --- |\n| 丙 | 丁 |".into(),
            id: None,
        },
    ];
    core.apply_ops(ops, ApplyMeta::view("test-insert"))
        .expect("apply");
    let w2 = core.word_count();
    assert_eq!(w2.headings, 1);
    assert_eq!(w2.tables, 1);
    assert_eq!(w2.cjk_chars, 9 + 5 + 4); // 段落 9 + 标题"二级标题甲" 5 + 表格甲乙丙丁 4
    assert_eq!(w2.paragraphs, 1);
    assert_eq!(w2.total_words, w2.cjk_chars + w2.western_words);

    // 撤销结构性新增 → 计数回退（undo 路径一致性）。
    core.undo().expect("undo");
    let w3 = core.word_count();
    assert_eq!(w3, w1);
}

// 静态断言：WordCount Copy + PartialEq（API 面钉定——宿主免 clone 焦虑，
// 相等比较是验收与测试的基本面）。
#[allow(dead_code)]
fn _word_count_api_surface_assertions(w: WordCount) {
    let _copy = w;
    let _eq = w == _copy;
}
