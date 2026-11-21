//! Faction reputation (`records/game/gamefactions.dbr`).
//!
//! The character file's faction values are indexed in
//! `FactionPack::Load` order (@0x1802a2190: `factionPlayer`,
//! `factionSurvivors`, `factionAetherials`, `factionCthonians`,
//! `factionOutlaws`, `factionBeasts`, `factionDrifters`, `factionNeutralNPC`,
//! `factionUser2..40`); `gamefactions.dbr` maps those keys to faction
//! records. Ranks: `factionValue1..8` thresholds with `factionTag1..8` names
//! and `factionMarketDiscount1..8`; faction merchants check
//! `GetFactionLevelValue("Friendly"|"Respected"|"Honored"|"Revered")`
//! (= ranks 5..8) per market tier (sub_18029ac60).
//! The faction's name is the tag `tag` + key (`tagFactionSurvivors`).
//!
//! The factions window's maths (`grimdawn` `ui/factions.rs`): the engine's level table ([`Levels`]: level lookup,
//! bounds, names, discounts), the packs' window data ([`Pack`], [`enemies`]) and the bars / rollover targets
//! ([`row_bar`], [`large_bar`], [`tier_target`], [`remaining`], [`standing_colour`]). The engine's level lookup
//! rounds a negative value towards 0 ([`Levels::level_of`]); the market discount ([`Levels::discount`]) takes
//! the highest level at or below the value.
use gd_formats::arz::Record;

pub const GAME_FACTIONS: &str = "records/game/gamefactions.dbr";

/// Faction keys in save-file order.
pub const KEYS: [&str; 47] = [
    "factionPlayer",
    "factionSurvivors",
    "factionAetherials",
    "factionCthonians",
    "factionOutlaws",
    "factionBeasts",
    "factionDrifters",
    "factionNeutralNPC",
    "factionUser2",
    "factionUser3",
    "factionUser4",
    "factionUser5",
    "factionUser6",
    "factionUser7",
    "factionUser8",
    "factionUser9",
    "factionUser10",
    "factionUser11",
    "factionUser12",
    "factionUser13",
    "factionUser14",
    "factionUser15",
    "factionUser16",
    "factionUser17",
    "factionUser18",
    "factionUser19",
    "factionUser20",
    "factionUser21",
    "factionUser22",
    "factionUser23",
    "factionUser24",
    "factionUser25",
    "factionUser26",
    "factionUser27",
    "factionUser28",
    "factionUser29",
    "factionUser30",
    "factionUser31",
    "factionUser32",
    "factionUser33",
    "factionUser34",
    "factionUser35",
    "factionUser36",
    "factionUser37",
    "factionUser38",
    "factionUser39",
    "factionUser40",
];

/// Number of faction slots in a save.
pub const FACTION_COUNT: usize = KEYS.len();

/// Save index and key of a faction record (a character's `factions` field).
pub fn index_of(game_factions: &Record, faction_record: &str) -> Option<(usize, String)> {
    KEYS.iter()
        .position(|k| {
            game_factions
                .str(k)
                .is_some_and(|r| r.eq_ignore_ascii_case(faction_record))
        })
        .map(|i| (i, KEYS[i].to_string()))
}

/// Market tier tables in tab order and the level each needs.
pub const TIERS: [(&str, u32); 4] = [
    ("friendlyNormalTable", 5),
    ("respectedNormalTable", 6),
    ("honoredNormalTable", 7),
    ("reveredNormalTable", 8),
];

/// The engine's faction type names in type order, which is the save order (`FactionType` Game.dll
/// `FactionPack::GetFactionTag` @0x1802a07b0): Player, Survivors, Aetherials, Cthonians, Outlaws, Beasts, then
/// `User0` (the `factionDrifters` key), `User1` (`factionNeutralNPC`), `User2` .. `User40`. The packs' `myFaction`
/// and `killBonusNFaction` fields use these names.
pub fn type_name(i: usize) -> Option<String> {
    const FIRST: [&str; 6] = [
        "Player",
        "Survivors",
        "Aetherials",
        "Cthonians",
        "Outlaws",
        "Beasts",
    ];
    match i {
        0..6 => Some(FIRST[i].to_owned()),
        6..47 => Some(format!("User{}", i - 6)),
        _ => None,
    }
}

/// The type of a faction name ([`type_name`], any case).
pub fn type_of(name: &str) -> Option<usize> {
    let name = name.trim();
    (0..47).find(|&i| type_name(i).is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

/// `GetFactionTag`: `tagFaction` + the type name (`tagFactionSurvivors`, `tagFactionUser0` = "Rovers").
pub fn tag(i: usize) -> Option<String> {
    type_name(i).map(|n| format!("tagFaction{n}"))
}

/// `GetFactionInfoTag` (@0x1802a0f50): the tag + `Info`.
pub fn info_tag(i: usize) -> Option<String> {
    tag(i).map(|t| format!("{t}Info"))
}

/// One reputation level of the engine's table (Game.dll `GameEngine::LoadFromDatabase` sub_1802C8750):
/// `factionTagN`, `factionValueN`, the price multiplier `1 − factionMarketDiscountN × 0.01`, and for the level
/// whose value is 0 `factionAlternateNeutralTag` (the name of a negative value in that level).
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub tag: String,
    pub alt_tag: Option<String>,
    pub value: f32,
    pub discount_mult: f32,
}

/// The level table, sorted by value (`gamefactions.dbr`, n = 1..8 with a non-empty tag).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Levels(pub Vec<Level>);

impl Levels {
    pub fn from_record(gf: &Record) -> Self {
        let mut v: Vec<Level> = (1..=8)
            .filter_map(|n| {
                let tag = gf.str(&format!("factionTag{n}"))?.to_owned();
                let value = gf.i32(&format!("factionValue{n}")).unwrap_or(0) as f32;
                let discount = gf.i32(&format!("factionMarketDiscount{n}")).unwrap_or(0) as f32;
                let alt_tag = (value == 0.0)
                    .then(|| gf.str("factionAlternateNeutralTag").map(str::to_owned))
                    .flatten();
                Some(Level {
                    tag,
                    alt_tag,
                    value,
                    discount_mult: 1.0 - discount * 0.01,
                })
            })
            .collect();
        v.sort_by(|a, b| a.value.total_cmp(&b.value));
        Self(v)
    }

    /// `FactionValueToLevel` (@0x1802f3930): a negative value is in the first level (from the bottom)
    /// whose value is at or above it (it rounds towards 0), any other in the last level at or below it.
    pub fn level_of(&self, v: f32) -> Option<usize> {
        if v < 0.0 {
            self.0.iter().position(|l| v <= l.value)
        } else {
            self.0.iter().rposition(|l| v >= l.value)
        }
    }

    /// `FactionLevelToValue` (@0x1802f3a20): the index clamped to the table.
    pub fn value_of(&self, level: usize) -> f32 {
        self.0
            .get(level.min(self.0.len().saturating_sub(1)))
            .map_or(0.0, |l| l.value)
    }

    /// `GetFactionLevelTag` / `GetFactionLevelName` (@0x1802f3bc0 / @0x1802f3ab0): the level's tag, its
    /// alternate one for a negative value; empty outside the table.
    pub fn tag(&self, v: f32) -> String {
        self.level_of(v)
            .map(|i| {
                let l = &self.0[i];
                match &l.alt_tag {
                    Some(a) if v < 0.0 => a.clone(),
                    _ => l.tag.clone(),
                }
            })
            .unwrap_or_default()
    }

    /// `GetFactionLevelBounds` (@0x1802f3c60): the level's value and the next level's away from 0 (the
    /// level's own at either end of the table), as ints; (0, 0) outside the table.
    pub fn bounds(&self, v: f32) -> (i32, i32) {
        let Some(l) = self.level_of(v) else {
            return (0, 0);
        };
        let lo = self.0[l].value as i32;
        let hi = if l == 0 || l + 1 >= self.0.len() {
            lo
        } else if v < 0.0 {
            self.0[l - 1].value as i32
        } else {
            self.0[l + 1].value as i32
        };
        (lo, hi)
    }

    /// `GetFactionDiscountMultiplier` (@0x1802f3ef0): the highest level at or below `v`, else 1.
    pub fn discount_mult(&self, v: f32) -> f32 {
        self.0
            .iter()
            .rev()
            .find(|l| v >= l.value)
            .map_or(1.0, |l| l.discount_mult)
    }

    /// The market discount of the standing `v`, percent (`factionMarketDiscountN` of the highest level at or
    /// below `v`, the level [`Self::discount_mult`] prices with; 0 outside the table).
    pub fn discount(&self, v: f32) -> i32 {
        self.0
            .iter()
            .rev()
            .find(|l| v >= l.value)
            .map_or(0, |l| ((1.0 - l.discount_mult) * 100.0).round() as i32)
    }
}

/// A faction pack's window data (`factionpack.tpl` Game.dll `FactionPack::Load`): the icons, the
/// reward tags per level (`factionRewards`), whether quests / bounties give reputation and the kill-bonus factions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pack {
    pub my_faction: Option<usize>,
    pub icon: String,
    pub large_icon: String,
    pub rewards: Vec<String>,
    pub quest: bool,
    pub bounty: bool,
    pub kill_bonus: Vec<usize>,
}

impl Pack {
    pub fn from_record(r: &Record) -> Self {
        let s = |k: &str| r.str(k).unwrap_or("").to_owned();
        Self {
            my_faction: r.str("myFaction").and_then(type_of),
            icon: s("factionIcon"),
            large_icon: s("factionLargeIcon"),
            rewards: r
                .strs("factionRewards")
                .iter()
                .flat_map(|t| t.split(';'))
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .collect(),
            quest: r.bool("questEnabled"),
            bounty: r.bool("bountyEnabled"),
            kill_bonus: (1..=3)
                .filter_map(|n| r.str(&format!("killBonus{n}Faction")).and_then(type_of))
                .collect(),
        }
    }

    /// `GetTierReward` (Game.dll `FactionPack::GetTierReward`): the reward tag of `level`, clamped.
    pub fn reward(&self, level: usize) -> Option<&str> {
        self.rewards
            .get(level.min(self.rewards.len().saturating_sub(1)))
            .map(String::as_str)
    }
}

/// `GetEnemies` (Game.dll @0x1802f42c0): the tags of the factions whose packs give `faction` a kill bonus
/// (their own `myFaction`), sorted as the window does (`std::sort` of the narrow tags).
pub fn enemies(packs: &[Pack], faction: usize) -> Vec<String> {
    let mut v: Vec<String> = packs
        .iter()
        .filter(|p| p.kill_bonus.contains(&faction))
        .filter_map(|p| p.my_faction.and_then(tag))
        .collect();
    v.sort();
    v
}

/// Which of the window's bars shows a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    Positive,
    Neutral,
    Negative,
}

/// A bar's `SetValue(value, min, max)` (Grim Dawn.exe sub_14011F3E0: the fill is
/// `(value − min) / (max − min)`, unclamped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarValue {
    pub kind: BarKind,
    pub value: i32,
    pub min: i32,
    pub max: i32,
}

impl BarValue {
    pub fn frac(&self) -> f32 {
        let span = self.max as i64 - self.min as i64;
        // (bad data: a degenerate table, `max == min`, would fill NaN / inf.)
        if span == 0 {
            return 0.0;
        }
        (self.value as i64 - self.min as i64) as f32 / span as f32
    }

    /// The numbers its text shows in mode 1 (sub_14011F7F0: `value − min` / `max − min`).
    pub fn numbers(&self) -> (i32, i32) {
        (self.value - self.min, self.max - self.min)
    }
}

/// A list row's bar (Grim Dawn.exe sub_1401C1D10, with `(lo, hi)` = [`Levels::bounds`]): a negative
/// value fills the negative bar with `(−v, −lo, −hi)`, a value in a positive level the positive bar with
/// `(v, lo, hi)`, a value in the level starting at 0 the neutral bar with `(v, 0, hi)`; at either end of the table
/// (`v == lo == hi`) `lo` moves one point towards 0 so the bar is full.
pub fn row_bar(levels: &Levels, v: i32) -> BarValue {
    let (mut lo, hi) = levels.bounds(v as f32);
    if v < 0 {
        if v == lo && v == hi {
            lo += 1;
        }
        BarValue {
            kind: BarKind::Negative,
            // (bad data: a save reputation or table value cast to `i32::MIN` saturates instead of overflowing.)
            value: v.saturating_neg(),
            min: lo.saturating_neg(),
            max: hi.saturating_neg(),
        }
    } else if lo != 0 {
        if v == lo && v == hi {
            lo -= 1;
        }
        BarValue {
            kind: BarKind::Positive,
            value: v,
            min: lo,
            max: hi,
        }
    } else {
        BarValue {
            kind: BarKind::Neutral,
            value: v,
            min: lo,
            max: hi,
        }
    }
}

/// The selected faction's large bar (sub_1401C1D10): negative `(−v, 0, −value(nemesis))`, else positive
/// `(v, 0, value(revered))`, the ends of the table.
pub fn large_bar(levels: &Levels, v: i32) -> BarValue {
    let end = |i: usize| levels.value_of(i) as i32;
    if v < 0 {
        BarValue {
            kind: BarKind::Negative,
            value: v.saturating_neg(),
            min: 0,
            max: end(0).saturating_neg(),
        }
    } else {
        BarValue {
            kind: BarKind::Positive,
            value: v,
            min: 0,
            max: end(levels.0.len().saturating_sub(1)),
        }
    }
}

/// The level the large bar's rollover describes (sub_1401C1D10 / mouse sub_1401C34D0): `hover` 0 = the
/// bar itself (the current level), else the hovered tier mark: positive 1..4 = friendly, respected, honored,
/// revered (levels 4..7), negative 1 = despised (2), 3 = hated (1), 4 = nemesis (0); the negative marks are
/// `negativeTier1..3` = 1, 3, 4.
pub fn tier_target(levels: &Levels, v: i32, hover: u8) -> Option<usize> {
    let t = match (v < 0, hover) {
        (_, 0) => return levels.level_of(v as f32),
        (false, 1..=4) => 3 + hover as usize,
        (true, 1) => 2,
        (true, 3) => 1,
        (true, 4) => 0,
        _ => return levels.level_of(v as f32),
    };
    (t < levels.0.len()).then_some(t)
}

/// The points the rollover says are left to `target` (sub_1401C1D10): `value(target) − v`, at least 0 for
/// a positive value and at most 0 for a negative one, as a magnitude.
pub fn remaining(levels: &Levels, v: i32, target: usize) -> i32 {
    let d = (levels.value_of(target) - v as f32) as i32;
    if v >= 0 { d.max(0) } else { d.min(0).abs() }
}

/// The colour code before the level name in the title and the rollover (sub_1401C1D10): red for a negative
/// value, green in a positive level, yellow in the level starting at 0.
pub fn standing_colour(levels: &Levels, v: i32) -> &'static str {
    if v < 0 {
        "^r"
    } else if levels.bounds(v as f32).0 != 0 {
        "^g"
    } else {
        "^y"
    }
}

/// A faction writ / bounty (`ItemFactionBooster`, `ItemFactionWarrant`). `ItemFactionBooster::Load`
/// @0x180345190: `boostedMultiplier` and `boostedFaction` (`FactionPack::GetFactionFromString` @0x1802a16a0: the
/// type names, any case, and `NeutralNPC` for `User1`); the warrant shares the load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Booster {
    pub faction: usize,
    pub multiplier: f32,
    /// An `ItemFactionWarrant` (bounty): it boosts losses (negative boost), not gains.
    pub negative: bool,
}

impl Booster {
    pub fn from_record(r: &Record) -> Option<Self> {
        let negative = match r.class.as_str() {
            "ItemFactionBooster" => false,
            "ItemFactionWarrant" => true,
            _ => return None,
        };
        let name = r.str("boostedFaction")?;
        let faction = if name.trim().eq_ignore_ascii_case("neutralnpc") {
            7
        } else {
            type_of(name)?
        };
        Some(Self {
            faction,
            multiplier: r.f32("boostedMultiplier").unwrap_or(0.0),
            negative,
        })
    }

    /// `AllowUse` = `GameEngine::CanFactionBePositiveBoosted` @0x1802f3fd0 (writ) /
    /// `CanFactionBeNegativeBoosted` @0x1802f40c0 (bounty): the value is below the last `factionValueN` (writ) /
    /// above the first (bounty), and the faction's current boost of that sign is unset (≤ 0) or below the item's
    /// multiplier. `limits` = (first, last) `factionValueN`; `current` = the boost of the item's sign.
    pub fn allowed(&self, value: f32, current: f32, limits: (f32, f32)) -> bool {
        let room = if self.negative {
            value > limits.0
        } else {
            value < limits.1
        };
        room && (current <= 0.0 || self.multiplier > current)
    }
}

/// `GameEngine::FactionBoosterClamp` @0x1802f3870 (inlined in `BoostFactionPositive` /
/// `BoostFactionNegative`): a boost is clamped to [0, `maxBoosterValue`] (`gamefactions.dbr`), 0 when that is not
/// positive.
pub fn booster_clamp(boost: f32, max: f32) -> f32 {
    if max > 0.0 {
        boost.clamp(0.0, max)
    } else {
        0.0
    }
}
