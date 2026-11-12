//! The menus.
//!
//! Each screen is a function that draws itself and hands back where its buttons are, so the
//! click handling is one `find` over a list rather than a widget tree. That is enough for the
//! four screens 1.8 actually needs to be playable, and it keeps the drawing honest: the layout
//! numbers below are vanilla's own, in interface units, measured from the same corners.

use ninefold_protocol::item::Slot;
use ninefold_protocol::play::{HOTBAR_START, INVENTORY_SLOTS};

use crate::font::{Font, LINE};
use crate::gui::{self, Button, SlotArt, Ui};

/// Which screen is in front of the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Playing, nothing in the way.
    None,
    Title,
    Connecting,
    Pause,
    Inventory,
    /// Typing, with the chat box open.
    Chat,
    /// The connection failed or was lost.
    Lost,
}

impl Screen {
    /// Does this screen want the mouse free and the world still?
    #[must_use]
    pub const fn modal(self) -> bool {
        !matches!(self, Screen::None | Screen::Chat)
    }
}

/// What clicking something would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Connect,
    Resume,
    Disconnect,
    Respawn,
    Quit,
    /// A slot in the inventory, by its window index.
    Slot(usize),
}

/// Somewhere that can be clicked.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub action: Action,
    pub at: (f32, f32),
    pub size: (f32, f32),
}

impl Hit {
    #[must_use]
    pub fn covers(&self, x: f32, y: f32) -> bool {
        x >= self.at.0
            && x < self.at.0 + self.size.0
            && y >= self.at.1
            && y < self.at.1 + self.size.1
    }
}

/// Lay out a column of buttons the way every vanilla menu does, and draw them.
fn buttons(
    ui: &mut Ui,
    font: &Font,
    mouse: (f32, f32),
    from: f32,
    rows: &[(Action, &str, bool)],
) -> Vec<Hit> {
    let (w, _) = ui.size();
    let width = 200.0;
    let x = w / 2.0 - width / 2.0;
    let mut hits = Vec::new();
    for (index, (action, label, enabled)) in rows.iter().enumerate() {
        let y = from + index as f32 * 24.0;
        let hit = Hit {
            action: *action,
            at: (x, y),
            size: (width, 20.0),
        };
        let state = if !*enabled {
            Button::Disabled
        } else if hit.covers(mouse.0, mouse.1) {
            Button::Over
        } else {
            Button::Idle
        };
        ui.button(font, label, (x, y), width, state);
        if *enabled {
            hits.push(hit);
        }
    }
    hits
}

/// The title screen: dirt, the logo, and somewhere to go.
pub fn title(ui: &mut Ui, font: &Font, mouse: (f32, f32), server: &str) -> Vec<Hit> {
    let (w, _) = ui.size();
    ui.dirt();
    // Vanilla draws the logo as two halves side by side, 310 units wide in all.
    let left = w / 2.0 - 137.0;
    ui.sprite(gui::LOGO_LEFT, left, 30.0);
    ui.sprite(gui::LOGO_RIGHT, left + 155.0, 30.0);

    let hits = buttons(
        ui,
        font,
        mouse,
        100.0,
        &[
            (Action::Connect, &format!("Join {server}"), true),
            (Action::Quit, "Quit Game", true),
        ],
    );
    ui.text(
        font,
        concat!("ninefold ", env!("CARGO_PKG_VERSION")),
        2.0,
        2.0,
        [1.0, 1.0, 1.0, 1.0],
    );
    let note = "Minecraft 1.8.9 in Rust. Not affiliated with Mojang.";
    let width = font.width(note);
    let (_, h) = ui.size();
    ui.text(font, note, w - width - 2.0, h - 10.0, [1.0, 1.0, 1.0, 1.0]);
    hits
}

/// Waiting for the server to answer.
pub fn connecting(ui: &mut Ui, font: &Font, server: &str, waited: f32) {
    let (w, h) = ui.size();
    ui.dirt();
    ui.text_centred(
        font,
        "Connecting to the server...",
        w / 2.0,
        h / 2.0 - 20.0,
        [1.0, 1.0, 1.0, 1.0],
    );
    // Something that moves, so a slow server does not look like a hung client.
    let dots = ".".repeat(1 + (waited * 2.0) as usize % 3);
    ui.text_centred(font, server, w / 2.0, h / 2.0, [0.63, 0.63, 0.63, 1.0]);
    ui.text_centred(font, &dots, w / 2.0, h / 2.0 + 14.0, [1.0, 1.0, 1.0, 1.0]);
}

/// It did not work, or it stopped working.
pub fn lost(ui: &mut Ui, font: &Font, mouse: (f32, f32), why: &str) -> Vec<Hit> {
    let (w, _) = ui.size();
    ui.dirt();
    ui.text_big(
        font,
        "Connection Lost",
        w / 2.0,
        60.0,
        [1.0, 1.0, 1.0, 1.0],
        1.5,
    );
    // A long disconnect reason has to wrap or it runs off both sides of the screen.
    for (row, line) in wrap(font, why, w - 60.0).iter().take(6).enumerate() {
        ui.text_centred(
            font,
            line,
            w / 2.0,
            90.0 + row as f32 * LINE,
            [1.0, 0.63, 0.63, 1.0],
        );
    }
    buttons(
        ui,
        font,
        mouse,
        160.0,
        &[
            (Action::Connect, "Try Again", true),
            (Action::Quit, "Quit Game", true),
        ],
    )
}

/// Escape, mid-game.
pub fn pause(ui: &mut Ui, font: &Font, mouse: (f32, f32)) -> Vec<Hit> {
    let (w, h) = ui.size();
    // Vanilla darkens the world rather than hiding it.
    ui.rect(0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.6]);
    ui.text_centred(font, "Game Menu", w / 2.0, 40.0, [1.0, 1.0, 1.0, 1.0]);
    buttons(
        ui,
        font,
        mouse,
        64.0,
        &[
            (Action::Resume, "Back to Game", true),
            (Action::Disconnect, "Disconnect", true),
            (Action::Quit, "Quit Game", true),
        ],
    )
}

/// Dead, waiting to be asked to come back.
pub fn died(ui: &mut Ui, font: &Font, mouse: (f32, f32)) -> Vec<Hit> {
    let (w, h) = ui.size();
    // The red wash vanilla puts over everything.
    ui.rect(0.0, 0.0, w, h, [0.4, 0.05, 0.05, 0.6]);
    ui.text_big(font, "You died!", w / 2.0, 30.0, [1.0, 1.0, 1.0, 1.0], 2.0);
    buttons(
        ui,
        font,
        mouse,
        72.0,
        &[
            (Action::Respawn, "Respawn", true),
            (Action::Disconnect, "Title Menu", true),
        ],
    )
}

/// The player's own inventory, at the coordinates `ContainerPlayer` puts its slots.
pub fn inventory(
    ui: &mut Ui,
    art: SlotArt<'_>,
    held: (&[Option<Slot>], Option<Slot>),
    mouse: (f32, f32),
) -> Vec<Hit> {
    let (slots, carried) = held;
    let (w, h) = ui.size();
    ui.rect(0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.4]);
    let (panel_w, panel_h) = (gui::INVENTORY.w as f32, gui::INVENTORY.h as f32);
    let (ox, oy) = ((w - panel_w) / 2.0, (h - panel_h) / 2.0);
    ui.sprite(gui::INVENTORY, ox, oy);
    ui.text_flat(art.font, "Crafting", ox + 97.0, oy + 7.0, [
        0.25, 0.25, 0.25, 1.0,
    ]);

    let mut hits = Vec::new();
    for index in 0..INVENTORY_SLOTS {
        let Some((sx, sy)) = slot_at(index) else {
            continue;
        };
        let (x, y) = (ox + sx, oy + sy);
        let hit = Hit {
            action: Action::Slot(index),
            at: (x, y),
            size: (16.0, 16.0),
        };
        // Vanilla lights a slot the mouse is over.
        if hit.covers(mouse.0, mouse.1) {
            ui.rect(x, y, 16.0, 16.0, [1.0, 1.0, 1.0, 0.4]);
        }
        if let Some(stack) = slots.get(index).copied().flatten() {
            ui.slot(art, stack, x, y);
        }
        hits.push(hit);
    }
    // Whatever the mouse is carrying follows it, drawn last so it is over everything.
    if let Some(stack) = carried {
        ui.slot(art, stack, mouse.0 - 8.0, mouse.1 - 8.0);
    }
    hits
}

/// Where one window slot sits inside the inventory panel.
#[must_use]
pub fn slot_at(index: usize) -> Option<(f32, f32)> {
    let cell = 18.0;
    match index {
        // The crafting result.
        0 => Some((144.0, 36.0)),
        // The two-by-two grid.
        1..=4 => {
            let n = index - 1;
            Some((98.0 + (n % 2) as f32 * cell, 18.0 + (n / 2) as f32 * cell))
        }
        // Armour, helmet at the top.
        5..=8 => Some((8.0, 8.0 + (index - 5) as f32 * cell)),
        // The main three rows.
        9..=35 => {
            let n = index - 9;
            Some((8.0 + (n % 9) as f32 * cell, 84.0 + (n / 9) as f32 * cell))
        }
        // And the hotbar along the bottom.
        36..=44 => Some((8.0 + (index - HOTBAR_START) as f32 * cell, 142.0)),
        _ => None,
    }
}

/// The chat log, and the box being typed into.
pub fn chat(ui: &mut Ui, font: &Font, lines: &[String], typing: Option<&str>) {
    let (w, h) = ui.size();
    // Vanilla's chat sits above the hotbar and fades the oldest lines; this keeps the last ten.
    let shown = lines.len().saturating_sub(10);
    let bottom = h - 48.0;
    for (row, line) in lines[shown..].iter().enumerate() {
        let y = bottom - (lines[shown..].len() - row) as f32 * LINE;
        let text = font.trim(line, w - 8.0);
        let width = font.width(&text).max(1.0);
        ui.rect(2.0, y - 1.0, width + 2.0, LINE, [0.0, 0.0, 0.0, 0.4]);
        ui.text(font, &text, 3.0, y, [1.0, 1.0, 1.0, 1.0]);
    }
    if let Some(typing) = typing {
        ui.rect(2.0, h - 14.0, w - 4.0, 12.0, [0.0, 0.0, 0.0, 0.5]);
        // The cursor vanilla blinks; a steady one is less work and no less clear.
        let shown = font.trim(typing, w - 16.0);
        ui.text(font, &format!("{shown}_"), 4.0, h - 12.0, [
            1.0, 1.0, 1.0, 1.0,
        ]);
    }
}

/// The readout F3 shows.
pub fn debug(ui: &mut Ui, font: &Font, lines: &[String]) {
    for (row, line) in lines.iter().enumerate() {
        let y = 2.0 + row as f32 * LINE;
        let width = font.width(line);
        ui.rect(1.0, y - 1.0, width + 2.0, LINE, [0.0, 0.0, 0.0, 0.4]);
        ui.text(font, line, 2.0, y, [1.0, 1.0, 1.0, 1.0]);
    }
}

/// Break a line into pieces that each fit `limit`.
fn wrap(font: &Font, text: &str, limit: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if font.width(&candidate) > limit && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ninefold_assets::{AssetSource, Texture};

    struct Flat;
    impl AssetSource for Flat {
        fn texture(&self, _id: &str) -> Option<Texture> {
            Some(Texture::flat(256, 256, [128, 128, 128, 255]))
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    fn font() -> Font {
        Font::measure(Some(&Texture::flat(128, 128, [255, 255, 255, 255])))
    }

    #[test]
    fn a_hit_covers_its_own_corner_but_not_the_next_one_along() {
        let hit = Hit {
            action: Action::Quit,
            at: (10.0, 20.0),
            size: (200.0, 20.0),
        };
        assert!(hit.covers(10.0, 20.0));
        assert!(hit.covers(209.9, 39.9));
        assert!(!hit.covers(210.0, 30.0));
        assert!(!hit.covers(100.0, 40.0));
        assert!(!hit.covers(9.0, 30.0));
    }

    #[test]
    fn the_title_screen_has_somewhere_to_click() {
        let mut ui = Ui::new(1280, 720);
        let hits = title(&mut ui, &font(), (0.0, 0.0), "127.0.0.1:25565");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].action, Action::Connect);
        assert!(!ui.sheet.is_empty(), "the dirt and the logo");
        assert!(!ui.text.is_empty(), "the button labels");
    }

    #[test]
    fn hovering_a_button_changes_how_it_is_drawn() {
        let draw = |mouse| {
            let mut ui = Ui::new(1280, 720);
            let hits = pause(&mut ui, &font(), mouse);
            let over = hits[0];
            (ui, over)
        };
        let (cold, button) = draw((0.0, 0.0));
        let middle = (button.at.0 + 10.0, button.at.1 + 10.0);
        let (warm, _) = draw(middle);
        assert_ne!(
            cold.sheet.len() + cold.text.len(),
            0,
            "the menu drew something"
        );
        // The hovered button samples a different row of the widget sheet.
        let cold_uv: Vec<_> = cold.sheet.iter().map(|g| g.uv[1]).collect();
        let warm_uv: Vec<_> = warm.sheet.iter().map(|g| g.uv[1]).collect();
        assert_ne!(cold_uv, warm_uv);
    }

    #[test]
    fn every_inventory_slot_has_a_place_and_they_do_not_overlap() {
        let mut seen = std::collections::HashSet::new();
        for index in 0..INVENTORY_SLOTS {
            let at = slot_at(index).unwrap_or_else(|| panic!("slot {index} has nowhere to go"));
            let key = (at.0 as i32, at.1 as i32);
            assert!(seen.insert(key), "slot {index} sits on top of another");
        }
        assert_eq!(slot_at(45), None, "there is no slot 45");
        // The hotbar is the bottom row, in order.
        let first = slot_at(HOTBAR_START).unwrap();
        let last = slot_at(HOTBAR_START + 8).unwrap();
        assert_eq!(first.1, last.1, "the hotbar is one row");
        assert!(last.0 > first.0);
    }

    #[test]
    fn clicking_the_inventory_lands_on_the_slot_under_the_mouse() {
        let atlas = crate::atlas::Atlas::build(&Flat);
        let items = crate::items::Items::build(&Flat);
        let font = font();
        let mut ui = Ui::new(1280, 720);
        let slots = vec![None; INVENTORY_SLOTS];
        let hits = inventory(
            &mut ui,
            SlotArt {
                atlas: &atlas,
                items: &items,
                font: &font,
                light: 1.0,
            },
            (&slots, None),
            (0.0, 0.0),
        );
        assert_eq!(hits.len(), INVENTORY_SLOTS);
        // Pick the first hotbar slot and click its middle.
        let hotbar = hits
            .iter()
            .find(|h| h.action == Action::Slot(HOTBAR_START))
            .expect("a hotbar slot");
        let middle = (hotbar.at.0 + 8.0, hotbar.at.1 + 8.0);
        let under = hits.iter().find(|h| h.covers(middle.0, middle.1));
        assert_eq!(
            under.map(|h| h.action),
            Some(Action::Slot(HOTBAR_START)),
            "the mouse is over exactly one slot"
        );
    }

    #[test]
    fn long_text_wraps_rather_than_running_off_the_screen() {
        let font = font();
        let long = "the server closed the connection because something went very wrong indeed";
        let lines = wrap(&font, long, 200.0);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(font.width(line) <= 200.0, "{line:?} is too wide");
        }
        // Nothing is lost in the wrapping.
        assert_eq!(lines.join(" "), long);
    }

    #[test]
    fn the_chat_shows_only_the_last_ten_lines() {
        let font = font();
        let lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        let mut ui = Ui::new(1280, 720);
        chat(&mut ui, &font, &lines, None);
        // Ten strips of background, one per line.
        assert_eq!(ui.sheet.len(), 10 * 6);
    }

    #[test]
    fn typing_adds_the_box_at_the_bottom() {
        let font = font();
        let mut quiet = Ui::new(1280, 720);
        chat(&mut quiet, &font, &[], None);
        let mut typing = Ui::new(1280, 720);
        chat(&mut typing, &font, &[], Some("hello"));
        assert!(typing.sheet.len() > quiet.sheet.len());
        assert!(!typing.text.is_empty());
    }

    #[test]
    fn only_some_screens_stop_the_world() {
        assert!(!Screen::None.modal());
        assert!(!Screen::Chat.modal(), "you can walk while typing");
        assert!(Screen::Pause.modal());
        assert!(Screen::Inventory.modal());
        assert!(Screen::Title.modal());
    }
}
