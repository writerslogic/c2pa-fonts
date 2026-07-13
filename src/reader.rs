use crate::error::Error;
use crate::sfnt::SfntFont;
use crate::table::{C2paTable, C2PA_TAG};

/// Read and decode the `C2PA` table from a font.
pub fn read_c2pa_table(font: &[u8]) -> Result<C2paTable, Error> {
    let parsed = SfntFont::parse(font)?;
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
}
