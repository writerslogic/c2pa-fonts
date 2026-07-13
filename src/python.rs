// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! PyO3 Python extension module, built with maturin (`features = ["python"]`).
//!
//! Exposes the zero-dependency core (locate, embed, hard-binding geometry) so a
//! Python caller can drive the reserve/hash/sign/fill flow using the C2PA
//! Python SDK for signing. Byte parameters accept `bytes`; byte results are
//! returned as `bytes`.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::{
    data_hash_exclusions, embed_manifest, fill_manifest, read_manifest, read_manifest_uri,
    remove_manifest, reserve_manifest, verify, ManifestSource,
};

fn map_err(e: crate::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Embed a signed Manifest Store into a font's `C2PA` table.
#[pyfunction]
fn embed_manifest_store<'py>(
    py: Python<'py>,
    font: &[u8],
    manifest_store: Vec<u8>,
) -> PyResult<Bound<'py, PyBytes>> {
    let out = embed_manifest(font, ManifestSource::embedded(manifest_store)).map_err(map_err)?;
    Ok(PyBytes::new(py, &out))
}

/// Embed a remote manifest URI into a font's `C2PA` table.
#[pyfunction]
fn embed_remote<'py>(py: Python<'py>, font: &[u8], uri: &str) -> PyResult<Bound<'py, PyBytes>> {
    let out = embed_manifest(font, ManifestSource::remote(uri)).map_err(map_err)?;
    Ok(PyBytes::new(py, &out))
}

/// Read the embedded Manifest Store from a font.
#[pyfunction]
fn read_manifest_store<'py>(py: Python<'py>, font: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
    let out = read_manifest(font).map_err(map_err)?;
    Ok(PyBytes::new(py, &out))
}

/// Read the active manifest URI from a font, if present.
#[pyfunction]
fn read_manifest_uri_str(font: &[u8]) -> PyResult<Option<String>> {
    read_manifest_uri(font).map_err(map_err)
}

/// Remove the `C2PA` table from a font.
#[pyfunction]
fn remove_manifest_table<'py>(py: Python<'py>, font: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
    let out = remove_manifest(font).map_err(map_err)?;
    Ok(PyBytes::new(py, &out))
}

/// Whether the font has a well-formed `C2PA` table with intact checksums.
#[pyfunction]
fn is_compliant(font: &[u8]) -> PyResult<bool> {
    Ok(verify(font).map_err(map_err)?.is_compliant())
}

/// Reserve `reserve_size` bytes for a manifest, returning the font with a
/// zero-filled placeholder.
#[pyfunction]
#[pyo3(signature = (font, reserve_size, uri=None))]
fn reserve_manifest_placeholder<'py>(
    py: Python<'py>,
    font: &[u8],
    reserve_size: usize,
    uri: Option<String>,
) -> PyResult<Bound<'py, PyBytes>> {
    let reserved = reserve_manifest(font, reserve_size, uri).map_err(map_err)?;
    Ok(PyBytes::new(py, &reserved.font))
}

/// The `c2pa.hash.data` exclusion ranges for a reserved font, as
/// `[(start, length), ...]`.
#[pyfunction]
fn data_hash_ranges(font: &[u8]) -> PyResult<Vec<(u64, u64)>> {
    Ok(data_hash_exclusions(font)
        .map_err(map_err)?
        .iter()
        .map(|e| (e.start, e.length))
        .collect())
}

/// Fill a reserved font's Manifest Store with signed bytes (<= reserved size).
#[pyfunction]
fn fill_manifest_store<'py>(
    py: Python<'py>,
    reserved_font: &[u8],
    signed_manifest: &[u8],
) -> PyResult<Bound<'py, PyBytes>> {
    let out = fill_manifest(reserved_font, signed_manifest).map_err(map_err)?;
    Ok(PyBytes::new(py, &out))
}

#[pymodule]
fn c2pa_fonts(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(embed_manifest_store, m)?)?;
    m.add_function(wrap_pyfunction!(embed_remote, m)?)?;
    m.add_function(wrap_pyfunction!(read_manifest_store, m)?)?;
    m.add_function(wrap_pyfunction!(read_manifest_uri_str, m)?)?;
    m.add_function(wrap_pyfunction!(remove_manifest_table, m)?)?;
    m.add_function(wrap_pyfunction!(is_compliant, m)?)?;
    m.add_function(wrap_pyfunction!(reserve_manifest_placeholder, m)?)?;
    m.add_function(wrap_pyfunction!(data_hash_ranges, m)?)?;
    m.add_function(wrap_pyfunction!(fill_manifest_store, m)?)?;
    Ok(())
}
