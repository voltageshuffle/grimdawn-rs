//! The skills a character's HUD status icons list: the player's buff bar and the small icons under a pet portrait.
//! Read-only queries of the gameplay state; the UI keeps the bar's own state (icon order, fade-out, cooldown phase).
//!
//! The player (Grim Dawn.exe `PlayerStatusIcons::Update`), in this order:
//! 1. `SkillManager::GetActiveSkillList` — the character's skills (save order), then its item skills, then its sub
//!    skills — each that `Skill::HasIconDisplay`, i.e. `IsTrackable`: false for the base `Skill` and
//!    `Skill_BuffRadius{,Toggled}` without `debufSkill` (their buff shows from step 2). Per class (Game.dll): the
//!    `Skill_BuffSelfToggled` family (`…Toggled`, `…Duration`, `…Shield`, `…Invulnerable`, `…Immobilize`,
//!    `…Colossus`, `…Sick`, `Skill_Shapeshift`), `Skill_BuffAttackRadius{Toggled,Duration,Lightning}` and
//!    `Skill_OnHitAttackRadius`: their active flag; `Skill_PassiveOnHitBuffSelf` / `…OnCritBuffSelf` /
//!    `…OnLifeBuffSelf` / `…OnHitBuffShield`: while their buff runs; `Skill_TargetedSpawnPet` with `trackSpawns`:
//!    while one of its pets lives; `Skill_WeaponPool_Charged*`: with charges; `Skill_ChargePotion`: while its
//!    absorption lasts.
//! 2. `Character::GetUITempSkillList`: the `SkillBuff`s on the character that are not `hideFromUI`;
//!    `SkillBuff_Debuf*` go to the debuff area.
//!
//! What an icon shows comes from `GetTrackableType`: 0 duration (a `SkillBuff`'s total is 0 when ≤ 1500 ms, so an
//! aura's buff has no timer), 1 charges (living pets, `Skill_WeaponPool_Charged…`, `SkillBuff_PassiveCharged`),
//! 2 Cadence's finale, 3 no timer (the `…Toggled` skills, `Skill_Shapeshift` with a set flag), 4 potions /
//! `Skill_PassiveOnHitBuffShield`. Passives (`Skill::IsPassive`) are drawn round and keep their icon through their
//! cooldown after the buff ends.
//!
//! A pet portrait (`Character::GetActiveStatusSkills`): the same two lists, as skill names; the icon is each
//! skill's up bitmap.
//!
//! PORT: the port has no `SkillBuff` objects. Step 2 for the player is its own switched-on
//! `Skill_BuffRadiusToggled` auras' buffs, then the buffs other characters keep on it (newest first, a curse with
//! its time left), then the timed buffs it got from a `Skill_BuffRadius` / `Skill_BuffOther`. For a pet: its
//! switched-on toggles, damage auras and timed self buffs, then the buffs on it, newest first. Item / sub skills
//! are the timed buffs left over after the learned skills.
//!
//! The icon: `GetTrackableIcon` when not empty, else the skill's up bitmap; only `Skill_ChargePotion` has one,
//! [`POTION_ABSORB_ICON`], type 4: left / total of the absorb pool, no text.
//!
//! Combo charges (`PlayerStatusIcons::UpdateComboCharge`): after the lists and the DoT icons, one entry of kind 2
//! in area 1 while `GetComboChargeMultiplier(count)` ≥ 1, icon [`COMBO_ICON`], the count as text and the time left
//! swept against the most seen (≤ 250 ms: no total); its rollover is `tagHUDComboCharge` /
//! `tagHUDComboChargeMultiplier`, not a skill tooltip.
use crate::combo::Combo;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;
use gd_game_core::casting::{ShieldKind, Shields};
use gd_game_core::player::Loadout;
use gd_game_core::skill_buffs::ReceivedBuffs;
use gd_game_core::weapon_pool::PoolCharges;
use gd_sim::combo::ComboTable;

/// `GetTrackableType`.
#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    /// Seconds left of a timed buff (its total is what the UI first saw).
    Duration(f32),
    /// A count drawn on the icon (living pets, weapon-pool charges) and its
    /// maximum (`TrackableTotalTime`: the pet limit / charge level at the
    /// skill's level), swept as (max - count) / max.
    Charges(u32, u32),
    /// No timer (toggles, auras' buffs).
    Plain,
    /// Type 4 (a potion's absorb): the sweep of `left` / `total`, no text.
    Gauge { left: f32, total: f32 },
    /// Combo charges: the count, seconds left and the multiplier (%).
    Combo {
        count: u32,
        left: f32,
        multiplier: f32,
    },
}

pub const COMBO_ICON: &str = "ui/hud/hud_combochargeicon.tex";

pub const COMBO_KEY: &str = "combo charges";

/// `Skill_ChargePotion::GetTrackableIcon`.
pub const POTION_ABSORB_ICON: &str =
    "ui/skills/icons/potionbuffs/skillicon_potion_absorbing_up.tex";

#[derive(Clone, Copy)]
pub struct PlayerState<'a> {
    pub loadout: &'a Loadout,
    pub links: Option<&'a gd_game_core::weapon_pool::SkillLinks>,
    pub pool: Option<&'a PoolCharges>,
    /// The summon skill of each living pet.
    pub pets: &'a [&'a str],
    pub received: Option<&'a ReceivedBuffs>,
    pub shield: Option<&'a Shields>,
    /// The player's combo charges and `gameengine.dbr`'s table.
    pub combo: Option<(&'a Combo, &'a ComboTable)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tracked {
    /// Identity in the bar (lower-case record of the tracked skill or buff).
    pub key: String,
    /// The record whose `skillUpBitmapName` is drawn, or a `.tex` path.
    pub icon: String,
    /// The skill whose tooltip the icon shows.
    pub tip: String,
    pub track: Track,
    /// A debuff (`SkillBuff_Debuf*`): the second area.
    pub debuff: bool,
    /// Drawn round (`roundBitmap` of it or its parent, or a passive).
    pub round: bool,
    /// A passive (`Skill::IsPassive`): its icon stays through its cooldown, `Caster::cooldowns[cooldown]`.
    pub passive: bool,
    pub cooldown: String,
}

fn self_trackable(class: &str) -> bool {
    class.starts_with("Skill_BuffSelf")
        || class == "Skill_Shapeshift"
        || class.starts_with("Skill_BuffAttackRadius")
        || class == "Skill_OnHitAttackRadius"
}

/// `Skill::IsPassive`.
pub fn is_passive(class: &str) -> bool {
    class.starts_with("Skill_Passive")
        || class == "Skill_Mastery"
        || class == "SkillSecondary_BuffSelfDuration"
}

fn plain(class: &str) -> bool {
    class.ends_with("Toggled") || class == "Skill_OnHitAttackRadius"
}

fn round(rec: &Record) -> bool {
    rec.bool("roundBitmap")
}

/// The buff record a `Skill_BuffRadius` / `Skill_BuffOther` / toggled aura puts on its targets.
fn buff_of<'a>(db: &'a Database, rec: &'a Record) -> Option<(&'a str, &'a Record)> {
    let b = gd_sim::skills::profile_name(rec)?;
    db.get(b).map(|r| (b, r))
}

/// (debuff, round, passive); round is its own `roundBitmap`, `parent_round` or a passive.
fn flags(rec: &Record, parent_round: bool) -> (bool, bool, bool) {
    let passive = is_passive(&rec.class);
    (
        gd_game_core::skill_buffs::is_debuff(rec),
        round(rec) || parent_round || passive,
        passive,
    )
}

/// How a skill of the active skill list (step 1) is tracked.
#[derive(Clone, Debug)]
enum SkillKind {
    /// A switched-on self / attack-radius buff (`self_trackable`): its running buff's time, or no timer (`plain`).
    SelfBuff { plain: bool },
    /// A switched-on debuf radius aura: no timer.
    DebufAura,
    /// A passive / `Skill_PassiveOnHitBuffShield`: while it or its buff record runs.
    Proc { buff: Option<String> },
    /// `Skill_TargetedSpawnPet` with `trackSpawns`: its living pets of the pet limit at its level.
    Spawns { max: u32 },
    /// `Skill_ChargePotion`: while its absorb lasts.
    Potion,
    /// `Skill_WeaponPool_Charged*`: with charges; Cadence's finale (no count), else of the charge level.
    Pool { finale: bool, max: u32 },
}

/// A skill of the active skill list that can show, with what does not change until the loadout does.
#[derive(Clone, Debug)]
struct Listed {
    name: String,
    lower: String,
    kind: SkillKind,
    flags: (bool, bool, bool),
}

/// One of the player's own switched-on auras' buff (step 2).
#[derive(Clone, Debug)]
struct OwnAura {
    buff: String,
    skill: String,
    flags: (bool, bool, bool),
}

/// Writes the list into a kept `Vec` in place, reusing the entries' strings.
struct Writer<'o> {
    out: &'o mut Vec<Tracked>,
    n: usize,
}

impl Writer<'_> {
    fn put(&mut self, key: &str, icon: &str, tip: &str, track: Track, flags: (bool, bool, bool)) {
        let (debuff, round, passive) = flags;
        if let Some(t) = self.out.get_mut(self.n) {
            t.key.clear();
            t.key.extend(key.chars().map(|c| c.to_ascii_lowercase()));
            for (dst, src) in [
                (&mut t.icon, icon),
                (&mut t.tip, tip),
                (&mut t.cooldown, tip),
            ] {
                dst.clear();
                dst.push_str(src);
            }
            (t.track, t.debuff, t.round, t.passive) = (track, debuff, round, passive);
        } else {
            self.out.push(Tracked {
                key: key.to_ascii_lowercase(),
                icon: icon.to_owned(),
                tip: tip.to_owned(),
                track,
                debuff,
                round,
                passive,
                cooldown: tip.to_owned(),
            });
        }
        self.n += 1;
    }
}

/// The player's status list kept across frames: [`Self::list`] resolves the skill list's records when the loadout
/// changes; every other frame it only reads the running state.
#[derive(Default)]
pub struct PlayerIcons {
    built: bool,
    skills: Vec<Listed>,
    auras: Vec<OwnAura>,
    used: Vec<usize>,
}

impl PlayerIcons {
    fn refresh(
        &mut self,
        db: &Database,
        loadout: &Loadout,
        links: Option<&gd_game_core::weapon_pool::SkillLinks>,
    ) {
        self.built = true;
        self.skills.clear();
        self.auras.clear();
        let skills = loadout.skills_and_item_toggles(db);
        for (s, l) in &skills {
            if *l == 0 {
                continue;
            }
            let Some(rec) = db.get(s) else { continue };
            let c = rec.class.as_str();
            let li = (*l as usize).saturating_sub(1);
            let kind = if self_trackable(c) && loadout.skill_active(db, s) {
                SkillKind::SelfBuff { plain: plain(c) }
            } else if (c == "Skill_BuffRadius" || c == "Skill_BuffRadiusToggled")
                && crate::auras::is_debuf(db, rec)
                && loadout.skill_active(db, s)
            {
                SkillKind::DebufAura
            } else if is_passive(c) || c == "Skill_PassiveOnHitBuffShield" {
                // A proc's buff runs under the passive's or its buff's name.
                SkillKind::Proc {
                    buff: buff_of(db, rec).map(|b| b.0.to_owned()),
                }
            } else if c == "Skill_TargetedSpawnPet" && rec.bool("trackSpawns") {
                // Skill_TargetedSpawnPet::TrackableTotalTime: GetPetLimit(level), the modifier-summed limit.
                let mods: Vec<(gd_formats::arz::Record, usize)> = links
                    .map(|l| {
                        l.mods_of(s)
                            .iter()
                            .filter_map(|(m, ml)| db.get_owned(m).map(|r| (r, *ml)))
                            .collect()
                    })
                    .unwrap_or_default();
                let max = gd_sim::skills::pet_limit(rec, li, &mods).max(1.0) as u32;
                SkillKind::Spawns { max }
            } else if c == "Skill_ChargePotion" {
                SkillKind::Potion
            } else if c.starts_with("Skill_WeaponPool_Charged") {
                // Skill_WeaponPool_ChargedFinale::GetTrackableType returns 2: Cadence's icon has no count and no
                // sweep.
                let finale = c == "Skill_WeaponPool_ChargedFinale";
                // Skill_WeaponPool_ChargedScaling::TrackableTotalTime: the charge level.
                let max = gd_sim::stats::level_value(rec, "skillChargeLevel", li)
                    .unwrap_or(1.0)
                    .max(1.0) as u32;
                SkillKind::Pool { finale, max }
            } else {
                continue;
            };
            self.skills.push(Listed {
                name: s.clone(),
                lower: s.to_ascii_lowercase(),
                kind,
                flags: flags(rec, false),
            });
        }
        // 2. The player's own switched-on auras put their buff on it.
        for (s, l) in &skills {
            if *l == 0 {
                continue;
            }
            let Some(rec) = db.get(s) else { continue };
            if rec.class != "Skill_BuffRadiusToggled"
                || crate::auras::is_debuf(db, rec)
                || !loadout.skill_active(db, s)
            {
                continue;
            }
            let Some((b, brec)) = buff_of(db, rec) else {
                continue;
            };
            if !brec.bool("hideFromUI") {
                self.auras.push(OwnAura {
                    buff: b.to_owned(),
                    skill: s.clone(),
                    flags: flags(brec, round(rec)),
                });
            }
        }
    }

    /// Writes the player's status list in the engine's order into `out`. `loadout_changed`: the loadout changed
    /// since the last call.
    pub fn list(
        &mut self,
        db: &Database,
        st: &PlayerState,
        loadout_changed: bool,
        out: &mut Vec<Tracked>,
    ) {
        let PlayerState {
            loadout,
            links,
            pool,
            pets,
            received,
            shield,
            combo,
        } = *st;
        if loadout_changed || !self.built {
            self.refresh(db, loadout, links);
        }
        let mut w = Writer { out, n: 0 };
        let used = &mut self.used;
        used.clear();
        let running = |name: &str| {
            loadout
                .buffs
                .list
                .iter()
                .position(|b| b.0.eq_ignore_ascii_case(name))
        };
        // 1. The active skill list: learned skills that are trackable, then the worn items' toggles.
        for l in &self.skills {
            let s = l.name.as_str();
            let track = match &l.kind {
                SkillKind::SelfBuff { plain } => match running(s) {
                    Some(i) if !plain => {
                        used.push(i);
                        Track::Duration(loadout.buffs.list[i].2)
                    }
                    _ => Track::Plain,
                },
                SkillKind::DebufAura => Track::Plain,
                SkillKind::Proc { buff } => {
                    let Some(i) = running(s).or_else(|| buff.as_deref().and_then(running)) else {
                        continue;
                    };
                    used.push(i);
                    Track::Duration(loadout.buffs.list[i].2)
                }
                SkillKind::Spawns { max } => {
                    let n = pets.iter().filter(|p| p.eq_ignore_ascii_case(s)).count() as u32;
                    if n == 0 {
                        continue;
                    }
                    Track::Charges(n, *max)
                }
                SkillKind::Potion => {
                    let absorb = shield.and_then(|sh| {
                        sh.potion()
                            .filter(|p| p.skill.eq_ignore_ascii_case(s))
                            .and_then(|p| match p.kind {
                                ShieldKind::Potion(total) => Some((p.absorb.flat, total)),
                                _ => None,
                            })
                    });
                    let Some((left, total)) = absorb.filter(|a| a.0 > 0.0) else {
                        continue;
                    };
                    w.put(
                        s,
                        POTION_ABSORB_ICON,
                        s,
                        Track::Gauge { left, total },
                        l.flags,
                    );
                    continue;
                }
                SkillKind::Pool { finale, max } => {
                    let n = pool
                        .and_then(|p| p.0.get(&l.lower))
                        .map_or(0, |ch| ch.count);
                    if n == 0 {
                        continue;
                    }
                    if *finale {
                        Track::Plain
                    } else {
                        Track::Charges(n, *max)
                    }
                }
            };
            w.put(s, s, s, track, l.flags);
        }
        // Item and sub skills: the timed buffs not matched above. Step 2's temp skills go after the auras' buffs.
        let temp = |rec: &Record| {
            matches!(
                rec.class.as_str(),
                "Skill_BuffRadius" | "Skill_BuffOther" | "Skill_ChargedBuffOther"
            ) && buff_of(db, rec).is_some()
                || rec.class.starts_with("SkillBuff")
        };
        for (i, (s, _, left)) in loadout.buffs.list.iter().enumerate() {
            if used.contains(&i) || !left.is_finite() {
                continue;
            }
            let Some(rec) = db.get(s) else { continue };
            if !temp(rec) {
                w.put(s, s, s, Track::Duration(*left), flags(rec, false));
            }
        }
        // 2. The player's own switched-on auras put their buff on it.
        let auras_from = w.n;
        for a in &self.auras {
            w.put(&a.buff, &a.buff, &a.skill, Track::Plain, a.flags);
        }
        // Allies' auras' buffs, unless the player's own aura has the same buff.
        for b in received.map(|r| r.list.as_slice()).unwrap_or_default() {
            let Some(brec) = db.get(&b.buff) else {
                continue;
            };
            if brec.bool("hideFromUI")
                || w.out[auras_from..w.n]
                    .iter()
                    .any(|a| b.buff.eq_ignore_ascii_case(&a.key))
            {
                continue;
            }
            let parent = db.get(&b.skill).is_some_and(round);
            // `SkillBuff::TrackableTotalTime` is 0 for ≤ 1500 ms (an aura pass's buff): no timer.
            let track = if b.total > crate::auras::BUFF_SECS {
                Track::Duration(b.left)
            } else {
                Track::Plain
            };
            w.put(&b.buff, &b.buff, &b.skill, track, flags(brec, parent));
        }
        // 2. The temp skills: a radius / other buff lands on the player as its buff record; a buff someone cast on
        // the player (a pet's `Skill_BuffOther`) runs under its buff record.
        for (i, (s, _, left)) in loadout.buffs.list.iter().enumerate() {
            if used.contains(&i) || !left.is_finite() {
                continue;
            }
            let Some(rec) = db.get(s) else { continue };
            if !temp(rec) {
                continue;
            }
            if let Some((b, brec)) = buff_of(db, rec).filter(|_| {
                matches!(
                    rec.class.as_str(),
                    "Skill_BuffRadius" | "Skill_BuffOther" | "Skill_ChargedBuffOther"
                )
            }) {
                if !brec.bool("hideFromUI") {
                    w.put(b, b, s, Track::Duration(*left), flags(brec, round(rec)));
                }
            } else if !rec.bool("hideFromUI") {
                w.put(s, s, s, Track::Duration(*left), flags(rec, false));
            }
        }
        if let Some((c, table)) = combo
            && c.0.count > 0
        {
            let multiplier = table.multiplier(c.0.count);
            if multiplier >= 1.0 {
                let track = Track::Combo {
                    count: c.0.count,
                    left: c.0.remaining_ms.max(0) as f32 / 1000.0,
                    multiplier,
                };
                w.put(COMBO_KEY, COMBO_ICON, "", track, (false, false, false));
            }
        }
        w.out.truncate(w.n);
    }
}

/// The icons under a pet's portrait: its trackable skills (switched-on toggles, damage auras, timed self buffs),
/// then the `SkillBuff`s on it newest first: buffs others cast on it, then aura buffs; bitmap paths. PORT: the two
/// buff lists are kept apart, so their relative order is by kind, not by age.
pub fn pet_icons<'a>(
    db: &'a Database,
    damage: Option<&gd_game_core::casting::DamageAuras>,
    pet: Option<&crate::pets::Pet>,
    received: Option<&ReceivedBuffs>,
) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut add = |rec: &'a Record| {
        if let Some(b) = rec.str("skillUpBitmapName").filter(|b| !b.is_empty()) {
            out.push(b);
        }
    };
    for (s, _) in pet.map(|p| p.toggled()).unwrap_or_default() {
        if let Some(rec) = db.get(s).filter(|r| self_trackable(&r.class)) {
            add(rec);
        }
    }
    for a in damage.map(|d| d.0.as_slice()).unwrap_or_default() {
        if let Some(rec) = db.get(a.skill()).filter(|r| self_trackable(&r.class)) {
            add(rec);
        }
    }
    let timed = pet.map(|p| p.buffs()).unwrap_or_default();
    for (s, _, _) in timed {
        if let Some(rec) = db.get(s).filter(|r| self_trackable(&r.class)) {
            add(rec);
        }
    }
    let shown = |r: &Record| {
        r.class.starts_with("SkillBuff")
            && !r.bool("hideFromUI")
            && !gd_game_core::skill_buffs::is_debuff(r)
    };
    for (s, _, _) in timed.iter().rev() {
        if let Some(rec) = db.get(s).filter(|r| shown(r)) {
            add(rec);
        }
    }
    for b in received.map(|r| r.list.as_slice()).unwrap_or_default() {
        if let Some(rec) = db
            .get(&b.buff)
            .filter(|r| !r.bool("hideFromUI") && !gd_game_core::skill_buffs::is_debuff(r))
        {
            add(rec);
        }
    }
    out
}
