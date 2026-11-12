//! Hearing the world.
//!
//! Three jobs: decide which of 1.8's sound files an event means, place it in the world relative
//! to the player's ears, and keep the decoded clips so a footstep does not read a file off the
//! disk twice.
//!
//! What a block sounds like is a property of the block, and vanilla groups them: stone for
//! everything mineral, wood for everything from a tree, gravel for dirt, cloth for wool, and so
//! on. The groups below are those groups.

use std::collections::HashMap;

use ninefold_assets::sounds::Sounds;
use ninefold_sound::{Clip, Mixer};

/// Past this many blocks away, nothing is heard at all. Vanilla's own cutoff is sixteen.
const EARSHOT: f64 = 16.0;

/// How far you walk between footsteps. Vanilla counts distance travelled and steps each time it
/// passes a whole unit of `distance * 0.6`, which comes to a step every one and two thirds blocks.
const STRIDE: f64 = 1.0 / 0.6;

/// Which family of noises a block belongs to.
#[must_use]
pub fn footing(block: u16) -> &'static str {
    match block >> 4 {
        // Grass, leaves and everything that grows.
        2 | 6 | 18 | 31 | 32 | 37 | 38 | 39 | 40 | 59 | 83 | 86 | 104 | 105 | 106 | 111 | 141
        | 142 | 161 | 175 => "grass",
        // Wood: planks, logs, fences, chests, workbenches.
        5 | 17 | 47 | 53 | 54 | 58 | 63 | 64 | 65 | 69 | 72 | 84 | 85 | 96 | 107 | 125 | 126
        | 134 | 135 | 136 | 146 | 162 | 163 | 164 | 183..=187 => "wood",
        // Dirt and gravel sound the same, and are the most common thing underfoot.
        3 | 13 | 60 | 110 => "gravel",
        12 | 24 | 88 | 179 => "sand",
        78 | 80 => "snow",
        35 | 26 | 171 => "cloth",
        // Everything mineral, and anything this does not recognise: stone is the default in
        // vanilla too.
        _ => "stone",
    }
}

/// A block's own break sound, where it is not simply its footing.
fn break_sound(block: u16) -> String {
    match block >> 4 {
        // Glass shatters; it does not crunch.
        20 | 95 | 102 => "random.glass".to_owned(),
        _ => format!("dig.{}", footing(block)),
    }
}

/// The sound card, the asset store and whatever has been decoded so far.
pub struct Audio {
    mixer: Option<Mixer>,
    store: Option<Sounds>,
    clips: HashMap<String, Option<Clip>>,
    /// Which variant to take next, so repeated sounds are not identical.
    pick: usize,
    /// How far the player has walked since the last footstep.
    walked: f64,
}

impl Audio {
    /// Open whatever can be opened. A machine with no sound card, or a copy of the game with no
    /// asset store, gets a silent game rather than no game.
    #[must_use]
    pub fn open() -> Self {
        Self {
            mixer: Mixer::open(),
            store: ninefold_assets::sounds::find(),
            clips: HashMap::new(),
            pick: 0,
            walked: 0.0,
        }
    }

    /// Is there anything to hear?
    #[must_use]
    pub fn working(&self) -> bool {
        self.mixer.is_some() && self.store.is_some()
    }

    /// What to say about the sound in the readout.
    #[must_use]
    pub fn describe(&self) -> String {
        match (&self.mixer, &self.store) {
            (Some(mixer), Some(store)) => {
                format!("{} sounds, {} playing", store.count(), mixer.playing())
            }
            (None, _) => "no sound card".to_owned(),
            (_, None) => "no asset store".to_owned(),
        }
    }

    /// Decode a sound, or hand back the one already decoded.
    fn clip(&mut self, name: &str, pick: usize) -> Option<&Clip> {
        let key = format!("{name}#{pick}");
        if !self.clips.contains_key(&key) {
            let decoded = self
                .store
                .as_ref()
                .and_then(|store| store.event(name, pick))
                .and_then(|bytes| ninefold_sound::decode(&bytes));
            self.clips.insert(key.clone(), decoded);
        }
        self.clips.get(&key)?.as_ref()
    }

    /// Play something right by the player: an interface click, their own footstep.
    pub fn here(&mut self, name: &str, volume: f32, pitch: f32) {
        self.pick = self.pick.wrapping_add(1);
        let pick = self.pick;
        let Some(clip) = self.clip(name, pick).cloned() else {
            return;
        };
        if let Some(mixer) = &self.mixer {
            mixer.play(&clip, volume, pitch, 0.0);
        }
    }

    /// Play something at a place in the world, heard from `ear` looking along `facing`.
    pub fn at(&mut self, name: &str, where_how: (&[f64; 3], f32, f32), ears: (&[f64; 3], f32)) {
        let (at, volume, pitch) = where_how;
        let (ear, yaw) = ears;
        let (dx, dy, dz) = (at[0] - ear[0], at[1] - ear[1], at[2] - ear[2]);
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();
        // A loud sound carries further, which is what the volume means on the wire.
        let reach = EARSHOT * f64::from(volume.max(1.0));
        if distance > reach {
            return;
        }
        let near = (1.0 - distance / reach).clamp(0.0, 1.0) as f32;
        self.pick = self.pick.wrapping_add(1);
        let pick = self.pick;
        let Some(clip) = self.clip(name, pick).cloned() else {
            return;
        };
        // Which ear it reaches first: the component of the direction along the player's right.
        let turn = f64::from(yaw).to_radians();
        let (right_x, right_z) = (turn.cos(), turn.sin());
        let pan = if distance > 0.01 {
            ((dx * right_x + dz * right_z) / distance) as f32
        } else {
            0.0
        };
        if let Some(mixer) = &self.mixer {
            mixer.play(&clip, volume.min(1.0) * near, pitch, pan);
        }
    }

    /// Count a tick of walking, and make a noise when a stride has gone by.
    pub fn walking(&mut self, moved: f64, standing_on: u16, in_air: bool) {
        if in_air || moved <= 0.0 {
            return;
        }
        self.walked += moved;
        if self.walked < STRIDE {
            return;
        }
        self.walked -= STRIDE;
        // Air underfoot means there is nothing to make a noise against.
        if standing_on >> 4 == 0 {
            return;
        }
        let name = format!("step.{}", footing(standing_on));
        // Vanilla's own numbers for a player's own footstep: quiet, and at the block's pitch.
        self.here(&name, 0.15, 1.0);
    }

    /// Breaking a block, and the little tap while breaking it.
    pub fn broke(&mut self, block: u16, at: [f64; 3], ear: [f64; 3], yaw: f32) {
        let name = break_sound(block);
        self.at(&name, (&at, 0.6, 0.85), (&ear, yaw));
    }

    /// Putting one down, which is the same sound a touch higher.
    pub fn placed(&mut self, block: u16, at: [f64; 3], ear: [f64; 3], yaw: f32) {
        let name = format!("dig.{}", footing(block));
        self.at(&name, (&at, 0.5, 0.95), (&ear, yaw));
    }

    /// The tap of a tool on a block that is still being dug.
    pub fn digging(&mut self, block: u16) {
        let name = format!("dig.{}", footing(block));
        self.here(&name, 0.08, 0.5);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_sound_like_what_they_are_made_of() {
        assert_eq!(footing(2 << 4), "grass", "a grass block");
        assert_eq!(footing(18 << 4), "grass", "leaves");
        assert_eq!(footing(5 << 4), "wood", "planks");
        assert_eq!(footing(17 << 4), "wood", "a log");
        assert_eq!(footing(3 << 4), "gravel", "dirt");
        assert_eq!(footing(12 << 4), "sand");
        assert_eq!(footing(35 << 4), "cloth", "wool");
        assert_eq!(footing(78 << 4), "snow");
        assert_eq!(footing(1 << 4), "stone");
        assert_eq!(footing(49 << 4), "stone", "obsidian");
        assert_eq!(footing(9999 << 4), "stone", "and anything unknown");
    }

    #[test]
    fn metadata_does_not_change_what_a_block_sounds_like() {
        for meta in 0..16u16 {
            assert_eq!(footing((5 << 4) | meta), "wood", "planks of any wood");
            assert_eq!(footing((35 << 4) | meta), "cloth", "wool of any colour");
        }
    }

    #[test]
    fn glass_shatters_and_stone_does_not() {
        assert_eq!(break_sound(20 << 4), "random.glass");
        assert_eq!(break_sound(1 << 4), "dig.stone");
        assert_eq!(break_sound(5 << 4), "dig.wood");
    }

    #[test]
    fn a_stride_is_the_distance_vanilla_uses() {
        // Vanilla steps each time 0.6 times the distance walked passes a whole number.
        assert!((STRIDE - 1.666_666).abs() < 1e-5);
    }

    /// The whole thing with no sound card and no assets, which is how it runs on a server or in
    /// a test: every call has to be a no-op rather than a panic.
    #[test]
    fn silence_is_not_a_failure() {
        let mut audio = Audio {
            mixer: None,
            store: None,
            clips: HashMap::new(),
            pick: 0,
            walked: 0.0,
        };
        assert!(!audio.working());
        audio.here("random.pop", 1.0, 1.0);
        audio.at("mob.pig.say", (&[0.0; 3], 1.0, 1.0), (&[0.0; 3], 0.0));
        audio.walking(10.0, 2 << 4, false);
        audio.broke(1 << 4, [0.0; 3], [0.0; 3], 0.0);
        audio.placed(1 << 4, [0.0; 3], [0.0; 3], 0.0);
        audio.digging(1 << 4);
        assert_eq!(audio.describe(), "no sound card");
    }

    #[test]
    fn walking_only_counts_on_the_ground_and_only_on_something() {
        let mut audio = Audio {
            mixer: None,
            store: None,
            clips: HashMap::new(),
            pick: 0,
            walked: 0.0,
        };
        audio.walking(5.0, 1 << 4, true);
        assert_eq!(audio.walked, 0.0, "nothing underfoot in mid air");
        audio.walking(1.0, 1 << 4, false);
        assert!(audio.walked > 0.0);
        // A full stride is taken off rather than reset, so the cadence does not drift.
        audio.walking(1.0, 1 << 4, false);
        assert!(audio.walked < STRIDE);
    }
}
