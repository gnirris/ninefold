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
| 2 | Headless join: chunks decoded, 1.8 movement prediction, walk a real server | **done** |
| 3 | Rendering: terrain meshing, entities, HUD | **in progress** — textured terrain, biome colour, crossed-sprite plants, mob and player models, the vanilla HUD and menus, inventory, placing, sound |
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

## The movement

The 1.8 feel is the reason this targets 1.8.9 at all, so the constants are checked against the
numbers the game is known for rather than against anything the code decided:

| | |
|---|---|
| walking | 4.317 blocks/second |
| sprinting | 5.612 blocks/second |
| jump apex | 1.2522 blocks |
| terminal fall | 3.92 blocks/tick |

The order inside a tick matters as much as the constants: friction applies to the motion carried
in from last tick *before* this tick's input is added, and gravity applies *after* the position
has moved. Swap either and the numbers above come out wrong.

Diagonal movement is about 2% faster, and that is correct — `moveFlying` only scales input down
when its magnitude is over 1, so a straight line keeps the 0.98 a held key gives while a diagonal
(1.386) is normalised to 1.0. The same 0.98 is what puts walking at 4.317, so "fixing" one breaks
the other.

Driven against a real server, the physics is accepted without a single correction:

```
sprinted 16.50 blocks in 3.0s = 5.500 blocks/second
  speed now 5.612 b/s, on ground true
  the server accepted every step
```

![ninefold drawing a flat world from a real 1.8.9 server](docs/first-light.png)

*An ordinary 1.8.9 world — oak forest, 169 chunks, 1,071,688 vertices — decoded, meshed and drawn
with the player's own textures.*

## Running it

```sh
cargo run --release -p ninefold-cli    -- ping 127.0.0.1:25599        # what is this server
cargo run --release -p ninefold-cli    -- join 127.0.0.1:25599 name   # headless: join, decode, walk
cargo run --release -p ninefold-client -- 127.0.0.1:25599 name        # with a window
```

It starts where the game starts — the title screen, with the logo and the dirt behind it. Press
enter, or click **Join**, to connect. `--join` after the name skips that and connects straight
away, which is what a launcher wants.

| | |
|---|---|
| WASD, space, left shift, left control | move, jump, sneak, sprint |
| mouse | look; click the window to take the cursor |
| left click | break the block under the crosshair — held down, for as long as the block takes |
| right click | place what is in hand, or use it |
| 1–9, scroll wheel | pick a hotbar slot |
| E | inventory |
| T, / | chat |
| Q | drop what is in hand |
| escape | the game menu |
| F3, F3+B | the readout, and hitboxes |

Breaking is timed against the block's own hardness, because a 1.8 server times the dig itself and
throws away a "finished" that arrives too early. In creative it is one hit, as it should be.

The physics runs on its own 20 Hz clock rather than per frame, the way Minecraft's own loop does,
so movement is identical at 30 frames a second or 300.

To put it in the application menu, pointing at the binary in this checkout:

```sh
packaging/install.sh                      # title screen only
packaging/install.sh 127.0.0.1:25599 me   # and a "join the usual server" entry under it
```

The entry and the icon go under `~/.local/share`, so nothing a package manager owns is touched.
A rebuild needs nothing; moving the checkout means running it again.

## The block table

Around 130 entries covering what an overworld is actually made of, keyed by id and optionally by
metadata so wool colours, wood types and stone variants sit on top of a default. Every texture
name in it was checked against a real 1.8.9 jar rather than typed from memory, and a block with
no entry draws a loud magenta tile so a gap is obvious instead of invisible. In a forest, 0.0% of
the screen comes back magenta.

## The atlas

A draw call samples one texture, so every block face comes out of a single image of 16×16 tiles.
Two of them are not simply copied in:

- **grass_top** and the leaves ship greyscale and take their colour from the biome. That colour
  is *not* baked into the tile, because one tile serves a grass block in a desert and the same
  block in a swamp — it goes on the vertices instead, read out of the game's own
  `colormap/grass.png` and `colormap/foliage.png` by the biome's temperature and rainfall, which
  is exactly how vanilla does it.
- **grass_side** is dirt with a dull fringe, and `grass_side_overlay` is the green part of that
  fringe as a separate cut-out. Vanilla draws the side plain and lays the tinted overlay over it,
  so the mesher emits two quads for a grass block's side. Compositing them into one tile would
  turn the dirt green in a jungle.

Sampling is nearest-neighbour, because 1.8's textures are 16 pixels across and are meant to look
it, and the fragment shader cuts out below half alpha rather than blending, so leaves and glass
work without sorting anything back to front.

## What is a cube, and what is not

Almost everything is a cube, which is the only reason a chunk renderer can be fast: a face is
emitted only where the block next to it does not hide it, which takes a section from 24,576
faces to a few hundred. Two decisions make that correct rather than merely fast.

**The list is of things that *are* cubes, not things that are not.** Every stair, slab, fence,
pane, torch, rail, door, carpet and piece of redstone is not a full block, and anything left off
a list of exceptions would wrongly hide its neighbours' faces. An unknown id erring towards
visible costs a few extra faces; the other way round punches holes in the world.

**A block decides what hides *it*.** "Can you see through it" and "does it hide the face behind
it" are different questions. Two leaf blocks are both see-through, and hiding the faces between
them turns a canopy into a checkerboard you can see sky through. Two water blocks are also both
see-through, and *not* hiding the faces between them fills an ocean with invisible geometry.

Plants are the exception that cannot be skipped: grass, flowers, saplings, crops and sugar cane
are two intersecting planes, drawn from both sides because back faces are culled and a flower
that vanishes when you walk round it is worse than no flower.

## Mobs and players

Every model in 1.8 is a handful of boxes, and each box's six faces take their pixels from one
unwrapped cross on the mob's texture. The layout is fixed — for a box `w` wide, `h` tall and `d`
deep placed at `(u, v)`:

```
       +--------+--------+
       |  top   | bottom |          d tall
+------+--------+--------+------+
| left | front  | right  | back |   h tall
+------+--------+--------+------+
   d       w        d       w
```

which is why a player's face is always at (8, 8) on a skin. Players, zombies, skeletons,
creepers, pigs, cows, sheep and chickens have models; anything else is still a box, honestly
drawn as one.

Nothing animates yet. Legs do not swing and heads turn with the body, because the server does not
send limb angles — vanilla works them out from how far the entity has moved, and that belongs
with a proper entity tick. Entities are eased towards where the server last put them rather than
snapped, because the server speaks twenty times a second at best and everything else in the world
would stutter even while the player's own movement is smooth.

## Sound

1.8 keeps its sounds in the launcher's asset store rather than the version jar: an index mapping
`minecraft/sounds/dig/stone1.ogg` to a SHA-1, and a pile of files named after their hash. They
are read in place, decoded with a pure-Rust Vorbis decoder, and mixed into one stream — panned by
where the sound is relative to where the player is looking, and quieter the further away it is.

Footsteps come from the block underfoot, every one and two thirds blocks, which is what vanilla's
own distance counter works out to. Breaking and placing use the block's own material. Everything
the server asks for by name — a pig, a door, an explosion — is resolved the way vanilla resolves
it: dots become slashes, and a name with several numbered variants picks between them.

No sound card, or no asset store, gives a silent game rather than no game.

## Where the artwork comes from

ninefold ships no Minecraft assets and never will: the engine can be rewritten, the artwork
cannot be redistributed. Nothing in the client opens a jar directly — it asks for
`blocks/grass_top`, and an [`AssetSource`](crates/ninefold-assets) decides where that comes from:

- your own copy of the game, read in place and nothing copied out of it, or
- ninefold's own placeholder art, so the client still runs for someone who has no copy.

That is the whole plug-and-play story. Installed, you get the game's look; not installed, you
still get a game. Only the first needs you to own Minecraft, and the client never has to be
started through the Java launcher — downloading a version is enough.

## The interface

The layout is written in the client; the pixels come out of the player's jar. Vanilla picks a
whole number for the interface scale so its 16-pixel art never lands between screen pixels, and
every placement is in those units rather than screen pixels — which is why the hotbar sits in the
same place at any window size.

The font is measured the way the game measures it: `ascii.png` carries no metrics, so each
glyph's width is found by scanning its cell from the right until a column has a pixel in it. An
`i` comes out two pixels wide and an `m` six. That matters for more than looks — the chat box,
the button labels and the item counts are all positioned by measuring a string.

Items in a slot are drawn two ways, because the game draws them two ways: an item has a flat
picture of its own in `textures/items`, and a block has none at all and is built as a little cube
out of its own faces.

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
