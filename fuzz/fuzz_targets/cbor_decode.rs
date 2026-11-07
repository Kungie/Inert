#![no_main]
use libfuzzer_sys::fuzz_target;

// Anything the strict decoder accepts must re-encode to exactly the same bytes.
fuzz_target!(|data: &[u8]| {
    if let Ok(v) = inert_proto::cbor::decode(data) {
        assert_eq!(inert_proto::cbor::encode(&v), data);
    }
});
