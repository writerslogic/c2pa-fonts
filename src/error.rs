use std::fmt;

/// Errors from reading, writing, or validating a font's `C2PA` table.
#[derive(Debug)]
pub enum Error {
    /// No `C2PA` table is present in the font.
    NotFound,
    /// The font is not a supported SFNT (TrueType/OpenType) file.
    NotSfnt,
    /// Font collections (`ttcf`) are not supported.
    Collection,
    /// WOFF/WOFF2 wrapped fonts are not supported; decompress to SFNT first.
    Woff,
    /// The SFNT structure could not be parsed.
    InvalidFont(String),
    /// The `C2PA` table could not be parsed or violates the spec.
    InvalidTable(String),
    /// Hard-binding or delegated (c2pa-rs) validation failed.
    Validation(String),
    /// An underlying I/O failure.
    Io(std::io::Error),
}

impl Error {
    /// The registered C2PA validation status code for this error, or `None`
    /// when the condition carries no status code.
    ///
    /// The specification defines no font-specific codes. [`Error::NotFound`]
    /// means the font carries no provenance, which is not a failure, and the
    /// parsing variants occur before any manifest is located.
    ///
    /// [`Error::Validation`] wraps a failure reported by the delegated
    /// validator, which carries its own status codes; those are surfaced in the
    /// report from [`crate::validate`] rather than flattened into this one
    /// string.
    ///
    /// Every crate in this family exposes this method, so a dispatcher handling
    /// several embedding methods can ask the same question of any of them.
    pub fn code(&self) -> Option<&'static str> {
        None
    }

    /// Whether this error means the font carries no provenance at all, as
    /// opposed to provenance that was found and rejected.
    pub fn is_no_manifest_located(&self) -> bool {
        matches!(self, Self::NotFound)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "no C2PA table found in font"),
            Self::NotSfnt => write!(f, "not a supported SFNT (TrueType/OpenType) font"),
            Self::Collection => write!(f, "font collections (ttcf) are not supported"),
            Self::Woff => write!(
                f,
                "WOFF/WOFF2 fonts are not supported; decompress to SFNT first"
            ),
            Self::InvalidFont(s) => write!(f, "invalid font: {s}"),
            Self::InvalidTable(s) => write!(f, "invalid C2PA table: {s}"),
            Self::Validation(s) => write!(f, "validation failed: {s}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The specification defines no font-specific codes, so none may appear.
    /// Guards against a later edit inventing one.
    #[test]
    fn no_variant_claims_a_status_code() {
        for e in [
            Error::NotFound,
            Error::NotSfnt,
            Error::Collection,
            Error::Woff,
            Error::InvalidFont("x".into()),
            Error::InvalidTable("x".into()),
            Error::Validation("x".into()),
            Error::Io(std::io::Error::other("x")),
        ] {
            assert_eq!(e.code(), None, "{e:?} claimed a status code");
        }
    }

    /// A font with no C2PA table is unsigned; an unparseable one is a different
    /// problem, and a delegated validation failure is different again.
    #[test]
    fn only_a_missing_table_means_unsigned() {
        assert!(Error::NotFound.is_no_manifest_located());
        for e in [
            Error::NotSfnt,
            Error::Woff,
            Error::InvalidTable("x".into()),
            Error::Validation("x".into()),
        ] {
            assert!(!e.is_no_manifest_located(), "{e:?} misreported as unsigned");
        }
    }
}
