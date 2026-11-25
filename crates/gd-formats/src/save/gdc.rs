//! `player.gdc` — the character file.
//!
//! Structure (Game.dll): `Player::WriteNewFormatHeader`,
//! `Player::ReadNewFormatHeader`, `Player::SaveNewFormatData`,
//! `Player::LoadNewFormatData` and the per-component
//! `WriteProperties` functions; offsets/types/order are from the writer. Field names follow
//! the game code that reads each member (named on each struct); members no game code reads
//! back keep neutral names and say so.
//!
//! Only the current layout (header v2, data v8, block versions as written by the
//! current game) is decoded; older versions are rejected with an error.
use super::cipher::{Decoder, Encoder};
use super::item::{Item, StashSack};
use anyhow::{Result, bail};

pub const MAGIC: u32 = 0x5843_4447; // "GDCX"

fn expect_version(what: &str, got: u32, want: u32) -> Result<()> {
    if got != want {
        bail!("{what}: version {got} not supported (expected {want})");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: u32,
    pub name: String,
    /// `GetGender() == 1` written as a bool byte: 1 = male. Per
    /// `Character::Load` (`characterGenderProfile` "Male" → 1, "Female" → 2).
    pub sex: u8,
    pub class_tag: String,
    pub level: u32,
    pub hardcore: u8,
    /// Expansion flags byte (header v >= 2): bit0 AoM, bit1 FG, bit2 FoA. Per
    /// `GameEngine::WritePlayerTransfer`: `IsExpansion1/2/3Loaded` →
    /// `SetExpansionFlag(1/2/4)`, the same helper `Player::AddExpansionState` uses.
    pub expansion_status: u8,
}

impl CharacterInfo {
    /// The difficulty last played, 0 normal, 1 elite, 2 ultimate
    /// (`Player::GetLastDifficultyPlayed`: the low nibble).
    pub fn difficulty(&self) -> usize {
        (self.last_difficulty as usize & 0xF).min(2)
    }
}

/// Block 1 (v5).
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterInfo {
    pub version: u32,
    pub is_in_main_quest: u8,
    pub has_been_in_game: u8,
    /// `difficulty | ((mode << 2) | challenge) << 4` when `has_been_in_game`.
    pub last_difficulty: u8,
    pub greatest_difficulty: u8,
    pub money: u32,
    pub greatest_survival_difficulty: u8,
    pub current_tribute: u32,
    pub compass_state: u8,
    pub skill_window_show_help: u8,
    pub alternate_config: u8,
    pub alternate_config_enabled: u8,
    pub texture: String,
    /// Count as written; only the first `min(count, 42)` flags are stored.
    pub loot_filter_count: u32,
    pub loot_filters: Vec<u8>,
}

/// Block 2 (v8): `Player` counters + `CharacterBio::WriteProperties`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bio {
    pub version: u32,
    pub level: u32,
    pub experience: u32,
    pub attribute_points: u32,
    pub skill_points: u32,
    pub devotion_points: u32,
    pub total_devotion: u32,
    /// Attribute ids 1..=5 of the character's attribute pak.
    pub physique: f32,
    pub cunning: f32,
    pub spirit: f32,
    pub health: f32,
    pub energy: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SackItem {
    pub item: Item,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InventorySack {
    pub block_id: u32,
    pub temp_bool: u8,
    pub items: Vec<SackItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EquipSlot {
    pub item: Item,
    pub attached: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeaponSet {
    pub flag: u8,
    pub slots: [EquipSlot; 2],
}

/// Equipment slot order as written by `EquipmentCtrl::WriteProperties`
/// (member offsets 32,48,96,112,128,176,144,160,192,208,64,80).
pub const EQUIP_SLOT_NAMES: [&str; 12] = [
    "head",
    "amulet",
    "chest",
    "legs",
    "feet",
    "hands",
    "ring1",
    "ring2",
    "waist",
    "shoulders",
    "medal",
    "relic",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Inventory {
    pub version: u32,
    /// `None` when the writer had no ControllerPlayer (flag byte 0).
    pub data: Option<InventoryData>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InventoryData {
    pub focused: u32,
    pub selected: u32,
    pub sacks: Vec<InventorySack>,
    pub use_alternate: u8,
    pub equipment: Vec<EquipSlot>,
    pub weapons: [WeaponSet; 2],
}

/// Block 4: private stash pages.
#[derive(Debug, Clone, PartialEq)]
pub struct Stash {
    pub version: u32,
    pub sacks: Vec<StashSack>,
}

pub type Uid = [u8; 16];

/// Block 5 (v1): per-difficulty list of unlocked respawn points + current one.
#[derive(Debug, Clone, PartialEq)]
pub struct Respawns {
    pub version: u32,
    pub lists: [Vec<Uid>; 3],
    pub current: [Uid; 3],
}

/// Blocks 6 (teleports) and 7 (markers) (v1): per-difficulty UID lists.
#[derive(Debug, Clone, PartialEq)]
pub struct UidLists {
    pub version: u32,
    pub lists: [Vec<Uid>; 3],
}

/// Block 17 (v2): per difficulty two lists: restored then discovered.
/// `Player::SaveNewFormatData` writes the vectors `Player::GetShrineUIDs`
/// (`AddShrineUID`) and `GetDiscoveredShrineUIDs` return, in that order.
#[derive(Debug, Clone, PartialEq)]
pub struct Shrines {
    pub version: u32,
    pub lists: [[Vec<Uid>; 2]; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub level: u32,
    pub enabled: u8,
    pub unk_205: u8,
    pub devotion_level: u32,
    pub devotion_experience: u32,
    pub sublevel: u32,
    pub skill_active: u8,
    pub skill_transition: u8,
    pub autocast_skill: String,
    pub autocast_controller: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemSkill {
    pub skill_name: String,
    pub autocast_skill: String,
    pub autocast_controller: String,
    /// `EquipmentCtrl::GetEquippedLocation`, bit31 = alternate weapon set.
    pub item_slot: u32,
    /// Item record, or item-set name when the skill comes from a set.
    pub item_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtraSkill {
    pub skill_name: String,
    pub autocast_skill: String,
    pub autocast_controller: String,
    /// Record of the parent skill (`Skill::GetSubSkillParentId` → object
    /// name), empty without one. Per `SkillManager::WriteProperties`.
    pub source_name: String,
}

/// Block 8 (v8): `SkillManager::WriteProperties`.
#[derive(Debug, Clone, PartialEq)]
pub struct Skills {
    pub version: u32,
    pub skills: Vec<Skill>,
    pub masteries_allowed: u32,
    pub skill_reclamation_points_used: u32,
    pub devotion_reclamation_points_used: u32,
    pub item_skills: Vec<ItemSkill>,
    pub extra_skills: Vec<ExtraSkill>,
}

/// Block 12 (v1): collected lore note records.
#[derive(Debug, Clone, PartialEq)]
pub struct Notes {
    pub version: u32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    pub modified: u8,
    pub unlocked: u8,
    pub value: f32,
    pub positive_boost: f32,
    pub negative_boost: f32,
}

/// Block 13 (v5): `FactionPack::WriteProperties`.
#[derive(Debug, Clone, PartialEq)]
pub struct Factions {
    pub version: u32,
    pub current: u32,
    pub factions: Vec<Faction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiSlot {
    pub a: String,
    pub b: String,
    pub flag: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HotSlot {
    /// Type 0xFFFFFFFF.
    Empty,
    /// Type 0 (`HotSlotOptionSkill`): skill record, item-skill flag, item record,
    /// equipped location (bit31 = alternate set).
    Skill {
        skill: String,
        is_item_skill: u8,
        item: String,
        equip_location: u32,
    },
    /// Type 4 (`HotSlotOptionScroll`).
    Scroll {
        a: String,
        b: String,
        c: String,
        label: String,
    },
    /// Types 2/3 (potions), 5 (evade) and any other type: no payload.
    Plain(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HotSlotBar {
    pub id: u32,
    pub slots: Vec<HotSlot>,
}

/// Block 14 (v7): UI state (`Player::WriteUiState`), `PlayerHotSlotCtrl` and camera distance.
/// The UI record (`unk1` byte, `unk2` u32, the slots, `unk3` byte) is
/// known to carry no game state: `SaveNewFormatData` always writes a
/// freshly constructed one (all zero/empty, first slot flag 1) and
/// `LoadNewFormatData` only hands the read one to a UI listener (Player
/// +18112, virtual +8).
#[derive(Debug, Clone, PartialEq)]
pub struct UiSettings {
    pub version: u32,
    pub unk1: u8,
    pub unk2: u32,
    pub unk3: u8,
    pub slots: Vec<UiSlot>,
    pub slots_per_bar: u32,
    pub bars: Vec<HotSlotBar>,
    pub camera_distance: f32,
}

/// Block 15 (v1): unlocked tutorial pages.
#[derive(Debug, Clone, PartialEq)]
pub struct Tutorials {
    pub version: u32,
    pub pages: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerDifficultyStats {
    pub greatest_monster_killed_name: String,
    pub greatest_monster_killed_level: u32,
    pub greatest_monster_killed_life_and_mana: u32,
    pub last_monster_hit: String,
    pub last_monster_hit_by: String,
}

/// Block 16 (v12): `PlayStats::WriteProperties` + two Player u32s.
/// Names come from the members' accessors and the equation variable names
/// of `PlayStats::ResolveEquationVariable` (Game.dll): play time
/// (seconds; `GetPlayTimeInMinutes` divides by 60), `IncrementDeaths`,
/// `IncrementKills` (kills, experience, champion-or-better and hero kills,
/// per-difficulty boss kills, Ultimate veteran boss kills, and "hits
/// inflicted": the killed creature's count of attacks taken,
/// `Character::TakeAttack`), `IncrementPotion`, `IncrementLevel`,
/// `ReceiveHit`, `Inflict`/`ReceiveCriticalHit`, `TallyDamageInflicted`/
/// `Received` (greatest single hit), `SetLastMonsterHit(By)Name` (the
/// damage of that hit), `ICrafted`, `ICraftedRelic` (all, class 1
/// transcendent, class 2 mythical), `UnlockedDevotionShrine`
/// (`GetNumberOfShrinesRestored`), `OpenedOneShotChest`,
/// `CollectedLoreNote`, `OpenedHiddenChest`, the survival getters (greatest
/// of the stored greatest and current wave/score), `SetSkillStat`
/// (`skill_stats`), the endless getters and `DifficultySkipUsed`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayStats {
    pub version: u32,
    pub play_time: u32,
    pub deaths: u32,
    pub kills: u32,
    pub experience_from_kills: u32,
    pub health_potions_used: u32,
    pub mana_potions_used: u32,
    pub max_level: u32,
    pub hits_received: u32,
    pub hits_inflicted: u32,
    pub crits_inflicted: u32,
    pub crits_received: u32,
    pub greatest_damage_inflicted: f32,
    pub per_difficulty: Vec<PerDifficultyStats>,
    pub champion_kills: u32,
    pub last_hit: f32,
    pub last_hit_by: f32,
    pub greatest_damage_received: f32,
    pub hero_kills: u32,
    pub items_crafted: u32,
    pub relics_crafted: u32,
    pub transcendent_relics_crafted: u32,
    pub mythical_relics_crafted: u32,
    pub shrines_restored: u32,
    pub one_shot_chests_opened: u32,
    pub lore_notes_collected: u32,
    pub boss_kills: [u32; 3],
    pub survival_greatest_wave: u32,
    pub survival_greatest_score: u32,
    pub survival_defenses_built: u32,
    pub survival_powerups_activated: u32,
    /// `PlayStats::SetSkillStat` map: record → highest value set (e.g.
    /// Shattered Realm shrine buff records).
    pub skill_stats: Vec<(String, u32)>,
    pub endless_souls: u32,
    pub endless_essence: u32,
    pub difficulty_skip: u8,
    pub ultimate_veteran_boss_kills: u32,
    pub hidden_chests_opened: u32,
    /// Two Player fields (+19604, +19608) written after PlayStats inside
    /// block 16. Unused: only the constructor (zero), the writer and
    /// `LoadNewFormatData` touch them.
    pub player_u4901: u32,
    pub player_u4902: u32,
}

/// Block 10 (v2): per-difficulty trigger tokens (strings).
#[derive(Debug, Clone, PartialEq)]
pub struct Tokens {
    pub version: u32,
    pub lists: [Vec<String>; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gdc {
    pub seed: u32,
    pub header: Header,
    pub data_version: u32,
    pub uid: Uid,
    pub info: CharacterInfo,
    pub bio: Bio,
    pub inventory: Inventory,
    pub stash: Stash,
    pub respawns: Respawns,
    pub teleports: UidLists,
    pub markers: UidLists,
    pub shrines: Shrines,
    pub skills: Skills,
    pub notes: Notes,
    pub factions: Factions,
    pub ui: UiSettings,
    pub tutorials: Tutorials,
    pub stats: PlayStats,
    pub tokens: Tokens,
}

fn read_uids(d: &mut Decoder) -> Result<Vec<Uid>> {
    let n = d.u32()?;
    (0..n).map(|_| d.uid()).collect()
}

fn write_uids(e: &mut Encoder, v: &[Uid]) {
    e.u32(v.len() as u32);
    for u in v {
        e.uid(u);
    }
}

fn read_strings(d: &mut Decoder) -> Result<Vec<String>> {
    let n = d.u32()?;
    (0..n).map(|_| d.string()).collect()
}

fn write_strings(e: &mut Encoder, v: &[String]) -> Result<()> {
    e.u32(v.len() as u32);
    for s in v {
        e.string(s)?;
    }
    Ok(())
}

/// Reads `u32 id; block { u32 version; body }`.
fn block<T>(d: &mut Decoder, id: u32, f: impl FnOnce(&mut Decoder, u32) -> Result<T>) -> Result<T> {
    let (_, end) = d.begin_block(Some(id))?;
    let version = d.u32()?;
    let v = f(d, version).map_err(|e| e.context(format!("block {id} (v{version})")))?;
    d.end_block(id, end)?;
    Ok(v)
}

fn wblock(
    e: &mut Encoder,
    id: u32,
    version: u32,
    f: impl FnOnce(&mut Encoder) -> Result<()>,
) -> Result<()> {
    e.begin_block(id);
    e.u32(version);
    f(e)?;
    e.end_block()
}

impl Header {
    fn read(d: &mut Decoder) -> Result<Self> {
        let magic = d.u32()?;
        if magic != MAGIC {
            bail!("not a GDC file (magic {magic:#x})");
        }
        let version = d.u32()?;
        if !(1..=2).contains(&version) {
            bail!("GDC header version {version} not supported");
        }
        let name = d.wstring()?;
        let sex = d.u8()?;
        let class_tag = d.string()?;
        let level = d.u32()?;
        let hardcore = d.u8()?;
        let expansion_status = if version >= 2 { d.u8()? } else { 0 };
        d.check_key("GDC header")?;
        Ok(Self {
            version,
            name,
            sex,
            class_tag,
            level,
            hardcore,
            expansion_status,
        })
    }

    fn write(&self, e: &mut Encoder) -> Result<()> {
        e.u32(MAGIC);
        e.u32(self.version);
        e.wstring(&self.name);
        e.u8(self.sex);
        e.string(&self.class_tag)?;
        e.u32(self.level);
        e.u8(self.hardcore);
        if self.version >= 2 {
            e.u8(self.expansion_status);
        }
        e.key_word();
        Ok(())
    }
}

impl Item {
    fn read_equip(d: &mut Decoder) -> Result<EquipSlot> {
        Ok(EquipSlot {
            item: Item::read(d)?,
            attached: d.u8()?,
        })
    }
}

fn write_equip(e: &mut Encoder, s: &EquipSlot) -> Result<()> {
    s.item.write(e)?;
    e.u8(s.attached);
    Ok(())
}

fn read_weapon_set(d: &mut Decoder) -> Result<WeaponSet> {
    let flag = d.u8()?;
    let a = Item::read_equip(d)?;
    let b = Item::read_equip(d)?;
    Ok(WeaponSet {
        flag,
        slots: [a, b],
    })
}

fn write_weapon_set(e: &mut Encoder, w: &WeaponSet) -> Result<()> {
    e.u8(w.flag);
    write_equip(e, &w.slots[0])?;
    write_equip(e, &w.slots[1])
}

impl HotSlot {
    fn read(d: &mut Decoder) -> Result<Self> {
        let ty = d.u32()?;
        Ok(match ty {
            0xFFFF_FFFF => HotSlot::Empty,
            0 => HotSlot::Skill {
                skill: d.string()?,
                is_item_skill: d.u8()?,
                item: d.string()?,
                equip_location: d.u32()?,
            },
            4 => HotSlot::Scroll {
                a: d.string()?,
                b: d.string()?,
                c: d.string()?,
                label: d.wstring()?,
            },
            t => HotSlot::Plain(t),
        })
    }

    fn write(&self, e: &mut Encoder) -> Result<()> {
        match self {
            HotSlot::Empty => e.u32(0xFFFF_FFFF),
            HotSlot::Skill {
                skill,
                is_item_skill,
                item,
                equip_location,
            } => {
                e.u32(0);
                e.string(skill)?;
                e.u8(*is_item_skill);
                e.string(item)?;
                e.u32(*equip_location);
            }
            HotSlot::Scroll { a, b, c, label } => {
                e.u32(4);
                e.string(a)?;
                e.string(b)?;
                e.string(c)?;
                e.wstring(label);
            }
            HotSlot::Plain(t) => e.u32(*t),
        }
        Ok(())
    }
}

impl Gdc {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(data)?;
        let seed = d.seed;
        let header = Header::read(&mut d)?;
        let data_version = d.u32()?;
        expect_version("GDC data", data_version, 8)?;
        let uid = d.uid()?;

        let info = block(&mut d, 1, |d, version| {
            expect_version("character info", version, 5)?;
            let mut c = CharacterInfo {
                version,
                is_in_main_quest: d.u8()?,
                has_been_in_game: d.u8()?,
                last_difficulty: d.u8()?,
                greatest_difficulty: d.u8()?,
                money: d.u32()?,
                greatest_survival_difficulty: d.u8()?,
                current_tribute: d.u32()?,
                compass_state: d.u8()?,
                skill_window_show_help: d.u8()?,
                alternate_config: d.u8()?,
                alternate_config_enabled: d.u8()?,
                texture: d.string()?,
                loot_filter_count: d.u32()?,
                loot_filters: Vec::new(),
            };
            for _ in 0..c.loot_filter_count.min(42) {
                c.loot_filters.push(d.u8()?);
            }
            Ok(c)
        })?;

        let bio = block(&mut d, 2, |d, version| {
            expect_version("bio", version, 8)?;
            Ok(Bio {
                version,
                level: d.u32()?,
                experience: d.u32()?,
                attribute_points: d.u32()?,
                skill_points: d.u32()?,
                devotion_points: d.u32()?,
                total_devotion: d.u32()?,
                physique: d.f32()?,
                cunning: d.f32()?,
                spirit: d.f32()?,
                health: d.f32()?,
                energy: d.f32()?,
            })
        })?;

        let inventory = block(&mut d, 3, |d, version| {
            expect_version("inventory", version, 11)?;
            if d.u8()? == 0 {
                return Ok(Inventory {
                    version,
                    data: None,
                });
            }
            let nsacks = d.u32()?;
            let focused = d.u32()?;
            let selected = d.u32()?;
            let mut sacks = Vec::new();
            for _ in 0..nsacks {
                let (block_id, end) = d.begin_block(None)?;
                let temp_bool = d.u8()?;
                let n = d.u32()?;
                let mut items = Vec::new();
                for _ in 0..n {
                    items.push(SackItem {
                        item: Item::read(d)?,
                        x: d.u32()?,
                        y: d.u32()?,
                    });
                }
                d.end_block(block_id, end)?;
                sacks.push(InventorySack {
                    block_id,
                    temp_bool,
                    items,
                });
            }
            let use_alternate = d.u8()?;
            let equipment = (0..12)
                .map(|_| Item::read_equip(d))
                .collect::<Result<Vec<_>>>()?;
            let weapons = [read_weapon_set(d)?, read_weapon_set(d)?];
            Ok(Inventory {
                version,
                data: Some(InventoryData {
                    focused,
                    selected,
                    sacks,
                    use_alternate,
                    equipment,
                    weapons,
                }),
            })
        })?;

        let stash = block(&mut d, 4, |d, version| {
            expect_version("stash", version, 11)?;
            let n = d.u32()?;
            let sacks = (0..n)
                .map(|_| StashSack::read(d))
                .collect::<Result<Vec<_>>>()?;
            Ok(Stash { version, sacks })
        })?;

        let respawns = block(&mut d, 5, |d, version| {
            expect_version("respawns", version, 1)?;
            let lists = [read_uids(d)?, read_uids(d)?, read_uids(d)?];
            let current = [d.uid()?, d.uid()?, d.uid()?];
            Ok(Respawns {
                version,
                lists,
                current,
            })
        })?;
        let uid_lists = |d: &mut Decoder, version: u32| -> Result<UidLists> {
            expect_version("uid list", version, 1)?;
            Ok(UidLists {
                version,
                lists: [read_uids(d)?, read_uids(d)?, read_uids(d)?],
            })
        };
        let teleports = block(&mut d, 6, uid_lists)?;
        let markers = block(&mut d, 7, uid_lists)?;
        let shrines = block(&mut d, 17, |d, version| {
            expect_version("shrines", version, 2)?;
            let mut l = || -> Result<[Vec<Uid>; 2]> { Ok([read_uids(d)?, read_uids(d)?]) };
            Ok(Shrines {
                version,
                lists: [l()?, l()?, l()?],
            })
        })?;

        let skills = block(&mut d, 8, |d, version| {
            expect_version("skills", version, 8)?;
            let n = d.u32()?;
            let mut skills = Vec::new();
            for _ in 0..n {
                skills.push(Skill {
                    name: d.string()?,
                    level: d.u32()?,
                    enabled: d.u8()?,
                    unk_205: d.u8()?,
                    devotion_level: d.u32()?,
                    devotion_experience: d.u32()?,
                    sublevel: d.u32()?,
                    skill_active: d.u8()?,
                    skill_transition: d.u8()?,
                    autocast_skill: d.string()?,
                    autocast_controller: d.string()?,
                });
            }
            let masteries_allowed = d.u32()?;
            let skill_reclamation_points_used = d.u32()?;
            let devotion_reclamation_points_used = d.u32()?;
            let n = d.u32()?;
            let mut item_skills = Vec::new();
            for _ in 0..n {
                item_skills.push(ItemSkill {
                    skill_name: d.string()?,
                    autocast_skill: d.string()?,
                    autocast_controller: d.string()?,
                    item_slot: d.u32()?,
                    item_name: d.string()?,
                });
            }
            let n = d.u32()?;
            let mut extra_skills = Vec::new();
            for _ in 0..n {
                extra_skills.push(ExtraSkill {
                    skill_name: d.string()?,
                    autocast_skill: d.string()?,
                    autocast_controller: d.string()?,
                    source_name: d.string()?,
                });
            }
            Ok(Skills {
                version,
                skills,
                masteries_allowed,
                skill_reclamation_points_used,
                devotion_reclamation_points_used,
                item_skills,
                extra_skills,
            })
        })?;

        let notes = block(&mut d, 12, |d, version| {
            expect_version("notes", version, 1)?;
            Ok(Notes {
                version,
                notes: read_strings(d)?,
            })
        })?;

        let factions = block(&mut d, 13, |d, version| {
            expect_version("factions", version, 5)?;
            let current = d.u32()?;
            let n = d.u32()?;
            let mut factions = Vec::new();
            for _ in 0..n {
                factions.push(Faction {
                    modified: d.u8()?,
                    unlocked: d.u8()?,
                    value: d.f32()?,
                    positive_boost: d.f32()?,
                    negative_boost: d.f32()?,
                });
            }
            Ok(Factions {
                version,
                current,
                factions,
            })
        })?;

        let ui = block(&mut d, 14, |d, version| {
            expect_version("ui", version, 7)?;
            let unk1 = d.u8()?;
            let unk2 = d.u32()?;
            let unk3 = d.u8()?;
            let mut slots = Vec::new();
            for _ in 0..5 {
                slots.push(UiSlot {
                    a: d.string()?,
                    b: d.string()?,
                    flag: d.u8()?,
                });
            }
            let nbars = d.u32()?;
            let slots_per_bar = d.u32()?;
            let mut bars = Vec::new();
            for _ in 0..nbars {
                let id = d.u32()?;
                let slots = (0..slots_per_bar)
                    .map(|_| HotSlot::read(d))
                    .collect::<Result<Vec<_>>>()?;
                bars.push(HotSlotBar { id, slots });
            }
            let camera_distance = d.f32()?;
            Ok(UiSettings {
                version,
                unk1,
                unk2,
                unk3,
                slots,
                slots_per_bar,
                bars,
                camera_distance,
            })
        })?;

        let tutorials = block(&mut d, 15, |d, version| {
            expect_version("tutorials", version, 1)?;
            let n = d.u32()?;
            Ok(Tutorials {
                version,
                pages: (0..n).map(|_| d.u32()).collect::<Result<_>>()?,
            })
        })?;

        let stats = block(&mut d, 16, |d, version| {
            expect_version("play stats", version, 12)?;
            let mut s = PlayStats {
                version,
                play_time: d.u32()?,
                deaths: d.u32()?,
                kills: d.u32()?,
                experience_from_kills: d.u32()?,
                health_potions_used: d.u32()?,
                mana_potions_used: d.u32()?,
                max_level: d.u32()?,
                hits_received: d.u32()?,
                hits_inflicted: d.u32()?,
                crits_inflicted: d.u32()?,
                crits_received: d.u32()?,
                greatest_damage_inflicted: d.f32()?,
                per_difficulty: Vec::new(),
                champion_kills: 0,
                last_hit: 0.0,
                last_hit_by: 0.0,
                greatest_damage_received: 0.0,
                hero_kills: 0,
                items_crafted: 0,
                relics_crafted: 0,
                transcendent_relics_crafted: 0,
                mythical_relics_crafted: 0,
                shrines_restored: 0,
                one_shot_chests_opened: 0,
                lore_notes_collected: 0,
                boss_kills: [0; 3],
                survival_greatest_wave: 0,
                survival_greatest_score: 0,
                survival_defenses_built: 0,
                survival_powerups_activated: 0,
                skill_stats: Vec::new(),
                endless_souls: 0,
                endless_essence: 0,
                difficulty_skip: 0,
                ultimate_veteran_boss_kills: 0,
                hidden_chests_opened: 0,
                player_u4901: 0,
                player_u4902: 0,
            };
            for _ in 0..3 {
                s.per_difficulty.push(PerDifficultyStats {
                    greatest_monster_killed_name: d.string()?,
                    greatest_monster_killed_level: d.u32()?,
                    greatest_monster_killed_life_and_mana: d.u32()?,
                    last_monster_hit: d.string()?,
                    last_monster_hit_by: d.string()?,
                });
            }
            s.champion_kills = d.u32()?;
            s.last_hit = d.f32()?;
            s.last_hit_by = d.f32()?;
            s.greatest_damage_received = d.f32()?;
            s.hero_kills = d.u32()?;
            s.items_crafted = d.u32()?;
            s.relics_crafted = d.u32()?;
            s.transcendent_relics_crafted = d.u32()?;
            s.mythical_relics_crafted = d.u32()?;
            s.shrines_restored = d.u32()?;
            s.one_shot_chests_opened = d.u32()?;
            s.lore_notes_collected = d.u32()?;
            s.boss_kills = [d.u32()?, d.u32()?, d.u32()?];
            s.survival_greatest_wave = d.u32()?;
            s.survival_greatest_score = d.u32()?;
            s.survival_defenses_built = d.u32()?;
            s.survival_powerups_activated = d.u32()?;
            let n = d.u32()?;
            for _ in 0..n {
                s.skill_stats.push((d.string()?, d.u32()?));
            }
            s.endless_souls = d.u32()?;
            s.endless_essence = d.u32()?;
            s.difficulty_skip = d.u8()?;
            s.ultimate_veteran_boss_kills = d.u32()?;
            s.hidden_chests_opened = d.u32()?;
            s.player_u4901 = d.u32()?;
            s.player_u4902 = d.u32()?;
            Ok(s)
        })?;

        let tokens = block(&mut d, 10, |d, version| {
            expect_version("tokens", version, 2)?;
            Ok(Tokens {
                version,
                lists: [read_strings(d)?, read_strings(d)?, read_strings(d)?],
            })
        })?;

        if !d.at_end() {
            bail!("{} trailing bytes after the last block", d.len() - d.pos);
        }
        Ok(Self {
            seed,
            header,
            data_version,
            uid,
            info,
            bio,
            inventory,
            stash,
            respawns,
            teleports,
            markers,
            shrines,
            skills,
            notes,
            factions,
            ui,
            tutorials,
            stats,
            tokens,
        })
    }

    /// Encodes with `self.seed`; identical input → byte-identical file.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new(self.seed);
        self.header.write(&mut e)?;
        e.u32(self.data_version);
        e.uid(&self.uid);

        let c = &self.info;
        wblock(&mut e, 1, c.version, |e| {
            e.u8(c.is_in_main_quest);
            e.u8(c.has_been_in_game);
            e.u8(c.last_difficulty);
            e.u8(c.greatest_difficulty);
            e.u32(c.money);
            e.u8(c.greatest_survival_difficulty);
            e.u32(c.current_tribute);
            e.u8(c.compass_state);
            e.u8(c.skill_window_show_help);
            e.u8(c.alternate_config);
            e.u8(c.alternate_config_enabled);
            e.string(&c.texture)?;
            e.u32(c.loot_filter_count);
            if c.loot_filters.len() != c.loot_filter_count.min(42) as usize {
                bail!("loot_filters length must be min(loot_filter_count, 42)");
            }
            for &f in &c.loot_filters {
                e.u8(f);
            }
            Ok(())
        })?;

        let b = &self.bio;
        wblock(&mut e, 2, b.version, |e| {
            for v in [
                b.level,
                b.experience,
                b.attribute_points,
                b.skill_points,
                b.devotion_points,
                b.total_devotion,
            ] {
                e.u32(v);
            }
            for v in [b.physique, b.cunning, b.spirit, b.health, b.energy] {
                e.f32(v);
            }
            Ok(())
        })?;

        let inv = &self.inventory;
        wblock(&mut e, 3, inv.version, |e| {
            let Some(i) = &inv.data else {
                e.u8(0);
                return Ok(());
            };
            e.u8(1);
            e.u32(i.sacks.len() as u32);
            e.u32(i.focused);
            e.u32(i.selected);
            for s in &i.sacks {
                e.begin_block(s.block_id);
                e.u8(s.temp_bool);
                e.u32(s.items.len() as u32);
                for it in &s.items {
                    it.item.write(e)?;
                    e.u32(it.x);
                    e.u32(it.y);
                }
                e.end_block()?;
            }
            e.u8(i.use_alternate);
            if i.equipment.len() != 12 {
                bail!("equipment must have 12 slots");
            }
            for s in &i.equipment {
                write_equip(e, s)?;
            }
            write_weapon_set(e, &i.weapons[0])?;
            write_weapon_set(e, &i.weapons[1])
        })?;

        wblock(&mut e, 4, self.stash.version, |e| {
            e.u32(self.stash.sacks.len() as u32);
            for s in &self.stash.sacks {
                s.write(e)?;
            }
            Ok(())
        })?;

        let r = &self.respawns;
        wblock(&mut e, 5, r.version, |e| {
            for l in &r.lists {
                write_uids(e, l);
            }
            for u in &r.current {
                e.uid(u);
            }
            Ok(())
        })?;
        for (id, l) in [(6, &self.teleports), (7, &self.markers)] {
            wblock(&mut e, id, l.version, |e| {
                for l in &l.lists {
                    write_uids(e, l);
                }
                Ok(())
            })?;
        }
        wblock(&mut e, 17, self.shrines.version, |e| {
            for pair in &self.shrines.lists {
                write_uids(e, &pair[0]);
                write_uids(e, &pair[1]);
            }
            Ok(())
        })?;

        let s = &self.skills;
        wblock(&mut e, 8, s.version, |e| {
            e.u32(s.skills.len() as u32);
            for k in &s.skills {
                e.string(&k.name)?;
                e.u32(k.level);
                e.u8(k.enabled);
                e.u8(k.unk_205);
                e.u32(k.devotion_level);
                e.u32(k.devotion_experience);
                e.u32(k.sublevel);
                e.u8(k.skill_active);
                e.u8(k.skill_transition);
                e.string(&k.autocast_skill)?;
                e.string(&k.autocast_controller)?;
            }
            e.u32(s.masteries_allowed);
            e.u32(s.skill_reclamation_points_used);
            e.u32(s.devotion_reclamation_points_used);
            e.u32(s.item_skills.len() as u32);
            for k in &s.item_skills {
                e.string(&k.skill_name)?;
                e.string(&k.autocast_skill)?;
                e.string(&k.autocast_controller)?;
                e.u32(k.item_slot);
                e.string(&k.item_name)?;
            }
            e.u32(s.extra_skills.len() as u32);
            for k in &s.extra_skills {
                e.string(&k.skill_name)?;
                e.string(&k.autocast_skill)?;
                e.string(&k.autocast_controller)?;
                e.string(&k.source_name)?;
            }
            Ok(())
        })?;

        wblock(&mut e, 12, self.notes.version, |e| {
            write_strings(e, &self.notes.notes)
        })?;

        let f = &self.factions;
        wblock(&mut e, 13, f.version, |e| {
            e.u32(f.current);
            e.u32(f.factions.len() as u32);
            for x in &f.factions {
                e.u8(x.modified);
                e.u8(x.unlocked);
                e.f32(x.value);
                e.f32(x.positive_boost);
                e.f32(x.negative_boost);
            }
            Ok(())
        })?;

        let u = &self.ui;
        wblock(&mut e, 14, u.version, |e| {
            e.u8(u.unk1);
            e.u32(u.unk2);
            e.u8(u.unk3);
            if u.slots.len() != 5 {
                bail!("ui must have 5 slots");
            }
            for s in &u.slots {
                e.string(&s.a)?;
                e.string(&s.b)?;
                e.u8(s.flag);
            }
            e.u32(u.bars.len() as u32);
            e.u32(u.slots_per_bar);
            for b in &u.bars {
                if b.slots.len() != u.slots_per_bar as usize {
                    bail!("hot slot bar {} must have {} slots", b.id, u.slots_per_bar);
                }
                e.u32(b.id);
                for s in &b.slots {
                    s.write(e)?;
                }
            }
            e.f32(u.camera_distance);
            Ok(())
        })?;

        wblock(&mut e, 15, self.tutorials.version, |e| {
            e.u32(self.tutorials.pages.len() as u32);
            for &p in &self.tutorials.pages {
                e.u32(p);
            }
            Ok(())
        })?;

        let s = &self.stats;
        wblock(&mut e, 16, s.version, |e| {
            for v in [
                s.play_time,
                s.deaths,
                s.kills,
                s.experience_from_kills,
                s.health_potions_used,
                s.mana_potions_used,
                s.max_level,
                s.hits_received,
                s.hits_inflicted,
                s.crits_inflicted,
                s.crits_received,
            ] {
                e.u32(v);
            }
            e.f32(s.greatest_damage_inflicted);
            if s.per_difficulty.len() != 3 {
                bail!("per_difficulty must have 3 entries");
            }
            for p in &s.per_difficulty {
                e.string(&p.greatest_monster_killed_name)?;
                e.u32(p.greatest_monster_killed_level);
                e.u32(p.greatest_monster_killed_life_and_mana);
                e.string(&p.last_monster_hit)?;
                e.string(&p.last_monster_hit_by)?;
            }
            e.u32(s.champion_kills);
            e.f32(s.last_hit);
            e.f32(s.last_hit_by);
            e.f32(s.greatest_damage_received);
            for v in [
                s.hero_kills,
                s.items_crafted,
                s.relics_crafted,
                s.transcendent_relics_crafted,
                s.mythical_relics_crafted,
                s.shrines_restored,
                s.one_shot_chests_opened,
                s.lore_notes_collected,
            ] {
                e.u32(v);
            }
            for v in s.boss_kills {
                e.u32(v);
            }
            for v in [
                s.survival_greatest_wave,
                s.survival_greatest_score,
                s.survival_defenses_built,
                s.survival_powerups_activated,
            ] {
                e.u32(v);
            }
            e.u32(s.skill_stats.len() as u32);
            for (name, n) in &s.skill_stats {
                e.string(name)?;
                e.u32(*n);
            }
            e.u32(s.endless_souls);
            e.u32(s.endless_essence);
            e.u8(s.difficulty_skip);
            e.u32(s.ultimate_veteran_boss_kills);
            e.u32(s.hidden_chests_opened);
            e.u32(s.player_u4901);
            e.u32(s.player_u4902);
            Ok(())
        })?;

        wblock(&mut e, 10, self.tokens.version, |e| {
            for l in &self.tokens.lists {
                write_strings(e, l)?;
            }
            Ok(())
        })?;
        e.finish()
    }

    /// Class display helper: mastery skill records in the skill list.
    pub fn masteries(&self) -> Vec<&Skill> {
        self.skills
            .skills
            .iter()
            .filter(|s| s.name.contains("/_classtree") || s.name.contains("_classtraining"))
            .collect()
    }
}
