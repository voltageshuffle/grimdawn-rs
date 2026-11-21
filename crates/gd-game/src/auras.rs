//! Radius buffs reaching allies: the buffs a character's auras
//! (`Skill_BuffRadiusToggled`) and timed radius buffs (`Skill_BuffRadius`)
//! put on the characters around it.
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
//! - A new buff goes to the front of [`ReceivedBuffs::list`](crate::skill_buffs::ReceivedBuffs). A new
//!   buff on another character starts the record's `charFxPakOtherNames` there.
//!
//! Emitters ([`pulse_auras`]): the player (switched-on toggles with `SkillLinks` modifiers, running
//! timed buffs), pets (`Pet::emissions`) and monsters (`MonsterSkills::emissions`). Sides come from
//! [`crate::combat::Side`].
//!
//! PORT: the player is never its own target here; its own auras and timed buffs count through
//! `SkillBook::stat_sources` / `Loadout::buffs`, and [`Activated`] gives it the bonus alone.
use crate::casting::{Bonus, level_f};
use crate::combat::{Combatant, Dead, Side};
use crate::data::GameData;
use crate::monster_skills::MonsterSkills;
use crate::pets::Pet;
use crate::player::{Loadout, Player};
use crate::skill_buffs::{ReceivedBuff, ReceivedBuffs};
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;

/// `Skill_BuffRadius` +383: ms between passes.
pub const PULSE_SECS: f32 = 1.0;
/// `Skill_BuffRadius` +384: how long one pass's buff lasts on a target.
pub const BUFF_SECS: f32 = 1.5;

/// An aura a character has on: what each pass puts on its targets.
#[derive(Clone, Debug, PartialEq)]
pub struct Emission {
    pub skill: String,
    pub level_index: usize,
    pub buff: String,
    pub radius: f32,
    pub mods: Vec<(String, usize)>,
    /// The caster is its own target (module docs; not for the player, never
    /// for a debuff).
    pub on_self: bool,
    /// A debuff aura (`debufSkill`): foes are its targets.
    pub debuf: bool,
    /// The skill's devotion level, which its buffs carry
    /// (`ReceivedBuff::devotion`; a celestial power's, else 0).
    pub devotion: u32,
}

/// Whether a radius-buff skill is a debuff aura (`debufSkill` of its buff
/// record, +1576; also read from the skill record).
pub fn is_debuf(db: &Database, rec: &Record) -> bool {
    rec.bool("debufSkill")
        || gd_sim::skills::profile_name(rec)
            .and_then(|b| db.get(b))
            .is_some_and(|b| b.bool("debufSkill"))
}

/// The emission of radius-buff skill `skill` at `li`: None without a buff
/// record or a radius.
pub fn emission(
    db: &Database,
    skill: &str,
    li: usize,
    mods: Vec<(String, usize)>,
    on_self: bool,
) -> Option<Emission> {
    let rec = db.get(skill)?;
    let buff = gd_sim::skills::profile_name(rec)?.to_owned();
    let brec = db.get(&buff)?;
    let debuf = brec.bool("debufSkill") || rec.bool("debufSkill");
    let radius = level_f(brec, "skillTargetRadius", li).max(level_f(rec, "skillTargetRadius", li));
    (radius > 0.0).then_some(Emission {
        skill: skill.to_owned(),
        level_index: li,
        buff,
        radius,
        mods,
        on_self: on_self && !debuf,
        debuf,
        devotion: 0,
    })
}

/// The player's auras on (module docs): switched-on radius toggles at their
/// effective level and running `Skill_BuffRadius` casts (the player's cast
/// runs under the wrapper's `buffSkillName`, `casting.rs`), each with its
/// devotion level (celestial powers: `devotion`'s).
pub fn player_emissions(
    db: &Database,
    loadout: &Loadout,
    effective: &gd_sim::skills::SkillBook,
    links: Option<&crate::weapon_pool::SkillLinks>,
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
    // Learned and worn items' radius toggles (`crate::player` docs, item toggles).
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

/// A `Skill_BuffRadius` goes off (`Skill_BuffRadius::ActivateNow`, module docs): its first pass happens now, with
/// the instant heal, whether or not the skill lasts (an instant one, no `skillActiveDuration`, has only this pass).
/// Sent for every cast of one ([`crate::monster_skills`]' buff-other cast, `casting`'s self buffs); a lasting one
/// also runs on its caster as a timed buff, whose later passes are the regular ones.
#[derive(Message, Clone, Debug)]
pub struct Activated {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    /// The caster's modifiers of the skill (they travel with its buffs and add to its bonus).
    pub mods: Vec<(String, usize)>,
}

/// Seconds to the next pass of each aura a character has on.
#[derive(Component, Default, Debug)]
pub struct AuraPulse(Vec<(String, f32)>);

/// An emitter's auras on, cached ([`pulse_auras`]). The player's: [`player_emissions`] clones
/// the skill book and scans the worn items, so it runs only when what it reads
/// changes (the [`Loadout`], the [`Caster`]'s timed buffs, the [`crate::weapon_pool::SkillLinks`]
/// modifiers, the devotion levels), not every frame. A pet's / monster's depend only on its
/// switched-on toggles and timed buffs (skill and level; their timers change every frame, so
/// change detection would always fire): `from` keeps those, compared each frame.
#[derive(Component, Default)]
pub struct CachedEmissions {
    /// A pet's / monster's toggles then timed buffs the list was made from (empty: the player).
    from: Vec<(String, usize)>,
    list: Vec<Emission>,
}

/// The aura pulse ([`pulse_auras`]): what adds buffs to be counted by the profile rebuilds runs `.before`, what reads them `.after`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AurasSet;

/// The passes (module docs): every [`PULSE_SECS`] each emitter's auras buff
/// the friendly (debuff auras: hostile) characters in their radius; buffs
/// not refreshed run out. A player whose received buffs changed is marked
/// [`ProfileDirty`]; pets and monsters rebuild theirs (`pets.rs`,
/// `monster_skills.rs`).
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
            Option<Ref<crate::weapon_pool::SkillLinks>>,
            // The player's effective skill levels (absent until derived).
            Option<Ref<crate::player::EffectiveSkills>>,
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
        (Without<Dead>, Without<crate::world::Breakable>),
    >,
    // Whether a buff's previous caster is still a live target (`land`'s alive
    // test), asked per replacement instead of collecting every combatant.
    alive_q: Query<
        Entity,
        (
            With<Combatant>,
            Without<Dead>,
            Without<crate::world::Breakable>,
        ),
    >,
    // Option: devotion is optional input (no devotion auras without it).
    devotion: Option<Res<crate::devotion::DevotionState>>,
    mut activated: MessageReader<Activated>,
    players: Query<Entity, (With<Player>, With<ReceivedBuffs>)>,
) {
    let dt = time.delta_secs();
    let activated: Vec<Activated> = activated.read().cloned().collect();
    // Buffs ran out already (`crate::skill_buffs::run_buffs`); a pass this
    // frame lands on the ones in range.
    // (emitter, at, side, aura, `ActivateNow`'s pass).
    let mut passes: Vec<(Entity, Vec3, Side, Emission, bool)> = Vec::new();
    for (e, tf, loadout, links, effective, pet, ms, sd, pulse, mut cache) in &mut emitters {
        // The auras on, cached (`CachedEmissions`): the player's rebuilt when what they read
        // changed, a pet's / monster's when its toggles or timed buffs did.
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
                    // Nothing on and nothing cached: no component for a character without auras.
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
        // Activations now: their pass, and a lasting one's next pass a second later.
        let now: Vec<Emission> = activated
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
            })
            .collect();
        for em in &now {
            passes.push((e, tf.translation, sd.side(), em.clone(), true));
        }
        let Some(mut pulse) = pulse else {
            if !on.is_empty() || !now.is_empty() {
                let next = now.iter().map(|em| (em.skill.clone(), PULSE_SECS));
                commands.entity(e).try_insert(AuraPulse(next.collect()));
            }
            continue;
        };
        for em in &now {
            pulse.0.retain(|(s, _)| *s != em.skill);
            pulse.0.push((em.skill.clone(), PULSE_SECS + dt));
        }
        pulse
            .0
            .retain(|(s, _)| on.iter().any(|o| &o.skill == s) || now.iter().any(|o| &o.skill == s));
        for em in on {
            // A new aura passes at once (`ActivateNow`), then every second.
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
            // A toggle's first pass is its `ActivateNow` / `AutoActivateNow` one (the bonus flag set,
            // `Skill_BuffRadiusToggled::ActivateNow` @0x1804e9fa0, `AutoActivateNow` @0x1804e9c50); a timed
            // buff's came with its [`Activated`].
            let first = *t == 0.0
                && data
                    .db
                    .get(&em.skill)
                    .is_some_and(|r| r.class == "Skill_BuffRadiusToggled");
            *t += PULSE_SECS;
            passes.push((e, tf.translation, sd.side(), em.clone(), first));
        }
    }
    // Characters' first buffs (inserted after the passes).
    let mut fresh: std::collections::BTreeMap<Entity, ReceivedBuffs> =
        std::collections::BTreeMap::new();
    // The targets' bodies in query order, indexed (`crate::combat::grid`): a pass scans the ones near it, plus
    // its emitter (whose bonus needs no radius), in that order.
    let (ids, grid): (Vec<Entity>, _) = if passes.is_empty() {
        (Vec::new(), None)
    } else {
        let (ids, boxes) = targets
            .iter()
            .map(|(t, ttf, cb, ..)| (t, cb.body_box(&ttf.compute_affine())))
            .unzip();
        (ids, Some(crate::combat::grid::BodyGrid::new(boxes)))
    };
    let mut near = Vec::new();
    for (from, at, side, em, activation) in passes {
        // `ActivateNow`'s pass gives each friend the skill's bonus (`Character::TakeBonus`, module docs).
        let bonus = data
            .db
            .get(&em.buff)
            .filter(|_| activation && !em.debuf)
            .map_or_else(Bonus::default, |r| {
                Bonus::of_record(&data.db, r, em.level_index, &em.mods)
            });
        let grid = grid.as_ref().expect("built for the passes");
        grid.near(at, em.radius, &mut near);
        if let Some(i) = ids.iter().position(|&t| t == from)
            && let Err(k) = near.binary_search(&i)
        {
            near.insert(k, i);
        }
        for &i in &near {
            let Ok((t, ttf, _, buffs, _, tv)) = targets.get_mut(ids[i]) else {
                continue;
            };
            if t == from {
                if !em.on_self {
                    // The player: found by the activation's pass too, its buff kept elsewhere (module docs).
                    bonus.queue(&mut commands, t);
                    continue;
                }
            } else {
                // `IsFoe` for a debuff aura, else `IsFriend` (its complement between characters); no hiding
                // character (`FilterInvalidTargets`, `combat::VictimSideItem::hiding`: a player's totems).
                if tv.hiding() || side.foe(tv.side.side_ref()) != em.debuf {
                    continue;
                }
            }
            // `Combatant::in_radius`, on the box taken this frame.
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
                    em.mods.clone(),
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
            // A new `SkillBuff` (also one replacing a running buff at another
            // level) goes active (`crate::skill_buffs::on_created`).
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
