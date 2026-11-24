//! `Skill_AttackLine` ("^Line Attack"; only the two `base_template skills/skill_attackline.dbr` records): a point
//! that travels from the caster toward the target, hitting what is around it every `hitPeriod`.
//!
//! Game.dll, unexported, a `SkillActivatedSpell`:
//! - ctor `Skill_AttackLine::Skill_AttackLine`: primary and secondary hot slot skill (`Skill.primaryHotSlot` /
//!   `Skill.secondaryHotSlot`), activation mode 0; speed 1,
//!   radius 0, period 100 ms until loaded. `GetValidTarget` is the folded always-valid one (not
//!   `SkillActivatedSpell`'s): the cast goes as aimed (`gd_sim::skills::request_aim`).
//! - `Load` (class-level): speed = `distance` / `time`, life `time` × 1000 (int ms),
//!   `attackRadius`, `hitPeriod` × 1000 (int ms), `loopSound`, `trailEffect` (a `TrailEffect`), `effectEntity` (an
//!   `EffectEntity`), `startSound`.
//! - `ActivateNow`: the line's coordinates are the caster's, looking from them at the target point
//!   (3D; a zero direction keeps the caster's facing), up +Y; it sends an attack with no targets there and runs
//!   `PrimaryActivateSecondarySkills` at the caster (`secondary::primary_activates`).
//! - `TargetResult` with no targets (that attack) pays the mana, starts the cooldown and adds a
//!   line: the trail (low-efficiency, started) and the effect (emitting) at its coordinates, the loop sound there,
//!   tracking the effect; `startSound` plays at the coordinates. With targets (the line's own attacks): per target
//!   `CollectCombatParameters`, the rag doll, `CreateTargetFx` and `PlayHitSound`, the attack; then the
//!   secondaries on that list at the line's coordinates.
//! - `Update`, per line each frame: it moves speed × dt along its heading, its effect and trail with
//!   it. Its hit timer counts down; at 0 it restarts from `hitPeriod`, every target in `attackRadius` of the line
//!   (`GameEngine::GetTargetsInRadius`, the caster's foes) is attacked at the line's coordinates (each again every
//!   period while it stays in reach), and `World::CreatePointDisturbance(coords, attackRadius, 5, falloff, splash
//!   1)` pushes the grass and water there ([`gd_game_core::visuals::PointDisturbance`]). Its life counts down; at its end
//!   the loop sound fades (250 ms), the effect stops emitting and removes itself, the trail is let go.
//!   `StopSkill` ends every line the same way.
//!
//! PORT: the hit timer starts at 0 (the first attack on the first frame; the engine's list node copies that field
//! from uninitialised stack in `TargetResult`); a caster that is gone is `StopSkill`. "In radius":
//! `Combatant::in_radius` (the sphere meets the foe's world box).
use super::*;

/// One travelling line of a `Skill_AttackLine` cast.
#[derive(Component, Clone, Debug)]
pub struct AttackLine {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    pub parent: Option<(String, usize)>,
    pub hostile: bool,
    /// Heading (unit) and speed (m/s).
    pub dir: Vec3,
    pub speed: f32,
    /// Life left, the hit timer and `hitPeriod` (s); `attackRadius`.
    pub life: f32,
    pub timer: f32,
    pub period: f32,
    pub radius: f32,
    /// Its effect entities and trail (moved with it), the loop sound.
    effects: Vec<Entity>,
    trail: Option<Entity>,
    loop_sound: Option<Entity>,
}

/// `ActivateNow` → `TargetResult` (no targets): the line starts at the caster, heading at `point`.
pub(super) fn start_attack_line(
    commands: &mut Commands,
    data: &GameData,
    rec: &Record,
    cast: &Casting,
    caster: Entity,
    tf: &Transform,
) {
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let ms = |k: &str| (f(k) * 1000.0) as i32 as f32 / 1000.0;
    let at = tf.translation;
    let d = cast.point - at;
    let dir = if d.length_squared() < 1e-4 {
        gd_game_core::character::facing(tf)
    } else {
        d.normalize()
    };
    // +Z forward (`character::facing`): `looking_to` aims −Z.
    let frame = Transform::from_translation(at).looking_to(-dir, Vec3::Y);
    let time = f("time");
    let speed = if time != 0.0 {
        f("distance") / time
    } else {
        0.0
    };
    let exists = |k: &str| {
        rec.str(k)
            .filter(|s| !s.is_empty() && data.db.get(s).is_some())
            .map(str::to_owned)
    };
    let line = commands.spawn_empty().id();
    let trail = exists("trailEffect").map(|t| {
        commands
            .spawn(gd_game_core::visuals::TrailRequest {
                owner: line,
                record: t,
                width: 0.0,
                anchors: None,
            })
            .id()
    });
    let effects: Vec<Entity> = exists("effectEntity")
        .and_then(|fx| data.db.get(&fx).map(|r| (fx, r)))
        .map(|(fx, r)| {
            let names: Vec<String> = if r.class == "FxPak" || r.get("particleEffectNames").is_some()
            {
                r.strs_iter("particleEffectNames")
                    .map(|s| s.to_string())
                    .collect()
            } else {
                vec![fx]
            };
            names
                .into_iter()
                .map(|n| {
                    let mut spec = gd_game_core::visuals::EffectSpec::record(n);
                    spec.despawn_when_complete = true;
                    commands.spawn((spec, frame)).id()
                })
                .collect()
        })
        .unwrap_or_default();
    let loop_sound = rec.str("loopSound").filter(|s| !s.is_empty()).map(|pak| {
        commands
            .spawn(gd_game_core::visuals::SoundLoop {
                pak: pak.to_owned(),
                on: Some(line),
                at,
                fade: crate::combat_sounds::SHORT_LOOP_FADE,
            })
            .id()
    });
    if let Some(pak) = rec.str("startSound").filter(|s| !s.is_empty()) {
        commands.write_message(gd_game_core::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(at),
        });
    }
    commands.entity(line).insert((
        AttackLine {
            caster,
            skill: cast.skill.clone(),
            level_index: cast.level_index,
            parent: cast.parent.clone(),
            hostile: cast.hostile,
            dir,
            speed,
            life: ms("time"),
            timer: 0.0,
            period: ms("hitPeriod"),
            radius: f("attackRadius"),
            effects,
            trail,
            loop_sound,
        },
        frame,
        Name::new("attack line"),
    ));
}

/// `Skill_AttackLine::Update`: moves the lines, attacks around them every period, ends them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn attack_lines(
    mut commands: Commands,
    time: Res<Time>,
    mut lines: Query<(Entity, &mut AttackLine, &mut Transform), Without<Combatant>>,
    mut frames: Query<&mut Transform, (Without<Combatant>, Without<AttackLine>)>,
    foes: Query<(Entity, &Transform, &Combatant, crate::combat::VictimSide), Without<Dead>>,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
    mut disturbances: MessageWriter<gd_game_core::visuals::PointDisturbance>,
) {
    let dt = time.delta_secs();
    for (e, mut line, mut tf) in &mut lines {
        if foes.get(line.caster).is_err() {
            // `StopSkill` (PORT: the caster gone or dead).
            end(&mut commands, e, &line);
            continue;
        }
        let line = &mut *line;
        tf.translation += line.dir * line.speed * dt;
        let at = tf.translation;
        for fx in &line.effects {
            if let Ok(mut t) = frames.get_mut(*fx) {
                *t = *tf;
            }
        }
        line.timer -= dt;
        if line.timer <= 0.0 {
            line.timer = line.period;
            let side = crate::combat::Side::lookup(&sides, line.caster, line.hostile);
            let targets: Vec<Entity> = foes
                .iter()
                .filter(|(t, ttf, cb, v)| {
                    *t != line.caster
                        && side.hits(v)
                        && cb.in_radius(&ttf.compute_affine(), at, line.radius)
                })
                .map(|(t, ..)| t)
                .collect();
            for &t in &targets {
                hits.write(SkillHit {
                    attacker: line.caster,
                    target: t,
                    skill: line.skill.clone(),
                    level_index: line.level_index,
                    parent: line.parent.clone(),
                    scale: 1.0,
                    projectile: false,
                });
            }
            activations.write_batch(crate::secondary::Activate::on_hits(
                line.caster,
                &line.skill,
                line.level_index,
                &line.parent,
                targets,
                at,
            ));
            disturbances.write(gd_game_core::visuals::PointDisturbance {
                pos: at,
                radius: line.radius,
                strength: 5.0,
                falloff: true,
                splash: 1,
            });
        }
        line.life -= dt;
        if line.life <= 0.0 {
            end(&mut commands, e, line);
        }
    }
}

/// A line's end: the loop sound fades (its entity goes), the effects stop, the trail is let go.
fn end(commands: &mut Commands, e: Entity, line: &AttackLine) {
    if let Some(s) = line.loop_sound {
        commands.entity(s).try_despawn();
    }
    for fx in &line.effects {
        commands
            .entity(*fx)
            .try_insert(gd_game_core::visuals::StopEmitting);
    }
    if let Some(t) = line.trail {
        commands
            .entity(t)
            .try_insert(gd_game_core::visuals::StopEmitting);
    }
    commands.entity(e).try_despawn();
}
