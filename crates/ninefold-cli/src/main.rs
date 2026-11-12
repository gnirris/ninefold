//! ninefold, from a terminal. Only `ping` so far — the client itself has no window yet.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ping") => match args.get(1) {
            Some(target) => ping(target),
            None => {
                eprintln!("usage: ninefold ping <host[:port]>");
                ExitCode::from(2)
            }
        },
        Some("join") => match args.get(1) {
            Some(target) => join(target, args.get(2).map_or("ninefold", String::as_str)),
            None => {
                eprintln!("usage: ninefold join <host[:port]> [name]");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!(
                "ninefold {}\n\nusage:\n  ninefold ping <host[:port]>\n  ninefold join <host[:port]> [name]",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::from(2)
        }
    }
}

fn ping(target: &str) -> ExitCode {
    let (host, port) = split_target(target);
    match ninefold_protocol::status::ping(host, port) {
        Ok(s) => {
            println!("{host}:{port}");
            println!("  version   {} (protocol {})", s.version, s.protocol);
            println!("  players   {}/{}", s.players_online, s.players_max);
            println!("  latency   {} ms", s.latency.as_millis());
            for line in s.motd.lines() {
                println!("  motd      {line}");
            }
            if s.protocol != ninefold_protocol::PROTOCOL_VERSION {
                println!(
                    "\n  note: this server speaks protocol {}, not {} (1.8.9). It answered the \
                     ping anyway — most servers reply to any version — but a 1.8.9 client may not \
                     be able to join.",
                    s.protocol,
                    ninefold_protocol::PROTOCOL_VERSION
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not ping {host}:{port}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `host`, `host:port`. The default is Minecraft's own port.
fn split_target(target: &str) -> (&str, u16) {
    match target.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().unwrap_or(25565)),
        None => (target, 25565),
    }
}

/// Log in, get into the world, walk a few blocks, and report what happened.
fn join(target: &str, name: &str) -> ExitCode {
    use ninefold_protocol::play;
    use std::time::Duration;

    let (host, port) = split_target(target);
    let (mut connection, profile) = match ninefold_protocol::login::login(host, port, name) {
        Ok(ok) => ok,
        Err(e) => {
            eprintln!("could not log in to {host}:{port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("logged in as {} ({})", profile.username, profile.uuid);
    match profile.compression {
        Some(t) => println!("  compression above {t} bytes"),
        None => println!("  no compression"),
    }

    let joined = match play::read_join(&mut connection) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("never got Join Game: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "joined: entity {}, gamemode {}, dimension {}, {} world, {} player slots",
        joined.entity_id, joined.gamemode, joined.dimension, joined.level_type, joined.max_players
    );

    let mut session = play::Session::new(connection);
    if let Err(e) = session.settle(Duration::from_secs(15)) {
        eprintln!("dropped while the world loaded: {e}");
        return ExitCode::FAILURE;
    }
    if !session.placed {
        eprintln!("the server never said where we are");
        return ExitCode::FAILURE;
    }
    let start = session.position;
    println!(
        "placed at x={:.2} y={:.2} z={:.2}",
        start.x, start.y, start.z
    );
    println!("chunks received: {}", session.chunks_seen);

    for _ in 0..10 {
        if let Err(e) = session.walk(0.5, 0.0) {
            eprintln!("walking failed: {e}");
            return ExitCode::FAILURE;
        }
        if let Err(e) = session.step() {
            eprintln!("dropped while walking: {e}");
            return ExitCode::FAILURE;
        }
    }
    let moved = session.position.x - start.x;
    println!("walked {moved:.2} blocks east, still connected");
    println!("keep-alives answered: {}", session.keep_alives);
    ExitCode::SUCCESS
}
