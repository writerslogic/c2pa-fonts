use crate::error::Error;
use crate::sfnt::{checksum_adjustment_valid, SfntFont};
use crate::table::{C2paTable, C2PA_TAG};

/// The result of checking a font's C2PA embedding for spec compliance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compliance {
    /// The font contains exactly one `C2PA` table.
    pub has_c2pa_table: bool,
    /// `(majorVersion, minorVersion)` of the C2PA table, if present.
    pub table_version: Option<(u16, u16)>,
    /// The table carries an embedded Manifest Store.
    pub has_embedded_manifest: bool,
    /// The table carries a remote manifest URI.
    pub has_remote_uri: bool,
    /// The `head.checkSumAdjustment` matches the font's current bytes.
    pub checksum_valid: bool,
}

impl Compliance {
    /// A font is compliant when it has a well-formed `C2PA` table referencing at
    /// least one manifest, and its SFNT checksums are intact.
    pub fn is_compliant(&self) -> bool {
        self.has_c2pa_table
            && (self.has_embedded_manifest || self.has_remote_uri)
            && self.checksum_valid
    }
}

/// Verify a font's C2PA embedding against the specification.
///
/// Returns a [`Compliance`] report. Structural failures that prevent parsing
/// (a non-SFNT file, a truncated table directory, more than one `C2PA` table,
/// or a malformed `C2PA` table) are returned as [`Error`].
pub fn verify(font: &[u8]) -> Result<Compliance, Error> {
    let parsed = SfntFont::parse(font)?;

    let c2pa_count = parsed.tables.iter().filter(|t| t.tag == C2PA_TAG).count();
    if c2pa_count > 1 {
        return Err(Error::InvalidTable(format!(
            "font contains {c2pa_count} C2PA tables; at most one is allowed"
        )));
    }

    let checksum_valid = checksum_adjustment_valid(font)?;

    let Some(table) = parsed.table(&C2PA_TAG) else {
        return Ok(Compliance {
            has_c2pa_table: false,
            table_version: None,
            has_embedded_manifest: false,
            has_remote_uri: false,
            checksum_valid,
        });
    };

    let decoded = C2paTable::decode(&table.data)?;
    Ok(Compliance {
        has_c2pa_table: true,
        table_version: Some((decoded.major_version, decoded.minor_version)),
        has_embedded_manifest: decoded.manifest_store.is_some(),
        has_remote_uri: decoded.active_manifest_uri.is_some(),
        checksum_valid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{embed_manifest, tests::sample_font};
    use crate::ManifestSource;

    #[test]
    fn compliant_embedded_font() {
        let font = embed_manifest(&sample_font(), ManifestSource::embedded(b"m".to_vec())).unwrap();
        let report = verify(&font).unwrap();
        assert!(report.is_compliant());
        assert!(report.has_embedded_manifest);
        assert_eq!(report.table_version, Some((0, 1)));
    }

    #[test]
    fn compliant_remote_font() {
        let font =
            embed_manifest(&sample_font(), ManifestSource::remote("https://x/m.c2pa")).unwrap();
        let report = verify(&font).unwrap();
        assert!(report.is_compliant());
        assert!(report.has_remote_uri);
        assert!(!report.has_embedded_manifest);
    }

    #[test]
    fn font_without_table_is_not_compliant() {
        let report = verify(&sample_font()).unwrap();
        assert!(!report.has_c2pa_table);
        assert!(!report.is_compliant());
    }

    #[test]
    fn tampered_font_fails_checksum() {
        let mut font =
            embed_manifest(&sample_font(), ManifestSource::embedded(b"m".to_vec())).unwrap();
        let last = font.len() - 1;
        font[last] ^= 0xFF;
        let report = verify(&font).unwrap();
        assert!(!report.checksum_valid);
        assert!(!report.is_compliant());
    }
}
