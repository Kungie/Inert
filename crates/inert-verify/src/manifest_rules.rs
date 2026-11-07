//! Static manifest rules (spec §7.1). Run by `inert-sign build` before
//! signing and by the browser after verifying the signature.

use crate::contrast::{MIN_TEXT_CONTRAST, ratio};
use crate::values::check_field_value;
use crate::{Result, VerifyError};
use inert_proto::limits::*;
use inert_proto::*;
use std::collections::{BTreeMap, BTreeSet};

fn err(path: impl Into<String>, msg: impl Into<String>) -> VerifyError {
    VerifyError::new(ErrorCode::ManifestInvalid, path, msg)
}

pub fn check_manifest(m: &Manifest) -> Result<()> {
    check_header(m)?;
    check_design(&m.design)?;
    check_assets(m)?;
    if m.screens.is_empty() || m.screens.len() > MAX_SCREENS {
        return Err(err("screens", format!("need 1..={MAX_SCREENS} screens")));
    }
    if m.actions.len() > MAX_ACTIONS {
        return Err(err("actions", format!("at most {MAX_ACTIONS} actions")));
    }
    if !m.screens.contains_key(&m.entry) {
        return Err(err("entry", format!("entry screen {:?} does not exist", m.entry)));
    }
    for (name, a) in &m.actions {
        let p = format!("actions.{name}");
        check_ident(name, &p)?;
        if a.fields.len() > MAX_FIELDS {
            return Err(err(&p, "too many fields"));
        }
        for (f, t) in &a.fields {
            check_ident(f, &format!("{p}.fields.{f}"))?;
            check_field_type(t, &format!("{p}.fields.{f}"))?;
        }
    }
    let mut triggered: BTreeSet<&str> = BTreeSet::new();
    for (name, s) in &m.screens {
        check_screen_def(m, name, s, &mut triggered)?;
    }
    Ok(())
}

fn check_ident(s: &str, path: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok { Ok(()) } else { Err(err(path, format!("{s:?} is not a valid name (lowercase letters, digits, _)"))) }
}

fn check_header(m: &Manifest) -> Result<()> {
    let domain_ok = !m.app.is_empty()
        && m.app.len() <= 253
        && m.app.split('.').count() >= 2
        && m.app.split('.').all(|l| {
            !l.is_empty() && l.len() <= 63 && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !l.starts_with('-') && !l.ends_with('-')
        });
    if !domain_ok {
        return Err(err("app", format!("{:?} is not a valid lowercase domain name", m.app)));
    }
    if m.name.trim().is_empty() || m.name.chars().count() > 64 {
        return Err(err("name", "name must be 1..=64 characters"));
    }
    if m.version == 0 {
        return Err(err("version", "version must be at least 1"));
    }
    Ok(())
}

fn check_design(d: &Design) -> Result<()> {
    let c = &d.colors;
    for (pair, a, b) in [("fg/bg", c.fg, c.bg), ("muted/bg", c.muted, c.bg), ("accent/bg", c.accent, c.bg), ("on_accent/accent", c.on_accent, c.accent)] {
        let r = ratio(a, b);
        if r < MIN_TEXT_CONTRAST {
            return Err(err(
                format!("design.colors.{pair}"),
                format!("contrast {r:.2}:1 is below {MIN_TEXT_CONTRAST}:1 (unreadable or invisible text is not allowed)"),
            ));
        }
    }
    for s in TextStyle::ALL {
        let v = d.size(s);
        if !(10..=160).contains(&v) {
            return Err(err(format!("design.sizes.{}", s.name()), "text size must be 10..=160 px"));
        }
    }
    for s in Spacing::ALL {
        if d.space(s) > 256 {
            return Err(err(format!("design.spacing.{}", s.name()), "spacing must be <= 256 px"));
        }
    }
    if d.radius > 64 {
        return Err(err("design.radius", "radius must be <= 64 px"));
    }
    Ok(())
}

fn check_image_meta(mime: &str, bytes: u64, w: u64, h: u64, path: &str) -> Result<()> {
    if mime != "image/png" && mime != "image/webp" {
        return Err(err(path, "only image/png and image/webp are allowed"));
    }
    if bytes == 0 || bytes > IMAGE_MAX_BYTES {
        return Err(err(path, "image too large"));
    }
    if w == 0 || h == 0 || w > IMAGE_MAX_SIDE || h > IMAGE_MAX_SIDE || w * h > IMAGE_MAX_PIXELS {
        return Err(err(path, "image dimensions out of bounds"));
    }
    Ok(())
}

fn check_assets(m: &Manifest) -> Result<()> {
    for (h, a) in &m.assets {
        check_image_meta(&a.mime, a.bytes, a.width, a.height, &format!("assets.{}", hex::encode(&h[..6])))?;
    }
    Ok(())
}

fn check_slot_type(t: &SlotType, path: &str, nested: bool) -> Result<()> {
    match t {
        SlotType::String { max } => {
            if *max == 0 || *max > STRING_MAX {
                return Err(err(path, format!("string max must be 1..={STRING_MAX}")));
            }
        }
        SlotType::Int { min, max } => {
            if min > max {
                return Err(err(path, "min > max"));
            }
        }
        SlotType::Decimal { scale, min, max } => check_decimal_type(*scale, *min, *max, path)?,
        SlotType::Bool | SlotType::Image => {}
        SlotType::List { max, item } => {
            if nested {
                return Err(err(path, "lists cannot be nested"));
            }
            if *max == 0 || *max > LIST_MAX {
                return Err(err(path, format!("list max must be 1..={LIST_MAX}")));
            }
            if item.is_empty() || item.len() > MAX_FIELDS {
                return Err(err(path, "list item needs 1..=64 fields"));
            }
            for (k, v) in item {
                check_ident(k, &format!("{path}.item.{k}"))?;
                check_slot_type(v, &format!("{path}.item.{k}"), true)?;
            }
        }
    }
    Ok(())
}

fn check_decimal_type(scale: u32, min: Decimal, max: Decimal, path: &str) -> Result<()> {
    if scale > DECIMAL_MAX_SCALE {
        return Err(err(path, "scale too large"));
    }
    if min.scale > scale || max.scale > scale {
        return Err(err(path, "min/max have more decimal places than scale"));
    }
    if min.cmp_num(max) == std::cmp::Ordering::Greater {
        return Err(err(path, "min > max"));
    }
    Ok(())
}

fn check_field_type(t: &FieldType, path: &str) -> Result<()> {
    match t {
        FieldType::String { max } => {
            if *max == 0 || *max > STRING_MAX {
                return Err(err(path, format!("string max must be 1..={STRING_MAX}")));
            }
        }
        FieldType::Int { min, max } => {
            if min > max {
                return Err(err(path, "min > max"));
            }
        }
        FieldType::Decimal { scale, min, max } => check_decimal_type(*scale, *min, *max, path)?,
        FieldType::Bool => {}
        FieldType::Enum { options } => {
            if options.is_empty() || options.len() > ENUM_MAX_OPTIONS {
                return Err(err(path, format!("enum needs 1..={ENUM_MAX_OPTIONS} options")));
            }
            let mut seen = BTreeSet::new();
            for o in options {
                if o.value.is_empty() || o.value.len() > 64 || o.label.trim().is_empty() || o.label.chars().count() > 128 {
                    return Err(err(path, "enum option value/label out of bounds"));
                }
                if !seen.insert(&o.value) {
                    return Err(err(path, format!("duplicate enum value {:?}", o.value)));
                }
            }
        }
        FieldType::Choice { from } => check_ident(from, path)?,
    }
    Ok(())
}

/// Resolves a bind path in a screen. `scope` is the list slot the node sits in.
fn resolve<'a>(s: &'a ScreenDef, scope: Option<&str>, path: &str) -> Option<&'a SlotType> {
    if let Some(rest) = path.strip_prefix("item.") {
        let list = s.slots.get(scope?)?;
        match list {
            SlotType::List { item, .. } => item.get(rest),
            _ => None,
        }
    } else {
        s.slots.get(path)
    }
}

/// Whether a slot value of type `slot` can always be used as a value of field type `field`.
fn bind_compatible(slot: &SlotType, field: &FieldType) -> bool {
    match (slot, field) {
        (SlotType::String { max: smax }, FieldType::String { max: fmax }) => smax <= fmax,
        (SlotType::String { .. }, FieldType::Enum { .. } | FieldType::Choice { .. }) => true,
        (SlotType::Int { min: a, max: b }, FieldType::Int { min: c, max: d }) => a >= c && b <= d,
        (SlotType::Decimal { scale: s1, min: a, max: b }, FieldType::Decimal { scale: s2, min: c, max: d }) => {
            s1 <= s2 && a.cmp_num(*c).is_ge() && b.cmp_num(*d).is_le()
        }
        (SlotType::Bool, FieldType::Bool) => true,
        _ => false,
    }
}

fn check_arg(s: &ScreenDef, scope: Option<&str>, a: &ArgSrc, t: &FieldType, path: &str) -> Result<()> {
    match a {
        ArgSrc::Lit(v) => {
            if matches!(t, FieldType::Choice { .. }) {
                return Err(err(path, "a choice field must be bound to screen data, not a literal"));
            }
            check_field_value(t, v).map_err(|e| err(path, e))
        }
        ArgSrc::Bind(p) => {
            let st = resolve(s, scope, p).ok_or_else(|| err(path, format!("bind {p:?} does not resolve")))?;
            if bind_compatible(st, t) { Ok(()) } else { Err(err(path, format!("slot {p:?} ({}) cannot fill a {} field", st.name(), t.name()))) }
        }
    }
}

fn check_screen_def<'m>(m: &'m Manifest, name: &str, s: &'m ScreenDef, triggered: &mut BTreeSet<&'m str>) -> Result<()> {
    let p = format!("screens.{name}");
    check_ident(name, &p)?;
    if s.title.trim().is_empty() || s.title.chars().count() > 128 {
        return Err(err(format!("{p}.title"), "title must be 1..=128 characters"));
    }
    if s.params.len() > MAX_FIELDS || s.slots.len() > MAX_FIELDS {
        return Err(err(&p, "too many params or slots"));
    }
    for (k, t) in &s.params {
        let pp = format!("{p}.params.{k}");
        check_ident(k, &pp)?;
        if matches!(t, FieldType::Choice { .. }) {
            return Err(err(pp, "screen params cannot be choice fields"));
        }
        check_field_type(t, &pp)?;
    }
    for (k, t) in &s.slots {
        check_ident(k, &format!("{p}.slots.{k}"))?;
        check_slot_type(t, &format!("{p}.slots.{k}"), false)?;
    }

    // Collect form fields and buttons while checking every node.
    let mut forms: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut buttons: Vec<(&str, &BTreeMap<String, ArgSrc>, String)> = Vec::new();
    let mut count = 0usize;
    let mut problem: Option<VerifyError> = None;
    let mut idx = 0usize;

    s.root.walk(&mut |n, depth, scope| {
        if problem.is_some() {
            return;
        }
        idx += 1;
        count += 1;
        let np = format!("{p}.root#{idx}({})", n.kind());
        let r = (|| -> Result<()> {
            if depth > TEMPLATE_MAX_DEPTH {
                return Err(err(&np, format!("template deeper than {TEMPLATE_MAX_DEPTH}")));
            }
            match n {
                Node::Stack { children, .. } => {
                    if children.len() > 256 {
                        return Err(err(&np, "too many children"));
                    }
                }
                Node::Text { text, .. } => match text {
                    TextSrc::Lit(t) => {
                        crate::values::check_slot_value(&SlotType::String { max: STRING_MAX }, &Value::text(t.clone())).map_err(|e| err(&np, e))?;
                    }
                    TextSrc::Bind(b) => {
                        let t = resolve(s, scope, b).ok_or_else(|| err(&np, format!("bind {b:?} does not resolve")))?;
                        if !matches!(t, SlotType::String { .. } | SlotType::Int { .. } | SlotType::Decimal { .. }) {
                            return Err(err(&np, format!("text cannot show a {} slot", t.name())));
                        }
                    }
                },
                Node::Button { label, action, args, .. } => {
                    check_label(label, &np)?;
                    let a = m.actions.get(action).ok_or_else(|| err(&np, format!("unknown action {action:?}")))?;
                    for (f, src) in args {
                        let t = a.fields.get(f).ok_or_else(|| err(&np, format!("action {action:?} has no field {f:?}")))?;
                        check_arg(s, scope, src, t, &format!("{np}.args.{f}"))?;
                    }
                    buttons.push((action.as_str(), args, np.clone()));
                }
                Node::Input { action, field, label, kind, placeholder, init } => {
                    check_label(label, &np)?;
                    if let Some(ph) = placeholder {
                        check_label(ph, &np)?;
                    }
                    let t = form_field(m, scope, action, field, &np)?;
                    let ok = match kind {
                        InputKind::Text | InputKind::Multiline => matches!(t, FieldType::String { .. }),
                        InputKind::Number => matches!(t, FieldType::Int { .. } | FieldType::Decimal { .. }),
                    };
                    if !ok {
                        return Err(err(&np, format!("input kind does not fit a {} field", t.name())));
                    }
                    check_init(s, init, t, &np)?;
                    add_form(&mut forms, action, field, &np)?;
                }
                Node::Select { action, field, label, init } => {
                    check_label(label, &np)?;
                    let t = form_field(m, scope, action, field, &np)?;
                    check_init(s, init, t, &np)?;
                    match t {
                        FieldType::Enum { .. } => {}
                        FieldType::Choice { from } => match s.slots.get(from) {
                            Some(SlotType::List { item, .. })
                                if matches!(item.get("id"), Some(SlotType::String { .. }))
                                    && matches!(item.get("label"), Some(SlotType::String { .. })) => {}
                            _ => return Err(err(&np, format!("choice source {from:?} must be a list slot with string `id` and `label`"))),
                        },
                        _ => return Err(err(&np, "select needs an enum or choice field")),
                    }
                    add_form(&mut forms, action, field, &np)?;
                }
                Node::Toggle { action, field, label, init } => {
                    check_label(label, &np)?;
                    let t = form_field(m, scope, action, field, &np)?;
                    check_init(s, init, t, &np)?;
                    if !matches!(t, FieldType::Bool) {
                        return Err(err(&np, "toggle needs a bool field"));
                    }
                    add_form(&mut forms, action, field, &np)?;
                }
                Node::List { bind, item } => {
                    if scope.is_some() {
                        return Err(err(&np, "lists cannot be nested"));
                    }
                    match s.slots.get(bind) {
                        Some(SlotType::List { .. }) => {}
                        _ => return Err(err(&np, format!("{bind:?} is not a list slot"))),
                    }
                    let _ = item;
                }
                Node::Divider => {}
                Node::Image { src, alt, width, height } => {
                    if alt.trim().is_empty() {
                        return Err(err(&np, "image needs alt text"));
                    }
                    check_label(alt, &np)?;
                    if *width == 0 || *height == 0 || *width > 2048 || *height > 2048 {
                        return Err(err(&np, "image display size must be 1..=2048"));
                    }
                    match src {
                        ImageSrc::Asset(h) => {
                            if !m.assets.contains_key(h) {
                                return Err(err(&np, "static image is not listed in assets"));
                            }
                        }
                        ImageSrc::Bind(b) => match resolve(s, scope, b) {
                            Some(SlotType::Image) => {}
                            _ => return Err(err(&np, format!("{b:?} is not an image slot"))),
                        },
                    }
                }
                Node::Link { label, target, params } => {
                    check_label(label, &np)?;
                    match target {
                        LinkTarget::Screen(to) => {
                            let ts = m.screens.get(to).ok_or_else(|| err(&np, format!("unknown screen {to:?}")))?;
                            let want: BTreeSet<&String> = ts.params.keys().collect();
                            let got: BTreeSet<&String> = params.keys().collect();
                            if want != got {
                                return Err(err(&np, format!("link params must be exactly {want:?}")));
                            }
                            for (k, src) in params {
                                check_arg(s, scope, src, &ts.params[k], &format!("{np}.params.{k}"))?;
                            }
                        }
                        LinkTarget::Href(h) => {
                            if !(h.starts_with("inert://") || h.starts_with("https://")) || h.len() > 2048 || h.chars().any(char::is_whitespace) {
                                return Err(err(&np, "href must be an inert:// or https:// URL"));
                            }
                            if !params.is_empty() {
                                return Err(err(&np, "external links take no params"));
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            problem = Some(e);
        }
    });
    if let Some(e) = problem {
        return Err(e);
    }
    if count > MAX_NODES_PER_SCREEN {
        return Err(err(&p, format!("more than {MAX_NODES_PER_SCREEN} nodes")));
    }

    // Form / action coherence: every button's form fields + args must be the action's field set.
    let mut actions_with_button: BTreeSet<&str> = BTreeSet::new();
    for (action, args, np) in &buttons {
        actions_with_button.insert(action);
        triggered.insert(action);
        let def = &m.actions[*action];
        let form = forms.get(action).cloned().unwrap_or_default();
        for f in args.keys() {
            if form.contains(f.as_str()) {
                return Err(err(np, format!("field {f:?} is both a form field and a button arg")));
            }
        }
        let mut have: BTreeSet<&str> = form;
        have.extend(args.keys().map(String::as_str));
        let want: BTreeSet<&str> = def.fields.keys().map(String::as_str).collect();
        if have != want {
            let missing: Vec<_> = want.difference(&have).collect();
            return Err(err(np, format!("button for {action:?} does not provide fields {missing:?}")));
        }
        // Choice fields must point at a list slot with id/label in this screen.
        for (f, t) in &def.fields {
            if let FieldType::Choice { from } = t {
                match s.slots.get(from) {
                    Some(SlotType::List { item, .. })
                        if matches!(item.get("id"), Some(SlotType::String { .. })) && matches!(item.get("label"), Some(SlotType::String { .. })) => {}
                    _ => return Err(err(np, format!("choice field {f:?} needs list slot {from:?} with string `id` and `label` in this screen"))),
                }
            }
        }
    }
    for action in forms.keys() {
        if !actions_with_button.contains(action) {
            return Err(err(&p, format!("form fields for {action:?} but no button triggers it")));
        }
    }
    Ok(())
}

/// A form field's initial value: a literal that fits the field, or a bind
/// (top-level slots only, since form fields cannot sit inside lists).
fn check_init(s: &ScreenDef, init: &Option<ArgSrc>, t: &FieldType, np: &str) -> Result<()> {
    match init {
        None => Ok(()),
        Some(a) => check_arg(s, None, a, t, &format!("{np}.init")),
    }
}

fn check_label(s: &str, path: &str) -> Result<()> {
    if s.trim().is_empty() || s.chars().count() > 256 {
        return Err(err(path, "label must be 1..=256 characters"));
    }
    crate::values::check_slot_value(&SlotType::String { max: 256 }, &Value::text(s)).map_err(|e| err(path, e))
}

fn form_field<'a>(m: &'a Manifest, scope: Option<&str>, action: &str, field: &str, np: &str) -> Result<&'a FieldType> {
    if scope.is_some() {
        return Err(err(np, "form fields cannot sit inside a list"));
    }
    let a = m.actions.get(action).ok_or_else(|| err(np, format!("unknown action {action:?}")))?;
    a.fields.get(field).ok_or_else(|| err(np, format!("action {action:?} has no field {field:?}")))
}

fn add_form<'a>(forms: &mut BTreeMap<&'a str, BTreeSet<&'a str>>, action: &'a str, field: &'a str, np: &str) -> Result<()> {
    if !forms.entry(action).or_default().insert(field) {
        return Err(err(np, format!("field {field:?} of {action:?} appears twice")));
    }
    Ok(())
}
