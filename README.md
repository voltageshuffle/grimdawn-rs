# grimdawn-rs

A re-implementation of Grim Dawn in Rust and Bevy. It covers the base game, all expansions and the Crucible,
and runs on the data from your own copy of the game. The repo holds no game assets. You need a legal copy of
Grim Dawn installed.

## Goals
1. **Plays like Grim Dawn.** Stats, damage, skills, loot, quests and saves follow the original rules. They come
   from the stock game database, and from decompiling the game where the data leaves a rule open.
2. **Our own engine.** We may restructure systems, or differ a little from the original, when that makes the
   code faster, more modular or simpler. Each such difference is written down in the doc comment of its module.
3. **Looks like Grim Dawn.** It only needs to read as the same game. It is not a pixel copy.

Currently out of scope: gamepad support, multiplayer, voice chat, Steam achievements and cloud saves.

## Status
Work in progress. The game is playable, but it is not yet complete or faithful to the original everywhere.
What exists today:

- **Formats:** ARC and ARZ archives, maps, meshes, textures and `.gdc` saves. The virtual file system merges the
  base game, the expansions and mods.
- **Gameplay** (headless ECS): combat, skills, devotion, pets, items, loot and loot filters, crafting, the
  stash, trading, factions, quests and story (through a Lua host), riftgates, Nemesis monsters, saving and
  loading.
- **Modes:** the campaign, the Crucible (survival) and the Shattered Realm (endless dungeon).
- **Client:** deferred rendering, characters, effects, decals, water, weather, world streaming, the UI, the
  main menu and audio.
- **Tools:** CLIs to inspect archives, records, maps, meshes, particle effects, textures, saves and story data.

## Quick start
```sh
cp local.env.example local.env   # set GD_ROOT to your Grim Dawn install
scripts/with-env cargo dev       # debug build with dev hooks
```

Tests: `scripts/with-env cargo test --workspace`. To skip the tests that read the game install, set
`GD_SKIP_DATA_TESTS=1`.

## Layout
| Crate | Holds |
|---|---|
| `gd-formats` | File formats and the virtual file system |
| `gd-sim` | Engine rules without Bevy: stats, combat, skills, items, navigation, story, Shattered Realm generation |
| `gd-game` | Headless gameplay ECS and the `TestGame` test harness |
| `gd-render` | Rendering |
| `grimdawn` | The game binary: streaming, UI, input, audio |
| `gd-tools` | Command-line tools for inspecting game data |

## Docs
- [`docs/architecture.md`](docs/architecture.md): how the systems are structured
- [`docs/running.md`](docs/running.md): running the game, test hooks and tests
- [`docs/reverse-engineering.md`](docs/reverse-engineering.md): how to decompile the original game

## Legal
This is a fan project. It is not affiliated with Crate Entertainment. Grim Dawn and its assets belong to
their owners.

The code is licensed under the [MIT License](LICENSE). That covers this repository only, not Grim Dawn's
data, which you need from your own copy of the game.
