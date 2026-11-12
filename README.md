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
| 1 | Protocol 47: codec, handshake, status, login | **in progress** |
| 2 | Headless join: chunks decoded, 1.8 movement prediction, walk a real server | |
| 3 | Rendering: terrain meshing, entities, HUD | |
| 4 | Singleplayer: world storage, worldgen, mob AI, redstone | |

Phase 2 is the one that decides whether the rest is worth building. It needs no renderer, and a
tick trace either feels like 1.8 or it doesn't.

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
