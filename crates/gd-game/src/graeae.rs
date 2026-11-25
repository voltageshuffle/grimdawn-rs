//! `ControllerGraeae` (the Wight hags of Scarstone, the Aetherial reanimators): sisters sharing one
//! eye. Game.dll:
//!
//! - Every Graeae is in one class list (`s_vGraeaes`), so all are sisters; one made while the list
//!   is empty sets `s_bFirstFire`.
//! - `Load`: `eyeTimeMin` / `eyeTimeMax` (s), `eyeFleeChance`, `eyeDodgeChance`, `eyeLeadChance`
//!   (x 0.01), `eyeRandomRepositionChance`.
//! - `InitialUpdate` keeps the attack skill and the eight special slots aside, empties every slot
//!   and puts back only slot 3 (`specialAttack4`, rearmed); `FleeChance`, `DodgeChance`, the
//!   difficulty's `LeadChance` and `randomRepositionChance` are kept aside too.
//! - `GetEye`: `hasEye` set, `eyeTimer` zeroed; the attack becomes slot 0's skill, slots 1 and 4
//!   come back rearmed and slot 3 goes; the chances become the `eye*` ones; `ToggleEyeSkill(true)`.
//!   `LoseEye` is the reverse (`ToggleEyeSkill(false, force)`).
//! - `ToggleEyeSkill` pushes "ToggleEye" (`ControllerGraeaeStateToggleEye`: 2000 ms doing nothing),
//!   then uses slot 2's toggle skill when its on state differs from the wanted one (or forced): at
//!   once for an `instantCast` skill, else `UseSkill` on itself. With the eye given, unless
//!   `s_bFirstFire`, it also uses `chainNextSkill` on itself; the flag is cleared.
//! - `Update`: when no sister holds the eye this one takes it; the holder's timer counts up and from
//!   `eyeTimeMin` on, each update with `IGenerate(0, 100) == 0` or past `eyeTimeMax`, the eye goes to
//!   `GetNextGraeae` (a random other living sister; none: kept).
//! - "Dying" and "Dead" pass a held eye on the same way.
//!
//! PORT: the eye skill toggles through `MonsterSkills::set_toggle` (a non-instant one with its cast
//! animation); the lead chance is set for every difficulty (the engine writes only the current one's).
use crate::combat::MonsterAi;
use crate::monster_skills::MonsterSkills;
use bevy::prelude::*;
use gd_game_core::casting::{Kind, kind_of};
use gd_game_core::character::Character;
use gd_game_core::combat::{ControllerClass, Dead};
use gd_game_core::data::GameData;
use gd_game_core::monster_skills::Special;
use gd_game_core::monsters::Monster;

pub struct GraeaePlugin;

impl Plugin for GraeaePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FirstFire>().add_systems(
            Update,
            (init, update, toggle_eye_states)
                .chain()
                .after(crate::monster_skills::MonsterSkillStage::Init)
                .before(crate::monster_skills::MonsterSkillStage::Choose),
        );
    }
}

/// `ControllerGraeae::s_bFirstFire`: the next eye given is the group's first (no `chainNextSkill`).
#[derive(Resource, Default)]
pub struct FirstFire(pub bool);

/// The "ToggleEye" temporary state: seconds left.
#[derive(Component)]
pub struct ToggleEye(pub f32);

const TOGGLE_EYE_SECS: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Chances {
    pub flee: u32,
    pub dodge: i32,
    pub lead: [f32; 3],
    pub reposition: i32,
}

impl Chances {
    fn of(ai: &MonsterAi) -> Self {
        Self {
            flee: ai.flee.chance,
            dodge: ai.dodge.chance,
            lead: ai.lead,
            reposition: ai.melee.random_reposition,
        }
    }

    fn apply(&self, ai: &mut MonsterAi) {
        ai.flee.chance = self.flee;
        ai.dodge.chance = self.dodge;
        ai.lead = self.lead;
        ai.melee.random_reposition = self.reposition;
    }
}

#[derive(Component, Debug)]
pub struct Graeae {
    /// `ControllerGraeae.hasEye`.
    pub eye: bool,
    /// `ControllerGraeae.eyeTimer` (s).
    timer: f32,
    /// `eyeTimeMin`, `eyeTimeMax` (s).
    time: (f32, f32),
    eye_chances: Chances,
    kept_chances: Chances,
    attack: Option<(String, usize, Kind, f32)>,
    slots: Vec<Special>,
    /// Slot 2's skill (record, level index, `instantCast`).
    eye_skill: Option<(String, usize, bool)>,
    chain_next: Option<(String, usize, Kind)>,
}

impl Graeae {
    fn slot(&self, n: usize) -> Option<Special> {
        self.slots
            .iter()
            .find(|s| s.slot == n)
            .map(Special::rearmed)
    }

    fn plain_kit(&self) -> (Option<(String, usize, Kind, f32)>, Vec<Special>) {
        (self.attack.clone(), self.slot(3).into_iter().collect())
    }

    fn eye_kit(&self) -> (Option<(String, usize, Kind, f32)>, Vec<Special>) {
        let attack = self
            .slots
            .iter()
            .find(|s| s.slot == 0)
            .map(|s| (s.skill.clone(), s.level_index, s.kind, s.range));
        (
            attack,
            [1, 4].into_iter().filter_map(|n| self.slot(n)).collect(),
        )
    }
}

#[allow(clippy::type_complexity)]
fn init(
    mut commands: Commands,
    data: Res<GameData>,
    mut first: ResMut<FirstFire>,
    existing: Query<(), With<Graeae>>,
    mut q: Query<
        (Entity, &Monster, &MonsterAi, &mut MonsterSkills),
        (Without<Graeae>, Added<MonsterSkills>),
    >,
) {
    let mut any = !existing.is_empty();
    for (e, m, ai, mut ms) in &mut q {
        if ai.class != ControllerClass::Graeae {
            continue;
        }
        let Some(rec) = data.db.get(&m.record) else {
            continue;
        };
        let ctl = rec.str("controller").and_then(|c| data.db.get(c));
        let f = |k: &str| ctl.as_ref().and_then(|c| c.f32(k)).unwrap_or(0.0);
        let i = |k: &str| ctl.as_ref().and_then(|c| c.i32(k)).unwrap_or(0);
        let levels = gd_sim::actor::monster_skill_levels(rec, m.level);
        let own = |key: &str| -> Option<(String, usize)> {
            let name = rec.str(key)?;
            levels
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(n, l)| (n.clone(), (*l).max(1) as usize - 1))
        };
        let eye_skill = own("specialAttack3SkillName").map(|(s, li)| {
            let instant = data.db.get(&s).is_some_and(|r| r.bool("instantCast"));
            (s, li, instant)
        });
        let chain_next =
            own("chainNextSkill").and_then(|(s, li)| data.db.get(&s).map(|r| (s, li, kind_of(r))));
        let kept = Chances::of(ai);
        let lead = i("eyeLeadChance") as f32 * 0.01;
        let g = Graeae {
            eye: false,
            timer: 0.0,
            time: (f("eyeTimeMin"), f("eyeTimeMax")),
            eye_chances: Chances {
                flee: i("eyeFleeChance").max(0) as u32,
                dodge: i("eyeDodgeChance"),
                lead: [lead; 3],
                reposition: i("eyeRandomRepositionChance"),
            },
            kept_chances: kept,
            attack: ms.attack.clone(),
            slots: ms.specials.clone(),
            eye_skill,
            chain_next,
        };
        let (attack, specials) = g.plain_kit();
        ms.set_kit(attack, specials);
        if !any {
            first.0 = true;
            any = true;
        }
        debug!(
            "graeae {e}: {} (eye skill {:?})",
            m.record,
            g.eye_skill.as_ref().map(|s| &s.0)
        );
        commands.entity(e).insert(g);
    }
}

/// `GetEye` / `LoseEye`.
#[allow(clippy::too_many_arguments)]
fn set_eye(
    commands: &mut Commands,
    data: &GameData,
    first: &mut FirstFire,
    e: Entity,
    g: &mut Graeae,
    ai: &mut MonsterAi,
    ms: &mut MonsterSkills,
    cb: &gd_game_core::combat::Combatant,
    tf: &mut Transform,
    ch: &mut Character,
    on: bool,
) {
    debug!("graeae {e}: {} the eye", if on { "gets" } else { "loses" });
    g.eye = on;
    if on {
        g.timer = 0.0;
    }
    let (attack, specials) = if on { g.eye_kit() } else { g.plain_kit() };
    ms.set_kit(attack, specials);
    if on { g.eye_chances } else { g.kept_chances }.apply(ai);
    commands.entity(e).try_insert(ToggleEye(TOGGLE_EYE_SECS));
    let Some((skill, li, instant)) = g.eye_skill.clone() else {
        return;
    };
    if ms.toggle_is_on(&skill) == on && on {
        return;
    }
    let now_on = !ms.toggle_is_on(&skill);
    if !instant && let Some(r) = data.db.get(&skill) {
        ch.start_cast(
            &r.class,
            r.str("skillSpecialAnimationName").map(str::to_owned),
        );
    }
    ms.set_toggle(&data.db, &skill, li, now_on);
    crate::monster_skills::sync_damage_auras(commands, data, e, ms);
    if on {
        if !first.0
            && let Some((s, li, kind)) = g.chain_next.clone()
        {
            ms.used(data, cb, &s, li);
            let here = tf.translation;
            crate::monster_skills::start(commands, data, e, tf, ch, &s, li, kind, here, Some(e));
        }
        first.0 = false;
    }
}

fn next_sister(me: Entity, sisters: &[(Entity, bool)]) -> Option<Entity> {
    let others: Vec<Entity> = sisters
        .iter()
        .filter(|(e, alive)| *e != me && *alive)
        .map(|s| s.0)
        .collect();
    (!others.is_empty()).then(|| others[gd_game_core::rng::usize(0..others.len())])
}

#[allow(clippy::type_complexity)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut first: ResMut<FirstFire>,
    mut q: Query<(
        Entity,
        &mut Graeae,
        &mut MonsterAi,
        &mut MonsterSkills,
        &gd_game_core::combat::Combatant,
        &mut Transform,
        &mut Character,
        Has<Dead>,
    )>,
    newly_dead: Query<Entity, (With<Graeae>, Added<Dead>)>,
) {
    let dt = time.delta_secs();
    let sisters: Vec<(Entity, bool)> = q.iter().map(|(e, .., dead)| (e, !dead)).collect();
    let mut pass: Vec<(Entity, Entity)> = Vec::new();
    for e in &newly_dead {
        if q.get(e).is_ok_and(|g| g.1.eye)
            && let Some(n) = next_sister(e, &sisters)
        {
            pass.push((e, n));
        }
    }
    let order: Vec<Entity> = sisters.iter().filter(|s| s.1).map(|s| s.0).collect();
    // Only the take below changes who holds the eye during this loop.
    let mut anyone = q.iter().any(|g| g.1.eye);
    for e in order {
        let Ok((_, mut g, mut ai, mut ms, cb, mut tf, mut ch, _)) = q.get_mut(e) else {
            continue;
        };
        if !anyone {
            anyone = true;
            set_eye(
                &mut commands,
                &data,
                &mut first,
                e,
                &mut g,
                &mut ai,
                &mut ms,
                cb,
                &mut tf,
                &mut ch,
                true,
            );
            continue;
        }
        if !g.eye {
            continue;
        }
        g.timer += dt;
        if g.timer >= g.time.0
            && (gd_game_core::rng::i32(0..101) == 0 || g.timer > g.time.1)
            && let Some(n) = next_sister(e, &sisters)
        {
            pass.push((e, n));
        }
    }
    for (from, to) in pass {
        for (e, on) in [(from, false), (to, true)] {
            let Ok((_, mut g, mut ai, mut ms, cb, mut tf, mut ch, _)) = q.get_mut(e) else {
                continue;
            };
            if g.eye != on {
                set_eye(
                    &mut commands,
                    &data,
                    &mut first,
                    e,
                    &mut g,
                    &mut ai,
                    &mut ms,
                    cb,
                    &mut tf,
                    &mut ch,
                    on,
                );
            }
        }
    }
}

fn toggle_eye_states(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut ToggleEye)>,
) {
    for (e, mut t) in &mut q {
        t.0 -= time.delta_secs();
        if t.0 <= 0.0 {
            commands.entity(e).remove::<ToggleEye>();
        }
    }
}
