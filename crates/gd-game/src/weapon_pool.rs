//! Weapon-pool attacks and skill-modifier links of the player
//! (`gd_sim::weapon_pool`, `gd_sim::actor::SkillUse`).
//!
//! [`SkillLinks`] keeps the learned modifiers per skill (their damage adds
//! to every hit of the skill, `combat::apply_hits`) and the usable pool
//! skills (`Skill_WPAttack*` with a chance weight, weapon requirements met).
//! Each weapon-pool attack — a plain swing or shot (the default attack) or a
//! `Skill_WeaponPool_*` cast (Fire Strike, Cadence, Righteous Fervor) — rolls
//! [`gd_sim::weapon_pool::select`]; a picked pool skill is fired instead as
//! a [`crate::casting::ProcCast`] with the pool attack as its parent.
//! The roll happens as the action starts (`SkillActivatedWeaponPool::
//! StartAction` @0x18051bb90: it asks the skill services for a pool skill
//! (`SkillManager::SelectWeaponPoolSkill` @0x180536b60), keeps its id and
//! starts that skill's own `StartAction`): [`start_swing`] for a plain
//! swing (`combat::player_attack`, `casting`'s Shift attack) and
//! [`pool_cast_starts`] for a pool-skill cast store the outcome in
//! [`PoolPick`] until the action's hit uses it. The picked skill's
//! `skillSpecialAnimationName` plays as the swing's clip
//! ([`pool_swing_specials`], `Character::swing_special`), and the hit point
//! is that clip's `RightHandHit`.
//! Shapes per class: [`pool_kind`].
//!
//! Monsters run the same code: `SelectWeaponPoolSkill` @0x180536b60 walks
//! whatever skills the manager holds, so a monster's
//! `Skill_WPAttack*` skills form its pool and its `Skill_WeaponPool_Charged*`
//! attack (the giants' Cleaving Strikes, a Righteous-Fervor-like scaling
//! skill) keeps [`PoolCharges`] (`monster_skills`). PORT: the monsters'
//! pool skills' weapon requirements are not checked.
//!
//! Charged pool skills ([`PoolCharges`], from the classes'
//! `StartAction` / `HitAction` / `WPPostAttackCallback` / `Update` /
//! `IncrementCharge` / `DecrementCharge`):
//! - `Skill_WeaponPool_ChargedFinale` (Cadence): an attack with targets gains
//!   a charge (up to `skillChargeLevel`) unless locked out; the lockout is
//!   `skillChargeTime` (`…Ranged` with a ranged weapon) / attack speed, the
//!   charges expire after `skillChargeDuration`. With full charges (and the
//!   lockout over) the next attack is the finale, decided as it starts
//!   (`Skill_WeaponPool_ChargedFinale::StartAction` @0x18050dd50:
//!   else the plain pool `StartAction` and its roll): the skill's own hit
//!   (no pool roll); a landed
//!   finale (or any with `spendChargesOnMiss`) clears the charges. Other
//!   attacks are plain pool attacks without the skill's damage (its
//!   constructor clears the parent flag `Skill` +263, which the
//!   `Skill_WPAttack::CollectLocal*` collectors require of a parent).
//! - `Skill_WeaponPool_ChargedScaling` (Righteous Fervor): an attack with
//!   targets gains a charge (up to `skillChargeLevel`) before it hits; each
//!   charge lasts `skillChargeDuration`, then one is lost; the skill's own
//!   damage and weapon % scale by `skillChargeMultipliers[charges-1]` %
//!   (none without charges). Lockout = `skillChargeTime` / attack speed
//!   (`IncrementCharge` sets it from the profile's
//!   `skillChargeTime`, never the ranged one, divided by the attack speed;
//!   `WPPreAttackCallback` gains a charge only with targets and the lockout
//!   over).
use crate::casting::{Casting, Kind};
use crate::character::{CharState, Character};
use crate::data::GameData;
use crate::player::{Loadout, Player};
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_sim::combat::Rng;
use gd_sim::equip::WeaponType;
use std::collections::HashMap;

pub struct WeaponPoolPlugin;

impl Plugin for WeaponPoolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PoolRng>().add_systems(
            Update,
            (
                rebuild_links.run_if(resource_exists::<crate::inventory::Inventory>),
                toggles_need_weapons.after(rebuild_links),
                tick_charges,
                charged_profiles.after(tick_charges),
                pool_cast_starts.before(crate::casting::CastingSet),
                // The swing's clip is picked before the drive plays it.
                pool_swing_specials
                    .in_set(crate::character::CharacterStage::Drive)
                    .before(crate::character::drive_characters),
            ),
        );
    }
}

/// A toggled skill switches off when the worn weapons stop suiting it
/// (`Skill_BuffSelfToggled::Update`: not `QualifyingWeapon` /
/// `QualifyingHandState` → deactivate); the auto toggle switches
/// it on again once they do (`crate::skill_buffs` docs, auto toggles).
fn toggles_need_weapons(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<(Entity, &SkillLinks, &mut Loadout), Changed<SkillLinks>>,
) {
    for (e, links, mut loadout) in &mut q {
        let off: Vec<String> = loadout
            .skills
            .skills
            .iter()
            .filter(|(s, l)| *l > 0 && !loadout.skills.switched_off(&s.to_ascii_lowercase()))
            .filter_map(|(s, _)| {
                data.db
                    .get(s)
                    .filter(|r| gd_sim::skills::is_toggle(&r.class))
                    .map(|r| (s, r))
            })
            .filter(|(s, r)| !links.weapons_allow(&data.db, r, s))
            .map(|(s, _)| s.to_ascii_lowercase())
            .collect();
        if off.is_empty() {
            continue;
        }
        for s in off {
            info!(
                "{} switched off: needs other weapons",
                s.rsplit('/').next().unwrap_or("")
            );
            loadout.skills.inactive.insert(s);
        }
        commands.entity(e).try_insert(crate::player::ProfileDirty);
    }
}

#[derive(Resource)]
pub struct PoolRng(pub Rng);

impl Default for PoolRng {
    fn default() -> Self {
        Self(Rng(crate::rng::u32(1..0x7FFF_FFFF)))
    }
}

/// Learned modifiers and usable pool skills of a character.
#[derive(Component, Default, Debug, Clone)]
pub struct SkillLinks {
    /// Lower-case skill → its learned modifiers (record, level index): the
    /// `Skill_Modifier` family (transmuters, projectile modifiers…), whose
    /// fields add to the skill (`Skill` +320 list, class check).
    pub mods: HashMap<String, Vec<(String, usize)>>,
    /// Lower-case skill → its learned `SkillSecondary_*` skills (`Skill`
    /// +368 list, `Skill::GetSecondarySkills`): separate effects, not damage
    /// (`secondary.rs`); the sheet's skill rollovers add their damage.
    pub secondaries: HashMap<String, Vec<(String, usize)>>,
    /// Lower-case skill → the gear-granted secondaries (`Skill` +392,
    /// `AddToItemSecondarySkills`): they act like the learned ones
    /// (`ActivateSecondarySkills` runs both lists) but the
    /// rollovers leave them out (`gd_sim::skills` docs).
    pub item_secondaries: HashMap<String, Vec<(String, usize)>>,
    /// Lower-case skill → its gear modifiers that are off (`Skill_Modifier::IsSkillEnabled`:
    /// exclusive to a learned transmuter, [`build_links`]): not in [`Self::mods`], so they add
    /// nothing and have no target effect; only the warm-up starts their paks (`casting::warmup`).
    pub off_mods: HashMap<String, Vec<(String, usize)>>,
    /// Usable pool skills: (record, level index, chance weight).
    pub pool: Vec<(String, usize, u32)>,
    /// The worn weapons' types (right, left hand).
    pub hands: (Option<WeaponType>, Option<WeaponType>),
    /// Lower-case skill → the modifier replacing its weapon requirements
    /// (`gd_sim::weapon_pool::weapon_list_override`).
    pub weapon_override: HashMap<String, String>,
}

impl SkillLinks {
    /// A monster's (or any record skill list's) links, as
    /// `SkillManager::LoadSkills` @0x18052ef00 builds them by calling
    /// `CreateAndLoadSkill` @0x18052f300 for each `skillNameN` in order:
    /// a `SkillSecondary_*` joins the last ordinary skill's +368
    /// list ([`Self::secondaries`]); a `Skill_Modifier` (and subclasses)
    /// joins the +320 list ([`Self::mods`]) of its `subSkillName`
    /// (`Skill_Modifier::Load` @0x1804faaf0, +1952) when that names a skill
    /// already created (`FindSkillId`: an earlier entry), else of the last
    /// ordinary skill (`gd_sim::skills::tree_bases`); a `Skill_Mastery`
    /// clears the last skill. Each is keyed by that skill and its
    /// `buffSkillName` (wrappers cast as it). Stock data: the giants' Cleaving
    /// Strikes modifier (`giant_cleavingstrikesmod`, a 45 deg / 5 target cone).
    pub fn of_list(db: &gd_formats::vfs::Database, skills: &[(String, u32)]) -> Self {
        let names: Vec<String> = skills.iter().map(|s| s.0.clone()).collect();
        let mut links = Self::default();
        for (i, ((sk, level), base)) in skills
            .iter()
            .zip(gd_sim::skills::tree_bases(db, &names))
            .enumerate()
        {
            let (Some(base), Some(r)) = (base, db.get(sk)) else {
                continue;
            };
            // `FindSkillId` only finds skills created before this one.
            if !names[..i].iter().any(|n| n.eq_ignore_ascii_case(&base)) {
                continue;
            }
            let secondary = gd_sim::skills::is_secondary_class(&r.class);
            let mut keys = vec![base.to_ascii_lowercase()];
            if let Some(b) = db
                .get(&base)
                .and_then(|br| gd_sim::skills::profile_name(br).map(str::to_ascii_lowercase))
            {
                keys.push(b);
            }
            let map = if secondary {
                &mut links.secondaries
            } else {
                &mut links.mods
            };
            for k in keys {
                map.entry(k)
                    .or_default()
                    .push((sk.clone(), (*level).max(1) as usize - 1));
            }
        }
        links
    }

    /// Whether the worn weapons allow `skill` (`Skill::SetAvailability`:
    /// `QualifyingWeapon` and `QualifyingHandState`).
    pub fn weapons_allow(&self, db: &gd_formats::vfs::Database, rec: &Record, skill: &str) -> bool {
        let over = self
            .weapon_override
            .get(&skill.to_ascii_lowercase())
            .and_then(|m| db.get(m));
        gd_sim::weapon_pool::qualifies(rec, over, self.hands.0, self.hands.1)
    }

    pub fn mods_of(&self, skill: &str) -> &[(String, usize)] {
        self.mods
            .get(&skill.to_ascii_lowercase())
            .map_or(&[], |v| v.as_slice())
    }

    /// [`Self::off_mods`] of `skill`.
    pub fn off_mods_of(&self, skill: &str) -> &[(String, usize)] {
        self.off_mods
            .get(&skill.to_ascii_lowercase())
            .map_or(&[], |v| v.as_slice())
    }

    /// Every secondary of `skill` (lower-case key): the learned ones, then
    /// the gear's (`Skill::ActivateSecondarySkills` order).
    pub fn all_secondaries_of(&self, skill: &str) -> impl Iterator<Item = &(String, usize)> {
        self.secondaries
            .get(skill)
            .into_iter()
            .flatten()
            .chain(self.item_secondaries.get(skill).into_iter().flatten())
    }

    /// The skills (lower-case keys) with any secondary.
    pub fn secondary_parents(&self) -> impl Iterator<Item = &String> {
        self.secondaries.keys().chain(
            self.item_secondaries
                .keys()
                .filter(|k| !self.secondaries.contains_key(*k)),
        )
    }

    /// Rolls the pool for one weapon-pool attack: the pool skill to fire
    /// instead, if any.
    pub fn roll(&self, rng: &mut Rng) -> Option<(String, usize)> {
        let weights: Vec<u32> = self.pool.iter().map(|p| p.2).collect();
        let i = gd_sim::weapon_pool::select(&weights, rng)?;
        Some((self.pool[i].0.clone(), self.pool[i].1))
    }
}

/// The cast replacing a weapon-pool attack with the picked pool skill
/// (`parent`: the pool skill cast, `None` for the default attack).
pub fn pick_cast(
    data: &GameData,
    (skill, level_index): (String, usize),
    parent: Option<(String, usize)>,
    point: Vec3,
    target: Option<Entity>,
    hostile: bool,
) -> Option<Casting> {
    let kind = pool_kind(data.db.get(&skill)?);
    debug!(
        "weapon pool: {} (parent {:?})",
        skill.rsplit('/').next().unwrap_or(""),
        parent
            .as_ref()
            .map(|p| p.0.rsplit('/').next().unwrap_or(""))
    );
    Some(Casting {
        skill,
        level_index,
        kind,
        point,
        target,
        hostile,
        parent,
    })
}

/// A weapon-pool action's outcome, decided as it starts
/// (`SkillActivatedWeaponPool::StartAction` @0x18051bb90 rolls
/// `SkillManager::SelectWeaponPoolSkill`; `Skill_WeaponPool_ChargedFinale::
/// StartAction` @0x18050dd50 picks the finale instead when charged) and
/// used by the action's hits.
#[derive(Component, Clone, Debug, Default)]
pub struct PoolPick {
    /// The action's skill: [`crate::casting::DEFAULT_ATTACK`] for a plain
    /// swing, else the pool skill cast.
    pub skill: String,
    /// The pool skill picked to fire instead (record, level index).
    pub pick: Option<(String, usize)>,
    /// Cadence's finale (no roll).
    pub finale: bool,
}

impl PoolPick {
    pub fn is_for(&self, skill: &str) -> bool {
        self.skill.eq_ignore_ascii_case(skill)
    }
}

/// A plain weapon swing (or shot) starts: roll the pool for it.
pub fn start_swing(
    commands: &mut Commands,
    entity: Entity,
    links: Option<&SkillLinks>,
    rng: &mut Rng,
) {
    let pick = links.and_then(|l| l.roll(rng));
    commands.entity(entity).try_insert(PoolPick {
        skill: crate::casting::DEFAULT_ATTACK.into(),
        pick,
        finale: false,
    });
}

/// A weapon-pool proc's swing plays the picked skill's special animation
/// (`SkillActivatedWeaponPool::StartAction` @0x18051bb90: it starts
/// the picked skill's own `StartAction`, which plays its
/// `skillSpecialAnimationName`): [`Character::swing_special`] names the
/// reference, `character::drive_characters` plays the `Special` slot at the
/// attack speed, and the hit point is that clip's `RightHandHit`
/// (`combat::apply_hits` / `casting::ranged_swings` fire the `ProcCast`
/// there). Runs as the swing starts (the roll's [`PoolPick`] was just
/// inserted), before the drive plays its clip; a pick without a special name
/// keeps the plain clip.
fn pool_swing_specials(
    data: Res<GameData>,
    mut q: Query<(&PoolPick, &mut Character), Changed<PoolPick>>,
) {
    for (p, mut ch) in &mut q {
        // Pool-skill casts (Fire Strike itself…) animate through their Cast
        // state; only a plain swing's roll replaces the attack clip.
        if ch.state != CharState::Attack {
            continue;
        }
        ch.swing_special = p.pick.as_ref().and_then(|(skill, _)| {
            data.db
                .get(skill)
                .and_then(|r| r.str("skillSpecialAnimationName").map(str::to_owned))
        });
    }
}

/// A pool skill's action starts: the finale when Cadence is charged, else
/// a roll.
pub fn start_pool_action(
    links: &SkillLinks,
    charges: &PoolCharges,
    rec: &Record,
    skill: &str,
    level_index: usize,
    rng: &mut Rng,
) -> PoolPick {
    let finale = charges.finale_ready(rec, skill, level_index);
    PoolPick {
        skill: skill.to_owned(),
        pick: if finale { None } else { links.roll(rng) },
        finale,
    }
}

/// Pool-skill casts (Fire Strike, Cadence, Righteous Fervor) starting their
/// animation decide their pool outcome now ([`start_pool_action`]).
fn pool_cast_starts(
    mut commands: Commands,
    data: Res<GameData>,
    mut rng: ResMut<PoolRng>,
    q: Query<(Entity, &Casting, &SkillLinks, &PoolCharges), Added<Casting>>,
) {
    for (e, cast, links, charges) in &q {
        if cast.kind != Kind::Weapon || cast.parent.is_some() {
            continue;
        }
        let Some(rec) = data.db.get(&cast.skill).filter(|r| is_pool_parent(r)) else {
            continue;
        };
        commands.entity(e).try_insert(start_pool_action(
            links,
            charges,
            rec,
            &cast.skill,
            cast.level_index,
            &mut rng.0,
        ));
    }
}

/// How a pool skill hits, from each class's own virtuals over
/// `Skill_WPAttack_BasicAttack`: `…_Radius` overrides `ApplyMeleeDamage` /
/// `ApplyRangedDamage` to hit the foes around the target
/// (`skillTargetRadius`), `…_AttackWave` has its own `ActivateNow` /
/// `RunTargeting` / `Update` (a wave, `waveDistance`), `…_ProjectileBurst`
/// its own `ExecuteRangedAttack` (`projectileLaunchNumber` projectiles);
/// `…_BasicAttack` and `…_RadialCrit` keep the weapon hit's targeting
/// (`RadialCrit` adds its own `OnHitTarget` / damage collectors), here a
/// weapon hit on the target plus `skillTargetNumber` foes in
/// `skillTargetAngle` (`casting::fire_casts`).
pub fn pool_kind(rec: &Record) -> Kind {
    match rec.class.as_str() {
        "Skill_WPAttack_Radius" => Kind::TargetRadius,
        "Skill_WPAttack_AttackWave" => Kind::Wave,
        "Skill_WPAttack_ProjectileBurst" => Kind::Projectile,
        _ => Kind::Weapon,
    }
}

/// Is this a weapon-pool skill whose attacks roll the pool?
pub fn is_pool_parent(rec: &Record) -> bool {
    rec.class.starts_with("Skill_WeaponPool")
}

/// Charge state of the charged pool skills (lower-case skill → charge).
#[derive(Component, Default, Debug)]
pub struct PoolCharges(pub HashMap<String, Charge>);

#[derive(Default, Debug, Clone, Copy)]
pub struct Charge {
    pub count: u32,
    /// Seconds before another charge can be gained.
    pub lockout: f32,
    /// Seconds before the charges expire (Cadence) / one is lost (Fervor).
    pub decay: f32,
}

/// What a weapon-pool skill's attack does ([`PoolCharges::step`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PoolStep {
    /// Cadence's finale: the skill's own hit, no pool roll.
    Finale,
    /// A pool attack; `parent_damage`: the skill's damage applies.
    Attack { parent_damage: bool },
}

fn lv(rec: &Record, k: &str, i: usize) -> f32 {
    gd_sim::stats::level_value(rec, k, i).unwrap_or(0.0)
}

impl PoolCharges {
    /// Whether Cadence's next attack is the finale: full charges and the
    /// lockout over (`Skill_WeaponPool_ChargedFinale::StartAction`).
    pub fn finale_ready(&self, rec: &Record, skill: &str, li: usize) -> bool {
        let level = lv(rec, "skillChargeLevel", li).max(1.0) as u32;
        rec.class == "Skill_WeaponPool_ChargedFinale"
            && self
                .0
                .get(&skill.to_ascii_lowercase())
                .is_some_and(|c| c.count >= level && c.lockout <= 0.0)
    }

    /// The hit of one attack of the pool skill `rec` (`has_target`:
    /// something to hit; `finale`: the attack started as Cadence's finale,
    /// [`Self::finale_ready`] at its start).
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        rec: &Record,
        skill: &str,
        li: usize,
        attack_speed: f32,
        has_target: bool,
        ranged: bool,
        finale_start: bool,
    ) -> PoolStep {
        let finale = rec.class == "Skill_WeaponPool_ChargedFinale";
        if !finale && rec.class != "Skill_WeaponPool_ChargedScaling" {
            return PoolStep::Attack {
                parent_damage: true,
            };
        }
        let level = lv(rec, "skillChargeLevel", li).max(1.0) as u32;
        let c = self.0.entry(skill.to_ascii_lowercase()).or_default();
        if finale && finale_start {
            if rec.bool("spendChargesOnMiss") {
                *c = Charge::default();
            }
            return PoolStep::Finale;
        }
        if has_target && c.lockout <= 0.0 && (c.count < level || !finale) {
            c.count = (c.count + 1).min(level);
            let t = if finale && ranged {
                lv(rec, "skillChargeTimeRanged", li)
            } else {
                lv(rec, "skillChargeTime", li)
            };
            c.lockout = t / attack_speed.max(0.1);
            c.decay = lv(rec, "skillChargeDuration", c.count as usize - 1);
        }
        PoolStep::Attack {
            parent_damage: !finale,
        }
    }

    /// Fervor's damage scale for `skill` (1 for other skills).
    pub fn scale(&self, rec: &Record, skill: &str) -> f32 {
        if rec.class != "Skill_WeaponPool_ChargedScaling" {
            return 1.0;
        }
        let n = self
            .0
            .get(&skill.to_ascii_lowercase())
            .map_or(0, |c| c.count);
        if n == 0 {
            return 0.0;
        }
        rec.f32s_iter("skillChargeMultipliers")
            .nth(n as usize - 1)
            .or_else(|| rec.f32s_iter("skillChargeMultipliers").last())
            .map(|m| m * 0.01)
            .unwrap_or(1.0)
    }
}

/// A charged `Skill_WeaponPool_ChargedScaling` adds its passive part to the player
/// (`gd_sim::skills::charged_scaling_passive`): the profile is rebuilt when the set of charged ones changes.
fn charged_profiles(
    mut commands: Commands,
    data: Res<GameData>,
    q: Query<(Entity, &PoolCharges, &crate::combat::Combatant), With<Player>>,
) {
    for (e, charges, cb) in &q {
        let mut now: Vec<&str> = charges
            .0
            .iter()
            .filter(|(s, c)| {
                c.count > 0
                    && data
                        .db
                        .get(s)
                        .is_some_and(|r| r.class == "Skill_WeaponPool_ChargedScaling")
            })
            .map(|(s, _)| s.as_str())
            .collect();
        now.sort_unstable();
        let mut built: Vec<String> = cb
            .profile
            .charged
            .iter()
            .map(|c| c.0.to_ascii_lowercase())
            .collect();
        built.sort_unstable();
        if now != built {
            commands.entity(e).try_insert(crate::player::ProfileDirty);
        }
    }
}

fn tick_charges(
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<&mut PoolCharges>,
    mut landed: MessageReader<crate::combat::HitLanded>,
) {
    let dt = time.delta_secs();
    for h in landed.read() {
        if let (Ok(mut pc), Some(s)) = (q.get_mut(h.attacker), &h.skill)
            && data
                .db
                .get(s)
                .is_some_and(|r| r.class == "Skill_WeaponPool_ChargedFinale")
            && let Some(c) = pc.0.get_mut(&s.to_ascii_lowercase())
        {
            *c = Charge::default();
        }
    }
    for mut pc in &mut q {
        for (skill, c) in pc.0.iter_mut() {
            c.lockout -= dt;
            if c.count == 0 {
                continue;
            }
            c.decay -= dt;
            if c.decay > 0.0 {
                continue;
            }
            let Some(rec) = data.db.get(skill) else {
                continue;
            };
            if rec.class == "Skill_WeaponPool_ChargedScaling" {
                c.count -= 1;
                c.decay = if c.count > 0 {
                    lv(rec, "skillChargeDuration", c.count as usize - 1)
                } else {
                    0.0
                };
            } else {
                c.count = 0;
            }
        }
    }
}

/// The [`SkillLinks`] of a loadout (its learned skills at their effective levels) with the skill modifiers
/// of the worn items `gear` and the weapon types `hands` (right, left): what [`rebuild_links`] keeps on the player and
/// what the item tooltip's DPS compare builds for the gear it tries (`Player::CompareItems` equips the
/// candidate, so the candidate's skill modifiers and bonuses count and the replaced item's do not).
pub fn build_links(
    data: &GameData,
    trees: &gd_sim::skills::SkillTrees,
    loadout: &Loadout,
    gear: &[gd_formats::save::item::Item],
    hands: (Option<WeaponType>, Option<WeaponType>),
) -> SkillLinks {
    let book = loadout.effective_skills(&data.db, trees);
    let (right, left) = hands;
    let mut links = SkillLinks::default();
    // Pool skills: (record, level index, learned modifiers), qualified once
    // the gear modifiers are known.
    let mut pool_candidates: Vec<(String, usize, Vec<(String, usize)>)> = Vec::new();
    links.hands = (right, left);
    // Lower-case skill → its learned modifiers (to tell them from gear ones).
    let mut learned_of: HashMap<String, Vec<String>> = HashMap::new();
    for (s, l) in &book.skills {
        if *l == 0
            || gd_sim::skills::is_modifier_class(
                &data.db.get(s).map(|r| r.class.clone()).unwrap_or_default(),
            )
        {
            continue;
        }
        let mods = book.modifiers_of(&data.db, trees, s);
        let Some(rec) = data.db.get(s) else { continue };
        let li = book.effective_level(&data.db, s).max(1) as usize - 1;
        if rec.class.starts_with("Skill_WPAttack") {
            pool_candidates.push((s.clone(), li, mods.clone()));
        }
        learned_of.insert(
            s.to_ascii_lowercase(),
            mods.iter().map(|(m, _)| m.to_ascii_lowercase()).collect(),
        );
        let (sec, mods): (Vec<_>, Vec<_>) = mods.into_iter().partition(|(m, _)| {
            data.db
                .get(m)
                .is_some_and(|r| gd_sim::skills::is_secondary_class(&r.class))
        });
        // Wrapper skills cast (and hit) as their `buffSkillName` record:
        // key the links under both names.
        let mut keys = vec![s.to_ascii_lowercase()];
        if let Some(b) = gd_sim::skills::profile_name(rec) {
            keys.push(b.to_ascii_lowercase());
        }
        for k in keys {
            if !mods.is_empty() {
                links.mods.insert(k.clone(), mods.clone());
            }
            if !sec.is_empty() {
                links.secondaries.insert(k, sec.clone());
            }
        }
    }
    // Gear skill modifiers (`modifiedSkillName{n}` → `modifierSkillName{n}`
    // on an item's records: legendary / ascended "skill modifiers") join the
    // learned ones at level index 0 (`ItemEquipment::AttachItem`
    // and `ItemRelic` / `ItemEnchantment::InstallOnCharacter` hand every
    // equipped item's modifiers to `SkillManager::AddItemSkillModifiers`,
    // which sets each modifier skill to level 1). The item (its ascendant affix with it), its
    // component (relic and completion bonus) and its augment are three such calls.
    // `AddItemSkillModifiers` / `Skill_Modifier::SetExclusiveItemSkillModifier`
    // / `IsSkillEnabled` @0x1804faec0: a modifier of skill S whose call also
    // modifies a `Skill_Transmuter` of S that S's learned modifiers hold is made
    // exclusive to it, and is off while that transmuter is enabled (learned: the
    // links are rebuilt as skills change). An off modifier adds nothing
    // (`Skill_Modifier::GetSkillModifierData` @0x1804fba40 skips it for its
    // parent) and has no target effect (`Skill::GetTargetFX`), so it is kept
    // apart ([`SkillLinks::off_mods`]): only `Skill::CreateWarmUpFx` runs it
    // (no enabled check there, `casting::warmup`).
    for it in gear {
        for names in [
            &[
                &it.base_name,
                &it.prefix_name,
                &it.suffix_name,
                &it.modifier_name,
                &it.ascendant_name,
            ][..],
            &[&it.relic_name, &it.relic_bonus][..],
            &[&it.augment_name][..],
        ] {
            let pairs: Vec<(String, String)> = names
                .iter()
                .filter(|n| !n.is_empty())
                .filter_map(|n| data.db.get(n.as_str()))
                .flat_map(|r| {
                    (1..=8)
                        .filter_map(|n| {
                            let target = r.str(&format!("modifiedSkillName{n}"))?;
                            let m = r.str(&format!("modifierSkillName{n}"))?;
                            (!target.is_empty() && !m.is_empty())
                                .then(|| (target.to_owned(), m.to_owned()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            for (target, m) in &pairs {
                let secondary = data
                    .db
                    .get(m)
                    .is_some_and(|mr| gd_sim::skills::is_secondary_class(&mr.class));
                let learned = learned_of.get(&target.to_ascii_lowercase());
                let off = !secondary
                    && pairs.iter().any(|(t, _)| {
                        !t.eq_ignore_ascii_case(target)
                            && learned.is_some_and(|l| l.contains(&t.to_ascii_lowercase()))
                            && data
                                .db
                                .get(t)
                                .is_some_and(|tr| tr.class == "Skill_Transmuter")
                    });
                let mut keys = vec![target.to_ascii_lowercase()];
                if let Some(b) = data
                    .db
                    .get(target)
                    .and_then(|tr| gd_sim::skills::profile_name(tr).map(str::to_ascii_lowercase))
                {
                    keys.push(b);
                }
                for k in keys {
                    let map = if secondary {
                        &mut links.item_secondaries
                    } else if off {
                        &mut links.off_mods
                    } else {
                        &mut links.mods
                    };
                    map.entry(k).or_default().push((m.clone(), 0));
                }
            }
        }
    }
    // Weapon requirement overrides (`overwriteBaseSkill`): first learned
    // modifier, else the last gear one.
    for (k, list) in &links.mods {
        let learned = learned_of.get(k).cloned().unwrap_or_default();
        let recs: Vec<(Record, bool)> = list
            .iter()
            .filter_map(|(m, _)| {
                data.db
                    .get_owned(m)
                    .map(|r| (r, learned.contains(&m.to_ascii_lowercase())))
            })
            .collect();
        let l: Vec<&Record> = recs.iter().filter(|(_, x)| *x).map(|(r, _)| r).collect();
        let g: Vec<&Record> = recs.iter().filter(|(_, x)| !*x).map(|(r, _)| r).collect();
        if let Some(o) = gd_sim::weapon_pool::weapon_list_override(&l, &g) {
            links.weapon_override.insert(k.clone(), o.name.clone());
        }
    }
    for (s, li, mods) in pool_candidates {
        let Some(rec) = data.db.get(&s) else { continue };
        let mrecs: Vec<(Record, usize)> = mods
            .iter()
            .filter_map(|(m, ml)| data.db.get_owned(m).map(|r| (r, *ml)))
            .collect();
        let gear: Vec<Record> = links
            .mods_of(&s)
            .iter()
            .filter(|(m, _)| !mods.iter().any(|(l, _)| l.eq_ignore_ascii_case(m)))
            .filter_map(|(m, _)| data.db.get_owned(m))
            .collect();
        let learned: Vec<&Record> = mrecs.iter().map(|(r, _)| r).collect();
        let over =
            gd_sim::weapon_pool::weapon_list_override(&learned, &gear.iter().collect::<Vec<_>>());
        if !gd_sim::weapon_pool::qualifies(rec, over, right, left) {
            continue;
        }
        let mrefs: Vec<(&Record, usize)> = mrecs.iter().map(|(r, l)| (r, *l)).collect();
        let w = gd_sim::weapon_pool::chance_weight(rec, li, &mrefs);
        if w > 0 {
            links.pool.push((s, li, w));
        }
    }
    links
}

fn rebuild_links(
    mut fresh: crate::session::NewSession,
    mut commands: Commands,
    data: Res<GameData>,
    inv: Res<crate::inventory::Inventory>,
    players: Query<(Entity, Ref<Loadout>), With<Player>>,
    mut sig: Local<u64>,
) {
    if fresh.begun() {
        *sig = Default::default();
    }
    use std::hash::{Hash, Hasher};
    let Ok((e, loadout)) = players.single() else {
        return;
    };
    // Only actual mutations wake this: the UI reads `Inventory::info` through
    // `&self` (its interior cache), so UI reads no longer tick the resource
    // every frame; equips and loadout changes still do. The signature hash
    // below is the dedup.
    if !loadout.is_changed() && !inv.is_changed() {
        return;
    }
    // Only skills and gear matter (the loadout also changes with buffs).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        e,
        &loadout.skills.skills,
        loadout
            .items
            .iter()
            .map(|i| {
                (
                    &i.base_name,
                    &i.prefix_name,
                    &i.suffix_name,
                    &i.relic_name,
                    &i.augment_name,
                )
            })
            .collect::<Vec<_>>(),
    )
        .hash(&mut h);
    // Detached (requirement-failing) gear grants nothing: hash, gear and
    // weapon types all come from attached slots only.
    let attached = |i: usize| {
        inv.is_attached(i)
            .then(|| inv.equipped[i].as_ref())
            .flatten()
    };
    (0..inv.equipped.len())
        .map(|i| {
            attached(i).map(|i| {
                (
                    &i.base_name,
                    &i.prefix_name,
                    &i.suffix_name,
                    &i.modifier_name,
                    &i.relic_name,
                    &i.augment_name,
                )
            })
        })
        .collect::<Vec<_>>()
        .hash(&mut h);
    if std::mem::replace(&mut *sig, h.finish()) == *sig {
        return;
    }
    let trees = data.skill_trees();
    let wtype = |i: usize| {
        attached(i)
            .and_then(|it| data.db.get(&it.base_name))
            .and_then(|r| WeaponType::from_class(&r.class))
    };
    let gear = inv.worn();
    let links = build_links(&data, &trees, &loadout, &gear, (wtype(12), wtype(13)));
    debug!(
        "skill links: pool {:?}, {} modified skills",
        links
            .pool
            .iter()
            .map(|p| (p.0.rsplit('/').next().unwrap_or(""), p.2))
            .collect::<Vec<_>>(),
        links.mods.len()
    );
    commands.entity(e).try_insert(links);
    commands.entity(e).try_insert_if_new(PoolCharges::default());
}
