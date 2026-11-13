//! A block's geometry, ready to be stamped into a chunk.
//!
//! The model files say what a block is made of in pixels, with texture names and property
//! strings. None of that belongs in the inner loop of a chunk mesher, which runs over four
//! thousand blocks a section and cares about exactly three things per face: where its corners
//! are, where it samples, and what would hide it.
//!
//! So a shape is *baked* once per distinct block state — corners in block units, texture
//! coordinates already resolved against the atlas — and then only translated and culled.

use crate::Face;

/// One face of one box, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    /// Corners in block units, bottom left first as seen from outside.
    pub corners: [[f32; 3]; 4],
    /// Where each corner samples in the atlas, in the same order.
    pub uv: [[f32; 2]; 4],
    pub normal: [f32; 3],
    /// The neighbour that would hide this face. A face with no cullface is drawn whatever is
    /// next to it, which is why a fence post still has sides inside its own block.
    pub cull: Option<Face>,
    /// 0 for no colouring, otherwise which of the block's colour maps applies.
    pub tint: u8,
    /// Whether the face is darkened by which way it points. Torches are not.
    pub shade: bool,
    /// Whether this face sits on the edge of its block. One that does takes its light from the
    /// neighbour it faces; one inside the block takes the block's own.
    pub boundary: bool,
}

/// Everything one block state is made of.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Baked {
    pub quads: Vec<Quad>,
    /// Whether this fills its cube, and so hides the faces of whatever is pressed against it.
    pub solid: bool,
    /// The boxes a player collides with, in block units. Empty means you walk through it.
    pub collision: Vec<([f32; 3], [f32; 3])>,
    pub ambient_occlusion: bool,
}

impl Baked {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    /// How tall the tallest part of this block is, in block units.
    ///
    /// What a player standing on it stands at — three quarters of a block on a bed, one eighth
    /// on a single layer of snow.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.collision
            .iter()
            .map(|(_, to)| to[1])
            .fold(0.0, f32::max)
    }
}
