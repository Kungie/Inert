//! Value conformance for slot types and field types.

use inert_proto::limits::{LIST_MAX, STRING_MAX};
use inert_proto::{Decimal, FieldType, SlotType, Value};
use std::cmp::Ordering;

fn check_string(v: &Value, max: u64) -> Result<(), String> {
    let s = v.as_text().ok_or("expected text")?;
    let n = s.chars().count() as u64;
    if n > max.min(STRING_MAX) {
        return Err(format!("text longer than {max} characters"));
    }
    // Newlines and tabs are fine (multiline input); every other control
    // character is rejected so nothing can hide in rendered text.
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("text contains control characters".into());
    }
    if s.chars().any(|c| matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')) {
        return Err("text contains invisible or bidi control characters".into());
    }
    Ok(())
}

fn check_int(v: &Value, min: i64, max: i64) -> Result<(), String> {
    let n = v.as_i64().ok_or("expected int")?;
    if n < min || n > max {
        return Err(format!("{n} is outside {min}..={max}"));
    }
    Ok(())
}

fn check_decimal(v: &Value, scale: u32, min: Decimal, max: Decimal) -> Result<(), String> {
    let d = Decimal::from_value(v).ok_or("expected decimal [mantissa, scale]")?;
    if d.scale > scale {
        return Err(format!("more than {scale} decimal places"));
    }
    if d.cmp_num(min) == Ordering::Less || d.cmp_num(max) == Ordering::Greater {
        return Err(format!("{d} is outside {min}..={max}"));
    }
    Ok(())
}

/// Checks a runtime slot value against its declared type.
pub fn check_slot_value(t: &SlotType, v: &Value) -> Result<(), String> {
    match t {
        SlotType::String { max } => check_string(v, *max),
        SlotType::Int { min, max } => check_int(v, *min, *max),
        SlotType::Decimal { scale, min, max } => check_decimal(v, *scale, *min, *max),
        SlotType::Bool => v.as_bool().map(|_| ()).ok_or_else(|| "expected bool".into()),
        SlotType::Image => match v.as_bytes() {
            Some(b) if b.len() == 32 => Ok(()),
            _ => Err("expected a 32-byte image hash".into()),
        },
        SlotType::List { max, item } => {
            let a = v.as_array().ok_or("expected array")?;
            if a.len() as u64 > (*max).min(LIST_MAX) {
                return Err(format!("list longer than {max}"));
            }
            for (i, it) in a.iter().enumerate() {
                let m = it.as_map().ok_or_else(|| format!("[{i}]: expected map"))?;
                if m.len() != item.len() {
                    return Err(format!("[{i}]: expected exactly the fields {:?}", item.keys().collect::<Vec<_>>()));
                }
                for (k, iv) in m {
                    let k = k.as_text().ok_or_else(|| format!("[{i}]: keys must be text"))?;
                    let ty = item.get(k).ok_or_else(|| format!("[{i}]: unexpected field {k:?}"))?;
                    check_slot_value(ty, iv).map_err(|e| format!("[{i}].{k}: {e}"))?;
                }
            }
            Ok(())
        }
    }
}

/// Checks a field value (action field or open param) against its type.
/// `choice` membership needs screen data and is checked by `check_action`.
pub fn check_field_value(t: &FieldType, v: &Value) -> Result<(), String> {
    match t {
        FieldType::String { max } => check_string(v, *max),
        FieldType::Int { min, max } => check_int(v, *min, *max),
        FieldType::Decimal { scale, min, max } => check_decimal(v, *scale, *min, *max),
        FieldType::Bool => v.as_bool().map(|_| ()).ok_or_else(|| "expected bool".into()),
        FieldType::Enum { options } => {
            let s = v.as_text().ok_or("expected text")?;
            if options.iter().any(|o| o.value == s) { Ok(()) } else { Err(format!("{s:?} is not one of the allowed options")) }
        }
        FieldType::Choice { .. } => {
            let s = v.as_text().ok_or("expected text")?;
            check_string(v, 256)?;
            if s.is_empty() { Err("empty choice".into()) } else { Ok(()) }
        }
    }
}
