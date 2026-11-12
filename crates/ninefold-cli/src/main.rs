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
        _ => {
            eprintln!(
                "ninefold {}\n\nusage:\n  ninefold ping <host[:port]>",
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
