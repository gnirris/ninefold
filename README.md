# ninefold

Minecraft: Java Edition **1.8.9**, rewritten in Rust.

> Not an official Minecraft product, and not approved by or associated with Mojang or Microsoft.
> ninefold contains no Minecraft code, assets or data — you need your own copy of the game.

## Credit where it's owed

The idea, and most of the groundwork, is **[Horizon](https://playhorizon.dev)**
([horizon-rust/horizon](https://github.com/horizon-rust/horizon)) — they rewrote Minecraft Java
Edition **26.3** in Rust and proved the whole thing was possible: a million lines, 300+
differential harnesses against the Java game, full vanilla parity. ninefold exists because they
showed the way, and it reuses their MIT-licensed foundations where they carry over.

ninefold is not a Horizon release and is not affiliated with them. Bugs here are ours.

**What ports across:** rendering (wgpu), windowing, asset plumbing.

**What cannot:** the data model, which is the hard part. 1.8.9 is *pre-flattening*. Blocks are an
ID plus four bits of metadata, not block states; chunks arrive as fixed arrays, not palettes;
items carry damage values as subtypes; combat has no attack cooldown — which is the entire reason
anyone still plays 1.8.9. None of that survives contact with a 1.13+ architecture.

## Why 1.8.9

Because that is where the players are. 1.8 PvP is a living scene, and the 1.9 combat update is
exactly what it refuses. A modern client with a translation proxy ships the thing that audience
rejects, so the mechanics have to be native.

## Scope

Full parity, singleplayer included — worldgen, mob AI, redstone, loot, the lot. This is a
multi-year target, built in order of what can be proved:

| Phase | | State |
|---|---|---|
| 1 | Protocol 47: codec, handshake, status, login | **done** |
| 2 | Headless join: chunks decoded, 1.8 movement prediction, walk a real server | **in progress** — joins, decodes the world, walks; physics next |
| 3 | Rendering: terrain meshing, entities, HUD | |
| 4 | Singleplayer: world storage, worldgen, mob AI, redstone | |

Phase 2 is the one that decides whether the rest is worth building. It needs no renderer, and a
tick trace either feels like 1.8 or it doesn't.

## Testing against a real server

Outbound 25565 is often filtered, and public servers rate-limit, so the honest test target is a
local one: Mojang's own 1.8.9 server jar, `online-mode=false`, on a spare port. `ninefold join`
then logs in, waits for the world and walks a few blocks:

```
logged in as ninefold (5d45374d-1452-3612-a52a-de8215caa576)
  compression above 256 bytes
joined: entity 0, gamemode 0, dimension 0, flat world, 5 player slots
placed at x=1015.50 y=4.00 z=590.50
walked 5.00 blocks east, still connected
```

The chunk decoder is checked against the one world whose contents are known in advance — a
flat world is bedrock, dirt, dirt, grass, and that is what comes out:

```
the column at x=1025 z=590:
  y=3  grass
  y=2  dirt
  y=1  dirt
  y=0  bedrock
```

The server agrees, which is the part that counts:

```
ninefold[/127.0.0.1:36038] logged in with entity id 0 at (1015.5, 4.0, 590.5)
ninefold joined the game
```

## Mods

Java mods will never work here. Forge, Fabric, OptiFine and anything else that loads into the JVM
has no JVM to load into. That is a consequence of the rewrite, not a gap to be filled later.

## Build

```sh
cargo test        # the protocol codec is checked against the protocol's own reference values
cargo clippy --all-targets
```

## Licence

MIT, matching Horizon.
