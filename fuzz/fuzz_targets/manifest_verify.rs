#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = inert_proto::Manifest::from_bytes(data) {
        let _ = inert_verify::check_manifest(&m);
    }
});
