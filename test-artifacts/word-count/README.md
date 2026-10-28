# N-3 中文字数统计——验收夹具与独立参考实现（issue #29）

`Core::word_count()`（`crates/mdcore/src/word_count.rs`）的验收三件套：

| 文件 | 角色 |
|---|---|
| `count_ref.py` | 独立 Python 参考实现（正则字符类 + 显式码位区间 + 逐字符扫描——与 Rust 零共享代码） |
| `fixture_400k.md` | 40 万字级混合文档夹具（确定性生成器产出，`crates/mdcore/tests/n3_word_count.rs` 内嵌；测试断言生成器输出与入库夹具**逐字节相等**——防验收文本换血） |
| `wc_rust.json` / `wc_python.json` | 验收当轮两侧输出（证据快照；测试运行时写到 `/tmp/mdcore-n3-word-count/`） |

## 口径（两侧实现同一份口径表，真源 = Rust 模块文档）

- **汉字数** = CJK 统一表意 URO `U+4E00..=U+9FFF` + 扩展 A `U+3400..=U+4DBF`（恰两段；CJK 标点/全角形式/扩展 B+ 不计）
- **西文词数** = 连续 `[A-Za-z0-9]+` 序列（下划线/撇号/连字符为分隔符）
- **总字数** = 汉字数 + 西文词数（**标书评审口径**）
- **总字符数** = 全部非空白字符
- **段落数/标题数/表格数** = 块树结构分计（列表项/代码块不计段落；引用内段落计段落）
- 代码块按字面计入三项字符/词统计；表格按单元格文本计入（管道符/分隔行不计）；行内标记（`**`/`*`/`` ` ``/`~~`/`<u>` 成对形）剥除定界符、未闭合按字面量

## 验收结果（本轮实测，2026-10-28）

| 指标 | Rust（引擎） | Python（count_ref.py） | 相对误差 |
|---|---|---|---|
| cjk_chars | 385,890 | 385,890 | 0 |
| western_words | 14,412 | 14,412 | 0 |
| total_words | **400,302** | **400,302** | 0 |
| total_chars | 472,763 | 472,763 | 0 |
| paragraphs | 3,406 | 3,406 | 0 |
| headings | 941 | 941 | 0 |
| tables | 313 | 313 | 0 |

全七指标**零差异**（验收线 < 0.1%）。夹具形态覆盖：941 标题 / 3,406 段落 /
313 表格（含 `\|` 转义与 `:---:` 对齐分隔行）/ 代码块（含 CJK 注释与
shell/rust/SQL 行）/ 无序+有序列表 / 引用 / 主题线 / 五类行内标记
（strong/em/del/code/u）随机混排。

## 复现

```sh
# 引擎侧验收（含夹具钉定 + python 比对断言；需 python3 或 python）
cargo test -p mdcore --test n3_word_count

# 单独跑 Python 参考实现（任意 markdown）
python3 test-artifacts/word-count/count_ref.py test-artifacts/word-count/fixture_400k.md
```

## 夹具刷新流程（生成器有意变更时）

```sh
MDCORE_N3_REGEN=1 cargo test -p mdcore --test n3_word_count   # 重写 fixture_400k.md
cp /tmp/mdcore-n3-word-count/wc_rust.json /tmp/mdcore-n3-word-count/wc_python.json \
   test-artifacts/word-count/                                 # 同步证据快照
```

注意：刷新后必须核对两侧数字仍满足 < 0.1%（测试会重跑比对），并在本 README
更新验收结果表。
