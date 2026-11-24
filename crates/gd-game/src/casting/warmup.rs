use super::*;

/// The modifiers of `skill` on `caster` for its warm-up ([`WarmUp`]):
/// (record, level index, from gear), the learned ones first (`Skill.modifiers`
/// order), then the gear's (`Skill.itemSkillModifiers`). From [`gd_game_core::weapon_pool::SkillLinks`]
/// (gear modifiers are those not learned, at level index 0).
pub fn warm_up_modifiers(w: &World, caster: Entity, skill: &str) -> Vec<(String, usize, bool)> {
    let Some(links) = w.get::<gd_game_core::weapon_pool::SkillLinks>(caster) else {
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
        commands.write_message(gd_game_core::visuals::PlaySound {
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
            .get::<gd_game_core::weapon_pool::SkillLinks>(caster)
            .map(|l| l.off_mods_of(&skill).to_vec())
            .unwrap_or_default();
        let db = &data.db;
        // `CreateWarmUpFx`: the skill's world pak, then each modifier's.
        let paks: Vec<String> = gd_game_core::skill_fx::level_str(&rec, "warmupFxPakName", li)
            .into_iter()
            .chain(
                mods.iter()
                    .map(|(m, mli, _)| (m, mli))
                    .chain(off.iter().map(|(m, mli)| (m, mli)))
                    .filter_map(|(m, mli)| {
                        db.get(m).and_then(|r| {
                            gd_game_core::skill_fx::level_str(r, "warmupFxPakName", *mli)
                        })
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
                    f.insert(gd_game_core::visuals::StopEmitting);
                }
            }
            w.entity_mut(caster)
                .remove::<gd_game_core::visuals::Hidden>();
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
            gd_game_core::visuals::notify_world(w, n);
            let spec = gd_game_core::visuals::EffectSpec {
                despawn_when_complete: true,
                ..gd_game_core::visuals::EffectSpec::record(n.clone())
            };
            let f = w.spawn((spec, Transform::from_translation(at))).id();
            w.entity_mut(caster).add_child(f);
            fx.push(f);
        }
        let mut e = w.entity_mut(caster);
        e.insert(WarmUp { skill, fx });
        if rec.class == "Skill_AttackWeaponBlink" {
            e.insert(gd_game_core::visuals::Hidden);
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
            commands
                .entity(*f)
                .try_insert(gd_game_core::visuals::StopEmitting);
        }
        commands
            .entity(e)
            .try_remove::<(WarmUp, gd_game_core::visuals::Hidden)>();
    }
}
