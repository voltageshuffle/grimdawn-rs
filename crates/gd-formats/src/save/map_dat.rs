//! `map.dat`: the per-map, per-difficulty save header in
//! `_<Name>/<map folder>/<Normal|Elite|Ultimate>/map.dat` (the map folder is
//! the map path with `/` and `\` turned into `_`, `GameEngine::GetDifficultyFolder`).
//! It is not fog (that is `map.fow`) and holds no level state: only which map
//! and mod the save belongs to and where the character starts.
//!
//! Grim Dawn.exe sub_140014CD0 (write; path from sub_140015350):
//! an `IOStreamWrite` property file (key = u32 len + bytes, value raw, see
//! [`super::tagged`]) with, in order:
//! - `mapVersion` int = 2;
//! - `mapPath` string (`levels/world001.map`, a Crucible arena
//!   `maps/survivalworld_a.map`); the file is written only when it is non-empty;
//! - `modName` string (empty for the campaign, `survivalmode` for the Crucible);
//! - `streamSpawnCoords` bool (4 bytes) = the spawn point has a region;
//! - `spawnCoords` `WorldCoords`, only when that bool is set: 64 bytes,
//!   `WorldCoords::Write` (Engine.dll 0x18022ad70) = the 3x3 rotation (three
//!   Vec3, this+24/+36/+48), the region-local position (this+8), the 16-byte
//!   `RegionId` (zero when there is no region).
//!
//! The spawn is `Player::GetClosestRespawnPoint` (Game.dll 0x1803d34f0): the
//! player's own coordinates, whose position and region are replaced by the
//! current respawn point's (the difficulty's `respawns.current` uid looked up
//! in the instance groups) when that entity and its region exist; the
//! rotation stays the player's.
//!
//! sub_140015530 (load): reads `mapVersion`, `mapPath`, `modName`
//! (only when the version is > 1), `streamSpawnCoords`; loads the world
//! `mapPath` (as a mod map when the session has a mod); then, if the bool is
//! set, reads `spawnCoords` into the character's spawn point
//! (`Character::SetSpawnPoint`, `World::SetPlayerSpawnPoint`). Without a
//! region (bool off, or the region id is not in the map) the character
//! starts at `World::GetPlayerSpawnPoint`. A missing `map.dat` fails the load.
use super::cipher::{bytes_to_string, string_to_bytes};
use crate::util::Reader;
use anyhow::{Context, Result, ensure};

/// `GAME::WorldCoords` as streamed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldCoords {
    /// The three rows of the engine's Mat3, as in level entities
    /// ([`crate::map::level::Entity::rotation`]).
    pub rotation: [f32; 9],
    /// Region-local.
    pub position: [f32; 3],
    /// `GAME::RegionId` ([`crate::map::RegionInfo::id`]); all zero = no region.
    pub region: [u32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapDat {
    pub version: u32,
    pub map_path: String,
    pub mod_name: String,
    /// `spawnCoords` (written when `streamSpawnCoords` is set).
    pub spawn: Option<WorldCoords>,
}

impl MapDat {
    pub const VERSION: u32 = 2;

    pub fn new(map_path: &str, mod_name: &str, spawn: Option<WorldCoords>) -> Self {
        Self {
            version: Self::VERSION,
            map_path: map_path.to_owned(),
            mod_name: mod_name.to_owned(),
            spawn,
        }
    }

    pub fn parse(d: &[u8]) -> Result<Self> {
        let mut r = Reader::new(d);
        let version = prop(&mut r, "mapVersion", 4).map(|b| u32_le(b, 0))?;
        let map_path = string(&mut r, "mapPath")?;
        let mod_name = if version > 1 {
            string(&mut r, "modName")?
        } else {
            String::new()
        };
        let stream = prop(&mut r, "streamSpawnCoords", 4).map(|b| u32_le(b, 0) != 0)?;
        let spawn = if stream {
            let b = prop(&mut r, "spawnCoords", 64)?;
            let f = |i: usize| f32::from_bits(u32_le(b, 4 * i));
            Some(WorldCoords {
                rotation: std::array::from_fn(f),
                position: std::array::from_fn(|i| f(9 + i)),
                region: std::array::from_fn(|i| u32_le(b, 48 + 4 * i)),
            })
        } else {
            None
        };
        ensure!(
            r.remaining() == 0,
            "map.dat: {} trailing bytes",
            r.remaining()
        );
        Ok(Self {
            version,
            map_path,
            mod_name,
            spawn,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut o = Vec::new();
        let key = |o: &mut Vec<u8>, k: &str| {
            o.extend_from_slice(&(k.len() as u32).to_le_bytes());
            o.extend_from_slice(k.as_bytes());
        };
        let string = |o: &mut Vec<u8>, s: &str| -> Result<()> {
            let b = string_to_bytes(s)?;
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(&b);
            Ok(())
        };
        key(&mut o, "mapVersion");
        o.extend_from_slice(&self.version.to_le_bytes());
        key(&mut o, "mapPath");
        string(&mut o, &self.map_path)?;
        if self.version > 1 {
            key(&mut o, "modName");
            string(&mut o, &self.mod_name)?;
        }
        key(&mut o, "streamSpawnCoords");
        o.extend_from_slice(&(self.spawn.is_some() as u32).to_le_bytes());
        if let Some(s) = &self.spawn {
            key(&mut o, "spawnCoords");
            for v in s.rotation.iter().chain(&s.position) {
                o.extend_from_slice(&v.to_le_bytes());
            }
            for v in &s.region {
                o.extend_from_slice(&v.to_le_bytes());
            }
        }
        Ok(o)
    }
}

fn u32_le(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}

/// The next key must be `key` (`IOStreamRead::HandlePropertyTag`).
fn key(r: &mut Reader, key: &str) -> Result<()> {
    let at = r.pos;
    let n = r.u32().with_context(|| format!("map.dat: missing {key}"))? as usize;
    let k = r
        .bytes(n)
        .with_context(|| format!("map.dat: truncated key at {at:#x}"))?;
    ensure!(
        k == key.as_bytes(),
        "map.dat: expected {key}, found {:?}",
        bytes_to_string(k)
    );
    Ok(())
}

fn prop<'a>(r: &mut Reader<'a>, k: &str, n: usize) -> Result<&'a [u8]> {
    key(r, k)?;
    let at = r.pos;
    r.bytes(n)
        .with_context(|| format!("map.dat: truncated value at {at:#x}"))
}

fn string(r: &mut Reader, k: &str) -> Result<String> {
    let n = u32_le(prop(r, k, 4)?, 0) as usize;
    let at = r.pos;
    let b = r
        .bytes(n)
        .with_context(|| format!("map.dat: truncated value at {at:#x}"))?;
    Ok(bytes_to_string(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let m = MapDat::new("maps/survivalworld_a.map", "survivalmode", None);
        let b = m.encode().unwrap();
        assert_eq!(MapDat::parse(&b).unwrap(), m);
        let s = WorldCoords {
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            position: [1.0, 2.0, 3.0],
            region: [1, 2, 3, 4],
        };
        let m = MapDat::new("levels/world001.map", "", Some(s));
        let b = m.encode().unwrap();
        assert_eq!(MapDat::parse(&b).unwrap(), m);
        assert!(MapDat::parse(&b[..b.len() - 1]).is_err());
    }
}
