//! Evade (`records/skills/default/defaultevade.dbr`, `Skill_Evade`; Space): the hotbar's evade slot
//! (`HotSlotOptionEvade::Activate`) makes the player dash towards the cursor point (or the facing direction, by an
//! option) and `Character::EvadeSkill` moves them there (`JumpToUseSkill`).
//!
//! `HotSlotOptionEvade::Activate`: the point is the controller's last world point under the cursor
//! ([`crate::hotbar::WorldCursor`]), not the cursor at the press. With `evadeFollowCursor` off (or a gamepad,
//! `movementType` = 2) it is the player's coordinates moved along the facing axis, or the keyboard movement's
//! direction while a key is held, by the skill's distance. An empty record (no mouse event over the world yet)
//! evades nowhere. PORT: Space goes through the same rule (the key's way to the evade button was not traced).
//! The record gives the distance (`waveDistance`, 10 m), the speed boost (`characterRunSpeedModifier` +250%) and
//! the cooldown (`skillCooldownTime` × (1 − CDR)) with its charges (`cooldownCharges` summed over the record and
//! its modifiers).
//!
//! The destination (`Character::GetMoveToPoint`, move type 7) is always the full `waveDistance` along the line to
//! the cursor point, checked by `NavManager::FindStraightMovePoint`. The move is a jump
//! (`ControllerPlayerStateEvade::OnBegin` → `EvadeAction::Execute` → `Character::EvadeSkill` = `JumpToUseSkill`,
//! animation type 48): between the evade animation's `StartJump` and `StopJump` the player covers the distance at
//! the animation's pace. The record grants no defence or immunity.
//! PORT: the straight-move query is the port's nav ground walk in 0.25 m steps.
//!
//! Evade during an attack (`ControllerPlayer::EvadeAction` → `RequestEvadeAction`): Idle, the move states and
//! `UseSkill` take `DefaultRequestEvadeAction`: refused for an unusable skill, a held repeat of a non-repeatable
//! skill or a move-to point with no region, else the Evade state is set. `MoveToUseSkill` / `JumpToUseSkill`
//! refuse it while their skill's warm-up was active (PORT: a [`gd_game_core::casting::Leap`] in flight, a
//! [`crate::movement::MoveToUseSkill`] with its warm-up paid). `EvadeAction::Execute` (refused before anything is
//! spent when the character cannot act) stops the current skill, clears the attack target and runs `EvadeSkill`;
//! the cooldown starts only in `Skill_Evade::WarmUpStart`, so a refused evade costs nothing; that function also
//! plays `skillWarmUpSound` at the player. In the Evade state skill and move requests are refused until the clip's
//! `StopJump`. The recovery is cancelable: `StopJump` arms [`Character::allow_interrupt`], so a new or queued
//! action cuts it. Its "End" sets Idle.
use crate::movement::MovePath;
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::casting::Caster;
use gd_game_core::character::{CharState, Character};
use gd_game_core::player::Player;

pub const EVADE: &str = "records/skills/default/defaultevade.dbr";

pub struct EvadePlugin;

impl Plugin for EvadePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EvadeRequest>().add_systems(
            Update,
            (start_evade.in_set(EvadeStage::Start), evading)
                .chain()
                .after(crate::hotbar::HotbarSet)
                .before(crate::casting::CastingSet),
        );
    }
}

/// An evade in progress, until its animation ends (the movement is its [`gd_game_core::casting::Leap`]).
#[derive(Component)]
pub struct Evading;

/// Towards the cursor record or the facing way; `scripted` (a test hook) always the facing way.
#[derive(Message, Clone, Copy, Debug)]
pub struct EvadeRequest {
    pub scripted: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn start_evade(
    mut commands: Commands,
    mut requests: MessageReader<EvadeRequest>,
    data: Res<gd_game_core::data::GameData>,
    nav: Res<Nav>,
    (cursor, options): (
        Res<crate::hotbar::WorldCursor>,
        Res<gd_game_core::options::GameOptions>,
    ),
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut Caster,
            &mut MovePath,
            &mut Character,
            &mut gd_game_core::combat::Combatant,
            Option<&crate::movement::MoveToUseSkill>,
            Has<gd_game_core::casting::Leap>,
            &gd_game_core::player::ControllerDirection,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
        ),
        (
            With<Player>,
            Without<Evading>,
            Without<gd_game_core::combat::Dead>,
            Without<crate::movement::Stunned>,
        ),
    >,
) {
    let Some(req) = requests.read().last().copied() else {
        return;
    };
    let Ok((e, tf, mut caster, mut path, mut ch, mut cb, pending, leaping, controller, links)) =
        players.single_mut()
    else {
        return;
    };
    let scripted = req.scripted;
    if ch.frozen {
        return;
    }
    // `ControllerPlayerStateMoveToUseSkill::RequestEvadeAction`, JumpToUseSkill's too.
    if leaping || pending.is_some_and(|m| m.warm_up.is_some()) {
        debug!("evade refused: a warmed-up skill is under way");
        return;
    }
    if !caster.cooldowns.usable(EVADE) {
        info!(
            "evade cooling down ({:.1} s)",
            caster.cooldowns.get(EVADE).copied().unwrap_or(0.0)
        );
        return;
    }
    let Some(rec) = data.db.get(EVADE) else {
        return;
    };
    let max = rec.f32("waveDistance").unwrap_or(10.0);
    // Towards the cursor record, else the facing axis or the keyboard movement's direction. Keyboard Only
    // (`movementType` 2) never follows the cursor.
    let follow = options.flag("evadeFollowCursor") && options.int("movementType") != 2 && !scripted;
    let facing = if controller.length > 0.0 && !scripted {
        controller.dir
    } else {
        (tf.rotation * Vec3::Z).with_y(0.0)
    };
    let dir = if follow {
        let Some(p) = cursor.point else {
            debug!("evade: no cursor point yet");
            return;
        };
        match Some((p - tf.translation).with_y(0.0)).filter(|d| d.length() > 0.3) {
            Some(d) => d,
            None => (tf.rotation * Vec3::Z).with_y(0.0),
        }
    } else {
        facing
    };
    // Move type 7: always the full distance.
    let dist = max;
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return;
    }
    // A closed door stops it.
    let to = crate::nav::straight_move_point(&nav.0, tf.translation, tf.translation + dir * dist);
    if to.distance(tf.translation) < 0.5 {
        return;
    }
    // The Evade runes: cooldown reduction and `cooldownCharges`.
    let mods: Vec<(gd_formats::arz::Record, usize)> = links
        .map(|l| {
            l.mods_of(EVADE)
                .iter()
                .filter_map(|(m, ml)| data.db.get_owned(m).map(|r| (r, *ml)))
                .collect()
        })
        .unwrap_or_default();
    let mod_refs: Vec<(&gd_formats::arz::Record, usize)> =
        mods.iter().map(|(r, l)| (r, *l)).collect();
    let cd = crate::casting::cooldown_for(
        rec,
        0,
        &mod_refs,
        cb.profile.skill_attribute("skillCooldownReduction"),
    );
    let charges = std::iter::once((rec, 0))
        .chain(mod_refs.iter().copied())
        .map(|(r, l)| gd_game_core::casting::level_f(r, "cooldownCharges", l))
        .sum::<f32>()
        .max(1.0) as u32;
    caster.cooldowns.start(EVADE.to_owned(), cd, charges);
    // `Skill_Evade::WarmUpStart`.
    if let Some(pak) = rec.str("skillWarmUpSound").filter(|s| !s.is_empty()) {
        commands.write_message(gd_game_core::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(tf.translation),
        });
    }
    path.points.clear();
    // `EvadeAction::Execute`: the current skill stops and the attack target becomes the evade's (none).
    cb.target = None;
    commands.entity(e).try_remove::<(
        gd_game_core::casting::Casting,
        gd_game_core::casting::Channel,
        crate::casting::AimPoint,
        crate::movement::MoveToUseSkill,
    )>();
    info!("evade {:.1} m", to.distance(tf.translation));
    // The evade is a cast of its skill record (no `cast_requests` path).
    gd_game_core::visuals::notify(&mut commands, EVADE);
    // `EvadeAction::Execute` looks at start + `World::GetDirection(start, point)` (not normalised): the point.
    ch.turn.look_at(to);
    ch.state = CharState::Evade;
    // Refuses skill / move requests until `StopJump`; the evade supersedes the pending slot.
    ch.evade_warmup = true;
    ch.queued = None;
    commands.entity(e).try_insert((
        Evading,
        gd_game_core::casting::Leap::new(vec![to], (to - tf.translation).with_y(0.0).length()),
    ));
}

fn evading(
    mut commands: Commands,
    mut events: MessageReader<gd_game_core::character::AnimCallback>,
    mut q: Query<(Entity, &mut Character), With<Evading>>,
) {
    for ev in events.read() {
        if ev.name != "End" || ev.anim != gd_sim::anim::AnimType::Evade {
            continue;
        }
        let Ok((e, mut ch)) = q.get_mut(ev.entity) else {
            continue;
        };
        if ch.state == CharState::Evade {
            ch.state = CharState::Idle;
        }
        debug!("evade ended");
        commands
            .entity(e)
            .try_remove::<(Evading, gd_game_core::casting::Leap)>();
    }
    // Knocked out of the evade (stun, death, no evade animation).
    for (e, ch) in &q {
        if ch.state != CharState::Evade {
            commands
                .entity(e)
                .try_remove::<(Evading, gd_game_core::casting::Leap)>();
        }
    }
}

/// What can cancel or block an evade orders before `Start`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvadeStage {
    Start,
}
