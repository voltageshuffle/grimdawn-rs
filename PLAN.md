# Improvement plan — remaining work

This file keeps only what is left and the constraints future work must respect; implemented work lives in
git history and the module doc comments.

## Do not attempt
- collections/util base crate (zero parallelism; everyone already depends on gd-formats).
- dress cluster crate (drags ui/ along: loot.rs pulls `gd_ui_kit::text::FontCache` / `gd_ui::character`).
- `gd-formats-ui` split (`ui`/`ssh`/`pfx`/`fnt`/`wav` out of gd-formats) and `gd-render-pipeline` /
  `gd-render-scene` split: measured off the critical path (clean workspace `cargo dev-build --timings`, 12
  threads: gd-formats 4 s; gd-render ends at 43 s while gd-game runs 8–84 s and the binary 84–112 s). The
  critical path is gd-sim → gd-game-core → gd-game → grimdawn; only those would pay off.
- save/load cluster crate (pulls most session state; breaks layering for little gain).
- `items/` alone out of gd-sim (depends on attributes/catalog/expr/stats/combat; depended on by actor,
  skills, story).
- **CastingSet parallelization (ECS-1)**: proven unsafe — the combat core and ~30 casting systems mutate the
  same components on the same entities and share the thread-local RNG draw order; none is provably disjoint.
  CombatSet ordering is already data-driven (full-state-hash verified).

## Constraints for any future change
- **Layering:** `gd-formats → gd-rules → {gd-sim ∥ gd-story} → gd-game-core → {gd-game ∥ gd-render ∥
  gd-ui-kit} → {gd-ui ∥ gd-world ∥ gd-audio} → grimdawn`. A lower crate never uses a higher one; gd-audio
  reads the map (`MapLayout`) and the menu (`MenuBackdrop`) from below, never gd-world.
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
- **Compile time:** the crate splits give the structural win (gd-render ∥ gd-game; gd-ui ∥ gd-world ∥
  gd-audio; edit isolation — a rules edit no longer rebuilds story/mlua, a systems edit no longer rebuilds
  render, a UI edit rebuilds gd-ui not the binary, a streaming or sound edit rebuilds only its crate and the
  binary). `-Zthreads=8` measured ~3% net but mixed per-crate (helps gd-game/core, hurts gd-ui/kit/render) —
  not recommended globally. Re-measure with `cargo dev-build --timings`.
- **Lua engine quirks preserved on purpose** (do not "fix"): `Character.Turn` no-ops (the glue passes
  ms=0), `Player.GetPlayerName` returns "", `Object.Cache/Release` are headless no-ops; overload dispatch
  stays lenient.
