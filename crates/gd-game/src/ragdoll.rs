//! Ragdolls: the death throw and the knockdown (physics simulation 3), after Game.dll.
//!
//! The motion: every attack that reaches the damage step of `CombatManager::TakeAttack` (a hit, its
//! multiplier above 0, blocked or not) stores its `PhysicsMotion` on the defender (`Character::SetRagdollData`;
//! not while knocked down), so a death by a later damage over time throws the body as the last hit did
//! ([`RagdollMotion`], [`store_motion`]):
//! - direction, effect and amplification are the attack skill's (`Skill::InitRagDollData`,
//!   [`attack_motion`]): `ragDollDirection` Push (also when unset) from the attack's origin to the defender, Pull
//!   the other way, None nothing; plus `ragDollPush` Left / Right (the attacker's ±x axis) and
//!   `ragDollElevation` Upward / Downward (±3 up), normalised. The effect is `ragDollEffect`, else the
//!   `RagDollData` `effectType` of the clip the attacker plays. The amplification is the level's
//!   `ragDollAmplification`, × (clamp(0.5 − d / r, 0, 0.5) + 0.5) for a skill with a
//!   `projectileExplosionRadius` r. A plain swing is the default weapon attack's; a retaliation or
//!   reflection carries the `ParametersCombat` default.
//! - strength is `physicsStrengthEquation` of the attack's absolute `damage` and the defender's `maxLife`,
//!   clamped to `physicsStrengthMin` .. `physicsStrengthMax` ([`physics_strength`]).
//! - the defender's record overrides: `overrideRagdollBehavior` the effect, a non-zero `overrideRagdollSpeed` the
//!   strength.
//!
//! The death (`DefaultDeathHandler::Execute`, [`death_ragdolls`]): unless it gibs, a character with
//! `ragdollPhysics` (default on), in the render preload frustum ([`RagdollView`]), whose mesh has ragdoll
//! bodies (`Actor::HasRigidBodyData`), without `doLateCrumple`, while fewer than [`MAX_ACTIVE`] ragdolls run
//! (knockdowns count) is thrown ([`Ragdoll`]). A `doLateCrumple` one is thrown at its death clip's `ragDoll`
//! callback instead (Crumple, straight up at strength 1, or the record's overrides; no cap,
//! [`late_crumples`]). A body dying outside the preload frustum is made invisible, the player's excepted.
//!
//! The throw (`Character::PhysicsSetup`) starts every body of the mesh with
//! - TakeHit: the direction × s with s = clamp(max(amplification, 1) × strength, 3, 12), plus 0.33 s up, plus a
//!   random ±j on x and z with j = clamp(s / 2, 3, 6) (nothing without a direction);
//! - Random: ±5 on x and z, 3 up;
//! - Crumple: the character's motion × 0.8, at most 4.5 m/s;
//!
//! and a living one (a knockdown) thrown TakeHit first checks its flight on the path mesh ([`reach`]). The
//! simulation ends when the body rests, is hidden, after [`MAX_SECONDS`] or the record's `physicsTimeLimit`.
//! A living one stays on the path mesh ([`slide_stays`]); both checks use
//! [`crate::nav::straight_move_point`], so a closed door stops the body as it stops a walk. The world is
//! `gd_sim::drop_body`'s (restitution 0). The first ground contact plays `bodyFallSound` when the mesh flags
//! `playFallSound` (no stock mesh does), and a dying body's contacts drop its `ragdollDecal` at most once a
//! second, not on water ([`BodyDecal`]).
//!
//! PORT (a lightweight ragdoll): the bodies move as one: velocity their mass-weighted mean, yaw rate the
//! vertical angular momentum of their spread over their moment, and the pose stays the clip's (the death
//! clip, or the get-up clip's first frame while knocked down), so the clip's callbacks fire as without a
//! ragdoll. The attack's origin is the attacker's position. Crumple takes the root motion, not the limbs'
//! own. The water contact plays no splash.
//!
//! Knockdown (`TakeKnockdownAction::Execute` → `Character::ExecuteKnockdown`): when the knockdown control
//! starts ([`KnockedDown`]) the skill stops, the body lies down and is thrown with its stored motion (no cap,
//! no frustum test, [`knock_downs`]), out of the crowd while down; once the control ended and the throw is
//! over it gets up (`GetUpFaceDown` / `GetUpFaceUp`, [`get_up`]). PORT: the engine reads the face off the
//! root bone when the simulation ends; the port's body stays upright, so the throw decides it
//! ([`falls_face_down`]).
use crate::collision::WorldCollision;
use crate::combat::{AttackMade, CounterHit, Overkill};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::afflictions::AfflictionStage;
use gd_game_core::character::{AnimCallback, CharState, Character, CharacterStage};
use gd_game_core::combat::{CombatSet, Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_sim::drop_body::{Clock, GRAVITY, STEP};
use gd_sim::stats::CombatFormulas;

pub struct RagdollPlugin;

impl Plugin for RagdollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RagdollView>()
            .add_message::<BodyDecal>()
            // Before the afflictions apply their knockdowns (the knocking hit's motion is kept).
            .add_systems(
                Update,
                store_motion
                    .run_if(resource_exists::<gd_game_core::rules::CombatRules>)
                    .in_set(RagdollStage::Motion)
                    .after(CombatSet)
                    .before(AfflictionStage::Tick),
            )
            // The throws lie the body down before the drive.
            .add_systems(
                Update,
                (death_ragdolls, late_crumples, knock_downs, fly, get_up)
                    .chain()
                    .in_set(RagdollStage::Throw)
                    .after(RagdollStage::Motion)
                    .after(AfflictionStage::Tick)
                    .before(CharacterStage::Drive),
            );
    }
}

/// `GameEngine::AllowRagdolls`: a death is thrown only while fewer ragdolls run.
pub const MAX_ACTIVE: usize = 5;
/// `Character::PhysicsSetup`: the 10000 ms countdown that ends the simulation.
pub const MAX_SECONDS: f32 = 10.0;
/// PORT: how far (m) out of the camera's frustum a character still counts as in the preload frustum
/// (`Entity::InRenderPreLoadFrustum`; its size was not decompiled).
const PRELOAD_MARGIN: f32 = 10.0;
/// `Character::PhysicsSetup`: a living TakeHit throw's flight length per (amplification × strength)².
const PREFLIGHT: f32 = 0.040_488_73;
/// PORT: a body on the ground slower than this (m/s) rests.
const REST_SPEED: f32 = 0.3;
/// PORT: the yaw rate (rad/s) the bodies' spread may give the one body at most, and each body's own moment
/// (m² per kg) added to the spread's.
const MAX_SPIN: f32 = 6.0;
const OWN_MOMENT: f32 = 0.05;
/// PORT: the height (m) above its feet at which a thrown body meets walls.
const BODY_MIDDLE: f32 = 0.5;
/// PORT: the lever (m) of a sliding body's yaw friction: the spin slows by friction × gravity / it.
const SPIN_LEVER: f32 = 0.5;
/// Water: no fall sound (the engine plays a splash), no decal.
const WATER: u32 = gd_sim::surface::Surface::Water as u32;

/// What reads a hit's stored motion orders after `Motion`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RagdollStage {
    Motion,
    Throw,
}

/// `PhysicsUtil::GetEffectEnum` (Engine.dll).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RagdollEffect {
    /// 0: the bodies keep the character's motion.
    Crumple,
    /// 1: thrown along the motion's direction.
    TakeHit,
    /// 2: thrown every which way.
    Random,
}

impl RagdollEffect {
    /// "Crumple" 0, "Random" 2, anything else (empty too) 1.
    pub fn parse(s: &str) -> Self {
        match s {
            "Crumple" => RagdollEffect::Crumple,
            "Random" => RagdollEffect::Random,
            _ => RagdollEffect::TakeHit,
        }
    }
}

/// The motion of the last hit on a character or prop (`Character.ragdollMotion`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct RagdollMotion {
    pub effect: RagdollEffect,
    /// The unit direction (zero: none).
    pub dir: Vec3,
    /// `ParametersCombat.physicsStrength`.
    pub strength: f32,
    /// `ParametersCombat.ragDollAmplification`.
    pub amplification: f32,
}

impl Default for RagdollMotion {
    /// `ParametersCombat::ParametersCombat`: TakeHit straight up, strength 1, amplification 1 (retaliation,
    /// reflection).
    fn default() -> Self {
        Self {
            effect: RagdollEffect::TakeHit,
            dir: Vec3::Y,
            strength: 1.0,
            amplification: 1.0,
        }
    }
}

impl RagdollMotion {
    /// `Character::SetRagdollData`: the defender's record's `overrideRagdollBehavior` replaces the
    /// effect, a non-zero `overrideRagdollSpeed` the strength.
    pub fn overridden(mut self, rec: Option<&Record>) -> Self {
        if let Some(r) = rec {
            if let Some(b) = r.str("overrideRagdollBehavior") {
                self.effect = RagdollEffect::parse(b);
            }
            if let Some(s) = r.f32("overrideRagdollSpeed").filter(|s| *s != 0.0) {
                self.strength = s;
            }
        }
        self
    }
}

/// A running ragdoll (physics simulation 3), moved by [`fly`] until it ends.
#[derive(Component, Clone, Debug)]
pub struct Ragdoll {
    pub vel: Vec3,
    /// Yaw rate (rad/s).
    pub spin: f32,
    /// The bodies' mass-weighted `friction` (at most 10).
    pub friction: f32,
    /// `playFallSound`, on the first ground contact ([`Self::fell`]).
    pub fall_sound: bool,
    pub fell: bool,
    /// A knockdown: kept on the path mesh. A death ends it.
    pub alive: bool,
    pub left: f32,
    /// `ragdollDecal`, and the seconds until a contact may drop it again.
    pub decal: Option<String>,
    pub cooldown: f32,
    clock: Clock,
}

/// Knocked down (`Character::ExecuteKnockdown`): on while the knockdown control runs; `up` once the control
/// ended: it gets up when its throw is over.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KnockedDown {
    pub up: bool,
}

/// The camera's frustum planes (`normal · p + d > 0` inside) as the preload frustum, written each frame by
/// the presentation. `None` without a camera: no death is thrown, none is hidden.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct RagdollView(pub Option<[Vec4; 6]>);

impl RagdollView {
    /// Whether a character at `at` is in the preload frustum; `None` without a camera.
    pub fn sees(&self, at: Vec3) -> Option<bool> {
        let planes = self.0.as_ref()?;
        let c = (at + Vec3::Y).extend(1.0);
        Some(planes.iter().all(|p| p.dot(c) + PRELOAD_MARGIN > 0.0))
    }
}

/// A dying body's ground contact drops its `ragdollDecal` (`Character::CollisionCallback`: Y the contact
/// normal, yaw random).
#[derive(Message, Clone, Debug)]
pub struct BodyDecal {
    pub record: String,
    pub at: Vec3,
    pub rotation: Quat,
}

/// `CombatManager::TakeAttack`: `physicsStrengthEquation` of the attack's absolute `damage` and the
/// defender's `maxLife`, clamped (no equation: 0 before the clamp).
pub fn physics_strength(f: &CombatFormulas, damage: f32, max_life: f32) -> f32 {
    let v = f.physics_strength.as_ref().map_or(0.0, |e| {
        e.eval(&|n: &str| {
            if n.eq_ignore_ascii_case("damage") {
                Some(damage)
            } else if n.eq_ignore_ascii_case("maxLife") {
                Some(max_life)
            } else {
                None
            }
        })
    });
    v.max(f.physics_strength_min).min(f.physics_strength_max)
}

/// `Skill::InitRagDollData`: the motion an attack of `skill` (record and level index; `None`: the default
/// weapon attack) from `from` gives a defender at `to`, before its strength. `clip_effect`: the
/// `RagDollData` `effectType` of the attacker's clip.
pub fn attack_motion(
    data: &GameData,
    skill: Option<(&Record, usize)>,
    clip_effect: Option<&str>,
    from: &Transform,
    to: Vec3,
) -> RagdollMotion {
    let (rec, li) = match skill {
        Some((r, li)) => (Some(r), li),
        None => (data.db.get(gd_game_core::casting::DEFAULT_ATTACK), 0),
    };
    let field = |k: &str| rec.and_then(|r| r.str(k));
    let at = from.translation;
    let push = (to - at).normalize_or_zero();
    let mut v = match field("ragDollDirection") {
        None | Some("Push") => push,
        Some("Pull") => -push,
        Some(_) => Vec3::ZERO,
    };
    let side = from.rotation * Vec3::X;
    match field("ragDollPush") {
        Some("Left") => v += side,
        Some("Right") => v -= side,
        _ => {}
    }
    match field("ragDollElevation") {
        Some("Upward") => v.y += 3.0,
        Some("Downward") => v.y -= 3.0,
        _ => {}
    }
    let dir = if v.length() > 1e-4 {
        v.normalize()
    } else {
        Vec3::ZERO
    };
    let effect = RagdollEffect::parse(field("ragDollEffect").or(clip_effect).unwrap_or(""));
    let level = |k: &str| rec.map_or(0.0, |r| gd_game_core::casting::level_f(r, k, li));
    let mut amplification = level("ragDollAmplification");
    let radius = level("projectileExplosionRadius");
    if radius > 0.0 {
        amplification *= (0.5 - at.distance(to) / radius).clamp(0.0, 0.5) + 0.5;
    }
    RagdollMotion {
        effect,
        dir,
        strength: 1.0,
        amplification,
    }
}

/// The floor a body moving `prev → p` lands on, and its normal: the first physics surface below it, from the
/// higher of the two heights so a fast drop does not pass through; else the nav ground, else nothing within
/// 100 m.
pub fn floor(coll: &WorldCollision, nav: &Nav, prev: Vec3, p: Vec3) -> (f32, Vec3) {
    let top = Vec3::new(p.x, prev.y.max(p.y) + 0.2, p.z);
    if let Some((hit, n)) = coll.floor_below(top, 100.0) {
        return (hit.y, n);
    }
    let y = nav.0.ground([p.x, p.y + 0.5, p.z]).unwrap_or(p.y - 100.0);
    (y, Vec3::Y)
}

/// `Character::PhysicsSetup`'s check of a living TakeHit throw: the share of the strength that keeps the
/// flight `offset` from `from` on the path mesh: all of it when the straight move ends within 1 m of the
/// flight's end, else the share of the way it got, none from off the mesh.
fn reach(nav: &Nav, from: Vec3, offset: Vec3) -> f32 {
    if nav.0.locate(from.to_array()).is_none() {
        return 0.0;
    }
    let flat = offset.with_y(0.0);
    let full = flat.length();
    if full <= 1.0 {
        return 1.0;
    }
    let reached = crate::nav::straight_move_point(&nav.0, from, from + flat);
    let got = (reached - from).with_y(0.0).length();
    if full - got > 1.0 { got / full } else { 1.0 }
}

/// `Character::PhysicsUpdate`'s straight move of a living body's slide: whether it gets all the way from the
/// path mesh's ground under `pos` to `next`.
fn slide_stays(nav: &Nav, pos: Vec3, next: Vec3) -> bool {
    let Some(g) = nav.0.ground([pos.x, pos.y + 0.5, pos.z]) else {
        return false;
    };
    let to = next.with_y(g);
    let end = crate::nav::straight_move_point(&nav.0, pos.with_y(g), to);
    (end - to).with_y(0.0).length_squared() < 1e-6
}

/// `Character::PhysicsSetup`: the bodies' start velocities by the effect, made one (PORT). `bodies`: each
/// body's world centre and mass; `drift`: the character's motion (Crumple).
fn initial_velocity(motion: &RagdollMotion, bodies: &[(Vec3, f32)], drift: Vec3) -> (Vec3, f32) {
    let u = |a: f32| (gd_game_core::rng::f32() * 2.0 - 1.0) * a;
    let crumple = (drift * 0.8).clamp_length_max(4.5);
    let s = (motion.amplification.max(1.0) * motion.strength).clamp(3.0, 12.0);
    let j = (s * 0.5).clamp(3.0, 6.0);
    let pushed = motion.dir.length_squared() >= 1e-4;
    let mut vels = Vec::with_capacity(bodies.len());
    let (mut mass, mut centre) = (0.0f32, Vec3::ZERO);
    for &(at, m) in bodies {
        vels.push(match motion.effect {
            RagdollEffect::Crumple => crumple,
            RagdollEffect::TakeHit if pushed => motion.dir * s + Vec3::new(u(j), 0.33 * s, u(j)),
            RagdollEffect::TakeHit => Vec3::ZERO,
            RagdollEffect::Random => Vec3::new(u(5.0), 3.0, u(5.0)),
        });
        mass += m;
        centre += at * m;
    }
    if mass <= 0.0 {
        return (Vec3::ZERO, 0.0);
    }
    centre /= mass;
    let (mut vel, mut turn, mut moment) = (Vec3::ZERO, 0.0f32, mass * OWN_MOMENT);
    for (&(at, m), v) in bodies.iter().zip(&vels) {
        let r = at - centre;
        vel += *v * m;
        turn += m * (r.z * v.x - r.x * v.z);
        moment += m * (r.x * r.x + r.z * r.z);
    }
    (vel / mass, (turn / moment).clamp(-MAX_SPIN, MAX_SPIN))
}

/// PORT: a knocked-down body falls on its face when pushed along its facing, on its back against it,
/// either way without a push.
fn falls_face_down(motion: &RagdollMotion, facing: Vec3, drift: Vec3) -> bool {
    let push = match motion.effect {
        RagdollEffect::TakeHit => motion.dir,
        RagdollEffect::Crumple => drift,
        RagdollEffect::Random => Vec3::ZERO,
    }
    .with_y(0.0);
    if push.length_squared() < 1e-4 {
        gd_game_core::rng::f32() < 0.5
    } else {
        push.dot(facing) > 0.0
    }
}

fn slow(v: &mut Vec3, by: f32) {
    let s = v.with_y(0.0).length();
    if s <= by {
        v.x = 0.0;
        v.z = 0.0;
    } else {
        let k = (s - by) / s;
        v.x *= k;
        v.z *= k;
    }
}

impl Ragdoll {
    /// `Character::PhysicsSetup`; `alive`: a knockdown. `None` when the mesh has no ragdoll bodies. The
    /// bodies lie where this frame's pose puts them.
    pub fn throw(
        data: &GameData,
        nav: &Nav,
        ch: &Character,
        tf: &Transform,
        mut motion: RagdollMotion,
        alive: bool,
    ) -> Option<Self> {
        let posed = ch.posed();
        let mut bodies: Vec<(Vec3, f32)> = Vec::new();
        let (mut mass, mut friction, mut fall_sound) = (0.0f32, 0.0f32, false);
        for b in ch.body.rigid_bodies() {
            let (Some(p), Some(c)) = (ch.body.bone_index(&b.parent), ch.body.bone_index(&b.child))
            else {
                continue;
            };
            let (Some(pp), Some(cp)) = (posed.get(p), posed.get(c)) else {
                continue;
            };
            let ends = Vec3::from(pp.transform_point(b.parent_origin))
                + Vec3::from(cp.transform_point(b.child_origin));
            let m = b.mass.max(1e-3);
            bodies.push((tf.transform_point(ends * 0.5), m));
            mass += m;
            friction += b.friction * m;
            fall_sound |= b.play_fall_sound;
        }
        if bodies.is_empty() {
            return None;
        }
        if alive && motion.effect == RagdollEffect::TakeHit {
            let flight = (motion.amplification * motion.strength).powi(2) * PREFLIGHT;
            motion.strength *= reach(nav, tf.translation, motion.dir * flight);
        }
        let (vel, spin) = initial_velocity(&motion, &bodies, ch.root_velocity);
        let rec = data.db.get(&ch.record);
        let left = rec
            .and_then(|r| r.f32("physicsTimeLimit"))
            .filter(|t| *t > 0.0)
            .map_or(MAX_SECONDS, |t| t.min(MAX_SECONDS));
        Some(Self {
            vel,
            spin,
            friction: (friction / mass).clamp(0.0, 10.0),
            fall_sound,
            fell: false,
            alive,
            left,
            decal: rec.and_then(|r| r.str("ragdollDecal")).map(str::to_owned),
            cooldown: 0.0,
            clock: Clock::default(),
        })
    }

    /// One world step: gravity; a wall (or, alive, the path mesh's edge) stops the slide; the floor stops the
    /// fall and its friction takes the slide, at the contact as much as the fall it stopped. Returns the
    /// floor met (height, normal).
    fn step(&mut self, coll: &WorldCollision, nav: &Nav, pos: &mut Vec3) -> Option<(f32, Vec3)> {
        self.vel.y -= GRAVITY * STEP;
        let mut next = *pos + self.vel * STEP;
        let mid = Vec3::Y * BODY_MIDDLE;
        let slides = (next - *pos).with_y(0.0).length_squared() > 0.0;
        if slides
            && ((self.alive && !slide_stays(nav, *pos, next))
                || coll
                    .segment_hit(*pos + mid, next.with_y(pos.y) + mid)
                    .is_some())
        {
            next.x = pos.x;
            next.z = pos.z;
            self.vel.x = 0.0;
            self.vel.z = 0.0;
        }
        let (ground, normal) = floor(coll, nav, *pos, next);
        let mut contact = None;
        if next.y <= ground {
            next.y = ground;
            if self.vel.y < 0.0 {
                let fall = -self.vel.y;
                self.vel.y = 0.0;
                slow(&mut self.vel, self.friction * fall);
            }
            slow(&mut self.vel, self.friction * GRAVITY * STEP);
            let turn = self.friction * GRAVITY / SPIN_LEVER * STEP;
            self.spin = self.spin.signum() * (self.spin.abs() - turn).max(0.0);
            contact = Some((ground, normal));
        }
        *pos = next;
        contact
    }
}

/// `Character::SetRagdollData` (a physics prop keeps the direction too, `PhysicsDecoration::TakeAttack`).
#[allow(clippy::too_many_arguments)]
fn store_motion(
    mut commands: Commands,
    data: Res<GameData>,
    rules: Res<gd_game_core::rules::CombatRules>,
    mut made: MessageReader<AttackMade>,
    mut counters: MessageReader<CounterHit>,
    mut skill_hits: MessageReader<gd_game_core::casting::SkillHit>,
    attackers: Query<(&Transform, Option<&Character>)>,
    mut targets: Query<(
        &Combatant,
        Option<&Character>,
        Option<&mut RagdollMotion>,
        Has<KnockedDown>,
    )>,
    places: Query<&Transform, With<Combatant>>,
) {
    // The level each skill hit was used at (the hit messages name the skill alone).
    let levels: Vec<(Entity, Entity, String, usize)> = skill_hits
        .read()
        .map(|h| (h.attacker, h.target, h.skill.clone(), h.level_index))
        .collect();
    let mut hits: Vec<(Entity, RagdollMotion, f32)> = Vec::new();
    for m in made.read() {
        if m.miss.is_some() {
            continue;
        }
        let (Ok((atf, ach)), Ok(ttf)) = (attackers.get(m.attacker), places.get(m.target)) else {
            continue;
        };
        let skill = m.skill.as_deref().and_then(|s| {
            let li = levels
                .iter()
                .rev()
                .find(|l| l.0 == m.attacker && l.1 == m.target && l.2 == s)
                .map_or(0, |l| l.3);
            data.db.get(s).map(|r| (r, li))
        });
        let clip = ach
            .and_then(|c| c.channel.current.as_ref())
            .and_then(|c| c.anim.ragdoll())
            .and_then(|b| b.str("effectType"));
        hits.push((
            m.target,
            attack_motion(&data, skill, clip, atf, ttf.translation),
            m.total,
        ));
    }
    for c in counters.read() {
        if !c.hit.missed {
            hits.push((c.target, RagdollMotion::default(), c.hit.total));
        }
    }
    for (target, motion, damage) in hits {
        let Ok((cb, ch, stored, knocked)) = targets.get_mut(target) else {
            continue;
        };
        if knocked {
            continue;
        }
        let motion = RagdollMotion {
            strength: physics_strength(&rules.0.formulas, damage, cb.profile.sheet.life),
            ..motion
        }
        .overridden(ch.and_then(|c| data.db.get(&c.record)));
        match stored {
            Some(mut s) => *s = motion,
            None => {
                commands.entity(target).try_insert(motion);
            }
        }
    }
}

/// `DefaultDeathHandler::Execute`.
#[allow(clippy::too_many_arguments)]
fn death_ragdolls(
    mut commands: Commands,
    data: Res<GameData>,
    view: Res<RagdollView>,
    nav: Res<Nav>,
    options: Res<gd_game_core::options::GameOptions>,
    active: Query<(), With<Ragdoll>>,
    mut dying: Query<
        (
            Entity,
            &Transform,
            &Character,
            Option<&RagdollMotion>,
            Option<&mut Ragdoll>,
            Has<Overkill>,
            Has<Player>,
        ),
        Added<Dead>,
    >,
) {
    let mut running = active.iter().count();
    let gore = options.flag("gore");
    for (e, tf, ch, motion, ragdoll, overkill, is_player) in &mut dying {
        let rec = data.db.get(&ch.record);
        let sees = view.sees(tf.translation);
        if sees == Some(false) && !is_player {
            commands.entity(e).try_insert(gd_game_core::visuals::Hidden);
        }
        if overkill && gore && rec.and_then(|r| r.str("gibEffect")).is_some() {
            continue;
        }
        // Knocked down: it goes on, dead now.
        if let Some(mut r) = ragdoll {
            r.alive = false;
            continue;
        }
        let allowed =
            rec.is_none_or(|r| r.i32("ragdollPhysics") != Some(0) && !r.bool("doLateCrumple"));
        if !allowed || sees != Some(true) || running >= MAX_ACTIVE {
            continue;
        }
        let motion = motion.copied().unwrap_or_default();
        if let Some(r) = Ragdoll::throw(&data, &nav, ch, tf, motion, false) {
            commands.entity(e).try_insert(r);
            running += 1;
        }
    }
}

/// `DefaultDeathHandler::AnimationCallback`.
fn late_crumples(
    mut commands: Commands,
    data: Res<GameData>,
    view: Res<RagdollView>,
    nav: Res<Nav>,
    mut callbacks: MessageReader<AnimCallback>,
    bodies: Query<(&Transform, &Character, Has<Ragdoll>), With<Dead>>,
) {
    for cb in callbacks.read() {
        if !cb.name.eq_ignore_ascii_case("ragDoll") {
            continue;
        }
        let Ok((tf, ch, running)) = bodies.get(cb.entity) else {
            continue;
        };
        let Some(rec) = data.db.get(&ch.record) else {
            continue;
        };
        // `Character::ShouldDoRagDoll`.
        if running
            || !rec.bool("doLateCrumple")
            || rec.i32("ragdollPhysics") == Some(0)
            || view.sees(tf.translation) != Some(true)
        {
            continue;
        }
        let motion = RagdollMotion {
            effect: RagdollEffect::Crumple,
            dir: Vec3::Y,
            strength: 1.0,
            amplification: 0.0,
        }
        .overridden(Some(rec));
        if let Some(r) = Ragdoll::throw(&data, &nav, ch, tf, motion, false) {
            commands.entity(cb.entity).try_insert(r);
        }
    }
}

/// `Character::ExecuteKnockdown`.
fn knock_downs(
    mut commands: Commands,
    data: Res<GameData>,
    nav: Res<Nav>,
    mut down: Query<
        (
            Entity,
            &Transform,
            &mut Character,
            &KnockedDown,
            Option<&RagdollMotion>,
            Has<Ragdoll>,
        ),
        (Added<KnockedDown>, Without<Dead>),
    >,
) {
    for (e, tf, mut ch, k, motion, running) in &mut down {
        if k.up {
            continue;
        }
        let motion = motion.copied().unwrap_or_default();
        let face_down = falls_face_down(
            &motion,
            gd_game_core::character::facing_flat(tf),
            ch.root_velocity,
        );
        ch.knock_down(face_down);
        commands
            .entity(e)
            .try_remove::<gd_game_core::casting::Casting>();
        if running {
            continue;
        }
        if let Some(r) = Ragdoll::throw(&data, &nav, &ch, tf, motion, true) {
            commands.entity(e).try_insert(r);
        }
    }
}

/// The simulation ends at rest, hidden, or out of time.
#[allow(clippy::too_many_arguments)]
fn fly(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    coll: Res<WorldCollision>,
    nav: Res<Nav>,
    mut bodies: Query<(
        Entity,
        &mut Ragdoll,
        &mut Transform,
        &Character,
        Has<gd_game_core::visuals::Hidden>,
    )>,
    mut sounds: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut decals: MessageWriter<BodyDecal>,
) {
    let dt = time.delta_secs();
    for (e, mut r, mut tf, ch, hidden) in &mut bodies {
        let r = &mut *r;
        r.left -= dt;
        r.cooldown = (r.cooldown - dt).max(0.0);
        if hidden || r.left <= 0.0 {
            commands.entity(e).try_remove::<Ragdoll>();
            continue;
        }
        let steps = r.clock.steps(dt);
        if steps == 0 {
            continue;
        }
        let mut pos = tf.translation;
        let mut touched = None;
        for _ in 0..steps {
            if let Some(c) = r.step(&coll, &nav, &mut pos) {
                touched = Some(c);
            }
        }
        tf.translation = pos;
        if r.spin != 0.0 {
            tf.rotation = Quat::from_rotation_y(r.spin * STEP * steps as f32) * tf.rotation;
        }
        let Some((ground, normal)) = touched else {
            continue;
        };
        // `Character::CollisionCallback`.
        let surface = coll.surface_below(pos);
        if r.fall_sound && !r.fell && surface != WATER {
            let at = ch
                .posed_point("Target")
                .map_or(pos + Vec3::Y, |p| tf.transform_point(p));
            let heard = crate::char_sounds::callback(
                &data,
                ch,
                "bodyFallSound",
                None::<crate::char_sounds::Surface>,
                crate::char_sounds::roll,
            );
            if let Some(pak) = heard.sound {
                sounds.write(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
            }
        }
        r.fell = true;
        // Not on water nor surface 9.
        if !r.alive
            && r.cooldown <= 0.0
            && !matches!(surface, 7 | 9)
            && let Some(decal) = &r.decal
        {
            decals.write(BodyDecal {
                record: decal.clone(),
                at: Vec3::new(pos.x, ground, pos.z),
                rotation: Quat::from_rotation_arc(Vec3::Y, normal)
                    * Quat::from_rotation_y(gd_game_core::rng::f32() * std::f32::consts::TAU),
            });
            r.cooldown = 1.0;
        }
        if r.vel.length() < REST_SPEED && r.spin == 0.0 {
            commands.entity(e).try_remove::<Ragdoll>();
        }
    }
}

/// `ControllerMonsterStateKnockedDown::EndKnockdown` → "GettingUp"; another state that took over (a stun,
/// a freeze) keeps it.
fn get_up(
    mut commands: Commands,
    mut down: Query<(Entity, &KnockedDown, &mut Character), (Without<Ragdoll>, Without<Dead>)>,
) {
    for (e, k, mut ch) in &mut down {
        if !k.up {
            continue;
        }
        if ch.state == CharState::KnockedDown {
            ch.get_up();
        }
        commands.entity(e).try_remove::<KnockedDown>();
    }
}
