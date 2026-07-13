//! End-to-end hard-binding round trip against real fonts.
//!
//! Requires the `validation` feature. Reserves a manifest in a real `.ttf`/`.otf`,
//! signs a data-hashed manifest with a test certificate, fills it, then validates
//! the result end to end (locate + hard binding here, signature/assertions via
//! c2pa-rs). Embedded fonts are written to the temp dir so an external tool
//! (e.g. `fontTools` with `checkChecksums=2`) can verify structural validity.
#![cfg(feature = "validation")]

use std::io::Cursor;
use std::path::PathBuf;

use c2pa::assertions::DataHash;
use c2pa::{create_signer, BoxedSigner, Builder, SigningAlg};

use c2pa_fonts::{
    data_hash_exclusions, data_hash_ranges, fill_manifest, read_manifest, reserve_manifest,
    validate, verify,
};

// c2pa-rs has no font asset handler, so the manifest is built with the
// pass-through `application/c2pa` composer, which returns the raw JUMBF
// Manifest Store — exactly what is stored in the font's `C2PA` table.
const SIGN_FORMAT: &str = "application/c2pa";
const RESERVE_SIZE: usize = 30_000;

fn candidate_fonts() -> Vec<PathBuf> {
    // Real fonts across macOS and common Linux/CI locations. Every one that
    // exists on the host is exercised.
    let candidates = [
        // macOS
        "/System/Library/Fonts/Supplemental/NotoSansLycian-Regular.ttf",
        "/System/Library/Fonts/LastResort.otf",
        // Debian/Ubuntu (installed in CI)
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/opentype/freefont/FreeSerif.otf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

fn signer() -> BoxedSigner {
    let certs = include_bytes!("fixtures/es256_certs.pem");
    let key = include_bytes!("fixtures/es256_private.key");
    create_signer::from_keys(certs, key, SigningAlg::Es256, None).expect("build test signer")
}

/// Reserve, hash-then-sign, and fill: the full generator flow.
fn sign_into_font(font: &[u8]) -> Vec<u8> {
    #[allow(deprecated)]
    let mut builder = Builder::from_json(
        r#"{"claim_generator_info":[{"name":"c2pa-fonts","version":"0.2.0"}],"assertions":[]}"#,
    )
    .expect("builder");
    builder.set_format(SIGN_FORMAT);

    // Fix the reserve size, then reserve that many bytes in the font.
    let placeholder = builder
        .data_hashed_placeholder(RESERVE_SIZE, SIGN_FORMAT)
        .expect("placeholder");
    let reserved = reserve_manifest(font, placeholder.len(), None).expect("reserve");

    // Hash the reserved font over its exclusions, then sign.
    let mut data_hash = DataHash::new("c2pa.hash.data", "sha256");
    for range in data_hash_ranges(&reserved.font).expect("ranges") {
        data_hash.add_exclusion(range);
    }
    data_hash
        .gen_hash_from_stream(&mut Cursor::new(&reserved.font))
        .expect("gen hash");

    let signed = builder
        .sign_data_hashed_embeddable(signer().as_ref(), &data_hash, SIGN_FORMAT)
        .expect("sign");
    assert!(
        signed.len() <= reserved.reserve_size,
        "signed manifest {} exceeds reserved {}",
        signed.len(),
        reserved.reserve_size
    );

    fill_manifest(&reserved.font, &signed).expect("fill")
}

#[test]
fn embed_sign_and_validate_real_fonts() {
    let fonts = candidate_fonts();
    assert!(
        !fonts.is_empty(),
        "no real fonts found on host; install fonts-dejavu-core / fonts-freefont-otf"
    );

    let out_dir = std::env::temp_dir().join("c2pa_fonts_test");
    std::fs::create_dir_all(&out_dir).unwrap();

    for path in fonts {
        let original = std::fs::read(&path).unwrap();
        let embedded = sign_into_font(&original);

        // Locate + extract works and the structure is compliant.
        assert!(read_manifest(&embedded).is_ok(), "{path:?}: read manifest");
        assert!(
            verify(&embedded).unwrap().is_compliant(),
            "{path:?}: structural compliance"
        );

        // Full validation: our hard binding + delegated signature/assertions.
        let result = validate(&embedded).unwrap();
        assert!(
            result.hard_binding_valid,
            "{path:?}: hard binding invalid; success={:?} failure={:?}",
            result.success_codes, result.failure_codes
        );
        assert!(
            result
                .success_codes
                .iter()
                .any(|c| c == "assertion.dataHash.match"),
            "{path:?}: missing dataHash.match; success={:?}",
            result.success_codes
        );
        assert!(
            result
                .success_codes
                .iter()
                .any(|c| c == "claimSignature.validated"),
            "{path:?}: missing claimSignature.validated; success={:?}",
            result.success_codes
        );

        // Write the embedded font for external structural validation (fontTools).
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(' ', "_");
        let out = out_dir.join(format!("{name}.embedded"));
        std::fs::write(&out, &embedded).unwrap();

        // Tamper a non-excluded byte: the C2PA table's majorVersion, which sits
        // 20 bytes before the manifest store (the largest exclusion). This keeps
        // the font parseable but must break the hard binding.
        let store = data_hash_exclusions(&embedded)
            .unwrap()
            .into_iter()
            .max_by_key(|e| e.length)
            .unwrap();
        let mut tampered = embedded.clone();
        tampered[store.start as usize - 20] ^= 0xFF;

        let tamper = validate(&tampered).unwrap();
        assert!(
            !tamper.hard_binding_valid,
            "{path:?}: tampered font passed hard binding; success={:?} failure={:?}",
            tamper.success_codes, tamper.failure_codes
        );
        assert!(
            !tamper
                .success_codes
                .iter()
                .any(|c| c == "assertion.dataHash.match"),
            "{path:?}: tampered font still reported dataHash.match"
        );
    }
}
