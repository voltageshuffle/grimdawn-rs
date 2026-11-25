use super::*;

/// Spawns a pak's effects on `owner`, replacing its earlier ones for the same skill (`SkillManager::AddCharFxPak`);
/// returns the effect entities.
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
                    gd_game_core::visuals::FxMeshRequest(mesh.to_owned()),
                    Transform::from_translation(at),
                    marker,
                ))
                .id(),
            None => w
                .spawn((
                    gd_game_core::visuals::EffectSpec::record(n.clone()),
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

/// Starts a buff's character effect pak on `owner` under `skill`; `toggled`: from a toggled skill.
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

/// Timed self buffs run out (every [`gd_game_core::skill_buffs::TimedBuffs`] store): their effects go,
/// `skillDeactivatedSound` plays at the caster, the profile rebuilds. A dead holder's are not run down.
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
            Option<(
                &crate::autocast::Autocasts,
                &gd_game_core::combat::Combatant,
            )>,
        ),
        (
            Without<Dead>,
            // Only the holders of a list.
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
    // Tick bypassing change detection: only an expiry is a change, else every `Changed` reader of `Loadout` /
    // `MonsterSkills` / `Pet` reruns each frame.
    for (e, tf, loadout, ms, pet, procs) in &mut q {
        let gone = if let Some(mut l) = loadout {
            // A life monitor's buff waits while life stays low (`Skill_PassiveOnLifeBuffSelf`'s `thresholdDuration`).
            let life_pct = procs.map_or(100.0, |(_, cb)| {
                100.0 * cb.life / cb.profile.sheet.life.max(1.0)
            });
            let held = |s: &str| procs.is_some_and(|(a, _)| a.holds(s, life_pct));
            let gone = l.bypass_change_detection().buffs.tick(dt, held);
            if !gone.is_empty() {
                l.set_changed();
                commands.entity(e).try_insert(ProfileDirty);
            }
            gone
        } else if let Some(mut m) = ms {
            let gone = m.bypass_change_detection().buffs.tick(dt, |_| false);
            if !gone.is_empty() {
                m.mark_profile_dirty();
            }
            gone
        } else if let Some(mut p) = pet {
            let gone = p.bypass_change_detection().buffs_mut().tick(dt, |_| false);
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
