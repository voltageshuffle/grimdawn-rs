# Architecture

## Crates
- **gd-formats**: one module per file format (`map/*`, `save/*`, ...). `vfs::Database` is the merged ARZ
  (base → gdx1 → gdx2 → gdx3 → mod), `vfs::Vfs` the merged ARC. The world is gdx3's `Levels.arc/world001.map`.
- **gd-rules**: Bevy-free primitives shared by gd-sim and gd-story: `expr` (equations), `mode`, `faction`, `Rng`.
- **gd-sim**: engine rules with no Bevy: `stats`, `combat` (TakeAttack), `actor` (profiles; `rules`, `display`,
  `retaliation`), `skills`, `skill_tooltip`, `equip`, `nav`, `proxy` / `setpiece`, `items/*`. The full list is in `lib.rs`.
- **gd-story**: the story / Lua host (`story/*`: quests, dialogs, the `lua` host) and Shattered Realm (`endless`);
  vendored mlua. Compiles in parallel with gd-sim.
- **gd-game-core**: gameplay data shared by gd-game and gd-render: components, resources, messages, pure
  resolvers and SystemSets — no systems.
- **gd-game**: headless gameplay as Bevy ECS (no render, window or audio). `GameLogicPlugin` adds everything.
  `testing` holds the `TestGame` harness.
- **gd-render**: the game's look over gd-game-core: deferred shading, lighting, post (`render/`), models and
  textures, character bodies, particles (`fx/`), decals, water, trails, beams, see-through scenery, camera. It
  reads gameplay components and the presentation state the app writes. It has no gameplay of its own. Mesh
  parts spawn with their final material (`MaterialAssets` builds them, `MaterialCache` keeps the shader
  analyses and variants); grass detail reads the `GrassQuality` resource.
- **gd-ui-kit**: the shared UI presentation layer over gd-game-core + gd-render (no gd-game): the record-driven
  widget builder (`Ui`, `UiAssets`), the font cache (`text`), common chrome (`widgets`, `tip`, `confirm`), key
  bindings (`keys`), the additive / masked UI materials, and the settings-file helpers / UI scale (`state::settings`).
- **gd-ui**: the in-game UI windows and HUD over gd-ui-kit + gd-game + gd-render: the movable window clusters
  (`hud`, `character`, `trade`, `skills`, `devotion`, `quests`, ...), the rebuild hub / Esc dispatcher (`lib.rs`),
  the window open/close manager (`window`), the paper-doll builders (`doll`), tooltips, and the presentation state
  (`state`) the binary's input / world systems share (Trade, InvLayout, CraftList, Hovered, AppState, MapWindow, ...).
  It reads gameplay and sends request messages; it never mutates Inventory / Stash / PlayerProgress.
- **grimdawn**: the binary: world streaming (`world`), input, audio, weather and menu scene, the UI windows coupled
  to them (`ui/`: options, localmap, riftmap, survival, credits, cinematic, mainmenu, quicktip, view), plus the
  `dress_*` systems that give gameplay entities models and effects. It imports gd-game modules at the root
  (`use gd_game::{…}`), so `crate::x::Y` paths resolve inside the crate.

## State and data flow
- **One source per value.** Raw state lives once (`Loadout`, `PlayerProgress`, `Inventory`, `Character`,
  `PlayerFactions`, ...); per-entity state lives in components, not resource maps (`Interactable`,
  `LevelMember`, `MeleeSlots`). Derived values come from one function or system, and readers query the
  component: `Loadout::effective_skills(db, &trees)` → `EffectiveSkills`, `Loadout::skill_level(db, &effective,
  skill)`, `rebuild_profile` on `ProfileDirty`, `sync_requirements` → `Requirements`. `SkillTrees` and
  `GameEngineRules` are built once per mounted database on `GameData` (`skill_trees()`, `engine()`) and
  dropped when the mod changes.
- **Requests.** UI and input decide what a click means and write a message, for example `SpendAttribute`,
  `RaiseSkill`, `CastRequest`, `InvRequest`, `BuyRequest`, `CraftRequest`, `AltarOffer`, `TakeStar` or
  `SaveRequest`. A gd-game system applies it (`.after(InventorySet).before(PlayerSet)`). Where the UI keeps
  state of its own, gd-game answers with a message (`SlotReturn`, `TradeDone`, `AltarAnswer`). Gameplay tells
  presentation through messages too (`EquipmentChanged`, `LeveledUp`, `PlaySound`, `MonsterKilled`, ...).
- **Dressing seam.** gd-game spawns data components (`Chest`, `Door`, `GroundItem`, `BodyRequest`,
  `visuals::EffectSpec`, ...). grimdawn adds models and effects on `Added<…>`.
- **Stable entities.** The player survives equipment changes. Only `gd_render::character::Body` is rebuilt.
- **Streaming despawns in `Last`.** Unloaded levels and what lives in them get `levels::Unload`, and
  `levels::despawn_unloaded` removes them after the frame's commands.
- **Asset lifetime.** Nothing is evicted on a timer: an asset goes when its last holder drops it. Path-keyed
  weak caches (`gd_game::weak_cache`, `gd_render::textures::WeakHandles`) find a live value again; `Recent`
  rings keep the last used ones warm. A model's joints despawn with it (`JointOf` relationship). After a level
  unload or a session end, `grimdawn/src/heap.rs` returns the allocator's free pages (`malloc_trim`).
- **Crucible and Shattered Realm.** The Crucible is a mod mounted over the install (`gd_formats::vfs` doc).
  `Choice::survival` is the single source of "this session is a Crucible": systems never look at the overlay
  to decide gameplay. The Shattered Realm is regions of `world001.map`, driven by `Story.endless` (gd-sim) and
  `gd-game/src/endless.rs`.

## App phases, gating and session lifecycle
- `gd_ui::state::session::AppState { Menu, Loading, InGame }` (the binary's `session.rs` mirrors the session
  resources into it). gd-game has no states. Data loads at startup.
- **Gating.** Every HUD, window and tooltip system is in `gd_ui_kit::UiPlaySet` (or one of its child sets
  `UiInputSet` / `HudSet`, never both). In gd-game, a system that needs a session resource takes a plain
  `Res` and is registered with `run_if(resource_exists::<..>)`. Use `Option<Res<..>>` only for genuinely
  optional data, with a comment saying why.
- **Ordering** goes through public sets (`InventorySet`, `PlayerSet`, `StoryStage`, `CharacterStage`, ...).
  A set nobody is in orders nothing, silently, so check that its `.in_set(..)` exists.
- **Session end** runs in process (`grimdawn/src/exit/mod.rs` + `gd_game::session::end_session_state`). It
  despawns the session's entities and removes or resets the session resources (the UI layout resets:
  `gd_ui::reset_session` + `grimdawn::ui::reset_session`), then the menu starts again. New session state needs its reset there. A `Local` caching
  session data takes `gd_game::session::NewSession` and clears itself when `begun()`. Never despawn
  `IsResource` entities.
- **Hooks.** `GD_*` variables are parsed once into the `Hooks` resource (`gd_game::hooks`). Hook-only code is
  `dev`-only: `grimdawn/src/dev/` or `#[cfg(feature = "dev")]`.
- Use `try_despawn` / `try_insert` on entities that another system may have removed the same frame.

## UI (performance)
- `gd_ui::rebuild` builds the HUD in root groups, each with a signature. A signature holds only what changes the
  layout, never a per-frame value: bars, numbers and cooldowns animate in place from `HudVitals`. Windows read
  `UiScaleOption` when they are built, not per frame.
- Dragging and scrolling move a container. They don't rebuild.
- Shared building blocks (use them, don't write another copy): `gd_ui_kit::contains`, `gd_ui_kit::cursor_px` (the one
  pointer read), `gd_ui::rollover::hud_rollover` (record-styled rollovers), `gd_ui_kit::tip` (skill-style tooltips),
  `gd_ui::tooltip` (items), `gd_ui_kit::confirm::confirm_box`, `gd_ui_kit::widgets::static_button`, `gd_ui::window::add_window`,
  `UiAssets::ui(..)`, `gd_ui_kit::spawn_root`.
- Input: one Esc dispatcher (`gd_ui::esc_dispatcher`, `UiInputSet`) walks the open modals and the focus stack,
  closes the frontmost and consumes the press, and opens the game menu when none is open; the PreUpdate input
  systems that swallow the keyboard leave Escape for it (`gd_ui_kit::keys::reset_but_escape`). Buttons activate on a
  left press (`gd_ui_kit::widgets::button_pressed`, sub_140124D90 / sub_1400A01D0). The shared window chrome
  (`gd_ui::window`): `add_window`, the `WindowPolicy` open/close table, `close_button` and the z bands. The UI reads
  `Inventory` immutably (`Inventory::info` is `&self`, an interior cache).

## Bevy notes
- At most 16 system params: group them with tuples or a `SystemParam` (`Session`, `ContainerLoot`,
  `creature::Spawner`). `clippy::type_complexity` is allowed crate-wide in gd-game, gd-render, gd-ui and grimdawn.
- Gameplay never draws from `fastrand::*`: use `gd_game::rng` (seeded per `TestGame`) or the engine's
  `gd_sim::combat::Rng`. `gd_formats::collections` hash maps iterate in a fixed but insertion-dependent
  order: sort where order matters.
