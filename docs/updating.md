# Updating to a new game version

The port runs on the install's data, so most of a patch arrives for free. The work is finding the few rules that
changed in the binary, and checking which of them we actually share. Work in this order; each step narrows the
next. A full update is a few hours, most of it waiting on IDA.

## 1. Install side by side
One folder per version (`gd_1_3_1_2`), one RE subfolder per version (`<re>/1_3_1_2`). Point `GD_ROOT` and
`GD_RE_DIR` in `local.env` at the new ones. **Keep the previous version until the port is done** — every step
below compares against it. Delete it afterwards.

The exe ships behind Steam's DRM wrapper: its `.text` is encrypted (entropy ≈ 8.0) and IDA finds only a few
thousand functions. Strip the wrapper (Steamless) and analyse the unpacked exe, or the exe half of the update
cannot be done. The DLLs need no unpacking. Check a database is fully analysed before trusting it: a healthy
`Grim Dawn.exe` has ~15.7k functions, `Game.dll` ~31k, `Engine.dll` ~14k.

## 2. Diff the records
```sh
scripts/with-env cargo run --release -p gd-tools --bin gd-rec -- \
  "$GD_ROOT" --diff <old install> > "$GD_RE_DIR/notes/records-diff-<old>.txt"
```
Triage the diff, not the patch notes:
- **New field names on existing records** — the only part that can need code. Grep each name in `crates/`;
  a name we never read is a candidate. (1.3.1.2: `correctParticles`, `removeOnUnload`.)
- **`class:` changes** — a record moving to a class we may not handle on that path. (1.3.1.2: a pet heal
  `Skill_BuffOther` → `Skill_BuffRadius`, a boss projectile → `Skill_AttackProjectileBurst`.)
- **Added / removed records** — usually content. Removed ones are often empty stubs.
- Everything else is values in fields we already read: no work.

## 3. Get the real change list from the binaries
The patch notes describe intent; the changed functions describe what happened. Dump both versions' functions
and pair them:
```sh
# per version, per module — on a COPY of the .i64, never one another agent has open
DUMP_OUT=<dir>/<version>/Game.dll.jsonl idat -A -Sscripts/re-features.py <copy>.i64
scripts/re-match <dir> <old> <new> --symbols <re>/<old>/symbols.tsv <re>/<new>/symbols.tsv
```
`re-match` anchors on names both builds export, then grows the pairing by instruction hash, size, strings and
the call graph. It writes:
- **`changed.tsv`** — the functions whose code differs. This is the update's work list. 1.3.1.2 changed **50**
  functions out of ~60k; reading those beats reading the patch notes.
- **`unmatched.tsv`** — old functions with no counterpart.
- the new **`symbols.tsv`**, with `fn` / `global` rows re-addressed. Rows it cannot place keep `?`; place those
  by hand from callers, strings and constants (1.3.1.2: 98 of ~3.4k). Field, vtable and option rows carry over
  **unchecked** — offsets move; verify one when a change touches it.

Sanity-check the matcher on a build pair you already know: hide a fifth of the names the old database has and
see how many it re-finds correctly. On 1.3.1.2 it was 2145/2150 (Game.dll) and 757/758 (Engine.dll).

Then write our names into the new databases so the decompiles read like the comments:
```sh
MODULE=Game.dll TABLE=<re>/<new>/symbols.tsv idat -A -Sscripts/re-apply-names.py <re>/<new>/idb/Game.dll.i64
```
Run modules one at a time. The script must not call `save_database` itself: `idat -A` already saves on exit,
and saving twice corrupts the `.i64`.

## 4. Decide what we actually share
**Their bug is not necessarily our bug.** We restructured the engine; a fix to their plumbing may have no
counterpart here. For each changed function, read the old and new decompile, then find our equivalent and ask
whether the wrong behaviour is reachable. Write the answer down either way.

1.3.1.2 examples:
- *Duration refresh* — a real rule change; we had the old rule. Ported.
- *Werewolf / Avalanche weapon damage* — their fix passes the animation name into the hit-callback lookup.
  Our weapon damage comes from hand states and the profile, never from the callback, so we never had the bug.
- *`removeOnUnload` on traps* — we already despawn every monster of an unloaded level.
- *Pet resists missing from the sheet* — our sheet derives from the pet profile each rebuild.

Only port what is reachable. A new visual field is not worth a renderer change unless our version looks wrong
(`AGENTS.md`, priority 3).

## 5. Verify and finish
- `scripts/with-env scripts/check-re-refs` against **both** tables: no addresses left in comments, no cited
  name missing from the table.
- `cargo clippy --workspace --all-targets`, then `scripts/with-env cargo test --workspace` on the new data.
- Bump `game-version` under `[workspace.metadata.grimdawn]` in the root `Cargo.toml` and the version in `README.md`:
  release titles and notes name it, and `scripts/release-notes` refuses to run while the two differ.
- New findings go in the implementing module's doc comment; new names go in the table
  (`docs/reverse-engineering.md`, "Citing the game"). A function new in this version gets a `gone` row in the
  old table.
