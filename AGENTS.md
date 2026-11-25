# Grim Dawn → Rust / Bevy port

A playable re-implementation of Grim Dawn (base game + all expansions + the Crucible) that runs on the
original install's data. Not wanted: gamepad, multiplayer, voice chat, Steam achievements, cloud saves.

## Priorities
1. **Plays like Grim Dawn.** Gameplay rules (stats, damage, skills, loot, quests, saves) follow the real game:
   read the stock data, and decompile (`docs/reverse-engineering.md`) when the data alone doesn't say.
2. **Our engine, our structure.** We may refactor systems and differ slightly from the real game when it buys
   performance, modularity or simpler code. Write a deliberate difference into the module doc comment.
3. **Looks like Grim Dawn, not a pixel copy.** Visuals only need to read as the same game. Don't spend hours
   matching the real game's shots, don't take long series of comparison screenshots, and never trade
   performance for parity. If our version looks better, that's fine.

## Rules
- **Layering:** `gd-formats` → `gd-rules` → {`gd-sim` ∥ `gd-story`} → `gd-game-core` → {`gd-game` ∥ `gd-render` ∥ `gd-ui-kit`} → {`gd-ui` ∥ `gd-world` ∥ `gd-audio`} → `grimdawn`. A lower crate never uses a higher one. Gameplay goes in gd-game, pure maths and rules in gd-sim / gd-rules.
- **One source per value.** Raw state lives once. Each derived value comes from one function or system.
  Never cache a copy.
- **The UI only reads.** `ui::view::collect` → `HudValues` and the gameplay components. UI and input send
  requests as messages, and gd-game applies the rules. A UI system never mutates `Inventory`, `Stash` or
  `PlayerProgress`.
- **Session state is resources** (`GameMode`, `AltarState`, `Choice`, `DroppedEpics`, ...), never process
  globals. A session ends in process (`grimdawn/src/exit/mod.rs`): new session state needs its reset there. A
  `Local` that caches session data takes `NewSession`. Test hooks come from `Res<Hooks>`, never `std::env`, and
  exist only with the `dev` feature: gate every hook read with `#[cfg(feature = "dev")]`.
- **Logging and `{:?}`.** Logging (`info!`, `warn!`, `error!`, ...) exists only in dev builds: a release compiles
  every log macro out, so nothing the player needs may go only to the log. `{:?}` / `{:#?}` only in log, panic and
  assert text, never in game logic, UI text, keys, sort keys, entity names or error messages: use Display or an
  explicit `name()`, so the code works under `-Zfmt-debug=none` too.
- **Build and run** with `cargo dev-build` / `cargo dev` (they enable the `dev` feature), never plain
  `cargo build -p grimdawn`.
- **`cargo clippy --workspace --all-targets` stays clean.**
- **No machine-specific paths in tracked files.** Paths to the install, saves or RE files live in the
  gitignored `local.env` (template `local.env.example`).
- **No new tests by default.** A bug fix or feature gets no test: verify it with a throwaway check (deleted
  afterwards) or a game run. Add a test only when the user asks, or for a save format change (data-loss risk).
  Never a regression test for one bug, one record or one edge case. Tests read nothing outside the repo except
  the game install (`GD_ROOT`), and need no GPU, window or audio device.

## Layout
| Crate | Holds |
|---|---|
| `gd-formats` | File formats (ARC, ARZ, maps, meshes, textures, saves, ...); `vfs` merges base + expansions + mods; the inventory grid types |
| `gd-rules` | Bevy-free primitives shared by gd-sim and gd-story: `expr` (equations), `mode`, `faction`, `Rng` |
| `gd-sim` | Engine rules without Bevy: stats, combat, actor profiles, skills, items, nav, equip, proxy / setpiece |
| `gd-story` | Story / Lua host (quests, dialogs) and Shattered Realm (`endless`) generation; vendored mlua |
| `gd-game-core` | Gameplay data: components, resources, messages, pure resolvers, SystemSets (no systems) |
| `gd-game` | Headless gameplay ECS systems (`GameLogicPlugin`); `testing::TestGame` harness |
| `gd-render` | The look over gd-game-core: deferred render chain, models, characters, FX, decals, water, camera |
| `gd-ui-kit` | Shared UI kit over gd-game-core + gd-render: the record-driven widget builder, fonts, chrome, confirm boxes, key bindings, additive / masked UI materials |
| `gd-ui` | The in-game UI windows and HUD over gd-ui-kit + gd-game + gd-render: the movable window clusters, the rebuild hub, the Esc dispatcher, the paper-doll builders, and the presentation state (`state`) the binary's input systems share |
| `gd-world` | World streaming over gd-game + gd-render: levels parsed in the background and spawned a slice per frame, lingering unloaded levels, decoration animation |
| `gd-audio` | Sound over gd-game + gd-render: area music / ambience, sound paks, emitters, voices, the port's own mixer and output devices |
| `grimdawn` | The binary: input, the menu scene, the audio / world / menu-scene-coupled windows (options, localmap, survival, credits, mainmenu, ...), dressing gameplay entities with models / effects |
| `gd-tools` | CLIs: `gd-tools` (arc / arz), `gd-map`, `gd-mesh`, `gd-nav`, `gd-pfx`, `gd-rec` (merged records), `gd-save`, `gd-story`, `gd-tex` |

Module doc comments are the reference for formats and rules: read them before re-deriving anything.

## Docs
| Task | Read |
|---|---|
| Any code change | this file + the module docs of the files you touch |
| New systems, states, gating, requests, session lifecycle, UI rebuilds | `docs/architecture.md` |
| Running the game, tests, hooks, smoke runs | `docs/running.md` |
| Decompiling the real game | `docs/reverse-engineering.md` |
| Porting to a new game version | `docs/updating.md` |
| Local, untracked notes (read if present) | `AGENTS.local.md` |

Keep docs short and current: no changelogs or Done lists (commit messages carry the history). Engine detail
goes in the module doc comment, not in `docs/`.

## Working efficiently
- Filter command output (`docs/running.md`, "Cheap output"). While working, test only the crate you touched.
  Run `cargo test --workspace` and clippy once, before committing.
- Prefer headless gd-game / gd-sim checks over game runs. Take a screenshot only to see that something works,
  then downscale and crop it.
- Agents: only for parallel, disjoint work. Each one owns its own files. No worktrees.
- Final reports: what changed, where, and the test result.
