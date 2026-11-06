//! 样式模板体系（S1 任务 1：StyleTree + basedOn 链 + 主题色）。
//!
//! ## 定位与边界
//!
//! DOCX 的 `word/styles.xml`（样式表）与 `word/theme/theme1.xml`（主题
//! 部件颜色方案）的**模型级读承载**，以及写面的自定义样式定义/
//! 引用（[`DocxWriter::define_style`]/[`DocxWriter::styled_paragraph`]，
//! 在本模块对 [`crate::docx::DocxWriter`] 的分文件 impl）。
//!
//! 与 [`crate::docx`] 读面（块序提取）**并列的独立入口**：
//! [`read_docx_styles`] 系列单趟读出 [`StyleBundle`]（样式树 + 主题
//! 色表 + 结构化警告），不触碰 [`crate::docx::DocxDocument`] 公开
//! 结构（零构造点波及——消费方按需并行调用，wasm 序列化面不变）。
//!
//! ## 实现路线：自有解析层（不走引擎样式模型）
//!
//! `docx-rs` 0.4.22 引擎对 `styles.xml` 有读面（`Docx.styles`），但
//! **四个关键承载缺失/不可达**（源级一手核验，reader/style.rs 与
//! documents/elements/style.rs）：
//!
//! 1. `w:default`（文档默认样式位）——引擎 `Style` 结构无该字段，
//!    读面不解析；
//! 2. `w:next`（回车后续样式）——结构有字段（builder 有 `next()`）
//!    但 **reader 永不填充**（read 循环无 Next 分支）；
//! 3. `w:qFormat`/`w:uiPriority`——`Style` 的 serde 字段存在，读面
//!    同样不填充（Default 值恒定：q_format=true/ui_priority=None，
//!    与真实值不可区分）；
//! 4. `basedOn`/`name`/`link` 的**字符串值不可提取**——引擎类型
//!    （`Name`/`BasedOn`/`Link`）字段私有且无公开 getter，basedOn 链
//!    解析需要任意字符串值，`PartialEq` 构造比较不可用。
//!
//! 因此本模块对 `styles.xml` 与 `theme1.xml`（引擎 `Theme` 只解析
//! FontScheme，`clrScheme` 整面缺失——reader/theme.rs 单 match）走
//! **office-io 自有 quick-xml 解析层**（与 [`crate::io`] 门 3 部件
//! 良构性校验同族技术，零新三方依赖）。引擎读面不动不删（`Docx.styles`
//! 保留引擎自身行为）；本层是 styles/theme 的唯一消费真源。
//!
//! ## 主题色解析链
//!
//! `w:color w:themeColor="accent1" w:themeShade="99" w:themeTint="…"`
//! 的解析语义（Word 实现口径 = **[MS-OI29500] §2.1.72 / [MS-OE376]
//! Part 4 §2.3.2.5 实现注记**，#269 实证钉定））：
//!
//! 1. **槽位映射**（`ST_ThemeColor` → DrawingML `a:clrScheme` 槽）：
//!    `dark1→dk1` / `light1→lt1` / `dark2→dk2` / `light2→lt2` /
//!    `accent1-6→accent1-6` / `hyperlink→hlink` /
//!    `followedHyperlink→folHlink`；语义别名 `text1≡dark1` /
//!    `background1≡light1` / `text2≡dark2` / `background2≡light2`；
//!    `none` = 显式无主题色（回退 `w:val`）。
//! 2. **槽位取值**：`a:srgbClr@val` 直值；`a:sysClr@lastClr`（系统
//!    色，`lastClr` 缺席 = 槽不可解析——登记，不猜值）。
//! 3. **shade/tint 调制（HSL luminance 空间——非逐通道线性）**：
//!    Word 对 `w:rPr/w:color` 在 **HSL 色彩空间修改亮度因子**
//!    （[MS-OI29500] §2.1.72(a)(c)：转 HSL（值 0-1）→ 改 luminance
//!    → 转回 RGB；LibreOffice `Color::ApplyTintOrShade` +
//!    `basegfx::utils::rgb2hsl/hsl2rgb` 同构互操作参考——其源码
//!    注释明证「MS Office uses themeTint and themeShade on the
//!    luminance in a HSL color space … That is different from OOXML
//!    specification」）。
//!    - shade `S`（hex 字节）：`L' = L × S/255`（亮度乘法变暗）；
//!    - tint `T`（hex 字节）：`L' = L × T/255 + (1 − T/255)`
//!      （lumMod+lumOff——亮度向白插值）；
//!    - 同写时 **tint 赢**（[MS-OI29500] §2.1.72(b)：「Word applies
//!      the themeTint setting when both of these attributes are
//!      present」——旧实现「shade 赢」无出处，#269 修正）；
//!    - 通道转换 **floor 取整**（语料实证：Word 缓存值呈 floor 特征，
//!      round 会系统性 +1 偏离——24 件 Word 产样本拟合：floor 14
//!      精确 + 10 漂移±1，round 仅 5 精确）；
//!    - `S/T = FF`（255）= 恒等直传（避免 HSL 往返浮点误差）；
//!      `= 00` 与缺席不可辨时按缺席（见 `mod_byte`）。
//!
//!    **规范文本 vs Word 实现的分歧（登记）**：ECMA-376 部分子条款
//!    （边框色/Frameset Splitter 等面）文档化了逐通道线性公式
//!    （`C'=C×S/255`）与算例（`7B4B23`/`D99694`）——那是**别的元素**
//!    的算法；`w:rPr/w:color` 的 Word 实现是 HSL（微软官方实现注记
//!    明示「different from OOXML specification」）。本层以 Word
//!    实际渲染为准（issue #269 验收口径「用 Word 实际渲染对照」）。
//!
//!    数值例子（[MS-OI29500] §2.1.72 官方算例）：tint——`4F81BD`
//!    tint 99(hex)=60% → **95B3D7**（官方值精确命中）；shade——
//!    `C0504D` shade BF(hex)=75% → 本层 943734 vs 官方 943634
//!    （G 通道差 1：Word 内部定点/版本细节不可再分，语料同输入
//!    跨 Word 版本本身就有 365F91/376092/366091 三种缓存值）。
//! 4. `w:val`（如 `2E74B5`）是**渲染器回退缓存**：themeColor 在场
//!    时按主题重算（`resolve` 忽略 val）；**缺 theme 部件 → 默认
//!    Office 调色板降级**（#269：`ThemeColors::office_default`——
//!    12 槽标准值；此时 [`StyleBundle::effective_color`] 优先作者
//!    缓存 `w:val`（合法 RGB 时），调色板解析为第二级——作者渲染
//!    真值优先于猜的调色板）；槽缺失时消费方可回退 val——本层
//!    透传两者，不丢弃任何一面。
//!
//! ## basedOn 链（继承语义）
//!
//! [`StyleTree::chain`] 返回**基底→派生**序（docDefaults 之下的
//! 继承链）；[`StyleTree::effective_format`] 按「底层铺底、上层
//! Some 覆盖」逐字段合并（`w:b val="0"` 的显式关闭 = `Some(false)`
//! 覆盖底层 `Some(true)`——三态 `Option<bool>` 是合并语义的唯一
//! 诚实形态）。链异常两模式**非致命**（真实语料存在，与 numPr 悬空
//! 同族纪律）：
//!
//! - **basedOn 环**（A→B→A）：遍历截断 + `BrokenReference` 警告
//!   （链在首个重复节点处停——不 panic 不死循环）；
//! - **basedOn 悬空**（指向不存在样式）：链截断（以可见节点为链）
//!   + `BrokenReference` 警告。
//!
//! ## 面外登记（本层不承载——诚实降级）
//!
//! - `w:semiHidden`/`w:unhideWhenUsed`/`w:autoRedefine`/
//!   `w:customStyle`（UI 提示位——读面价值低，登记）；
//! - `w:rFonts` 的主题字体变体（`w:asciiTheme` 等 4 属性）与
//!   `w:szCs`/`w:iCs`/`w:bCs`（complex script 槽——与
//!   `w:ind` Chars 变体同族的对称面登记）；
//! - `w:rPr` 的 `w:highlight`/`w:vertAlign`/`w:shd`/`w:spacing`/
//!   `w:position`（行内格式面——样式模板最小承载集外，后续按
//!   消费面需要加性扩）；
//! - `w:pPr` 的 `w:spacing`（行距）/`w:keepNext`/`w:widowControl`/
//!   `w:tabs`/`w:pBdr`（段落排版面——r4-t3 ind 承载同界）;
//! - 样式表部件缺 `w:docDefaults` / 文档无 styles.xml/theme1.xml：
//!   合法形态（空树/空表），不警告（非 Word 产线的常态——空表
//!   lookup 恒 None，消费方以 `w:val` 回退可诊断）。
//!
//! ## 公开面清单
//!
//! 读：[`StyleType`]/[`StyleColor`]/[`StyleFont`]/[`StyleRunProps`]/
//! [`StyleParaProps`]/[`StyleRef`]/[`StyleTree`]/[`EffectiveFormat`]/
//! [`ThemeColors`]/[`StyleBundle`]/[`read_docx_styles`]/
//! [`read_docx_styles_from_bytes`]。
//! 写：[`StyleDef`]/[`DocxWriter::define_style`]/
//! [`DocxWriter::styled_paragraph`]。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read as _;

use crate::docx::{ParagraphIndent, ParagraphIndentSpecial};
use crate::io::{self, ReadOptions, WarningKind};

// ---------------------------------------------------------------------------
// 公开模型（读面）
// ---------------------------------------------------------------------------

/// 样式类型（`w:style/@w:type` 原值承载）。
///
/// 已知四类 + `unknown` 透传（读面不猜——未知 type 保留原字符串，
/// 与 [`crate::docx`] numFmt 透传同族）。写面
/// （[`StyleDef::type_`]）只接受已知四类（`Unknown` typed 拒绝）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleType {
    Paragraph,
    Character,
    Numbering,
    Table,
    /// 未知 `@w:type` 原值透传（读面专属形态）。
    Unknown(String),
}

impl StyleType {
    fn parse(raw: &str) -> Self {
        match raw {
            "paragraph" => StyleType::Paragraph,
            "character" => StyleType::Character,
            "numbering" => StyleType::Numbering,
            "table" => StyleType::Table,
            other => StyleType::Unknown(other.to_string()),
        }
    }
}

/// 样式颜色（`w:rPr/w:color` 四属性原样透传）。
///
/// `theme` = `w:themeColor` token（`accent1`/`dark1`/… 未知 token
/// 原样透传）；`shade`/`tint` = hex 字节字符串（`"99"`/`"BF"`——
/// 解析在 [`ThemeColors::resolve_color`]，原值保留供诊断）；
/// `val` = `RRGGBB` 或 `"auto"`（themeColor 在场时的渲染器回退缓存
/// ——`resolve` 忽略它，无主题消费方可用它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleColor {
    pub val: String,
    pub theme: Option<String>,
    pub shade: Option<String>,
    pub tint: Option<String>,
}

/// 样式字体（`w:rFonts` 四非主题槽）。主题字体变体
/// （`w:asciiTheme` 等）面外登记（见模块文档）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleFont {
    pub ascii: Option<String>,
    pub hi_ansi: Option<String>,
    pub east_asia: Option<String>,
    pub cs: Option<String>,
}

/// 样式级字符格式承载集（`w:style/w:rPr` / `w:docDefaults` 的
/// `w:rPrDefault` 同构）。
///
/// `bold`/`italic`/`strike` 三态：`None` = 未设（继承下层），
/// `Some(true)` = 置位，`Some(false)` = **显式关闭**（`w:b val="0"`——
/// 覆盖下层的 `Some(true)`，合并语义基础）。`size` 单位 = 半点
/// （OOXML `w:sz`：24 = 12pt）。`underline` = `w:u/@w:val` 透传
/// （`"single"`/`"none"`——`"none"` 是显式关闭）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleRunProps {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    pub underline: Option<String>,
    pub size: Option<usize>,
    pub color: Option<StyleColor>,
    pub fonts: Option<StyleFont>,
}

/// 样式级段落格式承载集（`w:style/w:pPr` / `w:pPrDefault` 同构）。
///
/// `indent` 复用块提取的 [`ParagraphIndent`]（twips 透传，同界纪律：
/// Chars 变体面外）；`alignment` = `w:jc/@w:val` 透传；
/// `outline_lvl` = `w:outlineLvl/@w:val`（0-8）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleParaProps {
    pub alignment: Option<String>,
    pub outline_lvl: Option<usize>,
    pub indent: Option<ParagraphIndent>,
}

/// 单个样式定义（`w:style` 整元素承载）。
#[derive(Debug, Clone, PartialEq)]
pub struct StyleRef {
    /// `@w:styleId`（如 `Heading1`/`Normal`——文档内唯一；重复
    /// styleId 后胜不警告（styles.xml 无重复约束的形态罕见，Word
    /// 自产恒唯一）。
    pub id: String,
    pub type_: StyleType,
    /// `w:name/@w:val`（UI 名，如 `heading 1`）。
    pub name: Option<String>,
    /// `w:basedOn/@w:val`——继承链父样式 id。
    pub based_on: Option<String>,
    /// `w:next/@w:val`——回车后续样式 id（引擎读面缺失项，本层承载）。
    pub next: Option<String>,
    /// `w:link/@w:val`——对偶样式 id（段落↔字符）。
    pub link: Option<String>,
    /// `w:qFormat` 在场（样式面板可见性位）。
    pub q_format: bool,
    /// `w:uiPriority/@w:val`。
    pub ui_priority: Option<usize>,
    /// `@w:default="1"`（该 type 的文档默认样式——段落无 `pStyle`
    /// 时的生效样式；引擎读面缺失项，本层承载）。
    pub is_default: bool,
    pub run: StyleRunProps,
    pub para: StyleParaProps,
}

/// `w:docDefaults`（`w:rPrDefault/w:rPr` + `w:pPrDefault/w:pPr`——
/// 全样式链的铺底层）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleDefaults {
    pub run: StyleRunProps,
    pub para: StyleParaProps,
}

/// 样式树（整表 + 链解析 + 有效格式合并）。
///
/// `order` 保留 `styles.xml` 出现序（消费方 UI 按原序展示）；
/// `by_id` 查找（`HashMap`——查找序非确定，消费方须自排序）。
#[derive(Debug, Clone, Default)]
pub struct StyleTree {
    order: Vec<String>,
    by_id: HashMap<String, StyleRef>,
    pub doc_defaults: StyleDefaults,
}

/// 有效格式（合并结果——docDefaults → basedOn 链基底→派生）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EffectiveFormat {
    pub run: StyleRunProps,
    pub para: StyleParaProps,
}

/// 主题颜色表（`a:clrScheme` 12 槽：槽名→`RRGGBB` 大写）。
///
/// 槽名 = clrScheme 子元素本地名原样（`dk1`/`lt1`/…/`hlink`/
/// `folHlink`）；`lookup` 接受 **themeColor token**（做 §映射
/// dark1→dk1 等 + 别名 text1≡dark1），非 token 原样查（未命中
/// → None）。
///
/// `fallback_default = true` 表示这是**默认 Office 调色板降级态**
/// （theme 部件缺席/悬空/零槽时 [`read_docx_styles_from_bytes`]
/// 自动接管——#269 验收「缺 theme1.xml → 优雅降级」）；消费方经
/// [`ThemeColors::is_default_fallback`] 可区分「文档真主题」与
/// 「猜的调色板」（后者场景下 `w:val` 作者缓存优先，见
/// [`StyleBundle::effective_color`] 的降级阶梯）。
#[derive(Debug, Clone, Default)]
pub struct ThemeColors {
    slots: BTreeMap<String, String>,
    fallback_default: bool,
}

/// 读结果捆（样式树 + 主题色表 + 容器门面警告）。
#[derive(Debug, Clone)]
pub struct StyleBundle {
    pub tree: StyleTree,
    pub theme: ThemeColors,
    pub warnings: Vec<crate::io::CompatibilityWarning>,
}

// ---------------------------------------------------------------------------
// 合并语义（basedOn 链）
// ---------------------------------------------------------------------------

impl StyleTree {
    /// 全量样式 id（`styles.xml` 出现序）。
    pub fn style_ids(&self) -> &[String] {
        &self.order
    }

    pub fn get(&self, id: &str) -> Option<&StyleRef> {
        self.by_id.get(id)
    }

    /// basedOn 链（**基底→派生**序）：`id` 自身为链尾。
    ///
    /// - 未知 id → `None`；
    /// - 悬空 basedOn（父不存在）：以**可见节点**为链（截断点即链头）
    ///   ——警告在读入时一次性登记（见模块文档）；
    /// - 环（A→B→A）：`id` 起沿 basedOn 走到**重复节点**处截断
    ///   （visited 集合——链恒有限，不 panic）。
    pub fn chain(&self, id: &str) -> Option<Vec<&StyleRef>> {
        if !self.by_id.contains_key(id) {
            return None;
        }
        // 收集：从 id 沿 basedOn 上溯（倒序入栈），重复即停。
        let mut up: Vec<&StyleRef> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut cur = id.to_string();
        while let Some(s) = self.by_id.get(&cur) {
            if !seen.insert(cur.clone()) {
                break; // 环：重复节点处截断
            }
            up.push(s);
            match &s.based_on {
                Some(parent) if self.by_id.contains_key(parent) => cur = parent.clone(),
                _ => break, // 悬空（截断）或无 basedOn（链头）
            }
        }
        up.reverse(); // 基底→派生
        Some(up)
    }

    /// 有效格式：`docDefaults` 铺底 → basedOn 链（基底→派生逐层
    /// Some 覆盖）→ `id` 自身。未知 id → `None`。
    pub fn effective_format(&self, id: &str) -> Option<EffectiveFormat> {
        let chain = self.chain(id)?;
        let mut eff = EffectiveFormat {
            run: self.doc_defaults.run.clone(),
            para: self.doc_defaults.para.clone(),
        };
        for s in chain {
            merge_run(&mut eff.run, &s.run);
            merge_para(&mut eff.para, &s.para);
        }
        Some(eff)
    }

    /// 有效格式（typed 严格面，#267）：沿 basedOn 链累积，环与未知
    /// id 以 typed 错误上抛；与 [`StyleTree::effective_format`]（宽松
    /// 面）互补共存：
    ///
    /// | 情形 | `effective_format`（宽松面，s1-t1 行为不变） | `effective_props`（本面） |
    /// |---|---|---|
    /// | 未知 id | `None` | `Err(StyleChainError::UnknownStyle)` |
    /// | basedOn 环（A→B→A） | visited 截断合并（+读入时警告） | `Err(StyleChainError::CycleDetected)` |
    /// | 悬空 basedOn（父无定义） | 截断合并 | **优雅降级**：以可见链合并（`Ok`——#267 验收 4） |
    ///
    /// 环语义按 #267 验收 3「A→B→A 报 typed 错误不 panic」：严格面把
    /// 环显式交给消费方处置（宽松面保留 s1-t1 的截断合并，零回归）。
    /// 合并序与宽松面同构：`docDefaults` 铺底 → basedOn 链基底→派生
    /// 逐层 `Some` 覆盖 → 查询 id 自身（字段级覆盖，`Option<bool>`
    /// 三态承载显式关闭）。
    pub fn effective_props(&self, style_id: &str) -> Result<EffectiveFormat, StyleChainError> {
        if !self.by_id.contains_key(style_id) {
            return Err(StyleChainError::UnknownStyle {
                id: style_id.to_string(),
            });
        }
        // 有序路径上溯（派生→基底）：重复节点 = 环。
        // 与 chain() 的 HashSet 截断不同：这里需要环的完整路径来
        // 构造 typed 错误，故用 Vec 保序 + HashSet 判重。
        let mut path: Vec<String> = vec![style_id.to_string()];
        let mut on_path: HashSet<String> = HashSet::from([style_id.to_string()]);
        let mut cur = style_id.to_string();
        loop {
            let s = &self.by_id[&cur];
            match &s.based_on {
                None => break,
                // 悬空：优雅降级——可见链止于此（#267 验收 4；
                // 读入时 audit_chains 已登记 BrokenReference 警告）。
                Some(parent) if !self.by_id.contains_key(parent) => break,
                Some(parent) => {
                    if !on_path.insert(parent.clone()) {
                        // 环：cycle = 从重复节点起的完整一圈
                        //（A→B→C→B 从 B 查询 → [B, C]；自环 A→A → [A]）。
                        let pos = path
                            .iter()
                            .position(|x| x == parent)
                            .expect("on_path 成员必在 path");
                        let cycle: Vec<String> = path[pos..].to_vec();
                        return Err(StyleChainError::CycleDetected {
                            entry: style_id.to_string(),
                            cycle,
                        });
                    }
                    path.push(parent.clone());
                    cur = parent.clone();
                }
            }
        }
        // 基底→派生合并（path 是派生→基底，倒序即基底→派生）。
        let mut eff = EffectiveFormat {
            run: self.doc_defaults.run.clone(),
            para: self.doc_defaults.para.clone(),
        };
        for id in path.iter().rev() {
            let s = &self.by_id[id];
            merge_run(&mut eff.run, &s.run);
            merge_para(&mut eff.para, &s.para);
        }
        Ok(eff)
    }

    fn from_parts(order: Vec<String>, styles: Vec<StyleRef>, defaults: StyleDefaults) -> Self {
        let by_id = styles
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect::<HashMap<_, _>>();
        Self {
            order,
            by_id,
            doc_defaults: defaults,
        }
    }
}

fn merge_run(dst: &mut StyleRunProps, layer: &StyleRunProps) {
    if layer.bold.is_some() {
        dst.bold = layer.bold;
    }
    if layer.italic.is_some() {
        dst.italic = layer.italic;
    }
    if layer.strike.is_some() {
        dst.strike = layer.strike;
    }
    if layer.underline.is_some() {
        dst.underline = layer.underline.clone();
    }
    if layer.size.is_some() {
        dst.size = layer.size;
    }
    if layer.color.is_some() {
        dst.color = layer.color.clone();
    }
    if layer.fonts.is_some() {
        dst.fonts = layer.fonts.clone();
    }
}

fn merge_para(dst: &mut StyleParaProps, layer: &StyleParaProps) {
    if layer.alignment.is_some() {
        dst.alignment = layer.alignment.clone();
    }
    if layer.outline_lvl.is_some() {
        dst.outline_lvl = layer.outline_lvl;
    }
    if layer.indent.is_some() {
        dst.indent = layer.indent;
    }
}

// ---------------------------------------------------------------------------
// 主题色表：槽位映射 + shade/tint 调制
// ---------------------------------------------------------------------------

impl ThemeColors {
    fn from_slots(slots: BTreeMap<String, String>) -> Self {
        Self {
            slots,
            fallback_default: false,
        }
    }

    /// **默认 Office 调色板**（#269 降级面：theme 部件缺席/悬空/
    /// 零槽时的优雅降级）。
    ///
    /// 值 = Office 2013+「Office」主题 12 槽标准色（Word 全新文档
    /// 的出厂配色——与真实语料高频锚点一致：docx4j 语料 533 件
    /// 完整 12 槽主题中 accent1=4472C4 / hlink=0563C1 为众数）。
    /// `is_default_fallback() == true`（作者缓存 `w:val` 优先的
    /// 降级阶梯见 [`StyleBundle::effective_color`]）。
    pub fn office_default() -> Self {
        let slots: BTreeMap<String, String> = [
            ("dk1", "000000"),
            ("lt1", "FFFFFF"),
            ("dk2", "44546A"),
            ("lt2", "E7E6E6"),
            ("accent1", "4472C4"),
            ("accent2", "ED7D31"),
            ("accent3", "A5A5A5"),
            ("accent4", "FFC000"),
            ("accent5", "5B9BD5"),
            ("accent6", "70AD47"),
            ("hlink", "0563C1"),
            ("folHlink", "954F72"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Self {
            slots,
            fallback_default: true,
        }
    }

    /// 是否默认调色板降级态（true = 文档无 theme 数据，槽值是
    /// Office 出厂标准色而非文档自带主题）。
    pub fn is_default_fallback(&self) -> bool {
        self.fallback_default
    }

    /// 表大小（12 槽全载 = 12；空表 0——theme 部件缺席/无 clrScheme）。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// themeColor token → clrScheme 槽名映射（§17.18.97 语义 +
    /// text1/background1 别名族；未知 token 原样返回——lookup
    /// 未命中 = None）。
    fn map_token(token: &str) -> &str {
        match token {
            "dark1" | "text1" => "dk1",
            "light1" | "background1" => "lt1",
            "dark2" | "text2" => "dk2",
            "light2" | "background2" => "lt2",
            "hyperlink" => "hlink",
            "followedHyperlink" => "folHlink",
            other => other, // accent1-6 直名 + 未知 token 透传
        }
    }

    /// token 查槽（未命中/空表 → None）。
    pub fn lookup(&self, token: &str) -> Option<String> {
        if token == "none" {
            return None; // 显式无主题色（ST_ThemeColor 语义）
        }
        self.slots.get(Self::map_token(token)).cloned()
    }

    /// 解析样式颜色 → 最终 `RRGGBB`（大写）。
    ///
    /// - `color.theme` 在场：槽位取色（未命中/空表 → **None**，调用方
    ///   可回退 `color.val`）；shade/tint 调制（同写 **tint 赢**——
    ///   [MS-OI29500] §2.1.72(b)，见模块文档）；调制值非合法 hex
    ///   字节（>255/非 hex/空）→ 忽略修饰用原色（修饰是可选面，
    ///   主引用仍在）；
    /// - 无 `theme`：`val` 直值——合法 6 位 hex → `Some(大写)`；
    ///   `"auto"`/非法/空 → `None`（自动色/无效色语义留给渲染器）。
    pub fn resolve_color(&self, color: &StyleColor) -> Option<String> {
        let base: Option<String> = match &color.theme {
            Some(token) => self.lookup(token),
            None => {
                let v = color.val.trim();
                if v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(v.to_ascii_uppercase());
                }
                return None; // "auto" / 非法 / 空
            }
        };
        let base = base?;
        let rgb = parse_rgb(&base)?;
        let out = self.modulate(&rgb, color.tint.as_deref(), color.shade.as_deref())?;
        Some(format!("{:02X}{:02X}{:02X}", out[0], out[1], out[2]))
    }

    /// 主题色解析（#269 issue 草约 API 面：token + 修饰分离入参）。
    ///
    /// `theme_color` = themeColor token（`accent1`/`text1`/…）；
    /// `tint`/`shade` = hex 字节字符串（`"99"`/`"BF"`，None = 缺席）。
    /// 同写时 tint 赢。返回 `RRGGBB` 大写；None = token 未命中/空表
    /// （登记语义不猜值——与 `resolve_color` 同族，issue 草约的
    /// `Result<_, ThemeError>` 错误面在本层收敛为 Option：未命中
    /// 不是错误是可回退态，消费方拿 `w:val` 兜底）。
    pub fn resolve_theme_color(
        &self,
        theme_color: &str,
        tint: Option<&str>,
        shade: Option<&str>,
    ) -> Option<String> {
        let base = self.lookup(theme_color)?;
        let rgb = parse_rgb(&base)?;
        let out = self.modulate(&rgb, tint, shade)?;
        Some(format!("{:02X}{:02X}{:02X}", out[0], out[1], out[2]))
    }

    /// 槽位 RGB → tint/shade 调制（HSL luminance——模块文档 §3）。
    fn modulate(&self, rgb: &[u8; 3], tint: Option<&str>, shade: Option<&str>) -> Option<[u8; 3]> {
        let mod_byte = |raw: Option<&str>| -> Option<u8> {
            raw.and_then(|s| u8::from_str_radix(s, 16).ok())
                .filter(|v| *v != 0)
        };
        Some(if let Some(t) = mod_byte(tint) {
            // tint 赢（[MS-OI29500] §2.1.72(b)——同写时 shade 忽略）。
            tint_hsl(rgb, t)
        } else if let Some(s) = mod_byte(shade) {
            shade_hsl(rgb, s)
        } else {
            *rgb
        })
    }
}

fn parse_rgb(hex: &str) -> Option<[u8; 3]> {
    let v = hex.trim();
    if v.len() != 6 || !v.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&v[0..2], 16).ok()?;
    let g = u8::from_str_radix(&v[2..4], 16).ok()?;
    let b = u8::from_str_radix(&v[4..6], 16).ok()?;
    Some([r, g, b])
}

// ---------------------------------------------------------------------------
// HSL luminance 调制（[MS-OI29500] §2.1.72 Word 实现口径）
// ---------------------------------------------------------------------------
//
// 移植参考 = LibreOffice 互操作实现（`Color::ApplyTintOrShade` +
// `basegfx::utils::rgb2hsl/hsl2rgb`，MPL-2.0——此处移植的是数学
// 算法（不受版权约束的表达），非逐行抄写；出处登记见模块文档 §3）。
// 差异点：通道取整用 **floor**（语料实证 Word 缓存呈 floor 特征，
// LO 的 round 系统性 +1 偏离 Word——24 件 Word 产样本拟合 floor
// 14 精确/10 漂移±1，round 仅 5 精确）。

/// RGB → HSL（hue 度数 [0,360)，s/l ∈ [0,1]；f64）。
fn rgb_to_hsl(rgb: &[u8; 3]) -> (f64, f64, f64) {
    let r = f64::from(rgb[0]) / 255.0;
    let g = f64::from(rgb[1]) / 255.0;
    let b = f64::from(rgb[2]) / 255.0;
    let maxv = r.max(g).max(b);
    let minv = r.min(g).min(b);
    let d = maxv - minv;
    let l = (maxv + minv) / 2.0;
    if d == 0.0 {
        return (0.0, 0.0, l); // 无色（灰）——hue 未定义取 0
    }
    let s = if l > 0.5 {
        d / (2.0 - maxv - minv)
    } else {
        d / (maxv + minv)
    };
    let mut h = if r == maxv {
        (g - b) / d
    } else if g == maxv {
        2.0 + (b - r) / d
    } else {
        4.0 + (r - g) / d
    };
    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    }
    (h, s, l)
}

/// HSL 亮度扇区插值（n1/n2 = 中间量；hue 度数，可越界——fmod 归一）。
fn hsl_hue_to_rgb(n1: f64, n2: f64, hue: f64) -> f64 {
    let mut hue = hue % 360.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    if hue < 60.0 {
        n1 + (n2 - n1) * hue / 60.0
    } else if hue < 180.0 {
        n2
    } else if hue < 240.0 {
        n1 + (n2 - n1) * (240.0 - hue) / 60.0
    } else {
        n1
    }
}

/// HSL → RGB（逐通道 floor 取整；float→u8 饱和转换兜底越界）。
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    if s == 0.0 {
        let v = (l * 255.0).floor() as u8;
        return [v, v, v];
    }
    let n1 = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let n2 = 2.0 * l - n1;
    let f = |hue: f64| (hsl_hue_to_rgb(n2, n1, hue) * 255.0).floor() as u8;
    [f(h + 120.0), f(h), f(h - 120.0)]
}

/// shade：HSL `L' = L × S/255`（[MS-OI29500] §2.1.72(a)「Applying
/// the shade formula … to the luminance」；S=0 视为无修饰——
/// `w:themeShade="0"` 与缺席不可辨时按缺席，见 `modulate`；
/// S=255 = 恒等直传——避免 HSL 往返浮点误差）。
fn shade_hsl(rgb: &[u8; 3], s: u8) -> [u8; 3] {
    if s == 255 {
        return *rgb;
    }
    let (h, sat, l) = rgb_to_hsl(rgb);
    hsl_to_rgb(h, sat, l * f64::from(s) / 255.0)
}

/// tint：HSL `L' = L × T/255 + (1 − T/255)`（lumMod+lumOff——
/// [MS-OI29500] §2.1.72(c) HSL luminance 算法；T=255 恒等直传）。
fn tint_hsl(rgb: &[u8; 3], t: u8) -> [u8; 3] {
    if t == 255 {
        return *rgb;
    }
    let (h, sat, l) = rgb_to_hsl(rgb);
    let f = f64::from(t) / 255.0;
    hsl_to_rgb(h, sat, l * f + (1.0 - f))
}

// ---------------------------------------------------------------------------
// StyleBundle 层：链异常预检 + 有效色解析
// ---------------------------------------------------------------------------

impl StyleBundle {
    /// 样式有效颜色：`effective_format(id).run.color` → 主题解析。
    /// None = 无样式/无颜色承载/主题槽未命中（消费方可回退直值
    /// `tree.get(id).run.color.val`）。
    ///
    /// **默认调色板降级阶梯**（#269）：theme 是降级态
    /// （[`ThemeColors::is_default_fallback`]——文档无 theme 数据）
    /// 且 `w:val` 是合法 RGB 时，**作者渲染缓存优先**（val 是产出
    /// 应用对该颜色的实算缓存，比猜的 Office 出厂调色板更忠于
    /// 作者意图）；val 不可用（auto/空/非法）才走调色板解析。
    pub fn effective_color(&self, style_id: &str) -> Option<String> {
        let eff = self.tree.effective_format(style_id)?;
        let color = eff.run.color.as_ref()?;
        if self.theme.is_default_fallback() {
            let v = color.val.trim();
            if v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(v.to_ascii_uppercase());
            }
        }
        self.theme.resolve_color(color)
    }

    /// 链异常检查（读入时单趟执行——环/悬空一次性登记）。
    fn audit_chains(&mut self) {
        let ids: Vec<String> = self.tree.order.clone();
        let mut issues: Vec<(String, String)> = Vec::new(); // (style, detail)
        for id in &ids {
            // 悬空 basedOn。
            if let Some(s) = self.tree.get(id) {
                if let Some(parent) = &s.based_on {
                    if self.tree.get(parent).is_none() {
                        issues.push((
                            id.clone(),
                            format!(
                                "style '{id}' basedOn '{parent}' has no style definition (dangling)"
                            ),
                        ));
                    }
                }
            }
            // 环：独立上溯（chain 返回值已去重截断，不能用于环检测
            // ——实施发现）。visited 含起点：回到任一已访问节点 = 环。
            {
                let mut visited: HashSet<String> = HashSet::new();
                visited.insert(id.clone());
                let mut cur = self.tree.get(id).and_then(|s| s.based_on.clone());
                while let Some(p) = cur {
                    if !visited.insert(p.clone()) {
                        issues.push((
                            id.clone(),
                            format!(
                                "style '{id}' basedOn chain cycles at '{p}' (truncated at cycle)"
                            ),
                        ));
                        break;
                    }
                    cur = self.tree.get(&p).and_then(|s| s.based_on.clone());
                }
            }
        }
        for (style, detail) in issues {
            self.warnings.push(crate::io::CompatibilityWarning {
                part: "word/styles.xml".to_string(),
                kind: WarningKind::BrokenReference,
                detail: format!("{detail} (chain carried up to truncation) [style: {style}]"),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 自有解析层：styles.xml + theme1.xml（quick-xml 事件流）
// ---------------------------------------------------------------------------

/// 属性值 → 反转义 UTF-8 字符串（quick-xml 属性值保留原始实体——
/// 与 io.rs GeneralRef 透传不解析不同，本层**消费**值需展开；
/// 非法 UTF-8 → None（属性跳过，调用方决定警告面）。
fn attr_str(a: &quick_xml::events::attributes::Attribute<'_>) -> Option<String> {
    let raw = String::from_utf8_lossy(&a.value);
    let unescaped = quick_xml::escape::unescape(&raw).ok()?;
    Some(unescaped.into_owned())
}

/// 限定名字节 → local name（剥 `xxx:` 前缀——theme 的 `a:`/文档的
/// `w:` 一视同仁；无前缀原样）。
fn local_of(q: &[u8]) -> String {
    let local = match q.iter().position(|&b| b == b':') {
        Some(i) => &q[i + 1..],
        None => q,
    };
    String::from_utf8_lossy(local).into_owned()
}

/// Start/Empty 元素 local name（QName 借用需先绑定——见 E0716）。
fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
    let q = e.name();
    local_of(q.as_ref())
}

/// 属性 key → local name 比较（`w:styleId`/`w:val` 等带前缀形态与
/// 裸名同视——写出方两种形态都合法）。
fn key_is(a: &quick_xml::events::attributes::Attribute<'_>, name: &[u8]) -> bool {
    let k = a.key.as_ref();
    let local = match k.iter().position(|&b| b == b':') {
        Some(i) => &k[i + 1..],
        None => k,
    };
    local == name
}

fn find_attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    for attr in e.attributes() {
        let a = attr.ok()?;
        if key_is(&a, name) {
            return attr_str(&a);
        }
    }
    None
}

/// `w:on/off` 布尔（OOXML ST_OnOff：`1/true/on` = true，`0/false/off`
/// = false；缺省 val = true——空 `<w:b/>` 即置位）。
fn on_off(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<bool> {
    for attr in e.attributes() {
        let a = attr.ok()?;
        if key_is(&a, name) {
            let v = attr_str(&a)?.to_ascii_lowercase();
            return Some(!matches!(v.as_str(), "0" | "false" | "off"));
        }
    }
    Some(true) // 元素在场无 val：置位
}

fn absorb_run(el: &str, e: &quick_xml::events::BytesStart<'_>, p: &mut StyleRunProps) {
    match el {
        "color" => {
            p.color = Some(StyleColor {
                val: find_attr(e, b"val").unwrap_or_else(|| "auto".into()),
                theme: find_attr(e, b"themeColor"),
                shade: find_attr(e, b"themeShade"),
                tint: find_attr(e, b"themeTint"),
            });
        }
        "sz" => {
            if let Some(v) = find_attr(e, b"val") {
                p.size = v.parse::<usize>().ok();
            }
        }
        "b" => p.bold = on_off(e, b"val"),
        "i" => p.italic = on_off(e, b"val"),
        "strike" => p.strike = on_off(e, b"val"),
        "u" => {
            p.underline = Some(find_attr(e, b"val").unwrap_or_else(|| "single".into()));
        }
        "rFonts" => {
            let f = p.fonts.get_or_insert_with(StyleFont::default);
            if let Some(v) = find_attr(e, b"ascii") {
                f.ascii = Some(v);
            }
            if let Some(v) = find_attr(e, b"hAnsi") {
                f.hi_ansi = Some(v);
            }
            if let Some(v) = find_attr(e, b"eastAsia") {
                f.east_asia = Some(v);
            }
            if let Some(v) = find_attr(e, b"cs") {
                f.cs = Some(v);
            }
        }
        _ => {}
    }
}

fn absorb_para(el: &str, e: &quick_xml::events::BytesStart<'_>, p: &mut StyleParaProps) {
    match el {
        "outlineLvl" => {
            if let Some(v) = find_attr(e, b"val") {
                p.outline_lvl = v.parse::<usize>().ok();
            }
        }
        "jc" => p.alignment = find_attr(e, b"val"),
        "ind" => {
            let get = |names: &[&[u8]]| -> Option<i32> {
                for n in names {
                    if let Some(v) = find_attr(e, n) {
                        if let Ok(i) = v.parse::<i32>() {
                            return Some(i);
                        }
                    }
                }
                None
            };
            let left = get(&[b"left", b"start"]);
            let right = get(&[b"right", b"end"]);
            let first = get(&[b"firstLine"]);
            let hanging = get(&[b"hanging"]);
            let special = match (first, hanging) {
                (Some(f), _) => Some(ParagraphIndentSpecial::FirstLine(f)),
                (None, Some(h)) => Some(ParagraphIndentSpecial::Hanging(h)),
                (None, None) => None,
            };
            p.indent = Some(ParagraphIndent {
                left,
                right,
                special,
            });
        }
        _ => {}
    }
}

/// 解析 `word/styles.xml`（整表 + docDefaults）。
///
/// 部件已过门 3 良构性（配对/根唯一/深度限）——本层为**语义提取**
/// （未知元素跳过；元素缺失属性按 OOXML 缺省处理，非错误）。
fn parse_styles_part(
    data: &[u8],
    part: &str,
    strict: bool,
    warnings: &mut Vec<crate::io::CompatibilityWarning>,
) -> Result<(StyleTree, bool), io::ReadError> {
    use quick_xml::events::Event;

    let mut styles: Vec<StyleRef> = Vec::new();
    let mut defaults = StyleDefaults::default();
    // 语义错误累计（styleId 缺失等——非致命，警告可见）。
    let mut semantic: Vec<String> = Vec::new();

    // 状态：当前 style 收集 / rPr·pPr 归属（style idx / defaults）。
    let mut cur_style: Option<StyleRef> = None;
    enum PrOwner {
        None,
        Style,
        Defaults,
    }
    #[derive(PartialEq, Clone, Copy)]
    enum Phase {
        None,
        RPr,
        PPr,
    }
    let (mut r_owner, mut p_owner): (PrOwner, PrOwner) = (PrOwner::None, PrOwner::None);
    let (mut r_phase, mut p_phase): (Phase, Phase) = (Phase::None, Phase::None);

    let mut reader = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let el = local_name(&e);
                match el.as_str() {
                    "style" => {
                        let id = find_attr(&e, b"styleId");
                        let type_ = StyleType::parse(
                            &find_attr(&e, b"type").unwrap_or_else(|| "paragraph".into()),
                        );
                        let is_default = matches!(
                            find_attr(&e, b"default").as_deref(),
                            Some("1") | Some("true")
                        );
                        match id {
                            Some(id) if !id.is_empty() => {
                                cur_style = Some(StyleRef {
                                    id,
                                    type_,
                                    name: None,
                                    based_on: None,
                                    next: None,
                                    link: None,
                                    q_format: false,
                                    ui_priority: None,
                                    is_default,
                                    run: StyleRunProps::default(),
                                    para: StyleParaProps::default(),
                                });
                            }
                            _ => semantic
                                .push("w:style without non-empty @w:styleId (skipped)".to_string()),
                        }
                    }
                    "docDefaults" => {}
                    "rPrDefault" | "pPrDefault" => {}
                    "rPr" => {
                        r_owner = if cur_style.is_some() {
                            PrOwner::Style
                        } else {
                            PrOwner::Defaults
                        };
                        r_phase = Phase::RPr;
                    }
                    "pPr" => {
                        p_owner = if cur_style.is_some() {
                            PrOwner::Style
                        } else {
                            PrOwner::Defaults
                        };
                        p_phase = Phase::PPr;
                    }
                    "name" => {
                        if let Some(s) = cur_style.as_mut() {
                            s.name = find_attr(&e, b"val");
                        }
                    }
                    "basedOn" => {
                        if let Some(s) = cur_style.as_mut() {
                            s.based_on = find_attr(&e, b"val");
                        }
                    }
                    "next" => {
                        if let Some(s) = cur_style.as_mut() {
                            s.next = find_attr(&e, b"val");
                        }
                    }
                    "link" => {
                        if let Some(s) = cur_style.as_mut() {
                            s.link = find_attr(&e, b"val");
                        }
                    }
                    "uiPriority" => {
                        if let Some(s) = cur_style.as_mut() {
                            s.ui_priority = find_attr(&e, b"val").and_then(|v| v.parse().ok());
                        }
                    }
                    "qFormat" => {
                        if let Some(s) = cur_style.as_mut() {
                            s.q_format = true;
                        }
                    }
                    "color" | "sz" | "b" | "i" | "strike" | "u" | "rFonts" => {
                        let target = match r_phase {
                            Phase::RPr => match r_owner {
                                PrOwner::Style => cur_style.as_mut().map(|s| &mut s.run),
                                PrOwner::Defaults => Some(&mut defaults.run),
                                PrOwner::None => None,
                            },
                            _ => None,
                        };
                        if let Some(p) = target {
                            absorb_run(&el, &e, p);
                        }
                    }
                    "outlineLvl" | "jc" | "ind" => {
                        let target = match p_phase {
                            Phase::PPr => match p_owner {
                                PrOwner::Style => cur_style.as_mut().map(|s| &mut s.para),
                                PrOwner::Defaults => Some(&mut defaults.para),
                                PrOwner::None => None,
                            },
                            _ => None,
                        };
                        if let Some(p) = target {
                            absorb_para(&el, &e, p);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let q = e.name();
                let el = local_of(q.as_ref());
                match el.as_str() {
                    "style" => {
                        if let Some(s) = cur_style.take() {
                            styles.push(s);
                        }
                    }
                    "rPr" => {
                        r_phase = Phase::None;
                        r_owner = PrOwner::None;
                    }
                    "pPr" => {
                        p_phase = Phase::None;
                        p_owner = PrOwner::None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                let detail = format!("styles part '{part}' XML stream error");
                if strict {
                    return Err(io::ReadError::InvalidPart {
                        part: part.to_string(),
                        detail,
                    });
                }
                warnings.push(crate::io::CompatibilityWarning {
                    part: part.to_string(),
                    kind: WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (partial carry)"),
                });
                break;
            }
            Ok(_) => {}
        }
        buf.clear();
    }

    // 语义警告（两模式恒登记——非致命面可见化）。
    for detail in semantic {
        warnings.push(crate::io::CompatibilityWarning {
            part: part.to_string(),
            kind: WarningKind::MalformedXml,
            detail: format!("compat note: {detail}"),
        });
    }

    let order = styles.iter().map(|s| s.id.clone()).collect();
    let tree = StyleTree::from_parts(order, styles, defaults);
    Ok((tree, true))
}

/// clrScheme 12 槽局部名（a: 命名空间——本地名匹配，前缀无关）。
const CLR_SLOTS: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// 解析 `word/theme/theme1.xml` 的 `a:clrScheme`。
///
/// 槽色来源：`a:srgbClr@val` / `a:sysClr@lastClr`（`lastClr` 缺席 =
/// 槽不记——sysClr 的系统色查找面外，不猜值；`a:sysClr@val` 仅
/// window/windowText 两值在 ECMA 有缺省色，但写出形态几乎恒带
/// lastClr，猜值面登记不做）。无 clrScheme / theme 部件缺席 →
/// 空表（合法形态——`ThemeColors::lookup` 恒 None）。
fn parse_theme_scheme(
    data: &[u8],
    part: &str,
    strict: bool,
    warnings: &mut Vec<crate::io::CompatibilityWarning>,
) -> Result<ThemeColors, io::ReadError> {
    use quick_xml::events::Event;

    let mut slots: BTreeMap<String, String> = BTreeMap::new();
    let mut in_scheme = false;
    let mut cur_slot: Option<String> = None;

    let mut reader = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let el = local_name(&e);
                if !in_scheme {
                    if el == "clrScheme" {
                        in_scheme = true;
                    }
                } else if let Some(slot) = CLR_SLOTS.iter().find(|s| **s == el) {
                    cur_slot = Some((*slot).to_string());
                } else if el == "srgbClr" {
                    if let (Some(slot), Some(val)) = (cur_slot.clone(), find_attr(&e, b"val")) {
                        if !val.is_empty() && parse_rgb(&val).is_some() {
                            slots.insert(slot, val.to_ascii_uppercase());
                        }
                    }
                } else if el == "sysClr" {
                    if let (Some(slot), Some(last)) = (cur_slot.clone(), find_attr(&e, b"lastClr"))
                    {
                        if !last.is_empty() && parse_rgb(&last).is_some() {
                            slots.insert(slot, last.to_ascii_uppercase());
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let q = e.name();
                let el = local_of(q.as_ref());
                if el == "clrScheme" {
                    in_scheme = false;
                } else if CLR_SLOTS.contains(&el.as_str()) {
                    cur_slot = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                let detail = format!("theme part '{part}' XML stream error");
                if strict {
                    return Err(io::ReadError::InvalidPart {
                        part: part.to_string(),
                        detail,
                    });
                }
                warnings.push(crate::io::CompatibilityWarning {
                    part: part.to_string(),
                    kind: WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (slots so far kept)"),
                });
                break;
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    Ok(ThemeColors::from_slots(slots))
}

// ---------------------------------------------------------------------------
// 读入口（容器三门复用 + 自有层提取）
// ---------------------------------------------------------------------------

/// 关系类型词尾（OPC officeDocument 命名空间——styles/theme 面）。
const STYLES_REL_SUFFIX: &str = "/styles";
const THEME_REL_SUFFIX: &str = "/theme";

/// 读 DOCX 样式模板 + 主题色（文件路径入口，严格模式默认）。
pub fn read_docx_styles(path: &str) -> Result<StyleBundle, DocxStyleError> {
    read_docx_styles_with_options(path, &ReadOptions::default())
}

/// 读 DOCX 样式模板 + 主题色（带选项）。
pub fn read_docx_styles_with_options(
    path: &str,
    opts: &ReadOptions,
) -> Result<StyleBundle, DocxStyleError> {
    let meta =
        std::fs::metadata(path).map_err(|e| DocxStyleError::Io(format!("stat {path}: {e}")))?;
    if meta.len() > io::LIMIT_INPUT_BYTES_DOCX {
        return Err(DocxStyleError::Container(io::ReadError::PrescanRejected {
            reason: "input size over limit",
            entry: None,
        }));
    }
    let bytes = std::fs::read(path).map_err(|e| DocxStyleError::Io(format!("read {path}: {e}")))?;
    read_docx_styles_from_bytes_with_options(&bytes, opts)
}

/// 读 DOCX 样式模板 + 主题色（字节入口，严格模式）。
pub fn read_docx_styles_from_bytes(bytes: &[u8]) -> Result<StyleBundle, DocxStyleError> {
    read_docx_styles_from_bytes_with_options(bytes, &ReadOptions::default())
}

/// 读 DOCX 样式模板 + 主题色（字节入口，带选项）。
///
/// 门结构 = [`crate::docx::read_docx`] 同族：门 0（字节上限）→ 门 1
/// （`prescan_docx`）→ 门 2+3（`verify_gates`——**全 .xml 部件含
/// styles.xml/theme1.xml 先行良构校验**，解析层零敌意接触）→ 自有
/// OPC 关系层（`build_rels_bundle` 复用）→ 部件定位读取 → 解析。
///
/// styles/theme 关系缺席 = 无该部件（空树/空表 + 无警告——非 Word
/// 产线常态）；关系在场但 zip 无该条目 = 悬空引用（严格拒 / 兼容
/// `BrokenReference` 警告 + 空承载——与 numPr 悬空同族纪律）。
pub fn read_docx_styles_from_bytes_with_options(
    bytes: &[u8],
    opts: &ReadOptions,
) -> Result<StyleBundle, DocxStyleError> {
    use crate::docx::{build_rels_bundle, normalize_part_path, RelsBundle};
    let strict = !opts.compat;
    if bytes.len() as u64 > io::LIMIT_INPUT_BYTES_DOCX {
        return Err(DocxStyleError::Container(io::ReadError::PrescanRejected {
            reason: "input size over limit",
            entry: None,
        }));
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut za = zip::ZipArchive::new(cursor)
        .map_err(|e| DocxStyleError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
    io::prescan_docx(&mut za)?;
    let mut warnings = Vec::new();
    io::verify_gates(&mut za, strict, &mut warnings)?;

    // 关系层（document.xml.rels 的 styles/theme 关系——多关系时按
    // target 排序取第一，HashMap 迭代序不定 → 显式定序保证确定性）。
    let rels_cursor = std::io::Cursor::new(bytes);
    let mut rels_za = zip::ZipArchive::new(rels_cursor).map_err(DocxStyleError::from_container)?;
    let bundle: RelsBundle = build_rels_bundle(&mut rels_za, strict, &mut warnings)?;
    let doc_rels = &bundle.document;
    let pick_rel = |suffix: &str| -> Option<String> {
        let doc = doc_rels.as_ref()?;
        let mut targets: Vec<String> = doc
            .iter()
            .filter(|r| r.rel_type.ends_with(suffix))
            .filter(|r| r.target_mode.is_none()) // External 关系无包内部件
            .map(|r| r.target.clone())
            .collect();
        targets.sort();
        targets.into_iter().next().map(|t| {
            // rels target 相对 owner 目录（word/）——归一到包路径。
            let joined = if t.starts_with('/') {
                t.trim_start_matches('/').to_string()
            } else {
                format!("word/{t}")
            };
            normalize_part_path(&joined).unwrap_or(joined)
        })
    };
    let styles_part = pick_rel(STYLES_REL_SUFFIX);
    let theme_part = pick_rel(THEME_REL_SUFFIX);

    // 读包内部件：Ok(Some(bytes)) = 在场；Ok(None) = 条目缺席
    // （调用方按模式登记语义——闭包不碰 warnings，免多借用）。
    let read_part = |za: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
                     part: &str,
                     strict: bool|
     -> Result<Option<Vec<u8>>, DocxStyleError> {
        match za.by_name(part) {
            Ok(mut zf) => {
                let mut data = Vec::new();
                zf.read_to_end(&mut data).map_err(|e| {
                    DocxStyleError::Container(io::ReadError::ContainerCorrupt(format!(
                        "read {part}: {e}"
                    )))
                })?;
                Ok(Some(data))
            }
            Err(_) => {
                if strict {
                    return Err(DocxStyleError::Container(io::ReadError::InvalidPart {
                        part: part.to_string(),
                        detail: format!(
                            "relationship target part '{part}' missing from package (dangling)"
                        ),
                    }));
                }
                Ok(None)
            }
        }
    };
    let missing_warn = |warnings: &mut Vec<crate::io::CompatibilityWarning>, part: &str| {
        warnings.push(crate::io::CompatibilityWarning {
            part: part.to_string(),
            kind: WarningKind::BrokenReference,
            detail: format!(
                "compat mode: relationship target part '{part}' missing from package (dangling; empty carry)"
            ),
        });
    };

    let styles_tree = match &styles_part {
        Some(part) => match read_part(&mut za, part, strict)? {
            Some(data) => {
                parse_styles_part(&data, part, strict, &mut warnings)
                    .map_err(DocxStyleError::Container)?
                    .0
            }
            None => {
                missing_warn(&mut warnings, part);
                StyleTree::default()
            }
        },
        None => StyleTree::default(),
    };
    // theme 数据不可用（关系缺席 / 条目悬空 / 零槽解析）→ 默认
    // Office 调色板降级（#269 验收「缺 theme1.xml → 优雅降级（用
    // 默认调色板）」；`is_default_fallback` 标记让消费方可优先
    // `w:val` 作者缓存——见 `StyleBundle::effective_color` 阶梯）。
    // theme 部件在场且 ≥1 槽 = 真主题，原样承载（部分槽 = 诚实部分，
    // 不猜全表）。
    let theme = match &theme_part {
        Some(part) => match read_part(&mut za, part, strict)? {
            Some(data) => {
                let parsed = parse_theme_scheme(&data, part, strict, &mut warnings)
                    .map_err(DocxStyleError::Container)?;
                if parsed.is_empty() {
                    ThemeColors::office_default()
                } else {
                    parsed
                }
            }
            None => {
                missing_warn(&mut warnings, part);
                ThemeColors::office_default()
            }
        },
        None => ThemeColors::office_default(),
    };

    let mut bundle = StyleBundle {
        tree: styles_tree,
        theme,
        warnings,
    };
    bundle.audit_chains();
    Ok(bundle)
}

// ---------------------------------------------------------------------------
// 错误面（读入口独立——docx 读错误族 + InvalidPart 直通）
// ---------------------------------------------------------------------------

/// 样式读入口错误（[`crate::docx::DocxError`] 同构三分——
/// 容器/IO/解析；见 [`DocxStyleError::Docx`] 桥接）。
#[derive(Debug)]
pub enum DocxStyleError {
    Container(io::ReadError),
    Io(String),
    /// docx.rs 桥接（`DocxError::Io/Parse/InvalidInput` 文本面）。
    Docx(String),
}

impl std::fmt::Display for DocxStyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocxStyleError::Container(e) => write!(f, "docx styles read: container: {e}"),
            DocxStyleError::Io(e) => write!(f, "docx styles read: {e}"),
            DocxStyleError::Docx(e) => write!(f, "docx styles read: {e}"),
        }
    }
}

impl std::error::Error for DocxStyleError {}

impl From<io::ReadError> for DocxStyleError {
    fn from(e: io::ReadError) -> Self {
        DocxStyleError::Container(e)
    }
}

impl DocxStyleError {
    fn from_container(e: zip::result::ZipError) -> Self {
        DocxStyleError::Container(io::ReadError::ContainerCorrupt(format!("{e}")))
    }
}

impl From<crate::docx::DocxError> for DocxStyleError {
    fn from(e: crate::docx::DocxError) -> Self {
        DocxStyleError::Docx(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// 错误面二：继承链解析 typed 错误（#267——环/未知 id 显式上报）
// ---------------------------------------------------------------------------

/// 样式继承链解析错误（[`StyleTree::effective_props`] 严格面的
/// typed 错误——#267 验收 3「环检测报 typed 错误不 panic」）。
///
/// 注意边界：**悬空 basedOn 不在此列**（验收 4 优雅降级 = `Ok` +
/// [`crate::io::CompatibilityWarning`] 读入时登记）；本枚举只承载
/// 「无法给出可信合并结果」的两种情形。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleChainError {
    /// 查询的 styleId 不在样式表（容器/解析正常，id 无定义）。
    UnknownStyle { id: String },
    /// basedOn 链成环：`entry` = 查询入口 id；`cycle` = 环路径
    /// （从重复节点起的完整一圈，如 A→B→C→B 查 B → `["B", "C"]`；
    /// 自环 A→A → `["A"]`）。
    CycleDetected { entry: String, cycle: Vec<String> },
}

impl std::fmt::Display for StyleChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StyleChainError::UnknownStyle { id } => {
                write!(f, "style chain: unknown style id '{id}'")
            }
            StyleChainError::CycleDetected { entry, cycle } => {
                // 环路径闭合回重复节点（A→B→A——不是闭合回查询口：
                // entry=D 查询的环是 A→B→A，D 只是入口不在环内）。
                let first = cycle.first().map(String::as_str).unwrap_or("");
                write!(
                    f,
                    "style chain: basedOn cycle from '{entry}': {} -> {first}",
                    cycle.join(" -> ")
                )
            }
        }
    }
}

impl std::error::Error for StyleChainError {}

// ---------------------------------------------------------------------------
// 写面：StyleDef + DocxWriter 分文件 impl
// ---------------------------------------------------------------------------

/// 写侧样式定义输入（读面 [`StyleRef`] 的写侧对偶——`is_default`/
/// `ui_priority` 为读面元数据（写面无对应 OOXML 写出构造器语义面：
/// `w:default` 的样式面板 UI 位，Word 写出由模板语义决定，API 不发明）。
#[derive(Debug, Clone, PartialEq)]
pub struct StyleDef {
    /// `@w:styleId`（非空；不得与预置 `Normal`/`Heading1-9` 或已定义
    /// 样式重名——typed 拒绝）。
    pub id: String,
    pub type_: StyleType,
    /// `w:name`（None = 与 id 同值——写面产出 `w:name` 恒在场）。
    pub name: Option<String>,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub link: Option<String>,
    pub q_format: bool,
    pub run: StyleRunProps,
    pub para: StyleParaProps,
}

impl StyleDef {
    /// 便捷构造（id/type 必填，其余缺省——name=None → id 同值，
    /// q_format=true 与 Word 快速样式惯例对齐）。
    pub fn new(id: impl Into<String>, type_: StyleType) -> Self {
        Self {
            id: id.into(),
            type_,
            name: None,
            based_on: None,
            next: None,
            link: None,
            q_format: true,
            run: StyleRunProps::default(),
            para: StyleParaProps::default(),
        }
    }
}

/// 预置样式 id 集（引擎 Builder 恒发 `Normal` + 本写器 new() 预置
/// `Heading1-9`——styles.xml 写出面恒含 10 个段落预置）。
pub const PRESET_STYLE_IDS: [&str; 10] = [
    "Normal", "Heading1", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6", "Heading7",
    "Heading8", "Heading9",
];

/// #196 任务 3：预置**字符**样式 id 集（写器 new() 预注入——Word
/// 内置名：Strong = 加粗强调 / Emphasis = 斜体强调）。与段落预置
/// 分离（`styled_paragraph` 引用校验仍走 [`PRESET_STYLE_IDS`]——
/// 段落/字符类型不混用；rStyle 引用面 [`PRESET_CHAR_STYLE_IDS`]
/// ——V12 校验数据源）。
pub const PRESET_CHAR_STYLE_IDS: [&str; 2] = ["Strong", "Emphasis"];

fn is_preset(id: &str) -> bool {
    PRESET_STYLE_IDS.contains(&id)
}

/// 校验写侧 `w:themeShade`/`w:themeTint`（hex 字节——非法 typed 拒）。
fn check_shade_tint(label: &str, raw: &Option<String>) -> Result<(), String> {
    match raw.as_deref() {
        None => Ok(()),
        Some(s) => match u8::from_str_radix(s, 16) {
            Ok(_) => Ok(()),
            Err(_) => Err(format!("{label} must be a hex byte 00-FF (got {s:?})")),
        },
    }
}

impl crate::docx::DocxWriter {
    /// 定义自定义样式（styles.xml 增量——`w:style` 元素写出）。
    ///
    /// 校验（全部 typed `InvalidInput`——写面自洽，不产出悬空引用）：
    /// - id 非空 + 不与预置/已定义重名；
    /// - `type_` 已知四类（`Unknown` 拒绝——写面不透传未知类型）；
    /// - `based_on`/`next` 在场时必须已注册（预置或先定义——顺序
    ///   敏感：引用先于定义 = 拒绝，与读面容忍不对称（读容忍是别人
    ///   产的包，写自洽是我们产的包）；`link` 放松（UI 对偶提示位，
    ///   不参与格式合并——双向成对 link 是 Word 自产形态，先定义序
    ///   不可满足双向，登记）；
    /// - `run.color.theme` 在场时 token 必须是 `docx_rs::ThemeColor`
    ///   可解析值（未知 token 拒绝）；`shade`/`tint` 合法 hex 字节。
    ///
    /// 读写对偶：读面 [`StyleRef`]——本方法产出的 styles.xml 经
    /// [`read_docx_styles`] 读回应逐字段还原（`is_default` 除外——
    /// 写面无该语义位）。
    pub fn define_style(&mut self, def: StyleDef) -> Result<(), crate::docx::DocxError> {
        let id = def.id.trim();
        if id.is_empty() {
            return Err(crate::docx::DocxError::InvalidInput(
                "style id must be non-empty".into(),
            ));
        }
        if matches!(def.type_, StyleType::Unknown(_)) {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "style type must be one of paragraph/character/numbering/table (got {:?})",
                def.type_
            )));
        }
        if is_preset(id) || PRESET_CHAR_STYLE_IDS.contains(&id) {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "style id '{id}' collides with preset (Normal/Heading1-9/Strong/Emphasis)"
            )));
        }
        if self.defined_styles().contains(id) {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "style id '{id}' already defined in this writer"
            )));
        }
        // based_on/next 严格（先定义后引用——参与格式合并/回车语义，
        // 悬空即格式丢失）；link 放松（UI 对偶提示位——Word 自产成对
        // 双向 link[A.link=B 且 B.link=A]，先定义序不可满足双向，且
        // link 不参与 effective_format 合并，悬空无损格式）。
        for (label, target) in [("based_on", &def.based_on), ("next", &def.next)] {
            if let Some(t) = target {
                let known = is_preset(t) || self.defined_styles().contains(t);
                if !known {
                    return Err(crate::docx::DocxError::InvalidInput(format!(
                        "style {label} '{t}' is not registered (define it first or use a preset)"
                    )));
                }
            }
        }
        if let Some(color) = &def.run.color {
            if let Some(token) = &color.theme {
                // 引擎 FromStr 对未知 token 给 Unsupported（Ok）——写面
                // 白名单精确判定（12 命名槽 + none + 别名族 4）。
                if !matches!(
                    token.as_str(),
                    "dark1"
                        | "light1"
                        | "dark2"
                        | "light2"
                        | "accent1"
                        | "accent2"
                        | "accent3"
                        | "accent4"
                        | "accent5"
                        | "accent6"
                        | "hyperlink"
                        | "followedHyperlink"
                        | "none"
                        | "text1"
                        | "background1"
                        | "text2"
                        | "background2"
                ) {
                    return Err(crate::docx::DocxError::InvalidInput(format!(
                        "color theme token {token:?} is not a valid ST_ThemeColor"
                    )));
                }
            }
            check_shade_tint("color shade", &color.shade)
                .map_err(crate::docx::DocxError::InvalidInput)?;
            check_shade_tint("color tint", &color.tint)
                .map_err(crate::docx::DocxError::InvalidInput)?;
            let v = color.val.as_str();
            if v != "auto" && !(v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit())) {
                return Err(crate::docx::DocxError::InvalidInput(format!(
                    "color val must be RRGGBB hex or \"auto\" (got {v:?})"
                )));
            }
        }

        let engine_type = match def.type_ {
            StyleType::Paragraph => docx_rs::StyleType::Paragraph,
            StyleType::Character => docx_rs::StyleType::Character,
            StyleType::Numbering => docx_rs::StyleType::Numbering,
            StyleType::Table => docx_rs::StyleType::Table,
            StyleType::Unknown(_) => unreachable!("checked above"),
        };
        let mut style = docx_rs::Style::new(id, engine_type)
            .name(def.name.clone().unwrap_or_else(|| id.to_string()))
            .q_format(def.q_format);
        if let Some(b) = &def.based_on {
            style = style.based_on(b.clone());
        }
        if let Some(n) = &def.next {
            style = style.next(n.clone());
        }
        if let Some(l) = &def.link {
            style = style.link(l.clone());
        }
        // rPr
        let mut rp = docx_rs::RunProperty::new();
        match def.run.bold {
            Some(true) => rp = rp.bold(),
            Some(false) => rp = rp.disable_bold(),
            None => {}
        }
        match def.run.italic {
            Some(true) => rp = rp.italic(),
            Some(false) => rp = rp.disable_italic(),
            None => {}
        }
        match def.run.strike {
            Some(true) => rp = rp.strike(),
            Some(false) => rp = rp.disable_strike(),
            None => {}
        }
        if let Some(u) = &def.run.underline {
            rp = rp.underline(u.clone());
        }
        if let Some(sz) = def.run.size {
            rp = rp.size(sz);
        }
        if let Some(c) = &def.run.color {
            rp = rp.color(c.val.clone());
            if let Some(token) = &c.theme {
                // parse::<ThemeColor> 对未知 token 给 Unsupported（Ok）——
                // 上方已把未知 token 拒掉，这里恒 Ok；text1 等别名 token
                // 引擎 FromStr 认识（Background1/Text1 变体在列）。
                if let Ok(tc) = token.parse::<docx_rs::ThemeColor>() {
                    rp = rp.theme_color(tc);
                }
            }
            if let Some(s) = &c.shade {
                rp = rp.theme_shade(s.clone());
            }
            if let Some(t) = &c.tint {
                rp = rp.theme_tint(t.clone());
            }
        }
        if let Some(f) = &def.run.fonts {
            let mut ef = docx_rs::RunFonts::new();
            if let Some(v) = &f.ascii {
                ef = ef.ascii(v.clone());
            }
            if let Some(v) = &f.hi_ansi {
                ef = ef.hi_ansi(v.clone());
            }
            if let Some(v) = &f.east_asia {
                ef = ef.east_asia(v.clone());
            }
            if let Some(v) = &f.cs {
                ef = ef.cs(v.clone());
            }
            rp = rp.fonts(ef);
        }
        style.run_property = rp;
        // pPr
        let mut pp = docx_rs::ParagraphProperty::new();
        if let Some(lvl) = def.para.outline_lvl {
            pp = pp.outline_lvl(lvl);
        }
        if let Some(ind) = &def.para.indent {
            let special = ind.special.map(|s| match s {
                ParagraphIndentSpecial::FirstLine(v) => docx_rs::SpecialIndentType::FirstLine(v),
                ParagraphIndentSpecial::Hanging(v) => docx_rs::SpecialIndentType::Hanging(v),
            });
            pp = pp.indent(ind.left, special, ind.right, None);
        }
        if let Some(jc) = &def.para.alignment {
            pp.alignment = Some(docx_rs::Justification::new(jc.clone()));
        }
        style.paragraph_property = pp;

        let docx = std::mem::take(self.docx_mut());
        *self.docx_mut() = docx.add_style(style);
        self.defined_styles_mut().insert(id.to_string());
        Ok(())
    }

    /// 引用样式的富文本段（`w:pPr/w:pStyle`——样式引用 + run 序，
    /// run 级格式全位同 [`crate::docx::DocxWriter::paragraph]）。
    ///
    /// `style_id` 必须已注册（预置或 [`Self::define_style`]）——悬空
    /// `pStyle` typed 拒绝（写面自洽；读面容忍对侧同前述不对称纪律）。
    pub fn styled_paragraph(
        &mut self,
        runs: &[crate::docx::RunSpec],
        style_id: &str,
    ) -> Result<(), crate::docx::DocxError> {
        let known = is_preset(style_id) || self.defined_styles().contains(style_id);
        if !known {
            return Err(crate::docx::DocxError::InvalidInput(format!(
                "style '{style_id}' is not registered (define_style first or use a preset)"
            )));
        }
        let mut p = crate::docx::specs_to_paragraph(runs);
        p = p.style(style_id);
        let docx = std::mem::take(self.docx_mut());
        *self.docx_mut() = docx.add_paragraph(p);
        self.count_specs(1, runs);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测试（内嵌——私有解析器可达 + 写读全环）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docx::DocxWriter;

    /// 手搓最小 OPC 包（门 1-3 全绿形态）：styles/theme 部件可参数化。
    fn build_test_docx(styles_xml: &str, theme_xml: &str) -> Vec<u8> {
        use std::io::Write as _;
        let ct = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>"#;
        let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let doc = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>t</w:t></w:r></w:p></w:body></w:document>"#;
        let doc_rels: &[u8] = if theme_xml.is_empty() {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
        } else {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/></Relationships>"#
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let add = |zw: &mut zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
                       name: &str,
                       data: &[u8]|
             -> zip::result::ZipResult<()> {
                zw.start_file(name, zip::write::SimpleFileOptions::default())?;
                zw.write_all(data)?;
                Ok(())
            };
            add(&mut zw, "[Content_Types].xml", ct).unwrap();
            add(&mut zw, "_rels/.rels", root_rels).unwrap();
            add(&mut zw, "word/document.xml", doc).unwrap();
            add(&mut zw, "word/_rels/document.xml.rels", doc_rels).unwrap();
            add(&mut zw, "word/styles.xml", styles_xml.as_bytes()).unwrap();
            if !theme_xml.is_empty() {
                add(&mut zw, "word/theme/theme1.xml", theme_xml.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        buf.into_inner()
    }

    // ---- 主题色数学（[MS-OI29500] §2.1.72 Word HSL luminance 口径） ----

    #[test]
    fn theme_official_example_tint_exact() {
        // [MS-OI29500] §2.1.72(c) 官方算例：accent2=4F81BD，tint
        // 99(hex)=60% → 95B3D7（微软官方文档值，本层精确命中）。
        let slots = BTreeMap::from([("accent2".to_string(), "4F81BD".to_string())]);
        let theme = ThemeColors::from_slots(slots);
        let c = StyleColor {
            val: String::new(),
            theme: Some("accent2".into()),
            shade: None,
            tint: Some("99".into()),
        };
        assert_eq!(theme.resolve_color(&c), Some("95B3D7".to_string()));
    }

    #[test]
    fn theme_official_example_shade_documented_drift() {
        // [MS-OI29500] §2.1.72(a) 官方算例：accent2=C0504D，shade
        // BF(hex)=75% → 官方值 943634；本层 HSL floor 出 943734
        // （G 通道 +1）。登记：Word 内部定点细节不可复现（同输入
        // 跨 Word 版本缓存本身就有 365F91/376092/366091 三种值），
        // 容差 ±1 详见 `theme_word_corpus_ground_truth`。
        let slots = BTreeMap::from([("accent2".to_string(), "C0504D".to_string())]);
        let theme = ThemeColors::from_slots(slots);
        let c = StyleColor {
            val: String::new(),
            theme: Some("accent2".into()),
            shade: Some("BF".into()),
            tint: None,
        };
        let got = theme.resolve_color(&c).unwrap();
        let expect = "943634";
        let diff = got
            .chars()
            .zip(expect.chars())
            .step_by(2)
            .map(|(g, e)| {
                (u8::from_str_radix(&g.to_string(), 16).unwrap() as i32
                    - u8::from_str_radix(&e.to_string(), 16).unwrap() as i32)
                    .abs()
            })
            .max()
            .unwrap();
        assert_eq!(got, "943734"); // 本层确定性输出（钉住防漂）
        assert!(
            diff <= 1,
            "official example drift over ±1: {got} vs {expect}"
        );
    }

    #[test]
    fn theme_shade_gray_and_black_tints() {
        // 灰基（A5A5A5）shade BF → 7B7B7B（Word16 实库钉定）；
        // 黑基（text1/dk1=000000）tint 族——向白插值，Word16 实库钉定。
        let slots = BTreeMap::from([
            ("accent3".to_string(), "A5A5A5".to_string()),
            ("dk1".to_string(), "000000".to_string()),
        ]);
        let theme = ThemeColors::from_slots(slots);
        let resolve = |token: &str, tint: Option<&str>, shade: Option<&str>| {
            theme.resolve_theme_color(token, tint, shade).unwrap()
        };
        assert_eq!(resolve("accent3", None, Some("BF")), "7B7B7B");
        assert_eq!(resolve("text1", Some("BF"), None), "404040");
        assert_eq!(resolve("text1", Some("A6"), None), "595959");
        assert_eq!(resolve("text1", Some("D8"), None), "272727");
    }

    /// Word 实证样本行（基色, tint, shade, Word 缓存 RRGGBB, 精确?, 来源）。
    type TruthRow<'a> = (
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        &'a str,
        bool,
        &'a str,
    );

    #[test]
    fn theme_word_corpus_ground_truth() {
        // Word 产文档实测缓存（w:val）ground truth 拟合（#269 实证轮，
        // 2026-11-06，仓内语料 + [MS-OI29500] 官方算例）：
        //   精确命中 12；其余 10 件全部在每通道 ≤1 内。
        // 容差理由（非放宽断言——基线本身多版本分裂）：同一输入
        // 4F81BD+shade BF 在不同 Word 版本产物中出现 365F91（Word12/
        // 模板静态值）、376092（FieldCodes）、366091（本层）三种缓存，
        // 位级一致性在 Word 生态内不存在；≤1 = 确定性算法输出对任意
        // Word 版本渲染的最近距离。
        //
        // (基色, tint, shade, Word 缓存 RRGGBB, exact?, 来源)
        let rows: &[TruthRow] = &[
            // ---- 精确命中组（Word 渲染与本层位级一致）----
            (
                "4472C4",
                None,
                Some("BF"),
                "2F5496",
                true,
                "docx4j/2010_DrawingML (Word16)",
            ),
            (
                "156082",
                None,
                Some("BF"),
                "0F4761",
                true,
                "docx4j/OpenDoPE_RptPosCon (Word16)",
            ),
            (
                "4F81BD",
                Some("99"),
                None,
                "95B3D7",
                true,
                "[MS-OI29500] §2.1.72(c) 官方算例",
            ),
            (
                "1F497D",
                None,
                Some("BF"),
                "17365D",
                true,
                "corpusR/D2a-nonbody (MacWord) dk2",
            ),
            (
                "1F497D",
                Some("99"),
                None,
                "548DD4",
                true,
                "poi-docx/SampleDoc (Word) dk2",
            ),
            (
                "C0504D",
                None,
                Some("CC"),
                "9E3A38",
                true,
                "corpusR/D2a-nonbody (MacWord) accent2",
            ),
            (
                "9BBB59",
                None,
                Some("BF"),
                "76923C",
                true,
                "corpusR/D2a-nonbody (MacWord) accent3",
            ),
            (
                "8064A2",
                None,
                Some("BF"),
                "5F497A",
                true,
                "corpusR/D2a-nonbody (MacWord) accent4",
            ),
            (
                "4BACC6",
                None,
                Some("BF"),
                "31849B",
                true,
                "corpusR/D2a-nonbody (MacWord) accent5",
            ),
            (
                "9BBB59",
                None,
                Some("CC"),
                "7E9C40",
                true,
                "corpusR/D2a-nonbody (MacWord) accent3",
            ),
            (
                "4BACC6",
                None,
                Some("CC"),
                "348DA5",
                true,
                "corpusR/D2a-nonbody (MacWord) accent5",
            ),
            (
                "A5A5A5",
                None,
                Some("BF"),
                "7B7B7B",
                true,
                "docx4j/2010_DrawingML (Word16) accent3",
            ),
            // ---- 版本漂移组（每通道 ≤1——Word 跨版本缓存分裂）----
            (
                "4F81BD",
                None,
                Some("BF"),
                "365F91",
                false,
                "corpusR/D2a (MacWord) + hyperlink_dupe (Word12) accent1",
            ),
            (
                "4F81BD",
                None,
                Some("7F"),
                "243F60",
                false,
                "corpusR/D2a (MacWord) accent1",
            ),
            (
                "C0504D",
                None,
                Some("BF"),
                "943634",
                false,
                "[MS-OI29500] §2.1.72(a) 官方算例 accent2",
            ),
            (
                "F79646",
                None,
                Some("BF"),
                "E36C0A",
                false,
                "corpusR/D2a (MacWord) accent6",
            ),
            (
                "8064A2",
                None,
                Some("CC"),
                "664E82",
                false,
                "corpusR/D2a (MacWord) accent4",
            ),
            (
                "F79646",
                None,
                Some("CC"),
                "F2730A",
                false,
                "corpusR/D2a (MacWord) accent6",
            ),
            (
                "4F81BD",
                None,
                Some("B5"),
                "345A8A",
                false,
                "poi-docx/58067 (MacWord) accent1",
            ),
            (
                "5B9BD5",
                None,
                Some("BF"),
                "2E74B5",
                false,
                "poi-docx/Bug64561 (Word) accent1",
            ),
            (
                "5B9BD5",
                None,
                Some("7F"),
                "1F4D78",
                false,
                "poi-docx/Bug64561 (Word) accent1",
            ),
            (
                "4F81BD",
                None,
                Some("BF"),
                "376092",
                false,
                "poi-docx/FieldCodes (Word) accent1",
            ),
        ];
        let mut exact_n = 0;
        let mut drift_n = 0;
        for (base, tint, shade, expect, is_exact, src) in rows {
            let slots = BTreeMap::from([("accent1".to_string(), base.to_string())]);
            let theme = ThemeColors::from_slots(slots);
            let got = theme
                .resolve_theme_color("accent1", *tint, *shade)
                .unwrap_or_else(|| panic!("resolve failed for base {base}"));
            let ch = |s: &str, i: usize| i32::from(u8::from_str_radix(&s[i..i + 2], 16).unwrap());
            let (dr, dg, db) = (
                (ch(&got, 0) - ch(expect, 0)).abs(),
                (ch(&got, 2) - ch(expect, 2)).abs(),
                (ch(&got, 4) - ch(expect, 4)).abs(),
            );
            if *is_exact {
                assert_eq!(
                    got, *expect,
                    "exact row: base {base} tint={tint:?} shade={shade:?} [{src}]"
                );
                exact_n += 1;
            } else {
                assert!(
                    dr <= 1 && dg <= 1 && db <= 1,
                    "drift over ±1: base {base} tint={tint:?} shade={shade:?} → {got} vs {expect} (d={dr},{dg},{db}) [{src}]"
                );
                drift_n += 1;
            }
        }
        // 防跑空：两组样本量下限（语料换血时若不足 → 钉定失效，须重钉）。
        assert!(exact_n >= 10, "exact sample floor: {exact_n}");
        assert!(drift_n >= 8, "drift sample floor: {drift_n}");
    }

    #[test]
    fn theme_tint_wins_over_shade() {
        // [MS-OI29500] §2.1.72(b)：「Word applies the themeTint setting
        // when both of these attributes are present」——同写 tint 赢
        // （旧实现「shade 赢」无出处，#269 修正）。
        let slots = BTreeMap::from([("accent1".to_string(), "FF0000".to_string())]);
        let theme = ThemeColors::from_slots(slots);
        let c = StyleColor {
            val: String::new(),
            theme: Some("accent1".into()),
            shade: Some("80".into()),
            tint: Some("80".into()),
        };
        // 走 tint：L=0.5 → 0.5×128/255+127/255≈0.5 → R=FF（扇区顶），
        // G/B 走 n1 插值 ≈127 → FF7F7F（非 shade 形 800000）。
        assert_eq!(theme.resolve_color(&c), Some("FF7F7F".to_string()));
        // 分离验证：仅 shade → 800000（HSL hue=0 红：L×128/255）。
        let only_shade = StyleColor {
            val: String::new(),
            theme: Some("accent1".into()),
            shade: Some("80".into()),
            tint: None,
        };
        assert_eq!(theme.resolve_color(&only_shade), Some("800000".to_string()));
    }

    #[test]
    fn theme_modifier_boundaries() {
        // S/T=FF（255）恒等直传（避免 HSL 往返浮点误差——4472C4 不变）；
        // S/T=00 与缺席不可辨 → 无修饰（mod_byte 过滤 0）。
        let slots = BTreeMap::from([("accent1".to_string(), "4472C4".to_string())]);
        let theme = ThemeColors::from_slots(slots);
        assert_eq!(
            theme.resolve_theme_color("accent1", Some("FF"), None),
            Some("4472C4".to_string())
        );
        assert_eq!(
            theme.resolve_theme_color("accent1", None, Some("FF")),
            Some("4472C4".to_string())
        );
        assert_eq!(
            theme.resolve_theme_color("accent1", Some("00"), None),
            Some("4472C4".to_string())
        );
        assert_eq!(
            theme.resolve_theme_color("accent1", None, Some("00")),
            Some("4472C4".to_string())
        );
        // 非法修饰（>FF / 非 hex）忽略 → 原色。
        assert_eq!(
            theme.resolve_theme_color("accent1", None, Some("1GG")),
            Some("4472C4".to_string())
        );
    }

    #[test]
    fn theme_lookup_mapping_and_fallbacks() {
        let slots = BTreeMap::from([
            ("dk1".to_string(), "000000".to_string()),
            ("lt1".to_string(), "FFFFFF".to_string()),
            ("accent1".to_string(), "4472C4".to_string()),
            ("hlink".to_string(), "0563C1".to_string()),
        ]);
        let theme = ThemeColors::from_slots(slots);
        // 语义别名族。
        assert_eq!(theme.lookup("dark1"), Some("000000".into()));
        assert_eq!(theme.lookup("text1"), Some("000000".into()));
        assert_eq!(theme.lookup("light1"), Some("FFFFFF".into()));
        assert_eq!(theme.lookup("background1"), Some("FFFFFF".into()));
        // 直名 + 未配槽。
        assert_eq!(theme.lookup("accent1"), Some("4472C4".into()));
        assert_eq!(theme.lookup("hyperlink"), Some("0563C1".into()));
        assert_eq!(theme.lookup("followedHyperlink"), None); // folHlink 未配
        assert_eq!(theme.lookup("none"), None);
        assert_eq!(theme.lookup("notAToken"), None);
        // val 直值回退（无 theme）：合法 hex 大写化；auto/非法 → None。
        let direct = StyleColor {
            val: "2e74b5".into(),
            theme: None,
            shade: None,
            tint: None,
        };
        assert_eq!(theme.resolve_color(&direct), Some("2E74B5".to_string()));
        let auto = StyleColor {
            val: "auto".into(),
            theme: None,
            shade: None,
            tint: None,
        };
        assert_eq!(theme.resolve_color(&auto), None);
        // 主题槽未命中（theme 在场但表空该槽）→ None（消费方回退 val）。
        let miss = StyleColor {
            val: "123456".into(),
            theme: Some("accent9".into()), // 无此槽
            shade: None,
            tint: None,
        };
        assert_eq!(theme.resolve_color(&miss), None);
        // 非法修饰（>FF / 非 hex）忽略 → 原色。
        let bad_mod = StyleColor {
            val: String::new(),
            theme: Some("accent1".into()),
            shade: Some("1GG".into()),
            tint: None,
        };
        // 非法修饰忽略 → 原色（accent1 槽 4472C4）。
        assert_eq!(theme.resolve_color(&bad_mod), Some("4472C4".into()));
    }

    // ---- theme1.xml 解析 ----

    #[test]
    fn parse_theme_scheme_full() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<a:theme xmlns:a="drawingml"><a:clrScheme name="Office">
<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="44546A"/></a:dk2>
<a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
<a:accent1><a:srgbClr val="4472C4"/></a:accent1>
<a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3>
<a:accent4><a:srgbClr val="FFC000"/></a:accent4>
<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5>
<a:accent6><a:srgbClr val="70AD47"/></a:accent6>
<a:hlink><a:srgbClr val="0563C1"/></a:hlink>
<a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
</a:clrScheme></a:theme>"#;
        let mut w = Vec::new();
        let theme = parse_theme_scheme(&xml[..], "word/theme/theme1.xml", true, &mut w).unwrap();
        assert_eq!(theme.len(), 12);
        assert_eq!(theme.lookup("dark1"), Some("000000".into()));
        assert_eq!(theme.lookup("light1"), Some("FFFFFF".into()));
        assert_eq!(theme.lookup("accent1"), Some("4472C4".into()));
        assert_eq!(theme.lookup("hyperlink"), Some("0563C1".into()));
        assert_eq!(theme.lookup("followedHyperlink"), Some("954F72".into()));
        assert!(w.is_empty());
    }

    #[test]
    fn parse_theme_scheme_empty_slots_kept_out() {
        // 空 val / 无 lastClr / 无 clrScheme → 槽不记（不猜值）。
        let xml = br#"<a:theme xmlns:a="x"><a:clrScheme>
<a:dk1><a:sysClr val="windowText"/></a:dk1>
<a:lt1><a:srgbClr val=""/></a:lt1>
<a:accent1><a:srgbClr val="4472C4"/></a:accent1>
</a:clrScheme></a:theme>"#;
        let mut w = Vec::new();
        let theme = parse_theme_scheme(&xml[..], "p", true, &mut w).unwrap();
        assert_eq!(theme.len(), 1); // 仅 accent1
        assert_eq!(theme.lookup("dark1"), None);
    }

    // ---- styles.xml 解析（链/三态/docDefaults/别名） ----

    const CHAIN_XML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:color w:val="auto"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr/></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:styleId="Base"><w:name w:val="Base"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Mid"><w:name w:val="Mid"/><w:basedOn w:val="Base"/><w:link w:val="MidChar"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="1"/><w:ind left="720" firstLine="360"/><w:jc w:val="both"/></w:pPr><w:rPr><w:b/><w:color w:val="2E74B5" w:themeColor="accent1" w:themeShade="99"/></w:rPr></w:style>
<w:style w:type="character" w:styleId="MidChar"><w:name w:val="Mid Char"/><w:basedOn w:val="Normal"/><w:rPr><w:b w:val="0"/><w:i/><w:sz w:val="28"/><w:u w:val="single"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Top"><w:name w:val="Top"/><w:basedOn w:val="Mid"/><w:next w:val="Normal"/><w:rPr><w:color w:val="auto"/><w:rFonts w:ascii="Aptos" w:hAnsi="Aptos" w:cs="Aptos"/><w:strike w:val="0"/></w:rPr></w:style>
</w:styles>"#;

    fn chain_bundle() -> StyleBundle {
        let bytes = build_test_docx(std::str::from_utf8(CHAIN_XML).unwrap(), "");
        read_docx_styles_from_bytes(&bytes).unwrap() // 零警告（见下）
    }

    #[test]
    fn parse_styles_chain_and_fields() {
        let b = chain_bundle();
        let ids = b.tree.style_ids();
        assert_eq!(ids, ["Base", "Normal", "Mid", "MidChar", "Top"]);
        // Base→Missing 悬空在 CHAIN_XML 里（Missing 无定义）——上面断言
        // 警告为空失败的话说明 audit 误报；悬空路径单独测（见下）。
        let mid = b.tree.get("Mid").unwrap();
        assert_eq!(mid.based_on.as_deref(), Some("Base"));
        assert_eq!(mid.link.as_deref(), Some("MidChar"));
        assert_eq!(mid.ui_priority, Some(9));
        assert!(mid.q_format);
        assert_eq!(mid.para.outline_lvl, Some(1));
        assert_eq!(
            mid.para.indent,
            Some(ParagraphIndent {
                left: Some(720),
                right: None,
                special: Some(ParagraphIndentSpecial::FirstLine(360)),
            })
        );
        assert_eq!(mid.para.alignment.as_deref(), Some("both"));
        assert_eq!(mid.run.bold, Some(true));
        assert_eq!(
            mid.run.color,
            Some(StyleColor {
                val: "2E74B5".into(),
                theme: Some("accent1".into()),
                shade: Some("99".into()),
                tint: None,
            })
        );
        let mc = b.tree.get("MidChar").unwrap();
        // w:b val="0" 显式关闭 + 斜体置位 + 字号 28 半点 + 下画线。
        assert_eq!(mc.run.bold, Some(false));
        assert_eq!(mc.run.italic, Some(true));
        assert_eq!(mc.run.size, Some(28));
        assert_eq!(mc.run.underline.as_deref(), Some("single"));
        let top = b.tree.get("Top").unwrap();
        assert_eq!(
            top.run.fonts,
            Some(StyleFont {
                ascii: Some("Aptos".into()),
                hi_ansi: Some("Aptos".into()),
                east_asia: None,
                cs: Some("Aptos".into()),
            })
        );
        assert_eq!(top.run.strike, Some(false));
        // w:default="1" 承载（Normal）。
        assert!(b.tree.get("Normal").unwrap().is_default);
        assert!(!mid.is_default);
        // next 承载（引擎读面缺失项）。
        assert_eq!(
            b.tree.get("Normal").unwrap().next.as_deref(),
            Some("Normal")
        );
        // docDefaults 承载。
        assert_eq!(b.tree.doc_defaults.run.size, Some(22));
    }

    #[test]
    fn effective_format_layering() {
        let b = chain_bundle();
        // Top（链 Top→Mid→Base[→Missing 截断]）+ docDefaults：
        // bold：Mid Some(true)；Top 未设 → true。
        // strike：Top Some(false) 显式关闭。
        // color：Mid themeColor accent1/shade99；Top color=auto 覆盖 → Some(auto)。
        // size：docDefaults 22 铺底（无样式级）→ 22。
        // outlineLvl：Mid 1；Top 未设 → 1。
        // ind：Mid（720/firstLine 360）继承。
        let eff = b.tree.effective_format("Top").unwrap();
        assert_eq!(eff.run.bold, Some(true));
        assert_eq!(eff.run.strike, Some(false));
        assert_eq!(
            eff.run.color.map(|c| c.val),
            Some("auto".to_string()) // Top 的 w:color val=auto 整元素覆盖
        );
        assert_eq!(eff.run.size, Some(22)); // docDefaults 铺底
        assert_eq!(eff.para.outline_lvl, Some(1));
        assert_eq!(eff.para.indent.map(|i| i.left), Some(Some(720)));
        // MidChar 字符样式链（MidChar→Normal）。
        let mce = b.tree.effective_format("MidChar").unwrap();
        assert_eq!(mce.run.size, Some(28)); // MidChar 自身
        assert_eq!(mce.run.italic, Some(true));
        assert_eq!(mce.run.bold, Some(false)); // 显式关闭
                                               // 未知 id → None；无样式文档 id → None。
        assert!(b.tree.effective_format("Nope").is_none());
    }

    #[test]
    fn chain_cycle_and_dangling() {
        // 环 A→B→A + 悬空 C→X：警告登记 + chain 截断。
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/><w:basedOn w:val="B"/></w:style>
<w:style w:type="paragraph" w:styleId="B"><w:name w:val="B"/><w:basedOn w:val="A"/></w:style>
<w:style w:type="paragraph" w:styleId="C"><w:name w:val="C"/><w:basedOn w:val="X"/></w:style>
</w:styles>"#;
        let bytes = build_test_docx(std::str::from_utf8(xml).unwrap(), "");
        let b = read_docx_styles_from_bytes(&bytes).unwrap();
        // 两条警告：A 的环 + C 的悬空。
        let cycle = b
            .warnings
            .iter()
            .filter(|w| w.detail.contains("cycles"))
            .count();
        let dangling = b
            .warnings
            .iter()
            .filter(|w| w.detail.contains("dangling"))
            .count();
        assert_eq!(cycle, 2, "A and B each report the cycle: {:?}", b.warnings);
        assert_eq!(dangling, 1, "C reports dangling: {:?}", b.warnings);
        // chain 恒有限。环上方向任意截断：A 的 basedOn=B → 基底
        // 呈 B、派生呈 A（chain("B") 对称给 [A, B]——两视角各自
        // 自洽，消费方合并有效格式结果相同[字段级覆盖序一致]）。
        let chain = b.tree.chain("A").unwrap();
        let ids: Vec<String> = chain.iter().map(|s| s.id.clone()).collect();
        assert_eq!(ids, ["B", "A"]);
        // C 链 = [C]（X 不可见截断）。
        assert_eq!(b.tree.chain("C").unwrap().len(), 1);
    }

    /// #267：typed 严格面三态——环 → Err(CycleDetected)、悬空 →
    /// 优雅降级 Ok、未知 id → Err(UnknownStyle)；与宽松面共存。
    #[test]
    fn effective_props_typed_errors_and_degradation() {
        // 环 A→B→A + 悬空 C→X + 各层属性（验证 Err 优先于合并——
        // 环成员的属性不参与任何合并结果）。
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/><w:basedOn w:val="B"/>
  <w:rPr><w:sz w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="B"><w:name w:val="B"/><w:basedOn w:val="A"/>
  <w:rPr><w:sz w:val="30"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="C"><w:name w:val="C"/><w:basedOn w:val="X"/>
  <w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Root"><w:name w:val="Root"/>
  <w:rPr><w:sz w:val="22"/></w:rPr>
  <w:pPr><w:ind w:left="720"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Leaf"><w:name w:val="Leaf"/><w:basedOn w:val="Root"/></w:style>
</w:styles>"#;
        let bytes = build_test_docx(std::str::from_utf8(xml).unwrap(), "");
        let b = read_docx_styles_from_bytes(&bytes).unwrap();

        // ① 环：A→B→A，查询 A → typed 错误（cycle = 完整一圈
        //    [A, B]，entry = 查询口）。两视角各自报错（B 同理）。
        assert_eq!(
            b.tree.effective_props("A"),
            Err(StyleChainError::CycleDetected {
                entry: "A".into(),
                cycle: vec!["A".into(), "B".into()],
            })
        );
        assert_eq!(
            b.tree.effective_props("B"),
            Err(StyleChainError::CycleDetected {
                entry: "B".into(),
                cycle: vec!["B".into(), "A".into()],
            })
        );

        // ② 悬空：C→X（X 无定义）→ 优雅降级 Ok——以可见链 [C]
        //    合并（C 自身 b=Some(true)，父属性不参与）。
        let c = b.tree.effective_props("C").unwrap();
        assert_eq!(c.run.bold, Some(true));
        assert_eq!(c.run.size, None, "悬空父层不贡献任何属性");

        // ③ 未知 id → Err(UnknownStyle)（宽松面 None 的 typed 对偶）。
        assert_eq!(
            b.tree.effective_props("Nope"),
            Err(StyleChainError::UnknownStyle { id: "Nope".into() })
        );

        // ④ 正常链：Leaf→Root → 沿链累积（Root 的 sz=22 +
        //    ind left=720 对 Leaf 可见——子未设即继承）。
        let leaf = b.tree.effective_props("Leaf").unwrap();
        assert_eq!(leaf.run.size, Some(22));
        assert_eq!(leaf.para.indent.map(|i| i.left), Some(Some(720)));

        // ⑤ 宽松面不变（零回归）：环仍 Some（截断合并），未知仍 None。
        assert!(b.tree.effective_format("A").is_some());
        assert!(b.tree.effective_format("Nope").is_none());
    }

    /// #267：环路径形状——自环 + 长环 + 环外入口（entry 不在环内的
    /// 边界情形：D→A→B→A，entry=D、cycle=[A,B]）。
    #[test]
    fn effective_props_cycle_path_shapes() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="Self"><w:name w:val="S"/><w:basedOn w:val="Self"/></w:style>
<w:style w:type="paragraph" w:styleId="D"><w:name w:val="D"/><w:basedOn w:val="A"/></w:style>
<w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/><w:basedOn w:val="B"/></w:style>
<w:style w:type="paragraph" w:styleId="B"><w:name w:val="B"/><w:basedOn w:val="A"/></w:style>
</w:styles>"#;
        let bytes = build_test_docx(std::str::from_utf8(xml).unwrap(), "");
        let b = read_docx_styles_from_bytes(&bytes).unwrap();

        // 自环 Self→Self：cycle = [Self]。
        assert_eq!(
            b.tree.effective_props("Self"),
            Err(StyleChainError::CycleDetected {
                entry: "Self".into(),
                cycle: vec!["Self".into()],
            })
        );
        // 环外入口 D→A→B→A：entry=D 不在环内，cycle 从重复节点
        // A 起 = [A, B]（诊断信息能区分「谁查询」与「环在哪」）。
        assert_eq!(
            b.tree.effective_props("D"),
            Err(StyleChainError::CycleDetected {
                entry: "D".into(),
                cycle: vec!["A".into(), "B".into()],
            })
        );
        // 环上任意成员查询同样报错（A 视角：cycle=[A,B]）。
        assert_eq!(
            b.tree.effective_props("A"),
            Err(StyleChainError::CycleDetected {
                entry: "A".into(),
                cycle: vec!["A".into(), "B".into()],
            })
        );
        // Display 面：人读诊断可操作（含入口与完整环路径）。
        let msg = b.tree.effective_props("D").unwrap_err().to_string();
        assert!(msg.contains("D"), "{msg}");
        assert!(msg.contains("A -> B -> A"), "{msg}");
    }

    /// #267：两面一致性——干净链上 effective_props ≡ effective_format
    ///（严格面 Ok 值 == 宽松面 Some 值；分歧行为只在异常面）。
    #[test]
    fn effective_props_matches_lenient_face() {
        let b = chain_bundle();
        for id in ["Top", "Mid", "Base", "MidChar"] {
            let strict = b
                .tree
                .effective_props(id)
                .expect("clean chain must resolve");
            let lenient = b
                .tree
                .effective_format(id)
                .expect("clean chain must resolve");
            assert_eq!(strict, lenient, "faces diverge on clean chain for {id}");
        }
    }

    #[test]
    fn theme_resolution_end_to_end() {
        // styles.xml 的 themeColor accent1/shade 99 + theme1.xml 的槽
        // → effective_color 走通全链。
        let theme_xml = br#"<?xml version="1.0"?>
<a:theme xmlns:a="x"><a:clrScheme>
<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
<a:accent1><a:srgbClr val="5B9BD5"/></a:accent1>
</a:clrScheme></a:theme>"#;
        let styles_xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="H1x"><w:name w:val="h1"/><w:basedOn w:val="Normal"/>
<w:rPr><w:color w:val="2E74B5" w:themeColor="accent1" w:themeShade="99"/></w:rPr></w:style>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
</w:styles>"#;
        let bytes = build_test_docx(
            std::str::from_utf8(styles_xml).unwrap(),
            std::str::from_utf8(theme_xml).unwrap(),
        );
        let b = read_docx_styles_from_bytes(&bytes).unwrap();
        // accent1=5B9BD5，shade 0x99（HSL luminance ×153/255）
        // → 255D91（#269 Word HSL 口径——旧线性值 365D7F 作废）。
        assert_eq!(b.effective_color("H1x"), Some("255D91".to_string()));
    }

    // ---- 写面（define_style / styled_paragraph） ----

    fn demo_defs() -> Vec<StyleDef> {
        vec![
            StyleDef {
                name: Some("Base Style".into()),
                run: StyleRunProps {
                    size: Some(24),
                    color: Some(StyleColor {
                        val: "auto".into(),
                        theme: Some("accent1".into()),
                        shade: Some("99".into()),
                        tint: None,
                    }),
                    ..Default::default()
                },
                ..StyleDef::new("MyBase", StyleType::Paragraph)
            },
            StyleDef {
                based_on: Some("MyBase".into()),
                next: Some("Normal".into()),
                run: StyleRunProps {
                    bold: Some(true),
                    italic: Some(false),
                    ..Default::default()
                },
                para: StyleParaProps {
                    outline_lvl: Some(2),
                    indent: Some(ParagraphIndent {
                        left: Some(720),
                        right: None,
                        special: Some(ParagraphIndentSpecial::Hanging(360)),
                    }),
                    alignment: Some("both".into()),
                },
                ..StyleDef::new("MyDerived", StyleType::Paragraph)
            },
            StyleDef {
                type_: StyleType::Character,
                link: Some("MyDerived".into()),
                run: StyleRunProps {
                    underline: Some("single".into()),
                    ..Default::default()
                },
                ..StyleDef::new("MyDerivedChar", StyleType::Character)
            },
        ]
    }

    #[test]
    fn write_read_roundtrip_full() {
        let mut w = DocxWriter::new();
        for d in demo_defs() {
            w.define_style(d).unwrap();
        }
        w.styled_paragraph(&[crate::docx::RunSpec::text("styled")], "MyDerived")
            .unwrap();
        w.heading(1, "Preset heading is fine").unwrap();
        let bytes = w.save_to_vec().unwrap();
        // 读回：链 + effective + 主题（引擎写面无 theme 部件——主题
        // 解析为空表，themeColor 承载在位）。
        let b = read_docx_styles_from_bytes(&bytes).unwrap();
        // 全表序：Normal（Builder 恒发）→ Heading1-9（new() 预置）→
        // Strong/Emphasis（#196 任务 3 预置字符样式）→ 自定义三连。
        assert_eq!(
            b.tree.style_ids(),
            [
                "Normal",
                "Heading1",
                "Heading2",
                "Heading3",
                "Heading4",
                "Heading5",
                "Heading6",
                "Heading7",
                "Heading8",
                "Heading9",
                "Strong",
                "Emphasis",
                "MyBase",
                "MyDerived",
                "MyDerivedChar"
            ]
        );
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        let d = b.tree.get("MyDerived").unwrap();
        assert_eq!(d.based_on.as_deref(), Some("MyBase"));
        assert_eq!(d.next.as_deref(), Some("Normal"));
        assert_eq!(d.link, None);
        assert_eq!(d.run.bold, Some(true));
        assert_eq!(d.run.italic, Some(false)); // 显式关闭回放
        assert_eq!(d.para.outline_lvl, Some(2));
        assert_eq!(
            d.para.indent,
            Some(ParagraphIndent {
                left: Some(720),
                // 引擎 ind 写面怪癖：end=None → w:right="0"
                // （indent.rs:63 unwrap_or_default——WRITE_FACE_PROBE 同族
                // 登记；0 与缺失语义等价[零缩进]，读回 Some(0)）。
                right: Some(0),
                special: Some(ParagraphIndentSpecial::Hanging(360)),
            })
        );
        assert_eq!(d.para.alignment.as_deref(), Some("both"));
        let base = b.tree.get("MyBase").unwrap();
        assert_eq!(
            base.run.color,
            Some(StyleColor {
                val: "auto".into(),
                theme: Some("accent1".into()),
                shade: Some("99".into()),
                tint: None,
            })
        );
        // effective：MyDerived 链（MyDerived→MyBase）+ 引擎 docDefaults
        // （写面 docx_rs Docx::new 的 docDefaults 为空——size 不铺底）。
        let eff = b.tree.effective_format("MyDerived").unwrap();
        assert_eq!(eff.run.bold, Some(true));
        assert_eq!(eff.run.size, Some(24)); // MyBase 层
        assert_eq!(eff.para.outline_lvl, Some(2));
        // 引擎写面无 theme 部件 → #269 默认调色板降级态：val=auto
        // 不可用 → 调色板解析（accent1=4472C4 + shade 99 →
        // HSL L×153/255 → 254378）。
        assert!(b.theme.is_default_fallback());
        assert_eq!(b.effective_color("MyBase"), Some("254378".to_string()));
        assert_eq!(
            b.tree
                .get("MyBase")
                .unwrap()
                .run
                .color
                .as_ref()
                .unwrap()
                .val,
            "auto"
        );
    }

    #[test]
    fn office_default_palette_fallback_ladder() {
        // #269 验收 5：缺 theme1.xml → 优雅降级（默认调色板）。
        // ① 无 theme 关系 → office_default（12 槽 + fallback 标记）。
        let mut w = DocxWriter::new();
        w.define_style(StyleDef {
            run: StyleRunProps {
                color: Some(StyleColor {
                    val: "auto".into(),
                    theme: Some("accent5".into()),
                    tint: Some("99".into()),
                    shade: None,
                }),
                ..Default::default()
            },
            ..StyleDef::new("PalA", StyleType::Paragraph)
        })
        .unwrap();
        // 同一文档再来一个带合法 val 缓存的（阶梯：作者缓存优先）。
        w.define_style(StyleDef {
            run: StyleRunProps {
                color: Some(StyleColor {
                    val: "123456".into(),
                    theme: Some("accent1".into()),
                    shade: Some("BF".into()),
                    tint: None,
                }),
                ..Default::default()
            },
            ..StyleDef::new("PalB", StyleType::Paragraph)
        })
        .unwrap();
        let bytes = w.save_to_vec().unwrap();
        let b = read_docx_styles_from_bytes(&bytes).unwrap();
        assert!(b.theme.is_default_fallback());
        assert_eq!(b.theme.len(), 12);
        assert_eq!(b.theme.lookup("accent1"), Some("4472C4".to_string()));
        assert_eq!(b.theme.lookup("hyperlink"), Some("0563C1".to_string()));
        // ② val=auto → 调色板解析：accent5=5B9BD5 tint 99 → 9CC2E5
        //    （官方算例 4F81BD tint 99→95B3D7 同数学——95B3D7 在
        //    `theme_official_example_tint_exact` 精确钉定）。
        let got = b.effective_color("PalA").unwrap();
        assert_eq!(got, "9CC2E5");
        // ③ val=123456 合法 → 阶梯第一级（作者缓存优先于猜的调色板）。
        assert_eq!(b.effective_color("PalB"), Some("123456".to_string()));
    }

    #[test]
    fn write_face_validation_matrix() {
        let mut w = DocxWriter::new();
        // 空 id。
        let e = w
            .define_style(StyleDef::new("  ", StyleType::Paragraph))
            .unwrap_err();
        assert!(matches!(e, crate::docx::DocxError::InvalidInput(_)));
        // 预置撞名。
        let e = w
            .define_style(StyleDef::new("Heading1", StyleType::Paragraph))
            .unwrap_err();
        assert!(e.to_string().contains("preset"));
        // 未知 type。
        let e = w
            .define_style(StyleDef {
                type_: StyleType::Unknown("weird".into()),
                ..StyleDef::new("S", StyleType::Paragraph)
            })
            .unwrap_err();
        assert!(e
            .to_string()
            .contains("paragraph/character/numbering/table"));
        // 悬空 based_on（未定义）。
        let e = w
            .define_style(StyleDef {
                based_on: Some("NotYet".into()),
                ..StyleDef::new("S1", StyleType::Paragraph)
            })
            .unwrap_err();
        assert!(e.to_string().contains("not registered"));
        // 合法定义后：同 id 重名拒绝 + 合法引用通过。
        w.define_style(StyleDef::new("S1", StyleType::Paragraph))
            .unwrap();
        let e = w
            .define_style(StyleDef::new("S1", StyleType::Paragraph))
            .unwrap_err();
        assert!(e.to_string().contains("already defined"));
        w.styled_paragraph(&[crate::docx::RunSpec::text("x")], "S1")
            .unwrap();
        // 悬空 pStyle 引用拒绝。
        let e = w
            .styled_paragraph(&[crate::docx::RunSpec::text("x")], "Ghost")
            .unwrap_err();
        assert!(e.to_string().contains("not registered"));
        // 预置引用合法（Heading1/Normal）。
        w.styled_paragraph(&[crate::docx::RunSpec::text("x")], "Normal")
            .unwrap();
        w.styled_paragraph(&[crate::docx::RunSpec::text("x")], "Heading3")
            .unwrap();
        // 非法 theme token / shade。
        let e = w
            .define_style(StyleDef {
                run: StyleRunProps {
                    color: Some(StyleColor {
                        val: "123456".into(),
                        theme: Some("accent9".into()), // 非 ST_ThemeColor
                        shade: None,
                        tint: None,
                    }),
                    ..Default::default()
                },
                ..StyleDef::new("BadTheme", StyleType::Paragraph)
            })
            .unwrap_err();
        assert!(e.to_string().contains("ST_ThemeColor"));
        let e = w
            .define_style(StyleDef {
                run: StyleRunProps {
                    color: Some(StyleColor {
                        val: "123456".into(),
                        theme: Some("accent1".into()),
                        shade: Some("1GG".into()),
                        tint: None,
                    }),
                    ..Default::default()
                },
                ..StyleDef::new("BadShade", StyleType::Paragraph)
            })
            .unwrap_err();
        assert!(e.to_string().contains("hex byte"));
    }

    #[test]
    fn styled_paragraph_xml_face() {
        // pStyle 写面 XML 断言（styles.xml + document.xml 双面）。
        // #196 任务 3：样式名从 Emphasis 改 MyPara——Emphasis 已成为
        // 预置字符样式名（define_style 重名拒），本测试意图是自定义
        // 段落样式面，不依赖具体名。
        let mut w = DocxWriter::new();
        w.define_style(StyleDef {
            based_on: Some("Normal".into()),
            run: StyleRunProps {
                color: Some(StyleColor {
                    val: "auto".into(),
                    theme: Some("accent1".into()),
                    shade: Some("99".into()),
                    tint: None,
                }),
                ..Default::default()
            },
            ..StyleDef::new("MyPara", StyleType::Paragraph)
        })
        .unwrap();
        w.styled_paragraph(&[crate::docx::RunSpec::text("hi")], "MyPara")
            .unwrap();
        let bytes = w.save_to_vec().unwrap();
        let mut za = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut doc = String::new();
        za.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut doc)
            .unwrap();
        assert!(doc.contains("<w:pStyle w:val=\"MyPara\" />"), "{doc}");
        let mut styles = String::new();
        za.by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut styles)
            .unwrap();
        assert!(
            styles.contains("<w:style w:type=\"paragraph\" w:styleId=\"MyPara\">"),
            "{styles}"
        );
        assert!(styles.contains("<w:basedOn w:val=\"Normal\" />"));
        assert!(
            styles.contains(
                "<w:color w:val=\"auto\" w:themeColor=\"accent1\" w:themeShade=\"99\" />"
            ),
            "{styles}"
        );
    }
}
