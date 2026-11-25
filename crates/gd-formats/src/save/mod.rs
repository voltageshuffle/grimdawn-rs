//! Save-game formats (single player).
//!
//! | file | module | encoding |
//! |------|--------|----------|
//! | `main/_<name>/player.gdc` | [`gdc`] | CheckedWriter cipher, blocks |
//! | `.../<Difficulty>/quests.gdd`, `conversations.gdd` | [`gdd`] | cipher |
//! | `.../<Difficulty>/map.fow` | [`fow`] | LZ4 block |
//! | `.../<Difficulty>/map.dat` | [`map_dat`] | IOStream key/value |
//! | `formulas.gst` | [`tagged`] | IOStream key/value |
//! | `transfer.gst`, `transmutes.gst`, `reagents.gst` | [`gst`] | cipher |
//! | `leaderboard.brd` | [`survival`] | plain (PORT encoding) |
//! | `playmenu.cpn` | [`cpn`] | plain (partially inferred) |
//!
//! The account-wide files have a hardcore twin ending `.gsh` ([`Shared::file_name`]).
pub mod cipher;
pub mod cpn;
pub mod fow;
pub mod gdc;
pub mod gdd;
pub mod gst;
pub mod item;
pub mod map_dat;
pub mod survival;
pub mod tagged;

use anyhow::{Result, bail};
use std::path::Path;

/// The account-wide save files (`GameEngine::GetSharedSavePath` kinds 0, 1, 4, 5, 6):
/// `save\[<mod>\]<name>`, the extension `.gst`, or `.gsh` when the main player is hardcore
/// (`Player::IsHardcore`). `.bst` / `.cst` / `.dst` variants are backup / cloud copies; case 2
/// `playmenu.cpn` and case 3 `leaderboard.brd` have no hardcore twin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shared {
    Transfer,
    Formulas,
    Transmutes,
    Reagents,
    Potions,
}

impl Shared {
    pub fn file_name(self, hardcore: bool) -> &'static str {
        use Shared::*;
        match (self, hardcore) {
            (Transfer, false) => "transfer.gst",
            (Transfer, true) => "transfer.gsh",
            (Formulas, false) => "formulas.gst",
            (Formulas, true) => "formulas.gsh",
            (Transmutes, false) => "transmutes.gst",
            (Transmutes, true) => "transmutes.gsh",
            (Reagents, false) => "reagents.gst",
            (Reagents, true) => "reagents.gsh",
            (Potions, false) => "potions.gst",
            (Potions, true) => "potions.gsh",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SaveFile {
    Character(Box<gdc::Gdc>),
    Quests(gdd::Quests),
    Conversations(gdd::Conversations),
    FogOfWar(fow::Fow),
    Map(map_dat::MapDat),
    Tagged(tagged::Tagged),
    Transfer(gst::Transfer),
    Transmutes(gst::Transmutes),
    Reagents(gst::Reagents),
    PlayMenu(cpn::PlayMenu),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Character,
    Quests,
    Conversations,
    FogOfWar,
    Map,
    Tagged,
    Transfer,
    Transmutes,
    Reagents,
    PlayMenu,
}

impl Kind {
    /// From the file name; `.bak`/`.bakN` suffixes are ignored.
    pub fn from_path(path: &Path) -> Option<Kind> {
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        let base = match name.rfind(".bak") {
            Some(i) if name[i + 4..].chars().all(|c| c.is_ascii_digit()) => &name[..i],
            _ => &name[..],
        };
        Some(match base {
            "player.gdc" => Kind::Character,
            "quests.gdd" => Kind::Quests,
            "conversations.gdd" => Kind::Conversations,
            "map.fow" => Kind::FogOfWar,
            "map.dat" => Kind::Map,
            "formulas.gst" | "formulas.gsh" => Kind::Tagged,
            "transmutes.gst" | "transmutes.gsh" => Kind::Transmutes,
            "reagents.gst" | "reagents.gsh" => Kind::Reagents,
            "playmenu.cpn" => Kind::PlayMenu,
            b if (b.ends_with(".gst") || b.ends_with(".gsh")) && b.starts_with("transfer") => {
                Kind::Transfer
            }
            _ => return None,
        })
    }
}

impl SaveFile {
    pub fn parse(kind: Kind, data: &[u8]) -> Result<Self> {
        Ok(match kind {
            Kind::Character => SaveFile::Character(Box::new(gdc::Gdc::parse(data)?)),
            Kind::Quests => SaveFile::Quests(gdd::Quests::parse(data)?),
            Kind::Conversations => SaveFile::Conversations(gdd::Conversations::parse(data)?),
            Kind::FogOfWar => SaveFile::FogOfWar(fow::Fow::parse(data)?),
            Kind::Map => SaveFile::Map(map_dat::MapDat::parse(data)?),
            Kind::Tagged => SaveFile::Tagged(tagged::Tagged::parse(data)?),
            Kind::Transfer => SaveFile::Transfer(gst::Transfer::parse(data)?),
            Kind::Transmutes => SaveFile::Transmutes(gst::Transmutes::parse(data)?),
            Kind::Reagents => SaveFile::Reagents(gst::Reagents::parse(data)?),
            Kind::PlayMenu => SaveFile::PlayMenu(cpn::PlayMenu::parse(data)?),
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let Some(kind) = Kind::from_path(path) else {
            bail!("unknown save file type: {}", path.display())
        };
        Self::parse(kind, &std::fs::read(path)?)
    }

    /// Cipher seed of encrypted files (`None` for plain/LZ4/tagged files).
    pub fn seed(&self) -> Option<u32> {
        Some(match self {
            SaveFile::Character(x) => x.seed,
            SaveFile::Quests(x) => x.seed,
            SaveFile::Conversations(x) => x.seed,
            SaveFile::Transfer(x) => x.seed,
            SaveFile::Transmutes(x) => x.seed,
            SaveFile::Reagents(x) => x.seed,
            _ => return None,
        })
    }

    /// Change the cipher seed used by `encode` (the game uses `timeGetTime()`).
    pub fn set_seed(&mut self, seed: u32) {
        match self {
            SaveFile::Character(x) => x.seed = seed,
            SaveFile::Quests(x) => x.seed = seed,
            SaveFile::Conversations(x) => x.seed = seed,
            SaveFile::Transfer(x) => x.seed = seed,
            SaveFile::Transmutes(x) => x.seed = seed,
            SaveFile::Reagents(x) => x.seed = seed,
            _ => {}
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        match self {
            SaveFile::Character(x) => x.encode(),
            SaveFile::Quests(x) => x.encode(),
            SaveFile::Conversations(x) => x.encode(),
            SaveFile::FogOfWar(x) => x.encode(),
            SaveFile::Map(x) => x.encode(),
            SaveFile::Tagged(x) => x.encode(),
            SaveFile::Transfer(x) => x.encode(),
            SaveFile::Transmutes(x) => x.encode(),
            SaveFile::Reagents(x) => x.encode(),
            SaveFile::PlayMenu(x) => x.encode(),
        }
    }
}
