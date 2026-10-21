//! 确定性十进制（§四.4——"金额等财务数据不依赖二进制浮点"）。
//!
//! i128 定点 + 显式标度（十进制小数位数，0-18）。范围约 ±1.7e19 内
//! 18 位有效数字——覆盖标书级金额与数量场景；越界 = 显式错误而非
//! 静默精度损失。
//!
//! 舍入规则显式三选一（HalfUp 财务默认 / HalfEven 统计 / Truncate
//! 保守），记录在 `CalculationRecord` 内——复算必须同规则。所有
//! 运算**不隐式舍入**：加减乘结果标度 = 操作数标度的确定函数；除法
//! 必须显式给目标标度。
//!
//! 验证纪律：除法实现走两条显式路径（e′≥0 整除 + 余数舍入 /
//! e′<0 自然标度 floor + 论证过的安全降标），边界由单测钉死（半值
//! 四态、除零、跨标度）。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 定点十进制（值 = unscaled × 10^-scale）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Decimal {
    unscaled: i128,
    scale: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalError {
    /// 输入串格式错误（多小数点/非数字/空/科学记法）。
    Parse,
    /// 标度超域（>18）。
    Domain,
    /// 算术越 i128 域。
    Overflow,
    /// 除零。
    DivideByZero,
    /// 标度对齐所需 10^n 越界。
    ScaleAlign,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DecimalError::Parse => "decimal parse error",
            DecimalError::Domain => "decimal domain error (scale>18)",
            DecimalError::Overflow => "decimal overflow (i128)",
            DecimalError::DivideByZero => "divide by zero",
            DecimalError::ScaleAlign => "scale alignment overflow",
        })
    }
}
impl std::error::Error for DecimalError {}

/// 舍入规则（三选一——刻意不含天花板/地板：负数方向歧义面不值
/// MVP 复杂度；需要时显式加 mode 并补边界测试）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rounding {
    /// 半上（财务默认；恰半时远离零）。
    HalfUp,
    /// 银行家舍入（半偶；恰半时取偶）。
    HalfEven,
    /// 截断（向零）。
    Truncate,
}

impl Decimal {
    pub fn zero(scale: u8) -> Self {
        Decimal { unscaled: 0, scale }
    }

    /// 由字符串解析（可选正负号 + 数字 + 至多一个小数点；不支持
    /// 科学记法与千分位——登记面要求规范化输入）。
    pub fn parse(s: &str) -> Result<Self, DecimalError> {
        let t = s.trim();
        if t.is_empty() {
            return Err(DecimalError::Parse);
        }
        let (neg, digits) = match t.as_bytes()[0] {
            b'-' => (true, &t[1..]),
            b'+' => (false, &t[1..]),
            _ => (false, t),
        };
        if digits.is_empty() {
            return Err(DecimalError::Parse);
        }
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((i, f)) => (i, f),
            None => (digits, ""),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(DecimalError::Parse);
        }
        if !int_part.bytes().all(|b| b.is_ascii_digit())
            || !frac_part.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(DecimalError::Parse);
        }
        let scale = frac_part.len() as u8;
        if scale > 18 {
            return Err(DecimalError::Domain);
        }
        let mut unscaled: i128 = 0;
        for b in int_part.bytes().chain(frac_part.bytes()) {
            unscaled = unscaled
                .checked_mul(10)
                .and_then(|u| u.checked_add((b - b'0') as i128))
                .ok_or(DecimalError::Overflow)?;
        }
        Ok(Decimal {
            unscaled: if neg { -unscaled } else { unscaled },
            scale,
        })
    }

    pub fn from_unscaled(unscaled: i128, scale: u8) -> Result<Self, DecimalError> {
        if scale > 18 {
            return Err(DecimalError::Domain);
        }
        Ok(Decimal { unscaled, scale })
    }

    pub fn unscaled(&self) -> i128 {
        self.unscaled
    }
    pub fn scale(&self) -> u8 {
        self.scale
    }

    fn align(a: Decimal, b: Decimal) -> Result<(i128, i128, u8), DecimalError> {
        if a.scale == b.scale {
            return Ok((a.unscaled, b.unscaled, a.scale));
        }
        let (lo, hi) = if a.scale < b.scale { (a, b) } else { (b, a) };
        let mult = pow10(hi.scale - lo.scale).ok_or(DecimalError::ScaleAlign)?;
        let scaled = lo
            .unscaled
            .checked_mul(mult)
            .ok_or(DecimalError::Overflow)?;
        Ok((scaled, hi.unscaled, hi.scale))
    }

    /// 加（结果标度 = max(两标度)；checked_* 命名 = 显式错误算术
    /// 惯例——溢出返回 Err 而非 panic，刻意不实现 std::ops）。
    pub fn checked_add(self, other: Decimal) -> Result<Decimal, DecimalError> {
        let (a, b, s) = Self::align(self, other)?;
        Ok(Decimal {
            unscaled: a.checked_add(b).ok_or(DecimalError::Overflow)?,
            scale: s,
        })
    }

    /// 减（同加）。
    pub fn checked_sub(self, other: Decimal) -> Result<Decimal, DecimalError> {
        let (a, b, s) = Self::align(self, other)?;
        Ok(Decimal {
            unscaled: a.checked_sub(b).ok_or(DecimalError::Overflow)?,
            scale: s,
        })
    }

    /// 乘（结果标度 = 两标度之和；超 18 = 显式错误——调用方先
    /// 舍入入参，不隐式丢精度）。
    pub fn checked_mul(self, other: Decimal) -> Result<Decimal, DecimalError> {
        let scale = self.scale + other.scale;
        if scale > 18 {
            return Err(DecimalError::Domain);
        }
        Ok(Decimal {
            unscaled: self
                .unscaled
                .checked_mul(other.unscaled)
                .ok_or(DecimalError::Overflow)?,
            scale,
        })
    }

    /// 除（显式目标标度 + 舍入规则；checked_* 命名同理）。
    ///
    /// 实现两路径（按 e′ = target + other.scale − self.scale）：
    /// - **e′ ≥ 0**：numer = self.unscaled × 10^e′，整除得 target
    ///   标度下的 floor 商 + 余数，余数按模式舍入（恰半判定用
    ///   |2·rem| 与 |divisor| 比较——整数域精确，无浮点近似）。
    /// - **e′ < 0**：商的自然标度 N = self.scale − other.scale >
    ///   target。先取 N 标度 floor 商，再显式降标。安全性论证：
    ///   余项贡献 < 10^-N，而 target 处半值是 10^-N 的整数倍——
    ///   余项不可能翻转半边界（q 部 ≤ 半值 − 10^-N 恒立）。
    pub fn checked_div(
        self,
        other: Decimal,
        target_scale: u8,
        r: Rounding,
    ) -> Result<Decimal, DecimalError> {
        if other.unscaled == 0 {
            return Err(DecimalError::DivideByZero);
        }
        if target_scale > 18 {
            return Err(DecimalError::Domain);
        }
        let e = target_scale as i32 + other.scale as i32 - self.scale as i32;
        if e >= 0 {
            let mult = pow10(e as u8).ok_or(DecimalError::ScaleAlign)?;
            let numer = self
                .unscaled
                .checked_mul(mult)
                .ok_or(DecimalError::Overflow)?;
            let q = numer / other.unscaled;
            let rem = numer % other.unscaled;
            let bumped = round_with_rem(q, rem, other.unscaled, r)?;
            Ok(Decimal {
                unscaled: bumped,
                scale: target_scale,
            })
        } else {
            let q = self.unscaled / other.unscaled;
            let n_scale = (self.scale as i32 - other.scale as i32) as u8;
            Decimal {
                unscaled: q,
                scale: n_scale,
            }
            .rescale(target_scale, r)
        }
    }

    /// 显式重标（降标度 = 舍入；升标度 = 补零）。
    pub fn rescale(self, target: u8, r: Rounding) -> Result<Decimal, DecimalError> {
        if target > 18 {
            return Err(DecimalError::Domain);
        }
        if target >= self.scale {
            let mult = pow10(target - self.scale).ok_or(DecimalError::ScaleAlign)?;
            return Ok(Decimal {
                unscaled: self
                    .unscaled
                    .checked_mul(mult)
                    .ok_or(DecimalError::Overflow)?,
                scale: target,
            });
        }
        let div = pow10(self.scale - target).ok_or(DecimalError::ScaleAlign)?;
        round_with_rem(self.unscaled / div, self.unscaled % div, div, r).map(|unscaled| Decimal {
            unscaled,
            scale: target,
        })
    }

    /// 取负（i128 下界对称性：-(-2^127) 越界 = 显式错误；s1-r2 求值器
    /// 一元负号所需）。
    pub fn checked_neg(self) -> Result<Decimal, DecimalError> {
        Ok(Decimal {
            unscaled: self.unscaled.checked_neg().ok_or(DecimalError::Overflow)?,
            scale: self.scale,
        })
    }

    /// 数值相等（跨标度）。
    pub fn numeric_eq(self, other: Decimal) -> Result<bool, DecimalError> {
        let (a, b, _) = Self::align(self, other)?;
        Ok(a == b)
    }
}

/// floor 商 + 余数（相对 divisor）的舍入（q·div + rem = n，rem 与 n
/// 同号）。恰半 = |2·rem| == |div|；远离零方向 = sign(rem)。
fn round_with_rem(q: i128, rem: i128, divisor: i128, r: Rounding) -> Result<i128, DecimalError> {
    if rem == 0 {
        return Ok(q);
    }
    let away = if rem < 0 { -1 } else { 1 };
    let rem_abs = rem.unsigned_abs();
    let div_abs = divisor.unsigned_abs();
    let twice = rem_abs.checked_mul(2).ok_or(DecimalError::Overflow)?;
    let bump = match r {
        Rounding::Truncate => 0,
        Rounding::HalfUp => {
            if twice >= div_abs {
                away
            } else {
                0
            }
        }
        Rounding::HalfEven => {
            if twice > div_abs || (twice == div_abs && q % 2 != 0) {
                away
            } else {
                0
            }
        }
    };
    Ok(q + bump)
}

fn pow10(n: u8) -> Option<i128> {
    if n > 38 {
        return None;
    }
    let mut r: i128 = 1;
    for _ in 0..n {
        r = r.checked_mul(10)?;
    }
    Some(r)
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.unscaled.to_string();
        if self.scale == 0 {
            return f.write_str(&s);
        }
        let (sign, digits) = if let Some(d) = s.strip_prefix('-') {
            ("-", d)
        } else {
            ("", s.as_str())
        };
        let sc = self.scale as usize;
        if digits.len() <= sc {
            return write!(f, "{sign}0.{digits:0>sc$}", sc = sc);
        }
        let (i, fr) = digits.split_at(digits.len() - sc);
        write!(f, "{sign}{i}.{fr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_roundtrip() {
        for s in [
            "0",
            "1",
            "-1",
            "3.14",
            "-0.05",
            "123456789012345678",
            "0.000000000000000001",
        ] {
            let d = Decimal::parse(s).unwrap();
            assert_eq!(d.to_string(), s, "往返一致: {s}");
        }
        assert_eq!(Decimal::parse("+2.5").unwrap().to_string(), "2.5");
        assert!(Decimal::parse("1.2.3").is_err());
        assert!(Decimal::parse("").is_err());
        assert!(Decimal::parse("abc").is_err());
        assert!(Decimal::parse("1e5").is_err(), "科学记法显式拒绝");
        assert!(
            Decimal::parse("1.0000000000000000000").is_err(),
            "19 位小数超域"
        );
    }

    #[test]
    fn no_binary_float_artifacts() {
        let a = Decimal::parse("0.1").unwrap();
        let b = Decimal::parse("0.2").unwrap();
        assert_eq!(
            a.checked_add(b).unwrap().to_string(),
            "0.3",
            "0.1+0.2 精确 = 0.3"
        );
        assert!(Decimal::parse("1.10")
            .unwrap()
            .numeric_eq(Decimal::parse("1.1").unwrap())
            .unwrap());
    }

    #[test]
    fn quantity_times_price() {
        // §四.4 首个必覆盖式：数量 × 单价
        let qty = Decimal::parse("12.5").unwrap();
        let price = Decimal::parse("336.80").unwrap();
        assert_eq!(qty.checked_mul(price).unwrap().to_string(), "4210.000");
        // 求和
        let mut sum = Decimal::zero(3);
        for v in ["1000.000", "4210.000", "0.500"] {
            sum = sum.checked_add(Decimal::parse(v).unwrap()).unwrap();
        }
        assert_eq!(sum.to_string(), "5210.500");
    }

    #[test]
    fn division_rounding_modes() {
        let one = Decimal::parse("1").unwrap();
        let three = Decimal::parse("3").unwrap();
        assert_eq!(
            one.checked_div(three, 4, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "0.3333"
        );
        assert_eq!(
            one.checked_div(three, 4, Rounding::Truncate)
                .unwrap()
                .to_string(),
            "0.3333"
        );
        // 除法链的舍入差异在更深层可见
        assert_eq!(
            one.checked_div(three, 6, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "0.333333"
        );
        // 半值四态（5/2 = 2.5 → 目标标度 0）
        let five = Decimal::parse("5").unwrap();
        let two = Decimal::parse("2").unwrap();
        assert_eq!(
            five.checked_div(two, 0, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "3",
            "半上远离零"
        );
        assert_eq!(
            five.checked_div(two, 0, Rounding::HalfEven)
                .unwrap()
                .to_string(),
            "2",
            "半偶取偶"
        );
        assert_eq!(
            five.checked_div(two, 0, Rounding::Truncate)
                .unwrap()
                .to_string(),
            "2",
            "截断向零"
        );
        let neg_five = Decimal::parse("-5").unwrap();
        assert_eq!(
            neg_five
                .checked_div(two, 0, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "-3",
            "负半值远离零"
        );
        // e′<0 路径：深标度被除数 + 浅目标
        let deep = Decimal::parse("0.000001").unwrap(); // scale 6
        assert_eq!(
            deep.checked_div(one, 0, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "0"
        );
        assert_eq!(
            deep.checked_div(one, 6, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "0.000001"
        );
        // 跨标度除：1.00 / 0.8 = 1.25
        let a = Decimal::parse("1.00").unwrap();
        let b = Decimal::parse("0.8").unwrap();
        assert_eq!(
            a.checked_div(b, 2, Rounding::HalfUp).unwrap().to_string(),
            "1.25"
        );
        // 除零显式错误
        assert!(one
            .checked_div(Decimal::zero(0), 2, Rounding::HalfUp)
            .is_err());
    }

    #[test]
    fn checked_neg_boundaries() {
        let d = Decimal::parse("-2.5").unwrap();
        assert_eq!(d.checked_neg().unwrap().to_string(), "2.5");
        // i128 下界取负越界 = 显式错误（上界 -(-170141183460469231731..)
        // 不存在对称问题——下界才是非对称端）
        let min = Decimal::from_unscaled(i128::MIN, 0).unwrap();
        assert_eq!(
            min.checked_neg().unwrap_err(),
            DecimalError::Overflow,
            "-(i128::MIN) 越界显式错误"
        );
    }

    #[test]
    fn rescale_explicit() {
        let d = Decimal::parse("2.675").unwrap();
        assert_eq!(d.rescale(2, Rounding::HalfUp).unwrap().to_string(), "2.68");
        assert_eq!(
            d.rescale(2, Rounding::Truncate).unwrap().to_string(),
            "2.67"
        );
        let up = Decimal::parse("2.68")
            .unwrap()
            .rescale(3, Rounding::HalfUp)
            .unwrap();
        assert_eq!(up.to_string(), "2.680", "升标度补零");
        // 半偶在 rescale 面：2.5 → 2 / 3.5 → 4
        assert_eq!(
            Decimal::parse("2.5")
                .unwrap()
                .rescale(0, Rounding::HalfEven)
                .unwrap()
                .to_string(),
            "2"
        );
        assert_eq!(
            Decimal::parse("3.5")
                .unwrap()
                .rescale(0, Rounding::HalfEven)
                .unwrap()
                .to_string(),
            "4"
        );
        assert_eq!(
            Decimal::parse("2.5")
                .unwrap()
                .rescale(0, Rounding::HalfUp)
                .unwrap()
                .to_string(),
            "3"
        );
    }
}
