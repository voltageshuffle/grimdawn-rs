# grimdawn-rs

A re-implementation of Grim Dawn in Rust and Bevy. It covers the base game, all expansions and the Crucible,
and runs on the data from your own copy of the game. The repo holds no game assets. You need a legal copy of
Grim Dawn installed.

Targets Grim Dawn **1.3.1.2**. Other game versions may have different data and rules, and may not work.

## Goals
1. **Plays like Grim Dawn.** Stats, damage, skills, loot, quests and saves follow the original rules. They come
   from the stock game database, and from decompiling the game where the data leaves a rule open.
2. **Our own engine.** We may restructure systems, or differ a little from the original, when that makes the
   code faster, more modular or simpler. Each such difference is written down in the doc comment of its module.
3. **Looks like Grim Dawn.** It only needs to read as the same game. It is not a pixel copy.

## Status
**Playable single-player, feature-complete for 1.3.1.2's content — but not a pixel copy and not rule-identical
everywhere.** The campaign, all three expansions, the Crucible and the Shattered Realm run end to end,
including custom games (mods), quests and story through the Lua host, and saving/loading.

Not at parity, by decision: gamepad support, multiplayer, voice chat, Steam achievements and cloud saves are
out of scope. Visuals read as the same game rather than matching it pixel for pixel.

Known approximations and open questions (e.g. attach-point rotation, the `PlayAnimation` flag, `Item.HasAffix`'s
record→instance bridge, mod database enumeration, Crucible DLC gating) are tracked in
[`PLAN.md`](PLAN.md), which also lists the remaining optional work. Every deliberate rule difference is
documented in its module's doc comment (`PORT:` notes).

Save files use the original encodings (`.gdc`, `.gdd`, `.fow`, `.gst`, `map.dat`), so characters move between
the port and the real game. Two exceptions: the Crucible leaderboard (`leaderboard.brd`) is the port's own
plain encoding, and `playmenu.cpn` is partially inferred.

### Deliberate differences from the engine
Highlights; the module doc comments are the reference:
- **Structure:** eleven crates from formats to binary, headless gameplay ECS, signature-based UI rebuilds,
  weak asset caches with no timed eviction, backgrounded world streaming.
- **Mods:** one mod at a time, only `resources/*.arc` mounted, mod indexes kept for the process, Crucible packs
  always mounted; a start whose save names a different mod is refused instead of remounting it.
- **Lua:** lenient overload dispatch and sandbox budgets; engine quirks preserved on purpose
  (`Character.Turn` no-ops, `GetPlayerName` returns "").
- **UI:** content-sized adaptive layout instead of the engine's fixed texture-sized columns and rows;
  the FPS toggle lives with the other interface options.

### Added beyond the engine
- `showFPS` overlay and `maxFPS` frame limiter — the engine has neither.
- Dev-only verification: `GD_*` hooks (only in `dev` builds), the headless `TestGame` harness
- `gd-tools`: mod-aware record/story inspection (`--mod`, `--diff`) and `<arc>:<entry>` map reading,
  without extracting archives.
- Item styles: user rules (Grim Raynbow-like) that recolour and tag item names on ground labels and tooltips, or
  hide labels; edited on the loot filter window's Item Styles page, stored in `Settings/itemstyles.txt`.

## Playing
You need Grim Dawn **1.3.1.2** installed (Steam or GOG, with whichever expansions you own) and a GPU with
Vulkan (Linux) or Vulkan / DirectX 12 (Windows). The port never writes into the game's folder; it warns (log and
main menu) when the install is another version.

### From a GitHub release
1. Download `grimdawn-rs-windows-x86_64.zip` or `grimdawn-rs-linux-x86_64.tar.gz` from the
   [Releases](../../releases) page and unpack it.
2. Either copy `grimdawn.exe` (Windows) / `grimdawn` (Linux) **into your Grim Dawn folder**, the one that holds
   `database/database.arz`, and start it from there; or keep it anywhere and tell it where the game is:
   ```sh
   grimdawn --game-dir "C:\Program Files (x86)\Steam\steamapps\common\Grim Dawn"   # Windows
   ./grimdawn --game-dir "$HOME/.local/share/Steam/steamapps/common/Grim Dawn"      # Linux
   ```
   Without `--game-dir` the port looks in the folder of the executable, then in the folder you start it from.
   If neither holds the game it exits with "no Grim Dawn install found", naming the folders it tried.

Windows: the build is unsigned, so SmartScreen may warn ("More info" → "Run anyway"). Linux: the build needs
glibc 2.39 or newer (Ubuntu 24.04, Fedora 40, any current Arch) and runs on Wayland or X11; if the file lost its
execute bit, `chmod +x grimdawn`.

### Where the game usually is
|Store / launcher|Typical folder|
|---|---|
|Steam, Windows|`C:\Program Files (x86)\Steam\steamapps\common\Grim Dawn`|
|Steam, Windows, other library|`D:\SteamLibrary\steamapps\common\Grim Dawn`|
|GOG Galaxy, Windows|`C:\Program Files (x86)\GOG Galaxy\Games\Grim Dawn`|
|GOG offline installer, Windows|`C:\GOG Games\Grim Dawn`|
|Steam, Linux (Proton)|`~/.local/share/Steam/steamapps/common/Grim Dawn`|
|Steam, Linux, Flatpak|`~/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/Grim Dawn`|
|Heroic (GOG), Linux|`~/Games/Heroic/Grim Dawn`|

On Steam you can also right-click Grim Dawn → Manage → Browse local files to open the folder.

### Saves and settings
The port keeps its own folder, separate from the real game's: `%APPDATA%\grimdawn-rs` on Windows,
`~/.local/share/grimdawn-rs` on Linux (`$XDG_DATA_HOME/grimdawn-rs` if set). Characters are in `save/`, options
and key bindings in `Settings/`. The save files use the game's formats, so a character folder can be copied
between the port and the real game.

### Item styles
The loot filter window (O) has an **Item Styles** page: rules that recolour item names and add a short tag
(for example `[BP]` on blueprints), on ground labels and in tooltips, so valuable drops stand out; a rule can also
hide a label. The first rule that matches an item wins; **Load Preset** gives a ready-made colour scheme. The rules
are global (all characters) and live in `Settings/itemstyles.txt` in the port's folder, one rule per line, which
you can also edit by hand (a bad line is skipped with a warning in the log; no file means the stock look; saving
from the window keeps your `#` comment lines, moved to the top, and turns a skipped line into a `# skipped:` comment):
```
# conditions: rarity= type= set= mi= doublerare= ascended= level=MIN-MAX name="..."
# style: color=#RRGGBB prefix="..." | suffix="..." hide
type=blueprint color=#3fc8ff prefix="[BP]"
rarity=epic,legendary set=yes color=#33ff99 suffix="[Set]"
rarity=common type=weapon,armor level=-40 hide
name="aether" color=#00ffff
```
Types: `weapon offhand armor accessory component relic blueprint augment consumable material quest lore other`;
rarities: `common magic rare epic legendary`. A hide rule only hides more than the loot filter's checkboxes, and
holding "Show Items (No Filter)" still shows everything.

### Building it yourself
1. Install Rust with [rustup](https://rustup.rs) (stable, 1.97.1 or newer: `rustup update stable`).
   - Windows: rustup asks for the Visual Studio C++ Build Tools; install them when it does.
   - Linux: install the build libraries, e.g. on Debian / Ubuntu
     `sudo apt install build-essential pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`,
     on Arch `sudo pacman -S base-devel alsa-lib systemd-libs wayland libxkbcommon`.
2. Build and run:
   ```sh
   git clone https://github.com/voltageshuffle/grimdawn-rs
   cd grimdawn-rs
   cargo run -p grimdawn --release -- --game-dir "/path/to/Grim Dawn"
   ```
   The finished program is `target/release/grimdawn` (`grimdawn.exe` on Windows); copy it like a release
   download above.

## Development
```sh
cp local.env.example local.env   # set GD_ROOT to your Grim Dawn install
scripts/with-env cargo dev       # debug build with dev hooks
```

Tests: `scripts/with-env cargo test --workspace` (`GD_SKIP_DATA_TESTS=1` skips the tests that read the
install). Hooks, recipes and headless runs: [`docs/running.md`](docs/running.md).

## Layout
Thirteen crates, layered `gd-formats → gd-rules → {gd-sim ∥ gd-story} → gd-game-core →
{gd-game ∥ gd-render ∥ gd-ui-kit} → {gd-ui ∥ gd-world ∥ gd-audio} → grimdawn`, plus `gd-tools`. What each
holds and how the systems are structured: [`docs/architecture.md`](docs/architecture.md).

## Docs
- [`docs/architecture.md`](docs/architecture.md): crates, state flow, gating, session lifecycle, UI conventions
- [`docs/running.md`](docs/running.md): running the game, mods, test hooks and tests
- [`docs/reverse-engineering.md`](docs/reverse-engineering.md): how to decompile the original game
- [`docs/updating.md`](docs/updating.md): porting the project to a new game version
- [`PLAN.md`](PLAN.md): remaining work, known approximations and constraints

## AI notice
This port is developed primarily by AI coding agents (large language models) under human direction: the
code, documentation and commit messages are AI-written, reviewed and tested against the original game's
data and decompilation rather than taken from model memory. The human maintainer sets the goals, reviews
the work and owns the repository.

## Legal
This is a fan project. It is not affiliated with Crate Entertainment. Grim Dawn and its assets belong to
their owners.

The code is licensed under the [MIT License](LICENSE). That covers this repository only, not Grim Dawn's
data, which you need from your own copy of the game.
