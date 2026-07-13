use crate::error::Error;

const OFFSET_TABLE_LEN: usize = 12;
const DIRECTORY_RECORD_LEN: usize = 16;
const HEAD_TAG: [u8; 4] = *b"head";
pub(crate) const HEAD_CHECKSUM_ADJUSTMENT_OFFSET: usize = 8;
const CHECKSUM_MAGIC: u32 = 0xB1B0_AFBA;

/// The location of one table within a serialized font.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TableLoc {
    pub tag: [u8; 4],
    /// Offset of the 16-byte directory record (whose bytes 4..8 hold the checksum).
    pub record_offset: usize,
    /// Offset of the table data within the file.
    pub data_offset: usize,
    pub length: usize,
}

/// A single SFNT table: its 4-byte tag and raw (unpadded) data.
#[derive(Debug, Clone)]
pub struct Table {
    pub tag: [u8; 4],
    pub data: Vec<u8>,
}

/// A parsed SFNT font (a single TrueType or OpenType font, not a collection).
#[derive(Debug, Clone)]
pub struct SfntFont {
    pub sfnt_version: u32,
    pub tables: Vec<Table>,
}

impl SfntFont {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() < OFFSET_TABLE_LEN {
            return Err(Error::NotSfnt);
        }
        let sfnt_version = read_u32(data, 0);
        if sfnt_version == u32::from_be_bytes(*b"ttcf") {
            return Err(Error::Collection);
        }
        if sfnt_version == u32::from_be_bytes(*b"wOFF")
            || sfnt_version == u32::from_be_bytes(*b"wOF2")
        {
            return Err(Error::Woff);
        }
        if !is_known_sfnt_version(sfnt_version) {
            return Err(Error::NotSfnt);
        }

        let num_tables = read_u16(data, 4) as usize;
        let directory_end = OFFSET_TABLE_LEN + num_tables * DIRECTORY_RECORD_LEN;
        if data.len() < directory_end {
            return Err(Error::InvalidFont("table directory truncated".into()));
        }

        let mut tables = Vec::with_capacity(num_tables);
        for i in 0..num_tables {
            let rec = OFFSET_TABLE_LEN + i * DIRECTORY_RECORD_LEN;
            let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
            let offset = read_u32(data, rec + 8) as usize;
            let length = read_u32(data, rec + 12) as usize;
            let end = offset
                .checked_add(length)
                .ok_or_else(|| Error::InvalidFont("table length overflow".into()))?;
            if end > data.len() {
                return Err(Error::InvalidFont(format!(
                    "table {} extends past end of file",
                    tag_name(&tag)
                )));
            }
            tables.push(Table {
                tag,
                data: data[offset..end].to_vec(),
            });
        }

        Ok(Self {
            sfnt_version,
            tables,
        })
    }

    pub fn table(&self, tag: &[u8; 4]) -> Option<&Table> {
        self.tables.iter().find(|t| &t.tag == tag)
    }

    pub fn remove_table(&mut self, tag: &[u8; 4]) {
        self.tables.retain(|t| &t.tag != tag);
    }

    /// Insert or replace a table, keeping at most one entry per tag.
    pub fn set_table(&mut self, tag: [u8; 4], data: Vec<u8>) {
        self.remove_table(&tag);
        self.tables.push(Table { tag, data });
    }

    /// Serialize to a valid SFNT font.
    ///
    /// The table directory is written sorted by tag (as required by the
    /// OpenType specification), table offsets are laid out with 4-byte
    /// alignment, per-table checksums are recomputed, and the `head` table's
    /// `checkSumAdjustment` is recalculated over the whole font.
    pub fn serialize(&self) -> Vec<u8> {
        let mut tables = self.tables.clone();
        tables.sort_by_key(|t| t.tag);

        let num_tables = tables.len();
        let data_start = OFFSET_TABLE_LEN + num_tables * DIRECTORY_RECORD_LEN;

        // Assign aligned offsets to each table.
        let mut offset = data_start;
        let mut offsets = Vec::with_capacity(num_tables);
        for t in &tables {
            offsets.push(offset);
            offset += align4(t.data.len());
        }
        let total_len = offset;

        let mut out = vec![0u8; total_len];
        write_offset_table(&mut out, self.sfnt_version, num_tables);

        let mut head_offset = None;
        for (i, t) in tables.iter().enumerate() {
            let rec = OFFSET_TABLE_LEN + i * DIRECTORY_RECORD_LEN;
            let off = offsets[i];
            let checksum = if t.tag == HEAD_TAG {
                head_offset = Some(off);
                head_checksum(&t.data)
            } else {
                table_checksum(&t.data)
            };
            out[rec..rec + 4].copy_from_slice(&t.tag);
            write_u32(&mut out, rec + 4, checksum);
            write_u32(&mut out, rec + 8, off as u32);
            write_u32(&mut out, rec + 12, t.data.len() as u32);
            out[off..off + t.data.len()].copy_from_slice(&t.data);
        }

        // checkSumAdjustment = magic - checksum(whole font with the field zeroed)
        if let Some(head_off) = head_offset {
            let adj_pos = head_off + HEAD_CHECKSUM_ADJUSTMENT_OFFSET;
            write_u32(&mut out, adj_pos, 0);
            let whole = table_checksum(&out);
            write_u32(&mut out, adj_pos, CHECKSUM_MAGIC.wrapping_sub(whole));
        }

        out
    }
}

/// Verify that a font's `head.checkSumAdjustment` matches the value computed
/// over its current bytes.
pub fn checksum_adjustment_valid(data: &[u8]) -> Result<bool, Error> {
    let font = SfntFont::parse(data)?;
    let Some(head) = font.table(&HEAD_TAG) else {
        return Ok(false);
    };
    if head.data.len() < HEAD_CHECKSUM_ADJUSTMENT_OFFSET + 4 {
        return Ok(false);
    }
    let stored = read_u32(&head.data, HEAD_CHECKSUM_ADJUSTMENT_OFFSET);

    let mut zeroed = data.to_vec();
    // Locate the head table's checkSumAdjustment within the file and zero it.
    let num_tables = read_u16(data, 4) as usize;
    for i in 0..num_tables {
        let rec = OFFSET_TABLE_LEN + i * DIRECTORY_RECORD_LEN;
        if data[rec..rec + 4] == HEAD_TAG {
            let off = read_u32(data, rec + 8) as usize;
            let adj_pos = off + HEAD_CHECKSUM_ADJUSTMENT_OFFSET;
            if adj_pos + 4 > zeroed.len() {
                return Ok(false);
            }
            write_u32(&mut zeroed, adj_pos, 0);
            break;
        }
    }
    let expected = CHECKSUM_MAGIC.wrapping_sub(table_checksum(&zeroed));
    Ok(stored == expected)
}

/// Locate every table's directory record and data region within a serialized
/// font, without copying table data.
pub(crate) fn locate_tables(data: &[u8]) -> Result<Vec<TableLoc>, Error> {
    if data.len() < OFFSET_TABLE_LEN {
        return Err(Error::NotSfnt);
    }
    let num_tables = read_u16(data, 4) as usize;
    let directory_end = OFFSET_TABLE_LEN + num_tables * DIRECTORY_RECORD_LEN;
    if data.len() < directory_end {
        return Err(Error::InvalidFont("table directory truncated".into()));
    }
    let mut locs = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = OFFSET_TABLE_LEN + i * DIRECTORY_RECORD_LEN;
        let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
        let data_offset = read_u32(data, rec + 8) as usize;
        let length = read_u32(data, rec + 12) as usize;
        let end = data_offset
            .checked_add(length)
            .ok_or_else(|| Error::InvalidFont("table length overflow".into()))?;
        if end > data.len() {
            return Err(Error::InvalidFont("table extends past end of file".into()));
        }
        locs.push(TableLoc {
            tag,
            record_offset: rec,
            data_offset,
            length,
        });
    }
    Ok(locs)
}

fn is_known_sfnt_version(v: u32) -> bool {
    v == 0x0001_0000 // TrueType outlines
        || v == u32::from_be_bytes(*b"OTTO") // CFF outlines
        || v == u32::from_be_bytes(*b"true") // Apple TrueType
        || v == u32::from_be_bytes(*b"typ1") // Apple Type 1
}

fn write_offset_table(out: &mut [u8], sfnt_version: u32, num_tables: usize) {
    let n = num_tables as u16;
    let entry_selector = if n == 0 {
        0
    } else {
        15 - n.leading_zeros() as u16
    };
    let search_range = (1u16 << entry_selector).wrapping_mul(16);
    let range_shift = n.wrapping_mul(16).wrapping_sub(search_range);
    write_u32(out, 0, sfnt_version);
    write_u16(out, 4, n);
    write_u16(out, 6, search_range);
    write_u16(out, 8, entry_selector);
    write_u16(out, 10, range_shift);
}

/// SFNT table checksum: the sum of the data interpreted as big-endian
/// `uint32`s, zero-padded to a 4-byte multiple, with wrapping addition.
fn table_checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < data.len() {
        let mut word = [0u8; 4];
        let n = core::cmp::min(4, data.len() - i);
        word[..n].copy_from_slice(&data[i..i + n]);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
        i += 4;
    }
    sum
}

/// The `head` table checksum is computed with `checkSumAdjustment` treated as 0.
fn head_checksum(data: &[u8]) -> u32 {
    let mut copy = data.to_vec();
    if copy.len() >= HEAD_CHECKSUM_ADJUSTMENT_OFFSET + 4 {
        write_u32(&mut copy, HEAD_CHECKSUM_ADJUSTMENT_OFFSET, 0);
    }
    table_checksum(&copy)
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn tag_name(tag: &[u8; 4]) -> String {
    String::from_utf8_lossy(tag).into_owned()
}

fn read_u16(data: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([data[off], data[off + 1]])
}

fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn write_u16(out: &mut [u8], off: usize, v: u16) {
    out[off..off + 2].copy_from_slice(&v.to_be_bytes());
}

fn write_u32(out: &mut [u8], off: usize, v: u32) {
    out[off..off + 4].copy_from_slice(&v.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal but structurally valid font for tests.
    fn sample_font() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        write_u32(&mut head, 0, 0x0001_0000); // version
        write_u32(&mut head, 12, 0x5F0F_3CF5); // magicNumber
        let cmap = vec![0u8; 20];
        let glyf = b"glyphdata!".to_vec(); // length 10, needs padding
        SfntFont {
            sfnt_version: 0x0001_0000,
            tables: vec![
                Table {
                    tag: *b"glyf",
                    data: glyf,
                },
                Table {
                    tag: *b"cmap",
                    data: cmap,
                },
                Table {
                    tag: HEAD_TAG,
                    data: head,
                },
            ],
        }
        .serialize()
    }

    #[test]
    fn parse_roundtrip_preserves_tables() {
        let font = sample_font();
        let parsed = SfntFont::parse(&font).unwrap();
        assert_eq!(parsed.tables.len(), 3);
        assert_eq!(parsed.table(b"glyf").unwrap().data, b"glyphdata!");
        assert_eq!(parsed.table(b"cmap").unwrap().data.len(), 20);
    }

    #[test]
    fn directory_is_sorted_by_tag() {
        let font = sample_font();
        let n = read_u16(&font, 4) as usize;
        let mut tags = Vec::new();
        for i in 0..n {
            let rec = OFFSET_TABLE_LEN + i * DIRECTORY_RECORD_LEN;
            tags.push([font[rec], font[rec + 1], font[rec + 2], font[rec + 3]]);
        }
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(tags, sorted);
    }

    #[test]
    fn offsets_are_four_byte_aligned() {
        let font = sample_font();
        let n = read_u16(&font, 4) as usize;
        for i in 0..n {
            let rec = OFFSET_TABLE_LEN + i * DIRECTORY_RECORD_LEN;
            assert_eq!(read_u32(&font, rec + 8) % 4, 0);
        }
    }

    #[test]
    fn checksum_adjustment_is_valid() {
        let font = sample_font();
        assert!(checksum_adjustment_valid(&font).unwrap());
    }

    #[test]
    fn tampering_invalidates_checksum_adjustment() {
        let mut font = sample_font();
        let last = font.len() - 1;
        font[last] ^= 0xFF;
        assert!(!checksum_adjustment_valid(&font).unwrap());
    }

    #[test]
    fn rejects_collections() {
        let mut data = vec![0u8; 12];
        data[..4].copy_from_slice(b"ttcf");
        assert!(matches!(SfntFont::parse(&data), Err(Error::Collection)));
    }

    #[test]
    fn rejects_woff() {
        let mut data = vec![0u8; 12];
        data[..4].copy_from_slice(b"wOFF");
        assert!(matches!(SfntFont::parse(&data), Err(Error::Woff)));
        data[..4].copy_from_slice(b"wOF2");
        assert!(matches!(SfntFont::parse(&data), Err(Error::Woff)));
    }

    #[test]
    fn rejects_non_sfnt() {
        let data = b"not a font at all!!!".to_vec();
        assert!(matches!(SfntFont::parse(&data), Err(Error::NotSfnt)));
    }

    #[test]
    fn locate_tables_matches_parse() {
        let font = sample_font();
        let locs = locate_tables(&font).unwrap();
        assert_eq!(locs.len(), 3);
        for loc in &locs {
            // Directory record checksum field sits at record_offset + 4.
            assert!(loc.record_offset + DIRECTORY_RECORD_LEN <= font.len());
            assert!(loc.data_offset + loc.length <= font.len());
        }
        let head = locs.iter().find(|l| l.tag == HEAD_TAG).unwrap();
        assert!(head.length >= HEAD_CHECKSUM_ADJUSTMENT_OFFSET + 4);
    }
}
