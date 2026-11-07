use ed25519_dalek::{Signer, SigningKey};
use inert_proto::*;
use inert_verify::*;
use std::collections::BTreeMap;

fn design() -> Design {
    Design {
        colors: Colors {
            bg: Rgb(0, 0, 0),
            fg: Rgb(0xed, 0xed, 0xed),
            muted: Rgb(0x9a, 0x9a, 0x9a),
            accent: Rgb(0xb4, 0x8c, 0xff),
            on_accent: Rgb(0, 0, 0),
            line: Rgb(0x22, 0x22, 0x22),
        },
        fonts: [FontFamily::GeistSerif, FontFamily::GeistSerif, FontFamily::GeistSans, FontFamily::GeistMono, FontFamily::GeistMono],
        sizes: [56, 28, 16, 14, 11],
        spacing: [4, 8, 16, 24, 40],
        radius: 4,
    }
}

fn stack(children: Vec<Node>) -> Node {
    Node::Stack { dir: Dir::V, gap: Spacing::M, align: Align::Stretch, children }
}

fn fixture() -> Manifest {
    let mut slots = BTreeMap::new();
    slots.insert("balance".into(), SlotType::Decimal { scale: 2, min: Decimal::new(-100_000_000, 2), max: Decimal::new(100_000_000, 2) });
    let mut acct = BTreeMap::new();
    acct.insert("id".into(), SlotType::String { max: 32 });
    acct.insert("label".into(), SlotType::String { max: 64 });
    slots.insert("accounts".into(), SlotType::List { max: 10, item: acct });

    let mut fields = BTreeMap::new();
    fields.insert("from".into(), FieldType::Choice { from: "accounts".into() });
    fields.insert("amount".into(), FieldType::Decimal { scale: 2, min: Decimal::new(1, 2), max: Decimal::new(1_000_000, 2) });
    let mut actions = BTreeMap::new();
    actions.insert("transfer".into(), ActionDef { fields });

    let root = stack(vec![
        Node::Text { style: TextStyle::Display, text: TextSrc::Bind("balance".into()) },
        Node::List { bind: "accounts".into(), item: Box::new(Node::Text { style: TextStyle::Body, text: TextSrc::Bind("item.label".into()) }) },
        Node::Select { action: "transfer".into(), field: "from".into(), label: "From".into(), init: None },
        Node::Input { action: "transfer".into(), field: "amount".into(), label: "Amount".into(), kind: InputKind::Number, placeholder: None, init: None },
        Node::Button { label: "Send".into(), action: "transfer".into(), args: BTreeMap::new(), variant: Variant::Primary },
    ]);
    let mut screens = BTreeMap::new();
    screens.insert("overview".into(), ScreenDef { title: "Overview".into(), params: BTreeMap::new(), slots, root });
    Manifest { app: "bank.example".into(), name: "Bank".into(), version: 1, entry: "overview".into(), design: design(), assets: BTreeMap::new(), screens, actions }
}

fn screen_data() -> Fields {
    let mut d = Fields::new();
    d.insert("balance".into(), Decimal::new(12000, 2).to_value());
    d.insert(
        "accounts".into(),
        Value::Array(vec![cbor::map([("id", Value::text("chk")), ("label", Value::text("Checking"))])]),
    );
    d
}

#[test]
fn fixture_is_valid() {
    check_manifest(&fixture()).unwrap();
}

#[test]
fn low_contrast_is_rejected() {
    let mut m = fixture();
    m.design.colors.muted = Rgb(0x11, 0x11, 0x11);
    let e = check_manifest(&m).unwrap_err();
    assert!(e.path.contains("muted"), "{e}");
}

#[test]
fn dangling_bind_is_rejected() {
    let mut m = fixture();
    let s = m.screens.get_mut("overview").unwrap();
    if let Node::Stack { children, .. } = &mut s.root {
        children.push(Node::Text { style: TextStyle::Body, text: TextSrc::Bind("password".into()) });
    }
    assert!(check_manifest(&m).is_err());
}

#[test]
fn missing_form_field_is_rejected() {
    let mut m = fixture();
    let s = m.screens.get_mut("overview").unwrap();
    if let Node::Stack { children, .. } = &mut s.root {
        children.retain(|n| !matches!(n, Node::Input { .. }));
    }
    let e = check_manifest(&m).unwrap_err();
    assert!(e.msg.contains("amount"), "{e}");
}

#[test]
fn form_field_inside_list_is_rejected() {
    let mut m = fixture();
    let s = m.screens.get_mut("overview").unwrap();
    if let Node::Stack { children, .. } = &mut s.root {
        children[1] = Node::List {
            bind: "accounts".into(),
            item: Box::new(Node::Input { action: "transfer".into(), field: "amount".into(), label: "A".into(), kind: InputKind::Number, placeholder: None, init: None }),
        };
    }
    assert!(check_manifest(&m).is_err());
}

#[test]
fn screen_must_match_slots_exactly() {
    let m = fixture();
    let ok = ScreenMsg { screen: "overview".into(), data: screen_data() };
    check_screen(&m, &ok).unwrap();

    // Attack: extra field sneaked into the screen.
    let mut extra = ok.clone();
    extra.data.insert("password".into(), Value::text(""));
    assert_eq!(check_screen(&m, &extra).unwrap_err().code, ErrorCode::ScreenInvalid);

    // Missing slot.
    let mut missing = ok.clone();
    missing.data.remove("balance");
    assert!(check_screen(&m, &missing).is_err());

    // Wrong type and out-of-range.
    let mut wrong = ok.clone();
    wrong.data.insert("balance".into(), Value::text("lots"));
    assert!(check_screen(&m, &wrong).is_err());
    let mut big = ok.clone();
    big.data.insert("balance".into(), Decimal::new(1, 3).to_value());
    assert!(check_screen(&m, &big).is_err(), "scale 3 > 2");

    // Invisible characters in text.
    let mut hidden = ok;
    hidden.data.insert(
        "accounts".into(),
        Value::Array(vec![cbor::map([("id", Value::text("chk")), ("label", Value::text("Check\u{200B}ing"))])]),
    );
    assert!(check_screen(&m, &hidden).is_err());
}

#[test]
fn action_rules() {
    let m = fixture();
    let data = screen_data();
    let mut f = Fields::new();
    f.insert("from".into(), Value::text("chk"));
    f.insert("amount".into(), Decimal::new(500, 2).to_value());
    check_action(&m, "overview", Some(&data), "transfer", &f).unwrap();

    // Choice not offered on the screen.
    let mut bad = f.clone();
    bad.insert("from".into(), Value::text("someone-elses-account"));
    let e = check_action(&m, "overview", Some(&data), "transfer", &bad).unwrap_err();
    assert_eq!(e.field.as_deref(), Some("from"));

    // Amount above max.
    let mut big = f.clone();
    big.insert("amount".into(), Decimal::new(100_000_000, 2).to_value());
    assert_eq!(check_action(&m, "overview", Some(&data), "transfer", &big).unwrap_err().field.as_deref(), Some("amount"));

    // Extra field.
    let mut extra = f.clone();
    extra.insert("note".into(), Value::text("x"));
    assert!(check_action(&m, "overview", Some(&data), "transfer", &extra).is_err());

    // Unknown action.
    assert_eq!(check_action(&m, "overview", Some(&data), "drain", &f).unwrap_err().code, ErrorCode::ActionUnknown);
}

#[test]
fn signatures() {
    let m = fixture();
    let bytes = m.to_bytes();
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let pk = sk.verifying_key().to_bytes();
    let sig = sk.sign(&sig::signing_message(&bytes)).to_bytes();
    let kid = key_id(&pk);
    verify_welcome(&bytes, &sig, &kid, &pk, "bank.example").unwrap();

    // One flipped byte breaks the signature.
    let mut tampered = bytes.clone();
    let i = tampered.len() / 2;
    tampered[i] ^= 1;
    assert_eq!(verify_welcome(&tampered, &sig, &kid, &pk, "bank.example").unwrap_err().code, ErrorCode::BadSignature);

    // A different key is not the pinned key.
    let other = SigningKey::from_bytes(&[8u8; 32]);
    let osig = other.sign(&sig::signing_message(&bytes)).to_bytes();
    let okid = key_id(&other.verifying_key().to_bytes());
    assert_eq!(verify_welcome(&bytes, &osig, &okid, &pk, "bank.example").unwrap_err().code, ErrorCode::BadSignature);

    // Manifest for another host.
    assert_eq!(verify_welcome(&bytes, &sig, &kid, &pk, "evil.example").unwrap_err().code, ErrorCode::ManifestInvalid);
}
