//! Quests / conversations / scripts inspection.
//!
//! gd-story check [--game DIR] [--mod NAME]
//!                                      parse every .qst/.cnv of base+gdx1-3 (or the mod's layers),
//!                                      print counts + type stats (`--game` defaults to `GD_ROOT`)
//! gd-story quest <name-substring>      dump matching quests (tasks, objectives, conditions, texts)
//! gd-story conv <name-substring>       dump matching conversations as an indented tree
//! gd-story conv-file <file.cnv>...     dump conversation files (e.g. the Crucible's, extracted)
//! gd-story save <quests.gdd>           match a save's quest / task uids and condition words against the data
use anyhow::Result;
use clap::{Parser, Subcommand};
use gd_formats::arc::Arc;
use gd_formats::cnv::{Conversation, Step};
use gd_formats::qst::{Action, Conditions, Quest};
use gd_formats::vfs::{LAYERS, SURVIVAL_LAYERS, mod_layers};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    /// The Grim Dawn install.
    #[arg(long, env = "GD_ROOT")]
    game: PathBuf,
    /// Mount a mod's story layers (`mods/<name>`, the Crucible packs for `survivalmode`)
    /// instead of the default Crucible layers.
    #[arg(long = "mod", value_name = "NAME")]
    mod_name: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Check,
    Quest {
        name: String,
    },
    Conv {
        name: String,
    },
    ConvFile {
        files: Vec<PathBuf>,
    },
    /// Match a quests.gdd against the quest files: quest/task MD5 uids and saved condition words.
    Save {
        path: PathBuf,
    },
}

/// (layer, entry name, bytes) of every entry with `ext` in `<layer>/resources/<arc>`:
/// the install's layers, then the mod's ([`mod_layers`]) or the Crucible's by default.
fn entries(
    game: &std::path::Path,
    arc: &str,
    ext: &str,
    mod_name: Option<&str>,
) -> Result<Vec<(String, String, Vec<u8>)>> {
    let extra: Vec<String> = match mod_name {
        Some(m) => mod_layers(m),
        None => SURVIVAL_LAYERS.iter().map(|l| (*l).to_owned()).collect(),
    };
    let mut out = Vec::new();
    for layer in LAYERS
        .iter()
        .copied()
        .chain(extra.iter().map(String::as_str))
    {
        let p = game.join(layer).join("resources").join(arc);
        // survivalmode2 names its archives in lower case.
        let p = if p.exists() {
            p
        } else {
            game.join(layer)
                .join("resources")
                .join(arc.to_ascii_lowercase())
        };
        if !p.exists() {
            continue;
        }
        let a = Arc::open(&p)?;
        for e in &a.entries {
            if e.name.to_ascii_lowercase().ends_with(ext) {
                out.push((
                    if layer.is_empty() {
                        "base".into()
                    } else {
                        layer.to_string()
                    },
                    e.name.clone(),
                    a.read(e)?,
                ));
            }
        }
    }
    Ok(out)
}

fn variant_name<T: std::fmt::Debug>(v: &T) -> String {
    let s = format!("{v:?}");
    s.split([' ', '{', '(']).next().unwrap_or("").to_string()
}

#[derive(Default)]
struct Stats {
    conds: BTreeMap<String, (usize, String)>,
    actions: BTreeMap<String, (usize, String)>,
}

impl Stats {
    fn cond(&mut self, c: &Option<Conditions>) {
        for c in c.iter().flat_map(|c| &c.list) {
            let e = self
                .conds
                .entry(format!("{:02} {}", c.kind, variant_name(&c.cond)))
                .or_default();
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!("{:?} {:?}", c.comparator, c.cond);
            }
        }
    }
    fn acts(&mut self, a: &[Action]) {
        for a in a {
            let e = self.actions.entry(variant_name(a)).or_default();
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!("{a:?}");
            }
        }
    }
    fn step(&mut self, s: &Step) {
        self.cond(&s.conditions);
        self.acts(&s.actions);
        for c in &s.children {
            self.step(c);
        }
    }
}

fn print_conds(c: &Option<Conditions>, ind: &str) {
    if let Some(c) = c {
        println!("{ind}conditions ({}):", if c.any() { "any" } else { "all" });
        for e in &c.list {
            println!("{ind}  [{}] {:?} {:?}", e.kind, e.comparator, e.cond);
        }
    }
}

fn print_acts(a: &[Action], ind: &str) {
    for a in a {
        println!("{ind}action {a:?}");
    }
}

fn print_step(c: &Conversation, s: &Step, depth: usize) {
    let ind = "  ".repeat(depth);
    let text: String = c.text(s).chars().take(90).collect();
    println!(
        "{ind}#{} {:?} flags={} link={:?} \"{}\"",
        s.index, s.kind, s.flags, s.link, text
    );
    if !s.voice.is_empty() {
        println!("{ind}    vo {}", s.voice);
    }
    print_conds(&s.conditions, &format!("{ind}    "));
    print_acts(&s.actions, &format!("{ind}    "));
    for ch in &s.children {
        print_step(c, ch, depth + 1);
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(m) = &args.mod_name {
        anyhow::ensure!(
            mod_layers(m).iter().any(|l| args.game.join(l).is_dir()),
            "mod {m}: no layer folder under {}",
            args.game.display()
        );
    }
    match args.cmd {
        Cmd::Check => {
            let mut stats = Stats::default();
            for (arc, ext) in [("Quests.arc", ".qst"), ("Conversations.arc", ".cnv")] {
                let mut per: BTreeMap<String, (usize, usize)> = BTreeMap::new();
                for (layer, name, data) in entries(&args.game, arc, ext, args.mod_name.as_deref())?
                {
                    let e = per.entry(layer.clone()).or_default();
                    let res = if ext == ".qst" {
                        Quest::parse(&data).map(|q| {
                            for t in &q.tasks {
                                for ev in t.incentives.iter().chain(&t.rewards) {
                                    stats.cond(&ev.conditions);
                                    stats.acts(&ev.actions);
                                }
                                for o in &t.objectives {
                                    stats.cond(&o.conditions);
                                    stats.acts(&o.actions);
                                }
                            }
                        })
                    } else {
                        Conversation::parse(&data).map(|c| stats.step(&c.root))
                    };
                    match res {
                        Ok(()) => e.0 += 1,
                        Err(err) => {
                            e.1 += 1;
                            println!("ERROR {layer}/{name}: {err:#}");
                        }
                    }
                }
                for (layer, (ok, bad)) in per {
                    println!("{arc} {layer}: {ok} ok, {bad} errors");
                }
            }
            println!("\ncondition types:");
            for (k, (n, ex)) in &stats.conds {
                println!(
                    "  {k:28} {n:6}  e.g. {}",
                    ex.chars().take(150).collect::<String>()
                );
            }
            println!("\naction types:");
            for (k, (n, ex)) in &stats.actions {
                println!(
                    "  {k:28} {n:6}  e.g. {}",
                    ex.chars().take(150).collect::<String>()
                );
            }
        }
        Cmd::Quest { name } => {
            for (layer, n, data) in
                entries(&args.game, "Quests.arc", ".qst", args.mod_name.as_deref())?
            {
                if !n.to_ascii_lowercase().contains(&name.to_ascii_lowercase()) {
                    continue;
                }
                let q = Quest::parse(&data)?;
                println!(
                    "== {layer}/{n}: v{} id {:#010x} flags {} \"{}\" / \"{}\"",
                    q.version,
                    q.id,
                    q.flags,
                    q.group(),
                    q.name()
                );
                for t in &q.tasks {
                    println!(
                        "  task {:#010x} flag {} blocker {} \"{}\"",
                        t.uid,
                        t.flag,
                        t.blocker,
                        q.texts.get(t.name_text)
                    );
                    for ev in &t.incentives {
                        println!("    incentive \"{}\"", q.texts.get(ev.text));
                        print_conds(&ev.conditions, "      ");
                        print_acts(&ev.actions, "      ");
                    }
                    for o in &t.objectives {
                        println!("    objective {:#010x} \"{}\"", o.uid, q.texts.get(o.text));
                        print_conds(&o.conditions, "      ");
                        print_acts(&o.actions, "      ");
                    }
                    for ev in &t.rewards {
                        println!("    reward \"{}\"", q.texts.get(ev.text));
                        print_conds(&ev.conditions, "      ");
                        print_acts(&ev.actions, "      ");
                    }
                }
            }
        }
        Cmd::Save { path } => {
            let save = gd_formats::save::gdd::Quests::parse(&std::fs::read(&path)?)?;
            let mut defs = gd_formats::collections::HashMap::default();
            for (_, n, data) in entries(&args.game, "Quests.arc", ".qst", args.mod_name.as_deref())?
            {
                let q = Quest::parse(&data)?;
                defs.insert(q.id, (n, q));
            }
            let (mut qok, mut qbad, mut tok, mut tbad, mut wok, mut wbad, mut missing) =
                (0, 0, 0, 0, 0, 0, 0);
            for sq in &save.quests {
                let Some((n, q)) = defs.get(&sq.id) else {
                    missing += 1;
                    continue;
                };
                if q.hash == sq.uid {
                    qok += 1
                } else {
                    qbad += 1
                }
                for st in &sq.tasks {
                    let Some(t) = q.task(st.id) else {
                        println!("{n}: unknown task {:#x}", st.id);
                        continue;
                    };
                    if t.hash == st.uid {
                        tok += 1
                    } else {
                        tbad += 1
                    }
                    let words: usize = t.objectives.iter().map(|o| o.saved_words()).sum();
                    if words * 4 == st.conditions.len() {
                        wok += 1
                    } else {
                        wbad += 1;
                        println!(
                            "{n} task {:#x}: {} condition bytes, expected {} words",
                            st.id,
                            st.conditions.len(),
                            words
                        );
                    }
                    if st.state != 1 && !st.conditions.is_empty() {
                        println!(
                            "  {n} task {:#x} state {} words {:?}",
                            st.id,
                            st.state,
                            st.conditions
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|w| u32::from_le_bytes(*w))
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
            println!(
                "quests: {qok} hash ok, {qbad} hash mismatch, {missing} not in data; tasks: {tok} hash ok, {tbad} mismatch; condition words: {wok} ok, {wbad} bad"
            );
        }
        Cmd::Conv { name } => {
            for (layer, n, data) in entries(
                &args.game,
                "Conversations.arc",
                ".cnv",
                args.mod_name.as_deref(),
            )? {
                if !n.to_ascii_lowercase().contains(&name.to_ascii_lowercase()) {
                    continue;
                }
                let c = Conversation::parse(&data)?;
                println!("== {layer}/{n}: v{} id {:#010x}", c.version, c.id);
                print_step(&c, &c.root, 1);
            }
        }
        Cmd::ConvFile { files } => {
            for f in files {
                let c = Conversation::parse(&std::fs::read(&f)?)?;
                println!("== {}: v{} id {:#010x}", f.display(), c.version, c.id);
                print_step(&c, &c.root, 1);
            }
        }
    }
    Ok(())
}
