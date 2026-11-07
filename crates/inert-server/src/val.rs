//! Small helpers for building slot data and reading validated fields.

use inert_proto::{Decimal, Fields, Hash, Value};

pub fn text(s: impl Into<String>) -> Value {
    Value::text(s)
}

pub fn int(n: i64) -> Value {
    Value::int(n)
}

pub fn dec(mantissa: i64, scale: u32) -> Value {
    Decimal::new(mantissa, scale).to_value()
}

pub fn decimal(d: Decimal) -> Value {
    d.to_value()
}

pub fn bool(b: bool) -> Value {
    Value::Bool(b)
}

pub fn image(h: Hash) -> Value {
    Value::Bytes(h.to_vec())
}

pub fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(items.into_iter().collect())
}

pub fn obj<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
    inert_proto::cbor::map(entries)
}

/// Builder for a screen's slot data.
#[derive(Default, Debug, Clone)]
pub struct Data(pub Fields);

impl Data {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(mut self, k: &str, v: Value) -> Self {
        self.0.insert(k.to_owned(), v);
        self
    }

    pub fn into_fields(self) -> Fields {
        self.0
    }
}

/// Read access to already-validated action fields or open params.
/// Getters panic only on programmer error (asking for a field the manifest
/// does not declare with that type); the panic is caught and reported as `internal`.
pub struct Params<'a>(pub &'a Fields);

impl<'a> Params<'a> {
    pub fn get(&self, k: &str) -> Option<&'a Value> {
        self.0.get(k)
    }

    pub fn text(&self, k: &str) -> &'a str {
        self.0.get(k).and_then(Value::as_text).unwrap_or_else(|| panic!("field {k:?} is not text"))
    }

    pub fn int(&self, k: &str) -> i64 {
        self.0.get(k).and_then(Value::as_i64).unwrap_or_else(|| panic!("field {k:?} is not an int"))
    }

    pub fn decimal(&self, k: &str) -> Decimal {
        self.0.get(k).and_then(Decimal::from_value).unwrap_or_else(|| panic!("field {k:?} is not a decimal"))
    }

    pub fn bool(&self, k: &str) -> bool {
        self.0.get(k).and_then(Value::as_bool).unwrap_or_else(|| panic!("field {k:?} is not a bool"))
    }

    /// `enum` and `choice` values are text.
    pub fn choice(&self, k: &str) -> &'a str {
        self.text(k)
    }
}
