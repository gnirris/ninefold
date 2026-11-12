//! Chunks, in the format 1.8.9 sends them — which is nothing like a modern one.
//!
//! A column is sixteen 16×16×16 sections, and a bitmask says which of them were sent; the rest
//! are air. Each block is a little-endian `u16` of `id << 4 | metadata`: **pre-flattening**, so
//! there is no palette, no block state registry, and no sharing any of this with 1.13+.
//!
//! The payload is grouped by kind, not by section: every section's blocks, then every section's
//! block light, then every section's sky light, then the biomes. Reading it in section order
//! instead is the classic way to get a chunk that looks like noise.

use crate::{Error, Reader, Result};

pub const SECTION_BLOCKS: usize = 16 * 16 * 16;
/// Two bytes a block.
pub const SECTION_BYTES: usize = SECTION_BLOCKS * 2;
/// Half a byte a block.
pub const LIGHT_BYTES: usize = SECTION_BLOCKS / 2;
pub const SECTIONS: usize = 16;
pub const BIOME_BYTES: usize = 256;

/// One 16×16×16 cube of blocks.
#[derive(Debug, Clone)]
pub struct Section {
    /// `id << 4 | metadata`, indexed `y * 256 + z * 16 + x`.
    pub blocks: Vec<u16>,
}

impl Section {
    /// The block at a position inside this section.
    #[must_use]
    pub fn at(&self, x: usize, y: usize, z: usize) -> u16 {
        self.blocks[y * 256 + z * 16 + x]
    }

    /// Block id, without the metadata in the low four bits.
    #[must_use]
    pub fn id_at(&self, x: usize, y: usize, z: usize) -> u16 {
        self.at(x, y, z) >> 4
    }
}

/// A 16×256×16 column, as sent.
#[derive(Debug, Clone)]
pub struct Column {
    pub x: i32,
    pub z: i32,
    /// `None` where the section is all air and was not sent.
    pub sections: Vec<Option<Section>>,
    pub biomes: Option<Vec<u8>>,
}

impl Column {
    /// The block at a world position inside this column, or 0 (air) above or below the world.
    #[must_use]
    pub fn block_at(&self, x: usize, y: usize, z: usize) -> u16 {
        if y >= SECTIONS * 16 {
            return 0;
        }
        match &self.sections[y / 16] {
            Some(section) => section.at(x, y % 16, z),
            None => 0,
        }
    }

    /// Put a block into the column, growing an all-air section into existence if need be.
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: u16) {
        if y >= SECTIONS * 16 {
            return;
        }
        let section = &mut self.sections[y / 16];
        if section.is_none() {
            if block >> 4 == 0 {
                // Air into air: nothing to store.
                return;
            }
            *section = Some(Section {
                blocks: vec![0; SECTION_BLOCKS],
            });
        }
        if let Some(section) = section {
            section.blocks[(y % 16) * 256 + z * 16 + x] = block;
        }
    }

    /// How many blocks in the column are not air. Cheap sanity check on a decode.
    #[must_use]
    pub fn solid_count(&self) -> usize {
        self.sections
            .iter()
            .flatten()
            .flat_map(|s| s.blocks.iter())
            .filter(|b| **b != 0)
            .count()
    }
}

/// Decode one column's payload.
///
/// `mask` has a bit per section, lowest first. `ground_up` means the biome array is on the end,
/// which it is for a full column and is not for an update to part of one. `sky_light` is false
/// in the Nether and the End, where no sky light is sent at all.
pub fn column(
    x: i32,
    z: i32,
    mask: u16,
    ground_up: bool,
    sky_light: bool,
    data: &[u8],
) -> Result<Column> {
    let present: Vec<usize> = (0..SECTIONS).filter(|i| mask & (1 << i) != 0).collect();
    let wanted = present.len() * (SECTION_BYTES + LIGHT_BYTES)
        + if sky_light {
            present.len() * LIGHT_BYTES
        } else {
            0
        }
        + if ground_up { BIOME_BYTES } else { 0 };
    if data.len() < wanted {
        return Err(Error::Incomplete);
    }

    let mut sections: Vec<Option<Section>> = vec![None; SECTIONS];
    let mut r = Reader::new(data);
    // Blocks for every present section come first, all together.
    for index in &present {
        let mut blocks = Vec::with_capacity(SECTION_BLOCKS);
        for _ in 0..SECTION_BLOCKS {
            // Little-endian here, unlike everything else in the protocol.
            let low = u16::from(r.u8()?);
            let high = u16::from(r.u8()?);
            blocks.push(low | (high << 8));
        }
        sections[*index] = Some(Section { blocks });
    }
    // Then the light, which nothing here uses yet but has to be stepped over to reach the biomes.
    let light = present.len() * LIGHT_BYTES * if sky_light { 2 } else { 1 };
    for _ in 0..light {
        r.u8()?;
    }
    let biomes = if ground_up {
        let mut out = Vec::with_capacity(BIOME_BYTES);
        for _ in 0..BIOME_BYTES {
            out.push(r.u8()?);
        }
        Some(out)
    } else {
        None
    };

    Ok(Column {
        x,
        z,
        sections,
        biomes,
    })
}

/// Read a Chunk Data packet (play, clientbound, 0x21).
pub fn read_chunk_data(payload: &[u8]) -> Result<Option<Column>> {
    let mut r = Reader::new(payload);
    let x = r.i32()?;
    let z = r.i32()?;
    let ground_up = r.bool()?;
    let mask = r.u16()?;
    let size = usize::try_from(r.var_int()?).map_err(|_| Error::BadLength)?;
    let data = r.rest();
    if data.len() < size {
        return Err(Error::Incomplete);
    }
    // A full column with no sections is the server saying "forget this chunk".
    if ground_up && mask == 0 {
        return Ok(None);
    }
    // Sky light is only in the overworld, and the packet does not say which it is; the caller
    // knows the dimension, so assume the overworld and let it correct that later.
    column(x, z, mask, ground_up, true, &data[..size]).map(Some)
}

/// Read a Map Chunk Bulk packet (play, clientbound, 0x26).
///
/// This is how 1.8 sends the chunks around you when you join: every column's header first, then
/// every column's payload, back to back. Each one is a full column.
pub fn read_map_chunk_bulk(payload: &[u8]) -> Result<Vec<Column>> {
    let mut r = Reader::new(payload);
    let sky_light = r.bool()?;
    let count = usize::try_from(r.var_int()?).map_err(|_| Error::BadLength)?;
    // Guard a desynchronised stream before allocating from it.
    if count > 1024 {
        return Err(Error::BadLength);
    }
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        headers.push((r.i32()?, r.i32()?, r.u16()?));
    }
    let mut data = r.rest();
    let mut out = Vec::with_capacity(count);
    for (x, z, mask) in headers {
        let used = sent_bytes(mask, true, sky_light);
        if data.len() < used {
            return Err(Error::Incomplete);
        }
        out.push(column(x, z, mask, true, sky_light, &data[..used])?);
        data = &data[used..];
    }
    Ok(out)
}

/// How many bytes one column takes on the wire.
#[must_use]
pub fn sent_bytes(mask: u16, ground_up: bool, sky_light: bool) -> usize {
    let sections = mask.count_ones() as usize;
    sections * SECTION_BYTES
        + sections * LIGHT_BYTES
        + if sky_light { sections * LIGHT_BYTES } else { 0 }
        + if ground_up { BIOME_BYTES } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the payload a server would send for one section of a flat world.
    fn one_section(fill: u16) -> Vec<u8> {
        let mut data = Vec::new();
        for _ in 0..SECTION_BLOCKS {
            data.extend_from_slice(&fill.to_le_bytes());
        }
        data.extend(std::iter::repeat_n(0u8, LIGHT_BYTES * 2)); // block light + sky light
        data.extend(std::iter::repeat_n(1u8, BIOME_BYTES));
        data
    }

    #[test]
    fn a_single_section_decodes_to_the_blocks_that_were_sent() {
        // Stone (id 1) with metadata 0 is 1 << 4.
        let stone = 1u16 << 4;
        let c = column(3, -7, 0b1, true, true, &one_section(stone)).unwrap();
        assert_eq!((c.x, c.z), (3, -7));
        assert_eq!(
            c.solid_count(),
            SECTION_BLOCKS,
            "the whole section is stone"
        );
        assert_eq!(c.block_at(0, 0, 0) >> 4, 1, "block id 1");
        assert_eq!(c.block_at(15, 15, 15) >> 4, 1);
        assert_eq!(
            c.block_at(0, 16, 0),
            0,
            "the section above was not sent: air"
        );
        assert_eq!(c.biomes.as_ref().map(Vec::len), Some(BIOME_BYTES));
    }

    #[test]
    fn metadata_lives_in_the_low_four_bits() {
        // Wool (35) coloured 14 is 35 << 4 | 14.
        let red_wool = (35u16 << 4) | 14;
        let c = column(0, 0, 0b1, true, true, &one_section(red_wool)).unwrap();
        assert_eq!(c.block_at(1, 2, 3) >> 4, 35, "id");
        assert_eq!(c.block_at(1, 2, 3) & 0xF, 14, "metadata");
    }

    #[test]
    fn sections_land_where_the_mask_says() {
        // Only section 4 (y 64..79) was sent.
        let c = column(0, 0, 1 << 4, true, true, &one_section(1 << 4)).unwrap();
        assert!(c.sections[4].is_some());
        assert_eq!(c.sections.iter().flatten().count(), 1);
        assert_eq!(c.block_at(0, 64, 0) >> 4, 1, "the section starts at y=64");
        assert_eq!(c.block_at(0, 63, 0), 0, "below it is air");
    }

    #[test]
    fn a_short_payload_is_refused_rather_than_read_past() {
        let mut data = one_section(1 << 4);
        data.truncate(data.len() - 1);
        assert_eq!(
            column(0, 0, 0b1, true, true, &data).unwrap_err(),
            Error::Incomplete
        );
    }

    #[test]
    fn no_sky_light_means_a_smaller_payload() {
        // The Nether sends no sky light, so the same mask needs less data.
        let mut data = Vec::new();
        for _ in 0..SECTION_BLOCKS {
            data.extend_from_slice(&(1u16 << 4).to_le_bytes());
        }
        data.extend(std::iter::repeat_n(0u8, LIGHT_BYTES));
        let c = column(0, 0, 0b1, false, false, &data).unwrap();
        assert_eq!(c.solid_count(), SECTION_BLOCKS);
        assert!(c.biomes.is_none(), "no biomes without ground-up");
    }
}
