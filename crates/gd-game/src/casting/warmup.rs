use super::*;

/// `Skill::GetMoveTypeStateString` is "MoveToUseSkill" for these: the
/// player walks until within the skill's usable distance before casting.
/// `ControllerPlayerState::DefaultRequestSkillAction` @0x180163c30 (mouse
/// movement): a skill walks when its range profile is Melee, it
/// has `forceMovement`, or the `classicCasting` option is on; any other
/// ranged-profile skill casts at once and clamps in `try_cast`. Self-cast
/// kinds have no distance; weapon attacks are the combat system's.
/// `classic` is the `classicCasting` option (Options bool 34, default off: `Options::Options`); `gate` is
/// the engine's extra clause of `DefaultRequestSkillAction` @0x180163c30: with `movementType` 1 and no
/// target object, or `movementType` 2, only weapon charges walk into range (see [`Walk`]).
pub(super) fn walks_into_range(rec: &Record, kind: Kind, walk: Walk) -> bool {
    if !walk.gate {
        return gd_sim::skills::is_weapon_charge(&rec.class);
    }
    (matches!(
        kind,
        Kind::TargetRadius
            | Kind::Projectile
            | Kind::Drop
            | Kind::Wave
            | Kind::LineFan
            | Kind::Line
            | Kind::Storm
            | Kind::Area
            | Kind::Chain
            | Kind::Dispel
    ) || charges_to_range(rec))
        && (matches!(rec.str("distanceProfile").unwrap_or("Melee"), "Melee")
            || rec.bool("forceMovement")
            || walk.classic)
}

/// The walking rule's inputs: the `classicCasting` option and the keyboard-movement gate
/// (`(InputMode != 0 || movementType != 1 || targetId != 0) && movementType != 2`; the port has no gamepad input mode).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walk {
    pub classic: bool,
    pub gate: bool,
}

impl Walk {
    pub fn new(classic: bool, movement_type: i32, has_target: bool) -> Self {
        Walk {
            classic,
            gate: (movement_type != 1 || has_target) && movement_type != 2,
        }
    }
}

/// Move type 2 (`Skill_AttackPathCharge`): `Skill::GetMoveTypeStateString`
/// is "ChargeToUseSkill" (@0x180490fc0), so the out-of-range walk is a
/// charge ([`MoveToUseSkill::charging`]). Weapon charges (Blitz, Shadow
/// Strike: `gd_sim::skills::is_weapon_charge`) have move type 1, but
/// `DefaultRequestSkillAction` @0x180163c30 renames their "MoveToUseSkill"
/// to "ChargeToUseSkill", and being Melee-profile they walk into
/// range like any melee skill; see [`warms_up`].
pub(super) fn charges_to_range(rec: &Record) -> bool {
    rec.class == "Skill_AttackPathCharge" || gd_sim::skills::is_weapon_charge(&rec.class)
}

/// How [`try_cast`] is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Use {
    /// A fresh request (hotbar key, held left click).
    Request,
    /// The end of a `MoveToUseSkill` / ChargeToUseSkill move (no new walk).
    Arrived,
    /// The end of a charge whose warm-up already paid ([`warms_up`]).
    ArrivedWarm,
}

/// The weapon skills' warm-up (`Skill_AttackWeapon::WarmUpStart`
/// @0x1804e40b0, run by `MoveToAction::Execute` as a ChargeToUseSkill move
/// starts): when the skill is enabled and its profile allows a
/// warm-up (`skillAllowsWarmUp`, every player weapon charge), it plays the
/// warm-up FX (`warmUpEffectName`), starts the cooldown, pays
/// `GetManaCost` and flags the warm-up as active (skill + 1548 / 1549);
/// `Skill_AttackWeaponCharge::WarmUpStart` @0x1804e4800 first sets the
/// push-through flag and stores the start (`MaxSkillDistance`), Blink's
/// @0x1804e42d0 also hides the caster. The move then runs at the warm-up
/// run speed (`gd_sim::skills::warmup_run_multiplier`). The move's end
/// (`MoveToAction::Finish`) calls `WarmUpEnd` @0x1804e4850: the FX stops,
/// and unless the target is a living character (or a breakable) the skill
/// is cancelled (`Cancel` @0x1804e49a0). Reaching the target starts
/// UseSkill (`EndOfPathReached` @0x180167bb0) and the attack itself pays
/// nothing more: `Skill_AttackWeaponCharge::ActivateNow` @0x1804e44d0
/// bumps the hit iteration when the warm-up was active, and only the first
/// iteration pays and starts the cooldown (`SkillManager::
/// SkillTargetResult` @0x180535af0). It also runs the learned
/// `Skill_MeleeModifier`s only then (no stock record has one). A charge cut short (target dead:
/// `OnUpdate` @0x180167f80 → Idle; stun; a move order) keeps what it paid.
/// The warm-up's presentation is [`WarmUp`]. PORT: the push-through (the
/// port's player does not collide with characters) is not done.
pub(super) fn warms_up(rec: &Record) -> bool {
    gd_sim::skills::is_weapon_charge(&rec.class) && rec.bool("skillAllowsWarmUp")
}

/// A weapon skill's warm-up running on its caster: from `WarmUpStart` to
/// `WarmUpEnd` (the ChargeToUseSkill move, [`warms_up`]). Game.dll:
/// - `Skill_AttackWeapon::WarmUpStart` @0x1804e40b0 plays `skillWarmUpSound`
///   at the caster (the skill's sound +512, `Skill::PlayWarmUpSound`), then
///   `Skill::CreateWarmUpFx` @0x18049c690 starts the level's
///   `warmupFxPakName` entry (profile +0x8F0, level − 1 clamped to the list)
///   at the caster's coordinates, in the world (it stays where it started),
///   then `Skill::WarmUpEffect(caster, true)` @0x1804967a0: the first time,
///   `Character::AddParticleEffect(warmUpEffectName, warmUpEffectAttachPoint)`
///   (profile +0xA48 / +0xA68; the handle is kept at skill +580), then
///   `StartParticleEffect`. It goes with the caster, at the attach point.
/// - `Skill_AttackWeapon::WarmUpEnd` @0x1804e4850 (warm-up active only):
///   `WarmUpEffect(caster, false)` → `StopParticleEffect` →
///   `EffectEntity::StopEmitting` (`PFxManager::StopParticleEffect`): the
///   effect's live particles finish; the world pak plays on.
/// - `Skill_AttackWeaponBlink::WarmUpStart` @0x1804e42d0: when the warm-up
///   became active, `SetVisibility(false)` on the caster
///   ([`crate::visuals::Hidden`]); its `WarmUpEnd` @0x1804e4850's override
///   shows it again (`SetVisibility(true)`, unconditionally).
/// - Modifiers ([`warm_up_modifiers`]): `CreateWarmUpFx` then calls each
///   `Skill_Modifier` of the skill's learned list (+320, `AddModifier`) and
///   then of its gear list (+344, `AddToItemSkillModifiers`) through vtable
///   +944, the same `Skill::CreateWarmUpFx` on the modifier: its own
///   `warmupFxPakName` at its level starts too (no enabled check, so every
///   one adds to the skill's, an off gear modifier's too:
///   [`crate::weapon_pool::SkillLinks::off_mods`]). `WarmUpEffect` first asks the gear list, then
///   the learned one: the first modifier that `IsSkillEnabled` (vtable +488)
///   and whose own `WarmUpEffect` (+928) returns true, i.e. has a
///   `warmUpEffectName` (with its own `warmUpEffectAttachPoint`), wears its
///   effect instead of the skill's; none: the skill's own. The end goes the
///   same way, so the modifier's effect is the one stopped. The sound is the
///   skill's only (`WarmUpStart` plays only its own +512). Stock: the Vire's
///   Might gear modifiers (`viremight_warmuploop_<type>fx01`, no attach
///   point).
/// - `Skill_AttackPathCharge::WarmUpStart` @0x1804b9940 (Vire's Might) sets
///   its run up (push-through flag, start coordinates, the path active
///   +1641) and ends in `Skill_AttackWeapon::WarmUpStart`, its `WarmUpEnd`
///   @0x1804b9a90 in `WarmUpEffect(caster, false)` while the warm-up was
///   active (+1548).
///
/// The move that carries the warm-up: the player's ChargeToUseSkill
/// ([`MoveToUseSkill::warm_up`]), a monster's or pet's Pursue to its chosen
/// weapon charge ([`begin_pursuit_charge`], `MoveToAction` with the skill, as
/// the engine), or a [`Charge`] / [`PathCharge`] run after the cast. PORT: the
/// run after the cast stands for the move before it (a use within the
/// skill's range out of weapon reach, an AI hit whose target stepped out of
/// reach, every path charge); a fresh effect entity per warm-up (the engine
/// restarts the kept one); an effect without an attach point sits where the
/// port's other unattached character effects do.
#[derive(Component)]
pub struct WarmUp {
    pub skill: String,
    /// The attached `warmUpEffectName` effect(s).
    pub(super) fx: Vec<Entity>,
}

/// The modifiers of `skill` on `caster` for its warm-up ([`WarmUp`]):
/// (record, level index, from gear), the learned ones first (`Skill` +320
/// order), then the gear's (+344). From [`crate::weapon_pool::SkillLinks`]
/// (gear modifiers are those not learned, at level index 0).
pub fn warm_up_modifiers(w: &World, caster: Entity, skill: &str) -> Vec<(String, usize, bool)> {
    let Some(links) = w.get::<crate::weapon_pool::SkillLinks>(caster) else {
        return Vec::new();
    };
    let learned = |m: &str| {
        w.get::<Loadout>(caster).is_some_and(|l| {
            l.skills
                .skills
                .iter()
                .any(|(s, lv)| *lv > 0 && s.eq_ignore_ascii_case(m))
        })
    };
    let (mine, gear): (Vec<_>, Vec<_>) = links
        .mods_of(skill)
        .iter()
        .map(|(m, li)| (m.clone(), *li, !learned(m)))
        .partition(|x| !x.2);
    mine.into_iter().chain(gear).collect()
}

/// `WarmUpStart`'s presentation ([`WarmUp`]) for `skill` at level index `li`.
pub(super) fn start_warm_up(
    commands: &mut Commands,
    data: &GameData,
    caster: Entity,
    rec: &Record,
    skill: &str,
    li: usize,
    at: Transform,
) {
    if let Some(pak) = rec.str("skillWarmUpSound") {
        commands.write_message(crate::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(at.translation),
        });
    }
    let data = data.clone();
    let rec = rec.clone();
    let skill = skill.to_owned();
    commands.queue(move |w: &mut World| {
        let mods = warm_up_modifiers(w, caster, &skill);
        // Gear modifiers that are off (`IsSkillEnabled`) still start their paks, at level index 0.
        let off: Vec<(String, usize)> = w
            .get::<crate::weapon_pool::SkillLinks>(caster)
            .map(|l| l.off_mods_of(&skill).to_vec())
            .unwrap_or_default();
        let db = &data.db;
        // `CreateWarmUpFx`: the skill's world pak, then each modifier's.
        let paks: Vec<String> = crate::skill_fx::level_str(&rec, "warmupFxPakName", li)
            .into_iter()
            .chain(
                mods.iter()
                    .map(|(m, mli, _)| (m, mli))
                    .chain(off.iter().map(|(m, mli)| (m, mli)))
                    .filter_map(|(m, mli)| {
                        db.get(m)
                            .and_then(|r| crate::skill_fx::level_str(r, "warmupFxPakName", *mli))
                    }),
            )
            .collect();
        if !paks.is_empty() {
            let mut c = w.commands();
            for pak in &paks {
                spawn_fx(&mut c, &data, pak, at);
            }
            w.flush();
        }
        // `WarmUpEffect`: the first gear, then learned, modifier with an
        // effect wears it instead of the skill's.
        let owner = mods
            .iter()
            .filter(|m| m.2)
            .chain(mods.iter().filter(|m| !m.2))
            .find_map(|(m, ..)| {
                db.get(m)
                    .filter(|r| r.str("warmUpEffectName").is_some_and(|n| !n.is_empty()))
            });
        let from = owner.unwrap_or(&rec);
        let effect = from
            .str("warmUpEffectName")
            .filter(|n| !n.is_empty())
            .map(|n| {
                (
                    pak_effects(&data, n),
                    from.str("warmUpEffectAttachPoint")
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned),
                )
            });
        if let Some(old) = w
            .get_entity_mut(caster)
            .ok()
            .and_then(|mut e| e.take::<WarmUp>())
        {
            for f in old.fx {
                if let Ok(mut f) = w.get_entity_mut(f) {
                    f.insert(crate::visuals::StopEmitting);
                }
            }
            w.entity_mut(caster).remove::<crate::visuals::Hidden>();
        }
        let Ok(_) = w.get_entity(caster) else { return };
        let mut fx = Vec::new();
        for (n, point) in effect.iter().flat_map(|(effects, point)| {
            effects
                .iter()
                .map(move |(n, p)| (n, p.as_ref().or(point.as_ref())))
        }) {
            let at = point
                .and_then(|p| {
                    w.get::<Character>(caster)
                        .and_then(|c| attach_point_bind(&c.body, p))
                })
                .unwrap_or(Vec3::Y);
            crate::visuals::notify_world(w, n);
            let spec = crate::visuals::EffectSpec {
                despawn_when_complete: true,
                ..crate::visuals::EffectSpec::record(n.clone())
            };
            let f = w.spawn((spec, Transform::from_translation(at))).id();
            w.entity_mut(caster).add_child(f);
            fx.push(f);
        }
        let mut e = w.entity_mut(caster);
        e.insert(WarmUp { skill, fx });
        if rec.class == "Skill_AttackWeaponBlink" {
            e.insert(crate::visuals::Hidden);
        }
    });
}

/// `WarmUpEnd`: the move that carried the warm-up ended (arrival, a cut-short
/// charge, death, another request; [`WarmUp`]).
pub(super) fn end_warm_ups(
    mut commands: Commands,
    q: Query<(
        Entity,
        &WarmUp,
        Option<&crate::movement::MoveToUseSkill>,
        Option<&Charge>,
        Option<&PathCharge>,
    )>,
) {
    for (e, w, mv, charge, path) in &q {
        if mv.is_some_and(|m| m.warm_up.is_some() && m.skill == w.skill)
            || charge.is_some_and(|c| c.skill == w.skill)
            || path.is_some_and(|p| p.skill == w.skill)
        {
            continue;
        }
        for f in &w.fx {
            commands.entity(*f).try_insert(crate::visuals::StopEmitting);
        }
        commands
            .entity(e)
            .try_remove::<(WarmUp, crate::visuals::Hidden)>();
    }
}
