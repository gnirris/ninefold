//! An item stack, as 1.8 sends it.
//!
//! Short id, byte count, short damage, optional NBT. An id of -1 means the slot is empty, and
//! that is the whole of it — there is no component data, no registry name, nothing a modern
//! protocol would put here.
//!
//! For ids below 256 the id *is* a block id, which is what makes placing blocks possible without
//! a separate item registry: hold id 4, right click, and the server puts cobblestone down.

use crate::{Reader, Result, Writer, nbt};

/// One stack in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub id: i16,
    pub count: u8,
    pub damage: u16,
}

impl Slot {
    #[must_use]
    pub const fn new(id: i16, count: u8) -> Self {
        Self {
            id,
            count,
            damage: 0,
        }
    }

    /// The block this stack would place, or `None` if it is not a block.
    ///
    /// Items are numbered from 256 up, so anything below that is a block id, and the damage
    /// value doubles as the metadata the placed block gets.
    #[must_use]
    pub const fn as_block(self) -> Option<u16> {
        if self.id > 0 && self.id < 256 {
            Some(((self.id as u16) << 4) | (self.damage & 0xF))
        } else {
            None
        }
    }
}

/// Read a slot. `None` is an empty slot, not a failure.
pub fn read(r: &mut Reader) -> Result<Option<Slot>> {
    let id = r.i16()?;
    if id < 0 {
        return Ok(None);
    }
    let count = r.u8()?;
    let damage = r.u16()?;
    nbt::skip_optional(r)?;
    Ok(Some(Slot { id, count, damage }))
}

/// Write a slot back, in the same shape.
pub fn write(w: &mut Writer, slot: Option<Slot>) {
    match slot {
        None => {
            w.i16(-1);
        }
        Some(slot) => {
            w.i16(slot.id).u8(slot.count).u16(slot.damage).u8(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_slot_is_two_bytes() {
        let mut w = Writer::new();
        write(&mut w, None);
        let bytes = w.finish();
        assert_eq!(bytes, vec![0xFF, 0xFF]);
        assert_eq!(read(&mut Reader::new(&bytes)).unwrap(), None);
    }

    #[test]
    fn a_stack_survives_a_round_trip() {
        let stack = Slot {
            id: 278,
            count: 1,
            damage: 37,
        };
        let mut w = Writer::new();
        write(&mut w, Some(stack));
        let bytes = w.finish();
        assert_eq!(read(&mut Reader::new(&bytes)).unwrap(), Some(stack));
    }

    #[test]
    fn slots_can_be_read_one_after_another() {
        // Which is the whole reason the NBT has to be walked rather than guessed at.
        let mut w = Writer::new();
        write(&mut w, Some(Slot::new(1, 64)));
        write(&mut w, None);
        write(&mut w, Some(Slot::new(35, 3)));
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(read(&mut r).unwrap(), Some(Slot::new(1, 64)));
        assert_eq!(read(&mut r).unwrap(), None);
        assert_eq!(read(&mut r).unwrap(), Some(Slot::new(35, 3)));
    }

    #[test]
    fn blocks_are_the_items_below_two_hundred_and_fifty_six() {
        assert_eq!(Slot::new(4, 1).as_block(), Some(4 << 4));
        assert_eq!(
            Slot {
                id: 35,
                count: 1,
                damage: 14
            }
            .as_block(),
            Some((35 << 4) | 14),
            "red wool keeps its colour"
        );
        assert_eq!(Slot::new(278, 1).as_block(), None, "a pickaxe is not a block");
        assert_eq!(Slot::new(0, 1).as_block(), None, "air is not placeable");
    }
}
