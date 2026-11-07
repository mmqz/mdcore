//! z 序引擎（#402）——fractional indexing。
//!
//! 键 = **字符串**，字典序即 z 序：`"Zz" < "a0" < "a1" < "a1V" < "a2"`。
//! 三家共识路线（Figma / Excalidraw / tldraw——issue #402 指定）：
//!
//! - **O(1) 重排**：任意两邻键之间总能生成新键（无数组序的 O(N) 写放大）；
//! - **CRDT 友好**：键是对象原子属性，yrs Map 键级 LWW 即可合并；
//! - **长度可控**：append/prepend 走整数步进（键长摊还常数），中点
//!   插入每步至多摊还一位（62 进制空间对半分）。
//!
//! 算法出处：rocicorp/fractional-indexing（**CC0**，issue 指定 vendor
//! 路线）的 clean-room Rust 移植——数学同源（David Greenspan
//! 《Implementing Fractional Indexing》），含 prepend/append 长度优化。
//!
//! # 键型（固定字母表）
//!
//! ```text
//! 小数位（base-62，字符码升序）："0-9 A-Z a-z"
//! 整数头（base-52）            ：大写 A-Z = 负长度档 / 小写 a-z = 正长度档
//! 键 = <整数头><整数位…><小数…>（无分隔符；小数可空、禁尾零）
//! ```
//!
//! 整数头编码整数部分的**总长**：`'a'`→2（`"a0"`=0 … `"az"`=61）、
//! `'b'`→3（`"b00"`=62 起）、… `'z'`→27；`'Z'`→2（负侧）、`'A'`→27
//! （最负）。字典序上「头更小 ⇒ 整数更小」由字母表升序保证。
//!
//! # 并发语义（如实登记）
//!
//! 纯确定性算法在**两副本于同一对邻键间插入**时会生成相同键——这是
//! 确定性合并的性质而非缺陷（jitter 变体用随机扰动换撞号率，代价是
//! 不可复现）。消费侧以 `(z_index, id)` 决胜获得全序：合并后两副本
//! 排序一致（验收 3 的「收敛」语义），撞号由 ID 决胜吸收。
//!
//! # 非法输入（敌意载体是常态）
//!
//! 键可能来自对端写入的任意字符串。所有入口返回 [`ZOrderError`]，
//! 不 panic；排序面只做字典序比较（任意字符串全序比较总是安全），
//! 不校验键型。

/// 小数位字母表（base-62，字符码升序——字典序 = 数值序的前提）。
const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// 整数头字母表（base-52）：前半（A-Z）负长度档、后半（a-z）正长度
/// 档；头越靠两端整数越长。
const INT_DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 零位（`'0'`——最小小数位）。
const ZERO: u8 = DIGITS[0];
/// 最大小数位（`'z'`）。
const LAST: u8 = DIGITS[62 - 1];
/// 最小整数（`'A'` + 26 个 `'0'`——合法键的下界，本身不允许作为键）。
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";

/// 缺省键（空序列首键 / 存量对象无 z 字段的读侧兜底）。
pub const DEFAULT_KEY: &str = "a0";

/// z 序运算错误（重排命令映射到 `CanvasError::InvalidInput`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZOrderError {
    /// 键含字母表外字符 / 尾零 / 整数头与长度不符 / 界相等不可再分。
    InvalidKey(String),
    /// 键已到表示边界（仅理论极值可达——头到字母表末端）。
    Exhausted,
}

impl std::fmt::Display for ZOrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZOrderError::InvalidKey(k) => write!(f, "invalid order key: {k:?}"),
            ZOrderError::Exhausted => write!(f, "order key space exhausted"),
        }
    }
}

impl std::error::Error for ZOrderError {}

// ── 字位工具 ───────────────────────────────────────────────────

fn digit_value(c: u8) -> Option<usize> {
    DIGITS.iter().position(|&d| d == c)
}

fn int_digit_value(c: u8) -> Option<usize> {
    INT_DIGITS.iter().position(|&d| d == c)
}

fn char_of(b: u8) -> char {
    b as char
}

/// 整数头声明的整数部分**总长**（含头）。'a'→2、'b'→3、…、'z'→27；
/// 'Z'→2、'Y'→3、…、'A'→27（负侧对称）。
fn integer_length(head: u8) -> Result<usize, ZOrderError> {
    let i = int_digit_value(head)
        .ok_or_else(|| ZOrderError::InvalidKey(format!("bad integer head {:?}", char_of(head))))?;
    let half = INT_DIGITS.len() / 2;
    Ok(if i < half { half - i + 1 } else { i - half + 2 })
}

/// 键的整数部分（含头）。键比头声明的长度短 = 非法。
fn integer_part(key: &str) -> Result<&str, ZOrderError> {
    let head = key
        .as_bytes()
        .first()
        .copied()
        .ok_or_else(|| ZOrderError::InvalidKey("empty key".into()))?;
    let len = integer_length(head)?;
    if len > key.len() {
        return Err(ZOrderError::InvalidKey(format!(
            "key shorter than its head declares: {key:?}"
        )));
    }
    Ok(&key[..len])
}

/// 完整键校验：非空、头合法、整数位全在字母表、**小数**无尾零、
/// 非最小整数。（尾零规则只对小数——整数 "a0" 的 '0' 是整数位。）
fn validate_key(key: &str) -> Result<(), ZOrderError> {
    if key == SMALLEST_INTEGER {
        return Err(ZOrderError::InvalidKey(format!(
            "smallest integer is reserved, not a valid key: {key:?}"
        )));
    }
    let ip = integer_part(key)?;
    for &c in ip.as_bytes() {
        digit_value(c).ok_or_else(|| {
            ZOrderError::InvalidKey(format!(
                "char {:?} not in digit alphabet (key {key:?})",
                char_of(c)
            ))
        })?;
    }
    let fraction = &key[ip.len()..];
    if fraction.as_bytes().last() == Some(&ZERO) {
        return Err(ZOrderError::InvalidKey(format!(
            "trailing zero in fraction (key {key:?})"
        )));
    }
    Ok(())
}

// ── 整数步进（append/prepend 优化路径） ─────────────────────────

/// 整数部分 + 1（整体步进）。头已到最大档 → `Exhausted`。
fn increment_integer(x: &str) -> Result<String, ZOrderError> {
    let bytes = x.as_bytes();
    let head = bytes[0];
    // 从右往左找第一个可 +1 的位；途经满位（'z'）转为进位（补零）
    let mut i = bytes.len();
    while i > 1 {
        i -= 1;
        let d = digit_value(bytes[i]).ok_or_else(|| ZOrderError::InvalidKey(x.to_string()))? + 1;
        if d == DIGITS.len() {
            continue; // 满位进位
        }
        let mut out = Vec::with_capacity(bytes.len());
        out.push(head);
        out.extend_from_slice(&bytes[1..i]);
        out.push(DIGITS[d]);
        // i 之后的全是满位 → 全部归零
        out.extend(std::iter::repeat_n(ZERO, bytes.len() - i - 1));
        return Ok(String::from_utf8(out).expect("字母表字节"));
    }
    // 整数位全满 → 头步进，长度跟随（例 "az"→"b00"、"bzz"→"c000"）
    let hi = int_digit_value(head).ok_or_else(|| ZOrderError::InvalidKey(x.to_string()))?;
    if hi == INT_DIGITS.len() - 1 {
        return Err(ZOrderError::Exhausted);
    }
    let new_head = INT_DIGITS[hi + 1];
    let delta = integer_length(new_head)? as isize - integer_length(head)? as isize;
    let zero_count = (x.len() as isize - 1 + delta).max(0) as usize;
    let mut out = vec![new_head];
    out.extend(std::iter::repeat_n(ZERO, zero_count));
    Ok(String::from_utf8(out).expect("字母表字节"))
}

/// 整数部分 - 1（整体步退）。头已到最小档 → `Exhausted`。
fn decrement_integer(x: &str) -> Result<String, ZOrderError> {
    let bytes = x.as_bytes();
    let head = bytes[0];
    let mut i = bytes.len();
    while i > 1 {
        i -= 1;
        let d = digit_value(bytes[i]).ok_or_else(|| ZOrderError::InvalidKey(x.to_string()))?;
        if d == 0 {
            continue; // 空位借位
        }
        let mut out = Vec::with_capacity(bytes.len());
        out.push(head);
        out.extend_from_slice(&bytes[1..i]);
        out.push(DIGITS[d - 1]);
        // i 之后的全是零位 → 借位后全变最大位
        out.extend(std::iter::repeat_n(LAST, bytes.len() - i - 1));
        return Ok(String::from_utf8(out).expect("字母表字节"));
    }
    let hi = int_digit_value(head).ok_or_else(|| ZOrderError::InvalidKey(x.to_string()))?;
    if hi == 0 {
        return Err(ZOrderError::Exhausted);
    }
    let new_head = INT_DIGITS[hi - 1];
    let delta = integer_length(new_head)? as isize - integer_length(head)? as isize;
    let last_count = (x.len() as isize - 1 + delta).max(0) as usize;
    let mut out = vec![new_head];
    out.extend(std::iter::repeat_n(LAST, last_count));
    Ok(String::from_utf8(out).expect("字母表字节"))
}

// ── 小数中点 ───────────────────────────────────────────────────

/// 小数中点：`a < b`（空串 = 0）；`b = None` = 上界开放。
///
/// - 公共前缀剥离（a 侧越界补 `'0'`——小数语义补零不改值）；
/// - 首位分叉差 > 1 → 取半（floor——与参考实现的公开样例表一致）；
/// - 相邻位且 b 有余量 → 贴 b 首位（单字符，靠外层拼接补齐）；
/// - 相邻位且 b 单字符 / None → 贴 a 首位 + 余量递归。
fn midpoint(a: &str, b: Option<&str>) -> Result<String, ZOrderError> {
    if let Some(bb) = b {
        if a >= bb {
            return Err(ZOrderError::InvalidKey(format!(
                "midpoint with a >= b ({a:?} >= {bb:?})"
            )));
        }
        if a.ends_with(char_of(ZERO)) || bb.ends_with(char_of(ZERO)) {
            return Err(ZOrderError::InvalidKey(format!(
                "trailing zero ({a:?} / {bb:?})"
            )));
        }
        // 剥离公共前缀（a 越界补零；b 越界即停——b 不可能被 a 的补零
        // 追平：b 尾非零，全对齐意味着 a ≥ b 已在上面拦截）
        let ab = a.as_bytes();
        let bb2 = bb.as_bytes();
        let mut n = 0usize;
        while bb2.get(n).copied() == Some(ab.get(n).copied().unwrap_or(ZERO)) {
            n += 1;
        }
        if n > 0 {
            let a_tail = if n >= a.len() { "" } else { &a[n..] };
            return Ok(format!("{}{}", &bb[..n], midpoint(a_tail, Some(&bb[n..]))?));
        }
    }
    // 首位分叉（或 b = None 的开放上界）
    let da = if a.is_empty() {
        0usize
    } else {
        digit_value(a.as_bytes()[0]).ok_or_else(|| ZOrderError::InvalidKey(a.to_string()))?
    };
    let db = match b {
        Some(bb) => {
            digit_value(bb.as_bytes()[0]).ok_or_else(|| ZOrderError::InvalidKey(bb.to_string()))?
        }
        None => DIGITS.len(),
    };
    if db - da > 1 {
        let mid = DIGITS[(da + db) / 2];
        return Ok(char_of(mid).to_string());
    }
    // 首位相邻：b 有余量 → 贴 b 首位（值域下沿，安全侧）
    if let Some(bb) = b {
        if bb.len() > 1 {
            return Ok(bb[..1].to_string());
        }
    }
    // b 单字符或 None：贴 a 首位（a 空 = '0'），余量向开放侧递归
    let first = if a.is_empty() { ZERO } else { a.as_bytes()[0] };
    let a_tail = if a.len() <= 1 { "" } else { &a[1..] };
    Ok(format!("{}{}", char_of(first), midpoint(a_tail, None)?))
}

// ── 公开入口 ───────────────────────────────────────────────────

/// 在 `a`（`None` = 下界开放）与 `b`（`None` = 上界开放）之间生成一个键。
///
/// 约定（与 CC0 参考实现一致）：
///
/// | 界 | 结果 |
/// |---|---|
/// | `(None, None)` | `"a0"`（首键） |
/// | `(None, b)` | 严格小于 `b`（prepend 优化：整数步退） |
/// | `(a, None)` | 严格大于 `a`（append 优化：整数步进） |
/// | `(a, b)` 且 `a > b` | 交换后按 `(b, a)` 生成 |
/// | `a == b` | `InvalidKey`（不可再分） |
///
/// 输出恒为合法键（通过 [`validate_key`]——含自身不变量测试）。
pub fn generate_between(a: Option<&str>, b: Option<&str>) -> Result<String, ZOrderError> {
    if let Some(a) = a {
        validate_key(a)?;
    }
    if let Some(b) = b {
        validate_key(b)?;
    }
    let (a, b) = match (a, b) {
        (Some(a), Some(b)) if a > b => (Some(b), Some(a)),
        pair => pair,
    };
    match (a, b) {
        (None, None) => Ok(format!("{}{}", char_of(INT_DIGITS[26]), char_of(ZERO))),
        (None, Some(b)) => {
            let ib = integer_part(b)?;
            let fb = &b[ib.len()..];
            if ib == SMALLEST_INTEGER {
                // b 的整数已是最小档：只能在小数面下探
                return Ok(format!("{}{}", ib, midpoint("", Some(fb))?));
            }
            if ib < b {
                // b 带小数 → 整数部分本身就是严格更小的键
                return Ok(ib.to_string());
            }
            decrement_integer(ib)
        }
        (Some(a), None) => {
            let ia = integer_part(a)?;
            let fa = &a[ia.len()..];
            match increment_integer(ia) {
                // append 优化：直接给下一个整数（丢弃 a 的小数）
                Ok(i) => Ok(i),
                // 整数已到最大档：小数面向上延展
                Err(ZOrderError::Exhausted) => Ok(format!("{}{}", ia, midpoint(fa, None)?)),
                Err(e) => Err(e),
            }
        }
        (Some(a), Some(b)) => {
            let ia = integer_part(a)?;
            let fa = &a[ia.len()..];
            let ib = integer_part(b)?;
            let fb = &b[ib.len()..];
            if ia == ib {
                // 同整数：小数面取中点
                return Ok(format!("{}{}", ia, midpoint(fa, Some(fb))?));
            }
            // 异整数：先试 a 的整数 +1（若仍 < b 即为最短键）
            let i = increment_integer(ia)?;
            if i.as_str() < b {
                return Ok(i);
            }
            // +1 越过 b：回到 a 的整数，小数面向上延展
            Ok(format!("{}{}", ia, midpoint(fa, None)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考实现公开样例表（README）——移植正确性的第一道锚。
    #[test]
    fn reference_examples() {
        assert_eq!(generate_between(None, None).unwrap(), "a0");
        assert_eq!(generate_between(None, Some("a0")).unwrap(), "Zz");
        assert_eq!(generate_between(Some("a0"), None).unwrap(), "a1");
        assert_eq!(generate_between(Some("a1"), Some("a2")).unwrap(), "a1V");
        // prepend 链：Zz 之前继续下探（整数步退）
        assert_eq!(generate_between(None, Some("Zz")).unwrap(), "Zy");
    }

    /// 整数步进/步退的进位链（append/prepend 长度优化的核心）。
    #[test]
    fn integer_carry_chains() {
        // az → b00（'a' 档 2 位用尽 → 'b' 档 3 位归零）
        assert_eq!(generate_between(Some("az"), None).unwrap(), "b00");
        assert_eq!(generate_between(Some("bzz"), None).unwrap(), "c000");
        // b00 → az（步退对称）
        assert_eq!(generate_between(None, Some("b00")).unwrap(), "az");
        assert_eq!(generate_between(None, Some("c000")).unwrap(), "bzz");
    }

    /// 键型不变量：输出合法 + 严格夹在界内（性质测试）。
    #[test]
    fn output_is_valid_and_strictly_between() {
        let cases: Vec<(Option<&str>, Option<&str>)> = vec![
            (None, None),
            (None, Some("a0")),
            (None, Some("Zz")),
            (None, Some("Zy")),
            (None, Some("a1V")),
            (None, Some("b00")),
            (Some("a0"), None),
            (Some("Zz"), None),
            (Some("az"), None),
            (Some("b00"), None),
            (Some("a1"), Some("a2")),
            (Some("a0"), Some("b00")),
            (Some("a1V"), Some("a1W")),
            (Some("Zz"), Some("a0")),
            (Some("a1"), Some("c005")),
            (Some("a1"), Some("a1V")),
            (Some("a1V"), Some("a2")),
        ];
        for (a, b) in cases {
            let k = generate_between(a, b).unwrap_or_else(|e| panic!("{a:?},{b:?}: {e}"));
            validate_key(&k).unwrap_or_else(|e| panic!("{k:?}: {e}"));
            if let Some(a) = a {
                assert!(a < &k, "{a:?} < {k:?}（上界 {b:?}）");
            }
            if let Some(b) = b {
                assert!(k.as_str() < b, "{k:?} < {b:?}（下界 {a:?}）");
            }
        }
    }

    /// 乱序界交换；相等界报错（不可再分）。
    #[test]
    fn swapped_and_equal_bounds() {
        assert_eq!(
            generate_between(Some("a2"), Some("a1")).unwrap(),
            "a1V",
            "a > b 交换后与 (a1, a2) 同果"
        );
        assert!(matches!(
            generate_between(Some("a1"), Some("a1")),
            Err(ZOrderError::InvalidKey(_))
        ));
    }

    /// 非法输入面：空串 / 尾零 / 坏字符 / 头长度不符 / 最小整数。
    #[test]
    fn rejects_invalid_keys() {
        for bad in [
            "",
            "a",
            "a0V0",
            "a!",
            "1a",
            "A00000000000000000000000000",
            "z",
            "b0",
            "b00V0",
        ] {
            assert!(generate_between(Some(bad), None).is_err(), "{bad:?} 应被拒");
        }
    }

    /// 单调追加 1000 次：全不重复 + 键长不退化（验收 2 的算法面）。
    #[test]
    fn append_1000_unique_and_bounded() {
        let mut keys = vec![generate_between(None, None).unwrap()];
        for _ in 0..1000 {
            let last = keys.last().unwrap().clone();
            keys.push(generate_between(Some(&last), None).unwrap());
        }
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "全不重复");
        let max_len = keys.iter().map(|k| k.len()).max().unwrap();
        assert!(max_len <= 4, "追加路径键长应摊还常数（实得 {max_len}）");
    }

    /// 头部追加（prepend）1000 次：全不重复 + 有界。
    #[test]
    fn prepend_1000_unique_and_bounded() {
        let mut keys = vec![generate_between(None, None).unwrap()];
        for _ in 0..1000 {
            let first = keys[0].clone();
            keys.insert(0, generate_between(None, Some(&first)).unwrap());
        }
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "全不重复");
        let max_len = keys.iter().map(|k| k.len()).max().unwrap();
        assert!(
            max_len <= 29,
            "prepend 档位有限，最坏到 27 位深负档（实得 {max_len}）"
        );
    }

    /// 同点插入 1000 次（总在首二键之间）：不重复 + 长度对数增长。
    ///
    /// 如实整记：同点插入是**二分**语义（每位空间 62 档对半 → 约
    /// 6 次插一位），1000 次 ≈ 170 位——参考实现（CC0 源）与 Figma
    /// 博客记载的同款数学性质，不是缺陷；常见路径（追加/置顶）走
    /// 整数步进，键长摊还常数（见 append 测试）。
    #[test]
    fn midpoint_insert_1000_unique_and_bounded() {
        let mut lo = generate_between(None, None).unwrap();
        let hi = generate_between(Some(&lo), None).unwrap();
        let mut max_len = lo.len();
        for _ in 0..1000 {
            let mid = generate_between(Some(&lo), Some(&hi)).unwrap();
            assert!(lo < mid && mid < hi);
            lo = mid;
            max_len = max_len.max(lo.len());
        }
        // 对数增长：1000 次 ≈ 1000/log2(62) ≈ 170 位，留安全余量
        assert!(max_len <= 200, "同点插入键长应近对数（实得 {max_len}）");
    }

    /// 双副本同界插入产生相同键（确定性算法的如实性质——会话层以
    /// ID 决胜吸收，见 session 级收敛测试）。
    #[test]
    fn deterministic_same_bounds_same_key() {
        assert_eq!(
            generate_between(Some("a1"), Some("a2")).unwrap(),
            generate_between(Some("a1"), Some("a2")).unwrap()
        );
    }

    /// 表示边界是错误不是 panic：最小整数步退 / 最大整数步进。
    #[test]
    fn exhaustion_is_error_not_panic() {
        // 最小整数本身不可作为键（validate 拒），构造其上一档验证
        // decrement 路径不 panic 即可。
        let _ = decrement_integer(SMALLEST_INTEGER);
        // 最大整数 = 'z' 头 + 全 'z' 位（27 位）→ 步进 Exhausted
        assert_eq!(
            increment_integer("zzzzzzzzzzzzzzzzzzzzzzzzzzz"),
            Err(ZOrderError::Exhausted)
        );
        assert_eq!(
            decrement_integer("A00000000000000000000000000"),
            Err(ZOrderError::Exhausted)
        );
    }
}
