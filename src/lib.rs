// Copyright 2026 WritersLogic. All rights reserved.
// Licensed under the Apache License, Version 2.0 or the MIT license,
// at your option.

//! C2PA manifest embedding, hard binding, and verification for OpenType/TrueType
//! (SFNT) fonts.
//!
//! Implements the C2PA font embedding method — storing a C2PA Manifest Store
//! and/or a remote manifest URI in a `C2PA` SFNT table — together with its
//! `c2pa.hash.data` hard binding. Embedding re-serializes the font so the table
//! directory, offsets, and checksums (including `head.checkSumAdjustment`) stay
//! valid.
//!
//! This crate owns the two format-specific validation steps: locating and
//! extracting the manifest (step 1) and the hard binding (step 5). COSE
//! signature verification, X.509 trust evaluation, and assertion/ingredient
//! validation are delegated to the official
//! [`c2pa`](https://crates.io/crates/c2pa) SDK via the optional `validation`
//! feature. Manifest construction and signing remain the SDK's job.
//!
//! The `C2PA` font table format is **preliminary** in the C2PA specification and
//! is not yet defined in the OpenType/OFF specifications.

mod binding;
mod error;
mod reader;
mod sfnt;
mod table;
#[cfg(feature = "validation")]
mod validate;
mod verify;
#[cfg(target_arch = "wasm32")]
mod wasm;
mod writer;

pub use binding::{data_hash_exclusions, Exclusion};
pub use error::Error;
pub use reader::{read_c2pa_table, read_manifest, read_manifest_uri};
pub use table::{C2paTable, C2PA_TAG, MAJOR_VERSION, MINOR_VERSION};
pub use verify::{verify, Compliance};
pub use writer::{
    embed_manifest, fill_manifest, remove_manifest, reserve_manifest, ManifestSource, ReservedFont,
};

#[cfg(feature = "validation")]
pub use binding::{compute_data_hash, data_hash_ranges, verify_data_hash};
#[cfg(feature = "validation")]
pub use validate::{validate, Validation};
