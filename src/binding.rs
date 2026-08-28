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
    /// Byte offset of the first excluded byte.
    pub start: u64,
    /// Number of bytes excluded.
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
    use super::{data_hash_exclusions, Exclusion, HEAD_TAG};
    use crate::error::Error;
    use crate::sfnt::{self, locate_tables};
    use crate::table::C2PA_TAG;
    use c2pa::assertions::{BoxHash, BoxMap};
    use c2pa::HashRange;
    use std::io::Cursor;

    impl Exclusion {
        /// This exclusion as a c2pa-rs [`HashRange`], ready to attach to a
        /// `c2pa.hash.data` assertion.
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

    fn table_name(tag: &[u8; 4]) -> Result<String, Error> {
        if !tag.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
            return Err(Error::InvalidFont(
                "SFNT table tag is not printable ASCII".into(),
            ));
        }
        Ok(std::str::from_utf8(tag)
            .expect("printable ASCII is UTF-8")
            .to_string())
    }

    fn bytes_for_group(font: &[u8], locs: &[crate::sfnt::TableLoc]) -> Result<Vec<u8>, Error> {
        let first = locs
            .first()
            .ok_or_else(|| Error::Validation("empty font box group".into()))?;
        let last = locs.last().expect("first checked above");
        let end = last
            .data_offset
            .checked_add(last.length)
            .ok_or_else(|| Error::InvalidFont("table range overflow".into()))?;
        if first.data_offset > end
            || locs
                .windows(2)
                .any(|pair| pair[0].data_offset + pair[0].length > pair[1].data_offset)
        {
            return Err(Error::Validation(
                "grouped font tables are not in physical order".into(),
            ));
        }
        let mut bytes = font
            .get(first.data_offset..end)
            .ok_or_else(|| Error::InvalidFont("font box range is out of bounds".into()))?
            .to_vec();
        for loc in locs.iter().filter(|loc| loc.tag == HEAD_TAG) {
            if loc.length < sfnt::HEAD_CHECKSUM_ADJUSTMENT_OFFSET + 4 {
                return Err(Error::InvalidFont("head table is too short".into()));
            }
            let adjustment = loc.data_offset + sfnt::HEAD_CHECKSUM_ADJUSTMENT_OFFSET;
            let relative = adjustment - first.data_offset;
            bytes[relative..relative + 4].fill(0);
        }
        Ok(bytes)
    }

    fn digest(alg: &str, bytes: &[u8]) -> Result<Vec<u8>, Error> {
        c2pa::hash_stream_by_alg(alg, &mut Cursor::new(bytes), None, true)
            .map_err(|error| Error::Validation(error.to_string()))
    }

    /// Build the C2PA `c2pa.hash.boxes` assertion required for an SFNT font.
    /// Each table is emitted as its own box in table-directory order. The
    /// `C2PA` table is listed and excluded, while `head.checkSumAdjustment` is
    /// treated as zero when hashing the `head` box.
    pub fn compute_box_hash(font: &[u8], alg: &str) -> Result<BoxHash, Error> {
        let locs = locate_tables(font)?;
        if locs.iter().filter(|loc| loc.tag == C2PA_TAG).count() > 1 {
            return Err(Error::InvalidTable(
                "font contains more than one C2PA table".into(),
            ));
        }
        let mut boxes = Vec::with_capacity(locs.len());
        for loc in &locs {
            let excluded = loc.tag == C2PA_TAG;
            let hash = if excluded {
                vec![0]
            } else {
                digest(alg, &bytes_for_group(font, std::slice::from_ref(loc))?)?
            };
            boxes.push(BoxMap {
                names: vec![table_name(&loc.tag)?],
                alg: Some(alg.to_string()),
                hash: hash.into(),
                excluded: excluded.then_some(true),
                pad: Vec::new().into(),
                range_start: loc.data_offset as u64,
                range_len: loc.length as u64,
            });
        }
        Ok(BoxHash { boxes })
    }

    /// Verify a font `c2pa.hash.boxes` assertion with strict box accounting.
    /// Every SFNT table must appear exactly once and in table-directory order;
    /// an omitted, extra, or reordered table is reported as `unknownBox`.
    pub fn verify_box_hash(
        font: &[u8],
        assertion: &BoxHash,
        claim_alg: Option<&str>,
    ) -> Result<bool, Error> {
        let locs = locate_tables(font)?;
        let expected_names = locs
            .iter()
            .map(|loc| table_name(&loc.tag))
            .collect::<Result<Vec<_>, _>>()?;
        let asserted_names = assertion
            .boxes
            .iter()
            .flat_map(|box_map| box_map.names.iter().cloned())
            .collect::<Vec<_>>();
        if asserted_names != expected_names {
            return Err(Error::Validation("assertion.boxesHash.unknownBox".into()));
        }

        let mut index = 0;
        for box_map in &assertion.boxes {
            if box_map.names.is_empty() {
                return Err(Error::Validation("assertion.boxesHash.unknownBox".into()));
            }
            let end = index + box_map.names.len();
            let group = &locs[index..end];
            let contains_c2pa = group.iter().any(|loc| loc.tag == C2PA_TAG);
            if contains_c2pa {
                if group.len() != 1 || box_map.excluded != Some(true) {
                    return Err(Error::Validation("malformed C2PA font box".into()));
                }
            } else if box_map.excluded != Some(true) {
                let alg =
                    box_map.alg.as_deref().or(claim_alg).ok_or_else(|| {
                        Error::Validation("font box hash has no algorithm".into())
                    })?;
                let computed = digest(alg, &bytes_for_group(font, group)?)?;
                if box_map.hash.as_ref() != computed {
                    return Ok(false);
                }
            }
            index = end;
        }
        Ok(true)
    }
}

#[cfg(feature = "validation")]
pub use hashing::{
    compute_box_hash, compute_data_hash, data_hash_ranges, verify_box_hash, verify_data_hash,
};

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

    #[cfg(feature = "validation")]
    #[test]
    fn box_hash_enumerates_every_table_and_zeroes_head_adjustment() {
        let reserved = reserve_manifest(&sample_font(), 64, None).unwrap();
        let assertion = compute_box_hash(&reserved.font, "sha256").unwrap();
        assert!(verify_box_hash(&reserved.font, &assertion, None).unwrap());
        assert_eq!(
            assertion
                .boxes
                .iter()
                .filter(|box_map| box_map.names == ["C2PA"])
                .count(),
            1
        );

        let head = locate_tables(&reserved.font)
            .unwrap()
            .into_iter()
            .find(|loc| loc.tag == HEAD_TAG)
            .unwrap();
        let mut changed_adjustment = reserved.font.clone();
        changed_adjustment[head.data_offset + sfnt::HEAD_CHECKSUM_ADJUSTMENT_OFFSET] ^= 0xff;
        assert!(verify_box_hash(&changed_adjustment, &assertion, None).unwrap());
    }

    #[cfg(feature = "validation")]
    #[test]
    fn box_hash_rejects_unknown_or_tampered_tables() {
        let reserved = reserve_manifest(&sample_font(), 64, None).unwrap();
        let mut assertion = compute_box_hash(&reserved.font, "sha256").unwrap();
        assertion.boxes[0].names[0] = "FAKE".into();
        assert!(matches!(
            verify_box_hash(&reserved.font, &assertion, None),
            Err(Error::Validation(message)) if message == "assertion.boxesHash.unknownBox"
        ));

        let assertion = compute_box_hash(&reserved.font, "sha256").unwrap();
        let table = locate_tables(&reserved.font)
            .unwrap()
            .into_iter()
            .find(|loc| loc.tag != C2PA_TAG && loc.tag != HEAD_TAG)
            .unwrap();
        let mut tampered = reserved.font.clone();
        tampered[table.data_offset] ^= 1;
        assert!(!verify_box_hash(&tampered, &assertion, None).unwrap());
    }
}
