//! Buff data of a character: the buffs other characters keep on it
//! ([`ReceivedBuffs`], one [`ReceivedBuff`] entry each: record, level,
//! modifiers, caster, time left) and its own timed self buffs
//! ([`TimedBuffs`]). The entry is the raw state: its stats count in the
//! holder's profile rebuild ([`ReceivedBuffs::stat_records`],
//! [`TimedBuffs::stat_records`]); the systems that land, tick and run them
//! are `gd_game::skill_buffs` (its module docs carry the full engine trace).
//!
//! The landing rule ([`ReceivedBuffs::land`]; `CreateUpdateSkillBuff`,
//! a list of transfers per character searched by buff name):
//! - none of that name: appended, the buff created ([`Landed::New`]);
//! - one equal in every field (`SkillBuffTransfer::Equals`: level, devotion level, modifiers,
//!   duration, caster, flags, skill, item): `SkillBuff::SetTimeToLive`
//!   (duration) on the running buff ([`Landed::Refreshed`]);
//! - same caster, other values: the transfer copied over and the buff made
//!   anew (`CreateRemoveSkillBuffConfigCmd` → `SkillManager::CreateSkillBuff`
//!   destroys the old one first, [`Landed::Replaced`]); a transfer with a
//!   count (`SkillBuffTransfer.addTimeCount`) > 0 would `AddTimeToLive`
//!   instead (`ApplyBuffOnTarget` always passes 0);
//! - another caster, equal values but the caster
//!   (`SkillBuffTransfer::EqualsButCaster`): ignored
//!   while the first caster `IsAlive`, else replaced;
//! - another caster, other values: replaced when level + its modifiers'
//!   levels + its devotion level exceed the running one's, else ignored
//!   ([`Landed::Ignored`]).
//!
//! A contagious debuff (`SkillBuff_Contageous`) carries its spread state
//! ([`Contagion`]: `spreadTimer`, `spreadBudget`, `SkillBuff.spreading`)
//! and gives its holder a plague immunity to its own record
//! ([`plague_immunity_secs`], [`ReceivedBuffs::immune`]) so a spread never
//! re-infects a recent holder.
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_formats::vfs::Database;
use gd_sim::attributes::CombatType;

/// `SkillBuff_Debuf.attackTimer`: ms between a debuff's attacks.
pub const DEBUFF_TICK_SECS: f32 = 1.0;

/// One buff another character keeps on this one (the engine's
/// `SkillBuffTransfer` with its running `SkillBuff`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedBuff {
    /// The buff record (`buffSkillName`: stats, icon, `hideFromUI`).
    pub buff: String,
    /// The skill that put it here (the aura, the curse; tooltip).
    pub skill: String,
    pub level_index: usize,
    /// The caster.
    pub from: Entity,
    /// The caster's modifiers of the skill (record, level index).
    pub mods: Vec<(String, usize)>,
    /// The applying skill's devotion level (the transfer's `devotionLevel`, module
    /// docs; 0 but for a celestial power).
    pub devotion: u32,
    /// Seconds until it runs out.
    pub left: f32,
    /// The duration it was given (s; 0 = endless).
    pub total: f32,
    /// A debuff's next attack (s; None: not an attacking debuff).
    pub tick: Option<f32>,
    /// A trap / freeze debuff's hold: its type and next refresh (s).
    pub hold: Option<(CombatType, f32)>,
    /// A `SkillBuff_Contageous`'s spread (None: another class).
    pub contagion: Option<Contagion>,
    /// Seconds to the buff's next own bonus (`SkillBuff::Update`, `SkillBuff.bonusTimer`: 0 when created, then 1 s;
    /// `gd_game::casting::Bonus::buff_duration`).
    pub heal: f32,
}

/// A `SkillBuff_Contageous` entry's spread state (module docs: `spreadTimer`,
/// `spreadBudget`, `SkillBuff.spreading`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contagion {
    /// Seconds to the next spread.
    pub next: f32,
    /// Infections left (`contagionMaxSpread` at creation).
    pub left: i32,
    /// Still spreading (cleared once `left` reaches 0 in a spread).
    pub spreading: bool,
}

impl Contagion {
    /// `SkillBuff_Contageous::Install`: the first spread after
    /// `contagionInterval`, `contagionMaxSpread` infections.
    fn install(rec: &Record, level_index: usize) -> Self {
        let p = contagion_profile(rec, level_index);
        Self {
            next: p.interval,
            left: p.max_spread,
            spreading: true,
        }
    }
}

/// A contagious buff record's `contagion*` values at a level (the
/// `SkillProfile::GetContagion*` getters: the level's entry, the last one
/// past the end, defaults without values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContagionProfile {
    /// `contagionInterval` (s; whole ms, default 1 s).
    pub interval: f32,
    /// `contagionRadius` (m; default 2).
    pub radius: f32,
    /// `contagionLimit`: infections per spread (default 1).
    pub limit: i32,
    /// `contagionMaxSpread`: infections in all (default 1).
    pub max_spread: i32,
}

pub fn contagion_profile(rec: &Record, level_index: usize) -> ContagionProfile {
    let v = |k: &str| gd_sim::stats::level_value(rec, k, level_index);
    ContagionProfile {
        interval: v("contagionInterval").map_or(1.0, |s| (s * 1000.0) as i32 as f32 / 1000.0),
        radius: v("contagionRadius").unwrap_or(2.0),
        limit: v("contagionLimit").map_or(1, |n| n as i32),
        max_spread: v("contagionMaxSpread").map_or(1, |n| n as i32),
    }
}

/// `SkillBuff_Contageous::Install`: the holder's plague immunity to that
/// buff, (int)(its total ms × 1.5 + 4000) ms.
pub fn plague_immunity_secs(total: f32) -> f32 {
    ((total * 1000.0).round() * 1.5 + 4000.0) as i32 as f32 / 1000.0
}

impl ReceivedBuff {
    /// A buff of `total` seconds, attacking when its record is an attacking
    /// debuff ([`attacks`]).
    pub fn new(
        db: &Database,
        buff: &str,
        skill: &str,
        level_index: usize,
        from: Entity,
        mods: Vec<(String, usize)>,
        total: f32,
    ) -> Self {
        let rec = db.get(buff);
        let tick = rec.is_some_and(attacks).then_some(0.0);
        let hold = rec.and_then(holds).map(|t| (t, 0.0));
        let contagion = rec
            .filter(|r| r.class == CONTAGEOUS)
            .map(|r| Contagion::install(r, level_index));
        Self {
            buff: buff.to_owned(),
            skill: skill.to_owned(),
            level_index,
            from,
            mods,
            devotion: 0,
            left: total,
            total,
            tick,
            hold,
            contagion,
            heal: 0.0,
        }
    }

    /// The level sum the "other caster" rule compares (module docs).
    fn strength(&self) -> usize {
        self.level_index
            + 1
            + self.mods.iter().map(|m| m.1 + 1).sum::<usize>()
            + self.devotion as usize
    }

    /// Equal but for the caster (`SkillBuffTransfer::EqualsButCaster`).
    fn same_values(&self, o: &ReceivedBuff) -> bool {
        self.level_index == o.level_index
            && self.devotion == o.devotion
            && self.mods == o.mods
            && self.total == o.total
            && self.skill.eq_ignore_ascii_case(&o.skill)
    }
}

/// A debuff record whose `Update` attacks (module docs): its holder, or
/// for a `SkillBuff_DebufRadius` the caster's foes around it.
/// A `SkillBuff_DebufProjectile` launches its projectile from the holder on
/// that timer instead ([`DEBUF_PROJECTILE`]).
pub fn attacks(rec: &Record) -> bool {
    matches!(
        rec.class.as_str(),
        "SkillBuff_Debuf"
            | "SkillBuff_DebufTrap"
            | "SkillBuff_DebufFreeze"
            | "SkillBuff_DebufRadius"
            | CONTAGEOUS
            | DEBUF_PROJECTILE
    )
}

/// The projectile-launching debuff class (module docs).
pub const DEBUF_PROJECTILE: &str = "SkillBuff_DebufProjectile";

/// The dispelling buff class (module docs).
pub const DISPEL_MAGIC: &str = "SkillBuff_DispelMagic";

/// The contagious debuff class (module docs).
pub const CONTAGEOUS: &str = "SkillBuff_Contageous";

/// What a trap / freeze debuff holds its holder in (module docs).
pub fn holds(rec: &Record) -> Option<CombatType> {
    match rec.class.as_str() {
        "SkillBuff_DebufTrap" => Some(CombatType::Trap),
        "SkillBuff_DebufFreeze" => Some(CombatType::Freeze),
        _ => None,
    }
}

/// `SkillBuff_DebufTrap.holdTimer` / `.maxHold`: ms between holds, the longest hold.
pub const HOLD_EVERY_SECS: f32 = 0.1;
pub const HOLD_SECS: f32 = 0.25;

/// A `SkillBuff_Debuf*` record (`SkillBuff_Contageous` is one too: its
/// constructor runs `SkillBuff_Debuf`'s, module docs).
pub fn is_debuff(rec: &Record) -> bool {
    is_debuff_class(&rec.class)
}

/// A buff record a curse wrapper lands on its targets through
/// `Character::DebufTarget` rather than hitting them: the debuffs and the
/// dispelling buff (Nullification's `SkillBuff_DispelMagic`, module docs).
pub fn lands_as_debuff(rec: &Record) -> bool {
    is_debuff(rec) || rec.class == DISPEL_MAGIC
}

/// [`is_debuff`] by class name.
pub fn is_debuff_class(class: &str) -> bool {
    class.starts_with("SkillBuff_Debuf") || class == CONTAGEOUS
}

/// The timed self buffs of one character — the one store the player
/// (`gd_game::player::Loadout::buffs`), monsters
/// (`gd_game::monster_skills::MonsterSkills::buffs`) and pets
/// (`gd_game::pets::Pet::buffs`) share: (skill record, level index, seconds
/// left), plus the learned modifiers applied with each buff
/// (`SkillBuff::CollectPassiveCharAttributes` adds the level
/// stats and `Skill::AddModifierCharAttributes` while active), and each running buff's full duration (the
/// `SkillBuff`'s total: a duration refresh never takes the time left past it, `gd_game::refresh`).
/// The engine keeps one `SkillBuff` list per character; the stats it gives
/// derive from the list at the profile rebuild ([`Self::stat_records`], no
/// cached copy), and one system ticks and expires every character's list
/// (`gd_game::casting::expire_buffs`); death stops the timed entries (`gd_game::skill_buffs::stop_buffs_on_death`). A shapeshift form is an
/// entry with no end (`f32::INFINITY`): it leaves with the form, not by expiry, dispel or death.
#[derive(Component, Clone, Debug, Default)]
pub struct TimedBuffs {
    pub list: Vec<(String, usize, f32)>,
    /// Buff → its learned modifiers (record, level index); the player's
    /// (monsters and pets leave it empty).
    pub mods: HashMap<String, Vec<(String, usize)>>,
    /// Buff → its full duration (s), set when it starts.
    full: HashMap<String, f32>,
}

impl TimedBuffs {
    /// A buff starts (or starts again) under `skill`'s name; the old
    /// entry's modifiers go with it.
    pub fn add(&mut self, skill: &str, li: usize, secs: f32) {
        self.list.retain(|b| b.0 != skill);
        self.mods.remove(skill);
        self.list.push((skill.to_owned(), li, secs));
        self.full.insert(skill.to_owned(), secs);
    }

    /// Whether `skill`'s buff runs.
    pub fn running(&self, skill: &str) -> bool {
        self.list.iter().any(|b| b.0.eq_ignore_ascii_case(skill))
    }

    /// The full duration of `skill`'s running buff.
    pub fn full(&self, skill: &str) -> Option<f32> {
        self.running(skill)
            .then(|| self.full.get(skill).copied())
            .flatten()
    }

    /// Runs the list down by `dt`, except the entries `held` names (a life monitor waiting for life to come back,
    /// `gd_game::autocast::Autocasts::holds`); the expired names return (their effects
    /// and modifiers go, the caller plays the sounds and marks the profile
    /// dirty). A form entry (`f32::INFINITY`) never expires.
    pub fn tick(&mut self, dt: f32, held: impl Fn(&str) -> bool) -> Vec<String> {
        for b in self.list.iter_mut().filter(|b| !held(&b.0)) {
            b.2 -= dt;
        }
        let gone: Vec<String> = self
            .list
            .iter()
            .filter(|b| b.2 <= 0.0)
            .map(|b| b.0.clone())
            .collect();
        if !gone.is_empty() {
            self.list.retain(|b| b.2 > 0.0);
            for g in &gone {
                self.mods.remove(g);
                self.full.remove(g);
            }
        }
        gone
    }

    /// Ends every timed entry at once (death's `StopSkill`, `gd_game::skill_buffs::stop_buffs_on_death`); the ended names return as
    /// [`Self::tick`]'s do. A form entry (`f32::INFINITY`) stays: it leaves with the form.
    pub fn stop_timed(&mut self) -> Vec<String> {
        for b in self.list.iter_mut().filter(|b| b.2.is_finite()) {
            b.2 = 0.0;
        }
        self.tick(0.0, |_| false)
    }

    /// Ends the entries `pick` names (a potion modifier switched out, `gd_game::cauldron`); the ended names return
    /// as [`Self::tick`]'s do.
    pub fn stop_where(&mut self, pick: impl Fn(&str) -> bool) -> Vec<String> {
        let gone: Vec<String> = self
            .list
            .iter()
            .filter(|b| pick(&b.0))
            .map(|b| b.0.clone())
            .collect();
        self.list.retain(|b| !pick(&b.0));
        for g in &gone {
            self.mods.remove(g);
            self.full.remove(g);
        }
        gone
    }

    /// The stats the list gives: each entry's profile record (a wrapper's
    /// `buffSkillName`, else the entry itself) and its learned modifiers',
    /// at their levels — what the profile rebuild sums.
    pub fn stat_records(&self, db: &Database) -> Vec<(Record, usize)> {
        let mut out = Vec::new();
        for (s, li, _) in &self.list {
            let Some(r) = db.get_owned(s) else { continue };
            match gd_sim::skills::profile_name(&r).and_then(|b| db.get_owned(b)) {
                Some(p) => out.push((p, *li)),
                None => out.push((r, *li)),
            }
            if let Some(mods) = self.mods.get(s) {
                for (m, ml) in mods {
                    if let Some(r) = db.get_owned(m) {
                        out.push((r, *ml));
                    }
                }
            }
        }
        out
    }
}

/// The buffs other characters keep on this one, newest first (module docs).
#[derive(Component, Default, Clone, Debug)]
pub struct ReceivedBuffs {
    pub list: Vec<ReceivedBuff>,
    /// `Character::SetImmuneToPlagueTime`: contagious buff records this
    /// character can't catch by spread for the seconds left (module docs).
    pub immune: Vec<(String, f32)>,
    /// A buff came or went since the profile was last built.
    pub changed: bool,
}

/// What landing a buff did ([`ReceivedBuffs::land`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landed {
    /// Not on before: created.
    New,
    /// The running one made anew with the new values.
    Replaced,
    /// The running one's time reset.
    Refreshed,
    /// The running one kept; the new one dropped.
    Ignored,
}

impl Landed {
    /// A `SkillBuff` was created (it goes active: cast sound, effects).
    pub fn created(self) -> bool {
        matches!(self, Landed::New | Landed::Replaced)
    }
}

impl ReceivedBuffs {
    /// The stat records: each buff at its level, then its modifiers,
    /// without their energy reserve (`characterManaLimitReserve…`), a
    /// debuff's also without its `offensive*` fields (module docs).
    /// A `SkillBuff` adds its profile's `CharAttributeStore_Skill`
    /// (`SkillProfile.charAttributes`), which reads no reserve field; the reserve is the toggle's
    /// penalty store on the caster (`SkillBook::stat_sources`). A pet under
    /// the player's Elemental Infusion shows a full energy bar in the real
    /// game.
    pub fn stat_records(&self, db: &Database) -> Vec<(Record, usize)> {
        let strip = |mut r: Record, debuff: bool| {
            std::sync::Arc::make_mut(&mut r.fields).retain(|(k, _)| {
                !k.starts_with("characterManaLimitReserve")
                    && !(debuff && k.starts_with("offensive"))
            });
            r
        };
        let mut out = Vec::new();
        for b in &self.list {
            let Some(r) = db.get_owned(&b.buff) else {
                continue;
            };
            let debuff = is_debuff(&r);
            out.push((strip(r, debuff), b.level_index));
            out.extend(
                b.mods
                    .iter()
                    .filter_map(|(m, li)| db.get_owned(m).map(|r| (strip(r, debuff), *li))),
            );
        }
        out
    }

    /// Whether `buff` (a record name) is on.
    pub fn has(&self, buff: &str) -> bool {
        self.list.iter().any(|b| b.buff.eq_ignore_ascii_case(buff))
    }

    /// `Character::IsImmuneToPlague(buff)`.
    pub fn is_immune(&self, buff: &str) -> bool {
        self.immune
            .iter()
            .any(|i| i.0.eq_ignore_ascii_case(buff) && i.1 > 0.0)
    }

    /// `Character::SetImmuneToPlagueTime(buff, secs)`: set or replaced.
    pub fn set_immune(&mut self, buff: &str, secs: f32) {
        match self
            .immune
            .iter_mut()
            .find(|i| i.0.eq_ignore_ascii_case(buff))
        {
            Some(i) => i.1 = secs,
            None => self.immune.push((buff.to_owned(), secs)),
        }
    }

    /// `CreateUpdateSkillBuff` (module docs). `alive`: whether a caster is
    /// still alive.
    pub fn land(&mut self, b: ReceivedBuff, alive: impl Fn(Entity) -> bool) -> Landed {
        let Some(i) = self
            .list
            .iter()
            .position(|x| x.buff.eq_ignore_ascii_case(&b.buff))
        else {
            self.list.insert(0, b);
            self.changed = true;
            return Landed::New;
        };
        let old = &mut self.list[i];
        let replace = if old.from == b.from {
            if old.same_values(&b) {
                old.left = b.total;
                return Landed::Refreshed;
            }
            true
        } else if old.same_values(&b) {
            !alive(old.from)
        } else {
            b.strength() > old.strength()
        };
        if !replace {
            return Landed::Ignored;
        }
        self.list.remove(i);
        self.list.insert(0, b);
        self.changed = true;
        Landed::Replaced
    }

    /// Takes the "changed" flag.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}
