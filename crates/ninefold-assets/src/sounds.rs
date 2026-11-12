//! Where the noises come from.
//!
//! 1.8 does not keep its sounds in the version jar. They live in the launcher's asset store: an
//! index JSON mapping `minecraft/sounds/dig/stone1.ogg` to a SHA-1, and a pile of files named
//! after their hash. Nothing is copied out of it, exactly as with the textures — the player's own
//! install is read and the samples go straight to the sound card.
//!
//! A server asks for sounds by event name, `random.pop` or `mob.pig.say`, and vanilla resolves
//! that through `sounds.json` to one of several numbered variants. That file is not in the store,
//! so the mapping here is the obvious one: dots become slashes, and a name with no file of its
//! own is tried with `1`, `2`, `3` … after it, which is how every variant set in 1.8 is named.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// An opened asset store.
pub struct Sounds {
    root: PathBuf,
    /// Asset path to hash, for everything under `minecraft/sounds`.
    index: HashMap<String, String>,
}

impl Sounds {
    /// Read an index out of an asset store directory.
    ///
    /// # Errors
    /// If no index can be read or parsed.
    pub fn open(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let indexes = root.join("indexes");
        // Newest index wins, and 1.8's is called `1.8.json`.
        let mut names: Vec<PathBuf> = std::fs::read_dir(&indexes)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        names.sort();
        let preferred = names
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n == "1.8.json"))
            .cloned()
            .or_else(|| names.pop())
            .ok_or_else(|| std::io::Error::other("no asset index"))?;

        let text = std::fs::read_to_string(&preferred)?;
        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(std::io::Error::other)?;
        let mut index = HashMap::new();
        if let Some(objects) = parsed.get("objects").and_then(|o| o.as_object()) {
            for (path, entry) in objects {
                if !path.starts_with("minecraft/sounds/") {
                    continue;
                }
                if let Some(hash) = entry.get("hash").and_then(|h| h.as_str()) {
                    index.insert(path.clone(), hash.to_owned());
                }
            }
        }
        Ok(Self { root, index })
    }

    /// Where launchers usually keep an asset store.
    #[must_use]
    pub fn usual_places() -> Vec<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_default();
        [
            ".local/share/ArcticLauncher/shared/assets",
            ".minecraft/assets",
            ".local/share/PrismLauncher/assets",
        ]
        .iter()
        .map(|tail| Path::new(&home).join(tail))
        .filter(|p| p.join("indexes").is_dir())
        .collect()
    }

    /// How many sounds this store has.
    #[must_use]
    pub fn count(&self) -> usize {
        self.index.len()
    }

    /// The encoded bytes for one sound event, picking between variants with `pick`.
    ///
    /// `pick` is any number; it is taken modulo the number of variants, so a caller can pass a
    /// counter or anything random-ish without caring how many there are.
    #[must_use]
    pub fn event(&self, name: &str, pick: usize) -> Option<Vec<u8>> {
        let stem = format!("minecraft/sounds/{}", name.replace('.', "/"));
        // An event with a file of its own and no variants.
        let mut options: Vec<&String> = Vec::new();
        let exact = format!("{stem}.ogg");
        if let Some(hash) = self.index.get(&exact) {
            options.push(hash);
        }
        // Variants are the same name with a number on the end.
        for n in 1..=16 {
            if let Some(hash) = self.index.get(&format!("{stem}{n}.ogg")) {
                options.push(hash);
            }
        }
        let hash = options.get(pick % options.len().max(1))?;
        std::fs::read(self.path_of(hash)).ok()
    }

    /// Where one object's file is: two characters of the hash, then the hash.
    fn path_of(&self, hash: &str) -> PathBuf {
        self.root
            .join("objects")
            .join(&hash[..2.min(hash.len())])
            .join(hash)
    }
}

/// Open an asset store if one can be found.
#[must_use]
pub fn find() -> Option<Sounds> {
    Sounds::usual_places()
        .into_iter()
        .find_map(|root| Sounds::open(root).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_that_is_not_there_is_none_rather_than_a_panic() {
        assert!(Sounds::open("/nowhere/at/all").is_err());
    }

    /// Reads the player's actual asset store. Ignored by default, like the jar test.
    #[test]
    #[ignore = "needs a real 1.8 install"]
    fn the_real_store_has_the_sounds_the_game_makes() {
        let sounds = find().expect("an asset store");
        assert!(sounds.count() > 100, "{} sounds", sounds.count());
        let stone = sounds.event("dig.stone", 0).expect("dig.stone");
        assert_eq!(&stone[..4], b"OggS", "sounds are ogg files");
        // Variants differ: asking twice with different picks gives different files.
        let a = sounds.event("step.grass", 0).expect("step.grass 1");
        let b = sounds.event("step.grass", 1).expect("step.grass 2");
        assert_ne!(a, b);
        assert!(sounds.event("not.a.real.sound", 0).is_none());
    }
}
