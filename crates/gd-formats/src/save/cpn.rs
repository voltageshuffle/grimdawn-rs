//! `playmenu.cpn`: the main menu's "last played" settings (plain, unencrypted).
//!
//! `Grim Dawn.exe` writer 0x1400bc580 (on menu state changes and on menu destruction) and reader
//! 0x1400bbca0 (called from the menu's init 0x1400bead0), both through `GameEngine::GetSharedSavePath` case 2
//! (SaveManager::DirectWrite, optionally cloud-synced). Little endian:
//! `u32 0xFFFFFFFF` marker (a file whose first dword is not -1 is version 0, whose strings differ: not read),
//! `u32 version` (3), `u32 menu+576` (default 0), `u32 menu+580` (default 1), then
//! `wstring player name`, `wstring game name`, `wstring host name` (version >= 2), `string level name`,
//! `u32 max players` (the reader clamps to [2, global max]), `u32 level range` (>= 0), `u32 difficulty` (clamped 0..2),
//! `u32 challenge` (> 0 is true), `u32 mode` (0 campaign, 1 Crucible, 2 Custom, clamped 0..2; 2 becomes 0 when there is
//! no custom-mod list), `string mod name`, `string level name 2`, `u32 auto party`, `u32 pvp`, `u32 instanced loot`,
//! `wstring password`, `u32 is backup`, `u32 survival index`. `wstring` = `u32 char count` + UTF-16 units, `string` =
//! `u32 byte count` + bytes. Every field after the header is guarded by "bytes left" (shorter old files load; the
//! missing fields keep [`PlayMenu::default`]'s values, which are the reader's pre-read defaults).
//! What it remembers: the mode, the difficulty, Veteran (`challenge`), the Crucible arena (`survival_index` into the
//! menu's arena list, plus the level / mod names), the Custom mod (matched by mod name and level name in the menu's
//! mod list; not found keeps mode 2 without a selection), the selected character and the multiplayer settings.
//! Not verified: what the two menu words mean, whether the Random arena is stored as its resolved index (it is read
//! back by `survival_index` only).
use super::cipher::{bytes_to_string, string_to_bytes};
use crate::util::Reader;
use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub struct PlayMenu {
    /// 0xFFFFFFFF.
    pub marker: u32,
    /// 3 (the writer's version).
    pub version: u32,
    /// Menu words +576 / +580 (meaning unknown).
    pub unk0: u32,
    pub unk1: u32,
    pub character: String,
    pub game_name: String,
    pub host_name: String,
    pub level_name: String,
    pub max_players: u32,
    pub level_range: u32,
    /// 0 Normal, 1 Elite, 2 Ultimate.
    pub difficulty: u32,
    /// Non-zero: Veteran.
    pub challenge: u32,
    /// 0 campaign, 1 Crucible, 2 Custom.
    pub mode: u32,
    pub mod_name: String,
    pub level_name2: String,
    pub auto_party: u32,
    pub pvp: u32,
    pub instanced_loot: u32,
    pub password: String,
    pub is_backup: u32,
    /// The Crucible arena's index in the menu's arena list (-1 = none).
    pub survival_index: u32,
}

impl Default for PlayMenu {
    /// The reader's values before it reads (difficulty 0, level range 200, mode 0, auto party, instanced loot).
    fn default() -> Self {
        Self {
            marker: 0xFFFF_FFFF,
            version: 3,
            unk0: 0,
            unk1: 1,
            character: String::new(),
            game_name: String::new(),
            host_name: String::new(),
            level_name: String::new(),
            max_players: 4,
            level_range: 200,
            difficulty: 0,
            challenge: 0,
            mode: 0,
            mod_name: String::new(),
            level_name2: String::new(),
            auto_party: 1,
            pvp: 0,
            instanced_loot: 1,
            password: String::new(),
            is_backup: 0,
            survival_index: 0xFFFF_FFFF,
        }
    }
}

fn wstr(r: &mut Reader) -> Result<String> {
    let n = r.u32()? as usize;
    let b = r.bytes(n * 2)?;
    let u: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    match String::from_utf16(&u) {
        Ok(s) => Ok(s),
        Err(_) => bail!("invalid UTF-16 in playmenu.cpn"),
    }
}

fn bstr(r: &mut Reader) -> Result<String> {
    let n = r.u32()? as usize;
    Ok(bytes_to_string(r.bytes(n)?))
}

fn put_w(o: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    o.extend_from_slice(&(u.len() as u32).to_le_bytes());
    for c in u {
        o.extend_from_slice(&c.to_le_bytes());
    }
}

fn put_b(o: &mut Vec<u8>, s: &str) -> Result<()> {
    let m = string_to_bytes(s)?;
    o.extend_from_slice(&(m.len() as u32).to_le_bytes());
    o.extend_from_slice(&m);
    Ok(())
}

impl PlayMenu {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let mut r = Reader::new(d);
        let mut m = Self {
            marker: r.u32()?,
            ..Self::default()
        };
        if m.marker != 0xFFFF_FFFF {
            bail!("playmenu.cpn version 0 is not supported");
        }
        m.version = r.u32()?;
        m.unk0 = r.u32()?;
        m.unk1 = r.u32()?;
        m.character = wstr(&mut r)?;
        m.game_name = wstr(&mut r)?;
        if m.version >= 2 {
            m.host_name = wstr(&mut r)?;
        }
        m.level_name = bstr(&mut r)?;
        // Each field is read only while bytes are left (the engine's guard).
        macro_rules! field {
            ($f:ident, $read:expr) => {
                if r.remaining() > 0 {
                    m.$f = $read;
                }
            };
        }
        field!(max_players, r.u32()?);
        field!(level_range, r.u32()?);
        field!(difficulty, r.u32()?);
        field!(challenge, r.u32()?);
        field!(mode, r.u32()?);
        field!(mod_name, bstr(&mut r)?);
        field!(level_name2, bstr(&mut r)?);
        field!(auto_party, r.u32()?);
        field!(pvp, r.u32()?);
        field!(instanced_loot, r.u32()?);
        field!(password, wstr(&mut r)?);
        field!(is_backup, r.u32()?);
        field!(survival_index, r.u32()?);
        Ok(m)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut o = Vec::new();
        for v in [self.marker, self.version, self.unk0, self.unk1] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        put_w(&mut o, &self.character);
        put_w(&mut o, &self.game_name);
        if self.version >= 2 {
            put_w(&mut o, &self.host_name);
        }
        put_b(&mut o, &self.level_name)?;
        for v in [
            self.max_players,
            self.level_range,
            self.difficulty,
            self.challenge,
            self.mode,
        ] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        put_b(&mut o, &self.mod_name)?;
        put_b(&mut o, &self.level_name2)?;
        for v in [self.auto_party, self.pvp, self.instanced_loot] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        put_w(&mut o, &self.password);
        o.extend_from_slice(&self.is_backup.to_le_bytes());
        o.extend_from_slice(&self.survival_index.to_le_bytes());
        Ok(o)
    }
}
