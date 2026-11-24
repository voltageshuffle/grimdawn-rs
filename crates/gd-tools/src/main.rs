use anyhow::Result;
use clap::{Parser, Subcommand};
use gd_formats::{
    arc::Arc,
    arz::{Arz, RawValue},
    util::safe_join,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Grim Dawn data tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List entries of an .arc archive
    ArcList { arc: PathBuf },
    /// Extract .arc entries (optionally filtered by substring) to a directory
    ArcExtract {
        arc: PathBuf,
        out: PathBuf,
        #[arg(long)]
        filter: Option<String>,
    },
    /// Verify every entry of every .arc given decompresses
    ArcCheck { arcs: Vec<PathBuf> },
    /// Dump an .arz database to a directory of .dbr text files
    ArzDump { arz: PathBuf, out: PathBuf },
    /// Print one record
    ArzGet { arz: PathBuf, record: String },
}

fn fmt_val(v: &RawValue) -> String {
    match v {
        RawValue::Int(i) => i.to_string(),
        RawValue::Float(f) => format!("{f}"),
        RawValue::Str(s) => s.to_string(),
        RawValue::Bool(b) => (*b as i32).to_string(),
    }
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::ArcList { arc } => {
            let a = Arc::open(&arc)?;
            for e in &a.entries {
                println!("{:>10} {:>10} k{} {}", e.size, e.compressed, e.kind, e.name);
            }
        }
        Cmd::ArcExtract { arc, out, filter } => {
            let a = Arc::open(&arc)?;
            for e in &a.entries {
                if filter
                    .as_ref()
                    .is_some_and(|f| !e.name.contains(f.as_str()))
                {
                    continue;
                }
                let p = safe_join(&out, &e.name)?;
                std::fs::create_dir_all(p.parent().unwrap())?;
                std::fs::write(&p, a.read(e)?)?;
            }
        }
        Cmd::ArcCheck { arcs } => {
            for arc in arcs {
                let a = Arc::open(&arc)?;
                let mut bytes = 0u64;
                for e in &a.entries {
                    bytes += a.read(e)?.len() as u64;
                }
                println!(
                    "ok {} files {} MB {}",
                    a.entries.len(),
                    bytes >> 20,
                    arc.display()
                );
            }
        }
        Cmd::ArzDump { arz, out } => {
            let db = Arz::open(&arz)?;
            for h in &db.records {
                let rec = db.decode(h)?;
                let p = safe_join(&out, &rec.name)?;
                std::fs::create_dir_all(p.parent().unwrap())?;
                let mut s = format!("Class,{},\n", rec.class);
                for (k, vs) in rec.fields.iter() {
                    s += &format!(
                        "{k},{},\n",
                        vs.iter().map(fmt_val).collect::<Vec<_>>().join(";")
                    );
                }
                std::fs::write(p, s)?;
            }
            println!("{} records", db.records.len());
        }
        Cmd::ArzGet { arz, record } => {
            let db = Arz::open(&arz)?;
            let h = db
                .find(&record)
                .ok_or_else(|| anyhow::anyhow!("no record"))?;
            let rec = db.decode(h)?;
            println!("Class={}", rec.class);
            for (k, vs) in rec.fields.iter() {
                println!(
                    "{k} = {}",
                    vs.iter().map(fmt_val).collect::<Vec<_>>().join(";")
                );
            }
        }
    }
    Ok(())
}
