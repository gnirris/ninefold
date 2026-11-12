//! Where the pictures come from.
//!
//! ninefold ships no Minecraft assets and never will: the engine can be rewritten, the artwork
//! cannot be redistributed. So nothing in the client opens a jar directly. It asks for
//! `blocks/grass_top` and something else decides where that comes from:
//!
//! - [`Vanilla`] reads the player's own copy — the real textures, nothing redistributed.
//! - [`Fallback`] is ninefold's own art, so the client still runs for someone who has no copy.
//!
//! [`pick`] chooses between them, which is the whole plug-and-play story: installed, you get the
//! game's look; not installed, you still get a game.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Decoded pixels, ready for the GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// Four bytes a pixel, row by row.
    pub rgba: Vec<u8>,
}

impl Texture {
    /// A single colour, for the fallback pack and for anything missing.
    #[must_use]
    pub fn flat(width: u32, height: u32, colour: [u8; 4]) -> Self {
        Self {
            width,
            height,
            rgba: colour.repeat((width * height) as usize),
        }
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        let at = ((y * self.width + x) * 4) as usize;
        Some([
            *self.rgba.get(at)?,
            *self.rgba.get(at + 1)?,
            *self.rgba.get(at + 2)?,
            *self.rgba.get(at + 3)?,
        ])
    }
}

/// Somewhere textures can be read from.
pub trait AssetSource: Send + Sync {
    /// A texture by its name without extension, e.g. `blocks/grass_top`.
    fn texture(&self, id: &str) -> Option<Texture>;

    /// What to say in the UI about where the art is coming from.
    fn describe(&self) -> String;

    /// Whether this is the real game's artwork.
    fn is_vanilla(&self) -> bool {
        false
    }
}

/// The player's own copy of the game.
///
/// 1.8.x keeps textures inside the version jar, unlike later versions which moved most of them
/// into the asset objects store. Nothing is copied out of it: it is read and handed to the GPU.
pub struct Vanilla {
    jar: PathBuf,
    // A jar is a zip, and reopening it per texture would be slow; one handle, locked.
    archive: Mutex<zip::ZipArchive<File>>,
}

impl Vanilla {
    /// Open a version jar. Fails if it is missing or not a zip.
    pub fn open(jar: impl AsRef<Path>) -> std::io::Result<Self> {
        let jar = jar.as_ref().to_path_buf();
        let archive = zip::ZipArchive::new(File::open(&jar)?)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(Self {
            jar,
            archive: Mutex::new(archive),
        })
    }

    /// Where Minecraft launchers usually put a 1.8.9 jar, newest first.
    #[must_use]
    pub fn usual_places() -> Vec<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_default();
        [
            ".local/share/ArcticLauncher/shared/versions/1.8.9/1.8.9.jar",
            ".minecraft/versions/1.8.9/1.8.9.jar",
            ".local/share/PrismLauncher/libraries/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar",
        ]
        .iter()
        .map(|tail| Path::new(&home).join(tail))
        .filter(|p| p.is_file())
        .collect()
    }
}

impl AssetSource for Vanilla {
    fn texture(&self, id: &str) -> Option<Texture> {
        let path = format!("assets/minecraft/textures/{id}.png");
        let mut bytes = Vec::new();
        {
            let mut archive = self.archive.lock().ok()?;
            let mut entry = archive.by_name(&path).ok()?;
            entry.read_to_end(&mut bytes).ok()?;
        }
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
        let rgba = decoded.to_rgba8();
        Some(Texture {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        })
    }

    fn describe(&self) -> String {
        format!("your own copy of the game ({})", self.jar.display())
    }

    fn is_vanilla(&self) -> bool {
        true
    }
}

/// ninefold's own art, so the client runs for someone with no copy of Minecraft.
///
/// Placeholder colours for now, picked from the texture's name so the same block is always the
/// same colour and a world is at least readable. Real art replaces this without anything above
/// it changing.
pub struct Fallback;

impl AssetSource for Fallback {
    fn texture(&self, id: &str) -> Option<Texture> {
        Some(Texture::flat(16, 16, colour_for(id)))
    }

    fn describe(&self) -> String {
        "ninefold's own placeholder art (no copy of Minecraft found)".to_owned()
    }
}

/// A stable colour per texture name: the same block is the same colour every run, and two
/// different blocks are very unlikely to collide.
fn colour_for(id: &str) -> [u8; 4] {
    // FNV-1a, so this needs no dependency and never changes between runs.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Keep it out of the very dark and very bright ends, where everything looks the same.
    let channel = |shift: u32| 60 + ((hash >> shift) & 0xFF) as u8 / 2;
    [channel(0), channel(16), channel(32), 255]
}

/// Use the real artwork if a jar is there, and ninefold's own if not.
#[must_use]
pub fn pick(jar: Option<&Path>) -> Box<dyn AssetSource> {
    let found = jar
        .map(Path::to_path_buf)
        .or_else(|| Vanilla::usual_places().into_iter().next());
    match found.as_deref().map(Vanilla::open) {
        Some(Ok(vanilla)) => Box::new(vanilla),
        Some(Err(_)) | None => Box::new(Fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_always_has_something_to_draw() {
        let pack = Fallback;
        let texture = pack
            .texture("blocks/anything_at_all")
            .expect("never missing");
        assert_eq!((texture.width, texture.height), (16, 16));
        assert_eq!(texture.rgba.len(), 16 * 16 * 4);
        assert!(!pack.is_vanilla());
    }

    #[test]
    fn fallback_colours_are_stable_and_distinct() {
        assert_eq!(
            colour_for("blocks/grass_top"),
            colour_for("blocks/grass_top")
        );
        assert_ne!(colour_for("blocks/grass_top"), colour_for("blocks/dirt"));
        assert_ne!(colour_for("blocks/stone"), colour_for("blocks/bedrock"));
        // Fully opaque, and never nearly-black.
        let [r, g, b, a] = colour_for("blocks/stone");
        assert_eq!(a, 255);
        assert!(r >= 60 && g >= 60 && b >= 60);
    }

    #[test]
    fn a_jar_that_is_not_there_falls_back_rather_than_failing() {
        let source = pick(Some(Path::new("/nowhere/1.8.9.jar")));
        assert!(!source.is_vanilla());
        assert!(source.texture("blocks/stone").is_some());
    }

    /// Reads the player's actual 1.8.9 jar. Ignored by default because it needs the game
    /// installed; run with `cargo test -- --ignored` on a machine that has it.
    #[test]
    #[ignore = "needs a real 1.8.9 install"]
    fn the_real_jar_gives_real_textures() {
        let Some(jar) = Vanilla::usual_places().into_iter().next() else {
            panic!("no 1.8.9 jar in the usual places");
        };
        let vanilla = Vanilla::open(&jar).expect("open the jar");
        assert!(vanilla.is_vanilla());

        let grass = vanilla.texture("blocks/grass_top").expect("grass_top");
        assert_eq!(
            (grass.width, grass.height),
            (16, 16),
            "1.8 textures are 16x16"
        );
        // Grass top is greyscale in the file and tinted by biome at draw time, so the only safe
        // claim is that it is opaque and not empty.
        assert!(grass.rgba.iter().any(|b| *b != 0));
        assert_eq!(grass.pixel(0, 0).map(|p| p[3]), Some(255));

        assert!(vanilla.texture("blocks/bedrock").is_some());
        assert!(
            vanilla.texture("blocks/not_a_real_block").is_none(),
            "a missing texture is None, not a panic"
        );
    }
}
