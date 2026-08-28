use std::io::Cursor;

use c2pa::assertions::BoxHash;
use c2pa::{Context, Reader, ValidationState};

use crate::binding::{data_hash_exclusions, verify_box_hash};
use crate::error::Error;
use crate::reader::read_manifest;

/// MIME type passed to c2pa-rs for the font stream. c2pa-rs does not need to
/// parse the font (the manifest is supplied out of band); the string only
/// selects the data-hash (not BMFF) validation path.
const FONT_MIME: &str = "font/sfnt";

/// The outcome of validating a font's C2PA manifest end to end.
#[derive(Debug, Clone)]
pub struct Validation {
    /// Whether the font's hard binding holds. Current manifests use
    /// `c2pa.hash.boxes`; legacy data-hash manifests remain readable.
    pub hard_binding_valid: bool,
    /// Overall validation state from c2pa-rs (signature, trust, assertions).
    pub state: ValidationState,
    /// Success status codes reported by c2pa-rs for the active manifest.
    pub success_codes: Vec<String>,
    /// Failure status codes reported by c2pa-rs for the active manifest.
    pub failure_codes: Vec<String>,
    /// The validated manifest store as JSON.
    pub manifest_json: String,
}

/// Validate a font's C2PA manifest end to end.
///
/// This crate locates and extracts the Manifest Store (step 1) and owns the
/// format-specific hard binding (step 5): it verifies the font table sequence
/// and hashes table contents for `c2pa.hash.boxes`, including the special
/// zeroed treatment of `head.checkSumAdjustment`. c2pa-rs performs COSE
/// signature verification, X.509 trust evaluation, and remaining assertion and
/// ingredient validation.
///
/// Trust is evaluated with c2pa-rs default settings; configure trust anchors
/// through c2pa-rs and inspect [`Validation::state`] and the code lists.
pub fn validate(font: &[u8]) -> Result<Validation, Error> {
    // Step 1: locate + extract (this crate).
    let store = read_manifest(font)?;

    // Steps 2-4, 6 and the hash comparison: delegate to c2pa-rs.
    let reader = Reader::from_context(Context::new())
        .with_manifest_data_and_stream(&store, FONT_MIME, Cursor::new(font))
        .map_err(|e| Error::Validation(e.to_string()))?;

    let mut success_codes = Vec::new();
    let mut failure_codes = Vec::new();
    if let Some(results) = reader.validation_results() {
        if let Some(active) = results.active_manifest() {
            success_codes = active
                .success()
                .iter()
                .map(|s| s.code().to_string())
                .collect();
            failure_codes = active
                .failure()
                .iter()
                .map(|s| s.code().to_string())
                .collect();
        }
    }

    let hard_binding_valid = if let Some(manifest) = reader.active_manifest() {
        match manifest.find_assertion::<BoxHash>(BoxHash::LABEL) {
            Ok(assertion) => verify_box_hash(font, &assertion, None).unwrap_or(false),
            Err(_) => legacy_data_hash_valid(font, &success_codes, &failure_codes),
        }
    } else {
        false
    };

    Ok(Validation {
        hard_binding_valid,
        state: reader.validation_state(),
        success_codes,
        failure_codes,
        manifest_json: reader.json(),
    })
}

/// The hard binding holds when this crate can derive the font's exclusion
/// geometry (so the `C2PA` table and its store region are intact) and c2pa-rs
/// confirmed the data hash matches over those exclusions.
fn legacy_data_hash_valid(font: &[u8], success: &[String], failure: &[String]) -> bool {
    if data_hash_exclusions(font).is_err() {
        return false;
    }
    let matched = success.iter().any(|c| c == "assertion.dataHash.match");
    let broken = failure.iter().any(|c| c.starts_with("assertion.dataHash"));
    matched && !broken
}
