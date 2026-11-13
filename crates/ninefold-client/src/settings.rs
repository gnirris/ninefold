//! What the player has chosen, kept between runs.
//!
//! One file of `key=value` lines under the usual config directory. Not JSON and not TOML: there
//! are eight settings, a line each, and a format a person can fix in a text editor when a client
//! writes something stupid into it is worth more here than a parser.
//!
//! Anything missing or unreadable falls back to the default rather than refusing to start. A
//! settings file is not worth losing a game session over.

use std::fmt::Write as _;
use std::path::PathBuf;

/// Everything the menus can change.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The name shown to other players. On an offline-mode server this is simply whatever you
    /// say it is, which is the whole of what "cracked" means.
    pub username: String,
    /// Where to connect, as `host` or `host:port`.
    pub server: String,
    /// How far to draw, in chunks.
    pub render_distance: u8,
    /// Degrees of turn per pixel of mouse movement.
    pub sensitivity: f32,
    /// Vertical field of view, in degrees.
    pub fov: f32,
    /// How loud, 0 to 1.
    pub volume: f32,
    pub fullscreen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: "Player".to_owned(),
            server: "127.0.0.1:25565".to_owned(),
            render_distance: 8,
            sensitivity: 0.15,
            fov: 70.0,
            volume: 1.0,
            fullscreen: false,
        }
    }
}

impl Settings {
    /// Where the file lives.
    #[must_use]
    pub fn path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME").map_or_else(
            |_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
            },
            PathBuf::from,
        );
        base.join("ninefold").join("settings.txt")
    }

    /// Read what is there, falling back to the defaults for anything that is not.
    #[must_use]
    pub fn load() -> Self {
        let text = std::fs::read_to_string(Self::path()).unwrap_or_default();
        Self::parse(&text)
    }

    /// The same, from text that is already in hand.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "username" if !value.is_empty() => settings.username = value.to_owned(),
                "server" if !value.is_empty() => settings.server = value.to_owned(),
                "render-distance" => {
                    if let Ok(n) = value.parse() {
                        settings.render_distance = n;
                    }
                }
                "sensitivity" => {
                    if let Ok(n) = value.parse() {
                        settings.sensitivity = n;
                    }
                }
                "fov" => {
                    if let Ok(n) = value.parse() {
                        settings.fov = n;
                    }
                }
                "volume" => {
                    if let Ok(n) = value.parse() {
                        settings.volume = n;
                    }
                }
                "fullscreen" => settings.fullscreen = value == "true",
                _ => {}
            }
        }
        settings.tidy();
        settings
    }

    /// Keep every value somewhere it can actually be used.
    fn tidy(&mut self) {
        self.username = self
            .username
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(16)
            .collect();
        if self.username.is_empty() {
            self.username = "Player".to_owned();
        }
        self.render_distance = self.render_distance.clamp(2, 32);
        self.sensitivity = self.sensitivity.clamp(0.02, 1.0);
        self.fov = self.fov.clamp(30.0, 110.0);
        self.volume = self.volume.clamp(0.0, 1.0);
    }

    /// Write it back out, making the directory if it is not there.
    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = String::from("# ninefold settings\n");
        let _ = writeln!(out, "username={}", self.username);
        let _ = writeln!(out, "server={}", self.server);
        let _ = writeln!(out, "render-distance={}", self.render_distance);
        let _ = writeln!(out, "sensitivity={}", self.sensitivity);
        let _ = writeln!(out, "fov={}", self.fov);
        let _ = writeln!(out, "volume={}", self.volume);
        let _ = writeln!(out, "fullscreen={}", self.fullscreen);
        // A settings file that cannot be written is not worth interrupting anything over.
        let _ = std::fs::write(path, out);
    }

    /// The host and port to connect to, from whatever is in the server box.
    #[must_use]
    pub fn target(&self) -> (String, u16) {
        split_address(&self.server)
    }
}

/// `host`, `host:port`, or something a person typed in a hurry.
#[must_use]
pub fn split_address(address: &str) -> (String, u16) {
    let address = address.trim();
    match address.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            (host.to_owned(), port.trim().parse().unwrap_or(25565))
        }
        _ => (address.to_owned(), 25565),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let settings = Settings::parse("");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn rubbish_in_the_file_does_not_stop_the_rest_being_read() {
        let settings = Settings::parse(
            "# a comment\nnonsense\nfov=not a number\nusername=Stece\nvolume=0.5\n",
        );
        assert_eq!(settings.username, "Stece");
        assert!((settings.volume - 0.5).abs() < 1e-6);
        assert!((settings.fov - 70.0).abs() < 1e-6, "the bad line was skipped");
    }

    #[test]
    fn a_username_is_cleaned_up_rather_than_rejected() {
        // Minecraft names are letters, numbers and underscores, up to sixteen.
        assert_eq!(Settings::parse("username=Ste ce!").username, "Stece");
        assert_eq!(
            Settings::parse("username=aaaaaaaaaaaaaaaaaaaaaaaa").username.len(),
            16
        );
        assert_eq!(
            Settings::parse("username=!!!").username,
            "Player",
            "a name with nothing usable in it falls back"
        );
    }

    #[test]
    fn values_are_kept_inside_what_can_be_used() {
        let settings = Settings::parse("render-distance=200\nfov=5\nsensitivity=-3\nvolume=9");
        assert_eq!(settings.render_distance, 32);
        assert!((settings.fov - 30.0).abs() < 1e-6);
        assert!(settings.sensitivity > 0.0);
        assert!((settings.volume - 1.0).abs() < 1e-6);
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let settings = Settings {
            username: "Stece".to_owned(),
            server: "play.example.net:25566".to_owned(),
            fov: 90.0,
            fullscreen: true,
            ..Settings::default()
        };
        let mut out = String::from("# ninefold settings\n");
        let _ = writeln!(out, "username={}", settings.username);
        let _ = writeln!(out, "server={}", settings.server);
        let _ = writeln!(out, "render-distance={}", settings.render_distance);
        let _ = writeln!(out, "sensitivity={}", settings.sensitivity);
        let _ = writeln!(out, "fov={}", settings.fov);
        let _ = writeln!(out, "volume={}", settings.volume);
        let _ = writeln!(out, "fullscreen={}", settings.fullscreen);
        assert_eq!(Settings::parse(&out), settings);
    }

    #[test]
    fn an_address_can_be_written_the_way_people_write_them() {
        assert_eq!(
            split_address("play.example.net"),
            ("play.example.net".to_owned(), 25565)
        );
        assert_eq!(
            split_address("play.example.net:25566"),
            ("play.example.net".to_owned(), 25566)
        );
        assert_eq!(
            split_address(" 127.0.0.1:25599 "),
            ("127.0.0.1".to_owned(), 25599)
        );
        // A port nobody could parse is the default one, not a crash.
        assert_eq!(
            split_address("example.net:what"),
            ("example.net".to_owned(), 25565)
        );
    }
}
