//! mdc-office — Office 域引擎侧基础设施。
//!
//! office-impl T1（2026-10-12，office-line 开发启动轮）：第一模块
//! `sanitize` = 敌意输出面消毒五面；xlsx-r1 阶段 B（2026-10-13）
//! 增第六面 `url_policy`（URL scheme 白名单——t10-e 复核发现的缺口
//! 补面：scheme 级策略此前无承载面，字符转义对 scheme 攻击无效）。
//! 三域纪律（用户裁决 2026-10-12）：MD / PDF / Office 保持独立领域
//! 模型与引擎实现——本 crate 对 mdcore **零编译依赖**、零三方依赖
//! （纯 std），循 mdc-pdf 并列引擎先例接入 workspace（版本单源随
//! workspace.package）。XLSX IO 三方依赖承载于姊妹 crate
//! mdc-office-io（双 crate 方案 A，xlsx-r1 REPORT §2）。
//!
//! ## 消毒六面（规格 = 分析仓 office-d §5 一手代码级裁决）
//!
//! 背景：生态写库在三个安全面全部缺位或损坏——CSV 公式注入零消毒
//! （其一主动以 `=` 前缀重建公式文本）、XML 非法控制字符三档实况
//! （损坏档：非法字符原样透传记产物拒开；剥除档：合法 XML 但静默丢
//! 字符；唯保字符档达标）、域指令任意直通零白名单。**消毒责任在管线
//! 层**——本 crate 即引擎侧工具包，宿主/生成态管线在出口与喂入位调用。
//!
//! | 面 | 模块 | 职位 |
//! |----|------|------|
//! | CSV 公式首字符（`=` `+` `-` `@`） | [`sanitize::csv_formula`] | CSV/TSV 出口 |
//! | XML 控制字符（保字符档 `_xHHHH_`） | [`sanitize::xml_control`] | XML 文本喂入前 |
//! | XML 实体（含属性位兜底） | [`sanitize::xml_entities`] | XML 文本/属性喂入前 |
//! | HTML 出口转义（href 属性注入面） | [`sanitize::html_escape`] | HTML 出口 |
//! | wordprocessing 域指令白名单 | [`sanitize::field_whitelist`] | 生成侧（LLM 面） |
//! | URL scheme 白名单 | [`sanitize::url_policy`] | 链接位出口（结构位） |
//!
//! ## 消费契约注记
//!
//! 1. **xlsx 写路线豁免 CSV 消毒**：目标表格应用对包内共享字符串
//!    （t="s"）不重解释为公式——公式注入只发生在 CSV/TSV 粘贴与打开
//!    路径（office-d §5.1 裁决 2）。对 xlsx 出口误用本模块是规格外
//!    行为（多一层无害但无意义的前缀）。
//! 2. **_xHHHH_ 保字符档**：对 XML 1.0 非法控制字符与 U+FFFE/U+FFFF
//!    以 `_xHHHH_` 约定转义（ECMA-376 电子表格字符串既定约定），
//!    编码器含**字面量消歧**（正文恰含 `_xHHHH_` 形态时前置转义其
//!    下划线）——任意输入编码-解码往返零失真（性质测试锁死）；该
//!    消歧是本 crate 超出生态参照库的登记项（office-d §5.2 裁决 3
//!    的【推断】歧义面在此解决）。
//!    **幂等契约（面 2）**：幂等语义 = `decode(encode(x)) == x` 恒
//!    成立（含 x 本身已是消歧形态）；**字符串级** `encode(encode(x))
//!    == encode(x)` **不成立**（消歧前缀逐级增长，有意设计——否则
//!    产生解码歧义）。防重复编码的责任面 = 管线纪律（每出口消毒一次，
//!    不叠加）；已消毒文本自检可用输入扫面判形态后跳过。
//! 3. **域白名单默认口径**：白名单 DATE/PAGE/NUMPAGES/TOC/PAGEREF/
//!    MERGEFIELD/SEQ；黑名单首列 INCLUDETEXT/INCLUDEPICTURE（外部
//!    资源面）/DDE（执行面）——拒绝族**全 token 扫描**（指令参数
//!    内嵌拒绝族名同样拦截，防嵌套域注入）。
//!    **保守误杀登记（面 5）**：数据字段名恰以黑名单字开头（如
//!    `MERGEFIELD INCLUDETEXT_list`）会被拦截——安全优先于可用性，
//!    已知保守误杀，登记不修复（病理形态；真实 MERGEFIELD 字段名
//!    不叫这个）。
//! 4. **XML/HTML 实体面非幂等（有意）**：纯转义器必须把已转义形态
//!    当字面文本（`&amp;` → `&amp;amp;`）——否则下游解码失真。
//!    幂等性故意不成立，测试锁死防“优化”成幂等（那会破坏往返）。
//! 5. **URL 管线顺序（面 6 与面 4 联防）**：出口管线 = url_policy
//!    先（scheme 结构位，reject 出）→ html_escape 后（字符位转义
//!    余部）；顺序反了会把 reject 证据转义掉（不破坏安全性但破坏
//!    错误信息）。url_policy 判结构位（scheme/UNC/NUL/非法原生字
//!    符），html_escape 判字符位（属性与文本转义）——两职位不可
//!    互替。

pub mod sanitize;
