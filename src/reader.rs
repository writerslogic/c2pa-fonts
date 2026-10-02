use crate::error::Error;
use crate::sfnt::SfntFont;
use crate::table::{C2paTable, C2PA_TAG};

/// Read and decode the `C2PA` table from a font.
///
/// Returns [`Error::InvalidTable`] if the font contains more than one `C2PA`
/// table, matching the multiplicity check [`crate::verify::verify`] applies
/// so a crafted font can't smuggle a second, unverified table past callers
/// that only locate/extract (step 1) without also validating (step 6).
pub fn read_c2pa_table(font: &[u8]) -> Result<C2paTable, Error> {
    let parsed = SfntFont::parse(font)?;

    let c2pa_count = parsed.tables.iter().filter(|t| t.tag == C2PA_TAG).count();
    if c2pa_count > 1 {
        return Err(Error::InvalidTable(format!(
            "font contains {c2pa_count} C2PA tables; at most one is allowed"
        )));
    }

    let table = parsed.table(&C2PA_TAG).ok_or(Error::NotFound)?;
    C2paTable::decode(&table.data)
}

/// Read the embedded C2PA Manifest Store from a font.
///
/// Returns [`Error::NotFound`] if the font has no `C2PA` table, and
/// [`Error::InvalidTable`] if the table carries only a remote URI.
pub fn read_manifest(font: &[u8]) -> Result<Vec<u8>, Error> {
    read_c2pa_table(font)?
        .manifest_store
        .ok_or_else(|| Error::InvalidTable("C2PA table has no embedded manifest store".into()))
}

/// Read the active manifest URI from a font's `C2PA` table, if present.
pub fn read_manifest_uri(font: &[u8]) -> Result<Option<String>, Error> {
    Ok(read_c2pa_table(font)?.active_manifest_uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::embed_manifest;
    use crate::ManifestSource;

    fn sample_font() -> Vec<u8> {
        crate::writer::tests::sample_font()
    }

    #[test]
    fn read_embedded_manifest() {
        let font = sample_font();
        let embedded =
            embed_manifest(&font, ManifestSource::embedded(b"\x00\x01\x02".to_vec())).unwrap();
        assert_eq!(read_manifest(&embedded).unwrap(), b"\x00\x01\x02");
    }

    #[test]
    fn read_remote_uri() {
        let font = sample_font();
        let embedded =
            embed_manifest(&font, ManifestSource::remote("https://example.com/m.c2pa")).unwrap();
        assert_eq!(
            read_manifest_uri(&embedded).unwrap().as_deref(),
            Some("https://example.com/m.c2pa")
        );
    }

    #[test]
    fn read_manifest_missing_table() {
        let font = sample_font();
        assert!(matches!(read_manifest(&font), Err(Error::NotFound)));
    }

    #[test]
    fn read_manifest_remote_only_is_invalid() {
        let font = sample_font();
        let embedded =
            embed_manifest(&font, ManifestSource::remote("https://example.com/m.c2pa")).unwrap();
        assert!(matches!(
            read_manifest(&embedded),
            Err(Error::InvalidTable(_))
        ));
    }

    /// A font with two `C2PA` tables must be rejected by every entry point
    /// that reads one, not just [`crate::verify::verify`] -- otherwise a
    /// crafted font could smuggle an unverified second table past a caller
    /// that only locates/extracts (step 1) without also validating (step 6).
    #[test]
    fn read_manifest_rejects_duplicate_c2pa_table() {
        use crate::sfnt::{SfntFont, Table};

        let font = sample_font();
        let embedded =
            embed_manifest(&font, ManifestSource::embedded(b"\x00\x01\x02".to_vec())).unwrap();
        let mut parsed = SfntFont::parse(&embedded).unwrap();
        let c2pa = parsed
            .tables
            .iter()
            .find(|t| t.tag == C2PA_TAG)
            .cloned()
            .expect("embed_manifest wrote a C2PA table");
        parsed.tables.push(Table {
            tag: C2PA_TAG,
            data: c2pa.data.clone(),
        });
        let duplicated = parsed.serialize();

        assert!(matches!(
            read_c2pa_table(&duplicated),
            Err(Error::InvalidTable(_))
        ));
        assert!(matches!(
            read_manifest(&duplicated),
            Err(Error::InvalidTable(_))
        ));
        assert!(matches!(
            read_manifest_uri(&duplicated),
            Err(Error::InvalidTable(_))
        ));
    }
}
