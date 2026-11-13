//! Mods, for a client with no JVM to load them into.
//!
//! Java mods cannot work here and never will — that is a consequence of the rewrite, not a gap.
//! So this is the other kind of modding API: small scripts the client runs itself, with hooks
//! for the events a 1.8 client has and a few functions to act through. It covers what people
//! actually write client mods for — a readout, a keybind, a toggle, something that watches the
//! chat — without letting a script reach the filesystem or the network.
//!
//! A mod is one `.rhai` file in the mods directory. Any of these functions it defines get
//! called:
//!
//! ```text
//! fn on_load()                     once, when the mod is read
//! fn on_tick(game)                 twenty times a second, with the player's state
//! fn on_chat(line)                 whenever something is said
//! fn on_key(key)                   when a key is pressed, as "F", "G", …
//! fn on_block(x, y, z, block)      when a block changes
//! ```
//!
//! and it can call `hud(text)` to add a line to the screen, `say(text)` to send chat, and
//! `log(text)` to write to the console. `game` is a map of `x`, `y`, `z`, `yaw`, `pitch`,
//! `health`, `food`, `held`, `fps`, `on_ground` and `day_time`.
//!
//! The sandbox is rhai's own: no file access, no sockets, no process spawning, because none of
//! that is registered. A runaway loop is caught by an operation limit rather than hanging the
//! game, and a script that throws is reported once and then left alone.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rhai::{AST, Dynamic, Engine, Map, Scope};

/// How many operations one hook may use before it is stopped.
///
/// A script runs on the frame thread, so an accidental `while true {}` would freeze the game.
/// This is generous for anything sane and instant for anything that is not.
const OPERATION_LIMIT: u64 = 200_000;

/// What the player is doing, handed to `on_tick`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub health: f32,
    pub food: u8,
    /// The hotbar slot in hand, 0 to 8.
    pub held: u8,
    pub fps: f64,
    pub on_ground: bool,
    pub day_time: i64,
}

impl Context {
    fn as_map(self) -> Map {
        let mut map = Map::new();
        map.insert("x".into(), Dynamic::from_float(self.x));
        map.insert("y".into(), Dynamic::from_float(self.y));
        map.insert("z".into(), Dynamic::from_float(self.z));
        map.insert("yaw".into(), Dynamic::from_float(self.yaw));
        map.insert("pitch".into(), Dynamic::from_float(self.pitch));
        map.insert("health".into(), Dynamic::from_float(f64::from(self.health)));
        map.insert("food".into(), Dynamic::from_int(i64::from(self.food)));
        map.insert("held".into(), Dynamic::from_int(i64::from(self.held)));
        map.insert("fps".into(), Dynamic::from_float(self.fps));
        map.insert("on_ground".into(), Dynamic::from_bool(self.on_ground));
        map.insert("day_time".into(), Dynamic::from_int(self.day_time));
        map
    }
}

/// What the scripts have asked the client to do since it last looked.
#[derive(Debug, Default)]
pub struct Bus {
    /// Lines to draw on the screen, in the order they were added.
    pub hud: Vec<String>,
    /// Things to say in chat.
    pub say: Vec<String>,
}

/// Everything the registered functions reach, behind one lock.
///
/// `state` exists because rhai functions are pure: a script cannot see a variable declared at
/// the top of its own file, so a mod that wants to remember something between ticks — a
/// counter, a toggle — needs somewhere to put it. Each mod gets its own drawer, keyed by the
/// one that is currently running, so two mods cannot tread on each other's keys.
#[derive(Default)]
struct Shared {
    bus: Bus,
    state: Vec<HashMap<String, Dynamic>>,
    running: usize,
}

/// One script that has been read and compiled.
struct Loaded {
    name: String,
    ast: AST,
    scope: Scope<'static>,
    /// Which hooks this mod actually defines.
    ///
    /// Without this, a script calling a function that does not exist looks exactly like a mod
    /// that simply has no `on_tick` — both are "function not found" — and a typo would be
    /// silently ignored for ever.
    hooks: HashSet<String>,
    /// Set once a hook has failed, so a broken mod complains once rather than every tick.
    broken: bool,
}

/// Every mod that was found, and the engine they run in.
pub struct Mods {
    engine: Engine,
    loaded: Vec<Loaded>,
    shared: Arc<Mutex<Shared>>,
    /// Where they were read from, for the readout.
    pub directory: PathBuf,
}

impl Mods {
    /// Where mods live.
    #[must_use]
    pub fn directory() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME").map_or_else(
            |_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
            PathBuf::from,
        );
        base.join("ninefold").join("mods")
    }

    /// Read and compile every `.rhai` file in the mods directory.
    ///
    /// A mod that will not compile is reported and skipped: one broken script must not stop the
    /// others, and must certainly not stop the game.
    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&Self::directory())
    }

    /// The same, from a directory given by name.
    #[must_use]
    pub fn load_from(directory: &Path) -> Self {
        let shared: Arc<Mutex<Shared>> = Arc::new(Mutex::new(Shared::default()));
        let mut engine = Engine::new();
        engine.set_max_operations(OPERATION_LIMIT);
        // Deep recursion in a script is a stack overflow in the game.
        engine.set_max_call_levels(32);
        engine.set_max_string_size(8 * 1024);
        engine.set_max_array_size(4096);

        let hud = shared.clone();
        engine.register_fn("hud", move |text: &str| {
            if let Ok(mut shared) = hud.lock() {
                // A mod that adds a line every tick and never stops would otherwise grow for
                // ever; the client clears this each frame, so this is only a guard.
                if shared.bus.hud.len() < 64 {
                    shared.bus.hud.push(text.to_owned());
                }
            }
        });
        let say = shared.clone();
        engine.register_fn("say", move |text: &str| {
            if let Ok(mut shared) = say.lock()
                && shared.bus.say.len() < 8
            {
                shared.bus.say.push(text.to_owned());
            }
        });
        engine.register_fn("log", |text: &str| println!("mod: {text}"));
        let keep = shared.clone();
        engine.register_fn("store", move |key: &str, value: Dynamic| {
            if let Ok(mut shared) = keep.lock() {
                let which = shared.running;
                if let Some(drawer) = shared.state.get_mut(which)
                    && (drawer.len() < 256 || drawer.contains_key(key))
                {
                    drawer.insert(key.to_owned(), value);
                }
            }
        });
        let recall = shared.clone();
        engine.register_fn("recall", move |key: &str| -> Dynamic {
            recall
                .lock()
                .ok()
                .and_then(|shared| {
                    shared
                        .state
                        .get(shared.running)
                        .and_then(|drawer| drawer.get(key).cloned())
                })
                .unwrap_or(Dynamic::UNIT)
        });

        let mut loaded = Vec::new();
        let listing = std::fs::read_dir(directory).ok();
        let mut files: Vec<PathBuf> = listing
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "rhai"))
            .collect();
        // A stable order, so two mods drawing to the HUD do not swap places between runs.
        files.sort();
        for path in files {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "mod".to_owned());
            let source = match std::fs::read_to_string(&path) {
                Ok(source) => source,
                Err(e) => {
                    eprintln!("mod {name}: cannot be read ({e})");
                    continue;
                }
            };
            match engine.compile(&source) {
                Ok(ast) => {
                    let hooks = ast
                        .iter_functions()
                        .map(|function| function.name.to_owned())
                        .collect();
                    loaded.push(Loaded {
                        name,
                        ast,
                        scope: Scope::new(),
                        hooks,
                        broken: false,
                    });
                }
                Err(e) => eprintln!("mod {name}: {e}"),
            }
        }

        if let Ok(mut shared) = shared.lock() {
            shared.state = (0..loaded.len()).map(|_| HashMap::new()).collect();
        }
        let mut mods = Self {
            engine,
            loaded,
            shared,
            directory: directory.to_path_buf(),
        };
        mods.call("on_load", ());
        mods
    }

    /// How many mods are running.
    #[must_use]
    pub fn len(&self) -> usize {
        self.loaded.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    /// What they are called.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.loaded
            .iter()
            .filter(|m| !m.broken)
            .map(|m| m.name.clone())
            .collect()
    }

    /// Call one hook on every mod that defines it.
    fn call(&mut self, hook: &str, args: impl rhai::FuncArgs + Clone) {
        for (which, mod_) in self.loaded.iter_mut().enumerate() {
            if mod_.broken || !mod_.hooks.contains(hook) {
                continue;
            }
            // Which drawer `store` and `recall` reach while this one runs.
            if let Ok(mut shared) = self.shared.lock() {
                shared.running = which;
            }
            let outcome =
                self.engine
                    .call_fn::<Dynamic>(&mut mod_.scope, &mod_.ast, hook, args.clone());
            if let Err(e) = outcome {
                eprintln!("mod {}: {hook} failed, switching it off ({e})", mod_.name);
                mod_.broken = true;
            }
        }
    }

    /// Twenty times a second, with where the player is and how they are doing.
    pub fn tick(&mut self, context: Context) {
        self.call("on_tick", (context.as_map(),));
    }

    /// Something was said.
    pub fn chat(&mut self, line: &str) {
        self.call("on_chat", (line.to_owned(),));
    }

    /// A key went down, named as the player would name it.
    pub fn key(&mut self, key: &str) {
        self.call("on_key", (key.to_owned(),));
    }

    /// A block changed.
    pub fn block(&mut self, at: (i32, i32, i32), block: u16) {
        self.call(
            "on_block",
            (
                i64::from(at.0),
                i64::from(at.1),
                i64::from(at.2),
                i64::from(block),
            ),
        );
    }

    /// Take everything the mods have asked for since the last call.
    #[must_use]
    pub fn drain(&mut self) -> Bus {
        self.shared
            .lock()
            .map(|mut shared| Bus {
                hud: std::mem::take(&mut shared.bus.hud),
                say: std::mem::take(&mut shared.bus.say),
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write some scripts to a directory of their own and load them.
    fn with(scripts: &[(&str, &str)]) -> (tempdir::Dir, Mods) {
        let dir = tempdir::Dir::new();
        for (name, source) in scripts {
            std::fs::write(dir.path().join(format!("{name}.rhai")), source).unwrap();
        }
        let mods = Mods::load_from(dir.path());
        (dir, mods)
    }

    /// A directory that cleans up after itself, so the tests need no dependency for it.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNT: AtomicU32 = AtomicU32::new(0);

        pub struct Dir(PathBuf);

        impl Dir {
            pub fn new() -> Self {
                let n = COUNT.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "ninefold-mods-{}-{n}",
                    std::process::id()
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_mod_can_draw_on_the_screen() {
        let (_dir, mut mods) = with(&[(
            "coords",
            r#"fn on_tick(game) { hud("x " + game.x + " y " + game.y); }"#,
        )]);
        assert_eq!(mods.len(), 1);
        mods.tick(Context {
            x: 10.0,
            y: 64.0,
            ..Context::default()
        });
        let bus = mods.drain();
        assert_eq!(bus.hud.len(), 1);
        assert!(bus.hud[0].starts_with("x 10"), "{:?}", bus.hud[0]);
        // Draining takes it, so the next frame starts clean.
        assert!(mods.drain().hud.is_empty());
    }

    #[test]
    fn a_mod_with_no_hooks_at_all_is_not_an_error() {
        let (_dir, mut mods) = with(&[("quiet", "let x = 1;")]);
        mods.tick(Context::default());
        mods.chat("hello");
        mods.key("F");
        assert!(mods.drain().hud.is_empty());
        assert_eq!(mods.names(), vec!["quiet".to_owned()]);
    }

    #[test]
    fn a_mod_that_will_not_compile_is_skipped_and_the_others_still_run() {
        let (_dir, mut mods) = with(&[
            ("broken", "fn on_tick(game) { this is not rhai"),
            ("fine", r#"fn on_tick(game) { hud("still here"); }"#),
        ]);
        assert_eq!(mods.len(), 1, "only the one that compiled");
        mods.tick(Context::default());
        assert_eq!(mods.drain().hud, vec!["still here".to_owned()]);
    }

    #[test]
    fn a_mod_that_throws_is_switched_off_rather_than_run_every_tick() {
        let (_dir, mut mods) = with(&[("angry", r#"fn on_tick(game) { throw "no"; }"#)]);
        mods.tick(Context::default());
        assert!(mods.names().is_empty(), "it has been switched off");
        // And a second tick does nothing at all.
        mods.tick(Context::default());
    }

    #[test]
    fn a_runaway_loop_is_stopped_instead_of_hanging_the_game() {
        let (_dir, mut mods) = with(&[("spin", "fn on_tick(game) { while true { } }")]);
        let started = std::time::Instant::now();
        mods.tick(Context::default());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the operation limit did not stop it"
        );
        assert!(mods.names().is_empty(), "and it was switched off");
    }

    #[test]
    fn scripts_cannot_reach_the_filesystem() {
        // Nothing to do with permissions: the functions simply are not registered.
        let (_dir, mut mods) = with(&[(
            "nosy",
            r#"fn on_tick(game) { let f = open_file("/etc/passwd"); }"#,
        )]);
        mods.tick(Context::default());
        assert!(mods.names().is_empty(), "it failed, as it should");
    }

    #[test]
    fn chat_and_keys_reach_the_hooks_that_want_them() {
        let (_dir, mut mods) = with(&[(
            "watch",
            r#"
            fn on_chat(line) { if line.contains("ping") { say("pong"); } }
            fn on_key(key) { hud("key " + key); }
            "#,
        )]);
        mods.chat("someone said ping");
        mods.key("G");
        let bus = mods.drain();
        assert_eq!(bus.say, vec!["pong".to_owned()]);
        assert_eq!(bus.hud, vec!["key G".to_owned()]);
    }

    #[test]
    fn state_is_kept_between_ticks() {
        // A rhai function cannot see a variable declared at the top of the file, so a mod that
        // counts something keeps it in its own drawer instead.
        let (_dir, mut mods) = with(&[(
            "counter",
            r#"
            fn on_tick(game) {
                let seen = recall("seen");
                if seen == () { seen = 0; }
                seen += 1;
                store("seen", seen);
                hud("ticks " + seen);
            }
            "#,
        )]);
        for _ in 0..3 {
            mods.tick(Context::default());
        }
        let bus = mods.drain();
        assert_eq!(bus.hud.last().map(String::as_str), Some("ticks 3"));
    }

    #[test]
    fn two_mods_keep_their_own_state() {
        let script = r#"
            fn on_tick(game) {
                let n = recall("n");
                if n == () { n = 0; }
                store("n", n + 1);
                hud("n " + (n + 1));
            }
        "#;
        let (_dir, mut mods) = with(&[("one", script), ("two", script)]);
        mods.tick(Context::default());
        mods.tick(Context::default());
        let bus = mods.drain();
        // Four lines, and both mods counted to two rather than one of them to four.
        assert_eq!(bus.hud.len(), 4);
        assert_eq!(bus.hud.iter().filter(|line| *line == "n 2").count(), 2);
    }

    #[test]
    fn no_mods_directory_is_simply_no_mods() {
        let mods = Mods::load_from(Path::new("/nowhere/at/all"));
        assert!(mods.is_empty());
        assert!(mods.names().is_empty());
    }
}
