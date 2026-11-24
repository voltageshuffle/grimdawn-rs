# Architecture

Layering: `gd-formats → gd-rules → {gd-sim ∥ gd-story} → gd-game-core → {gd-game ∥ gd-render ∥ gd-ui-kit}
→ gd-ui → grimdawn`. A lower crate never uses a higher one. Module doc comments are the reference for
formats and rules; this file holds only the cross-cutting conventions.

## Crates
- **gd-formats**: file formats (`map/*`, `save/*`, ...) and the VFS: `vfs::Database` the merged ARZ
  (base → gdx1 → gdx2 → gdx3 → mod), `vfs::Vfs` the merged ARC.
- **gd-rules**: Bevy-free primitives shared by gd-sim and gd-story (`expr`, `mode`, `faction`, `Rng`).
- **gd-sim**: engine rules without Bevy (stats, combat, actor profiles, skills, items, nav, proxy/setpiece).
- **gd-story**: the story / Lua host and Shattered Realm generation (`endless`); vendored mlua.
- **gd-game-core**: gameplay data shared by gd-game and gd-render: components, resources, messages, pure
  resolvers, SystemSets — no systems.
- **gd-game**: headless gameplay ECS (`GameLogicPlugin`); the `TestGame` harness in `testing`.
- **gd-render**: the look over gd-game-core; no gameplay of its own.
- **gd-ui-kit**: shared UI presentation over gd-game-core + gd-render (no gd-game): record-driven widget
  builder, fonts, chrome, key bindings, adaptive layout helpers, settings/UI scale.
- **gd-ui**: the in-game windows and HUD over gd-ui-kit + gd-game + gd-render; the rebuild hub, Esc
  dispatcher, window manager and the presentation state the binary's input systems share.
- **grimdawn**: the binary: world streaming, input, audio, menu scene, the world/menu-coupled windows, and
  the `dress_*` systems that give gameplay entities models and effects.
- **gd-tools**: CLIs for inspecting game data (archives, records, maps, meshes, saves, story, mods).

## State and data flow
- **One source per value.** Raw state lives once; per-entity state lives in components, not resource maps.
  Derived values come from one function or system and readers query the component. `SkillTrees` and
  `GameEngineRules` are built once per mounted database on `GameData` and dropped when the mod changes —
  mount first, then invalidate, with a mount generation stamp so a concurrent reader cannot cache new-mod
  data under a stale mod key.
- **Requests.** UI and input decide what a click means and write a message; a gd-game system applies it.
  Where the UI keeps state of its own, gd-game answers with a message. Gameplay tells presentation through
  messages too. The UI never mutates `Inventory` / `Stash` / `PlayerProgress`.
- **Dressing seam.** gd-game spawns data components; grimdawn adds models and effects on `Added<…>`.
- **Stable entities.** The player survives equipment changes; only the render body is rebuilt.
- **Streaming despawns in `Last`.** Unloaded levels and what lives in them get `levels::Unload`;
  `levels::despawn_unloaded` removes them after the frame's commands.
- **Asset lifetime.** Nothing is evicted on a timer: an asset goes when its last holder drops it. Path-keyed
  weak caches (`gd_game_core::weak_cache`, `gd_render::textures::WeakHandles`) find a live value again;
  `Recent` rings keep the last used ones warm. After a level unload or session end the binary returns the
  allocator's free pages.
- **Crucible and Shattered Realm.** The Crucible is a mod mounted over the install; `Choice::survival` is the
  single source of "this session is a Crucible" — systems never look at the overlay to decide gameplay. The
  Shattered Realm is regions of the world map driven by `gd_story::endless` and `gd-game/src/endless.rs`.
  A start whose save's `map.dat` names a different mod is refused (`Choice::mod_mismatch`); the Crucible is
  exempt.
- **Determinism.** Gameplay draws only from `gd_game::rng` (seeded per `TestGame`) or `gd_sim::combat::Rng`,
  never `fastrand`. `gd_formats::collections` maps iterate in insertion-dependent order: sort where order
  matters.

## App phases, gating and session lifecycle
- `gd_ui::state::session::AppState { Menu, Loading, InGame }`; the binary mirrors the session resources into
  it. gd-game has no states. Data loads at startup.
- **Gating.** Every HUD, window and tooltip system is in `gd_ui_kit::UiPlaySet` (or one child set, never
  both). A gd-game system that needs a session resource takes a plain `Res` and is registered with
  `run_if(resource_exists::<..>)`; `Option<Res<..>>` only for genuinely optional data, with a comment.
- **Ordering** goes through public sets (`InventorySet`, `PlayerSet`, `StoryStage`, ...). A set nobody is in
  orders nothing, silently — check that its `.in_set(..)` exists.
- **Session end** runs in process (`grimdawn/src/exit/mod.rs` + `gd_game::session::end_session_state`):
  despawn the session's entities, remove or reset the session resources (including the UI layouts), then the
  menu starts again. New session state needs its reset there; the exit test enumerates every registered
  resource. A `Local` caching session data takes `NewSession` and clears itself when `begun()`. Never despawn
  `IsResource` entities.
- **Hooks.** `GD_*` variables are parsed once into `Hooks` (`gd_game_core::hooks`); hook-only code is
  `dev`-only. Use `try_despawn` / `try_insert` on entities another system may have removed the same frame.

## UI
- `gd_ui::rebuild` builds the HUD in root groups, each with a signature. A signature holds only what changes
  the layout, never a per-frame value: bars, numbers and cooldowns animate in place. Windows read the UI
  scale when built, not per frame. Dragging and scrolling move a container; they don't rebuild.
- Reuse the shared blocks instead of copying: gd-ui-kit's widgets / tip / confirm / layout helpers and
  gd-ui's window chrome, rollovers and item tooltips.
- Adaptive layout: pages are columns of rows placed from measured text (`gd_ui_kit::layout`); buttons grow to
  fit their label. FNTD (ja/ko/zh/vi) fonts measure with real TrueType metrics and wrap CJK-aware;
  single-line text never word-wraps.
- One Esc dispatcher (`gd_ui::esc_dispatcher`) walks the open modals and the focus stack, closes the
  frontmost and consumes the press, and opens the game menu when none is open; input systems that swallow
  the keyboard leave Escape for it. Buttons activate on a left press. Window open/close behaviour lives in
  the `WindowPolicy` table (`gd_ui::window`).