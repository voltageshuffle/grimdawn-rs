# Architecture

## Crates
- **gd-formats**: one module per file format (`map/*`, `save/*`, ...). `vfs::Database` is the merged ARZ
  (base → gdx1 → gdx2 → gdx3 → mod), `vfs::Vfs` the merged ARC. The world is gdx3's `Levels.arc/world001.map`.
- **gd-sim**: engine rules with no Bevy: `expr` (equations), `stats`, `combat` (TakeAttack), `actor`
  (profiles; `rules`, `display`, `retaliation`), `skills`, `skill_tooltip`, `equip`, `nav`, `proxy` /
  `setpiece`, `items/*`, `story/*` (quests, dialogs, the `lua` host), `endless` (Shattered Realm). The full
  list is in `lib.rs`.
- **gd-game**: headless gameplay as Bevy ECS (no render, window or audio). `GameLogicPlugin` adds everything.
  `testing` holds the `TestGame` harness.
- **gd-render**: the game's look over gd-game: deferred shading, lighting, post (`render/`), models and
  textures, character bodies, particles (`fx/`), decals, water, trails, beams, see-through scenery, camera. It
  reads gameplay components and the presentation state the app writes. It has no gameplay of its own. Mesh
  parts spawn with their final material (`MaterialAssets` builds them, `MaterialCache` keeps the shader
  analyses and variants); grass detail reads the `GrassQuality` resource.
- **grimdawn**: the binary: world streaming (`world`), UI (`ui/`, `tooltip`), input, audio, weather and menu
  scene, plus the `dress_*` systems that give gameplay entities models and effects. It imports gd-game modules
  at the root (`use gd_game::{…}`), so `crate::x::Y` paths resolve inside the crate.

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
- `grimdawn::session::AppState { Menu, Loading, InGame }`. gd-game has no states. Data loads at startup.
- **Gating.** Every HUD, window and tooltip system is in `ui::UiPlaySet` (or one of its child sets
  `UiInputSet` / `HudSet`, never both). In gd-game, a system that needs a session resource takes a plain
  `Res` and is registered with `run_if(resource_exists::<..>)`. Use `Option<Res<..>>` only for genuinely
  optional data, with a comment saying why.
- **Ordering** goes through public sets (`InventorySet`, `PlayerSet`, `StoryStage`, `CharacterStage`, ...).
  A set nobody is in orders nothing, silently, so check that its `.in_set(..)` exists.
- **Session end** runs in process (`grimdawn/src/exit/mod.rs` + `gd_game::session::end_session_state`). It
  despawns the session's entities and removes or resets the session resources (the UI layout resets:
  `ui::reset_session`), then the menu starts again. New session state needs its reset there. A `Local` caching
  session data takes `gd_game::session::NewSession` and clears itself when `begun()`. Never despawn
  `IsResource` entities.
- **Hooks.** `GD_*` variables are parsed once into the `Hooks` resource (`gd_game::hooks`). Hook-only code is
  `dev`-only: `grimdawn/src/dev/` or `#[cfg(feature = "dev")]`.
- Use `try_despawn` / `try_insert` on entities that another system may have removed the same frame.

## UI (performance)
- `ui::rebuild` builds the HUD in root groups, each with a signature. A signature holds only what changes the
  layout, never a per-frame value: bars, numbers and cooldowns animate in place from `HudVitals`. Windows read
  `UiScaleOption` when they are built, not per frame.
- Dragging and scrolling move a container. They don't rebuild.
- Shared building blocks (use them, don't write another copy): `ui::contains`, `ui::cursor_px` (the one
  pointer read), `ui/rollover.rs::hud_rollover` (record-styled rollovers), `ui::tip` (skill-style tooltips),
  `tooltip.rs` (items), `ui::confirm::confirm_box`, `ui::widgets::static_button`, `ui::window::add_window`,
  `UiAssets::ui(..)`, `ui::spawn_root`.
- Input: one Esc dispatcher (`ui::esc_dispatcher`, `UiInputSet`) walks the open modals and the focus stack,
  closes the frontmost and consumes the press, and opens the game menu when none is open; the PreUpdate input
  systems that swallow the keyboard leave Escape for it (`ui::keys::reset_but_escape`). Buttons activate on a
  left press (`ui::widgets::button_pressed`, sub_140124D90 / sub_1400A01D0). The shared window chrome
  (`ui::window`): `add_window`, the `WindowPolicy` open/close table, `close_button` and the z bands. The UI reads
  `Inventory` immutably (`Inventory::info` is `&self`, an interior cache).

## Bevy notes
- At most 16 system params: group them with tuples or a `SystemParam` (`Session`, `ContainerLoot`,
  `creature::Spawner`). `clippy::type_complexity` is allowed crate-wide in gd-game, gd-render and grimdawn.
- Gameplay never draws from `fastrand::*`: use `gd_game::rng` (seeded per `TestGame`) or the engine's
  `gd_sim::combat::Rng`. `std` hash maps iterate in random order: sort where order matters.
