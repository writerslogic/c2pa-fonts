use crate::binding::{data_hash_exclusions, Exclusion};
use crate::error::Error;
use crate::sfnt::SfntFont;
use crate::table::{C2paTable, C2PA_TAG};

/// What to write into a font's `C2PA` table: an embedded Manifest Store, a
/// remote manifest URI, or both.
#[derive(Debug, Clone)]
pub struct ManifestSource {
    pub active_manifest_uri: Option<String>,
    pub manifest_store: Option<Vec<u8>>,
}

impl ManifestSource {
    /// Embed a Manifest Store directly in the font.
    pub fn embedded(manifest_store: Vec<u8>) -> Self {
        Self {
            active_manifest_uri: None,
            manifest_store: Some(manifest_store),
        }
    }

    /// Reference a remote (or side-car) manifest by URI.
    pub fn remote(uri: impl Into<String>) -> Self {
        Self {
            active_manifest_uri: Some(uri.into()),
            manifest_store: None,
        }
    }

    /// Embed a Manifest Store and record the active manifest URI.
    pub fn both(uri: impl Into<String>, manifest_store: Vec<u8>) -> Self {
        Self {
            active_manifest_uri: Some(uri.into()),
            manifest_store: Some(manifest_store),
        }
    }
}

/// Embed a C2PA manifest into a font by inserting (or replacing) its `C2PA`
/// table.
///
/// The font is re-serialized: the table directory is re-sorted, offsets are
/// recomputed, and all checksums (including `head.checkSumAdjustment`) are
/// updated to keep the font structurally valid.
pub fn embed_manifest(font: &[u8], source: ManifestSource) -> Result<Vec<u8>, Error> {
    if source.active_manifest_uri.is_none() && source.manifest_store.is_none() {
        return Err(Error::InvalidTable(
            "manifest source has neither a URI nor an embedded store".into(),
        ));
    }
    let mut parsed = SfntFont::parse(font)?;
    let table = C2paTable::new(source.active_manifest_uri, source.manifest_store);
    parsed.set_table(C2PA_TAG, table.encode());
    Ok(parsed.serialize())
}

/// A font with a reserved (placeholder) Manifest Store, ready for the
/// hash-then-sign step of the hard-binding flow.
#[derive(Debug, Clone)]
pub struct ReservedFont {
    /// The serialized font with a zero-filled Manifest Store of `reserve_size`.
    pub font: Vec<u8>,
    /// The `c2pa.hash.data` exclusion ranges for this layout.
    pub exclusions: Vec<Exclusion>,
    /// Absolute byte offset of the reserved Manifest Store within `font`.
    pub manifest_offset: u64,
    /// Byte length of the reserved Manifest Store.
    pub reserve_size: usize,
}

/// Reserve space for a manifest of `reserve_size` bytes in a font's `C2PA`
/// table, returning the font-with-placeholder and its hard-binding exclusions.
///
/// This is the first step of the placeholder-then-fill flow: reserve, then hash
/// the returned font over the returned exclusions, then sign a manifest of
/// exactly `reserve_size` bytes, then [`fill_manifest`].
pub fn reserve_manifest(
    font: &[u8],
    reserve_size: usize,
    active_manifest_uri: Option<String>,
) -> Result<ReservedFont, Error> {
    if reserve_size == 0 {
        return Err(Error::InvalidTable("reserve_size must be non-zero".into()));
    }
    let mut parsed = SfntFont::parse(font)?;
    let table = C2paTable::new(active_manifest_uri, Some(vec![0u8; reserve_size]));
    parsed.set_table(C2PA_TAG, table.encode());
    let serialized = parsed.serialize();

    let exclusions = data_hash_exclusions(&serialized)?;
    let manifest_offset = exclusions
        .iter()
        .find(|e| e.length as usize == reserve_size)
        .map(|e| e.start)
        .ok_or_else(|| Error::InvalidTable("reserved store exclusion not found".into()))?;

    Ok(ReservedFont {
        font: serialized,
        exclusions,
        manifest_offset,
        reserve_size,
    })
}

/// Fill a reserved font's Manifest Store with signed manifest bytes.
///
/// The signed manifest must be no larger than the reserved size; if smaller it
/// is zero-padded to the reserved size so the store region — and therefore the
/// hard-binding exclusions and offsets — are unchanged. Trailing padding after
/// the JUMBF superbox is ignored by C2PA readers. The font is re-serialized so
/// the `C2PA` table checksum and `head.checkSumAdjustment` are recomputed;
/// because those two fields and the store are the hard-binding exclusions, the
/// data hash computed over the reserved font still matches.
pub fn fill_manifest(reserved_font: &[u8], signed_manifest: &[u8]) -> Result<Vec<u8>, Error> {
    let mut parsed = SfntFont::parse(reserved_font)?;
    let existing = parsed.table(&C2PA_TAG).ok_or(Error::NotFound)?;
    let table = C2paTable::decode(&existing.data)?;
    let reserved = table
        .manifest_store
        .as_ref()
        .map(Vec::len)
        .ok_or_else(|| Error::InvalidTable("reserved font has no manifest store".into()))?;
    if signed_manifest.len() > reserved {
        return Err(Error::InvalidTable(format!(
            "signed manifest is {} bytes but only {reserved} were reserved",
            signed_manifest.len()
        )));
    }
    let mut store = signed_manifest.to_vec();
    store.resize(reserved, 0);
    let filled = C2paTable::new(table.active_manifest_uri, Some(store));
    parsed.set_table(C2PA_TAG, filled.encode());
    Ok(parsed.serialize())
}

/// Remove the `C2PA` table from a font, if present, returning the re-serialized
/// font.
pub fn remove_manifest(font: &[u8]) -> Result<Vec<u8>, Error> {
    let mut parsed = SfntFont::parse(font)?;
    parsed.remove_table(&C2PA_TAG);
    Ok(parsed.serialize())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sfnt::Table;

    pub fn sample_font() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        SfntFont {
            sfnt_version: 0x0001_0000,
            tables: vec![
                Table {
                    tag: *b"glyf",
                    data: b"glyphdata!".to_vec(),
                },
                Table {
                    tag: *b"cmap",
                    data: vec![0u8; 20],
                },
                Table {
                    tag: *b"head",
                    data: head,
                },
            ],
        }
        .serialize()
    }

    #[test]
    fn embed_adds_c2pa_table() {
        let font = sample_font();
        let out = embed_manifest(&font, ManifestSource::embedded(b"manifest".to_vec())).unwrap();
        let parsed = SfntFont::parse(&out).unwrap();
        assert!(parsed.table(&C2PA_TAG).is_some());
        assert_eq!(parsed.tables.len(), 4);
    }

    #[test]
    fn embed_keeps_font_valid() {
        let font = sample_font();
        let out = embed_manifest(&font, ManifestSource::embedded(b"manifest".to_vec())).unwrap();
        assert!(crate::sfnt::checksum_adjustment_valid(&out).unwrap());
    }

    #[test]
    fn embed_replaces_existing_table() {
        let font = sample_font();
        let first = embed_manifest(&font, ManifestSource::embedded(b"old".to_vec())).unwrap();
        let second = embed_manifest(&first, ManifestSource::embedded(b"new".to_vec())).unwrap();
        let parsed = SfntFont::parse(&second).unwrap();
        assert_eq!(
            parsed.tables.iter().filter(|t| t.tag == C2PA_TAG).count(),
            1
        );
        assert_eq!(
            C2paTable::decode(&parsed.table(&C2PA_TAG).unwrap().data)
                .unwrap()
                .manifest_store
                .unwrap(),
            b"new"
        );
    }

    #[test]
    fn embed_preserves_other_tables() {
        let font = sample_font();
        let out = embed_manifest(&font, ManifestSource::embedded(b"m".to_vec())).unwrap();
        let parsed = SfntFont::parse(&out).unwrap();
        assert_eq!(parsed.table(b"glyf").unwrap().data, b"glyphdata!");
        assert_eq!(parsed.table(b"cmap").unwrap().data.len(), 20);
    }

    #[test]
    fn remove_strips_table_and_stays_valid() {
        let font = sample_font();
        let embedded = embed_manifest(&font, ManifestSource::embedded(b"m".to_vec())).unwrap();
        let removed = remove_manifest(&embedded).unwrap();
        let parsed = SfntFont::parse(&removed).unwrap();
        assert!(parsed.table(&C2PA_TAG).is_none());
        assert!(crate::sfnt::checksum_adjustment_valid(&removed).unwrap());
    }

    #[test]
    fn reserve_then_fill_changes_only_excluded_bytes() {
        let reserved = reserve_manifest(&sample_font(), 48, None).unwrap();
        let signed = vec![0xABu8; 48];
        let filled = fill_manifest(&reserved.font, &signed).unwrap();
        assert_eq!(filled.len(), reserved.font.len());

        let mut excluded = vec![false; filled.len()];
        for e in &reserved.exclusions {
            for i in e.start..e.start + e.length {
                excluded[i as usize] = true;
            }
        }
        for i in 0..filled.len() {
            if !excluded[i] {
                assert_eq!(
                    reserved.font[i], filled[i],
                    "byte {i} changed outside exclusions"
                );
            }
        }
    }

    #[test]
    fn filled_font_is_valid_and_readable() {
        let reserved = reserve_manifest(&sample_font(), 16, Some("urn:c2pa:m".into())).unwrap();
        let filled = fill_manifest(&reserved.font, b"0123456789abcdef").unwrap();
        assert!(crate::sfnt::checksum_adjustment_valid(&filled).unwrap());
        assert_eq!(crate::read_manifest(&filled).unwrap(), b"0123456789abcdef");
        assert_eq!(
            crate::read_manifest_uri(&filled).unwrap().as_deref(),
            Some("urn:c2pa:m")
        );
    }

    #[test]
    fn fill_rejects_oversized_manifest() {
        let reserved = reserve_manifest(&sample_font(), 8, None).unwrap();
        assert!(matches!(
            fill_manifest(&reserved.font, b"nine bytes"),
            Err(Error::InvalidTable(_))
        ));
    }

    #[test]
    fn fill_pads_shorter_manifest() {
        let reserved = reserve_manifest(&sample_font(), 32, None).unwrap();
        let filled = fill_manifest(&reserved.font, b"short").unwrap();
        assert!(crate::sfnt::checksum_adjustment_valid(&filled).unwrap());
        let store = crate::read_manifest(&filled).unwrap();
        assert_eq!(store.len(), 32);
        assert_eq!(&store[..5], b"short");
        assert!(store[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn reserve_rejects_zero_size() {
        assert!(matches!(
            reserve_manifest(&sample_font(), 0, None),
            Err(Error::InvalidTable(_))
        ));
    }

    #[test]
    fn empty_source_is_rejected() {
        let font = sample_font();
        let empty = ManifestSource {
            active_manifest_uri: None,
            manifest_store: None,
        };
        assert!(matches!(
            embed_manifest(&font, empty),
            Err(Error::InvalidTable(_))
        ));
    }
}
