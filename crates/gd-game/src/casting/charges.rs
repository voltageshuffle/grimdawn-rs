use super::*;

/// `Skill_AttackWeaponCharge` / `Skill_AttackWeaponBlink` warmed up: the
/// caster pushes through characters (`WarmUpStart` sets
/// `Character::SetPathingPushThru`'s flag) and runs at its warmed-up run
/// speed (`crate::combat::charge_move`). Two forms:
/// - a monster's Pursue to the weapon charge it chose ([`ChargePursuit`]):
///   its AI moves it, on the Run clip at the warmed-up rate, and its Attack
///   uses the skill at the end of the path (the engine's warm-up move, see
///   [`begin_pursuit_charge`]);
/// - a dash after the cast (the port's: a use within the skill's range out of
///   weapon reach, a monster's or pet's hit whose target stepped out of
///   reach): the caster paths to the target, the player on the Charge clip
///   (ChargeToUseSkill), an AI on the Run clip, at the warmed-up rate, stops
///   at reach and hits it on arrival.
///
/// The dash gives up once it has run
/// farther from its start than the target is plus `maxDistanceBuffer`
/// (`Skill_AttackWeaponCharge::MaxSkillDistance` @0x1804e4a00, asked by the
/// controller's `MaxSkillDistance`; no check when the buffer is 0); no time
/// limit. A blink hides the caster meanwhile (`Skill_AttackWeaponBlink::
/// WarmUpStart`). It ends without its hit when the target dies (`ControllerPlayerStateChargeToUseSkill::
/// OnUpdate` @0x180167f80 → Idle, and `WarmUpEnd` cancels the skill unless the target is a living character or a
/// breakable: [`warms_up`]) and when the caster dies or is stunned ([`stop_skills`]). The skill's
/// `skillTargetNumber` / `skillTargetAngle` sweep along the way is not done.
#[derive(Component)]
pub struct Charge {
    pub(super) target: Entity,
    /// `characterRunSpeedModifier` % of the skill.
    pub(super) bonus: f32,
    pub(super) skill: String,
    pub(super) level_index: usize,
    pub(super) start: Vec3,
    pub(super) buffer: f32,
    pub(super) path: Vec<Vec3>,
    pub(super) repath: f32,
}

impl Charge {
    /// The warmed-up charge of `skill` (level index `li`, its `characterRunSpeedModifier` % `bonus`) from `at` at
    /// `target`.
    pub(super) fn new(
        rec: &Record,
        skill: &str,
        li: usize,
        bonus: f32,
        at: Vec3,
        target: Entity,
    ) -> Self {
        Charge {
            target,
            bonus,
            skill: skill.to_owned(),
            level_index: li,
            start: at,
            buffer: rec.f32("maxDistanceBuffer").unwrap_or(0.0),
            path: Vec::new(),
            repath: 0.0,
        }
    }
}

/// The [`Charge`] is a monster's Pursue warm-up: its AI moves it (`combat::monster_ai`), its pace's clip at the
/// warmed-up rate (`crate::combat::charge_move` with `bonus` and `cap`), and ends it with the Pursue's move.
#[derive(Component)]
pub struct ChargePursuit {
    pub skill: String,
    /// The skill's `characterRunSpeedModifier` % and `absoluteRunSpeedCapMax` %.
    pub bonus: f32,
    pub cap: f32,
}

/// A monster's Pursue warms up the weapon charge it pursues to use; false (nothing done) for any other skill.
/// `ControllerMonsterStatePursue::OnBegin` @0x18013f6d0 runs `ControllerAI::MoveTo` @0x180124340 with the chosen
/// skill: Run (animation 5) at `GetRunSpeed` for a weapon charge (move type 1; the Charge animation at 1.0 is
/// move type 2's, `Skill_AttackPathCharge`). `MoveToAction::Execute` @0x180082c00 then calls the skill's
/// `WarmUpStart` (vtable 214): `Skill_AttackWeaponCharge::WarmUpStart` @0x1804e4800 sets the push-through flag and
/// the charge start (`MaxSkillDistance`'s, [`ChargeStarts`]), and `Skill_AttackWeapon::WarmUpStart` @0x1804e40b0,
/// when the profile allows a warm-up, plays the warm-up sound and effects ([`WarmUp`]) and starts the cooldown;
/// true sets the warm-up flag and `ForceSpeedUpdate`s the run (`crate::combat::charge_move`). The hit is the Attack
/// state's use of the skill at the path's end (`AttackAction`).
/// The warm-up ends with the move (`MoveToAction::Finish` @0x180082ef0: `WarmUpEnd`): the path's end (Attack), a
/// re-chosen skill's new Pursue, another state. PORT: the cooldown starts at the AI's use as for every monster
/// skill (`monster_skills`).
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

/// Where each charge skill of a character last started (`Skill_AttackWeaponCharge`,
/// `...Blink`, `Skill_AttackPathCharge`): `WarmUpStart` stores the caster's
/// coordinates at skill+1560, which `MaxSkillDistance` measures from.
/// Per skill (record name) as each skill object has its own copy.
/// PORT: a dash's when it starts to move, not at the warm-up start (the
/// caster stands still in between); a monster's Pursue warm-up's at its start.
#[derive(Component, Default)]
pub struct ChargeStarts(pub HashMap<String, Vec3>);

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
            &mut crate::character::Character,
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
        // Re-path towards the moving target now and then.
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
        // `CrowdAgentMoved`: the charger turns toward where it runs (`crate::turning`).
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
