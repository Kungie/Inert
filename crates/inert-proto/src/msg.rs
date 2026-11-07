//! Wire messages (spec §5.3). Every message is a CBOR map with a `t` field.

use crate::cbor::{self, Value, map};
use crate::error::{DecodeError, ErrorCode};
use crate::obj::{Obj, text_map};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u64 = 1;

/// Slot data / action fields / open params: text keys → values.
pub type Fields = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenMsg {
    pub screen: String,
    pub data: Fields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultError {
    pub code: ErrorCode,
    pub field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Hello { v: u64 },
    Welcome { v: u64, manifest: Vec<u8>, sig: [u8; 64], key_id: String },
    Open { screen: String, params: Fields },
    Screen(ScreenMsg),
    Action { id: [u8; 16], screen: String, action: String, fields: Fields },
    Result { id: [u8; 16], ok: bool, error: Option<ResultError>, next: Option<ScreenMsg> },
    GetAsset { hash: [u8; 32] },
    Asset { hash: [u8; 32], mime: String, bytes: Vec<u8> },
    Fatal { code: ErrorCode },
}

fn enc_fields(f: &Fields) -> Value {
    map(f.iter().map(|(k, v)| (k.clone(), v.clone())))
}

fn dec_fields(o: &mut Obj, field: &str) -> Result<Fields, DecodeError> {
    let p = o.sub(field);
    Ok(text_map(o.map(field)?, &p)?.into_iter().map(|(k, v)| (k.to_owned(), v.clone())).collect())
}

impl ScreenMsg {
    fn to_entries(&self) -> Vec<(&'static str, Value)> {
        vec![("screen", Value::text(self.screen.clone())), ("data", enc_fields(&self.data))]
    }

    fn from_obj(o: &mut Obj) -> Result<Self, DecodeError> {
        Ok(ScreenMsg { screen: o.text("screen")?, data: dec_fields(o, "data")? })
    }
}

impl Message {
    pub fn type_name(&self) -> &'static str {
        match self {
            Message::Hello { .. } => "hello",
            Message::Welcome { .. } => "welcome",
            Message::Open { .. } => "open",
            Message::Screen(_) => "screen",
            Message::Action { .. } => "action",
            Message::Result { .. } => "result",
            Message::GetAsset { .. } => "get_asset",
            Message::Asset { .. } => "asset",
            Message::Fatal { .. } => "fatal",
        }
    }

    /// Whether the message may use the large server → client frame limit.
    pub fn is_large(&self) -> bool {
        matches!(self, Message::Welcome { .. } | Message::Asset { .. })
    }

    pub fn to_value(&self) -> Value {
        let mut e: Vec<(&str, Value)> = vec![("t", Value::text(self.type_name()))];
        match self {
            Message::Hello { v } => e.push(("v", Value::Uint(*v))),
            Message::Welcome { v, manifest, sig, key_id } => {
                e.push(("v", Value::Uint(*v)));
                e.push(("manifest", Value::Bytes(manifest.clone())));
                e.push(("sig", Value::Bytes(sig.to_vec())));
                e.push(("key_id", Value::text(key_id.clone())));
            }
            Message::Open { screen, params } => {
                e.push(("screen", Value::text(screen.clone())));
                e.push(("params", enc_fields(params)));
            }
            Message::Screen(s) => e.extend(s.to_entries()),
            Message::Action { id, screen, action, fields } => {
                e.push(("id", Value::Bytes(id.to_vec())));
                e.push(("screen", Value::text(screen.clone())));
                e.push(("action", Value::text(action.clone())));
                e.push(("fields", enc_fields(fields)));
            }
            Message::Result { id, ok, error, next } => {
                e.push(("id", Value::Bytes(id.to_vec())));
                e.push(("ok", Value::Bool(*ok)));
                if let Some(err) = error {
                    let mut ee = vec![("code", Value::Uint(err.code.as_u64()))];
                    if let Some(f) = &err.field {
                        ee.push(("field", Value::text(f.clone())));
                    }
                    e.push(("error", map(ee)));
                }
                if let Some(n) = next {
                    e.push(("next", map(n.to_entries())));
                }
            }
            Message::GetAsset { hash } => e.push(("hash", Value::Bytes(hash.to_vec()))),
            Message::Asset { hash, mime, bytes } => {
                e.push(("hash", Value::Bytes(hash.to_vec())));
                e.push(("mime", Value::text(mime.clone())));
                e.push(("bytes", Value::Bytes(bytes.clone())));
            }
            Message::Fatal { code } => e.push(("code", Value::Uint(code.as_u64()))),
        }
        map(e)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        cbor::encode(&self.to_value())
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, DecodeError> {
        Self::from_value(&cbor::decode(b)?)
    }

    pub fn from_value(v: &Value) -> Result<Self, DecodeError> {
        let mut o = Obj::new(v, "")?;
        let t = o.text("t")?;
        let code_of = |o: &mut Obj, field: &str| -> Result<ErrorCode, DecodeError> {
            let p = o.sub(field);
            let n = o.uint(field)?;
            ErrorCode::from_u64(n).ok_or(DecodeError::Invalid { path: p, msg: format!("unknown error code {n}") })
        };
        let m = match t.as_str() {
            "hello" => Message::Hello { v: o.uint("v")? },
            "welcome" => Message::Welcome { v: o.uint("v")?, manifest: o.bytes("manifest")?, sig: o.fixed("sig")?, key_id: o.text("key_id")? },
            "open" => Message::Open { screen: o.text("screen")?, params: dec_fields(&mut o, "params")? },
            "screen" => Message::Screen(ScreenMsg::from_obj(&mut o)?),
            "action" => Message::Action {
                id: o.fixed("id")?,
                screen: o.text("screen")?,
                action: o.text("action")?,
                fields: dec_fields(&mut o, "fields")?,
            },
            "result" => {
                let id = o.fixed("id")?;
                let ok = o.bool("ok")?;
                let error = match o.opt("error") {
                    None => None,
                    Some(ev) => {
                        let mut eo = Obj::new(ev, "error")?;
                        let code = code_of(&mut eo, "code")?;
                        let field = eo.opt_text("field")?;
                        eo.finish()?;
                        Some(ResultError { code, field })
                    }
                };
                let next = match o.opt("next") {
                    None => None,
                    Some(nv) => {
                        let mut no = Obj::new(nv, "next")?;
                        let s = ScreenMsg::from_obj(&mut no)?;
                        no.finish()?;
                        Some(s)
                    }
                };
                if ok == error.is_some() {
                    return Err(DecodeError::Invalid { path: "result".into(), msg: "`error` must be present exactly when ok is false".into() });
                }
                Message::Result { id, ok, error, next }
            }
            "get_asset" => Message::GetAsset { hash: o.fixed("hash")? },
            "asset" => Message::Asset { hash: o.fixed("hash")?, mime: o.text("mime")?, bytes: o.bytes("bytes")? },
            "fatal" => Message::Fatal { code: code_of(&mut o, "code")? },
            other => return Err(DecodeError::UnknownMessage(other.to_owned())),
        };
        o.finish()?;
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(m: Message) {
        let b = m.to_bytes();
        assert_eq!(Message::from_bytes(&b).unwrap(), m);
    }

    #[test]
    fn roundtrips() {
        rt(Message::Hello { v: 1 });
        rt(Message::Welcome { v: 1, manifest: vec![1, 2, 3], sig: [7; 64], key_id: "k".into() });
        rt(Message::Open { screen: "overview".into(), params: Fields::new() });
        let mut data = Fields::new();
        data.insert("balance".into(), Value::text("12.00"));
        rt(Message::Screen(ScreenMsg { screen: "overview".into(), data: data.clone() }));
        rt(Message::Action { id: [1; 16], screen: "s".into(), action: "a".into(), fields: data.clone() });
        rt(Message::Result { id: [1; 16], ok: true, error: None, next: Some(ScreenMsg { screen: "x".into(), data }) });
        rt(Message::Result { id: [1; 16], ok: false, error: Some(ResultError { code: ErrorCode::FieldInvalid, field: Some("amount".into()) }), next: None });
        rt(Message::GetAsset { hash: [9; 32] });
        rt(Message::Asset { hash: [9; 32], mime: "image/png".into(), bytes: vec![0; 10] });
        rt(Message::Fatal { code: ErrorCode::BadSignature });
    }

    #[test]
    fn rejects_unknown_fields_and_types() {
        let v = map([("t", Value::text("hello")), ("v", Value::Uint(1)), ("x", Value::Null)]);
        assert!(matches!(Message::from_value(&v), Err(DecodeError::Unknown { .. })));
        let v = map([("t", Value::text("nope"))]);
        assert!(matches!(Message::from_value(&v), Err(DecodeError::UnknownMessage(_))));
        let v = map([("t", Value::text("result")), ("id", Value::Bytes(vec![0; 16])), ("ok", Value::Bool(false))]);
        assert!(Message::from_value(&v).is_err());
    }
}
