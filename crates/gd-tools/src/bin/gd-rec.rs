//! Print merged database records: `cargo run -p gd-tools --bin gd-rec -- <game dir> <record>...`
//! (`<game dir>` defaults to `GD_ROOT` when no argument is given).
//! `--mod <name>` mounts `mods/<name>` over the install (the Crucible packs for `survivalmode`);
//! `--classes <prefix>` counts the classes of records under a path prefix;
//! `--with <field> <prefix>` lists records there with a non-empty `field`;
//! `--diff <old game dir> [prefix]` lists the records added, removed and changed (field by field, old → new)
//! from the old install to this one: the record side of a game update. With `--mod` and no old dir,
//! `--diff [prefix]` compares the install against the mod.
use anyhow::Context;
use gd_formats::arz::RawValue;
use gd_formats::testdata::game_dir;
use gd_formats::vfs::Database;
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut mod_name = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--mod" {
            anyhow::ensure!(i + 1 < args.len(), "--mod needs a mod name");
            mod_name = Some(args.remove(i + 1));
            args.remove(i);
        } else {
            i += 1;
        }
    }
    let root = match args.first() {
        Some(a) if !a.starts_with("--") => PathBuf::from(args.remove(0)),
        _ => game_dir().context("pass the game root or set GD_ROOT")?,
    };
    let db = Database::open(&root)?;
    if let Some(m) = &mod_name {
        db.initialize_mod(&root, m)?;
    }
    if args.first().is_some_and(|a| a == "--classes") {
        let prefix = args
            .get(1)
            .map(|p| p.to_ascii_lowercase())
            .unwrap_or_default();
        let mut counts = std::collections::BTreeMap::<String, (usize, String)>::new();
        for n in db
            .names()
            .filter(|n| n.to_ascii_lowercase().starts_with(&prefix))
        {
            if let Some(r) = db.get(n) {
                let e = counts.entry(r.class.clone()).or_insert((0, n.to_owned()));
                e.0 += 1;
            }
        }
        for (c, (n, example)) in counts {
            println!("{n:6} {c:32} {example}");
        }
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--diff") {
        // With --mod, `--diff [prefix]` needs no old dir: only an existing directory is one.
        let old_dir = args
            .get(1)
            .filter(|a| !a.starts_with("--") && (mod_name.is_none() || Path::new(a).is_dir()));
        let old = match old_dir {
            Some(d) => Database::open(d)?,
            None => {
                anyhow::ensure!(
                    mod_name.is_some(),
                    "--diff needs the old game dir, or --mod to diff the mod against the install"
                );
                Database::open(&root)?
            }
        };
        let prefix = args
            .get(if old_dir.is_some() { 2 } else { 1 })
            .map(|p| p.to_ascii_lowercase())
            .unwrap_or_default();
        let under = |n: &&str| n.to_ascii_lowercase().starts_with(&prefix);
        let mut names: Vec<&str> = db.names().filter(under).collect();
        names.extend(old.names().filter(under).filter(|n| db.get(n).is_none()));
        names.sort_unstable_by_key(|n| n.to_ascii_lowercase());
        for n in names {
            match (old.get(n), db.get(n)) {
                (None, Some(r)) => println!("+ {n} ({})", r.class),
                (Some(r), None) => println!("- {n} ({})", r.class),
                (Some(o), Some(r)) => {
                    let mut changes = Vec::new();
                    if o.class != r.class {
                        changes.push(format!("  class: {} -> {}", o.class, r.class));
                    }
                    for (k, v) in r.fields.iter() {
                        match o.get(k) {
                            Some(ov) if ov == v.as_slice() => {}
                            ov => changes.push(format!("  {k}: {ov:?} -> {v:?}")),
                        }
                    }
                    for (k, v) in o.fields.iter() {
                        if r.get(k).is_none() {
                            changes.push(format!("  {k}: {v:?} -> None"));
                        }
                    }
                    if !changes.is_empty() {
                        println!("~ {n}");
                        for c in changes {
                            println!("{c}");
                        }
                    }
                }
                (None, None) => {}
            }
        }
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--with") {
        let field = args.get(1).cloned().unwrap_or_default();
        let prefix = args
            .get(2)
            .map(|p| p.to_ascii_lowercase())
            .unwrap_or_default();
        for n in db
            .names()
            .filter(|n| n.to_ascii_lowercase().starts_with(&prefix))
        {
            if let Some(r) = db.get(n)
                && let Some(v) = r.get(&field)
                && !v
                    .iter()
                    .any(|x| matches!(x, RawValue::Str(s) if s.is_empty()))
            {
                println!("{n} = {v:?}");
            }
        }
        return Ok(());
    }
    for name in args {
        let Some(rec) = db.get(&name) else {
            eprintln!("{name}: not found");
            continue;
        };
        println!("== {} ({})", rec.name, rec.class);
        for (k, v) in rec.fields.iter() {
            println!("{k} = {v:?}");
        }
    }
    Ok(())
}
