//! Decode / dump / round-trip Grim Dawn save files.
//!
//! gd-save [--dump] [--game <install dir>] [--out <dir>] <file or dir>...
//!
//! Every recognised file is decoded, re-encoded and compared with the input.
//! `--dump` prints the full decoded structure; `--game` resolves character
//! record names (equipment, class) against the merged database + Text_EN tags;
//! `--out` writes the re-encoded files there (mirroring the input file names).
use anyhow::{Context, Result, bail};
use clap::Parser;
use gd_formats::arc::Arc;
use gd_formats::save::gdc::{EQUIP_SLOT_NAMES, Gdc};
use gd_formats::save::item::Item;
use gd_formats::save::{Kind, SaveFile};
use gd_formats::text::TagTable;
use gd_formats::vfs::{Database, LAYERS};
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct Args {
    /// Print the whole decoded structure.
    #[arg(long)]
    dump: bool,
    /// Game install directory, used to resolve records and tags.
    #[arg(long)]
    game: Option<PathBuf>,
    /// Write re-encoded files into this directory.
    #[arg(long)]
    out: Option<PathBuf>,
    paths: Vec<PathBuf>,
}

struct Names {
    db: Database,
    tags: TagTable,
}

impl Names {
    fn open(root: &Path) -> Result<Self> {
        let db = Database::open(root)?;
        let mut tags = TagTable::default();
        // Engine order: later files don't override, so add the newest layer first.
        for layer in LAYERS.iter().rev() {
            let p = root.join(layer).join("resources").join("Text_EN.arc");
            let Ok(arc) = Arc::open(&p) else { continue };
            for e in &arc.entries {
                if TagTable::is_tag_file(&e.name) {
                    tags.add_file(&arc.read(e)?);
                }
            }
        }
        Ok(Self { db, tags })
    }

    fn tag(&self, t: &str) -> String {
        self.tags
            .get(t)
            .map(|s| s.to_string())
            .unwrap_or_else(|| t.to_string())
    }

    /// "Display Name" (record) or an error marker when a record is missing.
    fn item(&self, it: &Item) -> String {
        let missing: Vec<&str> = it
            .records()
            .into_iter()
            .filter(|r| self.db.get(r).is_none())
            .collect();
        let name = |rec: &str| -> Option<String> {
            let r = self.db.get(rec)?;
            let tag = r
                .str("itemNameTag")
                .or_else(|| r.str("description"))
                .or_else(|| r.str("lootRandomizerName"))?;
            Some(self.tag(tag))
        };
        let mut s = String::new();
        if !it.prefix_name.is_empty()
            && let Some(p) = name(&it.prefix_name)
        {
            s.push_str(&p);
            s.push(' ');
        }
        s.push_str(&name(&it.base_name).unwrap_or_else(|| "?".into()));
        if !it.suffix_name.is_empty()
            && let Some(p) = name(&it.suffix_name)
        {
            s.push(' ');
            s.push_str(&p);
        }
        if missing.is_empty() {
            s
        } else {
            format!("{s}  [MISSING RECORDS: {}]", missing.join(", "))
        }
    }
}

/// Recursively collects recognised files as (path, path relative to the argument).
fn collect(base: &Path, p: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    if p.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(p)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        v.sort();
        for c in v {
            collect(base, &c, out)?;
        }
    } else if Kind::from_path(p).is_some() {
        let rel = p.strip_prefix(base).unwrap_or(p).to_path_buf();
        out.push((p.to_path_buf(), rel));
    }
    Ok(())
}

fn summary(f: &SaveFile) -> String {
    match f {
        SaveFile::Character(g) => format!(
            "character {:?} level {} ({}), {} skills, {} stash pages",
            g.header.name,
            g.header.level,
            g.header.class_tag,
            g.skills.skills.len(),
            g.stash.sacks.len()
        ),
        SaveFile::Quests(q) => format!("{} quests, {} tokens", q.quests.len(), q.tokens.len()),
        SaveFile::Conversations(c) => format!("{} conversations", c.conversations.len()),
        SaveFile::FogOfWar(f) => format!("{} regions", f.regions.len()),
        SaveFile::Map(m) => format!(
            "map {:?}, mod {:?}, spawn {:?}",
            m.map_path,
            m.mod_name,
            m.spawn.map(|s| s.position)
        ),
        SaveFile::Tagged(t) => format!("{} properties", t.entries.len()),
        SaveFile::Transfer(t) => {
            format!(
                "{} pages, {} items",
                t.sacks.len(),
                t.sacks.iter().map(|s| s.items.len()).sum::<usize>()
            )
        }
        SaveFile::Transmutes(t) => format!("{} groups", t.entries.len()),
        SaveFile::Reagents(r) => format!("{} reagents", r.entries.len()),
        SaveFile::PlayMenu(p) => {
            format!("last character {:?}, map {:?}", p.character, p.level_name)
        }
    }
}

fn character_report(g: &Gdc, names: Option<&Names>) {
    let class = names
        .map(|n| n.tag(&g.header.class_tag))
        .unwrap_or_else(|| g.header.class_tag.clone());
    println!(
        "    name {:?}, class {class}, level {} (xp {}), money {}",
        g.header.name, g.bio.level, g.bio.experience, g.info.money
    );
    println!(
        "    attributes: physique {} cunning {} spirit {}; unspent: attr {} skill {} devotion {}",
        g.bio.physique,
        g.bio.cunning,
        g.bio.spirit,
        g.bio.attribute_points,
        g.bio.skill_points,
        g.bio.devotion_points
    );
    for m in g.masteries() {
        println!("    mastery {} (level {})", m.name, m.level);
    }
    let Some(inv) = &g.inventory.data else { return };
    let show = |slot: &str, it: &Item| {
        if it.is_empty() {
            return;
        }
        match names {
            Some(n) => println!("    {slot:10} {}  <{}>", n.item(it), it.base_name),
            None => println!("    {slot:10} {}", it.base_name),
        }
    };
    for (i, s) in inv.equipment.iter().enumerate() {
        show(EQUIP_SLOT_NAMES[i], &s.item);
    }
    for (w, set) in inv.weapons.iter().enumerate() {
        for (i, s) in set.slots.iter().enumerate() {
            show(&format!("weapon{}{}", w + 1, ["a", "b"][i]), &s.item);
        }
    }
}

fn check(
    p: &Path,
    rel: &Path,
    kind: Kind,
    data: &[u8],
    args: &Args,
    names: Option<&Names>,
) -> Result<()> {
    let f = SaveFile::parse(kind, data)?;
    println!("{}: {}", p.display(), summary(&f));
    let enc = f.encode()?;
    let again = SaveFile::parse(kind, &enc).context("re-decoding the encoded file")?;
    if again != f {
        bail!("decode(encode(x)) != x");
    }
    if let Some(seed) = f.seed() {
        // Re-encrypt under a different seed: the writer must not depend on the input bytes.
        let mut g = f.clone();
        g.set_seed(seed ^ 0x9E37_79B9);
        let other = g.encode()?;
        let mut back = SaveFile::parse(kind, &other).context("decoding the re-seeded file")?;
        back.set_seed(seed);
        if back != f || other == data {
            bail!("re-seeded encode/decode mismatch");
        }
    }
    if enc == data {
        println!(
            "    round trip: byte-identical ({} bytes); re-seeded encode decodes equal",
            data.len()
        );
    } else if let SaveFile::FogOfWar(fw) = &f {
        let (payload, missing) = gd_formats::save::fow::Fow::decompress_lossy(data)?;
        if payload != fw.payload()? {
            bail!("FOW payload mismatch");
        }
        println!(
            "    round trip: LZ4 stream differs ({} -> {} bytes; input was missing {missing} trailing stream bytes); \
             decompressed payload identical, decode(encode(x)) == x",
            data.len(),
            enc.len()
        );
    } else {
        bail!("re-encoded bytes differ");
    }
    if let SaveFile::Character(g) = &f {
        character_report(g, names);
    }
    if let Some(n) = names {
        // Every database reference anywhere in the decoded structure.
        let text = format!("{f:?}");
        let mut refs: Vec<&str> = text
            .match_indices("records/")
            .filter_map(|(i, _)| text[i..].find(".dbr").map(|j| &text[i..i + j + 4]))
            .collect();
        refs.sort();
        refs.dedup();
        let missing: Vec<&&str> = refs.iter().filter(|r| n.db.get(r).is_none()).collect();
        println!(
            "    database references: {} distinct, {} missing {:?}",
            refs.len(),
            missing.len(),
            missing
        );
    }
    if args.dump {
        println!("{f:#?}");
    }
    if let Some(out) = &args.out {
        let dst = out.join(rel);
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&dst, &enc)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.paths.is_empty() {
        bail!("no input files");
    }
    let names = match &args.game {
        Some(g) => Some(Names::open(g).context("opening game database")?),
        None => None,
    };
    let mut files = Vec::new();
    for p in &args.paths {
        let base = if p.is_dir() {
            p.as_path()
        } else {
            p.parent().unwrap_or(Path::new(""))
        };
        collect(base, p, &mut files)?;
    }
    let (mut ok, mut failed) = (0, 0);
    for (p, rel) in &files {
        let kind = Kind::from_path(p).unwrap();
        let data = std::fs::read(p)?;
        match check(p, rel, kind, &data, &args, names.as_ref()) {
            Ok(()) => ok += 1,
            Err(e) => {
                failed += 1;
                println!("{}: ERROR {e:#}", p.display());
            }
        }
    }
    println!("{ok} ok, {failed} failed");
    if failed > 0 {
        bail!("{failed} file(s) failed");
    }
    Ok(())
}
