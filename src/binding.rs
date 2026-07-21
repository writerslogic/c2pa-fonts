use crate::error::Error;
use crate::sfnt::{self, locate_tables, read_u32};
use crate::table::C2PA_TAG;

const HEAD_TAG: [u8; 4] = *b"head";
/// Offset of `manifestStoreOffset` / `manifestStoreLength` within the C2PA table.
const STORE_OFFSET_FIELD: usize = 12;
const STORE_LENGTH_FIELD: usize = 16;

/// A byte range excluded from the `c2pa.hash.data` hard binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exclusion {
    pub start: u64,
    pub length: u64,
}

/// Byte ranges excluded from the font's `c2pa.hash.data` hard binding.
///
/// Three regions change when a reserved manifest is filled with signed bytes,
/// so all three must be excluded for the placeholder-then-fill flow to leave
/// the hash intact:
///
/// 1. the embedded Manifest Store itself (it carries the signature over the hash),
/// 2. the `C2PA` table's directory checksum (a function of the store bytes), and
/// 3. `head.checkSumAdjustment` (a function of the whole font, hence of the store).
///
/// The ranges are returned sorted by start offset.
pub fn data_hash_exclusions(font: &[u8]) -> Result<Vec<Exclusion>, Error> {
    let locs = locate_tables(font)?;

    let c2pa = locs
        .iter()
        .find(|l| l.tag == C2PA_TAG)
        .ok_or(Error::NotFound)?;

    if c2pa.length < STORE_LENGTH_FIELD + 4 {
        return Err(Error::InvalidTable("C2PA table shorter than header".into()));
    }
    let store_offset = read_u32(font, c2pa.data_offset + STORE_OFFSET_FIELD) as usize;
    let store_length = read_u32(font, c2pa.data_offset + STORE_LENGTH_FIELD) as usize;
    if store_offset == 0 || store_length == 0 {
        return Err(Error::InvalidTable(
            "no embedded manifest store to bind".into(),
        ));
    }
    let store_start = c2pa
        .data_offset
        .checked_add(store_offset)
        .ok_or_else(|| Error::InvalidTable("store offset overflow".into()))?;
    let store_end = store_start
        .checked_add(store_length)
        .ok_or_else(|| Error::InvalidTable("store length overflow".into()))?;
    if store_end > c2pa.data_offset + c2pa.length {
        return Err(Error::InvalidTable(
            "manifest store extends past C2PA table".into(),
        ));
    }

    let mut exclusions = vec![
        Exclusion {
            start: store_start as u64,
            length: store_length as u64,
        },
        Exclusion {
            start: (c2pa.record_offset + 4) as u64,
            length: 4,
        },
    ];

    if let Some(head) = locs.iter().find(|l| l.tag == HEAD_TAG) {
        if head.length >= sfnt::HEAD_CHECKSUM_ADJUSTMENT_OFFSET + 4 {
            exclusions.push(Exclusion {
                start: (head.data_offset + sfnt::HEAD_CHECKSUM_ADJUSTMENT_OFFSET) as u64,
                length: 4,
            });
        }
    }

    exclusions.sort_by_key(|e| e.start);
    Ok(exclusions)
}

#[cfg(feature = "validation")]
mod hashing {
    use super::{data_hash_exclusions, Exclusion};
    use crate::error::Error;
    use c2pa::HashRange;
    use std::io::Cursor;

    impl Exclusion {
        pub fn to_hash_range(self) -> HashRange {
            HashRange::new(self.start, self.length)
        }
    }

    /// The font's exclusion ranges as c2pa-rs [`HashRange`]s, ready to attach to
    /// a `c2pa.hash.data` assertion.
    pub fn data_hash_ranges(font: &[u8]) -> Result<Vec<HashRange>, Error> {
        Ok(data_hash_exclusions(font)?
            .into_iter()
            .map(Exclusion::to_hash_range)
            .collect())
    }

    /// Compute the `c2pa.hash.data` value over the font using the font's own
    /// exclusion ranges, via the same hasher c2pa-rs uses at validation time.
    pub fn compute_data_hash(font: &[u8], alg: &str) -> Result<Vec<u8>, Error> {
        let ranges = data_hash_ranges(font)?;
        c2pa::hash_stream_by_alg(alg, &mut Cursor::new(font), Some(ranges), true)
            .map_err(|e| Error::Validation(e.to_string()))
    }

    /// Verify that the font's bytes hash to `expected` under its exclusion ranges.
    pub fn verify_data_hash(font: &[u8], expected: &[u8], alg: &str) -> Result<bool, Error> {
        Ok(compute_data_hash(font, alg)? == expected)
    }
}

#[cfg(feature = "validation")]
pub use hashing::{compute_data_hash, data_hash_ranges, verify_data_hash};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{reserve_manifest, tests::sample_font};

    #[test]
    fn exclusions_cover_store_and_two_checksums() {
        let reserved = reserve_manifest(&sample_font(), 64, None).unwrap();
        let ex = data_hash_exclusions(&reserved.font).unwrap();
        assert_eq!(ex.len(), 3);
        // The store exclusion is the 64-byte reserved region.
        assert!(ex.iter().any(|e| e.length == 64));
        // Two 4-byte checksum exclusions.
        assert_eq!(ex.iter().filter(|e| e.length == 4).count(), 2);
    }

    #[test]
    fn exclusions_sorted_and_in_bounds() {
        let reserved = reserve_manifest(&sample_font(), 32, None).unwrap();
        let ex = data_hash_exclusions(&reserved.font).unwrap();
        for w in ex.windows(2) {
            assert!(w[0].start <= w[1].start);
        }
        for e in &ex {
            assert!((e.start + e.length) as usize <= reserved.font.len());
        }
    }

    #[test]
    fn no_table_is_not_found() {
        assert!(matches!(
            data_hash_exclusions(&sample_font()),
            Err(Error::NotFound)
        ));
    }
}
