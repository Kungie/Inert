//! A strict reader over CBOR maps: every field must be consumed, unknown
//! fields are an error (spec §5.2 "every field not defined by the type").

use crate::cbor::Value;
use crate::error::DecodeError;

pub struct Obj<'a> {
    path: String,
    entries: &'a [(Value, Value)],
    used: Vec<bool>,
}

impl<'a> Obj<'a> {
    pub fn new(v: &'a Value, path: impl Into<String>) -> Result<Self, DecodeError> {
        let path = path.into();
        let entries = v.as_map().ok_or_else(|| DecodeError::Type { path: path.clone(), expected: "map" })?;
        for (k, _) in entries {
            if k.as_text().is_none() {
                return Err(DecodeError::Type { path: path.clone(), expected: "text keys" });
            }
        }
        Ok(Self { path, entries, used: vec![false; entries.len()] })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn sub(&self, field: &str) -> String {
        if self.path.is_empty() { field.to_owned() } else { format!("{}.{}", self.path, field) }
    }

    pub fn opt(&mut self, field: &str) -> Option<&'a Value> {
        for (i, (k, v)) in self.entries.iter().enumerate() {
            if k.as_text() == Some(field) {
                self.used[i] = true;
                return Some(v);
            }
        }
        None
    }

    pub fn req(&mut self, field: &str) -> Result<&'a Value, DecodeError> {
        self.opt(field).ok_or_else(|| DecodeError::Missing { path: self.sub(field) })
    }

    pub fn text(&mut self, field: &str) -> Result<String, DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_text().map(str::to_owned).ok_or(DecodeError::Type { path: p, expected: "text" })
    }

    pub fn opt_text(&mut self, field: &str) -> Result<Option<String>, DecodeError> {
        let p = self.sub(field);
        match self.opt(field) {
            None => Ok(None),
            Some(v) => v.as_text().map(|s| Some(s.to_owned())).ok_or(DecodeError::Type { path: p, expected: "text" }),
        }
    }

    pub fn uint(&mut self, field: &str) -> Result<u64, DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_u64().ok_or(DecodeError::Type { path: p, expected: "unsigned int" })
    }

    pub fn opt_uint(&mut self, field: &str) -> Result<Option<u64>, DecodeError> {
        let p = self.sub(field);
        match self.opt(field) {
            None => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or(DecodeError::Type { path: p, expected: "unsigned int" }),
        }
    }

    pub fn int(&mut self, field: &str) -> Result<i64, DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_i64().ok_or(DecodeError::Type { path: p, expected: "int" })
    }

    pub fn bool(&mut self, field: &str) -> Result<bool, DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_bool().ok_or(DecodeError::Type { path: p, expected: "bool" })
    }

    pub fn bytes(&mut self, field: &str) -> Result<Vec<u8>, DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_bytes().map(<[u8]>::to_vec).ok_or(DecodeError::Type { path: p, expected: "bytes" })
    }

    pub fn fixed<const N: usize>(&mut self, field: &str) -> Result<[u8; N], DecodeError> {
        let p = self.sub(field);
        let b = self.bytes(field)?;
        b.try_into().map_err(|_| DecodeError::Invalid { path: p, msg: format!("expected {N} bytes") })
    }

    pub fn map(&mut self, field: &str) -> Result<&'a [(Value, Value)], DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_map().ok_or(DecodeError::Type { path: p, expected: "map" })
    }

    pub fn array(&mut self, field: &str) -> Result<&'a [Value], DecodeError> {
        let p = self.sub(field);
        self.req(field)?.as_array().ok_or(DecodeError::Type { path: p, expected: "array" })
    }

    /// Fails if any field was not read.
    pub fn finish(self) -> Result<(), DecodeError> {
        for (i, used) in self.used.iter().enumerate() {
            if !used {
                let field = self.entries[i].0.as_text().unwrap_or("?").to_owned();
                return Err(DecodeError::Unknown { path: self.path, field });
            }
        }
        Ok(())
    }
}

/// Reads a CBOR map with text keys into `(key, value)` pairs.
pub fn text_map<'a>(entries: &'a [(Value, Value)], path: &str) -> Result<Vec<(&'a str, &'a Value)>, DecodeError> {
    entries
        .iter()
        .map(|(k, v)| {
            k.as_text().map(|k| (k, v)).ok_or_else(|| DecodeError::Type { path: path.to_owned(), expected: "text keys" })
        })
        .collect()
}
