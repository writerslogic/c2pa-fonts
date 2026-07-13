<p align="center">
  <h1 align="center">c2pa-fonts</h1>
  <p align="center">C2PA manifest embedding, hard binding, and verification for OpenType/TrueType (SFNT) fonts</p>
</p>

<p align="center">
  <a href="https://crates.io/crates/c2pa-fonts"><img src="https://img.shields.io/crates/v/c2pa-fonts.svg" alt="crates.io"></a>
  <a href="https://docs.rs/c2pa-fonts"><img src="https://docs.rs/c2pa-fonts/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/crates/l/c2pa-fonts.svg" alt="License"></a>
</p>

## Overview

Implements the **font embedding** method from the [C2PA Technical Specification](https://c2pa.org/specifications/) and its `c2pa.hash.data` hard binding, for fonts that conform to the [OpenType](https://learn.microsoft.com/en-us/typography/opentype/spec/) or [OFF](https://www.iso.org/standard/52136.html) (ISO/IEC 14496-22) specification.

The manifest is stored in a dedicated SFNT table with the tag `C2PA`, which may carry an embedded Manifest Store, a remote manifest URI, or both:

| Field | Type | Description |
|---|---|---|
| `majorVersion` / `minorVersion` | `uint16` | Version of the C2PA font table |
| `activeManifestUri` | `Offset32` + `uint16` | URI of the active manifest (offset + length; `0` when absent) |
| `manifestStore` | `Offset32` + `uint32` | Embedded C2PA Manifest Store (offset + length; `0` when absent) |

> **The `C2PA` font table is preliminary.** The C2PA specification states the table format "is not yet defined in the OFF nor OpenType specification; the following definition is preliminary." There is no stable, ratified conformance requirement for fonts. This crate tracks the preliminary definition and emits table version `0.1`.

## What this crate does — and does not

Validating a font's provenance is a fixed pipeline. Only two steps are font-specific; this crate owns exactly those:

| Step | Owner |
|---|---|
| 1. Locate + extract the Manifest Store | **c2pa-fonts** |
| 2. Parse the JUMBF/CBOR manifest | c2pa-rs |
| 3. Verify the COSE signature | c2pa-rs |
| 4. Evaluate the X.509 trust chain | c2pa-rs |
| 5. Hard binding: `c2pa.hash.data` exclusion geometry over the font | **c2pa-fonts** |
| 6. Validate assertions / ingredients | c2pa-rs |

This crate does **not** build manifests, sign, or implement COSE/trust — that is the [official `c2pa` SDK](https://crates.io/crates/c2pa)'s job. With the `validation` feature it delegates steps 2–4 and 6 to c2pa-rs, so an application using both can act as a C2PA **generator and verifier** for fonts. (This crate is a building block; C2PA conformance certification is a separate program for products, which this crate makes no claim to.)

Font collections (`.ttc`/`ttcf`) and WOFF/WOFF2 are rejected explicitly; decompress WOFF to SFNT first.

## Quick Start

```toml
[dependencies]
c2pa-fonts = "0.2"
# Hard-binding hashers and the c2pa-rs validation bridge:
c2pa-fonts = { version = "0.2", features = ["validation"] }
```

### Embed an already-signed manifest

```rust
use c2pa_fonts::{embed_manifest, ManifestSource};

let font: &[u8] = /* .ttf / .otf bytes */;
let signed = embed_manifest(font, ManifestSource::embedded(manifest_store)).unwrap();
// or ManifestSource::remote("https://example.com/m.c2pa") / ManifestSource::both(uri, store)
```

### Generate with a hard binding (placeholder-then-fill)

The manifest signs over the font, so the font must be laid out before signing. Reserve the store, hash over the returned exclusions, sign a manifest that fits, then fill:

```rust
use c2pa_fonts::{reserve_manifest, fill_manifest, data_hash_ranges};

// 1. Reserve space; get the font-with-placeholder and its exclusions.
let reserved = reserve_manifest(font, 30_000, None).unwrap();

// 2. Hash reserved.font over reserved.exclusions and sign a data-hashed manifest
//    with the c2pa SDK (see tests/roundtrip.rs for the full flow).
//    data_hash_ranges(&reserved.font) yields the ranges as c2pa `HashRange`s.

// 3. Fill the reserved region with the signed manifest (<= reserved size).
let final_font = fill_manifest(&reserved.font, &signed_manifest).unwrap();
```

The exclusions cover the manifest store **and** the two checksum fields that depend on it (the `C2PA` table's directory checksum and `head.checkSumAdjustment`), so filling never invalidates the hash.

### Read and verify

```rust
use c2pa_fonts::{read_manifest, read_manifest_uri, verify, validate};

let manifest = read_manifest(&final_font).unwrap();       // embedded store bytes
let uri = read_manifest_uri(&final_font).unwrap();        // Option<String>

let report = verify(&final_font).unwrap();                // structural, zero-dep
assert!(report.is_compliant());

// End-to-end (feature = "validation"): hard binding here, signature/trust via c2pa-rs.
let result = validate(&final_font).unwrap();
assert!(result.hard_binding_valid);
// result.state, result.success_codes, result.failure_codes, result.manifest_json
```

## Design

- The Manifest Store and/or active manifest URI live in a single `C2PA` SFNT table
- Embedding normalizes the font: the table directory is re-sorted by tag, offsets re-aligned to 4-byte boundaries, and per-table checksums plus `head.checkSumAdjustment` recomputed
- `verify` checks `C2PA` table well-formedness and SFNT checksum integrity; more than one `C2PA` table is rejected
- `validate` (feature) extracts the store and delegates COSE/trust/assertion validation to c2pa-rs; trust uses c2pa-rs default settings — inspect the returned state and status codes
- Single fonts only; collections and WOFF are rejected

## Related Crates

| Crate | Description |
|---|---|
| [c2pa-warc](https://crates.io/crates/c2pa-warc) | WARC web archive embedding (ISO 28500) |
| [c2pa-structured-text](https://crates.io/crates/c2pa-structured-text) | Structured text embedding via ASCII armour delimiters |
| [c2pa-text-binding](https://crates.io/crates/c2pa-text-binding) | Soft binding and content fingerprinting for text assets |
| [c2pa-rs](https://crates.io/crates/c2pa) | Official C2PA SDK |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

Built by [WritersLogic](https://writerslogic.com)
