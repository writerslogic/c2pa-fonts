// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! `wasm-bindgen` facade for the browser. Compiled only for `wasm32`.
//!
//! Exposes the zero-dependency core (locate, embed, hard-binding geometry) so a
//! JS layer can drive the reserve/hash/sign/fill flow using the C2PA JS SDK for
//! signing. Bytes cross as `Uint8Array`; nothing here needs a JSON dependency.

use wasm_bindgen::prelude::*;

use crate::{
    data_hash_exclusions, embed_manifest, fill_manifest, read_manifest, read_manifest_uri,
    remove_manifest, reserve_manifest, verify, ManifestSource,
};

fn js_err(e: crate::Error) -> JsError {
    JsError::new(&e.to_string())
}

/// Embed a signed Manifest Store into a font's `C2PA` table.
#[wasm_bindgen(js_name = embedManifest)]
pub fn embed_manifest_wasm(font: &[u8], manifest_store: Vec<u8>) -> Result<Vec<u8>, JsError> {
    embed_manifest(font, ManifestSource::embedded(manifest_store)).map_err(js_err)
}

/// Embed a remote manifest URI into a font's `C2PA` table.
#[wasm_bindgen(js_name = embedRemote)]
pub fn embed_remote_wasm(font: &[u8], uri: &str) -> Result<Vec<u8>, JsError> {
    embed_manifest(font, ManifestSource::remote(uri)).map_err(js_err)
}

/// Read the embedded Manifest Store from a font.
#[wasm_bindgen(js_name = readManifest)]
pub fn read_manifest_wasm(font: &[u8]) -> Result<Vec<u8>, JsError> {
    read_manifest(font).map_err(js_err)
}

/// Read the active manifest URI from a font, if present.
#[wasm_bindgen(js_name = readManifestUri)]
pub fn read_manifest_uri_wasm(font: &[u8]) -> Result<Option<String>, JsError> {
    read_manifest_uri(font).map_err(js_err)
}

/// Remove the `C2PA` table from a font.
#[wasm_bindgen(js_name = removeManifest)]
pub fn remove_manifest_wasm(font: &[u8]) -> Result<Vec<u8>, JsError> {
    remove_manifest(font).map_err(js_err)
}

/// Whether the font has a well-formed `C2PA` table with intact checksums.
#[wasm_bindgen(js_name = isCompliant)]
pub fn is_compliant_wasm(font: &[u8]) -> Result<bool, JsError> {
    Ok(verify(font).map_err(js_err)?.is_compliant())
}

/// Reserve `reserve_size` bytes for a manifest, returning the font with a
/// zero-filled placeholder. Pass `uri` as `undefined`/`null` to omit it.
#[wasm_bindgen(js_name = reserveManifest)]
pub fn reserve_manifest_wasm(
    font: &[u8],
    reserve_size: usize,
    uri: Option<String>,
) -> Result<Vec<u8>, JsError> {
    Ok(reserve_manifest(font, reserve_size, uri)
        .map_err(js_err)?
        .font)
}

/// The `c2pa.hash.data` exclusion ranges for a reserved font, as JSON:
/// `[{"start":N,"length":M}, ...]`.
#[wasm_bindgen(js_name = dataHashExclusions)]
pub fn data_hash_exclusions_wasm(font: &[u8]) -> Result<String, JsError> {
    let ex = data_hash_exclusions(font).map_err(js_err)?;
    let items: Vec<String> = ex
        .iter()
        .map(|e| format!("{{\"start\":{},\"length\":{}}}", e.start, e.length))
        .collect();
    Ok(format!("[{}]", items.join(",")))
}

/// Fill a reserved font's Manifest Store with signed bytes (<= reserved size).
#[wasm_bindgen(js_name = fillManifest)]
pub fn fill_manifest_wasm(
    reserved_font: &[u8],
    signed_manifest: &[u8],
) -> Result<Vec<u8>, JsError> {
    fill_manifest(reserved_font, signed_manifest).map_err(js_err)
}
