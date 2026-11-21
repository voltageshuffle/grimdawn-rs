//! Evade (`records/skills/default/defaultevade.dbr`, `Skill_Evade`; Space):
//! the hotbar's evade slot (`HotSlotOptionEvade::Activate` @0x180276840)
//! makes the player dash towards the cursor point (or the facing direction,
//! by an option) and `Character::EvadeSkill` moves them there
//! (`JumpToUseSkill`).
//!
//! `HotSlotOptionEvade::Activate`: the point is the player
//! controller's cursor record (`ControllerPlayer` +1088, the last world point
//! under the cursor: [`crate::hotbar::WorldCursor`]), not the cursor at the
//! press — an evade clicked on the hotbar dashes towards where the cursor last
//! was over the world. With `evadeFollowCursor` (option bool 75) off (or a
//! gamepad, int option 13 = 2) it is the player's coordinates moved along the
//! facing axis, or the keyboard movement's controller direction while a key is
//! held (`ControllerPlayer` +1112 / +1124), by the skill's distance (`NavManager::FindStraightMovePointOnSlopes`).
//! An empty record (no region: no mouse event over the world yet) evades
//! nowhere. PORT: Space goes through the same rule (the key's way to the
//! evade button was not traced). The record gives the distance
//! (`waveDistance`, 10 m), the speed boost (`characterRunSpeedModifier` +250%)
//! and the cooldown (`skillCooldownTime` × (1 − CDR), the shared rule).
//!
//! The destination is `Character::GetMoveToPoint` @0x18005e970
//! for move type 7 (set by the `Skill_Evade` constructor): always the full
//! `waveDistance` from the player along the line to the cursor point (the
//! cursor distance is not used, unlike types 3 / 5), checked by
//! `CharacterMovementManager::FindPath` / `NavManager::FindStraightMovePoint`.
//! The move itself is a jump: `ControllerPlayerStateEvade::OnBegin` →
//! `ControllerAI::Evade` (animation type 48 = the `<style>EvadeAnim` slots,
//! controller speed 1.0) → `EvadeAction::Execute` → `Character::EvadeSkill`
//! = `JumpToUseSkill`, the same root-motion movement as a leap
//! (`casting::Leap`): between the evade animation's `StartJump` and
//! `StopJump` points the player covers the distance at the animation's own
//! pace. The record grants no defence or immunity (no invulnerability).
//! PORT: the straight-move query is the port's nav ground walk in 0.25 m
//! steps. PORT: `GD_EVADE` (hooks.rs) evades forward once.
//!
//! Evade during an attack: `ControllerPlayer::EvadeAction`
//! @0x18015ff90 asks the controller state (`RequestEvadeAction`). Idle, the
//! move states and `UseSkill` (a swing or cast under way, the attack order)
//! take `DefaultRequestEvadeAction` @0x1801639c0: refused for an unusable
//! skill (Skill +176), a held repeat of a non-repeatable skill or a move-to
//! point with no region, else the Evade state is set (leaving UseSkill).
//! `MoveToUseSkill` / `JumpToUseSkill` @0x180167710 refuse it while their
//! skill's `GetWarmUpWasActive` is set (PORT: a [`crate::casting::Leap`]
//! in flight, a [`crate::movement::MoveToUseSkill`] with its warm-up paid).
//! `EvadeAction::Execute` @0x180085350 (refused before anything is spent when
//! the character cannot act) runs `SkillManager::StopCurrentSkill` (PORT:
//! drops the [`crate::casting::Casting`], channel, Shift aim and walk to a
//! skill), `SetCurrentAttackTarget` with the evade's target (none: PORT the
//! player's [`crate::combat::Combatant::target`] is cleared) and then
//! `EvadeSkill`; the cooldown starts only in `Skill_Evade::WarmUpStart`
//! @0x1804f7dc0 (`StartCooldown`), so a refused evade costs nothing; that
//! function also plays the skill's `skillWarmUpSound` (pak +512, stock
//! `spak_dodge`) at the player's coords (`Play3D(coords, 0, true)`). In the
//! Evade state skill and move requests are refused while the evade's warm-up
//! flag is up (`JumpToUseSkill::RequestSkillAction` @0x180168550,
//! `UseSkill::RequestMoveAction` @0x180167650): the dash, from the start to the
//! clip's `StopJump`. The recovery is cancelable — `StopJump` arms the character's
//! `AllowInterrupt` ([`Character::allow_interrupt`]), so a new or queued action
//! cuts it (the action queue, `crate::character`). Its "End" sets Idle
//! (`JumpToUseSkill::HandleEvent` @0x180168ae0).
use crate::casting::Caster;
use crate::character::{CharState, Character};
use crate::movement::MovePath;
use crate::nav::Nav;
use crate::player::Player;
use bevy::prelude::*;

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

/// An evade in progress: until its animation ends (the movement is the
/// [`crate::casting::Leap`] it carries).
#[derive(Component)]
pub struct Evading;

/// Evade (the game sends it for Space, `crate::hotbar` for an evade option):
/// towards the cursor record or the facing way (module docs); `scripted` (a
/// test hook) always the facing way.
#[derive(Message, Clone, Copy, Debug)]
pub struct EvadeRequest {
    pub scripted: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn start_evade(
    mut commands: Commands,
    mut requests: MessageReader<EvadeRequest>,
    data: Res<crate::data::GameData>,
    nav: Res<Nav>,
    (cursor, options): (
        Res<crate::hotbar::WorldCursor>,
        Res<crate::options::GameOptions>,
    ),
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut Caster,
            &mut MovePath,
            &mut Character,
            &mut crate::combat::Combatant,
            Option<&crate::movement::MoveToUseSkill>,
            Has<crate::casting::Leap>,
            &crate::player::ControllerDirection,
        ),
        (
            With<Player>,
            Without<Evading>,
            Without<crate::combat::Dead>,
            Without<crate::movement::Stunned>,
        ),
    >,
) {
    let Some(req) = requests.read().last().copied() else {
        return;
    };
    let Ok((e, tf, mut caster, mut path, mut ch, mut cb, pending, leaping, controller)) =
        players.single_mut()
    else {
        return;
    };
    let scripted = req.scripted;
    if ch.frozen {
        return;
    }
    // A leap in the air or a warmed-up walk to a skill refuses the evade
    // (`MoveToUseSkill` / `JumpToUseSkill::RequestEvadeAction`, module docs).
    if leaping || pending.is_some_and(|m| m.warm_up.is_some()) {
        debug!("evade refused: a warmed-up skill is under way");
        return;
    }
    if let Some(left) = caster.cooldowns.get(EVADE) {
        info!("evade cooling down ({left:.1} s)");
        return;
    }
    let Some(rec) = data.db.get(EVADE) else {
        return;
    };
    let max = rec.f32("waveDistance").unwrap_or(10.0);
    // Towards the cursor record, else the facing axis (+Z, `combat::face`), or the keyboard movement's controller
    // direction while a movement key is held (`crate::player::ControllerDirection`). Keyboard Only (`movementType` 2)
    // never follows the cursor (`HotSlotOptionEvade::Activate` @0x180276840).
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
    // Move type 7: always the full distance (not the cursor's).
    let dist = max;
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return;
    }
    // As far along the line as the ground is walkable.
    let mut to = tf.translation;
    let steps = (dist / 0.25).ceil() as i32;
    for i in 1..=steps {
        let p = tf.translation + dir * (dist * i as f32 / steps as f32);
        match nav.0.ground([p.x, p.y + 1.0, p.z]) {
            Some(g) if (g - to.y).abs() < 1.5 => to = Vec3::new(p.x, g, p.z),
            _ => break,
        }
    }
    if to.distance(tf.translation) < 0.5 {
        return;
    }
    let cd = crate::casting::cooldown_for(
        rec,
        0,
        &[],
        cb.profile.skill_attribute("skillCooldownReduction"),
    );
    caster.cooldowns.start(EVADE.to_owned(), cd, 1);
    // `Skill_Evade::WarmUpStart`: the warm-up sound (`skillWarmUpSound`, stock `spak_dodge`) at the player.
    if let Some(pak) = rec.str("skillWarmUpSound").filter(|s| !s.is_empty()) {
        commands.write_message(crate::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(tf.translation),
        });
    }
    path.points.clear();
    // `EvadeAction::Execute`: the current skill stops (the swing / cast /
    // channel / walk to a skill) and the attack target becomes the evade's
    // (none): the attack order is gone, the controller idles after the evade.
    cb.target = None;
    commands.entity(e).try_remove::<(
        crate::casting::Casting,
        crate::casting::Channel,
        crate::casting::AimPoint,
        crate::movement::MoveToUseSkill,
    )>();
    info!("evade {:.1} m", to.distance(tf.translation));
    // Test hook `GD_SHOT_ON`: the evade is a cast of its skill record (no `cast_requests` path).
    crate::visuals::notify(&mut commands, EVADE);
    // Facing the jump: `EvadeAction::Execute` sets the look-at point to the start plus
    // `World::GetDirection(start, point)` (Engine @0x1802249c0: the difference, not
    // normalised), i.e. the evade's point (`crate::turning`).
    ch.turn.look_at(to);
    ch.state = CharState::Evade;
    // The dash (warm-up) refuses skill / move requests until `StopJump`
    // (`advance_animations` clears it); the evade supersedes the pending slot.
    ch.evade_warmup = true;
    ch.queued = None;
    commands.entity(e).insert((
        Evading,
        crate::casting::Leap::new(vec![to], (to - tf.translation).with_y(0.0).length()),
    ));
}

/// Ends the evade when its animation ends.
fn evading(
    mut commands: Commands,
    mut events: MessageReader<crate::character::AnimCallback>,
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
            .remove::<(Evading, crate::casting::Leap)>();
    }
    // Knocked out of the evade (stun, death, no evade animation): drop it.
    for (e, ch) in &q {
        if ch.state != CharState::Evade {
            commands
                .entity(e)
                .try_remove::<(Evading, crate::casting::Leap)>();
        }
    }
}

/// The evade start: what can cancel or block it orders before `Start`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvadeStage {
    Start,
}
