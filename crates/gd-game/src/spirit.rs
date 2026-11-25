//! `ControllerSpirit` and `ControllerSpiritHost` (the Ancient Grove witch
//! ritual: the wraith `witchritual_spirit` animates the stone basilisk
//! `basilisk_witchritual`, whose class is `SpiritHost`). Game.dll:
//!
//! The host (`ControllerSpiritHost`, load `spiritThatCanAnimateMe` in `ControllerSpiritHost::Load`,
//! `numSpiritsToAnimateMe`):
//! - "Startup" `OnBegin`: a host not yet animated (`SpiritHost.animated`,
//!   a save flag; the port keeps none) plays animation type 18
//!   ("StatueSit"), `SetCausesAnger(false)` and `SetInvincible(true)`. It
//!   does nothing else: the Startup state is the stub with only `Attacked`.
//! - `ControllerSpiritHostStateStartup::Attacked`: the attacker
//!   must be a Character, and (when `spiritThatCanAnimateMe` is set) its
//!   record must be that record, else "failed to animate"; it must be a
//!   `Monster`; it is added once to the host's list of attackers (`ControllerSpiritHost.attackers`) and
//!   when that list holds `numSpiritsToAnimateMe` of them (0: at once) the
//!   host is animated (`ControllerSpiritHostState::SetAnimated`), going to "Animate".
//! - "Animate" `OnBegin`: `SetCausesAnger(true)`,
//!   `SetInvincible(false)`; its other handlers are `ControllerMonster`'s.
//!
//! The spirit (`ControllerSpirit`, load `spiritTarget`; its record is
//! `invincible`, with no attack skill):
//! - "Startup" `OnBegin`: `SetMyTeam` (faction
//!   -2), animation type 20, `FindNewAnimateTarget`: the allies
//!   in sight that are `SpiritHost`s, can still be animated (`ControllerSpiritHost::CanBeAnimated`:
//!   not animated and fewer living monsters among the host's assigned
//!   spirits (`ControllerSpiritHost.assignedSpirits`) than `numSpiritsToAnimateMe`) and whose record is
//!   `spiritTarget` (an empty one: any); the nearest is kept (`ControllerSpirit.animateTarget`) and the
//!   spirit joins its assigned list. The animation's "End"
//!   (`HandleEvent`) goes to "PursueToAnimate" with that host
//!   (`ChooseBestSkill`), or "Idle" when none was found.
//! - "PursueToAnimate" `OnBegin` / `OnUpdate`: a host that
//!   is alive and already in reach: "AttackToAnimate"; else moves to it. A
//!   host that became animated: a new `FindNewAnimateTarget`, none: the spirit
//!   kills itself (`KillMe(false)`). `EndOfPathReached`:
//!   "AttackToAnimate". `PathFailed`: "PursueToAnimate" again.
//! - "AttackToAnimate" `OnBegin`: `UseSkill` on the host (the
//!   chosen skill, none here: the plain attack); no host: "Idle". Its
//!   `HandleEvent` "End" kills the spirit.
//!
//! PORT: monsters never hit monsters (the port's sides), so the spirit's
//! swing is its Attack animation played at the host and `Attacked` runs when
//! it ends (the engine runs it at the hit, shortly before "End", which kills
//! the spirit); a missing Respawn / Attack animation passes at once; "in
//! sight" is the controller's `ViewDistance` (the faction -2 is not
//! modelled); reach is the two radii + 1.5 m (the plain attack); a failed
//! path is tried again after a second; the host is not saved (always dormant
//! on spawn). `SetCausesAnger` is the host's [`MonsterAi::as_target`]
//! `causes_anger`: a dormant host is not seen by its foes' sight
//! (`combat::sight`) and draws no anger when it is hit.
use crate::combat::{Invincible, MonsterAi, Suicide};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::{Combatant, ControllerClass, Dead};
use gd_game_core::data::GameData;
use gd_game_core::monsters::Monster;
use gd_sim::anim::AnimType;

pub struct SpiritPlugin;

impl Plugin for SpiritPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (init_hosts, init_spirits, spirits)
                .chain()
                .before(crate::monster_skills::MonsterSkillStage::Choose),
        );
    }
}

/// A `SpiritHost` that has not been animated (its AI is off).
#[derive(Component)]
pub struct Dormant;

/// The `ControllerSpiritHost` settings and bookkeeping.
#[derive(Component)]
pub struct SpiritHost {
    /// `spiritThatCanAnimateMe` ("": any attacker).
    pub spirit: String,
    /// `numSpiritsToAnimateMe`.
    pub needed: usize,
    /// The spirits that attacked it (`ControllerSpiritHost.attackers`).
    pub attackers: Vec<Entity>,
    /// The spirits assigned to it (`ControllerSpiritHost.assignedSpirits`).
    pub assigned: Vec<Entity>,
}

fn same_record(a: &str, b: &str) -> bool {
    a.replace('\\', "/")
        .eq_ignore_ascii_case(&b.replace('\\', "/"))
}

impl SpiritHost {
    /// `ControllerSpiritHostStateStartup::Attacked`: true when
    /// the attack by `by` (a monster of `record`) animates the host.
    pub fn attacked(&mut self, by: Entity, record: &str) -> bool {
        if !self.spirit.is_empty() && !same_record(&self.spirit, record) {
            warn!("ControllerSpiritHostStateStartup - failed to animate");
            return false;
        }
        if self.needed == 0 {
            return true;
        }
        if !self.attackers.contains(&by) {
            self.attackers.push(by);
        }
        self.attackers.len() >= self.needed
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// "Startup": waiting for the animation's end.
    Startup,
    Pursue,
    Attack,
    Idle,
}

/// The `ControllerSpirit` state.
#[derive(Component)]
pub struct Spirit {
    /// `spiritTarget`.
    target_record: String,
    /// The host it animates (`ControllerSpirit.animateTarget`).
    pub host: Option<Entity>,
    stage: Stage,
    begun: bool,
    swung: bool,
    path: Vec<Vec3>,
    retry: f32,
}

/// The plain attack's reach beyond the two radii.
const REACH: f32 = 1.5;

fn init_hosts(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (Entity, &Monster, &mut MonsterAi, &mut Character),
        (Without<SpiritHost>, Added<MonsterAi>),
    >,
) {
    for (e, m, mut ai, mut ch) in &mut q {
        if ai.class != ControllerClass::SpiritHost {
            continue;
        }
        let Some(rec) = data.db.get(&m.record) else {
            continue;
        };
        let Some(ctl) = rec.str("controller").and_then(|c| data.db.get(c)) else {
            continue;
        };
        // "Startup" `OnBegin`: StatueSit (type 18),
        // `SetCausesAnger(false)`, invincible.
        ch.play_scripted(AnimType::Special);
        ai.as_target.causes_anger = false;
        commands.entity(e).try_insert((
            SpiritHost {
                spirit: ctl.str("spiritThatCanAnimateMe").unwrap_or("").to_owned(),
                needed: ctl.i32("numSpiritsToAnimateMe").unwrap_or(1).max(0) as usize,
                attackers: Vec::new(),
                assigned: Vec::new(),
            },
            Dormant,
            Invincible,
        ));
    }
}

fn init_spirits(
    mut commands: Commands,
    data: Res<GameData>,
    q: Query<(Entity, &Monster, &MonsterAi), (Without<Spirit>, Added<MonsterAi>)>,
) {
    for (e, m, ai) in &q {
        if ai.class != ControllerClass::Spirit {
            continue;
        }
        let ctl = data
            .db
            .get(&m.record)
            .and_then(|r| r.str("controller").and_then(|c| data.db.get(c)));
        let target_record = ctl
            .and_then(|c| c.str("spiritTarget").map(str::to_owned))
            .unwrap_or_default();
        commands.entity(e).try_insert(Spirit {
            target_record,
            host: None,
            stage: Stage::Startup,
            begun: false,
            swung: false,
            path: Vec::new(),
            retry: 0.0,
        });
    }
}

type HostRow<'a> = (
    Entity,
    &'a mut SpiritHost,
    &'a Transform,
    &'a Combatant,
    &'a Monster,
    Has<Dormant>,
);

/// `ControllerSpiritState::FindNewAnimateTarget`: the nearest
/// host that can be animated; the spirit joins its list.
fn find_host(
    spirit: Entity,
    s: &mut Spirit,
    ai: &MonsterAi,
    me: Vec3,
    hosts: &mut Query<HostRow, (Without<Spirit>, Without<Dead>)>,
    living: &Query<(), (With<Monster>, Without<Dead>)>,
) {
    s.host = None;
    let mut best: Option<(Entity, f32)> = None;
    for (e, h, tf, _, m, dormant) in hosts.iter() {
        let d = tf.translation.distance(me);
        // `ControllerSpiritHost::CanBeAnimated`: not animated, room for another spirit.
        let room = h.assigned.iter().filter(|a| living.contains(**a)).count() < h.needed;
        if !dormant
            || !room
            || d > ai.view
            || (!s.target_record.is_empty() && !same_record(&s.target_record, &m.record))
        {
            continue;
        }
        if best.is_none_or(|b| d < b.1) {
            best = Some((e, d));
        }
    }
    if let Some((e, _)) = best {
        if let Ok((_, mut h, ..)) = hosts.get_mut(e)
            && !h.assigned.contains(&spirit)
        {
            h.assigned.push(spirit);
        }
        s.host = Some(e);
    }
}

#[allow(clippy::too_many_arguments)]
fn spirits(
    mut commands: Commands,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    time: Res<Time>,
    nav: Res<Nav>,
    mut sp: Query<
        (
            Entity,
            &mut Spirit,
            &mut Transform,
            &mut Character,
            &MonsterAi,
            &Combatant,
            &Monster,
        ),
        (Without<Dead>, Without<SpiritHost>, Without<Suicide>),
    >,
    mut hosts: Query<HostRow, (Without<Spirit>, Without<Dead>)>,
    mut host_ai: Query<&mut MonsterAi, With<SpiritHost>>,
    living: Query<(), (With<Monster>, Without<Dead>)>,
    mut crowd: Query<&mut crate::movement::CrowdAgent>,
) {
    let dt = time.delta_secs();
    for (e, mut s, mut tf, mut ch, ai, cb, m) in &mut sp {
        let s = &mut *s;
        match s.stage {
            Stage::Startup => {
                if !s.begun {
                    s.begun = true;
                    find_host(e, s, ai, tf.translation, &mut hosts, &living);
                    // Animation type 20 (Respawn); "End" moves on.
                    if ch.play_scripted(AnimType::Respawn) {
                        continue;
                    }
                }
                if ch.state == CharState::Scripted {
                    continue;
                }
                if s.host.is_some() {
                    s.stage = Stage::Pursue;
                } else {
                    warn!(
                        "Spirit error - went to idle because it could not find an animate target."
                    );
                    s.stage = Stage::Idle;
                }
            }
            Stage::Pursue => {
                let valid = s.host.and_then(|h| hosts.get(h).ok()).is_some_and(|h| h.5);
                if !valid {
                    find_host(e, s, ai, tf.translation, &mut hosts, &living);
                    s.path.clear();
                    if s.host.is_none() {
                        warn!(
                            "Spirit error - suicide because StatePursueToAnimate could not find a valid animate target."
                        );
                        commands.entity(e).try_insert(Suicide);
                        continue;
                    }
                }
                let Ok((_, _, htf, hcb, ..)) = hosts.get(s.host.unwrap()) else {
                    continue;
                };
                let (to, reach) = (htf.translation, cb.radius + hcb.radius + REACH);
                let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
                if flat(tf.translation, to) <= reach {
                    s.stage = Stage::Attack;
                    s.path.clear();
                    continue;
                }
                s.retry -= dt;
                if s.path.is_empty() {
                    if s.retry > 0.0 {
                        continue;
                    }
                    match nav.0.find_path(
                        &mut scratch,
                        tf.translation.to_array(),
                        to.to_array(),
                        crate::nav::PATH_BUDGET,
                    ) {
                        Some(p) => s.path = p.into_iter().skip(1).map(Vec3::from).collect(),
                        // `PathFailed`: pursue again.
                        None => {
                            s.retry = 1.0;
                            continue;
                        }
                    }
                }
                let moved = crate::combat::advance_steered(
                    &mut tf,
                    &mut s.path,
                    crate::combat::run_speed(&ch),
                    dt,
                    &mut ch.turn,
                    crowd.get_mut(e).ok().as_deref_mut(),
                );
                if let Some(g) =
                    nav.0
                        .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
                {
                    tf.translation.y = g;
                }
                ch.state = if moved {
                    CharState::Run
                } else {
                    CharState::Idle
                };
                // `EndOfPathReached`: attack.
                if s.path.is_empty() {
                    s.stage = Stage::Attack;
                }
            }
            Stage::Attack => {
                let Some((h, htf, dormant)) = s
                    .host
                    .and_then(|h| hosts.get(h).ok().map(|r| (r.0, r.2.translation, r.5)))
                else {
                    warn!(
                        "Spirit error - went to idle because StateAttackToAnimate could not find a valid animate target."
                    );
                    s.stage = Stage::Idle;
                    continue;
                };
                let finished = if s.swung {
                    ch.state != CharState::Scripted
                } else {
                    s.swung = true;
                    // PORT: the spirit's swing at its host is a played animation, not an attack action: it
                    // faces the host at once.
                    ch.turn.face_now(htf - tf.translation);
                    !ch.play_scripted(AnimType::Attack)
                };
                if !finished {
                    continue;
                }
                // "End": the swing landed (PORT) and the spirit dies.
                if dormant
                    && let Ok((_, mut host, ..)) = hosts.get_mut(h)
                    && host.attacked(e, &m.record)
                {
                    debug!("spirit {e} animates host {h}");
                    // "Animate" `OnBegin`: `SetCausesAnger(true)`,
                    // `SetInvincible(false)`.
                    if let Ok(mut hai) = host_ai.get_mut(h) {
                        hai.as_target.causes_anger = true;
                    }
                    commands.entity(h).try_remove::<(Dormant, Invincible)>();
                }
                commands.entity(e).try_insert(Suicide);
            }
            Stage::Idle => {}
        }
    }
}
