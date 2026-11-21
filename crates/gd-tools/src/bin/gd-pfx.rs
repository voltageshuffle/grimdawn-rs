//! Particle effect (.pfx) tools.
//!
//! Inputs are .arc files, directories, plain files or `ARCHIVE.arc::entry`.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use gd_formats::{arc::Arc, pfx::Pfx};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Grim Dawn .pfx tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse every .pfx in the given .arc files / directories and print stats
    Check { inputs: Vec<PathBuf> },
    /// Print every emitter of one effect
    Info { input: String },
}

fn each_pfx(inputs: &[PathBuf], f: impl FnMut(&str, &[u8])) -> Result<()> {
    gd_tools::each_input(inputs, |n| n.to_ascii_lowercase().ends_with(".pfx"), f)
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Check { inputs } => {
            let mut stats: BTreeMap<String, usize> = BTreeMap::new();
            let (mut files, mut errors) = (0, 0);
            each_pfx(&inputs, |name, data| {
                files += 1;
                match Pfx::parse(data) {
                    Ok(p) => {
                        *stats.entry("emitters".into()).or_default() += p.emitters.len();
                        for e in &p.emitters {
                            *stats.entry(format!("blend {:?}", e.blend())).or_default() += 1;
                            *stats.entry(format!("shape {:?}", e.shape)).or_default() += 1;
                            for (flag, on) in [
                                ("local", e.local()),
                                ("looping", e.looping()),
                                ("velocity-aligned", e.velocity_aligned()),
                                ("light", e.light()),
                                ("light-shadows", e.light() && e.light_shadows()),
                                ("light-simple", e.light() && e.light_simple()),
                                ("orient-vertically", e.orient_vertically()),
                            ] {
                                if on {
                                    *stats.entry(format!("flag {flag}")).or_default() += 1;
                                }
                            }
                            if e.fps > 0 {
                                *stats.entry("flipbook fps>0".into()).or_default() += 1;
                            }
                            if !e.drop_decal.is_empty() {
                                *stats.entry("drop decal".into()).or_default() += 1;
                            }
                        }
                    }
                    Err(err) => {
                        errors += 1;
                        eprintln!("{name}: {err:#}");
                    }
                }
            })?;
            for (k, v) in &stats {
                println!("{k:24} {v}");
            }
            println!("{files} files, {errors} errors");
        }
        Cmd::Info { input } => {
            let data = if let Some((arc, entry)) = input.split_once("::") {
                let a = Arc::open(arc)?;
                let e = a
                    .find(entry)
                    .with_context(|| format!("{entry} not in {arc}"))?;
                a.read(e)?
            } else {
                std::fs::read(&input).with_context(|| input.clone())?
            };
            let p = Pfx::parse(&data)?;
            for e in &p.emitters {
                println!(
                    "emitter {:?}: tex {} shader {} decal {:?}",
                    e.name, e.texture, e.shader, e.drop_decal
                );
                println!(
                    "  blend {:?} shape {:?} fps {} life {} rot {:?} duration {}",
                    e.blend(),
                    e.shape,
                    e.fps,
                    e.life,
                    e.rotation,
                    e.duration()
                );
                println!(
                    "  bools {:?}",
                    e.bools.iter().map(|&b| b as u8).collect::<Vec<_>>()
                );
                for (i, c) in e.curves.iter().enumerate() {
                    let pts: Vec<String> = c
                        .points
                        .iter()
                        .map(|p| format!("({:.3},{:.3})", p[0], p[1]))
                        .collect();
                    println!(
                        "  curve {i:2} domain {:.3} range {:.3}: {}",
                        c.domain,
                        c.range,
                        pts.join(" ")
                    );
                }
            }
        }
    }
    Ok(())
}
