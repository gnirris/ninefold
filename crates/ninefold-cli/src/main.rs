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
    println!(
        "chunks received: {} ({} columns decoded)",
        session.chunks_seen,
        session.columns.len()
    );

    // The column under our feet, as decoded. On a flat world this is known ground truth.
    let (bx, bz) = (start.x.floor() as i32, start.z.floor() as i32);
    println!("the column at x={bx} z={bz}:");
    let base = start.y.floor() as i32;
    for y in ((base - 3)..(base + 2)).rev() {
        match session.block_at(bx, y, bz) {
            Some(block) => println!(
                "  y={y}  id {:<3} meta {}  {}",
                block >> 4,
                block & 0xF,
                name_of(block >> 4)
            ),
            None => println!("  y={y}  (chunk not here)"),
        }
    }

    // Mesh what the server actually sent us, and see where the artwork would come from.
    {
        use ninefold_mesh::Blocks;
        struct World<'a>(&'a play::Session);
        impl Blocks for World<'_> {
            fn block(&self, x: i32, y: i32, z: i32) -> u16 {
                self.0.block_at(x, y, z).unwrap_or(0)
            }
        }
        let world = World(&session);
        let (cx, cz) = (bx.div_euclid(16) * 16, bz.div_euclid(16) * 16);
        let mut faces = 0;
        let mut vertices = 0;
        for section_y in 0..16 {
            let mesh = ninefold_mesh::section(&world, (cx, section_y * 16, cz));
            faces += mesh.faces();
            vertices += mesh.vertices.len();
        }
        println!("meshed the column at x={cx} z={cz}: {faces} faces, {vertices} vertices");
        let assets = ninefold_assets::pick(None);
        println!("textures: {}", assets.describe());
        if let Some(grass) = assets.texture("blocks/grass_top") {
            println!("  blocks/grass_top is {}x{}", grass.width, grass.height);
        }
    }

    // Now walk with the real physics, a tick at a time, and see whether the server agrees.
    use ninefold_protocol::physics::{Body, Input, TPS};
    let mut body = Body {
        x: start.x,
        y: start.y,
        z: start.z,
        on_ground: true,
        ..Body::default()
    };
    let input = Input {
        forward: 1.0,
        sprint: true,
        yaw: -90.0, // due east
        ..Input::default()
    };
    let ticks = 60;
    let tick = Duration::from_secs_f64(1.0 / TPS);
    let began = std::time::Instant::now();
    for n in 0..ticks {
        body.tick(input, &session);
        session.position.x = body.x;
        session.position.y = body.y;
        session.position.z = body.z;
        session.position.on_ground = body.on_ground;
        if let Err(e) = session.send_position() {
            eprintln!("dropped on tick {n}: {e}");
            return ExitCode::FAILURE;
        }
        if let Err(e) = session.pump() {
            eprintln!("dropped on tick {n}: {e}");
            return ExitCode::FAILURE;
        }
        std::thread::sleep(tick.saturating_sub(began.elapsed() / (n + 1).max(1)));
    }

    let travelled = ((body.x - start.x).powi(2) + (body.z - start.z).powi(2)).sqrt();
    let seconds = f64::from(ticks) / TPS;
    println!(
        "sprinted {travelled:.2} blocks in {seconds:.1}s = {:.3} blocks/second",
        travelled / seconds
    );
    println!(
        "  speed now {:.3} b/s, on ground {}",
        body.speed(),
        body.on_ground
    );
    println!(
        "  server has us at x={:.2} z={:.2}",
        session.position.x, session.position.z
    );
    println!("  keep-alives answered: {}", session.keep_alives);

    // If the server disagreed it would have teleported us back, and `position` would no longer
    // be where the physics put us.
    let drift =
        ((session.position.x - body.x).powi(2) + (session.position.z - body.z).powi(2)).sqrt();
    if drift > 0.01 {
        println!("  the server corrected us by {drift:.2} blocks: it rejected the movement");
        return ExitCode::FAILURE;
    }
    println!("  the server accepted every step");
    ExitCode::SUCCESS
}

/// Enough 1.8 block names to read a flat world by eye.
fn name_of(id: u16) -> &'static str {
    match id {
        0 => "air",
        1 => "stone",
        2 => "grass",
        3 => "dirt",
        7 => "bedrock",
        _ => "?",
    }
}
