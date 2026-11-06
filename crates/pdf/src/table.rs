//! 表格结构化提取（读面，#232 / R9·P1）。
//!
//! # 背景（issue #232 证据）
//!
//! 真实政采评分表/参数表在 PDF 线性化（`extract_text`）后列交错，下游
//! 启发式拼装不可持续（研究仓 #252 被迫写 785 行 `scoring_table` 专路仍
//! 16 锚点仅 4 命中）——结构化提取应在引擎侧一次做对。
//!
//! # 架构（分层——与既有读面同构）
//!
//! ```text
//! 位置层（引擎）                算法层（本模块，纯函数、零引擎依赖）
//! engine_pdfium::               table::extract_tables(&[PositionedChar])
//! positioned_chars()  ────────►  行带聚类 → 行内切格 → 连续多格行成表
//! （feature engine-pdfium；      → 列左缘锚聚类 → 单元格落列
//!  lopdf 位置层不在 v1——        → Table { rows: [[cell{文本,
//!  内容流字体度量解码重，               col_index, col_span, 包围盒}]] }
//!  登记见下「边界」）
//! ```
//!
//! 算法层与引擎解耦（`PositionedChar` 可注入——单测零 pdfium 依赖，
//! 与会话注入几何同哲学）；pdfium 位置实装见
//! `engine_pdfium::PdfiumEngine::positioned_chars`。
//!
//! # 语义
//!
//! - **行**：字符按垂直中心聚类（容差 = 0.5 × 中位字高，下限 1.5pt）；
//!   行序 = 自上而下（PDF 坐标 y 降序）。
//! - **单元格**：行内字符按 x 间隙切分（阈值 = max(4pt, 0.9 × 中位
//!   字高)）；行内按左缘升序。
//! - **成表**：**连续**多格（≥2 格）行构成一张表；单格行（标题/段落）
//!   不入表——行间断（垂直间隙 > 3 × 中位字高）或单格行截断表块。
//! - **列序**：全表单元格**左缘** 1D 聚类为列锚（容差 =
//!   max(6pt, 0.2 × 中位格宽)）；单元格 `col_index` = 起始列锚序号
//!   （左缘所在列），`col_span` = 覆盖的列锚数（合并格 ≥2）。
//! - **包围盒**：pt，PDF 用户空间（原点左下，bottom < top）——UI 选区/
//!   对账可直接消费。
//!
//! # 边界（v1 登记——不静默）
//!
//! - 对齐式表格（含无框线）——**矢量线框（ruling lines）检测不在 v1**
//!   （pdfium 页面对象路径面，语料实证后按需扩展）；
//! - 列锚以左缘聚类：左对齐列精确；**居中/右对齐列的 col_index 可能
//!   偏移**（真实语料验证留后续——issue 允许合成金标准先行）；
//! - 跨页表续接、嵌套表、旋转页（/Rotate ≠ 0）不在 v1；
//! - 单元格内多行文本（软换行）会拆成多行行带——v1 按独立行处理
//!   （重建时不影响列序，行数语义登记）。

use std::cmp::Ordering;

/// 位置字符（引擎位置层产物——pdfium 实装见
/// [`crate::engine_pdfium::PdfiumEngine::positioned_chars`]）。
///
/// 坐标 = PDF 用户空间 pt（原点左下）：`left < right`、`bottom < top`。
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedChar {
    pub ch: char,
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
    /// 字号（pt，缩放后实际值）——行带聚类参考。
    pub font_size: f32,
}

impl PositionedChar {
    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.top - self.bottom
    }

    fn center_y(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

/// 表格单元格：文本 + 列序（`col_index`/`col_span`）+ 包围盒。
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    /// 单元格文本（字符按左缘序拼接；间隙不插空格——v1 语义）。
    pub text: String,
    /// 起始列序号（0 基，列锚自左向右）。
    pub col_index: usize,
    /// 覆盖列锚数（合并格 ≥ 2）。
    pub col_span: usize,
    /// 包围盒（pt，PDF 用户空间）。
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

/// 表格行（单元格按左缘升序；行序自上而下由 [`Table::rows`] 承载）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

/// 一张表：行集合（自上而下）+ 全表包围盒。
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// 行自上而下（PDF y 降序）。
    pub rows: Vec<TableRow>,
    /// 全表包围盒（所有单元格并集；pt，PDF 用户空间）。
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

impl Table {
    /// 行数（自上而下计）。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 列数 = 列锚数（表内所有行覆盖的最大列序 + 1 的最大值——由
    /// `col_index + col_span` 上界取最大；空表 = 0）。
    pub fn column_count(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|r| r.cells.iter())
            .map(|c| c.col_index + c.col_span)
            .max()
            .unwrap_or(0)
    }
}

/// 中位数（空输入 = 0.0）。
fn median(mut xs: Vec<f32>) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    xs[xs.len() / 2]
}

/// 行带（聚类中间态）：字符按左缘升序。
struct LineBand {
    chars: Vec<PositionedChar>,
}

impl LineBand {
    fn top(&self) -> f32 {
        self.chars.iter().map(|c| c.top).fold(f32::MIN, f32::max)
    }

    fn bottom(&self) -> f32 {
        self.chars.iter().map(|c| c.bottom).fold(f32::MAX, f32::min)
    }

    fn center_y(&self) -> f32 {
        self.chars.iter().map(PositionedChar::center_y).sum::<f32>() / self.chars.len() as f32
    }
}

/// 表格结构化提取（纯算法——见模块文档「语义」节）。
///
/// 输入为空 / 全零尺寸字符 → 空表（Ok 语义，非错误）。
pub fn extract_tables(chars: &[PositionedChar]) -> Vec<Table> {
    // 0) 卫生过滤：零宽/零高字符（空白符的空包围盒等）不参与几何，
    //    但保留其文本贡献为零的语义（v1 直接丢弃——空格对齐在
    //    x 间隙切分中由相邻字符间隙体现）。
    let valid: Vec<&PositionedChar> = chars
        .iter()
        .filter(|c| c.width() > 0.05 && c.height() > 0.05)
        .collect();
    if valid.is_empty() {
        return Vec::new();
    }

    let h_med = median(valid.iter().map(|c| c.height()).collect());
    let line_tol = (0.5 * h_med).max(1.5);
    let cell_gap = (0.9 * h_med).max(4.0);

    // 1) 行带聚类：按垂直中心自上而下（y 降序），容差内同带。
    let mut sorted: Vec<&PositionedChar> = valid.clone();
    sorted.sort_by(|a, b| {
        b.center_y()
            .partial_cmp(&a.center_y())
            .unwrap_or(Ordering::Equal)
            .then(a.left.partial_cmp(&b.left).unwrap_or(Ordering::Equal))
    });
    let mut lines: Vec<LineBand> = Vec::new();
    for ch in sorted {
        match lines.last_mut() {
            Some(band) if (band.center_y() - ch.center_y()).abs() <= line_tol => {
                band.chars.push(ch.clone());
            }
            _ => lines.push(LineBand {
                chars: vec![ch.clone()],
            }),
        }
    }

    // 行带字符按左缘升序（聚类按 center_y 进栈，左序不保证——单元
    // 切分以左序游走；#232 实施发现：不排序时真实 pdfium 阅读序字符流
    // 会切出交错垃圾格，合成注入序碰巧左序故纯函数测试未暴露）。
    for band in &mut lines {
        band.chars
            .sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap_or(Ordering::Equal));
    }

    // 2) 行内切格 + 3) 连续多格行成表。
    //    每行先切格（RawCell），≥2 格的行是表行候选；连续性 = 与前
    //    一表行的垂直间隙 ≤ 3 × 中位字高（断档 = 新表）。
    struct RawCell {
        text: String,
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
    }
    struct RowCells {
        cells: Vec<RawCell>,
        top: f32,
        bottom: f32,
    }

    let mut all_rows: Vec<RowCells> = Vec::new();
    for band in &lines {
        let mut cells: Vec<RawCell> = Vec::new();
        let mut cur_text = String::new();
        let (mut l, mut r, mut b, mut t) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for ch in &band.chars {
            let starts_new = !cur_text.is_empty() && ch.left - r > cell_gap;
            if starts_new {
                cells.push(RawCell {
                    text: std::mem::take(&mut cur_text),
                    left: l,
                    right: r,
                    bottom: b,
                    top: t,
                });
                l = f32::MAX;
                r = f32::MIN;
                b = f32::MAX;
                t = f32::MIN;
            }
            cur_text.push(ch.ch);
            l = l.min(ch.left);
            r = r.max(ch.right);
            b = b.min(ch.bottom);
            t = t.max(ch.top);
        }
        if !cur_text.is_empty() {
            cells.push(RawCell {
                text: cur_text,
                left: l,
                right: r,
                bottom: b,
                top: t,
            });
        }
        if cells.len() >= 2 {
            all_rows.push(RowCells {
                top: band.top(),
                bottom: band.bottom(),
                cells,
            });
        }
    }

    // 表块切分：相邻表行垂直断档 > 3 × 中位字高 → 新表。
    let break_gap = 3.0 * h_med;
    let mut blocks: Vec<Vec<RowCells>> = Vec::new();
    for row in all_rows {
        match blocks.last_mut() {
            Some(block) => {
                let prev_bottom = block.last().map(|r| r.bottom).unwrap_or(f32::MAX);
                if prev_bottom - row.top > break_gap {
                    blocks.push(vec![row]);
                } else {
                    block.push(row);
                }
            }
            None => blocks.push(vec![row]),
        }
    }

    // 4) 列锚（单元格左缘 1D 聚类）+ 5) 落列。
    let mut tables = Vec::new();
    for block in blocks {
        let lefts: Vec<f32> = block
            .iter()
            .flat_map(|r| r.cells.iter().map(|c| c.left))
            .collect();
        let w_med = median(
            block
                .iter()
                .flat_map(|r| r.cells.iter().map(|c| c.right - c.left))
                .collect(),
        );
        let anchor_tol = (0.2 * w_med).max(6.0);
        let mut sorted_lefts = lefts.clone();
        sorted_lefts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        let mut clusters: Vec<(f32, usize)> = Vec::new();
        for x in sorted_lefts {
            match clusters.last_mut() {
                Some((pos, count)) if x - *pos <= anchor_tol => {
                    *pos = (*pos + x) / 2.0;
                    *count += 1;
                }
                _ => clusters.push((x, 1)),
            }
        }
        let eps = 3.0;

        // 幽灵锚防御：跨列格（span 在其它簇位置之内）的左缘**不是**列起点
        // ——若某簇的全部支撑者都是跨列格，该簇为合并格中途起笔产生的
        // 幽灵锚，剔除（全簇皆跨列的退化表回退为全锚——防御性）。
        let own_cluster = |left: f32| -> usize {
            let mut best = 0usize;
            let mut best_d = f32::MAX;
            for (i, (pos, _)) in clusters.iter().enumerate() {
                let d = (pos - left).abs();
                if d < best_d {
                    best_d = d;
                    best = i;
                }
            }
            best
        };
        let is_spanner = |left: f32, right: f32, own: usize| -> bool {
            clusters
                .iter()
                .enumerate()
                .any(|(i, (pos, _))| i != own && *pos > left + eps && *pos <= right - eps)
        };
        let mut has_non_spanner = vec![false; clusters.len()];
        for row in &block {
            for cell in &row.cells {
                let own = own_cluster(cell.left);
                if !is_spanner(cell.left, cell.right, own) {
                    has_non_spanner[own] = true;
                }
            }
        }
        let anchors: Vec<f32> = {
            let strong: Vec<f32> = clusters
                .iter()
                .zip(&has_non_spanner)
                .filter(|(_, keep)| **keep)
                .map(|((pos, _), _)| *pos)
                .collect();
            if strong.is_empty() {
                clusters.iter().map(|(pos, _)| *pos).collect()
            } else {
                strong
            }
        };

        // 表 bbox 纵向界取自块内首/末行带（先于 block 消费捕获）。
        let top = block.first().map(|r| r.top).unwrap_or_default();
        let bottom = block.last().map(|r| r.bottom).unwrap_or_default();
        let mut rows = Vec::with_capacity(block.len());
        for row in block {
            let mut cells = Vec::with_capacity(row.cells.len());
            for cell in row.cells {
                // 起始列 = 满足 anchor ≤ left + eps 的最大锚序号
                //（左缘在首锚之左 → 列 0，防御性钳制）。
                let mut col_index = 0usize;
                for (i, a) in anchors.iter().enumerate() {
                    if *a <= cell.left + eps {
                        col_index = i;
                    }
                }
                // 跨列 = 覆盖的列锚数（(left+eps, right-eps] 内的锚）+ 1。
                let covered = anchors
                    .iter()
                    .filter(|a| **a > cell.left + eps && **a <= cell.right - eps)
                    .count();
                cells.push(TableCell {
                    text: cell.text,
                    col_index,
                    col_span: covered + 1,
                    left: cell.left,
                    bottom: cell.bottom,
                    right: cell.right,
                    top: cell.top,
                });
            }
            rows.push(TableRow { cells });
        }

        let left = rows
            .iter()
            .flat_map(|r| r.cells.iter().map(|c| c.left))
            .fold(f32::MAX, f32::min);
        let right = rows
            .iter()
            .flat_map(|r| r.cells.iter().map(|c| c.right))
            .fold(f32::MIN, f32::max);
        tables.push(Table {
            rows,
            left,
            bottom,
            right,
            top,
        });
    }
    tables
}
