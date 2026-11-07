#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

// A fixed, valid manifest (the bank demo's shape) and fuzzed runtime messages.
fn manifest() -> &'static inert_proto::Manifest {
    static M: OnceLock<inert_proto::Manifest> = OnceLock::new();
    M.get_or_init(|| {
        let v: Vec<u8> = include_str!("../../spec/vectors/manifest.json")
            .split("\"cbor\": \"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .map(|h| (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect())
            .unwrap();
        inert_proto::Manifest::from_bytes(&v).unwrap()
    })
}

fuzz_target!(|data: &[u8]| {
    let m = manifest();
    match inert_proto::Message::from_bytes(data) {
        Ok(inert_proto::Message::Screen(s)) => {
            let _ = inert_verify::check_screen(m, &s);
        }
        Ok(inert_proto::Message::Action { screen, action, fields, .. }) => {
            let _ = inert_verify::check_action(m, &screen, Some(&fields), &action, &fields);
            let _ = inert_verify::check_action(m, &screen, None, &action, &fields);
        }
        Ok(inert_proto::Message::Open { screen, params }) => {
            let _ = inert_verify::check_open(m, &screen, &params);
        }
        _ => {}
    }
});
