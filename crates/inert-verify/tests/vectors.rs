//! Language-independent conformance vectors (spec §15).
//!
//! `spec/vectors/*.json` hold hex-encoded CBOR inputs with the expected
//! outcome. Any Inert implementation can run the same files. Regenerate the
//! files with `INERT_WRITE_VECTORS=1 cargo test -p inert-verify --test vectors`.

use inert_proto::*;
use serde_json::{Value as J, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/vectors")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn manifest() -> Manifest {
    let mut slots = BTreeMap::new();
    slots.insert("balance".into(), SlotType::Decimal { scale: 2, min: Decimal::new(-1_000_000, 2), max: Decimal::new(1_000_000, 2) });
    let mut item = BTreeMap::new();
    item.insert("id".into(), SlotType::String { max: 16 });
    item.insert("label".into(), SlotType::String { max: 40 });
    slots.insert("accounts".into(), SlotType::List { max: 5, item });
    let mut fields = BTreeMap::new();
    fields.insert("from".into(), FieldType::Choice { from: "accounts".into() });
    fields.insert("amount".into(), FieldType::Decimal { scale: 2, min: Decimal::new(1, 2), max: Decimal::new(100_000, 2) });
    let mut actions = BTreeMap::new();
    actions.insert("pay".into(), ActionDef { fields });
    let root = Node::Stack {
        dir: Dir::V,
        gap: Spacing::M,
        align: Align::Stretch,
        children: vec![
            Node::Text { style: TextStyle::Display, text: TextSrc::Bind("balance".into()) },
            Node::Select { action: "pay".into(), field: "from".into(), label: "From".into(), init: None },
            Node::Input { action: "pay".into(), field: "amount".into(), label: "Amount".into(), kind: InputKind::Number, placeholder: None, init: None },
            Node::Button { label: "Pay".into(), action: "pay".into(), args: BTreeMap::new(), variant: Variant::Primary },
        ],
    };
    let mut screens = BTreeMap::new();
    screens.insert("home".into(), ScreenDef { title: "Home".into(), params: BTreeMap::new(), slots, root });
    Manifest {
        app: "vectors.example".into(),
        name: "Vectors".into(),
        version: 1,
        entry: "home".into(),
        design: Design {
            colors: Colors { bg: Rgb(0, 0, 0), fg: Rgb(237, 237, 237), muted: Rgb(154, 154, 154), accent: Rgb(180, 140, 255), on_accent: Rgb(0, 0, 0), line: Rgb(34, 34, 34) },
            fonts: [FontFamily::GeistSerif, FontFamily::GeistSerif, FontFamily::GeistSans, FontFamily::GeistMono, FontFamily::GeistMono],
            sizes: [56, 28, 16, 14, 11],
            spacing: [4, 8, 16, 24, 40],
            radius: 2,
        },
        assets: BTreeMap::new(),
        screens,
        actions,
    }
}

fn data(extra: Option<(&str, Value)>, balance: Value) -> Fields {
    let mut d = Fields::new();
    d.insert("balance".into(), balance);
    d.insert("accounts".into(), Value::Array(vec![cbor::map([("id", Value::text("a1")), ("label", Value::text("Main"))])]));
    if let Some((k, v)) = extra {
        d.insert(k.into(), v);
    }
    d
}

/// (name, hex, expected: "ok" | error name)
fn cbor_cases() -> Vec<(String, String, String)> {
    let ok = |n: &str, b: &[u8]| (n.to_owned(), hex(b), "ok".to_owned());
    let bad = |n: &str, b: &[u8], e: &str| (n.to_owned(), hex(b), e.to_owned());
    vec![
        ok("uint_0", &[0x00]),
        ok("uint_24", &[0x18, 0x18]),
        ok("nint_minus_100", &[0x38, 0x63]),
        ok("text_abc", &[0x63, b'a', b'b', b'c']),
        ok("map_sorted_by_encoding", &[0xa2, 0x61, b'z', 0x00, 0x62, b'a', b'a', 0x00]),
        bad("uint_not_shortest", &[0x18, 0x05], "malformed_cbor"),
        bad("indefinite_array", &[0x9f, 0xff], "malformed_cbor"),
        bad("tag", &[0xc1, 0x00], "malformed_cbor"),
        bad("half_float", &[0xf9, 0x3c, 0x00], "malformed_cbor"),
        bad("undefined", &[0xf7], "malformed_cbor"),
        bad("map_unsorted", &[0xa2, 0x61, b'b', 0x01, 0x61, b'a', 0x02], "malformed_cbor"),
        bad("map_duplicate_key", &[0xa2, 0x61, b'a', 0x01, 0x61, b'a', 0x02], "malformed_cbor"),
        bad("trailing_bytes", &[0x00, 0x00], "malformed_cbor"),
        bad("invalid_utf8", &[0x62, 0xff, 0xfe], "malformed_cbor"),
        bad("length_past_end", &[0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], "malformed_cbor"),
    ]
}

fn screen_cases() -> Vec<(String, String, String)> {
    let enc = |d: Fields| hex(&Message::Screen(ScreenMsg { screen: "home".into(), data: d }).to_bytes());
    vec![
        ("valid".into(), enc(data(None, Decimal::new(1200, 2).to_value())), "ok".into()),
        ("injected_password_slot".into(), enc(data(Some(("password", Value::text(""))), Decimal::new(1200, 2).to_value())), "screen_invalid".into()),
        ("wrong_slot_type".into(), enc(data(None, Value::text("lots"))), "screen_invalid".into()),
        ("decimal_scale_too_large".into(), enc(data(None, Decimal::new(1, 3).to_value())), "screen_invalid".into()),
        ("decimal_out_of_range".into(), enc(data(None, Decimal::new(100_000_000, 2).to_value())), "screen_invalid".into()),
    ]
}

fn action_cases() -> Vec<(String, String, String)> {
    let enc = |from: &str, amount: Value, extra: bool| {
        let mut f = Fields::new();
        f.insert("from".into(), Value::text(from));
        f.insert("amount".into(), amount);
        if extra {
            f.insert("memo".into(), Value::text("x"));
        }
        hex(&Message::Action { id: [7; 16], screen: "home".into(), action: "pay".into(), fields: f }.to_bytes())
    };
    vec![
        ("valid".into(), enc("a1", Decimal::new(500, 2).to_value(), false), "ok".into()),
        ("choice_not_offered".into(), enc("someone-else", Decimal::new(500, 2).to_value(), false), "field_invalid:from".into()),
        ("amount_over_max".into(), enc("a1", Decimal::new(200_000, 2).to_value(), false), "field_invalid:amount".into()),
        ("unexpected_field".into(), enc("a1", Decimal::new(500, 2).to_value(), true), "field_invalid:memo".into()),
    ]
}

fn run_cbor(hexs: &str) -> String {
    match cbor::decode(&unhex(hexs)) {
        Ok(_) => "ok".into(),
        Err(_) => ErrorCode::MalformedCbor.name().into(),
    }
}

fn run_screen(m: &Manifest, hexs: &str) -> String {
    match Message::from_bytes(&unhex(hexs)) {
        Ok(Message::Screen(s)) => match inert_verify::check_screen(m, &s) {
            Ok(()) => "ok".into(),
            Err(e) => e.code.name().into(),
        },
        Ok(_) => "unknown_message".into(),
        Err(e) => e.code().name().into(),
    }
}

fn run_action(m: &Manifest, hexs: &str) -> String {
    let d = data(None, Decimal::new(1200, 2).to_value());
    match Message::from_bytes(&unhex(hexs)) {
        Ok(Message::Action { screen, action, fields, .. }) => match inert_verify::check_action(m, &screen, Some(&d), &action, &fields) {
            Ok(()) => "ok".into(),
            Err(e) => format!("{}{}", e.code.name(), e.field.map(|f| format!(":{f}")).unwrap_or_default()),
        },
        Ok(_) => "unknown_message".into(),
        Err(e) => e.code().name().into(),
    }
}

#[test]
fn conformance_vectors() {
    let m = manifest();
    inert_verify::check_manifest(&m).expect("vector manifest is valid");
    let suites = [("cbor", cbor_cases()), ("screen", screen_cases()), ("action", action_cases())];
    if std::env::var("INERT_WRITE_VECTORS").is_ok() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(dir().join("manifest.json"), serde_json::to_string_pretty(&json!({"comment": "Manifest used by screen and action vectors (deterministic CBOR, hex).", "cbor": hex(&m.to_bytes())})).unwrap()).unwrap();
        for (name, cases) in &suites {
            let v: Vec<J> = cases.iter().map(|(n, h, e)| json!({"name": n, "cbor": h, "expect": e})).collect();
            std::fs::write(dir().join(format!("{name}.json")), serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }
    }
    // Run the files on disk (what other implementations would consume).
    let mf: J = serde_json::from_str(&std::fs::read_to_string(dir().join("manifest.json")).expect("vectors exist; run with INERT_WRITE_VECTORS=1")).unwrap();
    let m2 = Manifest::from_bytes(&unhex(mf["cbor"].as_str().unwrap())).unwrap();
    assert_eq!(m2, m, "manifest vector matches");
    let mut n = 0;
    for (name, _) in &suites {
        let cases: Vec<J> = serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{name}.json"))).unwrap()).unwrap();
        for c in cases {
            let h = c["cbor"].as_str().unwrap();
            let want = c["expect"].as_str().unwrap();
            let got = match *name {
                "cbor" => run_cbor(h),
                "screen" => run_screen(&m2, h),
                _ => run_action(&m2, h),
            };
            assert_eq!(got, want, "{name}/{}", c["name"]);
            n += 1;
        }
    }
    assert!(n >= 24);
}
