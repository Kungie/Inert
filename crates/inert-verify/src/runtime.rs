//! Runtime conformance: `Screen` (§7.2), `Action` (§7.3), `Open` (§7.4).

use crate::values::{check_field_value, check_slot_value};
use crate::{Result, VerifyError};
use inert_proto::*;
use std::collections::BTreeSet;

/// Checks that a `Screen` message fills exactly the template's slots with valid values.
pub fn check_screen(m: &Manifest, s: &ScreenMsg) -> Result<()> {
    let err = |path: String, msg: String| VerifyError::new(ErrorCode::ScreenInvalid, path, msg);
    let def = m.screens.get(&s.screen).ok_or_else(|| err("screen".into(), format!("unknown screen {:?}", s.screen)))?;
    for k in s.data.keys() {
        if !def.slots.contains_key(k) {
            return Err(err(format!("data.{k}"), "not a slot of this screen".into()));
        }
    }
    for (k, t) in &def.slots {
        let v = s.data.get(k).ok_or_else(|| err(format!("data.{k}"), "missing slot".into()))?;
        check_slot_value(t, v).map_err(|e| err(format!("data.{k}"), e))?;
    }
    Ok(())
}

fn check_fields(want: &std::collections::BTreeMap<String, FieldType>, got: &Fields) -> Result<()> {
    for k in got.keys() {
        if !want.contains_key(k) {
            return Err(VerifyError::field(ErrorCode::FieldInvalid, k.clone(), "unexpected field"));
        }
    }
    for (k, t) in want {
        let v = got.get(k).ok_or_else(|| VerifyError::field(ErrorCode::FieldInvalid, k.clone(), "missing field"))?;
        check_field_value(t, v).map_err(|e| VerifyError::field(ErrorCode::FieldInvalid, k.clone(), e))?;
    }
    Ok(())
}

/// Checks an `Action`. `screen_data` is the data of the last `Screen` the
/// session sent for `screen` (needed for `choice` fields).
pub fn check_action(m: &Manifest, screen: &str, screen_data: Option<&Fields>, action: &str, fields: &Fields) -> Result<()> {
    let def = m
        .actions
        .get(action)
        .ok_or_else(|| VerifyError::new(ErrorCode::ActionUnknown, "action", format!("unknown action {action:?}")))?;
    let sdef = m
        .screens
        .get(screen)
        .ok_or_else(|| VerifyError::new(ErrorCode::ActionUnknown, "screen", format!("unknown screen {screen:?}")))?;
    let mut triggers = false;
    sdef.root.walk(&mut |n, _, _| {
        if let Node::Button { action: a, .. } = n {
            if a == action {
                triggers = true;
            }
        }
    });
    if !triggers {
        return Err(VerifyError::new(ErrorCode::ActionUnknown, "action", format!("{action:?} cannot be triggered from screen {screen:?}")));
    }
    check_fields(&def.fields, fields)?;
    for (k, t) in &def.fields {
        if let FieldType::Choice { from } = t {
            let chosen = fields[k].as_text().unwrap_or_default();
            let data = screen_data.ok_or_else(|| VerifyError::field(ErrorCode::FieldInvalid, k.clone(), "no screen data to choose from"))?;
            let ids: BTreeSet<&str> = data
                .get(from)
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|it| it.get("id").and_then(Value::as_text)).collect())
                .unwrap_or_default();
            if !ids.contains(chosen) {
                return Err(VerifyError::field(ErrorCode::FieldInvalid, k.clone(), format!("{chosen:?} is not one of the offered choices")));
            }
        }
    }
    Ok(())
}

/// Checks an `Open` request's params.
pub fn check_open(m: &Manifest, screen: &str, params: &Fields) -> Result<()> {
    let def = m
        .screens
        .get(screen)
        .ok_or_else(|| VerifyError::new(ErrorCode::FieldInvalid, "screen", format!("unknown screen {screen:?}")))?;
    check_fields(&def.params, params)
}
