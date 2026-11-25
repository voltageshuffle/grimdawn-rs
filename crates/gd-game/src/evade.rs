//! Evade (`records/skills/default/defaultevade.dbr`, `Skill_Evade`; Space):
//! the hotbar's evade slot (`HotSlotOptionEvade::Activate`)
//! makes the player dash towards the cursor point (or the facing direction,
//! by an option) and `Character::EvadeSkill` moves them there
//! (`JumpToUseSkill`).
//!
//! `HotSlotOptionEvade::Activate`: the point is the player
//! controller's cursor record (`ControllerPlayer.cursorCoords`, the last world point
//! under the cursor: [`crate::hotbar::WorldCursor`]), not the cursor at the
//! press — an evade clicked on the hotbar dashes towards where the cursor last
//! was over the world. With `evadeFollowCursor` off (or a
//! gamepad, `movementType` = 2) it is the player's coordinates moved along the
//! facing axis, or the keyboard movement's controller direction while a key is
//! held (`ControllerPlayer.moveKeyHeld` / `ControllerPlayer.moveDirection`), by the skill's distance
//! (`NavManager::FindStraightMovePointOnSlopes`).
//! An empty record (no region: no mouse event over the world yet) evades
//! nowhere. PORT: Space goes through the same rule (the key's way to the
//! evade button was not traced). The record gives the distance
//! (`waveDistance`, 10 m), the speed boost (`characterRunSpeedModifier` +250%)
//! and the cooldown (`skillCooldownTime` × (1 − CDR), the shared rule) with its charges (`cooldownCharges` summed
//! over the record and its modifiers, the Evade runes': the shared [`gd_game_core::casting::Cooldowns`] charge rule, from
//! the key and the hotbar alike).
//!
//! The destination is `Character::GetMoveToPoint`
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
//! asks the controller state (`RequestEvadeAction`). Idle, the
//! move states and `UseSkill` (a swing or cast under way, the attack order)
//! take `DefaultRequestEvadeAction`: refused for an unusable
//! skill (`Skill.unusable`), a held repeat of a non-repeatable skill or a move-to
//! point with no region, else the Evade state is set (leaving UseSkill).
//! `MoveToUseSkill` / `JumpToUseSkill` refuse it while their
//! skill's `GetWarmUpWasActive` is set (PORT: a [`gd_game_core::casting::Leap`]
//! in flight, a [`crate::movement::MoveToUseSkill`] with its warm-up paid).
//! `EvadeAction::Execute` (refused before anything is spent when
//! the character cannot act) runs `SkillManager::StopCurrentSkill` (PORT:
//! drops the [`gd_game_core::casting::Casting`], channel, Shift aim and walk to a
//! skill), `SetCurrentAttackTarget` with the evade's target (none: PORT the
//! player's [`gd_game_core::combat::Combatant::target`] is cleared) and then
//! `EvadeSkill`; the cooldown starts only in `Skill_Evade::WarmUpStart`
//! (`StartCooldown`), so a refused evade costs nothing; that
//! function also plays the skill's `skillWarmUpSound` (stock
//! `spak_dodge`) at the player's coords (`Play3D(coords, 0, true)`). In the
//! Evade state skill and move requests are refused while the evade's warm-up
//! flag is up (`ControllerPlayerStateJumpToUseSkill::RequestSkillAction`,
//! `ControllerPlayerStateUseSkill::RequestMoveAction`): the dash, from the start to the
//! clip's `StopJump`. The recovery is cancelable — `StopJump` arms the character's
//! `AllowInterrupt` ([`Character::allow_interrupt`]), so a new or queued action
//! cuts it (the action queue, `crate::character`). Its "End" sets Idle
//! (`ControllerPlayerStateJumpToUseSkill::HandleEvent`).
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

/// An evade in progress: until its animation ends (the movement is the
/// [`gd_game_core::casting::Leap`] it carries).
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
    // A leap in the air or a warmed-up walk to a skill refuses the evade
    // (`ControllerPlayerStateMoveToUseSkill::RequestEvadeAction`, JumpToUseSkill's too; module docs).
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
    // Towards the cursor record, else the facing axis (+Z, `combat::face`), or the keyboard movement's controller
    // direction while a movement key is held (`gd_game_core::player::ControllerDirection`). Keyboard Only (`movementType` 2)
    // never follows the cursor (`HotSlotOptionEvade::Activate`).
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
    // As far along the line as the ground is walkable (a closed door stops it).
    let to = crate::nav::straight_move_point(&nav.0, tf.translation, tf.translation + dir * dist);
    if to.distance(tf.translation) < 0.5 {
        return;
    }
    // The record and its modifiers (the Evade runes): cooldown reduction and `cooldownCharges`.
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
    // `Skill_Evade::WarmUpStart`: the warm-up sound (`skillWarmUpSound`, stock `spak_dodge`) at the player.
    if let Some(pak) = rec.str("skillWarmUpSound").filter(|s| !s.is_empty()) {
        commands.write_message(gd_game_core::visuals::PlaySound {
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
        gd_game_core::casting::Casting,
        gd_game_core::casting::Channel,
        crate::casting::AimPoint,
        crate::movement::MoveToUseSkill,
    )>();
    info!("evade {:.1} m", to.distance(tf.translation));
    // Test hook `GD_SHOT_ON`: the evade is a cast of its skill record (no `cast_requests` path).
    gd_game_core::visuals::notify(&mut commands, EVADE);
    // Facing the jump: `EvadeAction::Execute` sets the look-at point to the start plus
    // `World::GetDirection(start, point)` (Engine: the difference, not
    // normalised), i.e. the evade's point (`crate::turning`).
    ch.turn.look_at(to);
    ch.state = CharState::Evade;
    // The dash (warm-up) refuses skill / move requests until `StopJump`
    // (`advance_animations` clears it); the evade supersedes the pending slot.
    ch.evade_warmup = true;
    ch.queued = None;
    commands.entity(e).try_insert((
        Evading,
        gd_game_core::casting::Leap::new(vec![to], (to - tf.translation).with_y(0.0).length()),
    ));
}

/// Ends the evade when its animation ends.
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
    // Knocked out of the evade (stun, death, no evade animation): drop it.
    for (e, ch) in &q {
        if ch.state != CharState::Evade {
            commands
                .entity(e)
                .try_remove::<(Evading, gd_game_core::casting::Leap)>();
        }
    }
}

/// The evade start: what can cancel or block it orders before `Start`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvadeStage {
    Start,
}
