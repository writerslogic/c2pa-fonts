use crate::error::Error;

/// SFNT table tag identifying the C2PA table.
pub const C2PA_TAG: [u8; 4] = *b"C2PA";

/// Major version written into new C2PA tables. The format is preliminary in
/// the C2PA specification, so this crate emits version 0.1.
pub const MAJOR_VERSION: u16 = 0;
pub const MINOR_VERSION: u16 = 1;

/// Fixed portion of the C2PA table record, preceding the URI and manifest data.
const HEADER_LEN: usize = 20;

/// A decoded C2PA font table.
///
/// Per the C2PA specification, the table may carry a URI to the active
/// manifest, an embedded Manifest Store, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct C2paTable {
    pub major_version: u16,
    pub minor_version: u16,
    pub active_manifest_uri: Option<String>,
    pub manifest_store: Option<Vec<u8>>,
}

impl C2paTable {
    pub fn new(active_manifest_uri: Option<String>, manifest_store: Option<Vec<u8>>) -> Self {
        Self {
            major_version: MAJOR_VERSION,
            minor_version: MINOR_VERSION,
            active_manifest_uri,
            manifest_store,
        }
    }

    /// Encode the table record.
    ///
    /// Layout: a 20-byte header, followed by the URI bytes (if any), followed
    /// by the Manifest Store bytes (if any). All integers are big-endian.
    pub fn encode(&self) -> Vec<u8> {
        let uri = self.active_manifest_uri.as_deref().unwrap_or("").as_bytes();
        let store = self.manifest_store.as_deref().unwrap_or(&[]);

        let uri_offset = if uri.is_empty() { 0 } else { HEADER_LEN as u32 };
        let store_offset = if store.is_empty() {
            0
        } else {
            (HEADER_LEN + uri.len()) as u32
        };

        let mut out = Vec::with_capacity(HEADER_LEN + uri.len() + store.len());
        out.extend_from_slice(&self.major_version.to_be_bytes());
        out.extend_from_slice(&self.minor_version.to_be_bytes());
        out.extend_from_slice(&uri_offset.to_be_bytes());
        out.extend_from_slice(&(uri.len() as u16).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        out.extend_from_slice(&store_offset.to_be_bytes());
        out.extend_from_slice(&(store.len() as u32).to_be_bytes());
        out.extend_from_slice(uri);
        out.extend_from_slice(store);
        out
    }

    pub fn decode(data: &[u8]) -> Result<Self, Error> {
        if data.len() < HEADER_LEN {
            return Err(Error::InvalidTable(
                "table shorter than 20-byte header".into(),
            ));
        }
        let major_version = read_u16(data, 0);
        let minor_version = read_u16(data, 2);
        let uri_offset = read_u32(data, 4) as usize;
        let uri_length = read_u16(data, 8) as usize;
        // bytes 10..12 reserved
        let store_offset = read_u32(data, 12) as usize;
        let store_length = read_u32(data, 16) as usize;

        let active_manifest_uri = if uri_offset == 0 {
            None
        } else {
            let bytes = slice(data, uri_offset, uri_length, "active manifest URI")?;
            let s = std::str::from_utf8(bytes).map_err(|_| {
                Error::InvalidTable("active manifest URI is not valid UTF-8".into())
            })?;
            Some(s.to_string())
        };

        let manifest_store = if store_offset == 0 {
            None
        } else {
            Some(slice(data, store_offset, store_length, "manifest store")?.to_vec())
        };

        Ok(Self {
            major_version,
            minor_version,
            active_manifest_uri,
            manifest_store,
        })
    }
}

fn slice<'a>(data: &'a [u8], offset: usize, length: usize, what: &str) -> Result<&'a [u8], Error> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::InvalidTable(format!("{what} length overflow")))?;
    if end > data.len() {
        return Err(Error::InvalidTable(format!(
            "{what} extends past end of table"
        )));
    }
    Ok(&data[offset..end])
}

fn read_u16(data: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([data[off], data[off + 1]])
}

fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_embedded_only() {
        let table = C2paTable::new(None, Some(b"\x00\x01\x02\x03".to_vec()));
        let decoded = C2paTable::decode(&table.encode()).unwrap();
        assert_eq!(decoded, table);
        assert_eq!(decoded.active_manifest_uri, None);
        assert_eq!(decoded.manifest_store.unwrap(), b"\x00\x01\x02\x03");
    }

    #[test]
    fn encode_decode_remote_only() {
        let table = C2paTable::new(Some("https://example.com/m.c2pa".into()), None);
        let decoded = C2paTable::decode(&table.encode()).unwrap();
        assert_eq!(decoded, table);
        assert_eq!(decoded.manifest_store, None);
    }

    #[test]
    fn encode_decode_both() {
        let table = C2paTable::new(
            Some("urn:c2pa:manifest".into()),
            Some(b"\xCA\xFE\xBA\xBE".to_vec()),
        );
        let bytes = table.encode();
        let decoded = C2paTable::decode(&bytes).unwrap();
        assert_eq!(decoded, table);
    }

    #[test]
    fn null_offsets_when_absent() {
        let bytes = C2paTable::new(None, None).encode();
        assert_eq!(read_u32(&bytes, 4), 0); // uri offset
        assert_eq!(read_u32(&bytes, 12), 0); // store offset
    }

    #[test]
    fn rejects_truncated_header() {
        assert!(matches!(
            C2paTable::decode(&[0u8; 8]),
            Err(Error::InvalidTable(_))
        ));
    }

    #[test]
    fn rejects_out_of_bounds_offset() {
        let mut bytes = C2paTable::new(None, Some(b"data".to_vec())).encode();
        // Corrupt the store length to exceed the table.
        bytes[16..20].copy_from_slice(&9999u32.to_be_bytes());
        assert!(matches!(
            C2paTable::decode(&bytes),
            Err(Error::InvalidTable(_))
        ));
    }
}
