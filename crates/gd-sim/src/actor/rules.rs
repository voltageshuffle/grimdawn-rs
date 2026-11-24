//! Game-wide rule records ([`Rules`]): formulas, level table, engine
//! constants, the game-balance / challenge / mutator paks, and the speed caps
//! they define.
use super::*;
use anyhow::{Context, Result};

/// Game-wide rule records.
#[derive(Clone)]
pub struct Rules {
    pub formulas: CombatFormulas,
    pub regen: crate::stats::PlayerRegen,
    pub rates: AttributeRates,
    pub levels: LevelTable,
    pub caps: EngineConstants,
    /// `gameengine.dbr` `monsterAttributePak` / `petAttributePak` /
    /// `playerAttributePak`: game-balance attributes added to every monster,
    /// pet and player at the [`Rules::balance`] column
    /// (`Monster::ContributeGameBalance*`, `Player::ContributeGameBalance*`).
    pub monster_pak: Option<Record>,
    pub pet_pak: Option<Record>,
    pub player_pak: Option<Record>,
    /// Game balance level (`GameEngine::SetGameBalanceLevel`): the paks'
    /// column, `difficulty * 4 + players - 1` (the server's
    /// player-manager update sends `difficulty * min(maxPlayers, 4) +
    /// players` (`ServerPlayerManager::UpdateGameBalance`) and `CharAttributeVal::AddToAccumulator` reads
    /// index level - 1; the port takes maxPlayers >= 4); single player:
    /// `difficulty * 4`.
    pub balance: usize,
    /// `gameengine.dbr` (speed caps).
    pub engine: Option<Record>,
    /// Veteran: `challengeAdjustment` (`balancingadjustment_challengemode_
    /// enemies01.dbr`) added to monsters on Normal
    /// (`Monster::ContributeGameBalance*`).
    pub challenge: Option<Record>,
    /// The list index `challenge` is read at (0 for Veteran; the Crucible's ramp - 1).
    pub challenge_index: usize,
    /// The Ascendant altar's monster attribute paks installed this session
    /// (`crate::ascendant::Ascendant::monster_paks`).
    pub ascendant_paks: Vec<(String, usize, crate::ascendant::Scope)>,
    /// The Crucible's active mutators' attribute packs by who they affect
    /// ([`Rules::with_mutators`]): `AttributePak`s added to every monster / the player at the
    /// contributor's character level (list index `level - 1`).
    pub mutators_monster: Vec<Record>,
    pub mutators_player: Vec<Record>,
    /// The challenge area's `difficultyAdjustment` pak ([`Rules::with_challenge_area`]): added to monsters at
    /// the [`Rules::balance`] column after the challenge pak.
    pub area_adjustment: Option<Record>,
}

/// Speed cap class (`Player::Cap*`, `Monster::Cap*`: monster or boss caps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedClass {
    Player,
    Monster,
    Boss,
}

/// One speed before its cap: the uncapped value and the absolute
/// `Cap*Speed` bounds (0 = no cap, e.g. without `gameengine.dbr`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedCap {
    pub raw: f32,
    pub min: f32,
    pub max: f32,
}

impl Default for SpeedCap {
    fn default() -> Self {
        Self {
            raw: 1.0,
            min: 0.0,
            max: 0.0,
        }
    }
}

impl SpeedCap {
    /// The speed slowed by `slow_pct` % and then capped.
    /// `Character::GetRunSpeed` / `GetAttackSpeed` /
    /// `GetSpellCastSpeed`: speed × (1 − 0.01 × slow) goes into
    /// `Cap*Speed` (`Character::CapRunSpeed` / `CapAttackSpeed` / `CapSpellCastSpeed`), so a slow can only push the
    /// speed down to the cap's min (`*SpeedCapMin` % of the bio base), and a
    /// speed above the max loses nothing until the slowed value drops below it.
    /// The slow % is `DurationDamageManager::GetFixedDamage` of the run /
    /// attack / cast type (0x13 / 0x11 / 0x12).
    pub fn capped(&self, slow_pct: f32) -> f32 {
        let v = self.raw * (1.0 - slow_pct * 0.01);
        if self.min <= 0.0 || self.max <= 0.0 {
            return if self.raw <= 0.0 { 1.0 } else { v.max(0.0) };
        }
        let v = v.max(self.min).min(self.max);
        if v <= 0.0 { 1.0 } else { v }
    }
}

/// Run, attack and cast speed from the accumulators (
/// `Character::GetRunSpeed` / `GetAttackSpeed` /
/// `GetSpellCastSpeed`): run = RunSpeed, attack = AttackSpeed ×
/// (1 + BaseAttackSpeed), cast = CastSpeed, each clamped by `Cap*`:
/// [min% × base, (max% + `character*MaxModifier`) × base] with the caps of
/// `gameengine.dbr` (`player*`, `monster*`, `boss*SpeedCapMin/Max`, the
/// min per difficulty). Dual-wielding melee weapons (hand state 10) scales
/// BaseAttackSpeed (both weapons' sum) by `dwWeaponSpeedFactor` (0.5:
/// their average; `GetAttackSpeed`). `base` is the
/// bio base value (before gear/skills): `Player/Monster::CapAttackSpeed`
/// read the bio's base accumulator (`CharacterBio::ReloadBaseAttrAcc`).
/// Returned uncapped with the bounds so slows can be folded in
/// before the cap ([`SpeedCap::capped`]).
pub fn speed_caps(
    rules: &Rules,
    acc: &CharAttrAccumulator,
    base: &CharAttrAccumulator,
    class: SpeedClass,
    dual_melee: bool,
) -> [SpeedCap; 3] {
    // `gameengine.dbr` `<class><Run|Attack|SpellCast>SpeedCapMin/Max`.
    const KEYS: [[(&str, &str); 3]; 3] = [
        [
            ("playerRunSpeedCapMin", "playerRunSpeedCapMax"),
            ("playerAttackSpeedCapMin", "playerAttackSpeedCapMax"),
            ("playerSpellCastSpeedCapMin", "playerSpellCastSpeedCapMax"),
        ],
        [
            ("monsterRunSpeedCapMin", "monsterRunSpeedCapMax"),
            ("monsterAttackSpeedCapMin", "monsterAttackSpeedCapMax"),
            ("monsterSpellCastSpeedCapMin", "monsterSpellCastSpeedCapMax"),
        ],
        [
            ("bossRunSpeedCapMin", "bossRunSpeedCapMax"),
            ("bossAttackSpeedCapMin", "bossAttackSpeedCapMax"),
            ("bossSpellCastSpeedCapMin", "bossSpellCastSpeedCapMax"),
        ],
    ];
    let difficulty = rules.balance / 4;
    let keys = &KEYS[match class {
        SpeedClass::Player => 0,
        SpeedClass::Monster => 1,
        SpeedClass::Boss => 2,
    }];
    let cap = |(min_key, max_key): (&str, &str)| -> (f32, f32) {
        let e = rules.engine.as_ref();
        let min = e
            .map(|e| {
                e.f32s_iter(min_key)
                    .nth(difficulty)
                    .or_else(|| e.f32s_iter(min_key).last())
                    .unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        let max = e.and_then(|e| e.f32(max_key)).unwrap_or(0.0);
        (min, max)
    };
    let dw = if dual_melee {
        rules
            .engine
            .as_ref()
            .and_then(|e| e.f32("dwWeaponSpeedFactor"))
            .unwrap_or(1.0)
    } else {
        1.0
    };
    let raw = [
        acc.get(CharAttr::RunSpeed),
        acc.get(CharAttr::AttackSpeed) * (1.0 + acc.get(CharAttr::BaseAttackSpeed) * dw),
        acc.get(CharAttr::CastSpeed),
    ];
    let bases = [
        base.get(CharAttr::RunSpeed),
        base.get(CharAttr::AttackSpeed),
        base.get(CharAttr::CastSpeed),
    ];
    let extra = [
        acc.get(CharAttr::RunSpeedCap),
        acc.get(CharAttr::AttackSpeedCap),
        acc.get(CharAttr::CastSpeedCap),
    ];
    let mut out = [SpeedCap::default(); 3];
    for (i, &key) in keys.iter().enumerate() {
        let (lo, hi) = cap(key);
        let b = if bases[i] > 0.0 { bases[i] } else { raw[i] };
        out[i] = if lo <= 0.0 || hi <= 0.0 || b <= 0.0 {
            SpeedCap {
                raw: raw[i],
                min: 0.0,
                max: 0.0,
            }
        } else {
            SpeedCap {
                raw: raw[i],
                min: lo * 0.01 * b,
                max: (hi
                    + if class == SpeedClass::Player {
                        extra[i]
                    } else {
                        0.0
                    })
                    * 0.01
                    * b,
            }
        };
    }
    out
}

/// Whose game-balance pak a creature gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Monster,
    Pet,
}

impl Rules {
    pub fn load(db: &Database) -> Result<Self> {
        let formulas = CombatFormulas::from_record(
            db.get("records/game/combatformulas.dbr")
                .context("combatformulas")?,
        )?;
        let levels = LevelTable::from_record(
            db.get("records/creatures/pc/playerlevels.dbr")
                .context("playerlevels")?,
        )?;
        let engine = db.get_owned("records/game/gameengine.dbr");
        let caps = EngineConstants::from_record(engine.as_ref().context("gameengine")?);
        let pak = |k: &str| {
            engine
                .as_ref()
                .and_then(|e| e.str(k))
                .and_then(|p| db.get_owned(p))
        };
        Ok(Self {
            rates: levels.rates(),
            formulas,
            regen: db
                .get("records/game/playerresourcebehavior.dbr")
                .map(crate::stats::PlayerRegen::from_record)
                .unwrap_or_default(),
            levels,
            caps,
            monster_pak: pak("monsterAttributePak"),
            pet_pak: pak("petAttributePak"),
            player_pak: pak("playerAttributePak"),
            balance: 0,
            engine,
            challenge: None,
            challenge_index: 0,
            ascendant_paks: Vec::new(),
            mutators_monster: Vec::new(),
            mutators_player: Vec::new(),
            area_adjustment: None,
        })
    }

    /// The rules for a single-player game on `difficulty` (0 normal,
    /// 1 elite, 2 ultimate).
    pub fn with_difficulty(&self, difficulty: usize) -> Self {
        Self {
            balance: difficulty.min(2) * 4,
            ..self.clone()
        }
    }

    /// Challenge modes: monsters get the challenge pak
    /// (`GameEngine::GetChallengeAdjustment`): Veteran on Normal
    /// `gameengine.dbr` `challengeAdjustment`, Ultimate Veteran
    /// `gameascendant.dbr` `ultimateChallangeAdjustment` (sic).
    /// Both are read at list index 0: a Crucible ramp's index does not carry over.
    pub fn with_veteran(&self, db: &Database, on: bool) -> Self {
        let challenge = match self.balance / 4 {
            _ if !on => None,
            0 => self
                .engine
                .as_ref()
                .and_then(|e| e.str("challengeAdjustment"))
                .and_then(|p| db.get_owned(p)),
            2 => ultimate_challenge(db).cloned(),
            _ => None,
        };
        Self {
            challenge,
            challenge_index: 0,
            ..self.clone()
        }
    }
}

impl Rules {
    /// The Crucible's monster stats at a `ramp` (`Game.SetSurvivalDifficulty`): next to the
    /// stock pak, a monster with a non-zero ramp adds the survival adjustment pak
    /// (`survivalinfo.dbr` `survivalAdjustmentNormal` / `Elite` / `Ultimate` by game difficulty,
    /// `GameEngine::GetChallengeAdjustment` in mode 1) at list index `ramp - 1`
    /// (`CharAttributeVal::AddToAccumulator`: column 0 adds nothing, else
    /// `list[min(column - 1, len - 1)]`; `Monster::ContributeGameBalanceCharAttributes`).
    /// Players and pets take no such pak. Live monsters take the new pak through
    /// `gd_game::monster_skills::rebuild_profiles` (the engine's `Monster::OnDifficultyRampLevel` reload).
    pub fn with_survival_ramp(&self, db: &Database, ramp: u32) -> Self {
        let challenge = (ramp > 0)
            .then(|| {
                let key = ["survivalAdjustmentNormal", "survivalAdjustmentElite"]
                    .get(self.balance / 4)
                    .copied()
                    .unwrap_or("survivalAdjustmentUltimate");
                let info = self
                    .engine
                    .as_ref()
                    .and_then(|e| e.str("survivalRecord"))
                    .and_then(|p| db.get(p))?;
                db.get_owned(info.str(key)?)
            })
            .flatten();
        Self {
            challenge,
            challenge_index: ramp.saturating_sub(1) as usize,
            ..self.clone()
        }
    }
}

impl Rules {
    /// The Crucible's mutators (`Game.AddMutator(record)`): each `Mutator` record
    /// (`mutatorpak_*`: `affectsPlayers`, and `normalMutator` / `eliteMutator` / `ultimateMutator`
    /// attribute packs) contributes the pack of the game difficulty to the player (`affectsPlayers`)
    /// or to every monster. `Game.AddMutator` and
    /// `Monster` / `Player::ContributeMutator*`: the pak by
    /// difficulty (`normalMutator` / `eliteMutator` / `ultimateMutator`) at column = the contributor's
    /// character level (stock paks are single-valued, so any level reads the same value).
    pub fn with_mutators(&self, db: &Database, mutators: &[String]) -> Self {
        let (mut monster, mut player) = (Vec::new(), Vec::new());
        for m in mutators {
            let Some(rec) = db.get(m) else { continue };
            let key = match self.balance / 4 {
                0 => "normalMutator",
                1 => "eliteMutator",
                _ => "ultimateMutator",
            };
            let Some(pak) = rec.str(key).and_then(|p| db.get_owned(p)) else {
                continue;
            };
            if rec.f32("affectsPlayers").is_some_and(|v| v != 0.0) {
                player.push(pak);
            } else {
                monster.push(pak);
            }
        }
        Self {
            mutators_monster: monster,
            mutators_player: player,
            ..self.clone()
        }
    }
}

impl Rules {
    /// The rules of a character standing in a challenge area (`gd_game::challenge_areas`):
    /// `Monster::ContributeGameBalance*`: a monster adds the area's `difficultyAdjustment`
    /// `AttributePak` (`ChallengeArea.difficultyPak`, loaded by `ChallengeArea::Load`) at the game-balance column,
    /// after the challenge pak (pets and players take none, `Pet::` / `Player::ContributeGameBalance*`). `mutators`
    /// (`Some`: the area's rolled `mutatorPakList` / `playerMutatorPakList` paks) replace the mutators through
    /// [`Rules::with_mutators`]: `Monster::GetMonsterMutators` / `Player::GetPlayerMutators` filter the
    /// engine's list by the area's id (type 2), so in an area only its own count; `None` keeps them (the
    /// Crucible and the Shattered Realm's dungeon, whose types win over the area's).
    pub fn with_challenge_area(
        &self,
        db: &Database,
        adjustment: Option<&str>,
        mutators: Option<&[String]>,
    ) -> Self {
        let base = match mutators {
            Some(m) => self.with_mutators(db, m),
            None => self.clone(),
        };
        Self {
            area_adjustment: adjustment
                .filter(|p| !p.is_empty())
                .and_then(|p| db.get_owned(p)),
            ..base
        }
    }
}

/// Ultimate Veteran's adjustment pak.
pub fn ultimate_challenge(db: &Database) -> Option<&Record> {
    db.get("records/game/gameascendant.dbr")
        .and_then(|e| e.str("ultimateChallangeAdjustment").map(str::to_owned))
        .and_then(|p| db.get(&p))
}
