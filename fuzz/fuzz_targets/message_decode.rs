#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = inert_proto::Message::from_bytes(data) {
        // Typed round trip must be stable.
        let again = inert_proto::Message::from_bytes(&m.to_bytes()).expect("re-decodes");
        assert_eq!(again, m);
    }
});
