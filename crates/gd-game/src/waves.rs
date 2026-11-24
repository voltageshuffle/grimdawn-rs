//! `Skill_AttackWave` / `Skill_WPAttack_AttackWave`: a band that travels out
//! from its origin over `waveTime`, widening from `waveStartWidth` to
//! `waveEndWidth`, hitting each foe once as its front passes.
//!
//! Game.dll (`Skill_AttackWave`; `Skill_WPAttack_AttackWave`'s
//! `Update` / `RunTargeting` are the same code):
//! - `Load` (class-level, not per level): `waveTime` × 1000 ms
//!   clamped to [1, 30000], `waveDepth` clamped to [0.01, 20], `fxPakExtents`
//!   / `fxPakSpawnDistance` / `fxPakRandOffsetX` / `fxPakRandOffsetY` /
//!   `fxPakRandAngle` / `fxPakAlternate`, `waveEffectName` (a trail),
//!   `lineEffectName` (an effect), `fxPakName`, `waveStartSound`,
//!   `waveLoopSound`, `waveSource` (`Source` / `Target`), `useTargetDir`,
//!   `launchOffset`, `launchAttachPointName`. The per-level `waveStartWidth`,
//!   `waveEndWidth`, `waveDistance` are `SkillProfile_Modifiers` fields (loader
//!   `SkillProfile_Modifiers::Load`), taken with the learned modifiers'
//!   (`GetSkillModifierData`).
//! - `ActivateNow` places the wave: `waveSource` Target starts
//!   it at the validated target (none: no wave) heading away from the
//!   caster; Source at the caster heading toward the target (else the cast
//!   point) with `useTargetDir`, else along the caster's facing; a zero
//!   direction falls back to the facing. `waveSource` is an exact match
//!   (`ResolveEnum_Source`: only `Target` is 1; `Source;Target` is Source).
//!   With a `launchAttachPointName` a Source wave starts at that point of
//!   the caster's mesh instead (`Actor::GetAttachedCoords`, Engine.dll:
//!   the point at the current pose × the actor's coords; a name
//!   the mesh lacks gives the actor's own coords), put on the floor, and the
//!   `useTargetDir` heading is from there. `Name` ids are case-sensitive
//!   (`Name::Create` hashes the raw bytes; the mesh lookup
//!   compares ids), so the Questing Beast tail spit's `FxForwardGround` misses
//!   its mesh's `FXForwardGround` and starts at the beast; the Dread's
//!   `FX_ForwardMidGround` is 4.5 m ahead. Every point is raised 1.25 m
//!   (`Skill_AttackWave.launchHeight`, ctor), which the floor walks below undo. The
//!   origin moves `launchOffset` along the heading
//!   (`Skill_WPAttack_AttackWave::ActivateNow` has no launch
//!   point or offset and always heads from the caster to the target, else the
//!   cast point). It sends an
//!   attack with no targets, whose `TargetResult` creates the
//!   wave: widths clamped to [0.1, 20], length `waveDistance` clamped to
//!   [1, `GetWaveDistanceLimit` = 50]. A walk from the origin in 1 m steps
//!   along the floor (1 m above it) stops at the first static level object
//!   (`Entity::IsStatic && GetIsPartOfLevel`, the solid decorations of
//!   `collision.rs`; never a door) the step meets: the wave's length ends
//!   there, while the width keeps growing over the full length. The start
//!   sound plays at the origin, the trail / line effect start there.
//! - Effects (`GetEffect`: the last enabled modifier's
//!   `waveEffectOverride` / `lineEffectOverride` / `waveFxPakOverride`, else
//!   the skill's `waveEffectName` / `lineEffectName` / `fxPakName`). The trail
//!   is created as a `TrailEffect` and the line effect as an `EffectEntity`;
//!   a record of another class is dropped (`CreateObjectFromFile` class
//!   check, `Game::CreateObjectOfClass` / `Game::CheckObjectClass`; every stock `lineEffectName` is
//!   an `EffectEntity`, no stock skill sets `waveEffectName`). Both start at
//!   the origin with anchors (the trail's `SetAnchorPoints`, the line
//!   effect's `SetAnchored` + `SetAnchor1/2`) at ∓ start width / 2 along the
//!   coords' x axis.
//! - `Update`, per frame: `current` = min(elapsed / waveTime, 1)
//!   × length (so the wave always takes `waveTime`, even when cut short); the
//!   front is at `current + waveDepth`, as wide as
//!   `start + front / full length × (end − start)`; the trail and the line
//!   effect move to the front (put on the floor) and their anchors to ∓ that
//!   width / 2 across it ([`gd_game_core::visuals::EffectAnchors`]: an anchored
//!   emitter emits along the band between them). Between those two points it
//!   disturbs the grass and water (`World::CreateLineDisturbance`, strength 2,
//!   no splash; [`gd_game_core::visuals::LineDisturbance`]). The fx pak drops along the
//!   front each `fxPakSpawnDistance` (1.5 m before the first), at both ends
//!   of a row `width − fxPakExtents` wide and every `fxPakExtents` from its
//!   right end, each moved by random `fxPakRandOffsetX` sideways and forward,
//!   `fxPakRandOffsetY` down and turned by random `fxPakRandAngle`°, only on
//!   the nav mesh (`fxPakAlternate`: just the two ends turned ±90°).
//!   Targeting ([`slice`], `RunTargeting`) runs when
//!   `current − last ≥ waveDepth` or on the last frame (`current + depth ≥
//!   length`): the band from `last` to `current + depth` (on the last frame
//!   to `length + depth`) is a frustum, its near / far edges as wide as the
//!   wave there, 5 m tall about the floor; every foe in it not yet hit by this
//!   wave is hit (each once per wave). The targets go to `TargetResult` as one
//!   attack: per target the hit, `CreateTargetFx` and the hit sound, then the
//!   secondaries on that list at the wave's origin coordinates. The wave ends
//!   on that last frame (the effects stop, the loop sound fades).
//! - The underground stop, first thing each frame: only when the caster's
//!   region `IsUnderground` (Engine.dll: its level has a
//!   `GridRegion`, which `Level::Load` makes only for grid
//!   terrain; every stock level is a heightfield, `gd_formats::map::level`),
//!   a ray from the origin along the heading ends the wave when the level
//!   geometry it meets within `current` has surface 0
//!   (`World::GetIntersection`, surface 7, no entities), or when any entity
//!   it meets within `current` is a `FixedItemDoor`
//!   (`World::GetAllIntersections`; the door's open state is not checked).
//!   So on every stock level only the creation walk cuts a wave, and a
//!   door, open or closed, never does.
//!
//! PORT: the underground stop is not made (no stock level is underground
//! and the port loads no grid levels); the launch point is the bind pose (the
//! stock points are unparented: pose-free); headings are flat (the engine's
//! `GetDirection` between the raised points can tilt); a foe counts when its
//! world box (`Combatant::body_box`, the engine's bounds) meets the band's
//! planes (the box's projected half extents; the side planes upright); the fx paks' random offsets draw from
//! the gameplay stream (`gd_game_core::rng`; the engine's CRT `rand`); the
//! trail's `SetLowEfficiencyMode` is not modelled.
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::casting::SkillHit;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;

pub struct WavesPlugin;

impl Plugin for WavesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            travel
                .in_set(crate::casting::CastingSet)
                .after(gd_game_core::player::PlayerSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

/// `GetWaveDistanceLimit` (`Skill::GetWaveDistanceLimit`).
const DISTANCE_LIMIT: f32 = 50.0;
/// The targeting frustum's height (`Skill_AttackWave.frustumHeight`, 5.0, ctor).
const HEIGHT: f32 = 5.0;

/// A travelling wave (the gameplay state; its `Transform` is the front's
/// centre, facing along the wave, which the presentation reads).
#[derive(Component, Debug, Clone)]
pub struct Wave {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    /// The weapon-pool skill a pool attack runs under.
    pub parent: Option<(String, usize)>,
    pub hostile: bool,
    /// Where it started (on the floor) and its unit heading (y = 0).
    pub origin: Vec3,
    pub dir: Vec3,
    pub start_width: f32,
    pub end_width: f32,
    /// How far it goes (cut by the scenery) and its full length (widths).
    pub length: f32,
    pub full_length: f32,
    pub depth: f32,
    /// Travel time and time so far (s).
    pub time: f32,
    pub elapsed: f32,
    /// Distance targeted so far (`WaveInfo.lastDistance`).
    pub last: f32,
    /// Foes it has hit (each once).
    pub hit: Vec<Entity>,
    /// Fx pak drops (record, extents, spacing, x / y offsets, angle,
    /// alternate) and the front where the last row dropped (0: none yet).
    fx_pak: Option<(String, FxPak)>,
    last_drop: f32,
    /// The trail and the line effect riding the front.
    riders: Riders,
    /// The trail (`waveEffectName`, a `TrailEffect`) and line effect
    /// (`lineEffectName`, an `EffectEntity`) records, with the modifiers'.
    trail_name: Option<String>,
    line_name: Option<String>,
}

/// The effects riding a wave's front: the line effect (an anchored
/// `EffectEntity`), the trail and its anchor frame (a `TrailRequest` owner,
/// scaled across (x) to the wave's width; the trail's edges are its local
/// ±0.5 x, as `line_fan`'s).
#[derive(Debug, Clone, Default)]
struct Riders {
    line: Option<Entity>,
    trail: Option<(Entity, Entity)>,
}

#[derive(Debug, Clone, Copy)]
struct FxPak {
    extents: f32,
    spacing: f32,
    offset_x: f32,
    offset_y: f32,
    angle: f32,
    alternate: bool,
}

impl Wave {
    /// `current`: how far the wave has gone (before the depth).
    pub fn current(&self) -> f32 {
        (self.elapsed / self.time).clamp(0.0, 1.0) * self.length
    }

    /// The width at `x` metres from the origin.
    pub fn width_at(&self, x: f32) -> f32 {
        self.start_width + x / self.full_length * (self.end_width - self.start_width)
    }

    /// The front: `current + depth`.
    pub fn front(&self) -> f32 {
        self.current() + self.depth
    }

    fn finishing(&self) -> bool {
        self.front() >= self.length
    }
}

/// The band `RunTargeting` checks now, as distances from the origin, or
/// `None` when it does not run. Advances `last`.
fn slice(w: &mut Wave) -> Option<(f32, f32)> {
    let (cur, last) = (w.current(), w.last);
    let fin = w.finishing();
    if cur - last < w.depth && !fin {
        return None;
    }
    let mut len = w.depth;
    if last < cur {
        len += cur - last;
    }
    if last + len > w.length || fin {
        len = w.length - last + w.depth;
        if len < 0.0 {
            return None;
        }
    }
    w.last = last + len;
    Some((last, last + len))
}

/// `Actor::GetAttachedCoords(launchAttachPointName)`: the caster's mesh point
/// named exactly `name` (`Name` ids are case-sensitive hashes) in the world,
/// the caster's own position when the mesh has none.
pub(crate) fn launch_point(
    caster_tf: &Transform,
    body: Option<&gd_formats::msh::Mesh>,
    name: &str,
) -> Vec3 {
    body.filter(|b| b.attach_points().iter().any(|p| p.name == name))
        .and_then(|b| gd_game_core::casting::attach_point_bind(b, name))
        .map_or(caster_tf.translation, |local| {
            caster_tf.transform_point(local)
        })
}

/// The engine's line / trail anchors across the front at `at`, `width`
/// wide: along the coords' x axis (`LookIn` with +Y up: (z, 0, −x) of the
/// heading), anchor 1 at −width / 2.
fn anchors(at: Vec3, dir: Vec3, width: f32) -> gd_game_core::visuals::EffectAnchors {
    let side = Vec3::new(dir.z, 0.0, -dir.x) * width * 0.5;
    gd_game_core::visuals::EffectAnchors(at - side, at + side)
}

/// A frame at `at`, facing `dir` (+Z), `width` across (the trail's anchor).
fn trail_frame(at: Vec3, dir: Vec3, width: f32) -> Transform {
    Transform::from_translation(at)
        .with_rotation(Quat::from_rotation_y(dir.x.atan2(dir.z)))
        .with_scale(Vec3::new(width.max(0.0), 1.0, 1.0))
}

/// `GetEffect(type)`: the last learned modifier's non-empty `over` field,
/// else the skill's `key`.
fn effect_name<'a>(
    rec: &'a Record,
    mods: &'a [(Record, usize)],
    key: &str,
    over: &str,
) -> Option<&'a str> {
    mods.iter()
        .rev()
        .find_map(|(m, _)| m.str(over).filter(|s| !s.is_empty()))
        .or(rec.str(key))
        .filter(|s| !s.is_empty())
}

/// Floor height under `p` (the nav mesh, else the terrain, else `p`'s).
fn floor(nav: &Nav, coll: &crate::collision::WorldCollision, p: Vec3) -> f32 {
    nav.0
        .ground([p.x, p.y + 0.5, p.z])
        .or_else(|| coll.terrain_ground(p))
        .unwrap_or(p.y)
}

/// Where the cast puts its wave (`ActivateNow` / `TargetResult`), or `None`
/// (a `Target`-sourced wave without a valid target).
#[allow(clippy::too_many_arguments)]
pub fn start(
    data: &GameData,
    nav: &Nav,
    coll: &crate::collision::WorldCollision,
    rec: &Record,
    lf: &dyn Fn(&str) -> f32,
    cast: &gd_game_core::casting::Casting,
    caster: Entity,
    caster_tf: &Transform,
    target: Option<Vec3>,
    mods: &[(Record, usize)],
    body: Option<&gd_formats::msh::Mesh>,
) -> Option<Wave> {
    let facing = gd_game_core::character::facing_flat(caster_tf);
    let at = caster_tf.translation;
    let pool = rec.class == "Skill_WPAttack_AttackWave";
    // A `Source` wave starts at the named launch point (put on the floor
    // below), else the caster.
    let src = rec
        .str("launchAttachPointName")
        .filter(|n| !n.is_empty())
        .map_or(at, |n| launch_point(caster_tf, body, n));
    let (mut origin, dir) = if pool {
        // A weapon-pool wave always heads from the caster to the target,
        // else the cast point (`Skill_WPAttack_AttackWave::ActivateNow`).
        (
            at,
            (target.unwrap_or(cast.point) - at)
                .with_y(0.0)
                .normalize_or(facing),
        )
    } else if rec.str("waveSource") == Some("Target") {
        let t = target?;
        (t, (t - at).with_y(0.0).normalize_or(facing))
    } else if rec.bool("useTargetDir") {
        let aim = target.unwrap_or(cast.point);
        (src, (aim - src).with_y(0.0).normalize_or(facing))
    } else {
        (src, facing)
    };
    if !pool {
        origin += dir * rec.f32("launchOffset").unwrap_or(0.0);
    }
    origin.y = floor(nav, coll, origin);
    let full = lf("waveDistance").clamp(1.0, DISTANCE_LIMIT);
    // The walk along the floor, 1 m above it, to the first solid object.
    let mut length = full;
    let mut prev = origin + Vec3::Y;
    let mut step = 0.0;
    while step <= length {
        step += 1.0;
        let mut next = prev + dir;
        next.y = floor(nav, coll, next) + 1.0;
        if let Some((p, kind)) = coll.segment_hit(prev, next)
            && kind != "terrain"
        {
            length = length.min(step - 1.0 + (p - prev).length());
            break;
        }
        prev = next;
    }
    let time = (rec.f32("waveTime").unwrap_or(0.0) * 1000.0).clamp(1.0, 30000.0) / 1000.0;
    let depth = rec.f32("waveDepth").unwrap_or(0.0).clamp(0.01, 20.0);
    // `Skill_WPAttack_AttackWave::GetEffect(2)`: the last learned modifier's
    // `waveFxPakOverride` replaces the skill's `fxPakName`.
    let pak_name = effect_name(rec, mods, "fxPakName", "waveFxPakOverride");
    let fx_pak = pak_name.filter(|s| data.db.get(s).is_some()).map(|s| {
        let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
        (
            s.to_owned(),
            FxPak {
                extents: f("fxPakExtents", 1.0),
                spacing: f("fxPakSpawnDistance", 1.0),
                offset_x: f("fxPakRandOffsetX", 1.0),
                offset_y: f("fxPakRandOffsetY", 1.0),
                angle: f("fxPakRandAngle", 1.0),
                alternate: rec.bool("fxPakAlternate"),
            },
        )
    });
    Some(Wave {
        caster,
        skill: cast.skill.clone(),
        level_index: cast.level_index,
        parent: cast.parent.clone(),
        hostile: cast.hostile,
        origin,
        dir,
        start_width: lf("waveStartWidth").clamp(0.1, 20.0),
        end_width: lf("waveEndWidth").clamp(0.1, 20.0),
        length,
        full_length: full,
        depth,
        time,
        elapsed: 0.0,
        last: 0.0,
        hit: Vec::new(),
        fx_pak,
        last_drop: 0.0,
        riders: Riders::default(),
        trail_name: effect_name(rec, mods, "waveEffectName", "waveEffectOverride")
            .filter(|n| data.db.get(n).is_some_and(|r| r.class == "TrailEffect"))
            .map(str::to_owned),
        line_name: effect_name(rec, mods, "lineEffectName", "lineEffectOverride")
            .filter(|n| data.db.get(n).is_some_and(|r| r.class == "EffectEntity"))
            .map(str::to_owned),
    })
}

/// Spawns the wave with its start sound and riding effects: both start at
/// the origin with anchors as wide as the start width (`TargetResult`).
pub fn spawn(commands: &mut Commands, rec: &Record, mut wave: Wave) {
    let tf = Transform::from_translation(wave.origin).looking_to(-wave.dir, Vec3::Y);
    if let Some(pak) = rec.str("waveStartSound") {
        commands.write_message(gd_game_core::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(wave.origin),
        });
    }
    let (at, dir, w) = (wave.origin, wave.dir, wave.start_width);
    if let Some(t) = wave.trail_name.clone() {
        let owner = commands.spawn(trail_frame(at, dir, w)).id();
        let trail = commands
            .spawn(gd_game_core::visuals::TrailRequest {
                owner,
                record: t,
                width: 0.0,
                anchors: Some((Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0))),
            })
            .id();
        wave.riders.trail = Some((owner, trail));
    }
    if let Some(fx) = wave.line_name.clone() {
        let mut spec = gd_game_core::visuals::EffectSpec::record(fx);
        spec.despawn_when_complete = true;
        wave.riders.line = Some(commands.spawn((spec, tf, anchors(at, dir, w))).id());
    }
    let e = commands.spawn((wave, tf)).id();
    if let Some(pak) = rec.str("waveLoopSound").filter(|s| !s.is_empty()) {
        commands.spawn((
            gd_game_core::visuals::SoundLoop {
                pak: pak.to_owned(),
                on: Some(e),
                at: tf.translation,
                fade: crate::combat_sounds::SHORT_LOOP_FADE,
            },
            ChildOf(e),
        ));
    }
}

/// The fx pak row at `x` metres out (`Update`'s drop), random offsets from the gameplay stream (`gd_game_core::rng`).
fn drop_row(commands: &mut Commands, data: &GameData, nav: &Nav, w: &Wave, x: f32) {
    let Some((pak, f)) = &w.fx_pak else { return };
    let f = *f;
    let centre = w.origin + w.dir * x;
    let side = Vec3::new(w.dir.z, 0.0, -w.dir.x);
    let row = w.width_at(x) - f.extents;
    let (left, right) = (centre - side * row * 0.5, centre + side * row * 0.5);
    let mut points: Vec<(Vec3, f32)> = Vec::new();
    if f.alternate {
        points.push((left, -std::f32::consts::FRAC_PI_2));
        points.push((right, std::f32::consts::FRAC_PI_2));
    } else {
        points.push((left, 0.0));
        points.push((right, 0.0));
        let n = if f.extents > 0.0 {
            (row / f.extents).max(0.0) as u32
        } else {
            0
        };
        for i in 1..=n {
            points.push((right - side * f.extents * i as f32, 0.0));
        }
    }
    let r = |a: f32| (gd_game_core::rng::f32() * 2.0 - 1.0) * a;
    let yaw0 = w.dir.x.atan2(w.dir.z);
    for (p, turn) in points {
        let p = p
            + side * r(f.offset_x)
            + w.dir * r(f.offset_x)
            + Vec3::Y * (r(f.offset_y) - f.offset_y) * 0.5;
        if nav.0.locate([p.x, p.y + 0.5, p.z]).is_none() {
            continue;
        }
        let yaw = yaw0 + turn + r(f.angle).to_radians();
        let tf = Transform::from_translation(p).with_rotation(Quat::from_rotation_y(yaw));
        gd_game_core::visuals::spawn_fx(commands, data, pak, tf);
    }
}

/// Moves the waves, drops their effects and hits what their fronts reach.
#[allow(clippy::too_many_arguments)]
fn travel(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    nav: Res<Nav>,
    coll: Res<crate::collision::WorldCollision>,
    mut waves: Query<(Entity, &mut Wave, &mut Transform)>,
    mut riders: Query<
        (
            &mut Transform,
            Option<&mut gd_game_core::visuals::EffectAnchors>,
        ),
        (Without<Wave>, Without<Combatant>),
    >,
    foes: Query<
        (Entity, &Transform, &Combatant, crate::combat::VictimSide),
        (Without<Dead>, Without<Wave>),
    >,
    casters: Query<crate::combat::SideData, With<Combatant>>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let dt = time.delta_secs();
    for (e, mut w, mut tf) in &mut waves {
        let Ok(sd) = casters.get(w.caster) else {
            stop(&mut commands, e, &w);
            continue;
        };
        let caster_side = sd.side();
        w.elapsed += dt;
        let front = w.front().min(w.length + w.depth);
        let mut at = w.origin + w.dir * front;
        at.y = floor(&nav, &coll, at);
        *tf = Transform::from_translation(at).looking_to(-w.dir, Vec3::Y);
        // The line effect and the trail sit on the front, anchored across it
        // as wide as the wave there.
        let width = w.width_at(front);
        if let Some(l) = w.riders.line
            && let Ok((mut rt, a)) = riders.get_mut(l)
        {
            *rt = *tf;
            if let Some(mut a) = a {
                *a = anchors(at, w.dir, width);
            }
        }
        // `Update`: `World::CreateLineDisturbance` across the front between the same two points, strength
        // 2 (the float constant `Skill_AttackWave::kDisturbanceStrength`), no splash.
        let ends = anchors(at, w.dir, width);
        commands.write_message(gd_game_core::visuals::LineDisturbance {
            from: ends.0,
            to: ends.1,
            strength: 2.0,
            splash: 0,
        });
        if let Some((owner, _)) = w.riders.trail
            && let Ok((mut rt, _)) = riders.get_mut(owner)
        {
            *rt = trail_frame(at, w.dir, width);
        }
        let fin = w.finishing();
        if let Some((_, f)) = w.fx_pak {
            let spacing = if w.last_drop == 0.0 { 1.5 } else { f.spacing }.max(0.1);
            if front - w.last_drop >= spacing || fin {
                let x = if fin { w.last_drop + spacing } else { front };
                drop_row(&mut commands, &data, &nav, &w, x);
                w.last_drop = front;
            }
        }
        if let Some((a, b)) = slice(&mut w) {
            let (wa, wb) = (w.width_at(a) * 0.5, w.width_at(b) * 0.5);
            let side = Vec3::new(w.dir.z, 0.0, -w.dir.x);
            let mut struck = Vec::new();
            for (t, ttf, cb, v) in &foes {
                if !caster_side.hits(&v) || t == w.caster || w.hit.contains(&t) {
                    continue;
                }
                // Cheap reject on the feet position before the body box: no body reaches `BODY_REACH` past it.
                let d0 = ttf.translation - w.origin;
                let f0 = d0.dot(w.dir);
                if f0 < a - BODY_REACH
                    || f0 > b + BODY_REACH
                    || d0.dot(side).abs() > wa.max(wb) + BODY_REACH
                {
                    continue;
                }
                // The foe's world box against the band's planes: its centre within each plane's distance
                // plus the box's projected half extent.
                let bx = cb.body_box(&ttf.compute_affine());
                let (c, h) = (Vec3::from(bx.center), Vec3::from(bx.half));
                let reach = |n: Vec3| n.abs().dot(h);
                let d = c - w.origin;
                let (f, l) = (d.dot(w.dir), d.dot(side).abs());
                if f < a - reach(w.dir) || f > b + reach(w.dir) {
                    continue;
                }
                // The frustum is `HEIGHT` tall about the floor under the band.
                let band = w.origin + w.dir * f;
                if (c.y - floor(&nav, &coll, band.with_y(ttf.translation.y))).abs()
                    > HEIGHT * 0.5 + h.y
                {
                    continue;
                }
                let k = ((f - a) / (b - a).max(1e-4)).clamp(0.0, 1.0);
                if l <= wa + (wb - wa) * k + reach(side) {
                    struck.push(t);
                }
            }
            for &t in &struck {
                w.hit.push(t);
                hits.write(SkillHit {
                    attacker: w.caster,
                    target: t,
                    skill: w.skill.clone(),
                    level_index: w.level_index,
                    parent: w.parent.clone(),
                    scale: 1.0,
                    projectile: false,
                });
            }
            if !struck.is_empty() {
                debug!(
                    "wave {} hits {} at {a:.1}..{b:.1} m",
                    w.skill.rsplit('/').next().unwrap_or(""),
                    struck.len()
                );
            }
            // `TargetResult`: the secondaries on that list, at the origin.
            activations.write_batch(crate::secondary::Activate::on_hits(
                w.caster,
                &w.skill,
                w.level_index,
                &w.parent,
                struck,
                w.origin,
            ));
        }
        if fin {
            stop(&mut commands, e, &w);
        }
    }
}

/// Generous bound on how far a body box reaches horizontally from its entity's position ([`travel`]'s prefilter).
const BODY_REACH: f32 = 20.0;

fn stop(commands: &mut Commands, e: Entity, w: &Wave) {
    if let Some(l) = w.riders.line {
        commands
            .entity(l)
            .try_insert(gd_game_core::visuals::StopEmitting);
    }
    if let Some((owner, trail)) = w.riders.trail {
        commands
            .entity(trail)
            .try_insert(gd_game_core::visuals::StopEmitting);
        commands.entity(owner).try_despawn();
    }
    commands.entity(e).try_despawn();
}
