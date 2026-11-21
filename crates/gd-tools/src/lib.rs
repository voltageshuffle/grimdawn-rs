//! Helpers shared by the gd-tools binaries.
use anyhow::Result;
use gd_formats::arc::Arc;
use std::path::{Path, PathBuf};
// Dependencies of the binaries only (the lib target would report them unused).
use clap as _;
use gd_sim as _;
use png as _;

/// Every file under `dir` (recursive), sorted.
pub fn walk_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out)
                } else {
                    out.push(p)
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// Visits `(name, bytes)` of every `wanted` entry of each input: the entries of an `.arc`, the files
/// under a directory, or a file named directly (always visited).
pub fn each_input(
    inputs: &[PathBuf],
    wanted: impl Fn(&str) -> bool,
    mut f: impl FnMut(&str, &[u8]),
) -> Result<()> {
    for input in inputs {
        if input
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("arc"))
        {
            let a = Arc::open(input)?;
            for e in a.entries.iter().filter(|e| wanted(&e.name)) {
                f(&format!("{}::{}", input.display(), e.name), &a.read(e)?);
            }
        } else if input.is_dir() {
            for p in walk_files(input) {
                let n = p.to_string_lossy();
                if wanted(&n) {
                    f(&n, &std::fs::read(&p)?);
                }
            }
        } else {
            f(&input.to_string_lossy(), &std::fs::read(input)?);
        }
    }
    Ok(())
}
