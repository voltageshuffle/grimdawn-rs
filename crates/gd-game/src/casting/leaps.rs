use super::*;

/// Nullification (`Skill_DispelMagic::TargetResult` @0x1804f64e0): foes in
/// the radius take the skill (its debuff) and lose their
/// self buffs (`SkillManager::DispelSkillBuffs` @0x18053ccf0), except skills
/// flagged `notDispelable`; everyone on the caster's side in the radius
/// (the caster too) loses their skill debuffs (`DispelSkillDeBuffs`
/// @0x18053cf00) and, with `dispelDamageOverTime`, their damage over time
/// (`crate::skill_buffs::DispelOn`). `skillActiveDuration` is unused.
#[derive(Message, Clone)]
pub struct Dispel {
    pub caster: Entity,
    pub at: Vec3,
    pub radius: f32,
    pub foes: Vec<Entity>,
    pub dots: bool,
}

pub(super) fn dispels(
    mut events: MessageReader<Dispel>,
    sides: Query<(Entity, &GlobalTransform, crate::combat::VictimSide), Without<Dead>>,
    casters: Query<crate::combat::SideData>,
    mut out: MessageWriter<crate::skill_buffs::DispelOn>,
) {
    use crate::skill_buffs::DispelOn;
    for d in events.read() {
        for f in &d.foes {
            out.write(DispelOn {
                target: *f,
                buffs: true,
                debuffs: false,
                dots: false,
            });
        }
        // The caster's friends (`IsFriend`, the complement of `combat::Side::foe`), not hiding (the
        // projectile's friend query, `FilterInvalidTargets`: `combat::Side::hits`).
        let side = crate::combat::Side::lookup(&casters, d.caster, false);
        for (e, gt, v) in &sides {
            if (e == d.caster || (!v.hiding() && !side.foe(v.side.side_ref())))
                && gt.translation().distance(d.at) <= d.radius + 1.0
            {
                out.write(DispelOn {
                    target: e,
                    buffs: false,
                    debuffs: true,
                    dots: d.dots,
                });
            }
        }
    }
}

/// `Skill_AttackPathCharge` in flight (Vire's Might): runs straight to the
/// point (`waveDistance` at most) and hits
/// every foe within `skillTargetRadius` × `endRadiusMultiplier` there. On
/// the way, every `timeBetweenAttacks` ms (default 100) it hits the foes
/// within `skillTargetRadius` of the caster not hit yet, each once
/// (`Skill_AttackPathCharge::Load` / `Update`). Walls: the move
/// follows a nav path, and fails (the skill lands where the caster stands)
/// when that path is longer than `maxMoveRatio` × the straight distance
/// (`Character::GetMoveToPoint` @0x18005e970 passes `SkillProfile`'s
/// `maxMoveRatio` to `CharacterMovementManager::FindPath`).
/// The run: the skill moves nothing itself (`ActivateNow` @0x1804b8b50,
/// `Update` @0x1804b9160, `WarmUpStart` @0x1804b9940: no move); its move
/// is the controller's move type 2 one (ChargeToUseSkill, a monster's Pursue):
/// the Charge clip at speed factor 1, at the warmed-up run speed when the
/// warm-up started (`skillAllowsWarmUp`), `crate::combat::charge_move`. A set
/// without the Charge slot does not move. PORT: the run follows the cast.
#[derive(Component)]
pub struct PathCharge {
    /// Nav waypoints left to the end point.
    pub(super) to: Vec<Vec3>,
    /// Fixed speed (m/s, a teleport's), or `None`: the Charge clip's run (`crate::combat::charge_move`).
    pub(super) speed: Option<f32>,
    /// The warm-up started (`skillAllowsWarmUp`): the run is at the warmed-up run speed with this
    /// `characterRunSpeedModifier` %.
    pub(super) warm: bool,
    pub(super) bonus: f32,
    pub(super) skill: String,
    pub(super) level_index: usize,
    pub(super) radius: f32,
    pub(super) hostile: bool,
    pub(super) left: f32,
    /// Where the caster stood when it started ([`ChargeStarts`]).
    pub(super) start: Vec3,
    /// Hits along the way (`Skill_AttackPathCharge` only): radius (0 =
    /// none), seconds between, seconds to the next, foes already hit.
    pub(super) path: (f32, f32, f32, Vec<Entity>),
    /// `Skill_AttackPathCharge` only: `secondarySkillDistance` (`Load`,
    /// +0x66C) and the spot the special secondaries last went off (+0x670,
    /// the start: `WarmUpStart`). `Update` @0x1804b9160: each frame
    /// of the run, once the caster is that far (flat) from the spot, the spot
    /// moves to it and `SpecialActivateSecondarySkills` runs there with no
    /// targets (a distance of 0: every frame).
    pub(super) secondaries: Option<(f32, Vec3)>,
    /// The arrival's radius effect, resolved at the cast (`crate::skill_fx::radius_effect`: `fxChanges`
    /// over the level's `radiusEffectName`).
    pub(super) radius_fx: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn path_charges(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    nav: Res<Nav>,
    mut q: Query<(
        Entity,
        &mut Transform,
        Option<&mut crate::character::Character>,
        &mut PathCharge,
    )>,
    all: Query<
        (
            Entity,
            &GlobalTransform,
            &Combatant,
            crate::combat::VictimSide,
        ),
        Without<Dead>,
    >,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let dt = time.delta_secs();
    let cap = data.engine().absolute_run_cap;
    for (e, mut tf, mut ch, mut c) in &mut q {
        let side = crate::combat::Side::lookup(&sides, e, c.hostile);
        c.left -= dt;
        // The Charge clip's run (`crate::combat::charge_move`); without the clip (or a character) the move fails
        // and the skill lands where the caster stands.
        let speed = match (c.speed, ch.as_deref_mut()) {
            (Some(s), _) => s,
            (None, Some(ch)) if !c.to.is_empty() => {
                let warm = c.warm.then_some((c.bonus, cap));
                let (speed, state, rate) = crate::combat::charge_move(ch, CharState::Charge, warm);
                if state == CharState::Idle {
                    c.left = 0.0;
                } else {
                    ch.state = state;
                    ch.move_rate = Some(rate);
                }
                speed
            }
            (None, Some(_)) => 0.0,
            (None, None) => {
                c.left = 0.0;
                0.0
            }
        };
        // Reached waypoints snap the caster onto them.
        while let Some(w) = c.to.first().copied()
            && (w - tf.translation).with_y(0.0).length() <= speed * dt
            && c.left > 0.0
        {
            tf.translation = w;
            c.to.remove(0);
        }
        if let Some(w) = c.to.first().copied()
            && c.left > 0.0
        {
            let d = (w - tf.translation).with_y(0.0);
            let next = tf.translation + d.normalize() * speed * dt;
            match nav.0.ground([next.x, next.y + 0.5, next.z]) {
                Some(g) => tf.translation = Vec3::new(next.x, g, next.z),
                None => c.left = 0.0,
            }
            // `CrowdAgentMoved` (`crate::turning`).
            if let Some(ch) = ch.as_mut() {
                ch.turn.moved(d.normalize_or_zero() * speed);
            }
            c.path.2 -= dt;
            if c.path.0 > 0.0 && c.path.2 <= 0.0 {
                c.path.2 += c.path.1;
                let at = tf.translation;
                let mut struck = Vec::new();
                for (t, gt, cb, v) in &all {
                    if t != e
                        && side.hits(&v)
                        && !c.path.3.contains(&t)
                        && cb.in_radius(&gt.affine(), at, c.path.0)
                    {
                        c.path.3.push(t);
                        hits.write(SkillHit {
                            attacker: e,
                            target: t,
                            skill: c.skill.clone(),
                            level_index: c.level_index,
                            parent: None,
                            scale: 1.0,
                            projectile: false,
                        });
                        struck.push(t);
                    }
                }
                // Path hits resolve as they land (`Skill_AttackPathCharge::TargetResult`).
                activations.write_batch(crate::secondary::Activate::on_hits(
                    e,
                    &c.skill,
                    c.level_index,
                    &None,
                    struck,
                    at,
                ));
            }
            if let Some((every, last)) = c.secondaries.as_mut()
                && (tf.translation - *last).with_y(0.0).length() >= *every
            {
                *last = tf.translation;
                activations.write(crate::secondary::Activate {
                    caster: e,
                    skill: c.skill.clone(),
                    targets: Vec::new(),
                    point: tf.translation,
                    mode: crate::secondary::Mode::Special,
                    level_index: c.level_index,
                    replica: false,
                });
            }
            continue;
        }
        let at = tf.translation;
        let mut struck = Vec::new();
        for (t, gt, cb, v) in &all {
            if c.radius > 0.0 && t != e && side.hits(&v) && cb.in_radius(&gt.affine(), at, c.radius)
            {
                hits.write(SkillHit {
                    attacker: e,
                    target: t,
                    skill: c.skill.clone(),
                    level_index: c.level_index,
                    parent: None,
                    scale: 1.0,
                    projectile: false,
                });
                struck.push(t);
            }
        }
        // The end radius: the ordinary secondaries on what it hit, then the
        // special ones whatever it hit (`Skill_AttackRadiusLeap::TargetResult`;
        // `Skill_AttackPathCharge::Update` on arrival).
        activations.write_batch(crate::secondary::Activate::on_hits(
            e,
            &c.skill,
            c.level_index,
            &None,
            struck,
            at,
        ));
        if let Some(fx) = c.radius_fx.as_deref() {
            spawn_fx(
                &mut commands,
                &data,
                fx,
                Transform::from_translation(at + Vec3::Y * 0.2),
            );
        }
        activations.write(crate::secondary::Activate {
            caster: e,
            skill: c.skill.clone(),
            targets: Vec::new(),
            point: at,
            mode: crate::secondary::Mode::Special,
            level_index: c.level_index,
            replica: false,
        });
        commands.entity(e).try_remove::<PathCharge>();
    }
}

/// Requests the weapon trails of projectile `owner`: one [`crate::visuals::TrailRequest`] between its mesh's
/// `Anchor1` / `Anchor2` and, when there, one between `Anchor3` / `Anchor4` (`trails::anchors_of`; no
/// `Anchor1`, no trail), the anchors scaled by the model's `scale`. The mesh comes from the one mesh loader
/// ([`crate::creature::BodyCache`], emptied with the mounted mod by `session::set_mod_data`), read in a queued
/// command, so it applies with the caller's other commands.
pub(super) fn weapon_trails(
    commands: &mut Commands,
    owner: Entity,
    mesh: &str,
    record: &str,
    width: f32,
    scale: f32,
) {
    let (mesh, record) = (mesh.to_owned(), record.to_owned());
    commands.queue(move |world: &mut World| {
        let msh = world
            .try_resource_scope(|world, mut bodies: Mut<crate::creature::BodyCache>| {
                bodies.get(world.get_resource::<GameData>()?, &mesh)
            })
            .flatten();
        let Some(msh) = msh else { return };
        for second in [false, true] {
            let Some((a, b)) = crate::visuals::anchors_of(&msh, second) else {
                break;
            };
            world.spawn((
                crate::visuals::TrailRequest {
                    owner,
                    record: record.clone(),
                    width,
                    anchors: Some((a * scale, b * scale)),
                },
                Name::new("trail"),
            ));
        }
    });
}

/// Leap classes: `Skill_AttackRadiusLeap` and `Skill_AttackRadiusDisengage`
/// (derived from it).
pub fn is_leap(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackRadiusLeap" | "Skill_AttackRadiusDisengage"
    )
}

/// Where a move skill goes: toward `point`, at most `waveDistance` (default
/// 10), along the nav path, which is given up (no move) when longer than
/// `maxMoveRatio` × the straight distance (`Character::GetMoveToPoint`
/// @0x18005e970, see [`PathCharge`]). A teleport lands on the ground at the
/// end point.
pub(super) fn move_route(
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
    rec: &Record,
    from: Vec3,
    point: Vec3,
    teleport: bool,
) -> Vec<Vec3> {
    let max = rec.f32("waveDistance").unwrap_or(10.0);
    let to = (point - from).with_y(0.0);
    let dist = to.length().min(max);
    let end = from + to.normalize_or_zero() * dist;
    if teleport {
        return vec![
            nav.0
                .ground([end.x, end.y + 0.5, end.z])
                .map_or(end, |g| end.with_y(g)),
        ];
    }
    let mut route: Vec<Vec3> = nav
        .0
        .find_path(
            scratch,
            from.to_array(),
            end.to_array(),
            crate::nav::PATH_BUDGET,
        )
        .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
        .unwrap_or_default();
    let length: f32 = route
        .iter()
        .scan(from, |at, w| {
            let l = (*w - *at).with_y(0.0).length();
            *at = *w;
            Some(l)
        })
        .sum();
    let ratio = rec.f32("maxMoveRatio").unwrap_or(0.0);
    if ratio > 0.0 && length > ratio * dist.max(0.1) {
        route.clear();
    }
    route
}

/// A leap in progress (`Skill_AttackRadiusLeap` / `…Disengage`), per
/// Game.dll / Engine.dll: the skill's action is a `JumpAttackAction`
/// whose `Execute` (@0x1800849f0) calls `Character::JumpToUseSkill`
/// (@0x18006b8e0). That stores the start and the target (both put on the
/// floor) and sets the character's movement modifier to
/// `d / |z|`: `d` the horizontal start→target distance, `z` the jump
/// animation's total root-motion-bone translation (last key − first,
/// `GraphicsAnim::GetBoneCoords`), through the bone's bind matrix, forward
/// component (the frame-timing factor it also computes cancels out). The
/// animation's `StartJump` callback starts a `CharacterMovementManager::
/// MoveTo` to the target, `StopJump` stops it (`JumpAttackAction::
/// AnimationCallback` @0x180084e00). While moving, the crowd agent's speed is
/// the actor's motion — the root-motion velocity of the frame
/// (`AnimChannel::Update` → `Actor::UpdateSelf`) — times that modifier
/// (`Character::CrowdAgentUpdate` @0x180066d60), so the jump covers `d`
/// over the animation's root-motion curve, at its pace. The hit is the
/// animation's hit callback (`fire_casts`).
#[derive(Component)]
pub struct Leap {
    /// Nav waypoints left to the target.
    pub(super) to: Vec<Vec3>,
    /// Horizontal start→target distance (`JumpToUseSkill`).
    pub(super) dist: f32,
    /// The movement modifier, set at `StartJump`; `None` before it.
    pub(super) scale: Option<f32>,
}

impl Leap {
    /// A jump along `to` covering `dist` (also used by the evade, which is
    /// the same `JumpToUseSkill` movement, `evade.rs`).
    pub fn new(to: Vec<Vec3>, dist: f32) -> Self {
        Self {
            to,
            dist,
            scale: None,
        }
    }
}

/// Starts, moves and ends [`Leap`]s. The route is fixed when the cast starts
/// (the action's `Execute`); `StartJump` / `StopJump` / the animation's end
/// switch the move on and off.
#[allow(clippy::too_many_arguments)]
pub(super) fn leaps(
    mut commands: Commands,
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    data: Res<GameData>,
    nav: Res<Nav>,
    mut events: MessageReader<AnimCallback>,
    started: Query<(Entity, &Transform, &Casting), (Changed<Casting>, Without<Leap>)>,
    mut q: Query<(Entity, &mut Transform, &Character, &mut Leap)>,
) {
    for (e, tf, cast) in &started {
        let Some(rec) = data.db.get(&cast.skill).filter(|r| is_leap(&r.class)) else {
            continue;
        };
        // A Disengage (move type 5, set by its constructor) jumps AWAY from
        // its target: `Character::GetMoveToPoint` takes the straight
        // walkable point (`NavManager::FindStraightMovePointOnSlopes`) on the
        // line from the caster directly away from the target, `waveDistance`
        // out; the caster keeps facing the target (`JumpAttackAction::
        // Execute` reverses the facing direction for it). A Leap
        // (type 3) jumps toward the target. PORT: the straight-move query is
        // the port's nav ground walk in 0.25 m steps.
        let goal = if rec.class == "Skill_AttackRadiusDisengage" {
            let away = (tf.translation - cast.point)
                .with_y(0.0)
                .normalize_or_zero();
            let reach = rec.f32("waveDistance").unwrap_or(10.0);
            let mut to = tf.translation;
            for i in 1..=(reach / 0.25).ceil() as i32 {
                let p =
                    tf.translation + away * (reach * i as f32 / (reach / 0.25).ceil()).min(reach);
                match nav.0.ground([p.x, p.y + 1.0, p.z]) {
                    Some(g) if (g - to.y).abs() < 1.5 => to = Vec3::new(p.x, g, p.z),
                    _ => break,
                }
            }
            to
        } else {
            cast.point
        };
        let to = move_route(&nav, &mut scratch, rec, tf.translation, goal, false);
        // The target's distance (the clamped point, as `move_route`).
        let dist = (goal - tf.translation)
            .with_y(0.0)
            .length()
            .min(rec.f32("waveDistance").unwrap_or(10.0));
        debug!(
            "leap: from {} toward {}, {} waypoints",
            tf.translation,
            goal,
            to.len()
        );
        commands.entity(e).try_insert(Leap {
            to,
            dist,
            scale: None,
        });
    }
    for ev in events.read() {
        let Ok((e, tf, ch, mut leap)) = q.get_mut(ev.entity) else {
            continue;
        };
        match ev.name.as_str() {
            "StartJump" if leap.scale.is_none() => {
                // The jump animation's total root motion, forward component.
                let z = ch.channel.current.as_ref().and_then(|c| {
                    let root = gd_sim::anim::root_motion_bone(&ch.body)?;
                    let keys = &c.anim.track(root)?.keys;
                    let (a, b) = (keys.first()?.translation, keys.last()?.translation);
                    Some(
                        gd_sim::anim::root_velocity_object(
                            &ch.body,
                            [0, 1, 2].map(|i| b[i] - a[i]),
                            1.0,
                        )[2]
                        .abs(),
                    )
                });
                leap.scale = Some(z.filter(|z| *z > 1e-4).map_or(0.0, |z| leap.dist / z));
                debug!(
                    "leap: {:.2} m, root motion {z:?} m, modifier {:?}",
                    leap.dist, leap.scale
                );
            }
            "StopJump" | "End" => {
                debug!(
                    "leap: {} at {}, {} waypoints left",
                    ev.name,
                    tf.translation,
                    leap.to.len()
                );
                commands.entity(e).try_remove::<Leap>();
            }
            _ => {}
        }
    }
    let dt = time.delta_secs();
    for (e, mut tf, ch, mut leap) in &mut q {
        if !matches!(ch.state, CharState::Cast | CharState::Evade) {
            commands.entity(e).try_remove::<Leap>();
            continue;
        }
        let Some(scale) = leap.scale else { continue };
        let mut step = ch.root_velocity.with_y(0.0).length() * scale * dt;
        while step > 0.0
            && let Some(w) = leap.to.first().copied()
        {
            let d = (w - tf.translation).with_y(0.0);
            if d.length() <= step {
                tf.translation = w;
                step -= d.length();
                leap.to.remove(0);
                continue;
            }
            let next = tf.translation + d.normalize() * step;
            match nav.0.ground([next.x, next.y + 0.5, next.z]) {
                Some(g) => tf.translation = Vec3::new(next.x, g, next.z),
                None => leap.to.clear(),
            }
            step = 0.0;
        }
        if leap.to.is_empty() {
            debug!("leap: arrived at {}", tf.translation);
            commands.entity(e).try_remove::<Leap>();
        }
    }
}
