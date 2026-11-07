# THIRD-PARTY-NOTICES（草案 v1.6 —— collab-r1：协作引擎 yrs 闭包入账 + S1/S2 期间面漂移补账 + SBOM 四面复跑）

- 生成沿革：v1 2026-10-10 scripts/r2_gen_notices.py（可复跑）；v1.1 2026-10-10 R24-a 人工增补
  （toml_edit 族 8 项，逐项经 crates.io 版本级 API 按锁定版本取证）；v1.2 2026-10-11 PDF-e
  收口轮人工增补 + 完整 SBOM 复跑（兑现 PDF-c/d 登记的「PDF-e SBOM 收口」义务与
  R24-e 登记的完整 SBOM 复跑义务）；**v1.3 2026-10-13 xlsx-r1 阶段 B 人工增补 +
  四面复跑**（Office 线首批三方依赖入账：mdc-office-io 读引擎闭包；双重 crate
  方案 A——mdc-office 保持纯 std 零三方依赖，IO 依赖承载于新 crate）。**v1.4
  2026-10-14 xlsx-r2 Phase 1 人工增补 +
  四面复跑**（C2 写面产品化：
  rust_xlsxwriter 由 dev 面（分类账）升产品 normal 面）。**v1.5
  2026-10-14 xlsx-r2 Phase 3 人工增补 + 四面复跑**（DOCX 模型级
  切片：docx-rs 接入，Office 线第二批三方依赖）。**v1.6
  2026-10-21 collab-r1 人工增补 + 四面复跑 + 补账**（协作引擎
  mdc-collab 落地：yrs 0.28 闭包入账——协作与 Canvas 转正式交付
  目标的产品面首批协同依赖；同时补账 v1.5 后未随轮复跑的面漂移）。
- **v1.6 增量明细（透明登记）**：Cargo.lock 146→161 条目（+1 自研
  mdc-collab + 14 外部 crate）。14 外部 = 12 入全四面（yrs 0.28.0
  [MIT，协作引擎本体——D1 集成验证边界四项边界与撤回触发在档] +
  arc-swap 1.9.3 / async-lock 3.4.2 / async-trait 0.1.92 /
  crossbeam-utils 0.8.23 / dashmap 6.2.1 / event-listener 5.4.2 /
  event-listener-strategy 0.5.4 / lock_api 0.4.14 / parking 2.2.1 /
  parking_lot_core 0.9.12 / smallstr 0.3.1 [全 MIT OR Apache-2.0，
  registry manifest 直读按 MIT 履行]）+ hashbrown 0.14.5（dashmap
  链；与既有 0.17.1[calamine 链]版本并存——同名双版本，许可同族）+
  redox_syscall 0.5.18（redox 目标面，host linux 不编译——分类账）。
  四面复跑（main @ b3ec4da）：default 75 / +engine-pdfium 88 /
  +engine-lopdf 108 / dual 121（较 v1.5 文档值 62/75/95/108 净增
  +13/+13/+13/+13 = collab 12 + 复跑口径漂移；见下）。
- **v1.5 后面漂移补账（合规缺口诚实登记）**：v1.5 基准（main @
  460f302）后，t19-t22/r4/r5/S1/S2 期间 NOTICES 未随轮复跑。本轮
  复跑定谳两部分漂移：①serde/serde_json/itoa/ryu/serde_derive/
  proc-macro2/quote/syn/unicode-ident 族经 mdc-evidence/
  mdc-workbench（S1/S2 新 crate；lock 零新条目但 serde 族从 lopdf
  feature 面进入 default 面）；②futures-core/futures-task/
  futures-util/js-sys/pin-project-lite/slab 六项从分类账
  （wasm32 目标面）移入正文——rust_xlsxwriter 特性面经 t19
  constant_memory target-split 变化后进 host normal 面。全部
  MIT/Apache 双许可，零 copyleft 零 UNKNOWN；460f302 基线重建
  复测 63 项（v1.5 文档 62 ±1 计数方差）。本轮一并入账，此后纪律：
  凡 lock 变化或 feature 面变化的轮次，NOTICES 增量随批。
- **v1.5 增量明细（透明登记）**：Cargo.lock 142→144 条目（+2 外部
  crate：docx-rs 0.4.22[ADR-O1 O1-3 选型，bokuweb/docx-rs，MIT——
  分析仓 office-e 浅克隆 LICENSE 一手直读；default-features=false
  关 image 特性巨树] + base64 0.22.1[docx-rs 必依赖，MIT OR
  Apache-2.0 按 MIT 履行]）。正文 cargo 面 106→108（新入 2）；分类账
  28 恒（零新非编译项）；四面 58/71/93/106 → 62/75/95/108（default
  +4 = 新入 2 + thiserror/thiserror-impl[原仅 lopdf feature 面]随
  docx 链入 default 面；pdfium +4 同理；lopdf/dual +2 = 新入 2）。
  三链核验：lock cksum == .crate SHA256 2/2（index cksum 经 cargo
  下载验证传递）；许可两源直读在档。供应链观察项（office-e §4：
  上游已合入下游 fork 功能线）——锁版本 + 往返测试
  tests/docx_roundtrip.rs 首跑拦截纪律常驻化。
- **v1.4 增量明细（透明登记）**：Cargo.lock 零 diff（rust_xlsxwriter 0.99.1
  原在 lock[dev 面]，升产品依赖同版本——解析不变）。**纯面重分类**：
  正文 cargo 面 101→106（账→正文提升 5：rust_xlsxwriter[tempfile 为其
  constant_memory 特性的可选依赖，随特性激活进 normal 面] + tempfile/
  fastrand/rustix/linux-raw-sys[tempfile 链]）；分类账 33→28。四面：
  50/64/88/101 → 58/71/93/106（default +8 = 提升 5 + bitflags/getrandom/
  libc 经 pdfium/lopdf 面原已在正文[default 面新入]；dual +5）。零版本
  漂移；零正文项跌出编译面；零新 lock 条目。
- **v1.3 增量明细（透明登记）**：Cargo.lock 123→142 条目（+1 自研 mdc-office-io
  [注：v1.2 基准锁 3e450aa 尚无 mdc-settings/mdc-office，两者于 T3 复用轮与
  office-impl T1 轮入 lock，非本轮增量；v1.3 自研单列同步补齐至八 crate] +
  16 外部 crate）。四面复跑结果：正文 cargo 面 90→101（新入 9：calamine/zip/
  quick-xml/codepage/fast-float2/atoi_simd/debug_unsafe/typed-path/zopfli
  [office-io 直依赖 calamine+zip 及其闭包]；分类账提升 2：rustversion
  [atoi_simd 的 proc-macro 硬边，v1.2 时仅为 encoding_rs 声明态]、
  zlib-rs [zip→flate2 启用 zlib-rs 后端边，v1.2 时 lopdf 面仅 rust_backend]）；
  分类账 28→33（新增 7：rust_xlsxwriter [本仓 dev 面：office-io 测试语料
  构造件，-e normal 不入正文不随产物分发] + tempfile/fastrand/rustix/
  linux-raw-sys/errno [rust_xlsxwriter 的 dev 链，任何本仓 face 不编译] +
  windows-sys [windows 目标面，rustix 依赖]）。零版本漂移；零正文项跌出
  编译面。
- **取证方法（v1.3 增补）**：本会话 crates.io API 网络面恢复可达（v1.2 时
  403 的环境限制已过时）——16 新 crate 全部双源核验：① 三链（lock checksum ==
  sparse 索引 cksum == sha256(.crate 工件缓存)，15/16 全过；windows-sys 为
  win 目标件无 .crate 缓存，链①==② 过 + 许可经 crates.io API 直验）+ 许可
  字段 registry manifest 直读。**两处许可修正**（以锁定版本工件为准，与
  office-e/DEPS 首版记录的出入已在分析仓回写）：zip 8.6.0 = MIT 单许可
  （manifest 与 LICENSE 文件均单 MIT；首版记 MIT OR Apache-2.0 系上游仓
  历史版本口径混淆）；rust_xlsxwriter 0.99.1 = MIT OR Apache-2.0 双许可
  （首版记 MIT；按 MIT 履行不变，ADR-C1 §1 允许域）。
- **取证方法（v1.2，历史登记）**：v1.2 时环境 crates.io API 不可达（HTTP 403），
  改用与 cargo 解析同源的本地数据三链核验：① Cargo.lock `checksum` ==
  ② 本地 sparse 注册表索引（`~/.cargo/registry/index/…/.cache`，即 crates.io 注册表
  索引数据）该版本记录 `cksum`（118/118 第三方条目逐位一致），并 == ③
  `sha256(注册表 .crate 工件缓存)`（90/90 正文项逐位一致）；许可字段取自注册表 src
  内该 crate 的 Cargo.toml manifest（crates.io 分发工件本体）。
- 范围：产品仓 main @ 460f302 的 Cargo.lock（144 条目 = 8 自研 +
  136 第三方）四面闭包
  + web 随产物分发 npm 包（含传递闭包）+ pdfium 预编译二进制（首个分发资产，三平台）；
  devDependencies（构建期工具）不入正文，见 §dev；引擎二进制 licenses/ 全量见 §pdfium。
- 工作区自研件（非第三方，单列）：mdcore 0.1.0, mdcore-wasm 0.1.0, mdshell 0.1.0,
  mdshell-native 0.1.0, mdc-pdf 0.1.0, mdc-settings 0.1.0, mdc-office 0.1.0,
  mdc-office-io 0.1.0, mdc-evidence 0.1.0, mdc-workbench 0.1.0, mdc-collab 0.1.0
  （v1.2 基准锁后新入 lock 的自研件一并补齐单列；evidence/workbench [S1/S2]、
  collab [v1.6 本轮]）。
- 用途：①对产品仓 mdcore/ compliance/SBOM 做独立交叉核验；②可整体移交产品仓作为
  发行态 NOTICES 底稿。
- 许可证字段取自注册表 manifest（crates 按锁定版本、npm 按注册表元数据），发行前以
  **发行时点**元数据复核为准（许可证可能随版本变化）。

## 汇总

- 第三方组件总数：**136**（cargo 121 + npm ship 12 + pdfium 预编译二进制 3）
- cargo 四面规模：default 75 ／ +engine-pdfium 88 ／ +engine-lopdf 108 ／ dual 121
  （dual = 四面并集 = 正文 cargo 面。v1.5 文档 62/75/95/108 → 本轮 75/88/108/121：
  collab-r1 新入 12（yrs 闭包）+ v1.5 后未随轮复跑的面漂移补账 7（serde 族经
  evidence/workbench + futures/js-sys 族经 rust_xlsxwriter 特性面——见 v1.6
  增量明细的补账段）；
  Office 线第二批依赖入账——docx-rs 0.4.22（DOCX 模型级切片引擎，
  xlsx-r2 Phase 3）+ base64 0.22.1（docx-rs 必依赖）新入 2；default
  面 +4 = 新入 2 + thiserror/thiserror-impl（原仅 lopdf feature 面）
  随 docx 链入 default 面；pdfium 面 +4 同理；lopdf/dual 面 +2 =
  新入 2）
- 许可证族（133 = 121 cargo + 12 npm；v1.6 族计数以正文表行为准——表行 = 单一真源，族汇总为导航索引）：
  (MIT OR Apache-2.0) AND Unicode-3.0×1, (Apache-2.0 OR MIT) AND BSD-3-Clause×1,
  0BSD OR MIT OR Apache-2.0×1, Apache-2.0×1（zopfli，v1.3 新入）, Apache-2.0 WITH
  LLVM-exception OR Apache-2.0 OR MIT×2（rustix/linux-raw-sys，v1.4 账提升；
  LLVM-exception 仅为三选一 OR 选项之一，按 Apache-2.0 或 MIT 履行）,
  BSD-3-Clause×2,
  BSD-3-Clause/MIT×1, ISC×1, MIT×28（cargo 16 + npm 12；v1.5 +1：docx-rs；v1.6 +3：yrs/dashmap/slab[账→正文]）,
  MIT OR Apache-2.0×83（v1.6 +14：yrs 闭包 10 项 + 账→正文 futures-core/futures-task/futures-util/js-sys；hashbrown 0.14.5 与 0.17.1 同名双版本同族）
  （v1.4 +2：rust_xlsxwriter/tempfile；v1.5 +1：base64；另含同义写法
  Apache-2.0 OR MIT×6 与
  MIT/Apache-2.0×4——同义写法含 fastrand）, Unlicense OR MIT×2,
  Zlib×1（zlib-rs，v1.3 分类账提升）, Zlib OR Apache-2.0 OR MIT×2（另含同义写法
  MIT OR Zlib OR Apache-2.0×1）
- UNKNOWN：0（121/121 正文项 + 29/29 分类账项许可全部解析，须人工复核后方可发行的门槛未触发）
- GPL/AGPL：**零命中**（136 项 lock 条目全量核验；唯一非白名单字样 r-efi 的
  LGPL-2.1-or-later 为三选一 OR 选项之一且不进任何分发面，详见分类账注记）

## (MIT OR Apache-2.0) AND Unicode-3.0

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | unicode-ident | 1.0.26 | https://crates.io/crates/unicode-ident |

## (Apache-2.0 OR MIT) AND BSD-3-Clause

（AND 复合：MIT/Apache 分支任选其一 + 须同时满足 BSD-3-Clause 通知义务）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | encoding_rs | 0.8.42 | https://crates.io/crates/encoding_rs |

## 0BSD OR MIT OR Apache-2.0

（三选一：按 MIT 履行——ADR-C1 §1 允许域选择登记）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | adler2 | 2.0.1 | https://crates.io/crates/adler2 |

## Apache-2.0

（单许可：保留声明副本 + NOTICE 文件变更说明义务[如适用]。v1.3 新族——
zopfli 为 zip 8.6 的 DEFLATE 重压缩实现件）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | zopfli | 0.8.4 | https://crates.io/crates/zopfli |

## Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT

（三选一 OR 多许可：按 Apache-2.0 或 MIT 履行即可——LLVM-exception 仅为
OR 选项之一，不构成额外义务；v1.4 新族——两项均由分类账提升入正文
[rust_xlsxwriter constant_memory 特性激活 tempfile 链]）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | linux-raw-sys | 0.12.1 | https://crates.io/crates/linux-raw-sys |
| cargo | rustix | 1.1.5 | https://crates.io/crates/rustix |

## BSD-3-Clause

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | alloc-no-stdlib | 2.0.4 | https://crates.io/crates/alloc-no-stdlib |
| cargo | alloc-stdlib | 0.2.4 | https://crates.io/crates/alloc-stdlib |

## BSD-3-Clause/MIT

（双许可：按 MIT 履行即可）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | brotli-decompressor | 5.0.3 | https://crates.io/crates/brotli-decompressor |

## ISC

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | libloading | 0.9.0 | https://crates.io/crates/libloading |

## MIT

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | bytes | 1.12.1 | https://crates.io/crates/bytes |
| cargo | calamine | 0.36.1 | https://crates.io/crates/calamine |
| cargo | dashmap | 6.2.1 | https://crates.io/crates/dashmap |
| cargo | docx-rs | 0.4.22 | https://crates.io/crates/docx-rs |
| cargo | lopdf | 0.45.0 | https://crates.io/crates/lopdf |
| cargo | nom | 8.0.0 | https://crates.io/crates/nom |
| cargo | piston-float | 1.0.1 | https://crates.io/crates/piston-float |
| cargo | quick-xml | 0.41.0 | https://crates.io/crates/quick-xml |
| cargo | ropey | 1.6.1 | https://crates.io/crates/ropey |
| cargo | slab | 0.4.12 | https://crates.io/crates/slab |
| cargo | simd-adler32 | 0.3.10 | https://crates.io/crates/simd-adler32 |
| cargo | vecmath | 1.0.0 | https://crates.io/crates/vecmath |
| cargo | winnow | 1.0.4 | https://crates.io/crates/winnow |
| cargo | yrs | 0.28.0 | https://crates.io/crates/yrs |
| cargo | zip | 8.6.0 | https://crates.io/crates/zip |
| cargo | zmij | 1.0.23 | https://crates.io/crates/zmij |
| npm | @codemirror/commands | 6.11.1 | https://www.npmjs.com/package/@codemirror/commands |
| npm | @codemirror/language | 6.13.1 | https://www.npmjs.com/package/@codemirror/language |
| npm | @codemirror/state | 6.7.6 | https://www.npmjs.com/package/@codemirror/state |
| npm | @codemirror/streamparser | 6.0.0 | https://www.npmjs.com/package/@codemirror/streamparser |
| npm | @codemirror/view | 6.43.14 | https://www.npmjs.com/package/@codemirror/view |
| npm | @lezer/common | 1.5.3 | https://www.npmjs.com/package/@lezer/common |
| npm | @lezer/highlight | 1.2.5 | https://www.npmjs.com/package/@lezer/highlight |
| npm | @lezer/lr | 1.4.11 | https://www.npmjs.com/package/@lezer/lr |
| npm | @marijn/find-cluster-break | 1.0.4 | https://code.haverbeke.berlin/marijn/find-cluster-break |
| npm | crelt | 1.0.7 | https://code.haverbeke.berlin/marijn/crelt |
| npm | perfect-freehand | 1.2.3 | https://www.npmjs.com/package/perfect-freehand |
| npm | style-mod | 4.1.4 | https://code.haverbeke.berlin/marijn/style-mod |
| npm | w3c-keyname | 2.2.8 | https://github.com/marijnh/w3c-keyname#readme |

## MIT OR Apache-2.0

（双许可：按 MIT 履行即可——本选择登记于 ADR-C1 §1 允许域。
`Apache-2.0 OR MIT`、`MIT/Apache-2.0` 为 registry manifest 同义写法，同族分表列于后）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | aes | 0.9.3 | https://crates.io/crates/aes |
| cargo | arc-swap | 1.9.3 | https://crates.io/crates/arc-swap |
| cargo | async-lock | 3.4.2 | https://crates.io/crates/async-lock |
| cargo | async-trait | 0.1.92 | https://crates.io/crates/async-trait |
| cargo | atoi_simd | 0.18.1 | https://crates.io/crates/atoi_simd |
| cargo | base64 | 0.22.1 | https://crates.io/crates/base64 |
| cargo | bitflags | 2.13.2 | https://crates.io/crates/bitflags |
| cargo | block-buffer | 0.12.1 | https://crates.io/crates/block-buffer |
| cargo | block-padding | 0.4.2 | https://crates.io/crates/block-padding |
| cargo | bumpalo | 3.20.3 | https://crates.io/crates/bumpalo |
| cargo | cbc | 0.2.1 | https://crates.io/crates/cbc |
| cargo | cfg-if | 1.0.5 | https://crates.io/crates/cfg-if |
| cargo | chacha20 | 0.10.2 | https://crates.io/crates/chacha20 |
| cargo | chrono | 0.4.45 | https://crates.io/crates/chrono |
| cargo | cipher | 0.5.2 | https://crates.io/crates/cipher |
| cargo | cpubits | 0.1.1 | https://crates.io/crates/cpubits |
| cargo | cpufeatures | 0.3.1 | https://crates.io/crates/cpufeatures |
| cargo | crc32fast | 1.5.2 | https://crates.io/crates/crc32fast |
| cargo | crossbeam-utils | 0.8.23 | https://crates.io/crates/crossbeam-utils |
| cargo | crypto-common | 0.2.2 | https://crates.io/crates/crypto-common |
| cargo | debug_unsafe | 0.1.4 | https://crates.io/crates/debug_unsafe |
| cargo | digest | 0.11.3 | https://crates.io/crates/digest |
| cargo | ecb | 0.2.1 | https://crates.io/crates/ecb |
| cargo | either | 1.19.0 | https://crates.io/crates/either |
| cargo | event-listener | 5.4.2 | https://crates.io/crates/event-listener |
| cargo | event-listener-strategy | 0.5.4 | https://crates.io/crates/event-listener-strategy |
| cargo | fast-float2 | 0.2.4 | https://crates.io/crates/fast-float2 |
| cargo | futures-core | 0.3.34 | https://crates.io/crates/futures-core |
| cargo | futures-task | 0.3.34 | https://crates.io/crates/futures-task |
| cargo | futures-util | 0.3.34 | https://crates.io/crates/futures-util |
| cargo | flate2 | 1.1.10 | https://crates.io/crates/flate2 |
| cargo | getrandom | 0.4.3 | https://crates.io/crates/getrandom |
| cargo | hashbrown | 0.14.5 | https://crates.io/crates/hashbrown |
| cargo | hashbrown | 0.17.1 | https://crates.io/crates/hashbrown |
| cargo | hybrid-array | 0.4.15 | https://crates.io/crates/hybrid-array |
| cargo | iana-time-zone | 0.1.65 | https://crates.io/crates/iana-time-zone |
| cargo | inout | 0.2.2 | https://crates.io/crates/inout |
| cargo | itertools | 0.15.0 | https://crates.io/crates/itertools |
| cargo | js-sys | 0.3.106 | https://crates.io/crates/js-sys |
| cargo | itoa | 1.0.18 | https://crates.io/crates/itoa |
| cargo | libc | 0.2.190 | https://crates.io/crates/libc |
| cargo | lock_api | 0.4.14 | https://crates.io/crates/lock_api |
| cargo | log | 0.4.34 | https://crates.io/crates/log |
| cargo | maybe-owned | 0.3.4 | https://crates.io/crates/maybe-owned |
| cargo | md-5 | 0.11.0 | https://crates.io/crates/md-5 |
| cargo | num-traits | 0.2.19 | https://crates.io/crates/num-traits |
| cargo | once_cell | 1.21.4 | https://crates.io/crates/once_cell |
| cargo | parking | 2.2.1 | https://crates.io/crates/parking |
| cargo | parking_lot_core | 0.9.12 | https://crates.io/crates/parking_lot_core |
| cargo | pdfium-render | 0.9.4 | https://crates.io/crates/pdfium-render |
| cargo | proc-macro2 | 1.0.107 | https://crates.io/crates/proc-macro2 |
| cargo | quote | 1.0.47 | https://crates.io/crates/quote |
| cargo | rand | 0.10.3 | https://crates.io/crates/rand |
| cargo | rand_core | 0.10.1 | https://crates.io/crates/rand_core |
| cargo | rustversion | 1.0.23 | https://crates.io/crates/rustversion |
| cargo | rust_xlsxwriter | 0.99.1 | https://crates.io/crates/rust_xlsxwriter |
| cargo | scopeguard | 1.2.0 | https://crates.io/crates/scopeguard |
| cargo | serde | 1.0.229 | https://crates.io/crates/serde |
| cargo | serde_core | 1.0.229 | https://crates.io/crates/serde_core |
| cargo | serde_derive | 1.0.229 | https://crates.io/crates/serde_derive |
| cargo | serde_json | 1.0.151 | https://crates.io/crates/serde_json |
| cargo | smallstr | 0.3.1 | https://crates.io/crates/smallstr |
| cargo | sha2 | 0.11.0 | https://crates.io/crates/sha2 |
| cargo | simdutf8 | 0.1.5 | https://crates.io/crates/simdutf8 |
| cargo | smallvec | 1.16.2 | https://crates.io/crates/smallvec |
| cargo | str_indices | 0.4.4 | https://crates.io/crates/str_indices |
| cargo | syn | 3.0.6 | https://crates.io/crates/syn |
| cargo | thiserror | 2.0.21 | https://crates.io/crates/thiserror |
| cargo | thiserror-impl | 2.0.21 | https://crates.io/crates/thiserror-impl |
| cargo | toml_datetime | 1.1.2+spec-1.1.0 | https://crates.io/crates/toml_datetime |
| cargo | toml_edit | 0.25.17+spec-1.1.0 | https://crates.io/crates/toml_edit |
| cargo | toml_parser | 1.1.5+spec-1.1.0 | https://crates.io/crates/toml_parser |
| cargo | toml_writer | 1.1.3+spec-1.1.0 | https://crates.io/crates/toml_writer |
| cargo | tempfile | 3.27.0 | https://crates.io/crates/tempfile |
| cargo | typenum | 1.20.1 | https://crates.io/crates/typenum |
| cargo | typed-path | 0.12.3 | https://crates.io/crates/typed-path |
| cargo | unicode-bidi | 0.3.18 | https://crates.io/crates/unicode-bidi |
| cargo | unicode-normalization | 0.1.25 | https://crates.io/crates/unicode-normalization |
| cargo | utf16string | 0.2.0 | https://crates.io/crates/utf16string |
| cargo | wasm-bindgen | 0.2.129 | https://crates.io/crates/wasm-bindgen |
| cargo | wasm-bindgen-macro | 0.2.129 | https://crates.io/crates/wasm-bindgen-macro |
| cargo | wasm-bindgen-macro-support | 0.2.129 | https://crates.io/crates/wasm-bindgen-macro-support |
| cargo | wasm-bindgen-shared | 0.2.129 | https://crates.io/crates/wasm-bindgen-shared |
| cargo | weezl | 0.2.1 | https://crates.io/crates/weezl |

## Apache-2.0 OR MIT

（registry manifest 同义写法；双许可按 MIT 履行即可）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | codepage | 0.1.3 | https://crates.io/crates/codepage |
| cargo | const-oid | 0.10.2 | https://crates.io/crates/const-oid |
| cargo | equivalent | 1.0.2 | https://crates.io/crates/equivalent |
| cargo | fastrand | 2.5.0 | https://crates.io/crates/fastrand |
| cargo | indexmap | 2.14.2 | https://crates.io/crates/indexmap |
| cargo | multiversion_no_op | 1.0.0 | https://crates.io/crates/multiversion_no_op |

## MIT/Apache-2.0

（registry manifest 同义写法；双许可按 MIT 履行即可）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | core_detect | 1.0.0 | https://crates.io/crates/core_detect |
| cargo | rangemap | 1.8.0 | https://crates.io/crates/rangemap |
| cargo | stringprep | 0.1.5 | https://crates.io/crates/stringprep |
| cargo | unicode-properties | 0.1.4 | https://crates.io/crates/unicode-properties |

## Unlicense OR MIT

（双许可：按 MIT 履行即可）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | byteorder | 1.5.0 | https://crates.io/crates/byteorder |
| cargo | memchr | 2.8.3 | https://crates.io/crates/memchr |

## Zlib

（单许可：保留声明副本。v1.3 新族——zlib-rs 由 v1.2 分类账提升入正文：
zip 8.6 依赖链启用 flate2 的 zlib-rs 后端[zip→flate2 边]，v1.2 时 lopdf
面的 flate2 仅 rust_backend[miniz_oxide]）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | zlib-rs | 0.6.8 | https://crates.io/crates/zlib-rs |

## Zlib OR Apache-2.0 OR MIT

（三选一：按 MIT 履行——ADR-C1 §1 允许域选择登记）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | bytemuck | 1.25.2 | https://crates.io/crates/bytemuck |
| cargo | tinyvec | 1.13.3 | https://crates.io/crates/tinyvec |

## MIT OR Zlib OR Apache-2.0

（三选一：按 MIT 履行；与上表同族，manifest 写法异序单列）

| 生态 | 组件 | 版本 | 主页 |
|---|---|---|---|
| cargo | miniz_oxide | 0.9.1 | https://crates.io/crates/miniz_oxide |

## pdfium 预编译二进制（首个分发资产，三平台条目）

来源：bblanchon/pdfium-binaries（Chromium PDFium 官方树的社区构建轨，ADR-P1 选型定谳）。
版本 pin：**chromium/8086**（PDFium **157.0.8086.0**，VERSION 文件实测）；non-V8 构建
（args.gn 实证：`pdf_enable_v8=false`、`pdf_enable_xfa=false`、
`pdf_use_partition_alloc=false`、`is_debug=false`、`pdf_is_standalone=true`——
安全策略 §JS 面：V8 变体禁取）。取件脚本：分析仓 `md-line/pdf-a/spike/fetch-pdfium.sh`
（版本 pin + SHA256 验证闭环；升级 = 改 pin + 重跑脚本 + 复验三探针 + 本文件更新）。

| 生态 | 组件 | 平台/制品 | 版本 pin | SHA256 | 状态 |
|---|---|---|---|---|---|
| pdfium-bin | libpdfium（Chromium PDFium 非官方社区构建） | linux-x64 / libpdfium.so（7,942,208 B，tgz 内含 LICENSE、licenses/、include/、args.gn、VERSION） | chromium/8086 | tgz：`588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935`（本轮复验实测一致）；so：`e995a89f11d48605a79a0e44f101ca3a621e5c3ef9ee311a1405e03b44dc5b19` | **实取件**（2026-10-05 取件 / 2026-10-11 复验，spike bin/ 位） |
| pdfium-bin | libpdfium（同上） | macos-x64 | chromium/8086 | 待取件回填 | **占位：分发时取件声明**（同源 release 同名规则 pdfium-macos-x64.tgz；取件 = 扩展 fetch-pdfium.sh 的 ARTIFACT + 回填 SHA256 + licenses 复核） |
| pdfium-bin | libpdfium（同上） | win-x64 | chromium/8086 | 待取件回填 | **占位：分发时取件声明**（同上，pdfium-win-x64.tgz） |

**licenses/ 全量声明（linux-x64 实取件内 17 项，随分发携带义务）：**
pdfium.txt（BSD-3-Clause，Copyright 2014 The PDFium Authors；文末附 Apache-2.0 全文）；
abseil.txt（Apache-2.0）；agg23.txt（Anti-Grain Geometry 2.3，BSD 风格）；
dragonbox-Apache2-LLVM.txt（Apache-2.0 LLVM 例外）＋ dragonbox-Boost.txt
（Boost Software License 1.0）；fast_float.txt（MIT）；freetype.txt
（FreeType Project LICENSE，BSD 风格）；harfbuzz.txt（Old MIT）；icu.txt
（UNICODE LICENSE V3）；lcms.txt（lcms，MIT 风格）；libjpeg_turbo.ijg（IJG
BSD 风格）＋ libjpeg_turbo.md（IJG + Zlib 双兼容许可）；libopenjpeg.txt
（BSD-2-Clause）；libpng.txt（PNG Reference Library License v2，libpng/Zlib 风格）；
llvm-libc.txt（Apache-2.0 with LLVM Exceptions）；simdutf.txt（MIT）；
zlib.txt（Zlib）。分发时义务：licenses/ 目录全文随包（或随本 NOTICES 附录
全文引用），不得裁剪。

## dev（构建期工具，不随产物分发——记录备查）

| 生态 | 组件 | 版本 | 许可证 |
|---|---|---|---|
| npm | esbuild | 0.25.x | MIT |
| npm | typescript | 5.7.x | Apache-2.0 |

## SBOM 复跑结论（v1.6 collab-r1；v1.5 及更早为历史记录）

**v1.6 复跑记录（2026-10-21，本轮）**：

- 复跑基准：产品仓 main @ b3ec4da 的 Cargo.lock（161 条目 = 11 自研 +
  150 第三方）；cargo 1.99.0；四面命令与 v1.2-v1.5 同款（host
  linux-x64，`-e normal` 含 proc-macro 边，`--prefix none` 去重）：
  1. default → **75 项第三方**（v1.5 文档 62 + collab 12 + 补账漂移
     7 − 重建口径方差 1：460f302 基线重建复测 63）
  2. +engine-pdfium → **88 项**（+13 同构）
  3. +engine-lopdf → **108 项**（+13 同构）
  4. dual → **121 项**（+13；dual = 四面并集 = 正文 cargo 面）
- 增量源：mdc-collab 协作引擎（collab-r1，协作与 Canvas 转正式交付
  目标）——yrs 0.28.0（D1 集成验证边界四项边界在档：GC/UAF 隔离
  [skip_gc 不变量]、事务粒度、撤销栈归属、稳定锚点跨模型映射）+
  13 传递闭包（12 全四面 + redox_syscall 目标面账）。
- 核验结论：①新入 14 项 lock cksum == cargo 注册表下载验证（传递）；
  ②既有 132 项零版本漂移（hashbrown 0.17.1 原位）；③零正文项跌出
  编译面；④**零 UNKNOWN**（120 正文 + 29 分类账全部解析）；
  ⑤**GPL/AGPL 零命中**（150 项 lock 第三方全量——yrs 闭包全
  MIT/Apache 双许可，与 yrs-a 评估 pass 的闭包 45 包零 copyleft
  定谳一致）。
- 许可面：yrs/dashmap = MIT（registry manifest 直读 + yrs LICENSE
  一手直读在档[分析仓 yrs-a]）；其余 10 面 crate = MIT OR
  Apache-2.0（按 MIT 履行）——零新 AND 条款、零新 copyleft。
- web（npm）面：本轮零变更（package-lock.json 零 diff；npm ship
  12 项沿用）。
- ASSET_CHECK 对账：无新增分发资产（yrs 闭包全为源码 crate）。
- 补账登记（合规缺口）：v1.5 后 t19-t22/r4/r5/S1/S2 期间 NOTICES
  未随轮复跑——本轮复跑吸收该窗口全部面漂移（serde 族 + futures/
  js-sys 族，见 v1.6 增量明细补账段）；此后纪律恢复：**凡 lock 变
  化或 feature 面变化的轮次，NOTICES 增量随批**。

## SBOM 复跑结论（v1.5 xlsx-r2 Phase 3——历史记录）

**v1.5 复跑记录（2026-10-14，本轮）**：

- 复跑基准：产品仓 main @ 460f302 的 Cargo.lock（144 条目 = 8 自研 +
  136 第三方）；cargo 1.99.0；四面命令与 v1.2/v1.3/v1.4 同款（host
  linux-x64，`-e normal` 含 proc-macro 边，`--prefix none` 去重；
  default 面 = default-members + mdcore-wasm 的闭包，口径一致）：
  1. default → **62 项第三方**（v1.4 58 + 4：新入 2 [docx-rs/base64]
     + thiserror/thiserror-impl [原仅 lopdf feature 面] 随 docx 链
     入 default 面）
  2. +engine-pdfium → **75 项**（v1.4 71 + 4，同理）
  3. +engine-lopdf → **95 项**（v1.4 93 + 2 = 新入 2）
  4. dual → **108 项**（v1.4 106 + 2 = 新入 2；dual = 四面并集 =
     正文 cargo 面，逐项并集实证）
- 增量源：mdc-office-io DOCX 模型级读/写切片（xlsx-r2 Phase 3，
  用户三阶段指令）——docx-rs 0.4.22（ADR-O1 O1-3 选型：
  bokuweb/docx-rs，MIT；`default-features = false` 关闭 image 特性
  [png/tiff/gif/bmp/jpeg 巨树不入闭包]）+ base64 0.22.1（docx-rs
  必依赖；无传递依赖）。
- 核验结论：①三链核验 2/2 新入项（lock cksum == .crate SHA256
  逐位一致；index cksum 经 cargo 下载验证传递）；②既有 134 项零
  版本漂移；③零正文项跌出编译面；④**零 UNKNOWN**（108 正文 +
  28 分类账全部解析）；⑤**GPL/AGPL 零命中**（136 项 lock 全量）。
- 许可面：docx-rs = MIT（源 Cargo.toml 直读 + 分析仓 office-e
  浅克隆 LICENSE 一手在档）；base64 = MIT OR Apache-2.0（registry
  manifest 直读，按 MIT 履行）——零新 AND 条款、零新 copyleft。
- web（npm）面：本轮零变更（package-lock.json 零 diff；npm ship
  12 项沿用）。
- ASSET_CHECK 对账：无新增分发资产（docx-rs/base64 全为源码
  crate；pdfium 三平台条目维持）。
- 供应链观察项（office-e §4，v1.5 接线）：docx-rs 上游已合入下游
  fork 功能线（FORK_CHANGES.md）——缓解 = 锁版本（Cargo.lock 钉
  0.4.22）+ 破坏性变更先跑读侧往返测试（mdcore
  tests/docx_roundtrip.rs 常驻化——升级时首跑拦截）。

**以下为 v1.4（xlsx-r2 Phase 1）历史记录**：

**v1.4 复跑记录（2026-10-14，本轮）**：

- 复跑基准：产品仓 main @ e8c3a13 的 Cargo.lock（142 条目 = 8 自研 +
  134 第三方，与 v1.3 锁零 diff——本轮为纯面重分类，无新 lock 条目）；
  cargo 1.99.0；四面命令与 v1.2/v1.3 同款（host linux-x64，`-e normal`
  含 proc-macro 边，`--prefix none` 去重；default 面 = default-members +
  mdcore-wasm 的闭包，口径一致）：
  1. default → **58 项第三方**（v1.3 50 + 8：账提升 5 + 3 项原仅在
     feature 面的传递依赖[bitflags/getrandom/libc]随 tempfile 链入面）
  2. +engine-pdfium → **71 项**（v1.3 64 + 7）
  3. +engine-lopdf → **93 项**（v1.3 88 + 5）
  4. dual → **106 项**（v1.3 101 + 5 = 账提升 5；dual = 四面并集 =
     正文 cargo 面，逐项并集实证）
- 增量源：mdc-office-io 写面产品化（xlsx-r2 Phase 1，C2 写面——用户
  三阶段指令）——rust_xlsxwriter 0.99.1 由 dev 面（测试语料构造）升
  产品 [dependencies]（features = ["constant_memory"]，流式大表能力）；
  其 constant_memory 特性激活可选依赖 tempfile → fastrand/getrandom/
  cfg-if/rustix/linux-raw-sys 链进 normal 面。
- 核验结论：①零新 lock 条目（Cargo.lock 与 18d5bb2 逐位零 diff，
  三链核验态沿用 v1.3）；②零版本漂移；③零正文项跌出编译面；
  ④**零 UNKNOWN**（106 正文 + 28 分类账全部解析）；⑤**GPL/AGPL
  零命中**（134 项 lock 全量维持）。
- 许可面：提升 5 项中 rust_xlsxwriter/tempfile = MIT OR Apache-2.0
  （按 MIT 履行）、fastrand = Apache-2.0 OR MIT（同义写法，按 MIT
  履行）、rustix/linux-raw-sys = Apache-2.0 WITH LLVM-exception OR
  Apache-2.0 OR MIT（新正文族——LLVM-exception 仅为三选一 OR 选项
  之一，按 Apache-2.0 或 MIT 履行，不构成额外义务）。
- web（npm）面：本轮零变更（package-lock.json 零 diff；npm ship 12 项
  沿用，沿 R23 367 项口径不变）。
- ASSET_CHECK 对账：无新增分发资产（rust_xlsxwriter/tempfile 全为
  源码 crate；tempfile 运行时临时文件为用户态工件非分发资产；pdfium
  三平台条目维持）。

**以下为 v1.3（xlsx-r1 阶段 B）历史记录**：

**v1.3 复跑记录（2026-10-13，本轮）**：

- 复跑基准：产品仓 main @ 880864e 的 Cargo.lock（142 条目 = 8 自研 +
  134 第三方）；cargo 1.99.0；四面命令与 v1.2 同款（host linux-x64，
  `-e normal` 含 proc-macro 边，`--prefix none` 去重；default 面 =
  default-members + mdcore-wasm 的闭包——wasm 产物随 web 面分发，
  口径与 v1.2 一致）：
  1. default → **50 项第三方**（v1.2 29 + 21：新入 9 + 账提升 2 +
     原 PDF 面传递依赖 10 项随 office-io 进 default 面）
  2. +engine-pdfium → **64 项**（v1.2 45 + 19）
  3. +engine-lopdf → **88 项**（v1.2 76 + 12）
  4. dual → **101 项**（v1.2 90 + 11 = 新入 9 + 账提升 2；dual = 四面
     并集 = 正文 cargo 面，逐项并集实证）
- 增量源：mdc-office-io（xlsx-r1 阶段 B 新 crate，双 crate 方案 A）直依赖
  calamine 0.36.1 + zip 8.6.0 及其闭包（DEPS.md 锁定版）；dev 面
  rust_xlsxwriter（语料构造）不入正文。
- 核验结论：①三链核验 16/16（lock cksum == 索引 cksum == .crate SHA256，
  windows-sys 无 .crate 缓存→链①②+ API 直验）；②零版本漂移（118→
  134 全部同名同版）；③零正文项跌出编译面；④**零 UNKNOWN**（101 正文 +
  33 分类账全部解析）；⑤**GPL/AGPL 零命中**（134 项全量）。
- 两处许可修正（锁定版本工件为准，分析仓 DEPS.md 已回写）：zip 8.6.0 =
  MIT 单许可（首版记双许可系口径混淆）；rust_xlsxwriter 0.99.1 =
  MIT OR Apache-2.0（首版记 MIT；按 MIT 履行不变）。
- web（npm）面：本轮零变更（package-lock.json 零 diff；npm ship 12 项沿用，
  沿 R23 367 项口径不变）。
- ASSET_CHECK 对账：无新增分发资产（office 线零二进制；pdfium 三平台
  条目维持）。

**以下为 v1.2（PDF-e 收口轮）历史记录**：

- **复跑基准**：产品仓 main @ 3e450aa 的 Cargo.lock（git show 提取，与主 checkout
  `cargo tree --locked` 实跑一致）；cargo 1.99.0；四面命令（host linux-x64，`-e normal`
  含 proc-macro 边，`--prefix none` 去重）：
  1. default：`cargo tree --workspace --locked -e normal` → **29 项第三方**
  2. +engine-pdfium：`--features mdc-pdf/engine-pdfium` → **45 项**（default + 16）
  3. +engine-lopdf：`--features mdc-pdf/engine-lopdf` → **76 项**（default + 47）
  4. dual：`--features "mdc-pdf/engine-pdfium,mdc-pdf/engine-lopdf"` → **90 项**
- **加法自洽**：dual 90 = default 29 + pdfium 增 16 + lopdf 增 47 − 双面共享 2
  （bitflags、log 同为两引擎直接依赖）；dual 逐项 = 三面并集（cargo tree 实证
  dual ⊆ 并集且并集 ⊆ dual，零差集）。
- **新入账两族**：pdfium-render 族 16 项（pdfium-render 0.9.4 =0.9.4 pin
  （pdfium_7881 绑定面 + thread_safe；default-features=false 零 image）及其闭包
  bitflags/bytemuck/byteorder/bytes/chrono/either/iana-time-zone/itertools/
  libloading/log/maybe-owned/num-traits/piston-float/utf16string/vecmath）；
  lopdf 族 47 项（lopdf 0.45.0 =0.45.0 pin（default-features=false——
  chrono/rayon/image 均不进图，PDF-c/d 决策原文兑现）及其闭包
  adler2/aes/alloc-no-stdlib/alloc-stdlib/bitflags/block-buffer/block-padding/
  brotli-decompressor/cbc/chacha20/cipher/const-oid/core_detect/cpubits/
  cpufeatures/crc32fast/crypto-common/digest/ecb/encoding_rs/flate2/getrandom/
  hybrid-array/inout/libc/log/md-5/miniz_oxide/multiversion_no_op/nom/rand/
  rand_core/rangemap/scopeguard/sha2/simd-adler32/simdutf8/stringprep/thiserror/
  thiserror-impl/tinyvec/typenum/unicode-bidi/unicode-normalization/
  unicode-properties/weezl——**以四面复跑实测为准**，与 PDF-c/d 前瞻登记的
  「42 包 / 46 包（lock 解析口径）」差异即目标面门控与可选未启用项，全部
  落入下述分类账）。
- **核验结论**：①版本与 Cargo.lock 逐位一致（四面清单由 lock 解析生成，
  checksum 三链核验 118/118）；②许可分类 ADR-C1 白名单族全过
  （新族 BSD-3-Clause/ISC/0BSD/Zlib-三选一-elect-MIT/AND 复合均逐项论证，
  见 §义务备忘）；③**零 UNKNOWN**（90 正文 + 28 分类账全部解析）；
  ④**GPL/AGPL 零命中**。
- **lock 全量分类账（150 = 121 正文 + 29 非编译面，v1.6 实测）**：
  **v1.6 移出 6 项**（futures-core/futures-task/futures-util/js-sys/
  pin-project-lite/slab：原 wasm32 目标面账项，t19 后
  rust_xlsxwriter 特性面变化使其进入 host normal 正文面——已入
  正文表，下表原行保留作历史移出记录）；**v1.6 新入 1 项**
  （redox_syscall，见下表新增行）。现存 29 项逐项附
  版本、许可、状态分类如下（v1.2 遗留项维持 + v1.3 新增 windows-sys；
  rustversion/zlib-rs 两项因 office-io 读闭包、rust_xlsxwriter/tempfile/
  fastrand/rustix/linux-raw-sys 五项因 xlsx-r2 Phase 1 写面产品化
  [constant_memory 特性激活 tempfile 链]先后提升入正文已移出本账；
  v1.5 新入 docx-rs/base64 均进正文不入账；每项均通过
  index-cksum == lock-checksum 核验）：

| 组件 | 版本 | 许可（registry manifest 原文） | 状态分类 |
|---|---|---|---|
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | android 目标面（iana-time-zone 依赖；不分发） |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | host 构建期（num-traits 的 build-dependency，不进运行时面） |
| cc | 1.7.0 | MIT OR Apache-2.0 | haiku 目标面（iana-time-zone-haiku 的构建期链，不分发） |
| console_error_panic_hook | 0.1.7 | Apache-2.0/MIT | wasm32 目标面（pdfium-render/chrono 的 wasm 支线；本仓 wasm 构建不启用引擎特性，休眠） |
| console_log | 1.1.0 | MIT/Apache-2.0 | wasm32 目标面（同上） |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | macos 目标面（iana-time-zone 依赖；macos 分发时进图，届时增量入账） |
| errno | 0.3.14 | MIT OR Apache-2.0 | rustix 的 process/std 特性面依赖；本仓 normal 面 rustix（经 tempfile 链，v1.4 入正文）未启用该特性，任何本仓 face 不编译，不分发 |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | haiku 目标面（cc 的构建期链，不分发） |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | wasm32 目标面（同上） |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | wasm32 目标面（同上） |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | wasm32 目标面（同上） |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 | haiku 目标面（iana-time-zone 依赖；不分发） |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | wasm32 目标面（同上） |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | wasm32 目标面（同上） |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | uefi 目标面（getrandom 依赖；不分发）。LGPL-2.1-or-later 仅为三选一 OR 选项之一（可按 MIT/Apache-2.0 履行），且本项不进任何分发面——GPL/AGPL 零命中结论不受影响 |
| redox_syscall | 0.5.18 | MIT | redox 目标面（parking_lot_core 依赖；redox 分发时进图届时增量入账。v1.6 新入 lock——yrs 闭包） |
| shlex | 2.0.1 | MIT OR Apache-2.0 | haiku 目标面（cc 的构建期链，不分发） |
| slab | 0.4.12 | MIT | wasm32 目标面（同上） |
| syn | 2.0.119 | MIT OR Apache-2.0 | windows 目标面（windows-implement/interface 依赖；win 分发时进图届时增量入账。第二版本实例：本项 2.0.119，正文 syn 3.0.6 为 serde_derive/thiserror-impl 所用） |
| tokio | 1.53.2 | MIT | wasm32 目标面（同上） |
| wasm-bindgen-futures | 0.4.79 | MIT OR Apache-2.0 | wasm32 目标面（同上） |
| web-sys | 0.3.106 | MIT OR Apache-2.0 | wasm32 目标面（同上） |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | windows 目标面（iana-time-zone 依赖；win 分发时进图，届时增量入账） |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | windows 目标面（同上） |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | windows 目标面（同上） |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | windows 目标面（同上） |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | windows 目标面（同上） |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | windows 目标面（同上） |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | windows 目标面（rustix 依赖；rustix 已随 v1.4 入正文，但本项仅在 windows 目标进图，win 分发时届时增量入账。v1.3 新入 lock） |

- **目标面前瞻**：win/macos 分发（packaging/nsis、packaging/macos 已在档）时，
  上表 windows/macos 门控项进图 → 届时重跑 SBOM 增量入账（常驻门禁条款：
  「启用新系统依赖后重跑 SBOM 并更新 THIRD-PARTY-NOTICES」同类义务）。
- **web（npm）面**：本轮零变更——package-lock.json 自 R23 快照（24268ca）至
  main @ 3e450aa **零 diff**（唯一变动为 package.json 描述串文档更新，依赖面
  deps/devDeps 零变化）；NOTICES npm ship 12 项沿用；**沿 R23 367 项口径不变更**
  （R22-c SBOM 交叉基线继续有效）。
- **R24-e 义务兑现声明**：R24-c 打包轮登记「R24-e 收口时执行完整 SBOM 复跑」；
  PDF-c/d 登记的 NOTICES 前瞻（pdfium-render 族 / lopdf 族 / pdfium 二进制三平台）
  ——**本节即兑现记录**，PDF-e 收口后 SBOM/NOTICES 无未了义务。

## 许可证义务备忘（白名单族）

- MIT / BSD / ISC / Zlib / Unlicense-MIT / 0BSD：保留版权与许可声明副本（本文件即载体）。
- Apache-2.0：保留声明副本 + NOTICE 文件变更说明（如有）。
- 双许可（A OR B 形态：MIT OR Apache-2.0、Unlicense OR MIT、BSD-3-Clause/MIT、
  0BSD OR MIT OR Apache-2.0、Zlib OR Apache-2.0 OR MIT、MIT OR Zlib OR Apache-2.0）：
  按 MIT 履行即可（本选择登记于 ADR-C1 §1 允许域）。
- AND 复合（encoding_rs：(Apache-2.0 OR MIT) AND BSD-3-Clause）：MIT/Apache 分支
  任选其一 + **同时**履行 BSD-3-Clause 通知义务（两份声明均随附）。
- unicode-ident（(MIT OR Apache-2.0) AND Unicode-3.0）：MIT/Apache 任选其一 +
  Unicode-3.0 声明随附（Unicode-3.0 为宽松许可，含归属与免责条款）。
- pdfium 二进制：BSD-3-Clause（保留上述声明 + licenses/ 全量随包）；
  Apache-2.0 项（abseil 等）按 NOTICE 条款处理。
- GPL/AGPL 族：**零容忍**（ADR-C1）——本轮 118 项 lock 条目全量核验零命中；
  后续任何依赖变更复跑本 SBOM 时若 UNKNOWN 复核出此类许可，立即升级处置。
