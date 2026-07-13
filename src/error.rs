use std::fmt;

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
    Io(std::io::Error),
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
