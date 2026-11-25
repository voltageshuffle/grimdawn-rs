use super::*;

/// The [`Charge`] is a monster's Pursue warm-up: its AI moves it, its pace's clip at the warmed-up rate (`bonus`,
/// `cap`), and ends it with the Pursue's move.
#[derive(Component)]
pub struct ChargePursuit {
    pub skill: String,
    /// The skill's `characterRunSpeedModifier` % and `absoluteRunSpeedCapMax` %.
    pub bonus: f32,
    pub cap: f32,
}

/// A monster's Pursue warms up the weapon charge it pursues to use; false (nothing done) for any other skill.
/// `ControllerMonsterStatePursue::OnBegin` runs `ControllerAI::MoveTo` with the skill: Run at `GetRunSpeed` for a
/// weapon charge. `MoveToAction::Execute` then calls `Skill_AttackWeaponCharge::WarmUpStart` (push-through flag,
/// the charge start) and `Skill_AttackWeapon::WarmUpStart` (warm-up sound and effects, cooldown start), and
/// `ForceSpeedUpdate`s the run. The hit is the Attack state's use of the skill at the path's end; the warm-up ends
/// with the move (`MoveToAction::Finish`). PORT: the cooldown starts at the AI's use as for every monster skill.
pub fn begin_pursuit_charge(
    commands: &mut Commands,
    data: &GameData,
    caster: Entity,
    target: Entity,
    skill: &str,
    li: usize,
    tf: &Transform,
) -> bool {
    let Some(rec) = data.db.get(skill) else {
        return false;
    };
    if !warms_up(rec) {
        return false;
    }
    start_warm_up(commands, data, caster, rec, skill, li, *tf);
    let bonus = level_f(rec, "characterRunSpeedModifier", li);
    let charge = Charge::new(rec, skill, li, bonus, tf.translation, target);
    let pursuit = ChargePursuit {
        skill: skill.to_owned(),
        bonus: charge.bonus,
        cap: data.engine().absolute_run_cap,
    };
    commands.entity(caster).try_insert((charge, pursuit));
    true
}

pub(super) fn note_charge_starts(
    mut commands: Commands,
    mut q: Query<
        (
            Entity,
            Option<&mut ChargeStarts>,
            Option<&Charge>,
            Option<&PathCharge>,
        ),
        Or<(Added<Charge>, Added<PathCharge>)>,
    >,
) {
    for (e, starts, charge, path) in &mut q {
        let set = |starts: &mut ChargeStarts| {
            for (skill, at) in charge
                .map(|c| (&c.skill, c.start))
                .into_iter()
                .chain(path.map(|p| (&p.skill, p.start)))
            {
                starts.0.insert(skill.clone(), at);
            }
        };
        match starts {
            Some(mut s) => set(&mut s),
            None => {
                let mut s = ChargeStarts::default();
                set(&mut s);
                commands.entity(e).try_insert(s);
            }
        }
    }
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub(super) fn charges(
    mut commands: Commands,
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    nav: Res<Nav>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &Combatant,
            &mut gd_game_core::character::Character,
            Has<Player>,
            &mut Charge,
        ),
        Without<ChargePursuit>,
    >,
    targets: Query<(&GlobalTransform, &Combatant), (Without<Charge>, Without<Dead>)>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let dt = time.delta_secs();
    let cap = data.engine().absolute_run_cap;
    for (e, mut tf, cb, mut ch, player, mut c) in &mut q {
        let Ok((tgt, tcb)) = targets.get(c.target) else {
            commands.entity(e).try_remove::<Charge>();
            continue;
        };
        let goal = tgt.translation();
        let to = (goal - tf.translation).with_y(0.0);
        if to.length() <= cb.reach(tcb) + 0.5 {
            hits.write(SkillHit {
                attacker: e,
                target: c.target,
                skill: c.skill.clone(),
                level_index: c.level_index,
                parent: None,
                scale: 1.0,
                projectile: false,
            });
            // The arrival's hit resolves the attack (`Skill_AttackWeapon::TargetResult`).
            activations.write_batch(crate::secondary::Activate::on_hits(
                e,
                &c.skill,
                c.level_index,
                &None,
                vec![c.target],
                tf.translation,
            ));
            commands.entity(e).try_remove::<Charge>();
            continue;
        }
        if c.buffer > 0.0
            && (tf.translation - c.start).with_y(0.0).length()
                > (goal - c.start).with_y(0.0).length() + c.buffer
        {
            commands.entity(e).try_remove::<Charge>();
            continue;
        }
        c.repath -= dt;
        if c.repath <= 0.0 || c.path.is_empty() {
            c.repath = 0.25;
            c.path = nav
                .0
                .find_path(
                    &mut scratch,
                    tf.translation.to_array(),
                    goal.to_array(),
                    crate::nav::PATH_BUDGET,
                )
                .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
                .unwrap_or_default();
            if c.path.is_empty() {
                commands.entity(e).try_remove::<Charge>();
                continue;
            }
        }
        // The warmed-up move clip carries it: the player's the Charge clip, an AI's the Run clip.
        let clip = if player {
            CharState::Charge
        } else {
            CharState::Run
        };
        let (speed, state, rate) = crate::combat::charge_move(&ch, clip, Some((c.bonus, cap)));
        if state == CharState::Idle {
            commands.entity(e).try_remove::<Charge>();
            continue;
        }
        ch.state = state;
        ch.move_rate = Some(rate);
        // It stops at reach: the next frame's arrival hits (no step past it into the target).
        let mut budget = (speed * dt).min(to.length() - cb.reach(tcb));
        while budget > 0.0 && !c.path.is_empty() {
            let d = (c.path[0] - tf.translation).with_y(0.0);
            if d.length() <= budget {
                budget -= d.length();
                tf.translation = c.path.remove(0);
            } else {
                let next = tf.translation + d.normalize() * budget;
                tf.translation = Vec3::new(
                    next.x,
                    nav.0
                        .ground([next.x, next.y + 0.5, next.z])
                        .unwrap_or(next.y),
                    next.z,
                );
                budget = 0.0;
            }
        }
        // `CrowdAgentMoved`: the charger turns toward where it runs.
        if let Some(d) = c
            .path
            .first()
            .map(|p| (*p - tf.translation).with_y(0.0))
            .filter(|d| d.length() > 1e-3)
        {
            ch.turn.moved(d.normalize() * speed);
        } else {
            ch.turn.moved(to.normalize_or_zero() * speed);
        }
    }
}
