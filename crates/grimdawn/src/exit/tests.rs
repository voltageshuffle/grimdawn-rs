//! Every resource of the two game crates is either reset / removed when the session ends or listed in
//! [`super::KEEP`]: a new resource has to be classified, or the next Exit to Main Menu leaves its state behind (a
//! paused clock, a frozen preview, a cache built for the other difficulty).
//!
//! The resources are the types that derive `Resource` plus whatever a plugin registers by type (the
//! `init_resource` / `insert_resource` families and the non-send forms; Bevy's generic ones are skipped). A
//! reset is a call site inside one of the teardown functions ([`HANDLERS`]): the `remove_resource`,
//! `remove`, `reset` and `insert_resource` families. A mention anywhere else (a registration, a system
//! parameter, a comment) does not count, and neither does a partial reset through `resource_mut`: such a
//! resource goes in `KEEP` with the reason. `KEEP` is checked both ways: no entry that is also reset, none
//! that names no resource.
//!
//! Not seen here: a system `Local` that holds session data (it takes `NewSession`, module docs).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The teardown functions: (module, signature start).
const HANDLERS: &[(&str, &str)] = &[
    ("grimdawn::exit", "fn end_session("),
    ("gd_game::session", "fn end_session_state("),
    ("grimdawn::ui", "fn reset_session("),
    // `OnExit(AppState::InGame)`.
    ("grimdawn::ui::loading", "fn teardown_hud("),
    // Run by `end_session` (through `open_world`): the map's resources.
    ("grimdawn::world", "fn install_world("),
];

/// One source file: its module path (`grimdawn::ui::loading`) and its text.
struct Src {
    module: String,
    text: String,
}

/// The sources below `dir` (the `src` of crate `krate`), without their unit tests (the
/// `#[cfg(test)] mod` at the end of a file, a `tests.rs` file) and gd-game's test harness (`testing.rs`, the
/// `testing` feature): what runs in the game.
fn sources(krate: &str, root: &Path, dir: &Path, out: &mut Vec<Src>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(krate, root, &p, out);
        } else if p.extension().is_some_and(|x| x == "rs")
            && !p
                .file_stem()
                .is_some_and(|s| s == "tests" || s == "testing")
        {
            let mut text = std::fs::read_to_string(&p).unwrap();
            if let Some(i) = text.find("#[cfg(test)]\nmod ") {
                text.truncate(i);
            }
            let rel = p.strip_prefix(root).unwrap().with_extension("");
            let mut module = vec![krate.to_string()];
            module.extend(rel.iter().map(|s| s.to_string_lossy().into_owned()));
            if matches!(
                module.last().map(String::as_str),
                Some("mod" | "lib" | "main")
            ) && module.len() > 1
            {
                module.pop();
            }
            out.push(Src {
                module: module.join("::"),
                text,
            });
        }
    }
}

fn ident(s: &str) -> &str {
    let end = s
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(s.len());
    &s[..end]
}

fn last(path: &str) -> &str {
    path.rsplit("::").next().unwrap()
}

/// The path at the start of `s` up to the type it names, its first upper-case segment (`a::b::Type`
/// of `a::b::Type::default()`, `Type { .. }`, `Type(..)`). `None` for a variable and for a generic type
/// (Bevy's `Time<Fixed>`, `Assets<T>`).
fn type_at(s: &str) -> Option<String> {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != ':')
        .unwrap_or(s.len());
    if s[end..].starts_with('<') {
        return None;
    }
    let segs: Vec<&str> = s[..end].split("::").collect();
    let at = segs
        .iter()
        .position(|seg| seg.starts_with(char::is_uppercase))?;
    Some(segs[..=at].join("::"))
}

/// The names `src`'s `use` items bring in, to the path they name as written (`a::{b, c::D as E}`).
fn uses(src: &str) -> BTreeMap<String, String> {
    fn flatten(prefix: &str, tree: &str, out: &mut BTreeMap<String, String>) {
        let tree = tree.trim();
        if let Some(open) = tree.find('{') {
            let head = tree[..open].trim_end_matches("::");
            let prefix = [prefix, head]
                .iter()
                .filter(|p| !p.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("::");
            let inner = &tree[open + 1..tree.rfind('}').unwrap()];
            let (mut depth, mut start) = (0, 0);
            for (i, c) in inner.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        flatten(&prefix, &inner[start..i], out);
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            flatten(&prefix, &inner[start..], out);
            return;
        }
        let (path, alias) = match tree.split_once(" as ") {
            Some((p, a)) => (p.trim(), Some(a.trim())),
            None => (tree, None),
        };
        if path.is_empty() {
            return;
        }
        if let Some(glob) = path.strip_suffix("::*") {
            let full = [prefix, glob]
                .iter()
                .filter(|p| !p.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("::");
            out.insert(format!("*{full}"), full);
            return;
        }
        let full = match (prefix.is_empty(), path) {
            (false, "self") => prefix.to_string(),
            (true, _) => path.to_string(),
            (false, _) => format!("{prefix}::{path}"),
        };
        let name = alias.unwrap_or_else(|| last(&full)).to_string();
        out.insert(name, full);
    }
    let mut out = BTreeMap::new();
    for line in src.lines() {
        let l = line.trim_start();
        let l = l
            .strip_prefix("pub(crate) ")
            .or_else(|| l.strip_prefix("pub "))
            .unwrap_or(l);
        let Some(rest) = l.strip_prefix("use ") else {
            continue;
        };
        let at = rest.as_ptr() as usize - src.as_ptr() as usize;
        let item = src[at..].split(';').next().unwrap_or("");
        flatten("", item, &mut out);
    }
    out
}

/// The crates a path may start with as written.
const CRATES: &[&str] = &[
    "gd_formats",
    "gd_sim",
    "gd_game",
    "gd_render",
    "grimdawn",
    "bevy",
    "std",
];

/// What resolving a written path needs: each module's types and `use` items, and the resources.
struct Ctx {
    /// Every `struct` / `enum` of each module, by name.
    defs: BTreeMap<String, BTreeSet<String>>,
    uses: BTreeMap<String, BTreeMap<String, String>>,
    derived: BTreeSet<String>,
}

impl Ctx {
    fn new(files: &[Src]) -> Self {
        let mut ctx = Ctx {
            defs: BTreeMap::new(),
            uses: BTreeMap::new(),
            derived: BTreeSet::new(),
        };
        for f in files {
            let words: Vec<&str> = f.text.split_whitespace().collect();
            let defs = ctx.defs.entry(f.module.clone()).or_default();
            for w in words.windows(2) {
                if w[0] == "struct" || w[0] == "enum" {
                    defs.insert(ident(w[1]).to_string());
                }
            }
            ctx.uses
                .entry(f.module.clone())
                .or_default()
                .extend(uses(&f.text));
            derived(f, &mut ctx.derived);
        }
        ctx
    }

    fn defines(&self, module: &str, name: &str) -> bool {
        self.defs.get(module).is_some_and(|d| d.contains(name))
    }

    /// The module path `segs` names, written in `module`.
    fn module_path(&self, module: &str, segs: &[&str], depth: u32) -> Vec<String> {
        let mut base: Vec<String> = module.split("::").map(String::from).collect();
        let Some((&first, rest)) = segs.split_first() else {
            return base;
        };
        let mut path = match first {
            "crate" => vec![base[0].clone()],
            "self" => base,
            "super" => {
                base.pop();
                return self.module_path(&base.join("::"), &[&["self"], rest].concat(), depth);
            }
            first if CRATES.contains(&first) => vec![first.to_string()],
            first => match self.uses.get(module).and_then(|u| u.get(first)) {
                Some(u) if depth < 8 => {
                    let u: Vec<&str> = u.split("::").collect();
                    self.module_path(module, &u, depth + 1)
                }
                _ => {
                    base.push(first.to_string());
                    base
                }
            },
        };
        path.extend(rest.iter().map(|s| s.to_string()));
        path
    }

    /// The full path of the type `written` names in the file of `module`. A name that only an outside
    /// glob brings in (`bevy::prelude::*`) stays as written unless exactly one resource has that name.
    fn resolve(&self, module: &str, written: &str) -> String {
        self.resolve_at(module, written, 0)
    }

    fn resolve_at(&self, module: &str, written: &str, depth: u32) -> String {
        let segs: Vec<&str> = written.split("::").collect();
        let (name, prefix) = segs.split_last().unwrap();
        if !prefix.is_empty() {
            let parent = self.module_path(module, prefix, depth).join("::");
            if depth < 8 && self.uses.contains_key(&parent) {
                // Through the parent's own items and re-exports (`pub use gd_game::teleport::PendingTravel`).
                let full = self.resolve_at(&parent, name, depth + 1);
                if full.contains("::") {
                    return full;
                }
            }
            return format!("{parent}::{name}");
        }
        if self.defines(module, name) {
            return format!("{module}::{name}");
        }
        let used = self.uses.get(module);
        if let Some(u) = used.and_then(|u| u.get(*name))
            && depth < 8
        {
            let path: Vec<&str> = u.split("::").collect();
            let (_, prefix) = path.split_last().unwrap();
            let parent = self.module_path(module, prefix, depth).join("::");
            return self.resolve_at(&parent, &format!("self::{}", last(u)), depth + 1);
        }
        // The module's globs (`pub use gd_game::session::*`, `pub use self::leaps::*`).
        for (k, glob) in used.into_iter().flatten() {
            if !k.starts_with('*') || depth >= 8 {
                continue;
            }
            let g: Vec<&str> = glob.split("::").collect();
            let parent = self.module_path(module, &g, depth).join("::");
            if self.uses.contains_key(&parent) {
                let full = self.resolve_at(&parent, name, depth + 1);
                if full.contains("::") {
                    return full;
                }
            }
        }
        if depth > 0 {
            return name.to_string();
        }
        let same: Vec<_> = self.derived.iter().filter(|d| last(d) == *name).collect();
        match same[..] {
            [one] => one.clone(),
            _ => name.to_string(),
        }
    }
}

/// The types `src` derives `Resource` for, by full path.
fn derived(src: &Src, out: &mut BTreeSet<String>) {
    let text = &src.text;
    for (i, _) in text.match_indices("#[derive(") {
        let rest = &text[i..];
        let Some(end) = rest.find(")]") else { continue };
        if !rest[..end]
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "Resource")
        {
            continue;
        }
        // The item under the derive, past further attributes and comments.
        let item = rest[end + 2..]
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with("#[") && !l.starts_with("//"));
        let name = item.and_then(|l| {
            l.split_whitespace()
                .skip_while(|w| *w != "struct" && *w != "enum")
                .nth(1)
        });
        if let Some(name) = name {
            out.insert(format!("{}::{}", src.module, ident(name)));
        }
    }
}

/// The types `src` registers by name.
fn registered(ctx: &Ctx, src: &Src, out: &mut BTreeSet<String>) {
    for pat in [
        "init_resource::<",
        "init_non_send::<",
        "insert_resource(",
        "insert_non_send(",
    ] {
        for (i, _) in src.text.match_indices(pat) {
            if let Some(ty) = type_at(&src.text[i + pat.len()..]) {
                out.insert(ctx.resolve(&src.module, &ty));
            }
        }
    }
}

/// The body of the function whose signature starts with `sig`.
fn body<'a>(src: &'a str, sig: &str) -> &'a str {
    let at = src
        .find(sig)
        .unwrap_or_else(|| panic!("teardown function `{sig}` not found"));
    let open = at + src[at..].find('{').unwrap();
    let mut depth = 0;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..open + i];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after `{sig}`");
}

/// The types the teardown function `sig` of `src` removes or replaces, by full path.
fn resets(ctx: &Ctx, src: &Src, sig: &str, out: &mut BTreeSet<String>) {
    let body = body(&src.text, sig);
    for pat in [
        "remove_resource::<",
        "remove_non_send::<",
        "remove::<",
        "reset::<",
        "insert_resource(",
        "insert_non_send(",
    ] {
        for (i, _) in body.match_indices(pat) {
            let arg = body[i + pat.len()..].trim_start();
            let ty = type_at(arg).or_else(|| {
                // A value built earlier in the function: `let world = StreamedMap { .. }`.
                let bound = format!("let {} = ", ident(arg));
                body.find(&bound)
                    .and_then(|j| type_at(&body[j + bound.len()..]))
            });
            if let Some(ty) = ty {
                out.insert(ctx.resolve(&src.module, &ty));
            }
        }
    }
}

/// (every resource, the ones the teardown functions `handlers` (module, signature) reset), by full path.
fn scan(files: &[Src], handlers: &[(&str, &str)]) -> (BTreeSet<String>, BTreeSet<String>) {
    let ctx = Ctx::new(files);
    let mut all = ctx.derived.clone();
    for f in files {
        registered(&ctx, f, &mut all);
    }
    let mut reset = BTreeSet::new();
    for (module, sig) in handlers {
        let src = files
            .iter()
            .find(|f| f.module == *module && f.text.contains(sig))
            .unwrap_or_else(|| panic!("no `{sig}` in {module}"));
        resets(&ctx, src, sig, &mut reset);
    }
    (all, reset)
}

#[test]
fn every_registered_resource_is_reset_or_kept() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for (krate, dir) in [
        ("gd_game", "gd-game"),
        ("grimdawn", "grimdawn"),
        ("gd_render", "gd-render"),
    ] {
        let src = root.join(dir).join("src");
        sources(krate, &src, &src, &mut files);
    }
    let (all, reset) = scan(&files, HANDLERS);
    let keep: BTreeSet<String> = super::KEEP.iter().map(|k| k.to_string()).collect();
    assert_eq!(keep.len(), super::KEEP.len(), "KEEP has a duplicate");

    let missing: Vec<_> = all
        .iter()
        .filter(|t| !reset.contains(*t) && !keep.contains(*t))
        .collect();
    assert!(
        missing.is_empty(),
        "resources neither reset on exit nor in KEEP (exit/mod.rs): {missing:?}"
    );
    let both: Vec<_> = keep.intersection(&reset).collect();
    assert!(
        both.is_empty(),
        "KEEP entries that a teardown function resets: {both:?}"
    );
    let stale: Vec<_> = keep.difference(&all).collect();
    assert!(
        stale.is_empty(),
        "KEEP entries that name no resource: {stale:?}"
    );
}
