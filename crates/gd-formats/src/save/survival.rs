//! The Crucible leaderboard: the local top scores, shared by all characters.
//!
//! `GameEngine::SaveSurvivalScore` / `LoadSurvivalScore`:
//! entries of (player name, difficulty, score), at most
//! 15; a score of 0 or an empty name is not stored; with 15 entries the lowest is replaced if the
//! new score is higher, else dropped. The file is a shared save, `save\leaderboard.brd` (per
//! `GameEngine::GetSharedSavePath` case 3: the save root, no mod folder or hardcore twin):
//! `u32 count`, then per entry `u32 len`, the name as UTF-16 code
//! units, `u32 difficulty`, `u32 score`. PORT: whether `len` counts code units (assumed) or bytes, the
//! plain (uncompressed, unciphered) encoding and the highest-first order are not traced.
use crate::util::Reader;
use anyhow::{Context, Result, bail};

pub const MAX_ENTRIES: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub difficulty: u32,
    pub score: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Leaderboard {
    pub entries: Vec<Entry>,
}

impl Leaderboard {
    /// Adds a finished run (`SaveSurvivalScore`).
    pub fn add(&mut self, name: &str, difficulty: u32, score: u32) {
        if score == 0 || name.is_empty() {
            return;
        }
        let new = Entry {
            name: name.to_owned(),
            difficulty,
            score,
        };
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.sort_by_key(|e| e.score);
            match self.entries.iter().position(|e| e.score < score) {
                Some(i) => self.entries[i] = new,
                None => return,
            }
        } else {
            self.entries.push(new);
        }
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.score));
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = (self.entries.len() as u32).to_le_bytes().to_vec();
        for e in &self.entries {
            let units: Vec<u16> = e.name.encode_utf16().collect();
            out.extend((units.len() as u32).to_le_bytes());
            for u in units {
                out.extend(u.to_le_bytes());
            }
            out.extend(e.difficulty.to_le_bytes());
            out.extend(e.score.to_le_bytes());
        }
        out
    }

    pub fn parse(b: &[u8]) -> Result<Self> {
        let mut r = Reader::new(b);
        let n = r.u32().context("leaderboard: truncated")? as usize;
        if n > MAX_ENTRIES {
            bail!("leaderboard: {n} entries");
        }
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let len = r.u32().context("leaderboard: truncated")? as usize;
            let raw = r
                .bytes(len.saturating_mul(2))
                .context("leaderboard: truncated name")?;
            let units: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            entries.push(Entry {
                name: String::from_utf16_lossy(&units),
                difficulty: r.u32().context("leaderboard: truncated")?,
                score: r.u32().context("leaderboard: truncated")?,
            });
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_best_fifteen_and_round_trips() {
        let mut l = Leaderboard::default();
        l.add("", 0, 50);
        l.add("Zero", 0, 0);
        assert!(l.entries.is_empty(), "no name / no score: not stored");
        for i in 1..=15 {
            l.add("Ωmega", 1, i * 10);
        }
        assert_eq!(l.entries.len(), 15);
        l.add("Low", 0, 5);
        assert_eq!(l.entries.len(), 15);
        assert!(
            l.entries.iter().all(|e| e.name != "Low"),
            "too low: dropped"
        );
        l.add("High", 2, 1000);
        assert_eq!(
            (l.entries[0].name.as_str(), l.entries[0].score),
            ("High", 1000)
        );
        assert!(
            l.entries.iter().all(|e| e.score != 10),
            "the lowest was replaced"
        );
        assert_eq!(Leaderboard::parse(&l.encode()).unwrap(), l);
        Leaderboard::parse(&[1, 0, 0, 0]).unwrap_err();
    }
}
