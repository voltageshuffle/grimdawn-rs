# Running and testing

## Local settings
Copy `local.env.example` to `local.env` (gitignored) and set `GD_ROOT` to the Grim Dawn install (the folder
holding `database/database.arz`). Then either `source local.env`, or prefix commands with `scripts/with-env`:
`scripts/with-env cargo dev`, `scripts/with-env cargo test --workspace`. The game can also be started with
`cargo dev -- --game-dir <path>`.

## Running
- `cargo dev`: a debug build with the `dev` feature (hooks, screenshots, test scenes). gd-formats and gd-sim
  are built at opt-level 3.
- Profiling: `cargo build -p grimdawn --profile profiling --features dev` (release code and flags, symbols
  kept), then `perf record -F 999 --call-graph dwarf -o perf.data -- target/profiling/grimdawn` with
  `GD_SHOT` to end the run; `perf report -i perf.data` for CPU hot spots. Frame times: `GD_FPS=1`,
  per-pass GPU times: `GD_GPU_TIMES=1` (timestamp queries). Profile on the real display and in the real
  world (`GD_SAVE=<player.gdc>` in Devil's Crossing), never `scripts/headless-run.sh` (its compositor
  presents through the CPU, not the GPU) and never `GD_TESTWORLD` (the arena hides the real scene's
  costs). Stress on top: `GD_SPAWN_MONSTER`, `GD_FX` + `GD_FX_GRID`, `GD_GOD GD_AUTO_FIGHT`, `GD_OPTIONS`
  with every quality maxed. (Bevy's `trace_chrome` is not used: multi-GB traces and heavy latency.)
- Shipped build: `cargo build -p grimdawn --release` (thin LTO, panic = abort, stripped). Linux (Wayland and
  X11) and Windows x86-64.
- Releases: the whole workspace has one version (`[workspace.package] version` in the root `Cargo.toml`, inherited
  by every crate; it is the port's own, unrelated to the game's). To release, bump it, commit, and push a tag
  `v<version>`: `.github/workflows/release.yml` builds Linux and Windows and publishes the release, titled and
  described by `scripts/release-notes` (the game version, then the commits since the previous `v*` tag). The game
  version the port targets is `game-version` under `[workspace.metadata.grimdawn]` in the same file.
- Saves go to `GD_SAVE_DIR` (default `$XDG_DATA_HOME/grimdawn-rs/save`, `%APPDATA%\grimdawn-rs\save` on
  Windows). Maps are read straight from the install's archives; nothing is cached on disk.

## Running mods
A mod lives in `<install>/mods/<name>/`: `resources/*.arc` (maps, quests, scripts, assets) plus
`database/<name>.arz` (records). Start one from the main menu's custom-game picker, or script a session
with the `GD_MOD` / `GD_MOD_MAP` hooks (syntax: the hooks field docs). Limits: only `resources/*.arc` are
mounted — loose files are refused; one mod at a time; mod Lua runs under sandbox budgets; each mod's indexes are
kept for the process lifetime. Tools read mods without extraction: `gd-rec --mod <name> --diff` (mod-vs-install
record deltas), `gd-story --mod <name>`, `gd-map <archive.arc>:<entry>`.

## Hooks
`GD_*` environment variables script a `dev` run: pick a character (`GD_SAVE`, `GD_NAME`), place the player
(`GD_START=x,z`), fight (`GD_AUTO_FIGHT`, `GD_KILL_ALL=<secs>`, `GD_GOD`), talk (`GD_STORY_TALK`,
`GD_DIALOG_CLICK`), open windows (`GD_UI_OPEN`), take a screenshot and exit (`GD_SHOT=<png>`), and more. The
list, with syntax, is the field docs of `crates/gd-game-core/src/hooks.rs`. Hooks exist only in `dev` builds
(`cargo dev` / `cargo dev-build`, or `--features dev`): any other build, the tests included, has no hook code and
ignores the variables.

Recipes:
- Intro video: a new character from the menu plays `video/intro.ogg` (any key or click skips it); test sessions
  skip it, `GD_NO_MENU=1 GD_VIDEO=video/intro.ogg` plays it at the player's entry (add `GD_SHOT=<png>
  GD_SHOT_FRAMES=1500` for a frame around 15 s in).
- Crucible from the menu: `GD_MENU=1 GD_MENU_CRUCIBLE=1 GD_MENU_PICK=0`. Direct: `GD_SURVIVAL=a` (arena a..j).
  Full run: add `GD_GOD=1 GD_LUA='@3:Game.GetLocalPlayer():AdjustTribute(40)'
  GD_STORY_TALK=powerups,defensesite_01.,npc_event GD_DIALOG_CLICK=0,0,1,1@3 GD_AUTO_FIGHT GD_KILL_ALL=12
  GD_OPEN_CHESTS=70`.
- Shattered Realm full loop: `GD_GOD=1 GD_START=-3425,6975 GD_STORY_TALK=npc_dungeon_01a,npc_dungeon_02
  GD_DIALOG_CLICK=0,0,1@2 GD_USE_DOOR=dungeonentrance@12,totreasureroom,endlessdungeon_exit GD_WALK_TO=445,1920
  GD_KILL_ALL=8 GD_USE_EXIT_GATE=1 GD_USE_BUFF_SHRINE=1 GD_OPEN_CHESTS=1`. A specific floor:
  `GD_ENDLESS_FLOOR=<floor record>`.
- Test arena: `GD_TESTWORLD=1` (flat grid; `GD_ARENA_MONSTERS=<record>[*n];…`).
- Stress scene: `GD_STRESS=<n>` keeps `n` foes fighting around the player, kills a third every 4 s (gibs,
  loot, death FX) and respawns them, bursts impact effects every 0.5 s and travels to the next riftgate every
  20 s (levels stream in and out). In the real world: `GD_SAVE=<player.gdc> GD_GOD GD_AUTO_FIGHT=40
  GD_STRESS=300 GD_FPS=1 GD_GPU_TIMES=1 GD_SHOT_SECS=90` — profile it with `perf` per the Profiling note.
- Leak audit: `GD_EXIT_CYCLES=<n>` (+ `GD_SAVE`): repeated exit-to-menu cycles that log entity counts.
- Memory audit: `GD_MEM_LOG=5` logs the resident set, glibc's heap (in use / kept free), asset counts and cache
  sizes (`mem:`); `GD_MEM_TOUR=12@6` travels to 12 riftgates, `GD_MEM_EXIT=160` exits to the menu and starts the
  character again. With `LD_PRELOAD=/usr/lib/libjemalloc.so MALLOC_CONF=prof:true,prof_prefix:<dir>/je` each
  log dumps a heap profile (`jeprof --text --cum target/debug/grimdawn <dump>`; keep dumps out of /tmp).

## Tests
- `cargo test --workspace` (needs `GD_ROOT`; `GD_SKIP_DATA_TESTS=1` skips the tests that read the install).
  One binary: `cargo test -p gd-game --test game -- <module>::`, `-p gd-game --test crucible`,
  `-p gd-sim --test sim`, `-p gd-formats --test formats`.
- Headless gameplay: `gd_game::testing::TestGame` (real data, fixed 16 ms frames, single-threaded, seeded
  RNG; `new_character`, `spawn_player`, `spawn_monster`, `run_until`, `steps`).
- Tests share one process: never set environment variables. Files go in `gd_formats::testdata::TempDir`.
  Every `TestGame` has its own save folder.

## Cheap output
- Tests: `cargo test -q -p <crate> 2>&1 | grep -E '^test result|FAILED|panicked|^error'`. Rerun a failure
  with `--exact --nocapture`.
- Build / lint: `cargo dev-build 2>&1 | grep -E '^(error|warning)' -A5 | head -80`;
  `cargo clippy --workspace --all-targets --message-format short 2>&1 | grep -E '^(error|warning)' | sort -u`.
  Run it again with `-p grimdawn -p gd-game --all-targets --features dev`.
- Game logs: redirect to a file, strip ANSI codes (`sed 's/\x1b\[[0-9;]*m//g'`), then grep.
