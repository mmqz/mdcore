//! 表达式白名单求值器（§四.4——"受限确定性公式子集"）。
//!
//! # 白名单 = 文法本身
//!
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := factor (('*' | '/') factor)*
//! factor := '-'? atom
//! atom   := NUMBER | IDENT | '(' expr ')'
//! ```
//!
//! 刻意**没有**的东西（超出即显式错误，绝不猜测）：函数调用、幂、
//! 字符串、比较、位运算、科学记法、千分位、非 ASCII 标识符。四类
//! 目标式全部落在此文法内：数量×单价（`qty * price`）、求和
//! （`a + b + c`）、比例（`part / total * 100`）、单位换算
//! （`amount * 10000`——换算因子是字面量，因子正确性属人工审核面）。
//!
//! # 确定性纪律
//!
//! - 全程 [`Decimal`]（i128 定点），零浮点；
//! - 除法**必须**带目标标度与舍入规则——表达式内除法一律按
//!   [`PrecisionRule`]（记录级登记）执行，无隐式默认精度；
//! - 乘法不隐式舍入：标度和 > 18 = 显式错误（调用方先舍入入参）；
//! - 同输入 + 同表达式 + 同精度 ⇒ 同结果与同轨迹（复算的根基）。
//!
//! # 单位面（诚实边界）
//!
//! 单位传播到子表达式（乘 = `a*b` 复合串、除 = `a/b`），但**仅**
//! 加减法 operands 单位不一致时硬拒（[`EvalError::UnitMismatch`]）。
//! 结果单位与登记单位的对照、单位代数化简（万元·件/件 = 万元）、
//! 换算因子正确性——**不在机器验证面**（无单位本体不发明，§四.3
//! 归人工审核）。
//!
//! # 资源限（fail-closed）
//!
//! 表达式长度、原子数、括号深度、输入数四限——防病态输入，超限
//! 即显式错误（与 SECURITY_POLICY 限额表同纪律）。

use serde::{Deserialize, Serialize};

use crate::calc::{Decimal, DecimalError};
use crate::evidence::{CalcInput, CalculationRecord, PrecisionRule};
use crate::status::{CalcFailure, CalcStatus};

/// 表达式最大字节长。
pub const MAX_EXPRESSION_LEN: usize = 256;
/// 原子（数/变量）数上限。
pub const MAX_ATOMS: usize = 32;
/// 括号嵌套深度上限。
pub const MAX_DEPTH: u8 = 12;
/// 输入清单长度上限。
pub const MAX_INPUTS: usize = 16;

/// 求值产物：最终值 + 逐子步轨迹 + 传播单位（报告面）。
#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutcome {
    /// 按 `precision` 规范形的最终值。
    pub value: Decimal,
    /// 求值轨迹（每个二元/一元子步一行；`CalculationRecord.intermediates`
    /// 的产出源）。
    pub intermediates: Vec<String>,
    /// 传播单位（复合串——报告用，不参与判定）。
    pub computed_unit: Option<String>,
}

/// 求值错误（全部显式分类——报告层可诊断）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    ExpressionTooLong {
        len: usize,
        max: usize,
    },
    TooManyAtoms {
        count: usize,
        max: usize,
    },
    TooManyInputs {
        count: usize,
        max: usize,
    },
    DepthExceeded {
        max: u8,
    },
    UnexpectedChar {
        pos: usize,
        ch: char,
    },
    /// 语法错误（缺操作数/尾随残片/括号不闭合——pos 指向问题位置）。
    Syntax {
        pos: usize,
    },
    UnknownInput {
        name: String,
    },
    DuplicateInput {
        name: String,
    },
    /// 加减法两侧单位不一致（两侧均已声明且不同）。
    UnitMismatch {
        left: String,
        right: String,
    },
    Arithmetic(DecimalError),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::ExpressionTooLong { len, max } => {
                write!(f, "expression too long ({len} > {max} bytes)")
            }
            EvalError::TooManyAtoms { count, max } => {
                write!(f, "too many operands ({count} > {max})")
            }
            EvalError::TooManyInputs { count, max } => {
                write!(f, "too many inputs ({count} > {max})")
            }
            EvalError::DepthExceeded { max } => {
                write!(f, "paren nesting exceeds depth {max}")
            }
            EvalError::UnexpectedChar { pos, ch } => {
                write!(f, "unexpected character {ch:?} at byte {pos}")
            }
            EvalError::Syntax { pos } => {
                write!(f, "syntax error at byte {pos}")
            }
            EvalError::UnknownInput { name } => {
                write!(f, "expression references unknown input '{name}'")
            }
            EvalError::DuplicateInput { name } => {
                write!(f, "duplicate input name '{name}'")
            }
            EvalError::UnitMismatch { left, right } => {
                write!(
                    f,
                    "unit mismatch in additive expression: '{left}' vs '{right}'"
                )
            }
            EvalError::Arithmetic(e) => write!(f, "arithmetic error: {e}"),
        }
    }
}
impl std::error::Error for EvalError {}

// —— 词法 ——

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// 数（值 + 原文——轨迹 repr 用原文）。
    Num(Decimal, String),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize(src: &str) -> Result<Vec<(Tok, usize)>, EvalError> {
    let mut out = vec![];
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'+' => {
                out.push((Tok::Plus, i));
                i += 1;
            }
            b'-' => {
                out.push((Tok::Minus, i));
                i += 1;
            }
            b'*' => {
                out.push((Tok::Star, i));
                i += 1;
            }
            b'/' => {
                out.push((Tok::Slash, i));
                i += 1;
            }
            b'(' => {
                out.push((Tok::LParen, i));
                i += 1;
            }
            b')' => {
                out.push((Tok::RParen, i));
                i += 1;
            }
            b'0'..=b'9' => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    i += 1;
                    let frac_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i == frac_start {
                        return Err(EvalError::UnexpectedChar { pos: i, ch: '.' });
                    }
                }
                let text = &src[start..i];
                let value = Decimal::parse(text).map_err(EvalError::Arithmetic)?;
                out.push((Tok::Num(value, text.to_string()), start));
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push((Tok::Ident(src[start..i].to_string()), start));
            }
            other => {
                return Err(EvalError::UnexpectedChar {
                    pos: i,
                    ch: other as char,
                });
            }
        }
    }
    Ok(out)
}

// —— 语法树 ——

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
        }
    }
}

#[derive(Debug, Clone)]
enum Node {
    Lit {
        value: Decimal,
        text: String,
    },
    Var {
        name: String,
    },
    Neg(Box<Node>),
    Binary {
        op: Op,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
}

/// 解析器状态（深度/原子计数在递归中携带——超限即弃）。
struct Parser<'a> {
    toks: &'a [(Tok, usize)],
    pos: usize,
    atoms: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }
    fn pos_of(&self) -> usize {
        self.toks.get(self.pos).map(|(_, p)| *p).unwrap_or(0)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|(t, _)| t.clone());
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_expr(&mut self, depth: u8) -> Result<Node, EvalError> {
        let mut lhs = self.parse_term(depth)?;
        loop {
            match self.peek() {
                Some(Tok::Plus) => {
                    self.bump();
                    let rhs = self.parse_term(depth)?;
                    lhs = Node::Binary {
                        op: Op::Add,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Tok::Minus) => {
                    self.bump();
                    let rhs = self.parse_term(depth)?;
                    lhs = Node::Binary {
                        op: Op::Sub,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn parse_term(&mut self, depth: u8) -> Result<Node, EvalError> {
        let mut lhs = self.parse_factor(depth)?;
        loop {
            match self.peek() {
                Some(Tok::Star) => {
                    self.bump();
                    let rhs = self.parse_factor(depth)?;
                    lhs = Node::Binary {
                        op: Op::Mul,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Tok::Slash) => {
                    self.bump();
                    let rhs = self.parse_factor(depth)?;
                    lhs = Node::Binary {
                        op: Op::Div,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn parse_factor(&mut self, depth: u8) -> Result<Node, EvalError> {
        if matches!(self.peek(), Some(Tok::Minus)) {
            self.bump();
            let inner = self.parse_atom(depth)?;
            return Ok(Node::Neg(Box::new(inner)));
        }
        self.parse_atom(depth)
    }

    fn parse_atom(&mut self, depth: u8) -> Result<Node, EvalError> {
        if depth > MAX_DEPTH {
            return Err(EvalError::DepthExceeded { max: MAX_DEPTH });
        }
        match self.bump() {
            Some(Tok::Num(v, text)) => {
                self.atoms += 1;
                if self.atoms > MAX_ATOMS {
                    return Err(EvalError::TooManyAtoms {
                        count: self.atoms,
                        max: MAX_ATOMS,
                    });
                }
                Ok(Node::Lit { value: v, text })
            }
            Some(Tok::Ident(name)) => {
                self.atoms += 1;
                if self.atoms > MAX_ATOMS {
                    return Err(EvalError::TooManyAtoms {
                        count: self.atoms,
                        max: MAX_ATOMS,
                    });
                }
                Ok(Node::Var { name })
            }
            Some(Tok::LParen) => {
                let inner = self.parse_expr(depth + 1)?;
                match self.bump() {
                    Some(Tok::RParen) => Ok(inner),
                    _ => Err(EvalError::Syntax { pos: self.pos_of() }),
                }
            }
            Some(Tok::Minus) => {
                // factor 已消费一元负号；此处再遇 Minus = "a - - b" 类
                // 残片——不允许连续一元符号（白名单收紧面）。
                Err(EvalError::Syntax { pos: self.pos_of() })
            }
            Some(_) | None => Err(EvalError::Syntax { pos: self.pos_of() }),
        }
    }
}

/// 输入表（名字唯一性 + 名→值/单位索引）。
struct InputTable<'a> {
    entries: Vec<(&'a str, Decimal, Option<&'a str>)>,
}

impl<'a> InputTable<'a> {
    fn build(inputs: &'a [CalcInput]) -> Result<Self, EvalError> {
        if inputs.len() > MAX_INPUTS {
            return Err(EvalError::TooManyInputs {
                count: inputs.len(),
                max: MAX_INPUTS,
            });
        }
        let mut entries = vec![];
        for inp in inputs {
            if entries.iter().any(|(n, _, _)| *n == inp.name) {
                return Err(EvalError::DuplicateInput {
                    name: inp.name.clone(),
                });
            }
            entries.push((inp.name.as_str(), inp.value, inp.unit.as_deref()));
        }
        Ok(InputTable { entries })
    }

    fn get(&self, name: &str) -> Option<(Decimal, Option<&str>)> {
        self.entries
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, v, u)| (*v, *u))
    }
}

/// 求值节点产物：值 + 轨迹 repr + 传播单位。
struct NodeEval {
    value: Decimal,
    repr: String,
    unit: Option<String>,
}

/// 白名单求值主入口。
///
/// 输入名字必须唯一；表达式引用未登记名 = `UnknownInput`。轨迹每
/// 子步一行（叶子变量用名字、字面量用原文、子表达式用值串）。
pub fn evaluate(
    expression: &str,
    inputs: &[CalcInput],
    precision: &PrecisionRule,
) -> Result<EvalOutcome, EvalError> {
    if expression.len() > MAX_EXPRESSION_LEN {
        return Err(EvalError::ExpressionTooLong {
            len: expression.len(),
            max: MAX_EXPRESSION_LEN,
        });
    }
    let table = InputTable::build(inputs)?;
    let toks = tokenize(expression)?;
    if toks.is_empty() {
        return Err(EvalError::Syntax { pos: 0 });
    }
    let mut parser = Parser {
        toks: &toks,
        pos: 0,
        atoms: 0,
    };
    let ast = parser.parse_expr(0)?;
    if parser.pos != toks.len() {
        // 表达式末尾有残片（如 "a b" / "(a) )"）。
        return Err(EvalError::Syntax {
            pos: parser.pos_of(),
        });
    }
    let mut trace = vec![];
    let root = eval_node(&ast, &table, precision, &mut trace)?;
    let mut value = root.value;
    if value.scale() != precision.target_scale {
        let normalized = value
            .rescale(precision.target_scale, precision.rounding)
            .map_err(EvalError::Arithmetic)?;
        trace.push(format!(
            "rescale({} -> scale {}, {:?}) = {}",
            value, precision.target_scale, precision.rounding, normalized
        ));
        value = normalized;
    }
    Ok(EvalOutcome {
        value,
        intermediates: trace,
        computed_unit: root.unit,
    })
}

fn eval_node(
    node: &Node,
    table: &InputTable,
    precision: &PrecisionRule,
    trace: &mut Vec<String>,
) -> Result<NodeEval, EvalError> {
    match node {
        Node::Lit { value, text } => Ok(NodeEval {
            value: *value,
            repr: text.clone(),
            unit: None,
        }),
        Node::Var { name } => {
            let (value, unit) = table
                .get(name)
                .ok_or_else(|| EvalError::UnknownInput { name: name.clone() })?;
            Ok(NodeEval {
                value,
                repr: name.clone(),
                unit: unit.map(str::to_string),
            })
        }
        Node::Neg(inner) => {
            let child = eval_node(inner, table, precision, trace)?;
            let value = child.value.checked_neg().map_err(EvalError::Arithmetic)?;
            trace.push(format!("-({}) = {}", child.repr, value));
            Ok(NodeEval {
                value,
                repr: value.to_string(),
                unit: child.unit,
            })
        }
        Node::Binary { op, lhs, rhs } => {
            let l = eval_node(lhs, table, precision, trace)?;
            let r = eval_node(rhs, table, precision, trace)?;
            let (value, unit) = match op {
                Op::Add | Op::Sub => {
                    // 单位同类检查（两侧已声明且不同 = 硬拒；None = 通配）。
                    if let (Some(lu), Some(ru)) = (&l.unit, &r.unit) {
                        if lu != ru {
                            return Err(EvalError::UnitMismatch {
                                left: lu.clone(),
                                right: ru.clone(),
                            });
                        }
                    }
                    let v = if *op == Op::Add {
                        l.value.checked_add(r.value)
                    } else {
                        l.value.checked_sub(r.value)
                    }
                    .map_err(EvalError::Arithmetic)?;
                    (v, l.unit.clone().or(r.unit.clone()))
                }
                Op::Mul => {
                    let v = l
                        .value
                        .checked_mul(r.value)
                        .map_err(EvalError::Arithmetic)?;
                    let unit = match (&l.unit, &r.unit) {
                        (Some(a), Some(b)) => Some(format!("{a}*{b}")),
                        (Some(a), None) | (None, Some(a)) => Some(a.clone()),
                        (None, None) => None,
                    };
                    (v, unit)
                }
                Op::Div => {
                    // 表达式内除法一律按记录级精度（目标标度 + 舍入）。
                    let v = l
                        .value
                        .checked_div(r.value, precision.target_scale, precision.rounding)
                        .map_err(EvalError::Arithmetic)?;
                    let unit = match (&l.unit, &r.unit) {
                        (Some(a), Some(b)) => Some(format!("{a}/{b}")),
                        (Some(a), None) => Some(a.clone()),
                        (None, Some(b)) => Some(format!("1/{b}")),
                        (None, None) => None,
                    };
                    (v, unit)
                }
            };
            trace.push(format!("{} {} {} = {}", l.repr, op.symbol(), r.repr, value));
            Ok(NodeEval {
                value,
                repr: value.to_string(),
                unit,
            })
        }
    }
}

// —— 复算 ——

/// 当前输入的提取结果（来源现值或不可用——由调用方从真实来源提取）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CurrentInput {
    /// 来源现值（`source=None` 的人工常量：调用方回填登记值）。
    Available { value: Decimal },
    /// 来源不可达/歧义/已失效——级联诚实性：来源不实不许复算通过。
    Unavailable,
}

/// 复算产物：状态 + 计算值（报告/级联用；失败时 None）。
#[derive(Debug, Clone, PartialEq)]
pub struct RecalcOutcome {
    pub status: CalcStatus,
    /// 求值成功时的计算值（按记录精度规范形）。
    pub computed: Option<Decimal>,
}

/// 复算（记录级——§四.4 "保留输入公式精度可复算"）。
///
/// 五步链的前四步（第五步 = 正文主张值对照，在证据级经
/// `Sidecar::recalculate_calc` 级联）：
/// 1. 位置对应输入全部 `Available`？→ 否 = `InputUnavailable`
///    （长度不齐 = 输入缺失，同码）；
/// 2. 现值 == 登记值（跨标度数值相等）？→ 否 = `InputMismatch`；
/// 3. 表达式可求值（白名单文法 + 算术域）？→ 否 = `EvaluationError`；
/// 4. 计算值 == 登记值（`final_value`）？→ 否 = `PrecisionMismatch`
///    （输入一致 + 求值确定 ⇒ 不一致 = 记录内在失真：精度规则或
///    登记值被改）；`final_value` 为 None（未求值记录）不可复算
///    通过——先 `evaluate` + `apply_evaluation` 落值。
///
/// 刻意不比对 `intermediates`：轨迹是呈现层（版本可演化），权威
/// 完整性锚 = 值；指纹非防篡改（threat model：本地可信存储 + 意外
/// 失效，非对抗篡改——fingerprint 模块文档在档）。
pub fn recalc(record: &CalculationRecord, current: &[CurrentInput]) -> RecalcOutcome {
    use CalcFailure::*;
    let fail = |f: CalcFailure| RecalcOutcome {
        status: CalcStatus::Failed(f),
        computed: None,
    };
    // 1. 输入可用性（长度不齐 = 缺失）。
    if current.len() != record.inputs.len() {
        return fail(InputUnavailable);
    }
    for (reg, cur) in record.inputs.iter().zip(current) {
        match cur {
            CurrentInput::Unavailable => return fail(InputUnavailable),
            CurrentInput::Available { value } => {
                // 2. 现值对照（跨标度数值相等；对齐越界按不符报）。
                let matches = reg.value.numeric_eq(*value).unwrap_or(false);
                if !matches {
                    return fail(InputMismatch);
                }
            }
        }
    }
    // 3. 求值（登记输入 + 登记公式 + 登记精度）。
    let outcome = match evaluate(&record.expression, &record.inputs, &record.precision) {
        Ok(o) => o,
        Err(_) => return fail(EvaluationError),
    };
    // 4. 与登记值对照（None = 未求值记录不可通过）。
    match &record.final_value {
        None => fail(ResultMismatch),
        Some(registered) => {
            let consistent = registered.numeric_eq(outcome.value).unwrap_or(false);
            if !consistent {
                fail(PrecisionMismatch)
            } else {
                RecalcOutcome {
                    status: CalcStatus::Passed,
                    computed: Some(outcome.value),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::Rounding;
    use crate::evidence::{CalcInput, CalculationRecord};

    fn input(name: &str, value: &str, unit: Option<&str>) -> CalcInput {
        CalcInput {
            name: name.into(),
            value: Decimal::parse(value).unwrap(),
            source: None,
            source_doc_id: None,
            unit: unit.map(str::to_string),
        }
    }

    fn prec(scale: u8) -> PrecisionRule {
        PrecisionRule {
            target_scale: scale,
            rounding: Rounding::HalfUp,
        }
    }

    fn calc_record(
        inputs: Vec<CalcInput>,
        expression: &str,
        precision: PrecisionRule,
    ) -> CalculationRecord {
        let outcome = evaluate(expression, &inputs, &precision).unwrap();
        CalculationRecord {
            calc_id: "c1".into(),
            inputs,
            expression: expression.into(),
            unit: None,
            precision,
            intermediates: outcome.intermediates,
            final_value: Some(outcome.value),
            calc_status: CalcStatus::Passed,
        }
    }

    fn avail(v: &str) -> CurrentInput {
        CurrentInput::Available {
            value: Decimal::parse(v).unwrap(),
        }
    }

    // —— 四式覆盖 ——

    #[test]
    fn form_quantity_times_price() {
        let inputs = vec![
            input("qty", "12.5", Some("件")),
            input("price", "336.80", Some("万元")),
        ];
        let out = evaluate("qty * price", &inputs, &prec(2)).unwrap();
        assert_eq!(out.value.to_string(), "4210.00");
        assert_eq!(out.computed_unit.as_deref(), Some("件*万元"));
        // 乘法子步 + rescale 轨迹行（3 位 → 2 位）
        assert_eq!(
            out.intermediates,
            vec![
                "qty * price = 4210.000".to_string(),
                "rescale(4210.000 -> scale 2, HalfUp) = 4210.00".to_string(),
            ]
        );
    }

    #[test]
    fn form_sum_chain() {
        let inputs = vec![
            input("a", "1000.000", None),
            input("b", "4210.000", None),
            input("c", "0.500", None),
        ];
        let out = evaluate("a + b + c", &inputs, &prec(3)).unwrap();
        assert_eq!(out.value.to_string(), "5210.500");
        assert_eq!(out.intermediates.len(), 2, "链式两步");
        assert_eq!(out.intermediates[0], "a + b = 5210.000");
        assert_eq!(out.intermediates[1], "5210.000 + c = 5210.500");
    }

    #[test]
    fn form_ratio_percent() {
        // 4210.00 / 5210.50 * 100 = 80.79…%（precision 2——除法用记录级精度）
        let inputs = vec![
            input("part", "4210.00", None),
            input("total", "5210.50", None),
        ];
        let out = evaluate("part / total * 100", &inputs, &prec(2)).unwrap();
        // 4210.00/5210.50 = 0.807904… → scale2 = 0.81 → *100 = 81.00
        assert_eq!(out.value.to_string(), "81.00");
        assert_eq!(out.intermediates[0], "part / total = 0.81");
        assert_eq!(out.intermediates[1], "0.81 * 100 = 81.00");
        // 对照：precision 6 时除法保 6 位——确定性随精度规则走
        // （4210.00/5210.50 = 0.8079838… → scale6 = 0.807984 → ×100）
        let out6 = evaluate("part / total * 100", &inputs, &prec(6)).unwrap();
        assert_eq!(out6.value.to_string(), "80.798400");
    }

    #[test]
    fn form_unit_conversion_factor_literal() {
        // 万元 → 元：换算因子 = 字面量；因子正确性属人工审核面（文档纪律）
        let inputs = vec![input("amount", "0.4210", Some("万元"))];
        let out = evaluate("amount * 10000", &inputs, &prec(2)).unwrap();
        assert_eq!(out.value.to_string(), "4210.00");
        // 字面量无量纲 → 传播单位 = amount 自身（万元）；不与记录声明单位
        // 对照（单位代数不在机器验证面——诚实边界）
        assert_eq!(out.computed_unit.as_deref(), Some("万元"));
    }

    // —— 文法 ——

    #[test]
    fn precedence_parens_and_unary_minus() {
        let two = vec![input("x", "2", None)];
        assert_eq!(
            evaluate("2 + 3 * 4", &two, &prec(0))
                .unwrap()
                .value
                .to_string(),
            "14"
        );
        assert_eq!(
            evaluate("(2 + 3) * 4", &two, &prec(0))
                .unwrap()
                .value
                .to_string(),
            "20"
        );
        let a = vec![input("a", "5", None), input("b", "3", None)];
        let out = evaluate("-(a + b)", &a, &prec(0)).unwrap();
        assert_eq!(out.value.to_string(), "-8");
        assert_eq!(out.intermediates, vec!["a + b = 8", "-(8) = -8"]);
        // 一元负号不连续（白名单收紧）
        assert!(evaluate("--a", &a, &prec(0)).is_err());
        // 字面量前导负号（一元）
        assert_eq!(
            evaluate("-3", &a, &prec(0)).unwrap().value.to_string(),
            "-3"
        );
    }

    #[test]
    fn whitespace_tolerant() {
        let inputs = vec![input("qty", "2", None), input("price", "3", None)];
        assert_eq!(
            evaluate("qty*price", &inputs, &prec(0))
                .unwrap()
                .value
                .to_string(),
            "6"
        );
        assert_eq!(
            evaluate("  qty  *  price  ", &inputs, &prec(0))
                .unwrap()
                .value
                .to_string(),
            "6"
        );
    }

    // —— 拒绝面 ——

    #[test]
    fn syntax_and_char_errors() {
        let inputs = vec![input("a", "1", None)];
        let cases = [
            "",          // 空
            "a +",       // 尾随运算符
            "(a",        // 括号不闭合
            "a)",        // 多余闭括
            "a b",       // 相邻两原子
            "a $ b",     // 非法字符
            "1.2.3",     // 双小数点
            "1.",        // 小数点后无数字
            "a + + b",   // 连续运算符
            "sum(a, b)", // 函数调用（白名单外）
            "a; b",      // 分号
        ];
        for c in cases {
            assert!(evaluate(c, &inputs, &prec(2)).is_err(), "应拒绝: {c:?}");
        }
        // 科学记法经词法层拒绝（e 是标识符起始 → UnknownInput 或语法错）
        assert!(evaluate("1e5", &inputs, &prec(2)).is_err());
    }

    #[test]
    fn unknown_and_duplicate_inputs() {
        let inputs = vec![input("a", "1", None)];
        match evaluate("b + 1", &inputs, &prec(0)).unwrap_err() {
            EvalError::UnknownInput { name } => assert_eq!(name, "b"),
            e => panic!("期望 UnknownInput，得到 {e:?}"),
        }
        let dup = vec![input("a", "1", None), input("a", "2", None)];
        assert!(matches!(
            evaluate("a", &dup, &prec(0)).unwrap_err(),
            EvalError::DuplicateInput { .. }
        ));
        // 未引用的登记输入：合法（表达式语义不含它——文档纪律）
        assert_eq!(
            evaluate(
                "a",
                &[input("a", "1", None), input("unused", "9", None)],
                &prec(0)
            )
            .unwrap()
            .value
            .to_string(),
            "1"
        );
    }

    #[test]
    fn limits_enforced() {
        let a = vec![input("a", "1", None)];
        // 长度
        let long = "a + ".repeat(80);
        assert!(matches!(
            evaluate(&long, &a, &prec(0)).unwrap_err(),
            EvalError::ExpressionTooLong { .. }
        ));
        // 原子数（33 个）
        let many = format!("{}{}", "a + ".repeat(32), "a");
        assert!(matches!(
            evaluate(&many, &a, &prec(0)).unwrap_err(),
            EvalError::TooManyAtoms { .. }
        ));
        // 32 个恰好过
        let edge = format!("{}{}", "a + ".repeat(31), "a");
        assert!(evaluate(&edge, &a, &prec(0)).is_ok());
        // 深度（13 层括号）
        let deep = format!("{}a{}", "(".repeat(13), ")".repeat(13));
        assert!(matches!(
            evaluate(&deep, &a, &prec(0)).unwrap_err(),
            EvalError::DepthExceeded { .. }
        ));
        // 12 层恰好过
        let ok_depth = format!("{}a{}", "(".repeat(12), ")".repeat(12));
        assert!(evaluate(&ok_depth, &a, &prec(0)).is_ok());
        // 输入数（17 个）
        let many_inputs: Vec<CalcInput> = (0..17)
            .map(|i| input(&format!("v{i}"), "1", None))
            .collect();
        assert!(matches!(
            evaluate("v0", &many_inputs, &prec(0)).unwrap_err(),
            EvalError::TooManyInputs { .. }
        ));
    }

    #[test]
    fn arithmetic_errors_explicit() {
        let inputs = vec![input("a", "1", None), input("b", "0", None)];
        // 除零
        assert!(matches!(
            evaluate("a / b", &inputs, &prec(2)).unwrap_err(),
            EvalError::Arithmetic(DecimalError::DivideByZero)
        ));
        // 乘法标度和 > 18（不隐式舍入）
        let big = vec![
            input("x", "0.000000000000000001", None), // scale 18
            input("y", "0.1", None),                  // scale 1
        ];
        assert!(matches!(
            evaluate("x * y", &big, &prec(2)).unwrap_err(),
            EvalError::Arithmetic(DecimalError::Domain)
        ));
        // 溢出
        let huge = vec![input("h", "170141183460469231731687303715884105727", None)];
        assert!(matches!(
            evaluate("h + h", &huge, &prec(0)).unwrap_err(),
            EvalError::Arithmetic(DecimalError::Overflow)
        ));
    }

    // —— 单位面 ——

    #[test]
    fn unit_mismatch_on_additive_rejected() {
        let mixed = vec![
            input("amount", "100", Some("万元")),
            input("count", "3", Some("件")),
        ];
        assert!(matches!(
            evaluate("amount + count", &mixed, &prec(2)).unwrap_err(),
            EvalError::UnitMismatch { .. }
        ));
        assert!(matches!(
            evaluate("amount - count", &mixed, &prec(2)).unwrap_err(),
            EvalError::UnitMismatch { .. }
        ));
        // 同单位过；None 通配过
        let same = vec![
            input("a", "100", Some("万元")),
            input("b", "3", Some("万元")),
            input("c", "5", None),
        ];
        assert!(evaluate("a + b", &same, &prec(2)).is_ok());
        assert!(evaluate("a + c", &same, &prec(2)).is_ok());
        // 乘除混合单位不拒（复合维——报告面）
        assert!(evaluate("amount * count", &mixed, &prec(2)).is_ok());
    }

    #[test]
    fn unit_propagation_reporting() {
        let inputs = vec![
            input("qty", "2", Some("件")),
            input("price", "100", Some("万元")),
            input("total", "4", Some("万元")),
        ];
        let out = evaluate("qty * price / total", &inputs, &prec(2)).unwrap();
        assert_eq!(out.computed_unit.as_deref(), Some("件*万元/万元"));
        // 除法单侧：无量纲分子
        let out2 = evaluate("qty / total", &inputs, &prec(2)).unwrap();
        assert_eq!(out2.computed_unit.as_deref(), Some("件/万元"));
    }

    // —— 复算 ——

    #[test]
    fn recalc_pass_path() {
        let rec = calc_record(
            vec![input("qty", "12.5", None), input("price", "336.80", None)],
            "qty * price",
            prec(2),
        );
        let out = recalc(&rec, &[avail("12.5"), avail("336.80")]);
        assert_eq!(out.status, CalcStatus::Passed);
        assert_eq!(out.computed.unwrap().to_string(), "4210.00");
    }

    #[test]
    fn recalc_input_unavailable() {
        let rec = calc_record(vec![input("a", "1", None)], "a", prec(0));
        // 显式不可用
        let out = recalc(&rec, &[CurrentInput::Unavailable]);
        assert_eq!(
            out.status,
            CalcStatus::Failed(CalcFailure::InputUnavailable)
        );
        // 长度不齐 = 缺失
        let out2 = recalc(&rec, &[]);
        assert_eq!(
            out2.status,
            CalcStatus::Failed(CalcFailure::InputUnavailable)
        );
    }

    #[test]
    fn recalc_input_mismatch() {
        let rec = calc_record(vec![input("a", "1", None)], "a", prec(0));
        let out = recalc(&rec, &[avail("2")]);
        assert_eq!(out.status, CalcStatus::Failed(CalcFailure::InputMismatch));
        // 跨标度数值相等 = 不算不符（1 == 1.00）
        let out2 = recalc(&rec, &[avail("1.00")]);
        assert_eq!(out2.status, CalcStatus::Passed);
    }

    #[test]
    fn recalc_evaluation_error() {
        // 登记后公式被改成除零形（或公式域错）→ 求值失败
        let mut rec = calc_record(vec![input("a", "1", None)], "a", prec(0));
        rec.expression = "a / a - a".into(); // 1/1-1 = 0 … 再造除零：
        rec.expression = "a / (a - a)".into();
        let out = recalc(&rec, &[avail("1")]);
        assert_eq!(out.status, CalcStatus::Failed(CalcFailure::EvaluationError));
    }

    #[test]
    fn recalc_precision_mismatch_on_tamper() {
        let mut rec = calc_record(
            vec![input("a", "10", None)],
            "a / 3",
            prec(2), // 3.33
        );
        assert_eq!(rec.final_value.unwrap().to_string(), "3.33");
        // 篡改登记值 → 输入一致但复算值 ≠ 登记值
        rec.final_value = Some(Decimal::parse("3.34").unwrap());
        let out = recalc(&rec, &[avail("10")]);
        assert_eq!(
            out.status,
            CalcStatus::Failed(CalcFailure::PrecisionMismatch)
        );
        // 精度规则被改（scale 2 → 4）：复算按新规则 ≠ 登记值
        let mut rec2 = calc_record(vec![input("a", "10", None)], "a / 3", prec(2));
        rec2.precision = prec(4);
        let out2 = recalc(&rec2, &[avail("10")]);
        assert_eq!(
            out2.status,
            CalcStatus::Failed(CalcFailure::PrecisionMismatch)
        );
    }

    #[test]
    fn recalc_final_value_none_cannot_pass() {
        let mut rec = calc_record(vec![input("a", "1", None)], "a", prec(0));
        rec.final_value = None;
        let out = recalc(&rec, &[avail("1")]);
        assert_eq!(
            out.status,
            CalcStatus::Failed(CalcFailure::ResultMismatch),
            "未求值记录不可复算通过——先 evaluate 落值"
        );
    }

    #[test]
    fn determinism_same_inputs_same_trace() {
        let inputs = vec![
            input("qty", "12.5", Some("件")),
            input("price", "336.80", Some("万元")),
            input("fee", "0.25", None),
        ];
        let e1 = evaluate("(qty * price) + fee", &inputs, &prec(2)).unwrap();
        let e2 = evaluate("(qty * price) + fee", &inputs, &prec(2)).unwrap();
        assert_eq!(e1, e2, "同输入同表达式同精度 ⇒ 全等（值+轨迹+单位）");
        assert_eq!(e1.value.to_string(), "4210.25");
        assert_eq!(e1.intermediates.len(), 3);
    }
}
