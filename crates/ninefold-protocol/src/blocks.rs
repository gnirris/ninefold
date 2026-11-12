//! What a block *is*, as against what it looks like.
//!
//! The renderer needs textures; the physics needs to know what you can stand on and what you
//! sink into. Those are different questions about the same id, and this answers the second.

/// What the player is standing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fluid {
    None,
    Water,
    Lava,
}

/// Blocks you walk through: air, fluids, and the growing things.
const PASSABLE: [u16; 24] = [
    0, 6, 8, 9, 10, 11, 27, 28, 30, 31, 32, 37, 38, 39, 40, 50, 55, 59, 63, 66, 68, 76, 77, 175,
];

/// Can you walk through it?
#[must_use]
pub fn passable(block: u16) -> bool {
    PASSABLE.contains(&(block >> 4))
}

/// Can you stand on it?
#[must_use]
pub fn solid(block: u16) -> bool {
    !passable(block)
}

/// Is it a liquid, and which?
#[must_use]
pub fn fluid(block: u16) -> Fluid {
    match block >> 4 {
        8 | 9 => Fluid::Water,
        10 | 11 => Fluid::Lava,
        _ => Fluid::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn you_can_walk_through_water_and_stand_on_stone() {
        assert!(passable(8 << 4), "water");
        assert!(passable(9 << 4), "still water");
        assert!(passable(31 << 4), "tall grass");
        assert!(solid(1 << 4), "stone");
        assert!(solid(2 << 4), "grass block");
        assert!(solid(17 << 4), "a log");
    }

    #[test]
    fn fluids_are_told_apart() {
        assert_eq!(fluid(8 << 4), Fluid::Water);
        assert_eq!(fluid(10 << 4), Fluid::Lava);
        assert_eq!(fluid(11 << 4), Fluid::Lava);
        assert_eq!(fluid(1 << 4), Fluid::None);
        assert_eq!(fluid(0), Fluid::None);
    }

    #[test]
    fn metadata_does_not_change_what_a_block_is() {
        // Flowing water carries its level in the metadata and is still water.
        for level in 0..8u16 {
            assert_eq!(fluid((8 << 4) | level), Fluid::Water);
            assert!(passable((8 << 4) | level));
        }
    }
}
