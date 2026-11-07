//! App Manifest types (spec §6) and their CBOR mapping.
//!
//! Structural decoding lives here; semantic rules (bindings resolve, contrast,
//! form/action coherence, limits) live in `inert-verify`.

use crate::cbor::{Value, map};
use crate::decimal::Decimal;
use crate::error::DecodeError;
use crate::obj::{Obj, text_map};
use std::collections::BTreeMap;

pub type Hash = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Domain the manifest belongs to, e.g. `bank.example`.
    pub app: String,
    /// Human-readable app name shown in the browser.
    pub name: String,
    /// Monotonic app version; browsers reject rollbacks (spec §8).
    pub version: u64,
    pub entry: String,
    pub design: Design,
    pub assets: BTreeMap<Hash, AssetMeta>,
    pub screens: BTreeMap<String, ScreenDef>,
    pub actions: BTreeMap<String, ActionDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetMeta {
    pub mime: String,
    pub bytes: u64,
    pub width: u64,
    pub height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenDef {
    pub title: String,
    pub params: BTreeMap<String, FieldType>,
    pub slots: BTreeMap<String, SlotType>,
    pub root: Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDef {
    pub fields: BTreeMap<String, FieldType>,
}

// ---------------------------------------------------------------- design

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Self> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(p(0)?, p(2)?, p(4)?))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    GeistSerif,
    GeistSans,
    GeistMono,
    GeistPixel,
}

impl FontFamily {
    pub fn name(self) -> &'static str {
        match self {
            FontFamily::GeistSerif => "geist-serif",
            FontFamily::GeistSans => "geist-sans",
            FontFamily::GeistMono => "geist-mono",
            FontFamily::GeistPixel => "geist-pixel",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "geist-serif" => FontFamily::GeistSerif,
            "geist-sans" => FontFamily::GeistSans,
            "geist-mono" => FontFamily::GeistMono,
            "geist-pixel" => FontFamily::GeistPixel,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    Display,
    Title,
    Body,
    Mono,
    Label,
}

impl TextStyle {
    pub const ALL: [TextStyle; 5] = [TextStyle::Display, TextStyle::Title, TextStyle::Body, TextStyle::Mono, TextStyle::Label];

    pub fn name(self) -> &'static str {
        match self {
            TextStyle::Display => "display",
            TextStyle::Title => "title",
            TextStyle::Body => "body",
            TextStyle::Mono => "mono",
            TextStyle::Label => "label",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        TextStyle::ALL.into_iter().find(|t| t.name() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    Xs,
    S,
    M,
    L,
    Xl,
}

impl Spacing {
    pub const ALL: [Spacing; 5] = [Spacing::Xs, Spacing::S, Spacing::M, Spacing::L, Spacing::Xl];

    pub fn name(self) -> &'static str {
        match self {
            Spacing::Xs => "xs",
            Spacing::S => "s",
            Spacing::M => "m",
            Spacing::L => "l",
            Spacing::Xl => "xl",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Spacing::ALL.into_iter().find(|t| t.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colors {
    pub bg: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub line: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Design {
    pub colors: Colors,
    /// Indexed by `TextStyle as usize`.
    pub fonts: [FontFamily; 5],
    pub sizes: [u64; 5],
    /// Indexed by `Spacing as usize`.
    pub spacing: [u64; 5],
    pub radius: u64,
}

impl Design {
    pub fn font(&self, s: TextStyle) -> FontFamily {
        self.fonts[s as usize]
    }
    pub fn size(&self, s: TextStyle) -> u64 {
        self.sizes[s as usize]
    }
    pub fn space(&self, s: Spacing) -> u64 {
        self.spacing[s as usize]
    }
}

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotType {
    String { max: u64 },
    Int { min: i64, max: i64 },
    Decimal { scale: u32, min: Decimal, max: Decimal },
    Bool,
    Image,
    List { max: u64, item: BTreeMap<String, SlotType> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    String { max: u64 },
    Int { min: i64, max: i64 },
    Decimal { scale: u32, min: Decimal, max: Decimal },
    Bool,
    Enum { options: Vec<EnumOption> },
    /// Value must be the `id` of an item in the named list slot of the current screen.
    Choice { from: String },
}

// ---------------------------------------------------------------- nodes

/// A text that is either fixed in the manifest or bound to a slot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSrc {
    Lit(String),
    Bind(String),
}

/// A field value that is either a literal or bound to a slot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgSrc {
    Lit(Value),
    Bind(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSrc {
    Asset(Hash),
    Bind(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    V,
    H,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Ghost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Text,
    Number,
    Multiline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// Another screen of the same app.
    Screen(String),
    /// `inert://…` (other site) or `https://…` (unsecured web).
    Href(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Stack { dir: Dir, gap: Spacing, align: Align, children: Vec<Node> },
    Text { style: TextStyle, text: TextSrc },
    Button { label: String, action: String, args: BTreeMap<String, ArgSrc>, variant: Variant },
    Input { action: String, field: String, label: String, kind: InputKind, placeholder: Option<String>, init: Option<ArgSrc> },
    Select { action: String, field: String, label: String, init: Option<ArgSrc> },
    Toggle { action: String, field: String, label: String, init: Option<ArgSrc> },
    List { bind: String, item: Box<Node> },
    Divider,
    Image { src: ImageSrc, alt: String, width: u64, height: u64 },
    Link { label: String, target: LinkTarget, params: BTreeMap<String, ArgSrc> },
}

impl Node {
    pub fn kind(&self) -> &'static str {
        match self {
            Node::Stack { .. } => "stack",
            Node::Text { .. } => "text",
            Node::Button { .. } => "button",
            Node::Input { .. } => "input",
            Node::Select { .. } => "select",
            Node::Toggle { .. } => "toggle",
            Node::List { .. } => "list",
            Node::Divider => "divider",
            Node::Image { .. } => "image",
            Node::Link { .. } => "link",
        }
    }

    /// Depth-first visit with each node's depth (root = 1) and list-scope flag.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node, usize, Option<&'a str>)) {
        fn go<'a>(n: &'a Node, depth: usize, scope: Option<&'a str>, f: &mut impl FnMut(&'a Node, usize, Option<&'a str>)) {
            f(n, depth, scope);
            match n {
                Node::Stack { children, .. } => children.iter().for_each(|c| go(c, depth + 1, scope, f)),
                Node::List { bind, item } => go(item, depth + 1, Some(bind), f),
                _ => {}
            }
        }
        go(self, 1, None, f)
    }
}

// ---------------------------------------------------------------- encoding

fn bad(path: &str, msg: impl Into<String>) -> DecodeError {
    DecodeError::Invalid { path: path.to_owned(), msg: msg.into() }
}

fn enc_text_src(t: &TextSrc) -> Value {
    match t {
        TextSrc::Lit(s) => Value::text(s.clone()),
        TextSrc::Bind(p) => map([("bind", Value::text(p.clone()))]),
    }
}

fn dec_text_src(v: &Value, path: &str) -> Result<TextSrc, DecodeError> {
    if let Some(s) = v.as_text() {
        return Ok(TextSrc::Lit(s.to_owned()));
    }
    let mut o = Obj::new(v, path)?;
    let b = o.text("bind")?;
    o.finish()?;
    Ok(TextSrc::Bind(b))
}

fn enc_arg(a: &ArgSrc) -> Value {
    match a {
        ArgSrc::Lit(v) => map([("lit", v.clone())]),
        ArgSrc::Bind(p) => map([("bind", Value::text(p.clone()))]),
    }
}

fn dec_arg(v: &Value, path: &str) -> Result<ArgSrc, DecodeError> {
    let mut o = Obj::new(v, path)?;
    let r = if let Some(l) = o.opt("lit") {
        ArgSrc::Lit(l.clone())
    } else {
        ArgSrc::Bind(o.text("bind")?)
    };
    o.finish()?;
    Ok(r)
}

fn enc_args(a: &BTreeMap<String, ArgSrc>) -> Value {
    map(a.iter().map(|(k, v)| (k.clone(), enc_arg(v))))
}

fn dec_args(o: &mut Obj, field: &str) -> Result<BTreeMap<String, ArgSrc>, DecodeError> {
    let path = o.sub(field);
    let mut out = BTreeMap::new();
    if let Some(v) = o.opt(field) {
        let m = v.as_map().ok_or_else(|| DecodeError::Type { path: path.clone(), expected: "map" })?;
        for (k, v) in text_map(m, &path)? {
            out.insert(k.to_owned(), dec_arg(v, &format!("{path}.{k}"))?);
        }
    }
    Ok(out)
}

fn dec_init(o: &mut Obj) -> Result<Option<ArgSrc>, DecodeError> {
    let p = o.sub("init");
    match o.opt("init") {
        None => Ok(None),
        Some(v) => Ok(Some(dec_arg(v, &p)?)),
    }
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::V => "v",
        Dir::H => "h",
    }
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
        Align::Stretch => "stretch",
    }
}

impl Node {
    pub fn to_value(&self) -> Value {
        let mut e: Vec<(&str, Value)> = vec![("type", Value::text(self.kind()))];
        match self {
            Node::Stack { dir, gap, align, children } => {
                e.push(("dir", Value::text(dir_name(*dir))));
                e.push(("gap", Value::text(gap.name())));
                e.push(("align", Value::text(align_name(*align))));
                e.push(("children", Value::Array(children.iter().map(Node::to_value).collect())));
            }
            Node::Text { style, text } => {
                e.push(("style", Value::text(style.name())));
                e.push(("text", enc_text_src(text)));
            }
            Node::Button { label, action, args, variant } => {
                e.push(("label", Value::text(label.clone())));
                e.push(("action", Value::text(action.clone())));
                if !args.is_empty() {
                    e.push(("args", enc_args(args)));
                }
                e.push(("variant", Value::text(match variant {
                    Variant::Primary => "primary",
                    Variant::Ghost => "ghost",
                })));
            }
            Node::Input { action, field, label, kind, placeholder, init } => {
                if let Some(i) = init {
                    e.push(("init", enc_arg(i)));
                }
                e.push(("action", Value::text(action.clone())));
                e.push(("field", Value::text(field.clone())));
                e.push(("label", Value::text(label.clone())));
                e.push(("kind", Value::text(match kind {
                    InputKind::Text => "text",
                    InputKind::Number => "number",
                    InputKind::Multiline => "multiline",
                })));
                if let Some(p) = placeholder {
                    e.push(("placeholder", Value::text(p.clone())));
                }
            }
            Node::Select { action, field, label, init } | Node::Toggle { action, field, label, init } => {
                if let Some(i) = init {
                    e.push(("init", enc_arg(i)));
                }
                e.push(("action", Value::text(action.clone())));
                e.push(("field", Value::text(field.clone())));
                e.push(("label", Value::text(label.clone())));
            }
            Node::List { bind, item } => {
                e.push(("bind", Value::text(bind.clone())));
                e.push(("item", item.to_value()));
            }
            Node::Divider => {}
            Node::Image { src, alt, width, height } => {
                e.push(("src", match src {
                    ImageSrc::Asset(h) => map([("asset", Value::Bytes(h.to_vec()))]),
                    ImageSrc::Bind(p) => map([("bind", Value::text(p.clone()))]),
                }));
                e.push(("alt", Value::text(alt.clone())));
                e.push(("width", Value::Uint(*width)));
                e.push(("height", Value::Uint(*height)));
            }
            Node::Link { label, target, params } => {
                e.push(("label", Value::text(label.clone())));
                match target {
                    LinkTarget::Screen(s) => e.push(("to", Value::text(s.clone()))),
                    LinkTarget::Href(h) => e.push(("href", Value::text(h.clone()))),
                }
                if !params.is_empty() {
                    e.push(("params", enc_args(params)));
                }
            }
        }
        map(e)
    }

    pub fn from_value(v: &Value, path: &str) -> Result<Node, DecodeError> {
        let mut o = Obj::new(v, path)?;
        let ty = o.text("type")?;
        let node = match ty.as_str() {
            "stack" => {
                let dir = match o.text("dir")?.as_str() {
                    "v" => Dir::V,
                    "h" => Dir::H,
                    other => return Err(bad(&o.sub("dir"), format!("unknown dir {other:?}"))),
                };
                let gap_s = o.text("gap")?;
                let gap = Spacing::parse(&gap_s).ok_or_else(|| bad(&o.sub("gap"), "unknown spacing token"))?;
                let align = match o.text("align")?.as_str() {
                    "start" => Align::Start,
                    "center" => Align::Center,
                    "end" => Align::End,
                    "stretch" => Align::Stretch,
                    other => return Err(bad(&o.sub("align"), format!("unknown align {other:?}"))),
                };
                let cpath = o.sub("children");
                let children = o
                    .array("children")?
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Node::from_value(c, &format!("{cpath}[{i}]")))
                    .collect::<Result<_, _>>()?;
                Node::Stack { dir, gap, align, children }
            }
            "text" => {
                let s = o.text("style")?;
                let style = TextStyle::parse(&s).ok_or_else(|| bad(&o.sub("style"), "unknown text style"))?;
                let p = o.sub("text");
                let text = dec_text_src(o.req("text")?, &p)?;
                Node::Text { style, text }
            }
            "button" => {
                let label = o.text("label")?;
                let action = o.text("action")?;
                let args = dec_args(&mut o, "args")?;
                let variant = match o.text("variant")?.as_str() {
                    "primary" => Variant::Primary,
                    "ghost" => Variant::Ghost,
                    other => return Err(bad(&o.sub("variant"), format!("unknown variant {other:?}"))),
                };
                Node::Button { label, action, args, variant }
            }
            "input" => {
                let action = o.text("action")?;
                let field = o.text("field")?;
                let label = o.text("label")?;
                let kind = match o.text("kind")?.as_str() {
                    "text" => InputKind::Text,
                    "number" => InputKind::Number,
                    "multiline" => InputKind::Multiline,
                    other => return Err(bad(&o.sub("kind"), format!("unknown input kind {other:?}"))),
                };
                let placeholder = o.opt_text("placeholder")?;
                let init = dec_init(&mut o)?;
                Node::Input { action, field, label, kind, placeholder, init }
            }
            "select" | "toggle" => {
                let action = o.text("action")?;
                let field = o.text("field")?;
                let label = o.text("label")?;
                let init = dec_init(&mut o)?;
                if ty == "select" { Node::Select { action, field, label, init } } else { Node::Toggle { action, field, label, init } }
            }
            "list" => {
                let bind = o.text("bind")?;
                let p = o.sub("item");
                let item = Box::new(Node::from_value(o.req("item")?, &p)?);
                Node::List { bind, item }
            }
            "divider" => Node::Divider,
            "image" => {
                let sp = o.sub("src");
                let src_v = o.req("src")?;
                let mut so = Obj::new(src_v, &sp)?;
                let src = if let Some(a) = so.opt("asset") {
                    let b = a.as_bytes().ok_or_else(|| bad(&sp, "asset must be bytes"))?;
                    ImageSrc::Asset(b.try_into().map_err(|_| bad(&sp, "asset hash must be 32 bytes"))?)
                } else {
                    ImageSrc::Bind(so.text("bind")?)
                };
                so.finish()?;
                let alt = o.text("alt")?;
                let width = o.uint("width")?;
                let height = o.uint("height")?;
                Node::Image { src, alt, width, height }
            }
            "link" => {
                let label = o.text("label")?;
                let to = o.opt_text("to")?;
                let href = o.opt_text("href")?;
                let target = match (to, href) {
                    (Some(t), None) => LinkTarget::Screen(t),
                    (None, Some(h)) => LinkTarget::Href(h),
                    _ => return Err(bad(path, "link needs exactly one of `to` or `href`")),
                };
                let params = dec_args(&mut o, "params")?;
                Node::Link { label, target, params }
            }
            other => return Err(bad(path, format!("unknown component {other:?}"))),
        };
        o.finish()?;
        Ok(node)
    }
}

fn dec_decimal(o: &mut Obj, field: &str) -> Result<Decimal, DecodeError> {
    let p = o.sub(field);
    Decimal::from_value(o.req(field)?).ok_or(DecodeError::Type { path: p, expected: "decimal [mantissa, scale]" })
}

fn dec_scale(o: &mut Obj) -> Result<u32, DecodeError> {
    let p = o.sub("scale");
    let s = o.uint("scale")?;
    if s > crate::limits::DECIMAL_MAX_SCALE as u64 {
        return Err(bad(&p, "scale too large"));
    }
    Ok(s as u32)
}

impl SlotType {
    pub fn to_value(&self) -> Value {
        match self {
            SlotType::String { max } => map([("type", Value::text("string")), ("max", Value::Uint(*max))]),
            SlotType::Int { min, max } => {
                map([("type", Value::text("int")), ("min", Value::int(*min)), ("max", Value::int(*max))])
            }
            SlotType::Decimal { scale, min, max } => map([
                ("type", Value::text("decimal")),
                ("scale", Value::Uint(*scale as u64)),
                ("min", min.to_value()),
                ("max", max.to_value()),
            ]),
            SlotType::Bool => map([("type", Value::text("bool"))]),
            SlotType::Image => map([("type", Value::text("image"))]),
            SlotType::List { max, item } => map([
                ("type", Value::text("list")),
                ("max", Value::Uint(*max)),
                ("item", map(item.iter().map(|(k, v)| (k.clone(), v.to_value())))),
            ]),
        }
    }

    pub fn from_value(v: &Value, path: &str) -> Result<Self, DecodeError> {
        let mut o = Obj::new(v, path)?;
        let t = o.text("type")?;
        let r = match t.as_str() {
            "string" => SlotType::String { max: o.uint("max")? },
            "int" => SlotType::Int { min: o.int("min")?, max: o.int("max")? },
            "decimal" => SlotType::Decimal { scale: dec_scale(&mut o)?, min: dec_decimal(&mut o, "min")?, max: dec_decimal(&mut o, "max")? },
            "bool" => SlotType::Bool,
            "image" => SlotType::Image,
            "list" => {
                let max = o.uint("max")?;
                let ip = o.sub("item");
                let mut item = BTreeMap::new();
                for (k, v) in text_map(o.map("item")?, &ip)? {
                    item.insert(k.to_owned(), SlotType::from_value(v, &format!("{ip}.{k}"))?);
                }
                SlotType::List { max, item }
            }
            other => return Err(bad(path, format!("unknown slot type {other:?}"))),
        };
        o.finish()?;
        Ok(r)
    }

    pub fn name(&self) -> &'static str {
        match self {
            SlotType::String { .. } => "string",
            SlotType::Int { .. } => "int",
            SlotType::Decimal { .. } => "decimal",
            SlotType::Bool => "bool",
            SlotType::Image => "image",
            SlotType::List { .. } => "list",
        }
    }
}

impl FieldType {
    pub fn to_value(&self) -> Value {
        match self {
            FieldType::String { max } => map([("type", Value::text("string")), ("max", Value::Uint(*max))]),
            FieldType::Int { min, max } => {
                map([("type", Value::text("int")), ("min", Value::int(*min)), ("max", Value::int(*max))])
            }
            FieldType::Decimal { scale, min, max } => map([
                ("type", Value::text("decimal")),
                ("scale", Value::Uint(*scale as u64)),
                ("min", min.to_value()),
                ("max", max.to_value()),
            ]),
            FieldType::Bool => map([("type", Value::text("bool"))]),
            FieldType::Enum { options } => map([
                ("type", Value::text("enum")),
                (
                    "options",
                    Value::Array(
                        options
                            .iter()
                            .map(|o| map([("value", Value::text(o.value.clone())), ("label", Value::text(o.label.clone()))]))
                            .collect(),
                    ),
                ),
            ]),
            FieldType::Choice { from } => map([("type", Value::text("choice")), ("from", Value::text(from.clone()))]),
        }
    }

    pub fn from_value(v: &Value, path: &str) -> Result<Self, DecodeError> {
        let mut o = Obj::new(v, path)?;
        let t = o.text("type")?;
        let r = match t.as_str() {
            "string" => FieldType::String { max: o.uint("max")? },
            "int" => FieldType::Int { min: o.int("min")?, max: o.int("max")? },
            "decimal" => FieldType::Decimal { scale: dec_scale(&mut o)?, min: dec_decimal(&mut o, "min")?, max: dec_decimal(&mut o, "max")? },
            "bool" => FieldType::Bool,
            "enum" => {
                let op = o.sub("options");
                let options = o
                    .array("options")?
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let mut oo = Obj::new(v, format!("{op}[{i}]"))?;
                        let r = EnumOption { value: oo.text("value")?, label: oo.text("label")? };
                        oo.finish()?;
                        Ok(r)
                    })
                    .collect::<Result<_, DecodeError>>()?;
                FieldType::Enum { options }
            }
            "choice" => FieldType::Choice { from: o.text("from")? },
            other => return Err(bad(path, format!("unknown field type {other:?}"))),
        };
        o.finish()?;
        Ok(r)
    }

    pub fn name(&self) -> &'static str {
        match self {
            FieldType::String { .. } => "string",
            FieldType::Int { .. } => "int",
            FieldType::Decimal { .. } => "decimal",
            FieldType::Bool => "bool",
            FieldType::Enum { .. } => "enum",
            FieldType::Choice { .. } => "choice",
        }
    }
}

fn enc_fields(f: &BTreeMap<String, FieldType>) -> Value {
    map(f.iter().map(|(k, v)| (k.clone(), v.to_value())))
}

fn dec_fields(o: &mut Obj, field: &str) -> Result<BTreeMap<String, FieldType>, DecodeError> {
    let p = o.sub(field);
    let mut out = BTreeMap::new();
    for (k, v) in text_map(o.map(field)?, &p)? {
        out.insert(k.to_owned(), FieldType::from_value(v, &format!("{p}.{k}"))?);
    }
    Ok(out)
}

impl Design {
    pub fn to_value(&self) -> Value {
        let c = &self.colors;
        map([
            (
                "colors",
                map([
                    ("bg", Value::text(c.bg.to_hex())),
                    ("fg", Value::text(c.fg.to_hex())),
                    ("muted", Value::text(c.muted.to_hex())),
                    ("accent", Value::text(c.accent.to_hex())),
                    ("on_accent", Value::text(c.on_accent.to_hex())),
                    ("line", Value::text(c.line.to_hex())),
                ]),
            ),
            ("fonts", map(TextStyle::ALL.iter().map(|s| (s.name(), Value::text(self.font(*s).name()))))),
            ("sizes", map(TextStyle::ALL.iter().map(|s| (s.name(), Value::Uint(self.size(*s)))))),
            ("spacing", map(Spacing::ALL.iter().map(|s| (s.name(), Value::Uint(self.space(*s)))))),
            ("radius", Value::Uint(self.radius)),
        ])
    }

    pub fn from_value(v: &Value, path: &str) -> Result<Self, DecodeError> {
        let mut o = Obj::new(v, path)?;
        let cp = o.sub("colors");
        let mut co = Obj::new(o.req("colors")?, &cp)?;
        let mut col = |name: &str| -> Result<Rgb, DecodeError> {
            let p = co.sub(name);
            let s = co.text(name)?;
            Rgb::parse(&s).ok_or_else(|| bad(&p, "expected #rrggbb"))
        };
        let colors = Colors {
            bg: col("bg")?,
            fg: col("fg")?,
            muted: col("muted")?,
            accent: col("accent")?,
            on_accent: col("on_accent")?,
            line: col("line")?,
        };
        co.finish()?;

        let fp = o.sub("fonts");
        let mut fo = Obj::new(o.req("fonts")?, &fp)?;
        let mut fonts = [FontFamily::GeistSans; 5];
        for s in TextStyle::ALL {
            let p = fo.sub(s.name());
            let n = fo.text(s.name())?;
            fonts[s as usize] = FontFamily::parse(&n).ok_or_else(|| bad(&p, "unknown font (only built-in Geist fonts)"))?;
        }
        fo.finish()?;

        let sp = o.sub("sizes");
        let mut so = Obj::new(o.req("sizes")?, &sp)?;
        let mut sizes = [0u64; 5];
        for s in TextStyle::ALL {
            sizes[s as usize] = so.uint(s.name())?;
        }
        so.finish()?;

        let gp = o.sub("spacing");
        let mut go = Obj::new(o.req("spacing")?, &gp)?;
        let mut spacing = [0u64; 5];
        for s in Spacing::ALL {
            spacing[s as usize] = go.uint(s.name())?;
        }
        go.finish()?;

        let radius = o.uint("radius")?;
        o.finish()?;
        Ok(Design { colors, fonts, sizes, spacing, radius })
    }
}

impl Manifest {
    pub fn to_value(&self) -> Value {
        map([
            ("app", Value::text(self.app.clone())),
            ("name", Value::text(self.name.clone())),
            ("version", Value::Uint(self.version)),
            ("entry", Value::text(self.entry.clone())),
            ("design", self.design.to_value()),
            (
                "assets",
                Value::Map(
                    self.assets
                        .iter()
                        .map(|(h, m)| {
                            (
                                Value::Bytes(h.to_vec()),
                                map([
                                    ("mime", Value::text(m.mime.clone())),
                                    ("bytes", Value::Uint(m.bytes)),
                                    ("width", Value::Uint(m.width)),
                                    ("height", Value::Uint(m.height)),
                                ]),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "screens",
                map(self.screens.iter().map(|(k, s)| {
                    (
                        k.clone(),
                        map([
                            ("title", Value::text(s.title.clone())),
                            ("params", enc_fields(&s.params)),
                            ("slots", map(s.slots.iter().map(|(k, v)| (k.clone(), v.to_value())))),
                            ("root", s.root.to_value()),
                        ]),
                    )
                })),
            ),
            ("actions", map(self.actions.iter().map(|(k, a)| (k.clone(), map([("fields", enc_fields(&a.fields))]))))),
        ])
    }

    pub fn from_value(v: &Value) -> Result<Self, DecodeError> {
        let mut o = Obj::new(v, "manifest")?;
        let app = o.text("app")?;
        let name = o.text("name")?;
        let version = o.uint("version")?;
        let entry = o.text("entry")?;
        let dp = o.sub("design");
        let design = Design::from_value(o.req("design")?, &dp)?;

        let ap = o.sub("assets");
        let mut assets = BTreeMap::new();
        for (k, v) in o.map("assets")? {
            let h: Hash = k
                .as_bytes()
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| bad(&ap, "asset keys must be 32-byte hashes"))?;
            let mut mo = Obj::new(v, &ap)?;
            let meta = AssetMeta { mime: mo.text("mime")?, bytes: mo.uint("bytes")?, width: mo.uint("width")?, height: mo.uint("height")? };
            mo.finish()?;
            assets.insert(h, meta);
        }

        let sp = o.sub("screens");
        let mut screens = BTreeMap::new();
        for (k, v) in text_map(o.map("screens")?, &sp)? {
            let p = format!("{sp}.{k}");
            let mut so = Obj::new(v, &p)?;
            let title = so.text("title")?;
            let params = dec_fields(&mut so, "params")?;
            let slp = so.sub("slots");
            let mut slots = BTreeMap::new();
            for (sk, sv) in text_map(so.map("slots")?, &slp)? {
                slots.insert(sk.to_owned(), SlotType::from_value(sv, &format!("{slp}.{sk}"))?);
            }
            let rp = so.sub("root");
            let root = Node::from_value(so.req("root")?, &rp)?;
            so.finish()?;
            screens.insert(k.to_owned(), ScreenDef { title, params, slots, root });
        }

        let acp = o.sub("actions");
        let mut actions = BTreeMap::new();
        for (k, v) in text_map(o.map("actions")?, &acp)? {
            let mut ao = Obj::new(v, format!("{acp}.{k}"))?;
            let fields = dec_fields(&mut ao, "fields")?;
            ao.finish()?;
            actions.insert(k.to_owned(), ActionDef { fields });
        }
        o.finish()?;
        Ok(Manifest { app, name, version, entry, design, assets, screens, actions })
    }

    /// Deterministic CBOR bytes — what gets signed and sent in `welcome`.
    pub fn to_bytes(&self) -> Vec<u8> {
        crate::cbor::encode(&self.to_value())
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, DecodeError> {
        Self::from_value(&crate::cbor::decode(b)?)
    }
}
