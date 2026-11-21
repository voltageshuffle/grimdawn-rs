//! Print merged database records: `cargo run -p gd-tools --bin gd-rec -- <game dir> <record>...`
//! (`<game dir>` defaults to `GD_ROOT` when no argument is given).
//! `--classes <prefix>` counts the classes of records under a path prefix;
//! `--with <field> <prefix>` lists records there with a non-empty `field`.
use anyhow::Context;
use gd_formats::arz::RawValue;
use gd_formats::testdata::game_dir;
use gd_formats::vfs::Database;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .or_else(game_dir)
        .context("pass the game root or set GD_ROOT")?;
    let db = Database::open(&root)?;
    let args: Vec<String> = args.collect();
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
                && let Some((_, v)) = r
                    .fields
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(&field))
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
