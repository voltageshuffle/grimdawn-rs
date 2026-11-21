# Reverse engineering

Use it when stock data doesn't answer a gameplay question. Don't use it to chase visual parity.

## Setup
- Binaries: `$GD_ROOT/x64/{Grim Dawn.exe, Engine.dll, Game.dll}`. Exports are MSVC-mangled `GAME::` C++,
  so most classes and methods are named.
- The RE folder lives outside the repo (`GD_RE_DIR` in `local.env`): IDA databases in `idb/new/*.i64`
  (shared: never close another agent's instance; `list_databases` first), decompile notes in `notes/`, extracted
  archives / record dumps (`gd-tools arc-extract` / `arz-dump` regenerate them).
- Where things are: gameplay (combat, skills, AI, items, saves) is in **Game.dll**. Formats, rendering,
  animation and world are in **Engine.dll**. UI layout and drawing are in **Grim Dawn.exe**.

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
