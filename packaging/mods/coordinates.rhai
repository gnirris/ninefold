// A readout that does not need F3 held down.
//
// Drop this in ~/.config/ninefold/mods/ and it runs. `hud` adds a line to the screen; `game`
// carries where the player is and how they are doing, twenty times a second.

fn on_load() {
    log("coordinates: ready");
    store("shown", true);
}

// F toggles it, because a readout you cannot turn off is not a feature.
fn on_key(key) {
    if key == "KeyF" {
        let shown = recall("shown");
        store("shown", shown != true);
    }
}

fn on_tick(game) {
    if recall("shown") != true { return; }
    hud("xyz " + round(game.x) + " " + round(game.y) + " " + round(game.z));
    hud(round(game.fps) + " fps" + (if game.on_ground { "" } else { "  (falling)" }));
}

// Rhai has no `round` for display, so here is one that keeps a decimal place.
fn round(value) {
    let scaled = (value * 10.0).to_int();
    let whole = scaled / 10;
    let tenth = scaled % 10;
    if tenth < 0 { tenth = -tenth; }
    whole + "." + tenth
}
