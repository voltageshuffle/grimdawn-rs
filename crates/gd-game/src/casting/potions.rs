use super::*;

/// `Character::TakeBonus` of a `OneShot_*` item's bonus fields (same path as
/// the charge potion's own fields, see the module docs): `life_pts` + `life_pct` % of max
/// life and `mana_pct` % of max energy at once, `life_slow` % of max life
/// into the restore pool.
pub fn take_bonus(
    data: &GameData,
    commands: &mut Commands,
    e: Entity,
    cb: &mut Combatant,
    caster: &mut Caster,
    pools: Option<&PotionOverTime>,
    (life_pts, life_pct, life_slow, mana_pct): (f32, f32, f32, f32),
) {
    let sheet = &cb.profile.sheet;
    let (life_max, energy_max, energy_cap) = (sheet.life, sheet.energy, sheet.usable_energy());
    let h = 1.0 + sheet.heal_increase * 0.01;
    let heal = bonus_life(sheet, life_pts, life_pct);
    let gain = energy_max * mana_pct * 0.01;
    let slow_life = life_max * life_slow * 0.01 * h;
    cb.life = (cb.life + heal).min(life_max);
    caster.energy = (caster.energy + gain).min(energy_cap);
    if slow_life > 0.0 {
        let (_, life_cap, _, _) = resource_behavior(data);
        let old = pools.map_or(0.0, |p| p.life);
        commands.entity(e).try_insert(PotionOverTime {
            life: (old + slow_life).min(life_cap * life_max),
            energy: pools.map_or(0.0, |p| p.energy),
        });
    }
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn potions(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut requests: MessageReader<UsePotion>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &mut Caster,
            &mut Combatant,
            &Transform,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Option<&PotionOverTime>,
        ),
        (With<Player>, Without<Dead>),
    >,
    mut commands: Commands,
    mut activations: MessageWriter<crate::secondary::Activate>,
    time: Res<Time>,
    // The `GD_POTION` schedule left to run and the session's play time it is measured on: session state,
    // restarted with each session.
    mut scripted: Local<Option<(f32, Vec<(f32, bool)>)>>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *scripted = None;
    }
    let request = requests.read().last().copied();
    let Ok((e, mut caster, mut cb, tf, links, pools)) = q.single_mut() else {
        return;
    };
    let st = scripted.get_or_insert_with(|| (0.0, hooks.gameplay.potion.clone()));
    st.0 += time.delta_secs();
    let now = st.0;
    let test =
        st.1.iter()
            .position(|(t, _)| now >= *t)
            .map(|i| st.1.remove(i).1);
    let skill = if request == Some(UsePotion::Health) || test == Some(true) {
        HEALTH_POTION
    } else if request == Some(UsePotion::Energy) || test == Some(false) {
        ENERGY_POTION
    } else {
        return;
    };
    let Some(rec) = data.db.get(skill) else {
        return;
    };
    let mods: Vec<(&Record, usize)> = links
        .map(|l| {
            l.mods_of(skill)
                .iter()
                .filter_map(|(m, li)| data.db.get(m).map(|r| (r, *li)))
                .collect()
        })
        .unwrap_or_default();
    let base = |k: &str| level_f(rec, k, 0);
    let from_mods = |k: &str| mods.iter().map(|(r, li)| level_f(r, k, *li)).sum::<f32>();
    let total = |k: &str| base(k) + from_mods(k);
    let max_charges = total("cooldownCharges").max(1.0) as u32;
    if !caster.cooldowns.usable(skill) {
        info!(
            "potion cooling down ({:.1} s)",
            caster.cooldowns.get(skill).copied().unwrap_or(0.0)
        );
        // `HotSlotOptionPotion::Activate`: an undrinkable potion says so while its bar is not full
        // (energy: max − reserved; `error_vox`).
        let sheet = &cb.profile.sheet;
        let (id, short) = if skill == HEALTH_POTION {
            ("1", cb.life.max(0.0) < sheet.life)
        } else {
            ("2", caster.energy.max(0.0) < sheet.usable_energy())
        };
        if short {
            commands.write_message(crate::error_vox::ErrorVox::new(
                crate::error_vox::VoxKind::ItemCooldown,
                id,
            ));
        }
        return;
    }
    let potency = 1.0 + (total("modifierPotency") + total("potionTransfusion")) * 0.01;
    let sheet = &cb.profile.sheet;
    let (life_max, energy_max, energy_cap) = (sheet.life, sheet.energy, sheet.usable_energy());
    let h = 1.0 + sheet.heal_increase * 0.01;
    // The potion's own fields (none with transfusion; infusion moves the
    // instant parts into the pool), then each modifier's × potency.
    let infusion = total("potionInfusion");
    let own = |k: &str| {
        if total("potionTransfusion") > 0.0 {
            0.0
        } else {
            base(k)
        }
    };
    let (mut life_pct, mut life_flat, mut life_slow, mut mana_flat, mut mana_pct, mut mana_slow) = (
        own("skillLifePercent"),
        own("skillLifeBonus"),
        own("skillLifePercentSlow"),
        own("skillManaBonus"),
        own("skillManaPercent"),
        own("skillManaPercentSlow"),
    );
    let mut mana_flat_slow = 0.0;
    if infusion > 0.0 {
        let m = 1.0 + infusion * 0.01;
        (life_flat, life_slow, mana_flat_slow, mana_slow) = (
            life_flat * m,
            life_slow + life_pct * m,
            mana_flat * m,
            mana_slow + mana_pct * m,
        );
        (life_pct, mana_flat, mana_pct) = (0.0, 0.0, 0.0);
    }
    let from = |k: &str| from_mods(k) * potency;
    let (life_pct, life_flat, life_slow) = (
        life_pct + from("skillLifePercent"),
        life_flat + from("skillLifeBonus"),
        life_slow + from("skillLifePercentSlow"),
    );
    let (mana_flat, mana_pct, mana_slow) = (
        mana_flat + from("skillManaBonus"),
        mana_pct + from("skillManaPercent"),
        mana_slow + from("skillManaPercentSlow"),
    );
    let heal = bonus_life(sheet, 0.0, life_pct);
    let gain = mana_flat + energy_max * mana_pct * 0.01;
    let slow_life = (life_flat + life_max * life_slow * 0.01) * h;
    let slow_energy = mana_flat_slow + energy_max * mana_slow * 0.01;
    cb.life = (cb.life + heal).min(life_max);
    caster.energy = (caster.energy + gain).min(energy_cap);
    if slow_life > 0.0 || slow_energy > 0.0 {
        let (_, life_cap, _, mana_cap) = resource_behavior(&data);
        let (old_life, old_energy) = pools.map_or((0.0, 0.0), |p| (p.life, p.energy));
        commands.entity(e).try_insert(PotionOverTime {
            life: (old_life + slow_life).min(life_cap * life_max),
            energy: (old_energy + slow_energy).min(mana_cap * energy_cap),
        });
    }
    let absorb = total("absorbShieldPercentHealth") * 0.01 * life_max * potency;
    if absorb > 0.0 {
        // The default potions list no qualifying types: every type
        // (`Skill_ChargePotion::ModifyDamage`).
        let shield = Shield {
            skill: skill.to_owned(),
            absorb: gd_sim::combat::Absorb {
                pct: 0.0,
                flat: absorb,
                pool: true,
                types: Vec::new(),
            },
            left: None,
            kind: ShieldKind::Potion(absorb),
        };
        commands.queue(move |w: &mut World| {
            if let Some(mut s) = w.get_mut::<Shields>(e) {
                s.add(shield);
            }
        });
    }
    info!(
        "{}: +{heal:.0} life, +{gain:.0} energy, pooled +{slow_life:.0} / +{slow_energy:.0}, shield {absorb:.0} ({} modifiers)",
        skill.rsplit('/').next().unwrap_or(""),
        mods.len()
    );
    caster
        .cooldowns
        .start(skill.to_owned(), base("skillCooldownTime"), max_charges);
    activations.write(crate::secondary::Activate {
        caster: e,
        skill: skill.to_owned(),
        targets: Vec::new(),
        point: tf.translation,
        mode: crate::secondary::Mode::Normal,
        level_index: 0,
        replica: false,
    });
    if let Some(fx) = rec.str("skillActivatedAuraName") {
        spawn_fx(
            &mut commands,
            &data,
            fx,
            Transform::from_translation(tf.translation + Vec3::Y),
        );
    }
}

pub(super) fn potion_over_time(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<(Entity, &mut PotionOverTime, &mut Combatant, &mut Caster), Without<Dead>>,
) {
    let dt = time.delta_secs();
    let (life_feed, _, mana_feed, _) = resource_behavior(&data);
    for (e, mut p, mut cb, mut c) in &mut q {
        let sheet = &cb.profile.sheet;
        let usable = sheet.usable_energy();
        let life = (sheet.life / life_feed * dt).min(p.life);
        let energy = (usable / mana_feed * dt).min(p.energy);
        p.life -= life;
        p.energy -= energy;
        cb.life = (cb.life + life).min(cb.profile.sheet.life);
        c.energy = (c.energy + energy).min(usable);
        if p.life <= 0.0 && p.energy <= 0.0 {
            commands.entity(e).try_remove::<PotionOverTime>();
        }
    }
}

/// `GD_FX_TEST` (hooks.rs).
pub(super) fn fx_test(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    time: Res<Time>,
    mut t: Local<f32>,
    data: Res<GameData>,
    players: Query<&Transform, With<Player>>,
) {
    let Some(rec) = hooks.gameplay.fx_test.as_deref() else {
        return;
    };
    let Ok(p) = players.single() else { return };
    *t -= time.delta_secs();
    if *t <= 0.0 {
        *t = 2.0;
        spawn_fx(
            &mut commands,
            &data,
            rec,
            Transform::from_translation(p.translation + Vec3::new(2.0, 1.0, 0.0)),
        );
    }
}
