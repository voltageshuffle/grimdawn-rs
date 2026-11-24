# Improvement plan — remaining work

This file keeps only what is left and the constraints future work must respect; implemented work lives in
git history and the module doc comments.

## Remaining work (optional crate splits)

### SPLIT-4 — `gd-audio` + `gd-world` (modest compile win) — M, low–med
- `gd-world`: `world.rs` (~1.9k LOC, + `decoration_anim.rs` if its record-components move). Seams:
  `crate::audio::PeriodicSound` → gd-audio; `DecorationAnims/Floater` → gd-render; `ui::localmap::MapPoint`
  → gd-game::world. Reverse deps (menu_scene, menu, heap, exit, session, doors, riftgates, audio/sessions,
  ui/localmap, dev) mostly read `StreamedMap/Levels/Nav/TravelPreload/Lingering` — fine once world is a crate.
- `gd-audio`: `audio/` (~3.5k LOC). Seams: `crate::story::{StoryEffect,StoryActor}` (thin wrappers down or
  messages), `crate::doll::DollBody` + `crate::menu_scene::MenuScene` (small seam resources),
  `crate::world::StreamedMap` (used only for `w.map` — read `gd_game_core::save::MapLayout`, the same
  `Arc<MapFile>`, instead).
- Both depend on gd-render ⇒ they compile in parallel **with each other**, not with render; ~5.4k LOC leaves
  the binary. The dress cluster is **not viable** (loot.rs pulls `gd_ui_kit::text::FontCache`/`gd_ui::character`).

### SPLIT-5 — `gd-formats-ui` (only if gd-formats measures hot) — M, low
`ui.rs` + `ssh.rs` + `pfx.rs` + `fnt.rs` + `wav.rs` (~1.9k LOC); imports only `crate::arz::Record` +
`crate::util`; no gd-sim/gd-game consumers. Consumers: gd-render (material ssh, fx pfx), grimdawn (hover ui,
audio wav, fonts), gd-tools. Compiles in parallel with gd-sim. `save/`, `map/`, `qst/cnv` **cannot** leave
gd-formats (gd-sim uses them).

### SPLIT-6 — `gd-render-pipeline` vs `gd-render-scene` (lower priority)
render/ pipeline cluster (~10k: material/post/env/shadow/lights/terrain/grass) leans mostly on
gd-formats+bevy; scene cluster (model/character/fx/water/decal/trails/beams) on gd-game-core. Only worth it
if timings still show gd-render on the critical path.

## Do not attempt
- collections/util base crate (zero parallelism; everyone already depends on gd-formats).
- dress cluster crate (drags ui/ along).
- save/load cluster crate (pulls most session state; breaks layering for little gain).
- `items/` alone out of gd-sim (depends on attributes/catalog/expr/stats/combat; depended on by actor,
  skills, story).
- **CastingSet parallelization (ECS-1)**: proven unsafe — the combat core and ~30 casting systems mutate the
  same components on the same entities and share the thread-local RNG draw order; none is provably disjoint.
  CombatSet ordering is already data-driven (full-state-hash verified).

## Constraints for any future change
- **Layering:** `gd-formats → gd-rules → {gd-sim ∥ gd-story} → gd-game-core → {gd-game ∥ gd-render ∥
  gd-ui-kit} → gd-ui → grimdawn`. A lower crate never uses a higher one.
- **One source per value**; the **UI only reads** (request messages, never mutates Inventory/Stash/
  PlayerProgress); **session state = resources**, reset in `grimdawn/src/exit/mod.rs` (exhaustive,
  test-enforced by `exit/tests.rs::every_registered_resource_is_reset_or_kept` — add new crates to its
  CRATES + scan list and retarget KEEP entries).
- **Parity:** combat f32 operation order and RNG draw order are pinned. Verify with gd-sim
  `tests/sim/stats_db.rs`, gd-game `tests/game/determinism.rs`, and seeded `TestGame` full-state hashes
  before/after. Never edit a test to make it pass.
- **Preserve:** per-reader spatial grids; signature-based UI rebuilds (HudSig/TradeSig/CharacterSig exclude
  per-frame values); centralized Esc dispatcher + WindowPolicy; pooled particle materials; weak caches +
  Fresh/Recent (no timed eviction); backgrounded streaming; `Without<Dressed>` markers; gd-formats
  fail-closed parsing + LZ4/dimension caps; vfs merge precedence.
- **Build/test:** `cargo dev-build` / `cargo dev`; `cargo test --workspace`; `cargo clippy --workspace
  --all-targets` (+ `-p grimdawn -p gd-game --features dev`). `GD_UI_OPEN` smoke for UI. No new tests except
  save-format changes.
- **Compile time:** the crate splits give the structural win (gd-render ∥ gd-game; edit isolation — a rules
  edit no longer rebuilds story/mlua, a systems edit no longer rebuilds render, a UI edit rebuilds gd-ui not
  the binary). `-Zthreads=8` measured ~3% net but mixed per-crate (helps gd-game/core, hurts gd-ui/kit/
  render) — not recommended globally. Re-measure with `cargo dev-build --timings`.

## Modding — open questions (verify against the exe before changing)
- Mod database enumeration: only `<folder>.arz` loads (`gd-formats/src/vfs.rs`); engine `LoadDatabase` may
  enumerate the folder (mods ship multiple `.arz`). At minimum warn on unmatched `.arz` in the folder.
- Base `Database::open` loads every `*.arz` per layer vs the engine's named files; a stray `.arz` would
  override stock. Match named files or document the divergence.
- Crucible toggle has no DLC/install check (`grimdawn/src/ui/mainmenu.rs`); verify engine `GameModeWindow`
  gating before adding one.
- LZ4 amplification is bounded at 255× (`gd-formats/src/util.rs`); an optional cap for hostile mods.

## Lua — known approximations (future work)
- `GetAttachedCoords`: mesh-space translation only (no bone animation/rotation).
- `PlayAnimation`'s fourth bool is stored; its engine reader is untraced. AI scripted-busy checks test
  `Scripted` only, so a held monster may be interrupted.
- `Item.HasAffix` bridges record → first inventory instance (port items carry no object ids).
- `BreakDestructible` dir/force are not consumed by port debris; `Say` reuses the quick-tip box look
  (`records/sandbox/paul/speechwindow.dbr` is not shipped).
- `Debug.AddStatisticText` raises a Lua error: no engine counterpart found.

Engine quirks preserved on purpose (do not "fix"): `Character.Turn` no-ops (the glue passes ms=0),
`Player.GetPlayerName` returns "", `Object.Cache/Release` are headless no-ops; overload dispatch stays lenient.
