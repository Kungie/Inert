//! Fixed-point decimals. Wire form: `[mantissa: int, scale: uint]` (spec §6.4).

use crate::cbor::Value;
use crate::limits::DECIMAL_MAX_SCALE;
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal {
    pub mantissa: i64,
    pub scale: u32,
}

impl Decimal {
    pub const fn new(mantissa: i64, scale: u32) -> Self {
        Self { mantissa, scale }
    }

    pub fn to_value(self) -> Value {
        Value::Array(vec![Value::int(self.mantissa), Value::Uint(self.scale as u64)])
    }

    pub fn from_value(v: &Value) -> Option<Self> {
        let a = v.as_array()?;
        if a.len() != 2 {
            return None;
        }
        let mantissa = a[0].as_i64()?;
        let scale = a[1].as_u64()?;
        if scale > DECIMAL_MAX_SCALE as u64 {
            return None;
        }
        Some(Self { mantissa, scale: scale as u32 })
    }

    fn scaled(self, to: u32) -> Option<i128> {
        let mut m = self.mantissa as i128;
        for _ in self.scale..to {
            m = m.checked_mul(10)?;
        }
        Some(m)
    }

    /// Compares two decimals numerically regardless of their scales.
    pub fn cmp_num(self, other: Self) -> Ordering {
        let s = self.scale.max(other.scale);
        // Both fit: mantissa < 2^63 and scale <= 18 keeps us well inside i128.
        self.scaled(s).unwrap().cmp(&other.scaled(s).unwrap())
    }

    /// Parses `"-12.30"`. The scale is the number of digits after the point.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (neg, body) = match s.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, s),
        };
        if body.is_empty() {
            return None;
        }
        let (int_part, frac) = match body.split_once('.') {
            Some((i, f)) => (i, f),
            None => (body, ""),
        };
        if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if body.contains('.') && frac.is_empty() {
            return None;
        }
        let scale = frac.len() as u32;
        if scale > DECIMAL_MAX_SCALE {
            return None;
        }
        let digits = format!("{int_part}{frac}");
        let mut m: i64 = digits.parse().ok()?;
        if neg {
            m = -m;
        }
        Some(Self { mantissa: m, scale })
    }

    /// Re-expresses the value with exactly `scale` fraction digits if that is lossless.
    pub fn rescale(self, scale: u32) -> Option<Self> {
        if scale >= self.scale {
            let m = self.scaled(scale)?;
            Some(Self { mantissa: i64::try_from(m).ok()?, scale })
        } else {
            let mut m = self.mantissa;
            for _ in scale..self.scale {
                if m % 10 != 0 {
                    return None;
                }
                m /= 10;
            }
            Some(Self { mantissa: m, scale })
        }
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.mantissa);
        }
        let neg = self.mantissa < 0;
        let digits = (self.mantissa as i128).unsigned_abs().to_string();
        let s = self.scale as usize;
        let padded = if digits.len() <= s { format!("{}{}", "0".repeat(s - digits.len() + 1), digits) } else { digits };
        let (i, frac) = padded.split_at(padded.len() - s);
        write!(f, "{}{}.{}", if neg { "-" } else { "" }, i, frac)
    }
}

impl fmt::Debug for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        for s in ["0", "12", "-3.50", "0.01", "10000.00", "-0.5"] {
            assert_eq!(Decimal::parse(s).unwrap().to_string(), s);
        }
        assert!(Decimal::parse("1.").is_none());
        assert!(Decimal::parse(".5").is_none());
        assert!(Decimal::parse("1e3").is_none());
        assert!(Decimal::parse("").is_none());
    }

    #[test]
    fn compare_across_scales() {
        let a = Decimal::parse("1.5").unwrap();
        let b = Decimal::parse("1.50").unwrap();
        let c = Decimal::parse("1.49").unwrap();
        assert_eq!(a.cmp_num(b), Ordering::Equal);
        assert_eq!(a.cmp_num(c), Ordering::Greater);
    }

    #[test]
    fn rescale() {
        assert_eq!(Decimal::parse("1.5").unwrap().rescale(2).unwrap().to_string(), "1.50");
        assert!(Decimal::parse("1.55").unwrap().rescale(1).is_none());
    }
}
