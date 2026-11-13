// Watches the chat and answers one thing, as a small example of a mod that reacts to the world
// rather than just drawing on it.
//
// Every mod gets its own drawer for state, because a rhai function cannot see a variable
// declared at the top of the file — `store` and `recall` are how a mod remembers anything.

fn on_load() {
    store("answered", 0);
}

fn on_chat(line) {
    // Nothing clever: if someone says "ping", say "pong", at most three times a session so it
    // cannot turn into two mods talking to each other for ever.
    let answered = recall("answered");
    if answered == () { answered = 0; }
    if answered < 3 && line.to_lower().contains("ping") {
        say("pong");
        store("answered", answered + 1);
    }
}

fn on_block(x, y, z, block) {
    // Block id 7 is bedrock. Worth a comment if one ever changes next to you.
    if block / 16 == 7 {
        log("bedrock appeared at " + x + " " + y + " " + z);
    }
}
