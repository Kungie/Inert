//! Strict, deterministic CBOR (RFC 8949 §4.2.1 "core deterministic encoding").
//!
//! The decoder accepts exactly the subset the Inert spec allows (spec §5.2):
//! shortest-form integers and lengths, definite lengths only, map keys in
//! strictly ascending bytewise order of their encodings (which also rules out
//! duplicates), no tags, no floats, and only `false`/`true`/`null` as simple
//! values. Anything else is rejected. `encode(decode(x)) == x` for every input
//! the decoder accepts.

use crate::limits;
use std::fmt;

/// A CBOR data item restricted to what Inert allows.
#[derive(Clone, PartialEq, Eq)]
pub enum Value {
    /// Major type 0.
    Uint(u64),
    /// Major type 1: the value is `-1 - n`.
    Nint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    /// Entries are kept in deterministic (encoded-key) order after decoding.
    /// The encoder sorts them, so construction order does not matter.
    Map(Vec<(Value, Value)>),
    Bool(bool),
    Null,
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Uint(n) => write!(f, "{n}"),
            Value::Nint(n) => write!(f, "-{}", (*n as i128) + 1),
            Value::Bytes(b) => write!(f, "h'{}'", hex_lower(b)),
            Value::Text(s) => write!(f, "{s:?}"),
            Value::Array(a) => f.debug_list().entries(a).finish(),
            Value::Map(m) => f.debug_map().entries(m.iter().map(|(k, v)| (k, v))).finish(),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Null => write!(f, "null"),
        }
    }
}

fn hex_lower(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

impl Value {
    pub fn text(s: impl Into<String>) -> Self {
        Value::Text(s.into())
    }

    pub fn int(n: i64) -> Self {
        if n >= 0 { Value::Uint(n as u64) } else { Value::Nint((-1 - n) as u64) }
    }

    /// Returns the value as an `i64` if it is an integer that fits.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Uint(n) => i64::try_from(n).ok(),
            Value::Nint(n) => {
                if n <= i64::MAX as u64 { Some(-1 - n as i64) } else { None }
            }
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::Uint(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Looks up a text key in a map value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_map()?
            .iter()
            .find(|(k, _)| k.as_text() == Some(key))
            .map(|(_, v)| v)
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Value::Uint(_) | Value::Nint(_) => "int",
            Value::Bytes(_) => "bytes",
            Value::Text(_) => "text",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::Bool(_) => "bool",
            Value::Null => "null",
        }
    }
}

/// Builds a map value from `(text key, value)` pairs.
pub fn map<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (Value::Text(k.into()), v)).collect())
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CborError {
    #[error("unexpected end of input")]
    Eof,
    #[error("trailing bytes after the top-level item")]
    Trailing,
    #[error("integer or length is not in shortest form")]
    NonCanonicalInt,
    #[error("indefinite-length items are not allowed")]
    Indefinite,
    #[error("CBOR tags are not allowed")]
    Tag,
    #[error("floating point and simple values other than false/true/null are not allowed")]
    Simple,
    #[error("reserved additional information")]
    Reserved,
    #[error("text is not valid UTF-8")]
    Utf8,
    #[error("map keys are not in strictly ascending deterministic order (or are duplicated)")]
    KeyOrder,
    #[error("nesting deeper than {0}")]
    TooDeep(usize),
    #[error("item length exceeds the remaining input")]
    TooLong,
}

/// Encodes a value deterministically. Map keys are sorted by their encoding.
pub fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(v, &mut out);
    out
}

fn head(major: u8, n: u64, out: &mut Vec<u8>) {
    let m = major << 5;
    if n < 24 {
        out.push(m | n as u8);
    } else if n <= u8::MAX as u64 {
        out.push(m | 24);
        out.push(n as u8);
    } else if n <= u16::MAX as u64 {
        out.push(m | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= u32::MAX as u64 {
        out.push(m | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn encode_into(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Uint(n) => head(0, *n, out),
        Value::Nint(n) => head(1, *n, out),
        Value::Bytes(b) => {
            head(2, b.len() as u64, out);
            out.extend_from_slice(b);
        }
        Value::Text(s) => {
            head(3, s.len() as u64, out);
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(a) => {
            head(4, a.len() as u64, out);
            for x in a {
                encode_into(x, out);
            }
        }
        Value::Map(m) => {
            let mut entries: Vec<(Vec<u8>, &Value)> =
                m.iter().map(|(k, v)| (encode(k), v)).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            head(5, entries.len() as u64, out);
            for (k, v) in entries {
                out.extend_from_slice(&k);
                encode_into(v, out);
            }
        }
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Null => out.push(0xf6),
    }
}

/// Decodes exactly one item that must span the whole input.
pub fn decode(input: &[u8]) -> Result<Value, CborError> {
    decode_with_depth(input, limits::CBOR_MAX_DEPTH)
}

pub fn decode_with_depth(input: &[u8], max_depth: usize) -> Result<Value, CborError> {
    let mut d = Decoder { buf: input, pos: 0, max_depth };
    let v = d.item(0)?;
    if d.pos != input.len() {
        return Err(CborError::Trailing);
    }
    Ok(v)
}

struct Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
    max_depth: usize,
}

impl<'a> Decoder<'a> {
    fn byte(&mut self) -> Result<u8, CborError> {
        let b = *self.buf.get(self.pos).ok_or(CborError::Eof)?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CborError> {
        let end = self.pos.checked_add(n).ok_or(CborError::TooLong)?;
        if end > self.buf.len() {
            return Err(CborError::TooLong);
        }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// Reads the argument for additional info `ai`, enforcing shortest form.
    fn arg(&mut self, ai: u8) -> Result<u64, CborError> {
        let n = match ai {
            0..=23 => return Ok(ai as u64),
            24 => {
                let n = self.byte()? as u64;
                if n < 24 {
                    return Err(CborError::NonCanonicalInt);
                }
                n
            }
            25 => {
                let s = self.take(2)?;
                let n = u16::from_be_bytes([s[0], s[1]]) as u64;
                if n <= u8::MAX as u64 {
                    return Err(CborError::NonCanonicalInt);
                }
                n
            }
            26 => {
                let s = self.take(4)?;
                let n = u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as u64;
                if n <= u16::MAX as u64 {
                    return Err(CborError::NonCanonicalInt);
                }
                n
            }
            27 => {
                let s = self.take(8)?;
                let n = u64::from_be_bytes(s.try_into().unwrap());
                if n <= u32::MAX as u64 {
                    return Err(CborError::NonCanonicalInt);
                }
                n
            }
            31 => return Err(CborError::Indefinite),
            _ => return Err(CborError::Reserved),
        };
        Ok(n)
    }

    fn len(&mut self, ai: u8) -> Result<usize, CborError> {
        let n = self.arg(ai)?;
        // A length can never exceed the bytes left; this also bounds allocations.
        let left = (self.buf.len() - self.pos) as u64;
        if n > left {
            return Err(CborError::TooLong);
        }
        Ok(n as usize)
    }

    fn item(&mut self, depth: usize) -> Result<Value, CborError> {
        if depth > self.max_depth {
            return Err(CborError::TooDeep(self.max_depth));
        }
        let ib = self.byte()?;
        let major = ib >> 5;
        let ai = ib & 0x1f;
        match major {
            0 => Ok(Value::Uint(self.arg(ai)?)),
            1 => Ok(Value::Nint(self.arg(ai)?)),
            2 => {
                let n = self.len(ai)?;
                Ok(Value::Bytes(self.take(n)?.to_vec()))
            }
            3 => {
                let n = self.len(ai)?;
                let s = self.take(n)?;
                let s = std::str::from_utf8(s).map_err(|_| CborError::Utf8)?;
                Ok(Value::Text(s.to_owned()))
            }
            4 => {
                let n = self.len(ai)?;
                let mut a = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    a.push(self.item(depth + 1)?);
                }
                Ok(Value::Array(a))
            }
            5 => {
                let n = self.len(ai)?;
                let mut m = Vec::with_capacity(n.min(1024));
                let mut prev: Option<&'a [u8]> = None;
                for _ in 0..n {
                    let start = self.pos;
                    let k = self.item(depth + 1)?;
                    let kbytes = &self.buf[start..self.pos];
                    if let Some(p) = prev {
                        if kbytes <= p {
                            return Err(CborError::KeyOrder);
                        }
                    }
                    prev = Some(kbytes);
                    let v = self.item(depth + 1)?;
                    m.push((k, v));
                }
                Ok(Value::Map(m))
            }
            6 => Err(CborError::Tag),
            7 => match ai {
                20 => Ok(Value::Bool(false)),
                21 => Ok(Value::Bool(true)),
                22 => Ok(Value::Null),
                31 => Err(CborError::Indefinite),
                _ => Err(CborError::Simple),
            },
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn d(b: &[u8]) -> Result<Value, CborError> {
        decode(b)
    }

    #[test]
    fn rfc_examples() {
        assert_eq!(d(&[0x00]).unwrap(), Value::Uint(0));
        assert_eq!(d(&[0x17]).unwrap(), Value::Uint(23));
        assert_eq!(d(&[0x18, 0x18]).unwrap(), Value::Uint(24));
        assert_eq!(d(&[0x20]).unwrap(), Value::int(-1));
        assert_eq!(d(&[0x38, 0x63]).unwrap(), Value::int(-100));
        assert_eq!(d(&[0x63, b'a', b'b', b'c']).unwrap(), Value::text("abc"));
        assert_eq!(d(&[0xf5]).unwrap(), Value::Bool(true));
    }

    #[test]
    fn rejects_non_shortest_ints() {
        assert_eq!(d(&[0x18, 0x05]), Err(CborError::NonCanonicalInt));
        assert_eq!(d(&[0x19, 0x00, 0xff]), Err(CborError::NonCanonicalInt));
        assert_eq!(d(&[0x1a, 0, 0, 0xff, 0xff]), Err(CborError::NonCanonicalInt));
        assert_eq!(d(&[0x1b, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]), Err(CborError::NonCanonicalInt));
        // Non-shortest length on a text string.
        assert_eq!(d(&[0x78, 0x01, b'a']), Err(CborError::NonCanonicalInt));
    }

    #[test]
    fn rejects_indefinite_tags_floats() {
        assert_eq!(d(&[0x9f, 0xff]), Err(CborError::Indefinite));
        assert_eq!(d(&[0x5f, 0xff]), Err(CborError::Indefinite));
        assert_eq!(d(&[0xc1, 0x00]), Err(CborError::Tag));
        assert_eq!(d(&[0xf9, 0x3c, 0x00]), Err(CborError::Simple)); // half float 1.0
        assert_eq!(d(&[0xfb, 0, 0, 0, 0, 0, 0, 0, 0]), Err(CborError::Simple));
        assert_eq!(d(&[0xf7]), Err(CborError::Simple)); // undefined
    }

    #[test]
    fn rejects_bad_map_order_and_duplicates() {
        // {"b": 1, "a": 2}
        assert_eq!(d(&[0xa2, 0x61, b'b', 0x01, 0x61, b'a', 0x02]), Err(CborError::KeyOrder));
        // {"a": 1, "a": 2}
        assert_eq!(d(&[0xa2, 0x61, b'a', 0x01, 0x61, b'a', 0x02]), Err(CborError::KeyOrder));
        // Shorter keys sort first in bytewise order of the encoding: {"z":0, "aa":0} is valid.
        assert!(d(&[0xa2, 0x61, b'z', 0x00, 0x62, b'a', b'a', 0x00]).is_ok());
    }

    #[test]
    fn rejects_trailing_and_truncation() {
        assert_eq!(d(&[0x00, 0x00]), Err(CborError::Trailing));
        assert_eq!(d(&[0x62, b'a']), Err(CborError::TooLong));
        assert_eq!(d(&[]), Err(CborError::Eof));
        // Huge declared length must not allocate.
        assert_eq!(d(&[0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]), Err(CborError::TooLong));
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert_eq!(d(&[0x62, 0xff, 0xfe]), Err(CborError::Utf8));
    }

    #[test]
    fn depth_limit() {
        let mut b = vec![0x81; limits::CBOR_MAX_DEPTH + 1];
        b.push(0x00);
        assert_eq!(d(&b), Err(CborError::TooDeep(limits::CBOR_MAX_DEPTH)));
        let mut ok = vec![0x81; limits::CBOR_MAX_DEPTH];
        ok.push(0x00);
        assert!(d(&ok).is_ok());
    }

    #[test]
    fn encoder_sorts_keys() {
        let v = map([("bb", Value::Uint(1)), ("a", Value::Uint(2)), ("c", Value::Uint(3))]);
        let bytes = encode(&v);
        let back = decode(&bytes).unwrap();
        let keys: Vec<_> = back.as_map().unwrap().iter().map(|(k, _)| k.as_text().unwrap().to_owned()).collect();
        assert_eq!(keys, ["a", "c", "bb"]);
    }

    fn arb_value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            any::<u64>().prop_map(Value::Uint),
            any::<u64>().prop_map(Value::Nint),
            proptest::collection::vec(any::<u8>(), 0..40).prop_map(Value::Bytes),
            ".{0,20}".prop_map(Value::Text),
            any::<bool>().prop_map(Value::Bool),
            Just(Value::Null),
        ];
        leaf.prop_recursive(4, 64, 8, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
                proptest::collection::btree_map("[a-z]{1,6}", inner, 0..8)
                    .prop_map(|m| map(m.into_iter())),
            ]
        })
    }

    proptest! {
        #[test]
        fn roundtrip(v in arb_value()) {
            let bytes = encode(&v);
            let back = decode(&bytes).unwrap();
            prop_assert_eq!(encode(&back), bytes);
        }

        #[test]
        fn decoder_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            if let Ok(v) = decode(&bytes) {
                // Whatever is accepted must re-encode to exactly the same bytes.
                prop_assert_eq!(encode(&v), bytes);
            }
        }
    }
}
