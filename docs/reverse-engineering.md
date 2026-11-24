# Reverse engineering

Use it when stock data doesn't answer a gameplay question. Don't use it to chase visual parity.

## Setup
- Binaries: `$GD_ROOT/x64/{Grim Dawn.exe, Engine.dll, Game.dll}`. Exports are MSVC-mangled `GAME::` C++,
  so most classes and methods are named.
- The RE folder lives outside the repo, one subfolder per game version (`<re>/1_3_1_2`). `GD_RE_DIR` in
  `local.env` points at the version under work. Each holds IDA databases in `idb/*.i64` (shared: never close
  another agent's instance; `dsm.list()` first), decompile notes in `notes/`, extracted archives / record
  dumps (`gd-tools arc-extract` / `arz-dump` regenerate them).
- Game installs follow the same scheme (`gd_1_3_1_2`); `GD_ROOT` points at the current one.
- A game update: [`updating.md`](updating.md). Keep the previous version only while porting. The new version's
  notes start empty; read the old version's notes for anything not yet re-checked.
- `$GD_RE_DIR/symbols.tsv` is the version's symbol table ("Citing the game").
- Where things are: gameplay (combat, skills, AI, items, saves) is in **Game.dll**. Formats, rendering,
  animation and world are in **Engine.dll**. UI layout and drawing are in **Grim Dawn.exe**.

## Citing the game
Behavior lives in the module doc comments; build-specific locations live in one table per game version, so an
update changes the table, never the comments.
- Comments cite **names only**, never addresses, IDA auto-names (`sub_…`), or in-memory offsets / vtable /
  option indices: they move between builds. File-format offsets (`gd-formats`) are data layout and stay.
  - Functions: `` `Class::Method` `` (exports demangled, without `GAME::`; e.g. `Skill::StartCooldown`).
  - An unnamed function gets a name of ours in the same form, after what it does (`Skill::RefreshBuffDuration`).
  - Struct members: `` `Class.member` `` (ours, camelCase). Vtable slots: the virtual's name, as a function.
  - Options: the option's own name (`evadeFollowCursor`).
- `$GD_RE_DIR/symbols.tsv`, tab-separated: `name  kind  module  value  origin`. `kind` is `fn`, `global`,
  `field`, `vslot` or `opt`; `value` is the address, `+offset` or index in that build; `origin` is `export`
  (the binary's own name), `idb` (named in the IDA database) or `ours`. Names of ours are applied to the IDA
  database too, so the decompile reads the same as the comments.
- A new citation adds its row. `scripts/with-env scripts/check-re-refs` fails on addresses in `crates/` and lists
  cited names the table lacks.
- A game update: copy the old table, re-resolve each row in the new databases (`export` rows by name, the others
  by callers, strings or code shape), and re-check only the behavior the patch notes and records diff touch.

## Rules
- Check the module doc comments and `$GD_RE_DIR/notes` before opening IDA.
- Query narrowly: one function per call, with output capped. Write a long decompile to the notes folder and
  grep it.
- Findings go into the implementing module's doc comment.

## Facts that are easy to get wrong
- Coordinates are Y-up, in metres; characters face +Z; mesh triangles are clockwise; `.anm` quaternions are
  stored conjugated, as deltas on bind-local; lighting is gamma-space.
- Equations: shunting-yard, `^` left-associative, f32; unresolved variables → 0 (`gd-sim/src/expr.rs`).
- Save `lastDifficulty` = `difficulty | ((mode << 2) | challenge) << 4`. Challenge 1 = Veteran (Normal) and
  3 = Ultimate Veteran (Ultimate). Mode 1 = Crucible.
- Stats pass order: bio → skills → item sets → game balance → mutators → equipment. Game-balance pak
  column = difficulty × 4 + players − 1.
- Combat: Cunning scales physical / pierce (÷245, DoT ÷215), Spirit scales the rest (÷215, DoT ÷200); 70 %
  absorption.
