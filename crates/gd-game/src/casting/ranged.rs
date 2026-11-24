use super::*;

pub(super) fn sync_ranged(
    mut commands: Commands,
    data: Res<GameData>,
    inv: Res<gd_game_core::inventory::Inventory>,
    players: Query<(Entity, Option<&RangedWeapon>), With<Player>>,
) {
    let Ok((e, cur)) = players.single() else {
        return;
    };
    // Only mutations wake this (UI reads `Inventory::info` through `&self`); the projectile comparison
    // below is the dedup.
    if !inv.is_changed() && !inv.is_added() {
        return;
    }
    let ranged = inv.equipped[12]
        .as_ref()
        .and_then(|i| ranged_weapon(&data, &i.base_name));
    match ranged {
        Some(r) if cur.is_none_or(|c| c.projectile != r.projectile) => {
            commands.entity(e).insert(r);
        }
        None if cur.is_some() => {
            commands.entity(e).remove::<RangedWeapon>();
        }
        _ => {}
    }
}

/// Weapon swings with a ranged weapon (plain attacks, no skill cast): the
/// hit point fires the weapon's projectile at the target or the aim point.
#[allow(clippy::too_many_arguments)]
pub(super) fn ranged_swings(
    mut commands: Commands,
    mut events: MessageReader<AnimCallback>,
    data: Res<GameData>,
    q: Query<
        (
            &Transform,
            &Combatant,
            &RangedWeapon,
            Option<&AimPoint>,
            Has<MonsterAi>,
            Option<&crate::weapon_pool::PoolPick>,
            Option<&gd_game_core::skill_fx::WeaponEnchantments>,
            Has<Player>,
            Option<&gd_game_core::character::Character>,
        ),
        Without<Casting>,
    >,
    targets: Query<&Transform, Without<Dead>>,
    mut procs: MessageWriter<ProcCast>,
    options: Res<gd_game_core::options::GameOptions>,
    holdings: crate::combat_sounds::Holdings,
) {
    for ev in events.read() {
        if !matches!(ev.name.as_str(), "RightHandHit" | "LeftHandHit") {
            continue;
        }
        let Ok((tf, cb, rw, aim, monster, pick, ench, player, ch)) = q.get(ev.entity) else {
            continue;
        };
        let point = match (aim, cb.target.and_then(|t| targets.get(t).ok())) {
            (Some(a), _) => a.0,
            (None, Some(t)) => t.translation,
            _ => continue,
        };
        commands.entity(ev.entity).try_remove::<AimPoint>();
        // A shot is a weapon-pool attack too: the pool skill picked as the
        // swing started (`weapon_pool::start_swing`) fires instead.
        if let Some(cast) = pick
            .filter(|p| p.is_for(DEFAULT_ATTACK))
            .and_then(|p| p.pick.clone())
            .and_then(|p| crate::weapon_pool::pick_cast(&data, p, None, point, cb.target, monster))
        {
            procs.write(ProcCast {
                caster: ev.entity,
                cast,
            });
            continue;
        }
        let Some(prec) = data.db.get(&gd_game_core::skill_fx::weapon_projectile(
            &data.db,
            &rw.projectile,
            ench,
            player,
            &options,
        )) else {
            continue;
        };
        debug!(
            "ranged shot ({}) at {point:?}",
            if monster { "monster" } else { "player side" }
        );
        let cast = Casting {
            skill: DEFAULT_ATTACK.into(),
            level_index: 0,
            kind: Kind::Projectile,
            point,
            target: cb.target,
            hostile: monster,
            parent: None,
        };
        // The default attack's own added projectile effect (`AddProjectileEffects`, no modifiers).
        let mods = ProjMods {
            effects: data
                .db
                .get(DEFAULT_ATTACK)
                .and_then(|r| gd_game_core::skill_fx::projectile_fx(r, &[]))
                .into_iter()
                .collect(),
            ..default()
        };
        fire_weapon_projectile(
            &mut commands,
            &data,
            prec,
            ev.entity,
            &cast,
            tf,
            proj_speed(cb, 0.0),
            &holdings.of(ev.entity),
            &mods,
            ch,
            if ev.name == "LeftHandHit" {
                "L Hand"
            } else {
                "R Hand"
            },
        );
    }
}

/// `ProjectileBase::GetTotalSpeed`: velocity × (1 + `skillProjectileSpeedModifier`
/// %), the skill's own value plus the character's (gear, passives)
/// (`SkillActivated::CreateProjectile` → `GetTotalSkillAttribute(3)`).
pub fn proj_speed(cb: &Combatant, own: f32) -> f32 {
    (1.0 + (own
        + cb.profile
            .totals
            .get("skillProjectileSpeedModifier")
            .copied()
            .unwrap_or(0.0))
        * 0.01)
        .max(0.0)
}

/// A skill creating a projectile plays its `skillHitSound` at the launch
/// point (`crate::combat_sounds::HitAt::Launch`).
pub(crate) fn skill_launch_sound(commands: &mut Commands, data: &GameData, skill: &str, at: Vec3) {
    if let Some(pak) =
        crate::combat_sounds::spell_hit(data, skill, crate::combat_sounds::HitAt::Launch)
    {
        commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
    }
}

/// One weapon projectile from the hand towards `cast.point`, with the
/// weapon's shot sound (`crate::combat_sounds::shot`) at the shooter and
/// the cast's added effects (`mods`).
#[allow(clippy::too_many_arguments)]
pub(super) fn fire_weapon_projectile(
    commands: &mut Commands,
    data: &GameData,
    prec: &Record,
    attacker: Entity,
    cast: &Casting,
    tf: &Transform,
    speed_factor: f32,
    held: &crate::combat_sounds::Held,
    mods: &ProjMods,
    ch: Option<&Character>,
    hand: &str,
) {
    if let Some(pak) = crate::combat_sounds::shot(data, held) {
        commands.write_message(gd_game_core::visuals::PlaySound {
            pak,
            at: Some(tf.translation),
        });
    }
    let speed = prec.f32("projectileVelocity").unwrap_or(30.0).max(1.0) * speed_factor;
    let dist = prec.f32("projectileDistance").unwrap_or(MAX_RANGE).max(1.0);
    let start = launch_coords(data, tf, ch, prec, hand);
    // The request point itself, no height lift
    // (`SkillActivated::ExecuteRangedAttack`: direction = point − launch
    // coords; the point is the clicked object's `Entity::GetCoords`): the shot descends
    // onto low breakables instead of flying over them.
    let aim = (cast.point - start).normalize_or(gd_game_core::character::facing(tf));
    skill_launch_sound(commands, data, &cast.skill, tf.translation);
    spawn_projectile(
        commands,
        data,
        prec,
        attacker,
        cast,
        start,
        aim * speed,
        dist,
        0.0,
        false,
        mods,
    );
}
