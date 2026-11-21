use super::*;

/// Bind-pose position of a named mesh attach point in the character's space
/// (the parent bone's bind transform × the point's frame).
pub fn attach_point_bind(body: &gd_formats::msh::Mesh, name: &str) -> Option<Vec3> {
    let p = body.attach_point(name)?;
    let parent = p
        .parent_bone
        .map_or(gd_formats::msh::Coords::IDENTITY, |i| body.bones[i].abs);
    Some(Vec3::from(parent.mul(&p.coords).origin))
}

/// A pak's effects with their attach points (the record itself when it is
/// an effect, not a pak).
pub(crate) fn pak_effects(data: &GameData, pak: &str) -> Vec<(String, Option<String>)> {
    let Some(rec) = data.db.get(pak) else {
        return Vec::new();
    };
    if rec.get("particleEffectNames").is_none() {
        return vec![(pak.to_owned(), None)];
    }
    let points = rec.strs("particleEffectAttachPoints");
    let mut out: Vec<(String, Option<String>)> = rec
        .strs_iter("particleEffectNames")
        .enumerate()
        .map(|(i, n)| {
            (
                n.to_string(),
                points
                    .get(i)
                    .map(|p| p.to_string())
                    .filter(|p| !p.is_empty()),
            )
        })
        .collect();
    // `CharFxPak::LoadMesh`: its `meshEffectNames` (`FxMesh` records, `"mesh:"` marks them here) ride the character
    // too, at the record's `attachPoint`, else its origin (`PFxManager::AddMeshEffect` @0x1803caf80:
    // `Attach(effect, identity, attachPoint)`).
    out.extend(rec.strs_iter("meshEffectNames").map(|m| {
        let point = data
            .db
            .get(m)
            .and_then(|r| r.str("attachPoint").map(str::to_owned))
            .unwrap_or_default();
        (format!("{MESH_PREFIX}{m}"), Some(point))
    }));
    out
}

/// Marks an `FxMesh` record among a pak's effect names ([`pak_effects`]).
pub(crate) const MESH_PREFIX: &str = "mesh:";

/// Spawns a pak's effects on `owner` (replacing its earlier ones for the
/// same skill: a re-applied buff restarts them, `SkillManager::AddCharFxPak`
/// @0x18053d980); returns the effect entities.
pub(crate) fn spawn_pak_on(
    w: &mut World,
    effects: &[(String, Option<String>)],
    owner: Entity,
    skill: &str,
    toggled: bool,
) -> Vec<Entity> {
    let old: Vec<Entity> = w
        .query::<(Entity, &BuffFx)>()
        .iter(w)
        .filter(|(_, b)| b.owner == owner && b.skill == skill)
        .map(|(e, _)| e)
        .collect();
    for e in old {
        w.despawn(e);
    }
    let mut out = Vec::new();
    for (n, point) in effects {
        // An empty point is the character's origin (an aura loop); no point: chest height.
        let origin = point.as_deref() == Some("");
        let named = point.clone().filter(|p| {
            !p.is_empty()
                && w.get::<Character>(owner)
                    .is_some_and(|c| attach_point_bind(&c.body, p).is_some())
        });
        let at = if origin {
            Vec3::ZERO
        } else {
            named
                .as_deref()
                .and_then(|p| {
                    w.get::<Character>(owner)
                        .and_then(|c| attach_point_bind(&c.body, p))
                })
                .unwrap_or(Vec3::Y)
        };
        let marker = BuffFx {
            owner,
            skill: skill.to_owned(),
            toggled,
            point: named,
        };
        let fx = match n.strip_prefix(MESH_PREFIX) {
            Some(mesh) => w
                .spawn((
                    crate::visuals::FxMeshRequest(mesh.to_owned()),
                    Transform::from_translation(at),
                    marker,
                ))
                .id(),
            None => w
                .spawn((
                    crate::visuals::EffectSpec::record(n.clone()),
                    Transform::from_translation(at),
                    marker,
                ))
                .id(),
        };
        if let Ok(mut o) = w.get_entity_mut(owner) {
            o.add_child(fx);
        }
        out.push(fx);
    }
    out
}

/// Starts a buff's character effect pak on `owner` under `skill`;
/// `toggled`: it comes from a toggled skill ([`BuffFx::toggled`]).
pub(crate) fn attach_buff_fx(
    commands: &mut Commands,
    data: &GameData,
    pak: &str,
    owner: Entity,
    skill: &str,
    toggled: bool,
) {
    let effects = pak_effects(data, pak);
    let sounds = char_pak_sounds(data, std::slice::from_ref(&pak.to_owned()));
    let skill = skill.to_owned();
    commands.queue(move |w: &mut World| {
        let fx = spawn_pak_on(w, &effects, owner, &skill, toggled);
        start_char_pak_sounds(w, &sounds, owner, &fx);
    });
}

/// The `CharFxPak` records' `startSound`, `activeSound` and `stopSound`.
pub(crate) fn char_pak_sounds(data: &GameData, paks: &[String]) -> Vec<[Option<String>; 3]> {
    let field = |r: &Record, k: &str| r.str(k).filter(|s| !s.is_empty()).map(str::to_owned);
    paks.iter()
        .filter_map(|p| data.db.get(p))
        .map(|r| {
            [
                field(r, "startSound"),
                field(r, "activeSound"),
                field(r, "stopSound"),
            ]
        })
        .collect()
}

/// `CharFxPak::Start` @0x1800ed580 and what `Stop` @0x1800ed6f0 does when the
/// pak's first effect goes (`crate::combat_sounds` module doc): the pak's
/// sounds for a character effect started on `owner`. The loop and the stop
/// sound hang from that effect entity.
pub(crate) fn start_char_pak_sounds(
    w: &mut World,
    sounds: &[[Option<String>; 3]],
    owner: Entity,
    fx: &[Entity],
) {
    let at = w
        .get::<Transform>(owner)
        .map_or(Vec3::ZERO, |t| t.translation);
    for [start, active, stop] in sounds {
        if let Some(pak) = start.clone() {
            w.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
        }
        let Some(first) = fx.first().copied() else {
            continue;
        };
        if let Some(pak) = active.clone() {
            w.spawn((
                crate::visuals::SoundLoop {
                    pak,
                    on: Some(owner),
                    at,
                    fade: 0.01,
                },
                ChildOf(first),
            ));
        }
        if let Some(pak) = stop.clone() {
            w.entity_mut(first)
                .insert(crate::visuals::StopSound { pak, on: owner });
        }
    }
}

/// Timed self buffs run out — the one [`crate::skill_buffs::TimedBuffs`]
/// store of every character (the player's on its `Loadout`, a monster's on
/// its `MonsterSkills`, a pet's on its `Pet`): their effects go,
/// `skillDeactivatedSound` plays at the caster
/// (`crate::skill_buffs::timed_buffs_ended`), the profile rebuilds. A dead
/// holder's are not run down: death stopped them (`crate::skill_buffs`'
/// `stop_buffs_on_death`).
pub(super) fn expire_buffs(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &Transform,
            Option<&mut Loadout>,
            Option<&mut crate::monster_skills::MonsterSkills>,
            Option<&mut crate::pets::Pet>,
        ),
        (
            Without<Dead>,
            // Only the holders of a list: not every entity with a transform.
            Or<(
                With<Loadout>,
                With<crate::monster_skills::MonsterSkills>,
                With<crate::pets::Pet>,
            )>,
        ),
    >,
    fx: Query<(Entity, &BuffFx)>,
) {
    let dt = time.delta_secs();
    // The countdown is not a change: only an expiry is (it rebuilds the profile). Ticking through
    // `DerefMut` would flag `Loadout` / `MonsterSkills` / `Pet` changed every frame, and their `Changed`
    // readers (pet profile refresh, toggles, auras, sheet DPS…) would rerun every frame.
    for (e, tf, loadout, ms, pet) in &mut q {
        let gone = if let Some(mut l) = loadout {
            let gone = l.bypass_change_detection().buffs.tick(dt);
            if !gone.is_empty() {
                l.set_changed();
                commands.entity(e).try_insert(ProfileDirty);
            }
            gone
        } else if let Some(mut m) = ms {
            let gone = m.bypass_change_detection().buffs.tick(dt);
            if !gone.is_empty() {
                m.mark_profile_dirty();
            }
            gone
        } else if let Some(mut p) = pet {
            let gone = p.bypass_change_detection().buffs_mut().tick(dt);
            if !gone.is_empty() {
                p.mark_profile_dirty();
            }
            gone
        } else {
            continue;
        };
        if gone.is_empty() {
            continue;
        }
        crate::skill_buffs::timed_buffs_ended(&mut commands, &data, &fx, e, tf.translation, &gone);
    }
}

/// `Skill_BuffSelfImmobilize` (Blade Barrier) roots its caster by refreshing a short hold while it runs.
/// Game.dll `Skill_BuffSelfImmobilize::Update` @0x1804eaee0: after `Skill_BuffSelfDuration::Update`,
/// while the skill is active (+220) the frame's ms come off its timer (+1592: 0 from the constructor
/// @0x1804eae60, never reset by an activation); at ≤ 0 the timer is re-armed with +1596 (100 ms) and the holder
/// gets `DurationDamageManager::AddFixedDamage(47 = immobilize, 1.0, min(+1600 = 0.25 s, time left +1540 ms ×
/// 0.001))` (a fixed entry: the duration is replaced, `crate::afflictions::Afflictions::hold`), then
/// `EndAttack`. So the hold lasts while the buff runs and ends within 0.25 s of its stop, whatever stops it
/// (expiry, a dispel, death). Inserted at the first cast ([`fire_casts`]); the running buff is the caster's buff
/// list entry ([`crate::player::Loadout::buffs`], `MonsterSkills::buffs`; no stock pet record has the class).
#[derive(Component, Debug)]
pub struct ImmobilizeRefresh {
    /// The skill under its buff list name.
    pub(super) skill: String,
    /// +1592, seconds.
    pub(super) timer: f32,
}

/// [`ImmobilizeRefresh`]'s re-arm (+1596 = 100 ms, the constructor).
const IMMOBILIZE_PERIOD_SECS: f32 = 0.1;

/// [`ImmobilizeRefresh`]'s hold cap (+1600 = 0.25 s, the constructor).
const IMMOBILIZE_HOLD_SECS: f32 = 0.25;

/// [`ImmobilizeRefresh`]: each running Blade Barrier refreshes its hold.
pub(super) fn immobilize_holds(
    time: Res<Time>,
    mut q: Query<
        (
            &mut ImmobilizeRefresh,
            &mut crate::afflictions::Afflictions,
            Option<&Loadout>,
            Option<&crate::monster_skills::MonsterSkills>,
        ),
        Without<Dead>,
    >,
) {
    let dt = time.delta_secs();
    for (mut r, mut a, l, ms) in &mut q {
        let left = l
            .map(|l| &l.buffs.list)
            .into_iter()
            .chain(ms.map(|m| &m.buffs.list))
            .flatten()
            .find(|b| b.0 == r.skill)
            .map(|b| b.2)
            .filter(|l| *l > 0.0);
        let Some(left) = left else { continue };
        r.timer -= dt;
        if r.timer <= 0.0 {
            r.timer = IMMOBILIZE_PERIOD_SECS;
            a.hold(
                gd_sim::attributes::CombatType::Immobilize,
                left.min(IMMOBILIZE_HOLD_SECS),
            );
        }
    }
}
