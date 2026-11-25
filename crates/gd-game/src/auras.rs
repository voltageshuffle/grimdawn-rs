//! Radius buffs: the buffs a character's auras (`Skill_BuffRadiusToggled`) and
//! timed radius buffs (`Skill_BuffRadius`) put on the characters around it.
//!
//! Rules (`Skill_BuffRadius::Update` / `ActivateNow` / `TargetResult`):
//! - Every 1 s while on (the first pass at activation) the aura takes the targets in
//!   `skillTargetRadius`: friends, or foes when the buff record sets `debufSkill`. "In radius" means
//!   the sphere meets the target's bounding box (`Combatant::in_radius`). Hiding characters (totems,
//!   runes, traps) are skipped. A non-debuff aura also buffs its caster.
//! - Each target gets the `buffSkillName` record at the aura's level with the caster's modifiers for
//!   1.5 s ([`BUFF_SECS`]). It is refreshed while in range.
//! - The activation's pass (and a toggle's switch-on pass) of a non-debuff skill gives each target
//!   the skill's bonus first ([`crate::casting::Bonus`]). A skill without a duration has only that pass.
//! - A new buff goes to the front of [`ReceivedBuffs::list`](gd_game_core::skill_buffs::ReceivedBuffs). A new
//!   buff on another character starts the record's `charFxPakOtherNames` there.
//!
//! Emitters: the player (switched-on toggles with `SkillLinks` modifiers, running timed buffs), pets
//! (`Pet::emissions`) and monsters (`MonsterSkills::emissions`).
//!
//! PORT: the player is never its own target here; its own auras and timed buffs count through
//! `SkillBook::stat_sources` / `Loadout::buffs`, and [`Activated`] gives it the bonus alone.
use crate::casting::Bonus;
use crate::combat::Side;
use crate::monster_skills::MonsterSkills;
use crate::pets::Pet;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;
use gd_game_core::casting::level_f;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player};
use gd_game_core::skill_buffs::{ReceivedBuff, ReceivedBuffs};
use std::sync::Arc;

/// `Skill_BuffRadius.pulseInterval`.
pub const PULSE_SECS: f32 = 1.0;
/// `Skill_BuffRadius.buffDuration`.
pub const BUFF_SECS: f32 = 1.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Emission {
    pub skill: Arc<str>,
    pub level_index: usize,
    pub buff: Arc<str>,
    pub radius: f32,
    pub mods: Arc<[(String, usize)]>,
    /// Not for the player, never for a debuff.
    pub on_self: bool,
    /// `debufSkill`: foes are its targets.
    pub debuf: bool,
    /// Carried by its buffs (`ReceivedBuff::devotion`; a celestial power's, else 0).
    pub devotion: u32,
}

/// `debufSkill` of the skill's buff record or the skill record.
pub fn is_debuf(db: &Database, rec: &Record) -> bool {
    rec.bool("debufSkill")
        || gd_sim::skills::profile_name(rec)
            .and_then(|b| db.get(b))
            .is_some_and(|b| b.bool("debufSkill"))
}

/// None without a buff record or a radius.
pub fn emission(
    db: &Database,
    skill: &str,
    li: usize,
    mods: Vec<(String, usize)>,
    on_self: bool,
) -> Option<Emission> {
    let rec = db.get(skill)?;
    let buff: Arc<str> = Arc::from(gd_sim::skills::profile_name(rec)?);
    let brec = db.get(&buff)?;
    let debuf = brec.bool("debufSkill") || rec.bool("debufSkill");
    let radius = level_f(brec, "skillTargetRadius", li).max(level_f(rec, "skillTargetRadius", li));
    (radius > 0.0).then_some(Emission {
        skill: Arc::from(skill),
        level_index: li,
        buff,
        radius,
        mods: mods.into(),
        on_self: on_self && !debuf,
        debuf,
        devotion: 0,
    })
}

/// Switched-on radius toggles at their effective level and running `Skill_BuffRadius` casts (under the
/// wrapper's `buffSkillName`), each with its devotion level.
pub fn player_emissions(
    db: &Database,
    loadout: &Loadout,
    effective: &gd_sim::skills::SkillBook,
    links: Option<&gd_game_core::weapon_pool::SkillLinks>,
    devotion: Option<&crate::devotion::DevotionState>,
) -> Vec<Emission> {
    let mods = |s: &str| links.map_or_else(Vec::new, |l| l.mods_of(s).to_vec());
    let emission =
        |db: &Database, s: &str, li: usize, mods: Vec<(String, usize)>, on_self: bool| {
            emission(db, s, li, mods, on_self).map(|e| Emission {
                devotion: devotion.map_or(0, |d| d.devotion_level(s)),
                ..e
            })
        };
    let mut out = Vec::new();
    for (s, l) in &loadout.skills_and_item_toggles(db) {
        if *l == 0
            || !db
                .get(s)
                .is_some_and(|r| r.class == "Skill_BuffRadiusToggled")
            || !loadout.skill_active(db, s)
        {
            continue;
        }
        let level = loadout.skill_level(db, effective, s).max(1);
        out.extend(emission(db, s, (level - 1) as usize, mods(s), false));
    }
    for (s, li, left) in loadout.buffs.list.as_slice() {
        if *left <= 0.0 {
            continue;
        }
        let wrapper = if db.get(s).is_some_and(|r| r.class == "Skill_BuffRadius") {
            Some(s.clone())
        } else {
            loadout
                .skills
                .skills
                .iter()
                .map(|(w, _)| w)
                .find(|w| {
                    db.get(w).is_some_and(|r| {
                        r.class == "Skill_BuffRadius"
                            && gd_sim::skills::profile_name(r)
                                .is_some_and(|b| b.eq_ignore_ascii_case(s))
                    })
                })
                .cloned()
        };
        if let Some(w) = wrapper {
            out.extend(emission(db, &w, *li, mods(&w), false));
        }
    }
    out
}

/// A `Skill_BuffRadius` goes off (`Skill_BuffRadius::ActivateNow`): its first pass happens now, with the instant
/// heal; an instant one (no `skillActiveDuration`) has only this pass, a lasting one also runs on its caster as a
/// timed buff.
#[derive(Message, Clone, Debug)]
pub struct Activated {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    /// They travel with its buffs and add to its bonus.
    pub mods: Vec<(String, usize)>,
}

#[derive(Component, Default, Debug)]
pub struct AuraPulse(Vec<(Arc<str>, f32)>);

/// An emitter's auras on, cached: the player's rebuilt when what [`player_emissions`] reads changes; a pet's /
/// monster's when `from` (its toggles and timed buffs, skill and level) differs, compared each frame.
#[derive(Component, Default)]
pub struct CachedEmissions {
    /// Empty for the player.
    from: Vec<(String, usize)>,
    list: Vec<Emission>,
}

/// What adds buffs to be counted by the profile rebuilds runs `.before`, what reads them `.after`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AurasSet;

/// [`pulse_auras`]'s per-frame scratch.
#[derive(Default)]
pub struct AuraScratch {
    activated: Vec<Activated>,
    // (emitter, at, side, aura, `ActivateNow`'s pass).
    passes: Vec<(Entity, Vec3, Side, Emission, bool)>,
    now: Vec<Emission>,
    ids: Vec<Entity>,
    /// Entity → index into [`Self::ids`] / the grid's bodies (the targets' query order).
    index: bevy::ecs::entity::EntityHashMap<usize>,
    grid: crate::combat::grid::BodyGrid,
    near: Vec<usize>,
}

/// Every [`PULSE_SECS`] each emitter's auras buff the friendly (debuff auras: hostile) characters in their
/// radius; buffs not refreshed run out. A player whose received buffs changed is marked [`ProfileDirty`].
#[allow(clippy::too_many_arguments)]
pub fn pulse_auras(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut emitters: Query<
        (
            Entity,
            &Transform,
            Option<Ref<Loadout>>,
            Option<Ref<gd_game_core::weapon_pool::SkillLinks>>,
            Option<Ref<gd_game_core::player::EffectiveSkills>>,
            Option<&Pet>,
            Option<&MonsterSkills>,
            crate::combat::SideData,
            Option<&mut AuraPulse>,
            Option<&mut CachedEmissions>,
        ),
        (
            Without<Dead>,
            Or<(With<Loadout>, With<Pet>, With<MonsterSkills>)>,
        ),
    >,
    mut targets: Query<
        (
            Entity,
            &Transform,
            &mut Combatant,
            Option<&mut ReceivedBuffs>,
            Has<Player>,
            crate::combat::VictimSide,
        ),
        (Without<Dead>, Without<gd_game_core::world::Breakable>),
    >,
    alive_q: Query<
        Entity,
        (
            With<Combatant>,
            Without<Dead>,
            Without<gd_game_core::world::Breakable>,
        ),
    >,
    // Option: no devotion auras without it.
    devotion: Option<Res<crate::devotion::DevotionState>>,
    mut activated: MessageReader<Activated>,
    players: Query<Entity, (With<Player>, With<ReceivedBuffs>)>,
    mut scratch: Local<AuraScratch>,
) {
    let sc = &mut *scratch;
    let dt = time.delta_secs();
    sc.activated.clear();
    sc.activated.extend(activated.read().cloned());
    sc.passes.clear();
    for (e, tf, loadout, links, effective, pet, ms, sd, pulse, mut cache) in &mut emitters {
        let computed: Vec<Emission>;
        let on: &[Emission] = match (loadout.as_deref(), pet, ms) {
            (Some(l), ..) => {
                let Some(eff) = effective.as_deref() else {
                    continue;
                };
                let stale = cache.as_deref().is_none_or(|_| {
                    loadout.as_ref().is_some_and(|r| r.is_changed())
                        || effective.as_ref().is_some_and(|r| r.is_changed())
                        || links.as_ref().is_some_and(|r| r.is_changed())
                        || devotion.as_ref().is_some_and(|d| d.is_changed())
                });
                if stale {
                    computed =
                        player_emissions(&data.db, l, eff, links.as_deref(), devotion.as_deref());
                    match cache.as_mut() {
                        Some(c) => c.list = computed.clone(),
                        None => {
                            commands.entity(e).try_insert(CachedEmissions {
                                from: Vec::new(),
                                list: computed.clone(),
                            });
                        }
                    }
                    &computed
                } else {
                    &cache.as_deref().expect("a present cache when fresh").list
                }
            }
            (None, ..) => {
                let (toggled, buffs) = match (pet, ms) {
                    (Some(p), _) => (p.toggled(), p.buffs()),
                    (None, Some(m)) => (m.toggled(), m.buffs.list.as_slice()),
                    (None, None) => continue,
                };
                let from = || {
                    toggled
                        .iter()
                        .map(|(s, li)| (s, *li))
                        .chain(buffs.iter().map(|(s, li, _)| (s, *li)))
                };
                match cache.as_deref() {
                    Some(c) if c.from.iter().map(|(s, li)| (s, *li)).eq(from()) => &c.list,
                    None if toggled.is_empty() && buffs.is_empty() => &[],
                    _ => {
                        computed = match (pet, ms) {
                            (Some(p), _) => p.emissions(&data.db),
                            (None, Some(m)) => m.emissions(&data.db),
                            (None, None) => unreachable!("an emitter without skills went on"),
                        };
                        let fresh = CachedEmissions {
                            from: from().map(|(s, li)| (s.clone(), li)).collect(),
                            list: computed.clone(),
                        };
                        match cache.as_mut() {
                            Some(c) => **c = fresh,
                            None => {
                                commands.entity(e).try_insert(fresh);
                            }
                        }
                        &computed
                    }
                }
            }
        };
        sc.now.clear();
        sc.now.extend(
            sc.activated
                .iter()
                .filter(|a| a.caster == e)
                .filter_map(|a| {
                    emission(
                        &data.db,
                        &a.skill,
                        a.level_index,
                        a.mods.clone(),
                        loadout.is_none(),
                    )
                }),
        );
        for em in &sc.now {
            sc.passes
                .push((e, tf.translation, sd.side(), em.clone(), true));
        }
        let Some(mut pulse) = pulse else {
            if !on.is_empty() || !sc.now.is_empty() {
                let next = sc.now.iter().map(|em| (em.skill.clone(), PULSE_SECS));
                commands.entity(e).try_insert(AuraPulse(next.collect()));
            }
            continue;
        };
        for em in &sc.now {
            pulse.0.retain(|(s, _)| *s != em.skill);
            pulse.0.push((em.skill.clone(), PULSE_SECS + dt));
        }
        pulse.0.retain(|(s, _)| {
            on.iter().any(|o| &o.skill == s) || sc.now.iter().any(|o| &o.skill == s)
        });
        for em in on {
            let t = match pulse.0.iter_mut().find(|(s, _)| *s == em.skill) {
                Some((_, t)) => {
                    *t -= dt;
                    t
                }
                None => {
                    pulse.0.push((em.skill.clone(), 0.0));
                    &mut pulse.0.last_mut().expect("just pushed").1
                }
            };
            if *t > 0.0 {
                continue;
            }
            // A toggle's first pass is its `ActivateNow` / `AutoActivateNow` one (the bonus flag set); a timed
            // buff's came with its [`Activated`].
            let first = *t == 0.0
                && data
                    .db
                    .get(&em.skill)
                    .is_some_and(|r| r.class == "Skill_BuffRadiusToggled");
            *t += PULSE_SECS;
            sc.passes
                .push((e, tf.translation, sd.side(), em.clone(), first));
        }
    }
    // Characters' first buffs, inserted after the passes.
    let mut fresh: std::collections::BTreeMap<Entity, ReceivedBuffs> =
        std::collections::BTreeMap::new();
    // The targets' bodies in query order: a pass scans the ones near it, plus its emitter (whose bonus
    // needs no radius), in that order.
    let grid = if sc.passes.is_empty() {
        None
    } else {
        sc.ids.clear();
        sc.index.clear();
        sc.grid
            .rebuild(targets.iter().enumerate().map(|(i, (t, ttf, cb, ..))| {
                sc.index.insert(t, i);
                sc.ids.push(t);
                cb.body_box(&ttf.compute_affine())
            }));
        Some(&sc.grid)
    };
    for (from, at, side, em, activation) in sc.passes.drain(..) {
        // `ActivateNow`'s pass gives each friend the skill's bonus (`Character::TakeBonus`).
        let bonus = data
            .db
            .get(&em.buff)
            .filter(|_| activation && !em.debuf)
            .map_or_else(Bonus::default, |r| {
                Bonus::of_record(&data.db, r, em.level_index, &em.mods)
            });
        let grid = grid.expect("built for the passes");
        grid.near(at, em.radius, &mut sc.near);
        if let Some(&i) = sc.index.get(&from)
            && let Err(k) = sc.near.binary_search(&i)
        {
            sc.near.insert(k, i);
        }
        for &i in &sc.near {
            let Ok((t, ttf, _, buffs, _, tv)) = targets.get_mut(sc.ids[i]) else {
                continue;
            };
            if t == from {
                if !em.on_self {
                    // The player: found by the activation's pass too, its buff kept elsewhere.
                    bonus.queue(&mut commands, t);
                    continue;
                }
            } else {
                // `IsFoe` for a debuff aura, else `IsFriend`; no hiding character (`FilterInvalidTargets`).
                if tv.hiding() || side.foe(tv.side.side_ref()) != em.debuf {
                    continue;
                }
            }
            if !grid.body(i).meets_sphere(at.to_array(), em.radius) {
                continue;
            }
            bonus.queue(&mut commands, t);
            let b = ReceivedBuff {
                devotion: em.devotion,
                ..ReceivedBuff::new(
                    &data.db,
                    &em.buff,
                    &em.skill,
                    em.level_index,
                    from,
                    em.mods.to_vec(),
                    BUFF_SECS,
                )
            };
            let put = match buffs {
                Some(mut list) => list.land(b.clone(), |c| alive_q.contains(c)),
                None => {
                    debug!(
                        "aura {}: {} gets a buff",
                        em.skill.rsplit('/').next().unwrap_or(""),
                        t
                    );
                    fresh
                        .entry(t)
                        .or_default()
                        .land(b.clone(), |c| alive_q.contains(c))
                }
            };
            // A new `SkillBuff` (also one replacing a running buff at another level) goes active.
            if put.created() {
                crate::skill_buffs::on_created(&mut commands, &data, &b, t, ttf.translation, put);
            }
        }
    }
    for (e, list) in fresh {
        commands.entity(e).try_insert(list);
    }
    for p in &players {
        if let Ok((e, _, _, buffs, player, _)) = targets.get_mut(p) {
            crate::skill_buffs::mark_player(&mut commands, e, buffs, player);
        }
    }
}
