//! S2-1（PDF-f）· 页面组织——lopdf 页树手术（feature `engine-lopdf`）。
//!
//! # 定位与范围
//!
//! issue #103 任务 1：把 PDF 从「能读」变成「能组织」——增删/旋转/重排/
//! 合并/拆分，全部经 **lopdf 对象图手术**（不动 pdfium——阅读引擎与
//! 结构编辑分治，PDF-c/PDF-d 分工延续）。API 形状对齐 issue 草图；
//! 错误面复用 [`crate::command::CommandError`]（crate 统一流通货币；
//! issue 草图的 `PdfError` 即此）。
//!
//! # 外科手术式方法论（docx T1/S1 验证过的三纪律）
//!
//! 1. **不动目标外结构**：只编辑页树 /Kids、祖先 /Count、被删页关联的
//!    书签/链接/表单域引用——内容流、资源字典、字体、其余对象**零触碰**
//!    （引用原样保留，对象号不重排）。唯一的全局性动作是**可达性剪除**
//!    （trailer 可达集之外的对象移除——被删页的孤儿内容流/独占资源靠它
//!    收走，保证往返不膨胀）。
//! 2. **保真**：页删除后幸存页的文本逐页与源一致（测试断言）；书签/
//!    链接的页引用在删除后**重映射或显式丢弃**（绝不留悬空引用——那是
//!    Acrobat 都会崩面的脏结构）。
//! 3. **显式登记丢失**：[`OrganizeReport`] 承载全部降级面计数——丢弃的
//!    书签（含子树展开数）/失效链接/随页表单域/合并冲突命名目的地/外观
//!    资源弃用/继承物化页数/剪除对象数。调用方读报告即知代价，无静默丢。
//!
//! # 坑位清单（issue 预警 × 实现对策）
//!
//! | 坑 | 对策 |
//! |---|---|
//! | 页码引用（书签/链接里的页码） | 删除集已知 → 书签树逐项解析 /Dest 三形态（数组/命名/GoTo 动作），目标页 ∈ 删除集 → 整子树丢弃 + 计数（Acrobat 语义）；幸存页链接目标 ∈ 删除集 → 移除该 /Link + 计数 |
//! | 表单域 | 被删页 /Annots 里的 /Widget 经 /T（本体）或 /Parent 链对应 /AcroForm /Fields 条目 → 同步移除 + 计数（不留悬空域） |
//! | 页面继承属性 | /Resources /MediaBox /CropBox /Rotate 可挂 /Pages 中间节点（spec 7.7.3.3 表 30）。重排/抽取/合并触发**继承物化**：祖先链有效值以引用拷贝（对象本体零复制）落到页字典，物化键数登记 |
//! | 交叉引用表 | lopdf 保存时全量重建（经典 xref 表）——产物经 qpdf --check 结构验证 + pdfium 重开验证（测试面） |
//! | 对象流（xref stream） | lopdf 解析面支持；保存面回归经典 xref 表（首轮保存可能解包增体积——`first_save_growth_pct` 登记；10 次往返稳定性另行断言） |
//! | 线性化 | 编辑后线性化本就失效；线性化字典不可达随剪除移除（`linearized_dropped` 登记） |
//! | 加密文档 | `from_bytes` 双形态拒（仍加密/曾加密）——S5 加密档零结构写，与 [`crate::writer_lopdf::LopdfWriter`] 同防线同理由码 |
//! | 嵌套页树的重排 | 跨继承组换父必改语义 → 物化 + 扁平化到根 /Pages。单父组（含平树）直接 /Kids 重排零物化 |
//! | 合并的对象号冲突 | 源对象图整体重编号（基底 max_id 之后顺序新号）注入——零冲突零重排基底对象 |
//!
//! # 页号口径
//!
//! 0 基 [`usize`]（对齐 issue 草图）；越界 → [`CommandError::InvalidPage`]。
//! lopdf `get_pages()` 的 1 基 BTreeMap 仅在边界换算处出现（单点）。
//!
//! # 不做什么（如实登记，不硬造）
//!
//! - **内容流编辑**（页内绘制指令增删）——超出「组织」边界；标注面归
//!   PDF-d 增量写；
//! - **多段修订**（incremental update）——组织操作是结构性重写，产物
//!   全量保存（前缀保持承诺是标注面的语义锁，不适用于此）；
//! - **加密档任何形式的结构操作**（含「先解密再组织」）——口令不可得，
//!   拒绝且不静默降级；
//! - **双方都带名称树的命名目的地合并**（排序插入的树重构）——v0 基底
//!   赢 + `dropped_named_dests` 计数登记（单侧存在时完整过继零损失）；
//! - **签名档**：结构重写后增量签名必然失效——本模块不验签（pdfium 侧
//!   另议），调用方对签名档慎用（登记为已知边界，不阻断）。

use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::command::CommandError;
use crate::page::PageIndex;
use crate::security::{Limits, OpenAssessment, REASON_ENCRYPTED_WRITE};

// ---------------------------------------------------------------------------
// 公开类型
// ---------------------------------------------------------------------------

/// 页面尺寸（PDF 用户空间，pt = 1/72 inch）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    /// 宽（pt）。必须为有限正值。
    pub width_pt: f64,
    /// 高（pt）。必须为有限正值。
    pub height_pt: f64,
}

impl PageSize {
    pub const A4: Self = Self {
        width_pt: 595.0,
        height_pt: 842.0,
    };
    pub const LETTER: Self = Self {
        width_pt: 612.0,
        height_pt: 792.0,
    };
    pub const A4_LANDSCAPE: Self = Self {
        width_pt: 842.0,
        height_pt: 595.0,
    };
    pub const LETTER_LANDSCAPE: Self = Self {
        width_pt: 792.0,
        height_pt: 612.0,
    };

    /// 构造即校验（有限正值——NaN/Inf/零/负拒绝，不产出非法 /MediaBox）。
    pub fn new(width_pt: f64, height_pt: f64) -> Result<Self, CommandError> {
        if width_pt.is_finite() && height_pt.is_finite() && width_pt > 0.0 && height_pt > 0.0 {
            Ok(Self {
                width_pt,
                height_pt,
            })
        } else {
            Err(CommandError::engine("页尺寸非法：须为有限正值（pt）"))
        }
    }
}

/// 组织操作报告——**显式登记丢失面**（外科手术式方法论的核心出口：
/// 调用方据此向用户呈现代价，无静默丢）。计数器跨操作累加（会话期全量）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrganizeReport {
    // --- 结构操作计数（成功应用的结构变化） ---
    /// 删除的页数（去重后）。
    pub deleted_pages: usize,
    /// 插入的空白页数。
    pub inserted_pages: usize,
    /// 旋转的页数。
    pub rotated_pages: usize,
    /// 重排是否发生（单父组 /Kids 重排或物化扁平化）。
    pub reordered: bool,
    /// 合并进来的文档数（不含首文档）。
    pub merged_docs: usize,
    /// 合并携入的书签项数（追加/过继——增益面计数）。
    pub merged_bookmarks: usize,

    // --- 显式登记的丢失/降级面 ---
    /// 丢弃的书签数（目标页被删——含整子树展开计数；Acrobat 语义）。
    pub dropped_bookmarks: usize,
    /// 移除的失效链接数（幸存页上指向已删页的 /Link——不留悬空目标）。
    pub dropped_links: usize,
    /// 随页删除的表单域数（/AcroForm /Fields 同步移除）。
    pub dropped_form_fields: usize,
    /// 合并丢弃的命名目的地数（基底赢策略/名称树不重构）。
    pub dropped_named_dests: usize,
    /// 合并弃用的表单外观资源数（/DR //DA 冲突基底赢——源域外观可能
    /// 渲染回退，计数面登记）。
    pub dropped_form_appearances: usize,
    /// 继承物化的页数（嵌套页树物化扁平化触发；引用拷贝非对象复制）。
    pub materialized_pages: usize,
    /// 可达性剪除的对象数（孤儿面 + 死中间节点 + 失效线性化字典）。
    pub pruned_objects: usize,
    /// 线性化字典是否随剪除移除（编辑后线性化本就失效——登记非损失）。
    pub linearized_dropped: bool,
    /// 首次保存的结构重排增长率（对象流解包/经典 xref 回归；百分数，
    /// 负值 = 缩小）。首轮产物的体积预期管理位。
    pub first_save_growth_pct: f64,
}

/// PDF 页面组织器（lopdf 对象图手术台）。
///
/// 生命周期：`from_bytes` 装载（安全镜像 [`crate::writer_lopdf::LopdfWriter`]
/// 打开面：S3-L7 输入限 → 解析 → S5 加密拒 → S2/S6 扫描 → S3-L4 预算）
/// → 结构操作（任意组合，全部立即生效于内存态）→ [`PdfOrganizer::
/// save_to_vec`] 全量序列化（xref 重建）。
#[derive(Debug, Clone)]
pub struct PdfOrganizer {
    doc: Document,
    /// 打开评估（S2 危险族告警 + S6 表单探针——产品面出口 [`Self::assessment`]）。
    assessment: OpenAssessment,
    /// 输入是否带线性化字典（首次剪除后置位 linearized_dropped 的前提）。
    had_linearized: bool,
    /// 输入原始长度（first_save_growth_pct 的分母基线）。
    input_len: u64,
    report: OrganizeReport,
}

impl PdfOrganizer {
    // -----------------------------------------------------------------------
    // 打开面（安全镜像 writer_lopdf：S3-L7 → 解析 → S5 → S2/S6 → S3-L4）
    // -----------------------------------------------------------------------

    /// 从 PDF 字节构造（限额 = [`Limits::default`]；自定义限额用
    /// [`PdfOrganizer::from_bytes_with_limits`]）。
    pub fn from_bytes(pdf: Vec<u8>) -> Result<Self, CommandError> {
        Self::from_bytes_with_limits(pdf, Limits::default())
    }

    /// 带限额构造。失败面：输入超限/预算超限 → `SecurityDeny`；解析
    /// 失败 → `Engine`；加密档（仍/曾）→ `SecurityDeny`（S5）；零页
    /// 文档 → `Engine`（spec 7.7.4：页树至少一页）。
    pub fn from_bytes_with_limits(pdf: Vec<u8>, limits: Limits) -> Result<Self, CommandError> {
        limits.check_open_input_bytes(pdf.len() as u64)?;
        let started = crate::security::portable_now();
        let doc = Document::load_mem(&pdf)
            .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
        if doc.is_encrypted() || doc.was_encrypted() {
            // S5：加密档零结构写（结构重写对加密档意味着整档重加密语义，
            // 口令不可得时一律拒绝——与 LopdfWriter 同理由码）。
            return Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE,
            });
        }
        let assessment = crate::security_scan::scan_document(&doc, &limits);
        limits.check_open_budget(started)?;
        let had_linearized = doc
            .objects
            .values()
            .any(|o| matches!(o, Object::Dictionary(d) if d.get(b"Linearized").is_ok()));
        let input_len = pdf.len() as u64;
        let org = Self {
            doc,
            assessment,
            had_linearized,
            input_len,
            report: OrganizeReport::default(),
        };
        if org.page_ids().is_empty() {
            return Err(CommandError::engine(
                "零页文档（页树为空——spec 7.7.4 拒绝）",
            ));
        }
        Ok(org)
    }

    /// 打开评估（喂 `PdfSession::open` 的 assessment 参数——PDF-e 接线面）。
    pub fn assessment(&self) -> &OpenAssessment {
        &self.assessment
    }

    /// 操作报告（丢失面显式登记——跨操作累加）。
    pub fn report(&self) -> &OrganizeReport {
        &self.report
    }

    /// 当前页数。
    pub fn page_count(&self) -> usize {
        self.page_ids().len()
    }

    // -----------------------------------------------------------------------
    // 增删
    // -----------------------------------------------------------------------

    /// 删除页（0 基；重复索引去重；空切片 = no-op）。删除全部页拒绝
    /// （PDF 至少一页——spec 7.7.4）。
    ///
    /// 联动手术：①父链 /Kids 摘除 + /Count 递减；②目标页被删的书签整
    /// 子树丢弃（Acrobat 语义）；③幸存页指向被删页的 /Link 移除；
    /// ④被删页 /Annots 的 /Widget 对应 /AcroForm /Fields 条目移除；
    /// ⑤可达性剪除（孤儿内容流/独占资源/死节点收走）。
    pub fn delete_pages(&mut self, pages: &[usize]) -> Result<(), CommandError> {
        let n = self.page_count();
        let mut set: Vec<usize> = Vec::new();
        for &p in pages {
            if p >= n {
                return Err(CommandError::InvalidPage(PageIndex(p as u32)));
            }
            if !set.contains(&p) {
                set.push(p);
            }
        }
        if set.is_empty() {
            return Ok(());
        }
        if set.len() == n {
            return Err(CommandError::engine(
                "不能删除全部页（PDF 页树至少一页——spec 7.7.4）",
            ));
        }
        set.sort_unstable();
        let page_ids = self.page_ids();
        let dead: HashSet<ObjectId> = set.iter().map(|&i| page_ids[i]).collect();

        // ② 书签（先于剪除——书签引用是被删页对象的另一可达路径）。
        self.report.dropped_bookmarks += drop_dead_bookmarks(&mut self.doc, &dead);
        // ③ 失效链接（幸存页 /Annots 内——目标页死则链接种死）。
        self.report.dropped_links += remove_dead_links(&mut self.doc, &dead);
        // ④ 表单域（被删页 widget 的域条目同步摘除）。
        self.report.dropped_form_fields += remove_dead_form_fields(&mut self.doc, &dead);

        // ① 页树摘除（降序删除防 Kids 索引漂移）+ 祖先 /Count 递减。
        for &i in set.iter().rev() {
            detach_page_from_tree(&mut self.doc, page_ids[i])?;
        }

        // ⑤ 可达性剪除。
        let pruned = prune_unreachable(&mut self.doc);
        self.report.deleted_pages += set.len();
        self.register_prune(pruned);
        Ok(())
    }

    /// 插入空白页（`at` ∈ 0..=n：0 = 最前，n = 末尾追加，否则插在现 at
    /// 位页之前）。新页显式携带 /MediaBox 与空 /Resources（不依赖继承
    /// ——尺寸由调用方钦定）；无 /Contents（空白页合法形态，spec 7.7.3）。
    pub fn insert_blank_page(&mut self, at: usize, size: PageSize) -> Result<(), CommandError> {
        let n = self.page_count();
        if at > n {
            return Err(CommandError::InvalidPage(PageIndex(at as u32)));
        }
        let page_ids = self.page_ids();
        // 宿位父节点：插在现 at 位页的父下（其 Kids 内 at 页之前）；末尾
        // 追加则挂最后页的父。n == 0 在 from_bytes 已拒（防御分支保留）。
        let (parent_id, pos_in_kids) = if n == 0 {
            (root_pages_id(&self.doc)?, 0)
        } else if at < n {
            let sibling = page_ids[at];
            let parent = parent_of(&self.doc, sibling)?;
            (parent, kids_index_of(&self.doc, parent, sibling)?)
        } else {
            let last = page_ids[n - 1];
            let parent = parent_of(&self.doc, last)?;
            let pos = kids_index_of(&self.doc, parent, last)?
                .checked_add(1)
                .ok_or_else(|| CommandError::engine("Kids 索引溢出（不可达）"))?;
            (parent, pos)
        };

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(parent_id));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(size.width_pt as f32),
                Object::Real(size.height_pt as f32),
            ]),
        );
        page.set("Resources", Object::Dictionary(Dictionary::new()));
        let page_id = self.doc.add_object(Object::Dictionary(page));

        insert_into_kids(&mut self.doc, parent_id, pos_in_kids, page_id)?;
        bump_count_chain(&mut self.doc, parent_id, 1)?;

        self.report.inserted_pages += 1;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 旋转
    // -----------------------------------------------------------------------

    /// 旋转页（顺时针；`degrees` ∈ {90, 180, 270}）。
    ///
    /// 语义：与页**现有效角度**叠加（`(cur + deg) % 360`）——与 Acrobat
    /// 「再转 90°」一致，非绝对设置。现值查找走继承链（/Rotate 可挂
    /// 祖先 /Pages 节点）；结果**显式落在页字典**（Acrobat 同款——页级
    /// 覆盖优先，不动祖先共享值，兄弟页零波及）。
    pub fn rotate_page(&mut self, idx: usize, degrees: u16) -> Result<(), CommandError> {
        if !matches!(degrees, 90 | 180 | 270) {
            return Err(CommandError::engine(format!(
                "旋转角非法：{degrees}（合法值 90/180/270——顺时针）"
            )));
        }
        let page_id = self.page_id_at(idx)?;
        let cur = inherited_integer(&self.doc, page_id, b"Rotate").unwrap_or(0);
        let next = (cur + i64::from(degrees)).rem_euclid(360);
        let page = self
            .doc
            .get_object_mut(page_id)
            .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
            .as_dict_mut()
            .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
        page.set("Rotate", Object::Integer(next));
        self.report.rotated_pages += 1;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 重排
    // -----------------------------------------------------------------------

    /// 重排页（`new_order[new_idx] = old_idx`——新序列第 i 位放原第
    /// new_order[i] 页）。必须为 0..n 的**恰排列**（长度相等/无重复/
    /// 无越界），否则 `Engine` 拒绝（部分重排语义不明，宁拒不猜）。
    ///
    /// 两种路径：①所有页共享同一父节点（含平树）→ 直接 /Kids 重排，
    /// 零结构损伤；②嵌套页树（多继承组）→ 继承物化 + 扁平化到根
    /// （跨组换父必改继承语义，物化是唯一保真路径——祖先键以**引用
    /// 拷贝**落到页字典，对象本体零复制），物化页数登记。
    pub fn reorder(&mut self, new_order: &[usize]) -> Result<(), CommandError> {
        let page_ids = self.page_ids();
        let n = page_ids.len();
        if new_order.len() != n {
            return Err(CommandError::engine(format!(
                "重排长度不匹配：给出 {} 页，文档 {n} 页",
                new_order.len()
            )));
        }
        let mut seen = vec![false; n];
        for &old in new_order {
            if old >= n || seen[old] {
                return Err(CommandError::engine(
                    "重排序列非恰排列（越界或重复——必须为 0..n 的置换）",
                ));
            }
            seen[old] = true;
        }
        let ordered: Vec<ObjectId> = new_order.iter().map(|&i| page_ids[i]).collect();

        let root = root_pages_id(&self.doc)?;
        let single_group = page_ids.iter().all(|&p| {
            parent_of(&self.doc, p)
                .map(|par| par == root)
                .unwrap_or(false)
        });

        if single_group {
            // ① 单父组：/Kids 原位重排（页对象零触碰）。
            let kids = kids_array_mut(&mut self.doc, root)?;
            kids.clear();
            for id in ordered {
                kids.push(Object::Reference(id));
            }
        } else {
            // ② 物化 + 扁平化：每页先物化继承键（引用拷贝），再改挂根。
            let mut materialized = 0usize;
            for &id in &page_ids {
                materialized += materialize_inheritance(&mut self.doc, id)?;
                let page = self
                    .doc
                    .get_object_mut(id)
                    .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
                    .as_dict_mut()
                    .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
                page.set("Parent", Object::Reference(root));
            }
            let kids = kids_array_mut(&mut self.doc, root)?;
            kids.clear();
            for id in ordered {
                kids.push(Object::Reference(id));
            }
            set_count(&mut self.doc, root, n as i64)?;
            self.report.materialized_pages += materialized;
        }
        self.report.reordered = true;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 拆分/合并
    // -----------------------------------------------------------------------

    /// 提取页区间为独立 PDF（半开区间 `range`：`start..end`，0 基）。
    ///
    /// 实现 = 自身快照 + 删补集（复用 [`PdfOrganizer::delete_pages`] 全部
    /// 联动手术：区间外书签/链接/表单域同步处理 + 剪除）——单一代码
    /// 路径，删除面的全部语义保证在抽取面同样成立。空区间拒绝。
    pub fn extract(&self, range: std::ops::Range<usize>) -> Result<Vec<u8>, CommandError> {
        let n = self.page_count();
        if range.start >= range.end || range.end > n {
            return Err(CommandError::engine(format!(
                "抽取区间非法：{}..{}（文档 {n} 页，半开区间须非空且不越界）",
                range.start, range.end
            )));
        }
        let mut part = self.clone();
        let complement: Vec<usize> = (0..n).filter(|i| !range.contains(i)).collect();
        part.delete_pages(&complement)?;
        part.save_to_vec()
    }

    /// 每 `n` 页拆为一块（`n >= 1`；末块可为零头）。返回块字节序列，
    /// 块内页序 = 文档序。
    pub fn split_every(&self, n: usize) -> Result<Vec<Vec<u8>>, CommandError> {
        if n == 0 {
            return Err(CommandError::engine("split_every(0)：块大小须 >= 1"));
        }
        let total = self.page_count();
        let mut out = Vec::with_capacity(total.div_ceil(n));
        let mut start = 0usize;
        while start < total {
            let end = (start + n).min(total);
            out.push(self.extract(start..end)?);
            start = end;
        }
        Ok(out)
    }

    /// 合并多个 PDF（`docs[0]` 为基底——catalog 元信息/Info/ID 保留；
    /// 后续文档的页**追加**到末尾，携带各自书签/表单域/命名目的地）。
    ///
    /// 对每个源文档：安全镜像装载（加密源 → 整个 merge 拒绝——混合
    /// 产物的加密语义不可定义）→ 对象图整体重编号注入（零 id 冲突，
    /// 基底对象零重排）→ 页继承物化 + 挂接基底根 /Pages → AcroForm
    /// /Fields 拼接（/DR /DA 冲突基底赢，登记）/ 大纲顶层链接追加
    /// （或整体过继）/ 命名目的地单侧过继·双侧基底赢（计数）→
    /// 可达性剪除（源死中间节点收走）。
    pub fn merge(docs: &[&[u8]]) -> Result<Vec<u8>, CommandError> {
        Self::merge_with_limits(docs, Limits::default())
    }

    /// 带限额合并（每份源文档独立过限额面——单源超限即整体拒绝）。
    pub fn merge_with_limits(docs: &[&[u8]], limits: Limits) -> Result<Vec<u8>, CommandError> {
        if docs.is_empty() {
            return Err(CommandError::engine("merge：至少一份输入（空切片拒绝）"));
        }
        let mut target = Self::from_bytes_with_limits(docs[0].to_vec(), limits)?;
        for (i, src_bytes) in docs.iter().enumerate().skip(1) {
            let src = Self::from_bytes_with_limits((*src_bytes).to_vec(), limits)?;
            merge_one_into(&mut target, src.doc, &mut OrganizeReport::default())?;
            target.report.merged_docs += 1;
            let _ = i;
        }
        let pruned = prune_unreachable(&mut target.doc);
        target.register_prune(pruned);
        target.save_to_vec()
    }

    // -----------------------------------------------------------------------
    // 保存
    // -----------------------------------------------------------------------

    /// 全量序列化（lopdf xref 重建；确定性：对象按 id 序写出）。
    ///
    /// `max_id` 重算（实施发现）：lopdf 写出器从 `Document::max_id` 派生
    /// trailer /Size 与 xref 规模，而结构手术使该字段必陈旧——删页成孔
    /// （/Size 虚高）与合并注入（/Size 虚低，qpdf --check 对此出警告
    /// "reported number of objects is not one plus the highest object
    /// number"）。保存前按对象表真实最大号收口。
    pub fn save_to_vec(&mut self) -> Result<Vec<u8>, CommandError> {
        if let Some((&(true_max, _), _)) = self.doc.objects.iter().next_back() {
            self.doc.max_id = true_max;
        }
        let mut out = Vec::new();
        self.doc
            .save_to(&mut out)
            .map_err(|e| CommandError::engine(format!("lopdf 保存失败：{e}")))?;
        if self.report.first_save_growth_pct == 0.0 && self.input_len > 0 {
            let growth = (out.len() as f64 - self.input_len as f64) / self.input_len as f64;
            self.report.first_save_growth_pct = (growth * 1000.0).round() / 10.0;
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------
    // 内部辅助
    // -----------------------------------------------------------------------

    /// 文档序页对象 id 列表（权威——lopdf get_pages 1 基 BTreeMap 单点
    /// 换算）。
    fn page_ids(&self) -> Vec<ObjectId> {
        self.doc.get_pages().values().copied().collect()
    }

    fn page_id_at(&self, idx: usize) -> Result<ObjectId, CommandError> {
        let ids = self.page_ids();
        ids.get(idx)
            .copied()
            .ok_or(CommandError::InvalidPage(PageIndex(idx as u32)))
    }

    /// 剪除计数 + 线性化字典检测（输入带线性化且已不可达 → 置位。
    /// 编辑后线性化本就失效——登记非损失）。
    fn register_prune(&mut self, pruned: usize) {
        self.report.pruned_objects += pruned;
        if self.had_linearized && pruned > 0 && !self.report.linearized_dropped {
            let still = self
                .doc
                .objects
                .values()
                .any(|o| matches!(o, Object::Dictionary(d) if d.get(b"Linearized").is_ok()));
            if !still {
                self.report.linearized_dropped = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 页树手术（自由函数——组织器内部共用）
// ---------------------------------------------------------------------------

/// 根 /Pages 节点 id（catalog /Pages 引用）。
fn root_pages_id(doc: &Document) -> Result<ObjectId, CommandError> {
    let catalog = doc
        .catalog()
        .map_err(|e| CommandError::engine(format!("catalog 读取失败：{e}")))?;
    match catalog.get(b"Pages") {
        Ok(Object::Reference(id)) => Ok(*id),
        Ok(other) => Err(CommandError::engine(format!(
            "catalog /Pages 非间接引用形态：{other:?}（本版本要求引用形态）"
        ))),
        Err(_) => Err(CommandError::engine("catalog 缺 /Pages（页树根缺失）")),
    }
}

/// 页对象 /Parent（缺 /Parent 的孤儿页 = 结构破损，拒而非猜）。
fn parent_of(doc: &Document, page_id: ObjectId) -> Result<ObjectId, CommandError> {
    let page = doc
        .get_object(page_id)
        .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
        .as_dict()
        .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
    match page.get(b"Parent") {
        Ok(Object::Reference(id)) => Ok(*id),
        Ok(other) => Err(CommandError::engine(format!(
            "/Parent 非引用形态：{other:?}"
        ))),
        Err(_) => Err(CommandError::engine(
            "页缺 /Parent（孤儿页——结构破损拒绝手术）",
        )),
    }
}

/// /Kids 数组可变引用（直接数组/间接引用两形态；缺省建立空数组）。
fn kids_array_mut(
    doc: &mut Document,
    parent_id: ObjectId,
) -> Result<&mut Vec<Object>, CommandError> {
    // 先看形态（只读面），分支后取可变引用——避免借用交叉。
    let site = {
        let parent = doc
            .get_object(parent_id)
            .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
        let dict = parent
            .as_dict()
            .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
        match dict.get(b"Kids") {
            Ok(Object::Array(_)) => 0,     // 直接数组
            Ok(Object::Reference(_)) => 1, // 间接数组
            Ok(_) => 2,                    // 非法形态（下方报错）
            Err(_) => 3,                   // 缺省（下方建立）
        }
    };
    match site {
        0 => {
            let parent = doc
                .get_object_mut(parent_id)
                .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
            let dict = parent
                .as_dict_mut()
                .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
            match dict.get_mut(b"Kids") {
                Ok(Object::Array(arr)) => Ok(arr),
                Ok(other) => Err(CommandError::engine(format!("/Kids 非数组形态：{other:?}"))),
                Err(e) => Err(CommandError::engine(format!("/Kids 读取失败：{e}"))),
            }
        }
        1 => {
            let arr_id = {
                let parent = doc
                    .get_object(parent_id)
                    .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
                let dict = parent
                    .as_dict()
                    .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
                match dict.get(b"Kids") {
                    Ok(Object::Reference(id)) => *id,
                    _ => return Err(CommandError::engine("间接 Kids 解析失败（不可达）")),
                }
            };
            let arr_obj = doc
                .get_object_mut(arr_id)
                .map_err(|e| CommandError::engine(format!("/Kids 间接数组读取失败：{e}")))?;
            arr_obj
                .as_array_mut()
                .map_err(|e| CommandError::engine(format!("/Kids 非数组：{e}")))
        }
        2 => {
            let parent = doc
                .get_object(parent_id)
                .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
            let dict = parent
                .as_dict()
                .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
            let bad = dict.get(b"Kids").err().unwrap_or_else(|| unreachable!());
            let _ = bad;
            Err(CommandError::engine(
                "/Kids 非数组/引用形态（结构破损拒绝手术）",
            ))
        }
        _ => {
            let parent = doc
                .get_object_mut(parent_id)
                .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
            let dict = parent
                .as_dict_mut()
                .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
            dict.set("Kids", Object::Array(Vec::new()));
            let parent = doc
                .get_object_mut(parent_id)
                .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
            let dict = parent
                .as_dict_mut()
                .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
            match dict.get_mut(b"Kids") {
                Ok(Object::Array(arr)) => Ok(arr),
                _ => Err(CommandError::engine("Kids 建立失败（不可达）")),
            }
        }
    }
}

/// 页在父 /Kids 中的位置（0 基；不在 = 结构破损拒）。
fn kids_index_of(
    doc: &Document,
    parent_id: ObjectId,
    page_id: ObjectId,
) -> Result<usize, CommandError> {
    let parent = doc
        .get_object(parent_id)
        .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
    let dict = parent
        .as_dict()
        .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
    let kids: Vec<Object> = match dict.get(b"Kids") {
        Ok(Object::Array(arr)) => arr.clone(),
        Ok(Object::Reference(arr_id)) => doc
            .get_object(*arr_id)
            .map_err(|e| CommandError::engine(format!("/Kids 间接数组读取失败：{e}")))?
            .as_array()
            .map_err(|e| CommandError::engine(format!("/Kids 非数组：{e}")))?
            .clone(),
        _ => return Err(CommandError::engine("父节点 /Kids 缺失（结构破损）")),
    };
    kids.iter()
        .position(|o| matches!(o, Object::Reference(id) if *id == page_id))
        .ok_or_else(|| CommandError::engine("页不在父 /Kids 中（结构破损拒绝手术）"))
}

/// 从父 /Kids 摘除页引用 + 祖先 /Count 递减（负值钳 0——输入文档
/// /Count 失真防御，不 panic）。页对象本体**不删**（可达性剪除统一收）。
fn detach_page_from_tree(doc: &mut Document, page_id: ObjectId) -> Result<(), CommandError> {
    let parent_id = parent_of(doc, page_id)?;
    let idx = kids_index_of(doc, parent_id, page_id)?;
    {
        let kids = kids_array_mut(doc, parent_id)?;
        kids.remove(idx);
    }
    bump_count_chain(doc, parent_id, -1)
}

/// /Kids 定位插入（pos 越界钳到末尾——防御面）。
fn insert_into_kids(
    doc: &mut Document,
    parent_id: ObjectId,
    pos: usize,
    page_id: ObjectId,
) -> Result<(), CommandError> {
    let kids = kids_array_mut(doc, parent_id)?;
    let pos = pos.min(kids.len());
    kids.insert(pos, Object::Reference(page_id));
    Ok(())
}

/// 沿祖先链调 /Count（delta ±1；祖先链环防御：visited 集截断）。
fn bump_count_chain(doc: &mut Document, from: ObjectId, delta: i64) -> Result<(), CommandError> {
    let mut cur = Some(from);
    let mut visited = HashSet::new();
    while let Some(id) = cur {
        if !visited.insert(id) {
            return Err(CommandError::engine("/Pages 祖先链成环（结构破损）"));
        }
        let node = doc
            .get_object_mut(id)
            .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
        let dict = node
            .as_dict_mut()
            .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?;
        let next = match dict.get(b"Parent") {
            Ok(Object::Reference(pid)) => Some(*pid),
            _ => None,
        };
        if let Ok(Object::Integer(count)) = dict.get(b"Count") {
            let new = (*count + delta).max(0);
            dict.set("Count", Object::Integer(new));
        }
        cur = next;
    }
    Ok(())
}

/// 直接设置节点 /Count（扁平化后根计数重算）。
fn set_count(doc: &mut Document, id: ObjectId, count: i64) -> Result<(), CommandError> {
    let node = doc
        .get_object_mut(id)
        .map_err(|e| CommandError::engine(format!("/Pages 节点读取失败：{e}")))?;
    node.as_dict_mut()
        .map_err(|e| CommandError::engine(format!("/Pages 节点非字典：{e}")))?
        .set("Count", Object::Integer(count));
    Ok(())
}

/// 继承键（spec 7.7.3.3 表 30——/Resources /MediaBox /CropBox /Rotate）。
const INHERITABLE_KEYS: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// 继承物化：把祖先链（页 → 根）的有效继承键以**引用/值直拷**落到页
/// 字典（仅页上缺省的键；近祖优先——沿链向上首见即有效值）。对象本体
/// 零复制（间接引用保持共享）。返回物化键数。
fn materialize_inheritance(doc: &mut Document, page_id: ObjectId) -> Result<usize, CommandError> {
    // 祖先链收集（页自身也入链——物化只补缺省，页自带值不覆盖）。
    let mut chain = Vec::new();
    let mut cur = page_id;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(cur) {
            return Err(CommandError::engine("/Pages 祖先链成环（结构破损）"));
        }
        chain.push(cur);
        let obj = doc
            .get_object(cur)
            .map_err(|e| CommandError::engine(format!("节点读取失败：{e}")))?;
        let dict = obj
            .as_dict()
            .map_err(|e| CommandError::engine(format!("节点非字典：{e}")))?;
        cur = match dict.get(b"Parent") {
            Ok(Object::Reference(pid)) => *pid,
            _ => break,
        };
    }
    // 近祖优先：从 chain[1]（父）向根找每键首见值。
    let mut to_set: Vec<(&[u8], Object)> = Vec::new();
    for key in INHERITABLE_KEYS {
        for &node_id in chain.iter().skip(1) {
            let obj = doc
                .get_object(node_id)
                .map_err(|e| CommandError::engine(format!("节点读取失败：{e}")))?;
            let dict = obj
                .as_dict()
                .map_err(|e| CommandError::engine(format!("节点非字典：{e}")))?;
            if let Ok(value) = dict.get(key) {
                to_set.push((key, value.clone()));
                break;
            }
        }
    }
    let page = doc
        .get_object_mut(page_id)
        .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
        .as_dict_mut()
        .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
    let mut applied = 0usize;
    for (key, value) in to_set {
        if page.get(key).is_err() {
            page.set(key.to_vec(), value);
            applied += 1;
        }
    }
    Ok(applied)
}

/// 继承整数键查找（/Rotate 用——沿祖先链首见）。
fn inherited_integer(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<i64> {
    let mut cur = page_id;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(cur) {
            return None;
        }
        let obj = doc.get_object(cur).ok()?;
        let dict = obj.as_dict().ok()?;
        if let Ok(Object::Integer(v)) = dict.get(key) {
            return Some(*v);
        }
        cur = match dict.get(b"Parent") {
            Ok(Object::Reference(pid)) => *pid,
            _ => return None,
        };
    }
}

/// 可达性剪除（trailer 可达集之外移除；返回剪除数）。
fn prune_unreachable(doc: &mut Document) -> usize {
    doc.prune_objects().len()
}

// ---------------------------------------------------------------------------
// 书签（/Outlines）手术
// ---------------------------------------------------------------------------

/// 丢弃目标页 ∈ dead 的书签（整子树——Acrobat 语义），返回丢弃项数
/// （含子树展开）。幸存项**原地保留**（对象号不动），只做：
/// ①死项从父的 /First//Last 与兄弟 /Next//Prev 链摘除；
/// ②父项（含大纲根）/Count 重算（符号沿旧值——开/合状态保真）；
/// ③全灭时 catalog /Outlines 摘除（根对象交剪除）。
fn drop_dead_bookmarks(doc: &mut Document, dead: &HashSet<ObjectId>) -> usize {
    let outlines_id = match doc.catalog().and_then(|c| c.get(b"Outlines")) {
        Ok(Object::Reference(id)) => *id,
        _ => return 0,
    };

    // 树遍历收集（前序 + 父子表；敌意环防御 10 万步截断）。
    let mut all: Vec<ObjectId> = Vec::new();
    let mut children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    let top: Vec<ObjectId> = sibling_chain(doc, outlines_id).unwrap_or_default();
    let mut stack: Vec<ObjectId> = top.iter().rev().copied().collect();
    let mut guard = 0usize;
    while let Some(id) = stack.pop() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        all.push(id);
        if let Some(kids) = child_items(doc, id) {
            for k in kids.iter().rev() {
                stack.push(*k);
            }
            children.insert(id, kids);
        }
    }
    if all.is_empty() {
        return 0;
    }

    // 死集：目标页 ∈ dead 的项 + 其全部子孙（子树语义）。
    let mut dead_items: HashSet<ObjectId> = HashSet::new();
    for &item in &all {
        if !dead_items.contains(&item)
            && item_dest_page(doc, item).is_some_and(|p| dead.contains(&p))
        {
            let mut sub = vec![item];
            while let Some(cur) = sub.pop() {
                if dead_items.insert(cur) {
                    if let Some(kids) = children.get(&cur) {
                        sub.extend(kids.iter().copied());
                    }
                }
            }
        }
    }
    let dropped = dead_items.len();
    if dropped == 0 {
        return 0;
    }

    // 兄弟链 + 父 /First//Last 重排（只动幸存者之间的链接；对象零删除
    // ——死项本体交剪除）。
    let mut relink_parents: Vec<ObjectId> = children
        .keys()
        .copied()
        .chain(std::iter::once(outlines_id))
        .filter(|p| !dead_items.contains(p))
        .collect();
    relink_parents.sort_unstable();
    relink_parents.dedup();
    for parent in relink_parents {
        let siblings: Vec<ObjectId> = if parent == outlines_id {
            sibling_chain(doc, outlines_id).unwrap_or_default()
        } else {
            children.get(&parent).cloned().unwrap_or_default()
        };
        let survivors: Vec<ObjectId> = siblings
            .into_iter()
            .filter(|id| !dead_items.contains(id))
            .collect();
        relink_sibling_chain(doc, parent, &survivors);
        let alive = count_descendants(&children, &dead_items, &parent);
        if parent == outlines_id {
            set_outline_count(doc, outlines_id, alive as i64);
        } else if let Some(old) = get_outline_count(doc, parent) {
            let sign = if old >= 0 { 1 } else { -1 };
            set_outline_count(doc, parent, sign * alive as i64);
        }
    }
    if all.len() == dropped {
        // 全灭：catalog 摘除 /Outlines（大纲根对象交剪除）。
        if let Ok(catalog) = doc.catalog_mut() {
            catalog.remove(b"Outlines");
        }
    }
    dropped
}

/// 节点直接子项序（/First→/Next 链收集）。
fn child_items(doc: &Document, item: ObjectId) -> Option<Vec<ObjectId>> {
    let dict = doc.get_object(item).ok()?.as_dict().ok()?;
    match dict.get(b"First") {
        Ok(Object::Reference(first)) => Some(sibling_chain_from(doc, *first)),
        _ => None,
    }
}

/// 大纲根的顶层链（根 /First 起）。
fn sibling_chain(doc: &Document, outlines_id: ObjectId) -> Option<Vec<ObjectId>> {
    let dict = doc.get_object(outlines_id).ok()?.as_dict().ok()?;
    match dict.get(b"First") {
        Ok(Object::Reference(first)) => Some(sibling_chain_from(doc, *first)),
        _ => None,
    }
}

/// /Next 链收集（环防御）。
fn sibling_chain_from(doc: &Document, first: ObjectId) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut cur = first;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(cur) {
            break;
        }
        out.push(cur);
        let next = doc
            .get_object(cur)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| match d.get(b"Next") {
                Ok(Object::Reference(id)) => Some(*id),
                _ => None,
            });
        match next {
            Some(id) => cur = id,
            None => break,
        }
    }
    out
}

/// 兄弟链重排（survivors 依序设 /Next//Prev + /Parent；父设 /First//Last；
/// 空链摘除父的 /First//Last）。
fn relink_sibling_chain(doc: &mut Document, parent: ObjectId, survivors: &[ObjectId]) {
    for (i, &id) in survivors.iter().enumerate() {
        let dict = match doc.get_object_mut(id).map(|o| o.as_dict_mut().ok()) {
            Ok(Some(d)) => d,
            _ => continue,
        };
        dict.set("Parent", Object::Reference(parent));
        match i.checked_sub(1) {
            Some(prev) => dict.set("Prev", Object::Reference(survivors[prev])),
            None => {
                dict.remove(b"Prev");
            }
        }
        match survivors.get(i + 1) {
            Some(next) => dict.set("Next", Object::Reference(*next)),
            None => {
                dict.remove(b"Next");
            }
        }
    }
    let parent_dict = match doc.get_object_mut(parent).map(|o| o.as_dict_mut().ok()) {
        Ok(Some(d)) => d,
        _ => return,
    };
    match (survivors.first(), survivors.last()) {
        (Some(f), Some(l)) => {
            parent_dict.set("First", Object::Reference(*f));
            parent_dict.set("Last", Object::Reference(*l));
        }
        (None, _) => {
            parent_dict.remove(b"First");
            parent_dict.remove(b"Last");
        }
        _ => unreachable!(),
    }
}

/// 大纲项 /Count（无 = None）。
fn get_outline_count(doc: &Document, id: ObjectId) -> Option<i64> {
    doc.get_object(id)
        .ok()?
        .as_dict()
        .ok()?
        .get(b"Count")
        .ok()?
        .as_i64()
        .ok()
}

fn set_outline_count(doc: &mut Document, id: ObjectId, count: i64) {
    if let Ok(Some(d)) = doc.get_object_mut(id).map(|o| o.as_dict_mut().ok()) {
        d.set("Count", Object::Integer(count));
    }
}

/// 存活后代计数（含子树展开；parent 本身不计）。
fn count_descendants(
    children: &HashMap<ObjectId, Vec<ObjectId>>,
    dead: &HashSet<ObjectId>,
    parent: &ObjectId,
) -> usize {
    let mut total = 0;
    let mut stack: Vec<ObjectId> = children.get(parent).cloned().unwrap_or_default();
    while let Some(cur) = stack.pop() {
        if dead.contains(&cur) {
            continue;
        }
        total += 1;
        stack.extend(children.get(&cur).cloned().unwrap_or_default());
    }
    total
}

/// 大纲项目标页（三形态：/Dest 数组 / /Dest 名称 / /A /GoTo 动作）。
/// 无目标/不可解析 → None（保守保留——预存破损不是本手术的损失面）。
fn item_dest_page(doc: &Document, item: ObjectId) -> Option<ObjectId> {
    let dict = doc.get_object(item).ok()?.as_dict().ok()?;
    if let Ok(dest) = dict.get(b"Dest") {
        if let Some(page) = dest_page_of(doc, dest) {
            return Some(page);
        }
    }
    if let Ok(Object::Dictionary(action)) = dict.get(b"A") {
        if let (Ok(subtype), Ok(dest)) = (action.get(b"S"), action.get(b"D")) {
            if matches!(subtype, Object::Name(n) if n == b"GoTo") {
                if let Some(page) = dest_page_of(doc, dest) {
                    return Some(page);
                }
            }
        }
    }
    None
}

/// 目标对象 → 页 id（数组形态 [page /XYZ ...]；名称形态经 /Dests 字典
/// 或 /Names 名称树解析；显式页号形态（spec 12.3.2.2）换算文档序）。
fn dest_page_of(doc: &Document, dest: &Object) -> Option<ObjectId> {
    match dest {
        Object::Array(arr) => match arr.first() {
            Some(Object::Reference(id)) => Some(*id),
            Some(Object::Integer(n)) if *n >= 0 => doc.get_pages().get(&(*n as u32 + 1)).copied(),
            _ => None,
        },
        Object::Name(name) | Object::String(name, _) => resolve_named_dest(doc, name),
        _ => None,
    }
}

/// 命名目的地解析：PDF 1.1 /Dests 字典（catalog）与 /Names /Dests 名称
/// 树（spec 7.9.6；Kids 递归深度 ≤ 16，/Names 数组 [k1 v1 k2 v2 ...]）。
fn resolve_named_dest(doc: &Document, name: &[u8]) -> Option<ObjectId> {
    let catalog = doc.catalog().ok()?;
    if let Ok(Object::Reference(dests_id)) = catalog.get(b"Dests") {
        if let Ok(Object::Dictionary(dests)) = doc.get_object(*dests_id) {
            if let Ok(dest) = dests.get(name) {
                if let Some(page) = dest_page_of(doc, dest) {
                    return Some(page);
                }
            }
        }
    }
    if let Ok(Object::Reference(names_id)) = catalog.get(b"Names") {
        if let Ok(Object::Dictionary(names)) = doc.get_object(*names_id) {
            if let Ok(Object::Reference(dests_id)) = names.get(b"Dests") {
                if let Some(dest) = name_tree_lookup(doc, *dests_id, name, 0) {
                    if let Some(page) = dest_page_of(doc, &dest) {
                        return Some(page);
                    }
                }
            }
        }
    }
    None
}

/// 名称树查找（节点 /Names 数组或 /Kids 递归；深度防御）。
fn name_tree_lookup(
    doc: &Document,
    node_id: ObjectId,
    name: &[u8],
    depth: usize,
) -> Option<Object> {
    if depth > 16 {
        return None;
    }
    let node = doc.get_object(node_id).ok()?.as_dict().ok()?;
    if let Ok(Object::Array(pairs)) = node.get(b"Names") {
        let mut i = 0;
        while i + 1 < pairs.len() {
            let key_matches = match &pairs[i] {
                Object::Name(k) | Object::String(k, _) => k == name,
                _ => false,
            };
            if key_matches {
                return Some(pairs[i + 1].clone());
            }
            i += 2;
        }
    }
    if let Ok(Object::Array(kids)) = node.get(b"Kids") {
        for kid in kids {
            if let Object::Reference(kid_id) = kid {
                if let Some(found) = name_tree_lookup(doc, *kid_id, name, depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// 名称树条目计数（递归 /Names 对数——命名目的地合并的登记面）。
fn name_tree_count(doc: &Document, node_id: ObjectId, depth: usize) -> usize {
    if depth > 16 {
        return 0;
    }
    let Ok(node) = doc.get_object(node_id).map(|o| o.as_dict().ok()) else {
        return 0;
    };
    let Some(node) = node else {
        return 0;
    };
    let mut total = 0usize;
    if let Ok(Object::Array(pairs)) = node.get(b"Names") {
        total += pairs.len() / 2;
    }
    if let Ok(Object::Array(kids)) = node.get(b"Kids") {
        for kid in kids {
            if let Object::Reference(kid_id) = kid {
                total += name_tree_count(doc, *kid_id, depth + 1);
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// 链接标注手术（幸存页上目标失效的 /Link）
// ---------------------------------------------------------------------------

/// 移除幸存页上指向 dead 页的 /Link 标注（不留悬空目标）。URI/远程
/// GoToR 链接不涉本档页——零波及。
fn remove_dead_links(doc: &mut Document, dead: &HashSet<ObjectId>) -> usize {
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let mut removed = 0usize;
    for page_id in page_ids {
        if dead.contains(&page_id) {
            continue;
        }
        // /Annots 内容读出（两形态），过滤后写回原形态。
        let indirect_arr: Option<ObjectId> = {
            let page = match doc.get_object(page_id).map(|o| o.as_dict().ok()) {
                Ok(Some(d)) => d,
                _ => continue,
            };
            match page.get(b"Annots") {
                Ok(Object::Reference(id)) => Some(*id),
                Ok(Object::Array(_)) => None,
                _ => continue,
            }
        };
        let raw: Vec<Object> = {
            let page = match doc.get_object(page_id).map(|o| o.as_dict().ok()) {
                Ok(Some(d)) => d,
                _ => continue,
            };
            match page.get(b"Annots") {
                Ok(Object::Array(arr)) => arr.clone(),
                Ok(Object::Reference(id)) => match doc.get_object(*id).map(|o| o.as_array().ok()) {
                    Ok(Some(arr)) => arr.clone(),
                    _ => continue,
                },
                _ => continue,
            }
        };
        let raw_len = raw.len();
        let mut kept: Vec<Object> = Vec::with_capacity(raw_len);
        for entry in raw {
            let is_dead_link = match &entry {
                Object::Reference(id) => link_targets_dead(doc, *id, dead),
                Object::Dictionary(dict) => link_dict_targets_dead(doc, dict, dead),
                _ => false,
            };
            if is_dead_link {
                removed += 1;
            } else {
                kept.push(entry);
            }
        }
        if kept.len() != raw_len {
            match indirect_arr {
                Some(arr_id) => {
                    if let Ok(obj) = doc.get_object_mut(arr_id) {
                        if let Ok(arr) = obj.as_array_mut() {
                            *arr = kept;
                        }
                    }
                }
                None => {
                    if let Ok(Some(page_dict)) =
                        doc.get_object_mut(page_id).map(|o| o.as_dict_mut().ok())
                    {
                        page_dict.set("Annots", Object::Array(kept));
                    }
                }
            }
        }
    }
    removed
}

/// 间接引用的链接标注是否目标死页。
fn link_targets_dead(doc: &Document, annot_id: ObjectId, dead: &HashSet<ObjectId>) -> bool {
    match doc.get_object(annot_id).map(|o| o.as_dict().ok()) {
        Ok(Some(dict)) => link_dict_targets_dead(doc, dict, dead),
        _ => false,
    }
}

/// 链接标注字典是否目标死页（/Dest 或 /A /GoTo → 目标页 ∈ dead）。
fn link_dict_targets_dead(doc: &Document, dict: &Dictionary, dead: &HashSet<ObjectId>) -> bool {
    let is_link = matches!(dict.get(b"Subtype"), Ok(Object::Name(n)) if n == b"Link");
    if !is_link {
        return false;
    }
    if let Ok(dest) = dict.get(b"Dest") {
        if let Some(page) = dest_page_of(doc, dest) {
            return dead.contains(&page);
        }
    }
    if let Ok(Object::Dictionary(action)) = dict.get(b"A") {
        if let (Ok(subtype), Ok(dest)) = (action.get(b"S"), action.get(b"D")) {
            if matches!(subtype, Object::Name(n) if n == b"GoTo") {
                if let Some(page) = dest_page_of(doc, dest) {
                    return dead.contains(&page);
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 表单域手术（被删页 widget 的 /AcroForm /Fields 条目）
// ---------------------------------------------------------------------------

/// 被删页 /Annots 中的 /Widget 对应的域条目从 /AcroForm /Fields 移除。
/// 域本体定位：widget 自带 /T（终端域即本体）或 /Parent 链首见带 /T
/// 的祖先。返回移除的域条目数。
fn remove_dead_form_fields(doc: &mut Document, dead_pages: &HashSet<ObjectId>) -> usize {
    let mut dead_fields: HashSet<ObjectId> = HashSet::new();
    for &page_id in dead_pages {
        let annots: Vec<Object> = {
            let page = match doc.get_object(page_id).map(|o| o.as_dict().ok()) {
                Ok(Some(d)) => d,
                _ => continue,
            };
            match page.get(b"Annots") {
                Ok(Object::Array(arr)) => arr.clone(),
                Ok(Object::Reference(id)) => doc
                    .get_object(*id)
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .cloned()
                    .unwrap_or_default(),
                _ => continue,
            }
        };
        for entry in annots {
            let widget_id = match entry {
                Object::Reference(id) => id,
                Object::Dictionary(d) => {
                    // 直接字典 widget（少见形态）：本体即此字典——无对象号，
                    // 无 /Fields 条目可摘，跳过（剪除页对象时随之消失）。
                    let _ = d;
                    continue;
                }
                _ => continue,
            };
            let dict = match doc.get_object(widget_id).map(|o| o.as_dict().ok()) {
                Ok(Some(d)) => d,
                _ => continue,
            };
            if !matches!(dict.get(b"Subtype"), Ok(Object::Name(n)) if n == b"Widget") {
                continue;
            }
            let mut field_id = widget_id;
            let mut cur = widget_id;
            for _ in 0..16 {
                let d = match doc.get_object(cur).map(|o| o.as_dict().ok()) {
                    Ok(Some(d)) => d,
                    _ => break,
                };
                if d.get(b"T").is_ok() {
                    field_id = cur;
                    break;
                }
                match d.get(b"Parent") {
                    Ok(Object::Reference(p)) => cur = *p,
                    _ => break,
                }
            }
            dead_fields.insert(field_id);
        }
    }
    if dead_fields.is_empty() {
        return 0;
    }
    // /AcroForm /Fields 摘除（间接引用/直接字典两形态）。
    let acroform_ref: Option<ObjectId> = match doc.catalog().and_then(|c| c.get(b"AcroForm")) {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    match acroform_ref {
        Some(form_id) => {
            let form = match doc.get_object_mut(form_id).map(|o| o.as_dict_mut().ok()) {
                Ok(Some(d)) => d,
                _ => return 0,
            };
            filter_fields(form, &dead_fields)
        }
        None => {
            let catalog = match doc.catalog_mut() {
                Ok(c) => c,
                Err(_) => return 0,
            };
            match catalog.get_mut(b"AcroForm") {
                Ok(Object::Dictionary(form)) => filter_fields(form, &dead_fields),
                _ => 0,
            }
        }
    }
}

/// /Fields 数组过滤（摘除 dead 域引用），返回摘除数。
fn filter_fields(form: &mut Dictionary, dead: &HashSet<ObjectId>) -> usize {
    let fields = match form.get_mut(b"Fields") {
        Ok(Object::Array(arr)) => arr,
        _ => return 0,
    };
    let before = fields.len();
    fields.retain(|o| !matches!(o, Object::Reference(id) if dead.contains(id)));
    before - fields.len()
}

// ---------------------------------------------------------------------------
// merge 手术（源文档 → 基底注入）
// ---------------------------------------------------------------------------

/// 源文档锚点（注入前从源视图采集——注入后源 catalog 不可达）。
struct SrcAnchors {
    /// 源页对象 id（文档序）。
    pages: Vec<ObjectId>,
    /// 源 catalog /AcroForm 字典克隆（源 id；None = 无表单）。
    acroform: Option<Dictionary>,
    /// 源 catalog /Outlines 引用（源 id；None = 无大纲）。
    outlines: Option<ObjectId>,
    /// 源旧式 /Dests 字典（catalog /Dests；间接引用克隆）。
    old_dests: Option<Dictionary>,
    /// 源 catalog /Names 引用（含 /Dests 名称树等通道）。
    names: Option<ObjectId>,
    /// 源 /Names 树内 /Dests 节点引用（源 id）。
    names_dests: Option<ObjectId>,
}

/// 单源合并：锚点采集 → 源页物化 → 对象图重编号注入 → 页挂接 →
/// AcroForm/大纲/命名目的地拼接。
fn merge_one_into(
    target: &mut PdfOrganizer,
    mut src_doc: Document,
    _src_report: &mut OrganizeReport,
) -> Result<(), CommandError> {
    // 0. 基底页树扁平化（嵌套 → 物化 + 根 Kids 全叶——追加面恒定）。
    let target_root = root_pages_id(&target.doc)?;
    flatten_target_if_nested(target, target_root)?;

    // 1. 源锚点采集（源视图）。
    let anchors = collect_src_anchors(&src_doc)?;

    // 2. 源页继承物化（源视图写——祖先链注入前算好）。
    let mut materialized = 0usize;
    for &p in &anchors.pages {
        materialized += materialize_inheritance(&mut src_doc, p)?;
    }

    // 3. 对象图重编号注入（源全部对象 → 基底 max_id 之后顺序新号）。
    let next_free = next_free_id(&target.doc);
    let id_map: BTreeMap<ObjectId, ObjectId> = src_doc
        .objects
        .keys()
        .enumerate()
        .map(|(i, &old)| (old, (next_free + i as u32, 0u16)))
        .collect();
    let entries: Vec<(ObjectId, Object)> =
        std::mem::take(&mut src_doc.objects).into_iter().collect();
    for (old_id, mut obj) in entries {
        rewrite_refs(&mut obj, &id_map);
        target.doc.set_object(id_map[&old_id], obj);
    }

    // 4. 源页挂接基底根（/Parent 改指 + Kids 追加 + Count 重算）。
    let total_after;
    {
        let kids = kids_array_mut(&mut target.doc, target_root)?;
        for &p in &anchors.pages {
            kids.push(Object::Reference(id_map[&p]));
        }
        total_after = kids.len();
    }
    for &p in &anchors.pages {
        let new_id = id_map[&p];
        if let Ok(Some(d)) = target
            .doc
            .get_object_mut(new_id)
            .map(|o| o.as_dict_mut().ok())
        {
            d.set("Parent", Object::Reference(target_root));
        }
    }
    set_count(&mut target.doc, target_root, total_after as i64)?;

    // 5. AcroForm 拼接。
    merge_acroform(target, &anchors, &id_map)?;
    // 6. 大纲拼接。
    let merged_bm = merge_outlines(target, &anchors, &id_map)?;
    // 7. 命名目的地拼接。
    let dropped_nd = merge_named_dests(target, &anchors, &id_map)?;

    // 源侧增益/损失计数并入基底（跨源累加）。
    target.report.materialized_pages += materialized;
    target.report.merged_bookmarks += merged_bm;
    target.report.dropped_named_dests += dropped_nd;
    Ok(())
}

/// 基底页树扁平化（嵌套时物化 + 根 Kids 全叶；平树零动作）。
fn flatten_target_if_nested(target: &mut PdfOrganizer, root: ObjectId) -> Result<(), CommandError> {
    let pages: Vec<ObjectId> = target.doc.get_pages().values().copied().collect();
    let nested = pages.iter().any(|&p| parent_of(&target.doc, p) != Ok(root));
    if !nested {
        return Ok(());
    }
    let mut materialized = 0usize;
    for &p in &pages {
        materialized += materialize_inheritance(&mut target.doc, p)?;
        if let Ok(Some(d)) = target.doc.get_object_mut(p).map(|o| o.as_dict_mut().ok()) {
            d.set("Parent", Object::Reference(root));
        }
    }
    let kids = kids_array_mut(&mut target.doc, root)?;
    kids.clear();
    for &p in &pages {
        kids.push(Object::Reference(p));
    }
    set_count(&mut target.doc, root, pages.len() as i64)?;
    target.report.materialized_pages += materialized;
    Ok(())
}

/// 源锚点采集（catalog /AcroForm //Outlines //Dests //Names 五面）。
fn collect_src_anchors(src_doc: &Document) -> Result<SrcAnchors, CommandError> {
    let pages: Vec<ObjectId> = src_doc.get_pages().values().copied().collect();
    let catalog_id = match src_doc.trailer.get(b"Root") {
        Ok(Object::Reference(id)) => *id,
        _ => return Err(CommandError::engine("源文档缺 catalog（trailer /Root）")),
    };
    let catalog = src_doc
        .get_object(catalog_id)
        .map_err(|e| CommandError::engine(format!("源 catalog 读取失败：{e}")))?
        .as_dict()
        .map_err(|e| CommandError::engine(format!("源 catalog 非字典：{e}")))?;

    let acroform = match catalog.get(b"AcroForm") {
        Ok(Object::Dictionary(f)) => Some(f.clone()),
        Ok(Object::Reference(id)) => src_doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned(),
        _ => None,
    };
    let outlines = match catalog.get(b"Outlines") {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    let old_dests = match catalog.get(b"Dests") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        Ok(Object::Reference(id)) => src_doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned(),
        _ => None,
    };
    let names = match catalog.get(b"Names") {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    let names_dests = names.and_then(|names_id| {
        src_doc
            .get_object(names_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| match d.get(b"Dests") {
                Ok(Object::Reference(id)) => Some(*id),
                _ => None,
            })
    });
    Ok(SrcAnchors {
        pages,
        acroform,
        outlines,
        old_dests,
        names,
        names_dests,
    })
}

/// 基底下一空闲对象号。
fn next_free_id(doc: &Document) -> u32 {
    doc.objects
        .keys()
        .map(|(num, _)| num)
        .max()
        .map(|m| m + 1)
        .unwrap_or(1)
}

/// 深度引用重写（对象树全走；流内容零触碰——内容流不含对象引用语法）。
fn rewrite_refs(obj: &mut Object, map: &BTreeMap<ObjectId, ObjectId>) {
    match obj {
        Object::Reference(id) => {
            if let Some(new) = map.get(id) {
                *id = *new;
            }
        }
        Object::Array(items) => {
            for item in items {
                rewrite_refs(item, map);
            }
        }
        Object::Dictionary(dict) => rewrite_dict_refs(dict, map),
        Object::Stream(stream) => rewrite_dict_refs(&mut stream.dict, map),
        _ => {}
    }
}

/// 字典引用重写。
fn rewrite_dict_refs(dict: &mut Dictionary, map: &BTreeMap<ObjectId, ObjectId>) {
    let keys: Vec<Vec<u8>> = dict.iter().map(|(k, _)| k.clone()).collect();
    for key in keys {
        if let Ok(v) = dict.get_mut(&key) {
            rewrite_refs(v, map);
        }
    }
}

/// AcroForm 拼接：基底无 → 过继源（重写后直挂 catalog）；双方皆有 →
/// /Fields 追加（源域引用经 id_map）+ /DR /DA 冲突基底赢（登记
/// dropped_form_appearances）。返回拼接域数。
fn merge_acroform(
    target: &mut PdfOrganizer,
    anchors: &SrcAnchors,
    id_map: &BTreeMap<ObjectId, ObjectId>,
) -> Result<usize, CommandError> {
    let Some(src_form) = &anchors.acroform else {
        return Ok(0);
    };
    // 基底现状。
    let target_has_form = matches!(
        target.doc.catalog().and_then(|c| c.get(b"AcroForm")),
        Ok(Object::Dictionary(_)) | Ok(Object::Reference(_))
    );
    if !target_has_form {
        // 过继：源字典重写后直挂 catalog（直接字典形态——合法且免新对象）。
        let mut form = src_form.clone();
        rewrite_dict_refs(&mut form, id_map);
        let catalog = target
            .doc
            .catalog_mut()
            .map_err(|e| CommandError::engine(format!("catalog 写入失败：{e}")))?;
        catalog.set("AcroForm", Object::Dictionary(form));
        return Ok(0);
    }
    // 双方皆有：/Fields 追加。
    let src_fields: Vec<Object> = match src_form.get(b"Fields") {
        Ok(Object::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    };
    let mut mapped: Vec<Object> = Vec::with_capacity(src_fields.len());
    for f in src_fields {
        match f {
            Object::Reference(id) => {
                let new = id_map.get(&id).copied().unwrap_or(id);
                mapped.push(Object::Reference(new));
            }
            other => mapped.push(other),
        }
    }
    let appended = mapped.len();
    // 外观资源冲突登记（双方皆带 /DR 或 /DA ——基底赢，源域可能回退渲染）。
    let target_form_dict = read_target_acroform(&target.doc);
    if let Some(tf) = target_form_dict.as_ref() {
        if tf.get(b"DR").is_ok() && src_form.get(b"DR").is_ok() {
            target.report.dropped_form_appearances += 1;
        }
    }
    if let Some(tf) = target_form_dict.as_ref() {
        if tf.get(b"DA").is_ok() && src_form.get(b"DA").is_ok() {
            target.report.dropped_form_appearances += 1;
        }
    }
    // /Fields 写回（直接数组/间接引用两形态）。
    let fields_ref: Option<ObjectId> = match target.doc.catalog().and_then(|c| c.get(b"AcroForm")) {
        Ok(Object::Reference(id)) => match target.doc.get_object(*id).map(|o| {
            o.as_dict().ok().and_then(|d| match d.get(b"Fields") {
                Ok(Object::Reference(f)) => Some(*f),
                _ => None,
            })
        }) {
            Ok(Some(f)) => Some(f),
            _ => None,
        },
        _ => None,
    };
    match fields_ref {
        Some(fields_arr_id) => {
            if let Ok(obj) = target.doc.get_object_mut(fields_arr_id) {
                if let Ok(arr) = obj.as_array_mut() {
                    arr.extend(mapped);
                }
            }
        }
        None => {
            // 直接字典路径：catalog /AcroForm → /Fields 数组（基底或过继态）。
            let acroform_ref = match target.doc.catalog().and_then(|c| c.get(b"AcroForm")) {
                Ok(Object::Reference(id)) => Some(*id),
                _ => None,
            };
            match acroform_ref {
                Some(form_id) => {
                    if let Ok(Some(form)) = target
                        .doc
                        .get_object_mut(form_id)
                        .map(|o| o.as_dict_mut().ok())
                    {
                        match form.get_mut(b"Fields") {
                            Ok(Object::Array(arr)) => arr.extend(mapped),
                            _ => form.set("Fields", Object::Array(mapped)),
                        }
                    }
                }
                None => {
                    if let Ok(Some(form)) = target.doc.catalog_mut().map(|c| {
                        c.get_mut(b"AcroForm")
                            .ok()
                            .and_then(|o| o.as_dict_mut().ok())
                    }) {
                        match form.get_mut(b"Fields") {
                            Ok(Object::Array(arr)) => arr.extend(mapped),
                            _ => form.set("Fields", Object::Array(mapped)),
                        }
                    }
                }
            }
        }
    }
    Ok(appended)
}

/// 基底 /AcroForm 字典读取（两形态；无 = None）。
fn read_target_acroform(doc: &Document) -> Option<Dictionary> {
    match doc.catalog().and_then(|c| c.get(b"AcroForm")) {
        Ok(Object::Dictionary(f)) => Some(f.clone()),
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned(),
        _ => None,
    }
}

/// 大纲拼接：基底有 /Outlines → 源顶层链接追加到基底顶层链尾（/Next/
/// /Prev 修复 + /Parent 改指基底根 + 根 /Last 更新 + /Count 重算）；
/// 基底无 → 源大纲根过继为 catalog /Outlines。返回携入项数（全树）。
fn merge_outlines(
    target: &mut PdfOrganizer,
    anchors: &SrcAnchors,
    id_map: &BTreeMap<ObjectId, ObjectId>,
) -> Result<usize, CommandError> {
    let Some(&src_root_src) = anchors.outlines.as_ref() else {
        return Ok(0);
    };
    let src_root = id_map[&src_root_src];
    // 源全树计数（携入登记）。
    let src_top = sibling_chain(&target.doc, src_root).unwrap_or_default();
    let mut merged = src_top.len();
    for &item in &src_top {
        merged += count_subtree(&target.doc, item);
    }
    let target_root_outline = match target.doc.catalog().and_then(|c| c.get(b"Outlines")) {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    match target_root_outline {
        None => {
            // 过继：源大纲根（已注入）直挂 catalog。
            let catalog = target
                .doc
                .catalog_mut()
                .map_err(|e| CommandError::engine(format!("catalog 写入失败：{e}")))?;
            catalog.set("Outlines", Object::Reference(src_root));
        }
        Some(t_root) => {
            let t_chain = sibling_chain(&target.doc, t_root).unwrap_or_default();
            match (t_chain.last(), src_top.first(), src_top.last()) {
                (Some(&t_last), Some(&s_first), Some(&s_last)) => {
                    // 链接：基底末项 /Next → 源首项；源首项 /Prev ← 基底末项。
                    if let Ok(Some(d)) = target
                        .doc
                        .get_object_mut(t_last)
                        .map(|o| o.as_dict_mut().ok())
                    {
                        d.set("Next", Object::Reference(s_first));
                    }
                    if let Ok(Some(d)) = target
                        .doc
                        .get_object_mut(s_first)
                        .map(|o| o.as_dict_mut().ok())
                    {
                        d.set("Prev", Object::Reference(t_last));
                    }
                    // 源顶层项 /Parent → 基底大纲根。
                    for &item in &src_top {
                        if let Ok(Some(d)) = target
                            .doc
                            .get_object_mut(item)
                            .map(|o| o.as_dict_mut().ok())
                        {
                            d.set("Parent", Object::Reference(t_root));
                        }
                    }
                    // 根 /Last 更新 + /Count 重算（全树正计数）。
                    if let Ok(Some(d)) = target
                        .doc
                        .get_object_mut(t_root)
                        .map(|o| o.as_dict_mut().ok())
                    {
                        d.set("Last", Object::Reference(s_last));
                    }
                    let total = outline_total(&target.doc, t_root);
                    set_outline_count(&mut target.doc, t_root, total as i64);
                    // 源大纲根对象本体不可达（顶层已改父）→ 交剪除。
                }
                _ => {
                    // 基底链空（存在但无顶层项）：直接接管源顶层。
                    if let (Some(&s_first), Some(&s_last)) = (src_top.first(), src_top.last()) {
                        if let Ok(Some(d)) = target
                            .doc
                            .get_object_mut(t_root)
                            .map(|o| o.as_dict_mut().ok())
                        {
                            d.set("First", Object::Reference(s_first));
                            d.set("Last", Object::Reference(s_last));
                        }
                        for &item in &src_top {
                            if let Ok(Some(d)) = target
                                .doc
                                .get_object_mut(item)
                                .map(|o| o.as_dict_mut().ok())
                            {
                                d.set("Parent", Object::Reference(t_root));
                            }
                        }
                        let total = outline_total(&target.doc, t_root);
                        set_outline_count(&mut target.doc, t_root, total as i64);
                    }
                }
            }
        }
    }
    Ok(merged)
}

/// 大纲全树项数（顶层链 + 各子树）。
fn outline_total(doc: &Document, outlines_root: ObjectId) -> usize {
    let top = sibling_chain(doc, outlines_root).unwrap_or_default();
    let mut total = top.len();
    for &item in &top {
        total += count_subtree(doc, item);
    }
    total
}

/// 项子树计数（不含自身）。
fn count_subtree(doc: &Document, item: ObjectId) -> usize {
    let kids = match child_items(doc, item) {
        Some(k) => k,
        None => return 0,
    };
    let mut total = kids.len();
    for k in kids {
        total += count_subtree(doc, k);
    }
    total
}

/// 命名目的地拼接：旧式 /Dests 字典（键级合并，基底赢+冲突计数）；
/// /Names 通道（基底已有 /Names → 源树丢弃+计数；基底无 → 整体过继
/// 源 /Names 字典）。返回丢弃数。
fn merge_named_dests(
    target: &mut PdfOrganizer,
    anchors: &SrcAnchors,
    id_map: &BTreeMap<ObjectId, ObjectId>,
) -> Result<usize, CommandError> {
    let mut dropped = 0usize;
    // 通道 1：旧式 /Dests。
    if let Some(src_dests) = &anchors.old_dests {
        let target_dests: Option<Dictionary> =
            match target.doc.catalog().and_then(|c| c.get(b"Dests")) {
                Ok(Object::Dictionary(d)) => Some(d.clone()),
                Ok(Object::Reference(id)) => target
                    .doc
                    .get_object(*id)
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .cloned(),
                _ => None,
            };
        match target_dests {
            None => {
                // 过继。
                let mut d = src_dests.clone();
                rewrite_dict_refs(&mut d, id_map);
                let catalog = target
                    .doc
                    .catalog_mut()
                    .map_err(|e| CommandError::engine(format!("catalog 写入失败：{e}")))?;
                catalog.set("Dests", Object::Dictionary(d));
            }
            Some(mut t_dests) => {
                // 键级合并：缺键补 + 冲突计。
                for (key, value) in src_dests.iter() {
                    if t_dests.get(key).is_ok() {
                        dropped += 1;
                    } else {
                        let mut v = value.clone();
                        rewrite_refs(&mut v, id_map);
                        t_dests.set(key.clone(), v);
                    }
                }
                write_target_dests(&mut target.doc, t_dests)?;
            }
        }
    }
    // 通道 2：/Names（含 /Dests 名称树）。
    if let Some(&src_names_src) = anchors.names.as_ref() {
        let src_names_id = id_map[&src_names_src];
        let target_names: Option<ObjectId> =
            match target.doc.catalog().and_then(|c| c.get(b"Names")) {
                Ok(Object::Reference(id)) => Some(*id),
                _ => None,
            };
        match target_names {
            None => {
                // 过继（整体 /Names 字典——源注入对象已在位）。
                let catalog = target
                    .doc
                    .catalog_mut()
                    .map_err(|e| CommandError::engine(format!("catalog 写入失败：{e}")))?;
                catalog.set("Names", Object::Reference(src_names_id));
            }
            Some(_) => {
                // 基底已有 /Names：源 /Dests 树整体丢弃（计数——不重构
                // 排序树，见模块文档「不做什么」）。
                if let Some(&src_dests_node) = anchors.names_dests.as_ref() {
                    dropped += name_tree_count(&target.doc, id_map[&src_dests_node], 0);
                }
            }
        }
    }
    Ok(dropped)
}

/// 基底旧式 /Dests 写回（维持原形态：直接字典或间接引用）。
fn write_target_dests(doc: &mut Document, dests: Dictionary) -> Result<(), CommandError> {
    match doc.catalog().and_then(|c| c.get(b"Dests")) {
        Ok(Object::Reference(id)) => {
            doc.set_object(*id, Object::Dictionary(dests));
        }
        Ok(Object::Dictionary(_)) => {
            let catalog = doc
                .catalog_mut()
                .map_err(|e| CommandError::engine(format!("catalog 写入失败：{e}")))?;
            catalog.set("Dests", Object::Dictionary(dests));
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 模块测试（纯函数面）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_validation() {
        assert!(PageSize::new(595.0, 842.0).is_ok());
        assert!(PageSize::new(0.0, 100.0).is_err());
        assert!(PageSize::new(-1.0, 100.0).is_err());
        assert!(PageSize::new(f64::NAN, 100.0).is_err());
        assert!(PageSize::new(f64::INFINITY, 100.0).is_err());
        assert_eq!(PageSize::A4.width_pt, 595.0);
        assert_eq!(PageSize::LETTER.height_pt, 792.0);
    }

    #[test]
    fn rotate_math_is_modular() {
        // rotate_page 语义钉定：(cur + deg) % 360（rem_euclid 负角防御）。
        assert_eq!((90i64 + 90).rem_euclid(360), 180);
        assert_eq!((180i64 + 270).rem_euclid(360), 90);
        assert_eq!((270i64 + 90).rem_euclid(360), 0);
    }

    #[test]
    fn permutation_validator_semantics() {
        // reorder 恰排列判据钉定：长度/越界/重复三拒一收。
        let n = 3usize;
        let cases: Vec<(&str, Vec<usize>, bool)> = vec![
            ("长度短", vec![0, 1], false),
            ("越界", vec![0, 1, 3], false),
            ("重复", vec![0, 1, 1], false),
            ("恒等", vec![0, 1, 2], true),
            ("逆序", vec![2, 0, 1], true),
        ];
        for (name, order, expect_ok) in cases {
            let ok_len = order.len() == n;
            let ok_perm =
                order.iter().all(|x| *x < n) && order.iter().collect::<HashSet<_>>().len() == n;
            assert_eq!(ok_len && ok_perm, expect_ok, "case {name}");
        }
    }

    #[test]
    fn report_defaults_all_zero() {
        // 报告零值起点——「无操作 = 无损失」的登记面基线。
        let r = OrganizeReport::default();
        assert_eq!(r.deleted_pages, 0);
        assert_eq!(r.dropped_bookmarks, 0);
        assert_eq!(r.dropped_links, 0);
        assert_eq!(r.dropped_form_fields, 0);
        assert_eq!(r.dropped_named_dests, 0);
        assert_eq!(r.first_save_growth_pct, 0.0);
        assert!(!r.reordered);
        assert!(!r.linearized_dropped);
    }
}
