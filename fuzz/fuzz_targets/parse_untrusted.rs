#![no_main]

use c2pa_fonts::{read_c2pa_table, read_manifest, read_manifest_uri};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = read_c2pa_table(data);
    let _ = read_manifest(data);
    let _ = read_manifest_uri(data);
});
