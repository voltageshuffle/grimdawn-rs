//! What gameplay asks the presentation to show or play. These are plain data
//! components and messages: gameplay spawns them (an effect at a spot, a
//! lightning bolt, a projectile's model, a trail) and grimdawn dresses
//! them with meshes, particles and sounds. Headless, they are inert data.
use bevy::prelude::*;

/// What to play. Inserted by [`spawn_fx`] / [`EffectSpec::record`]; the
/// game resolves it into a running particle effect.
#[derive(Component, Clone, Debug)]
pub struct EffectSpec {
    pub source: EffectSource,
    /// Remove the entity once every emitter has finished (`EffectEntity` self-delete).
    pub despawn_when_complete: bool,
    /// Particle scale (`EffectEntity::SetEffectScale`).
    pub scale: f32,
    /// `DisableLooping`: looping emitters end after one cycle.
    pub no_loop: bool,
}

/// `EffectEntity::SetAnchored(true)` with `SetAnchor1` / `SetAnchor2`: the
/// effect's two world anchor points, kept current by gameplay.
/// Engine.dll `EffectEntity::UpdateSelf`: the stored anchors go to every
/// emitter each update, which then emits along the band between them
/// (`grimdawn::fx::sim`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EffectAnchors(pub Vec3, pub Vec3);

/// `EffectEntity::StopEmitting`: every emitter stops emitting; the live
/// particles finish (a self-deleting effect then goes). One that isn't
/// simulated (far away / hidden) has nothing visible left to finish and is
/// removed at once when self-deleting.
#[derive(Component)]
pub struct StopEmitting;

/// `TrailEffect::StartPulse` (Engine.dll): the trail's segments start pulsing their
/// alpha (`pulseAlphaMin` ↔ `pulseAlphaMax` every `pulseAlphaCycleTime` s; `grimdawn::trails`). Put on a
/// trail entity (a line fan's when its line's travel ends, `gd_game::line_fan`).
#[derive(Component)]
pub struct StartPulse;

/// The character is not drawn: `Actor::SetVisibility(false)` (Engine.dll
/// `Entity::SetVisibility` sets `Entity.visibility` to 0, 3
/// when shown). `Actor::AddToScene`: visibility 1 / 3
/// adds the mesh, 2 / 3 its attached entities (else only those flagged
/// `Entity.forceToScene`, `AddForcedEntitiesToScene`), so 0 draws neither; the directional
/// shadow pass (`GraphicsMTRenderer::BuildShadowDirectional`)
/// skips actors that are not 1 / 3, so no shadow either. Effects in the
/// world, pets and the UI are separate entities and stay. Set by a blink's
/// warm-up (`casting::WarmUp`).
#[derive(Component)]
pub struct Hidden;

/// Start the effect again (this spec) whenever it completes (a buff's
/// character effect while the buff lasts).
#[derive(Component)]
pub struct Restart(pub EffectSpec);

#[derive(Clone, Debug)]
pub enum EffectSource {
    /// A `.pfx` resource path.
    Pfx(String),
    /// An `EffectEntity` database record.
    Record(String),
}

impl EffectSpec {
    pub fn pfx(path: impl Into<String>) -> Self {
        Self {
            source: EffectSource::Pfx(path.into()),
            despawn_when_complete: false,
            scale: 1.0,
            no_loop: false,
        }
    }
    pub fn record(record: impl Into<String>) -> Self {
        Self {
            source: EffectSource::Record(record.into()),
            ..Self::pfx("")
        }
    }
}

/// An effect's ground decal to place: the `EffectEntity` record whose
/// `decal` (a list of `Decal` records, `decalChance`, `decalRotate`) the
/// presentation resolves (grimdawn `decal.rs`), at this entity's transform.
#[derive(Component)]
pub struct PendingDecal(pub String);

/// An `FxMesh` record to show at this entity's transform (an FxPak's `meshEffectNames`; the presentation builds it,
/// grimdawn `fx/mesh.rs`).
#[derive(Component)]
pub struct FxMeshRequest(pub String);

/// A plain story object's model (`gd_game::story_world::sync_actors`): the presentation builds the record's `mesh` at
/// this entity, its `CreateEntity` attachments and lights (Engine.dll `Actor::CreateAttachmentsFromMesh`,
/// every actor) and the record's animations. Its path obstacles are gameplay's.
#[derive(Component, Clone, Debug)]
pub struct DecorationRequest {
    pub record: String,
    pub mesh: String,
}

/// A lightning bolt to create: `record` from `from` (ignored with
/// `fromSky`) to `to`.
#[derive(Component)]
pub struct PendingBolt {
    pub record: String,
    pub from: Vec3,
    pub to: Vec3,
}

/// A ribbon trail to attach to `owner` (a projectile): `record`, fallback
/// width, and the owner-local anchor points of its edges.
#[derive(Component, Clone)]
pub struct TrailRequest {
    pub owner: Entity,
    pub record: String,
    pub width: f32,
    pub anchors: Option<(Vec3, Vec3)>,
}

/// A projectile's model to attach: (mesh path, record scale, glow texture).
#[derive(Component)]
pub struct ProjectileMesh(pub String, pub f32, pub Option<String>);

/// The model entity follows this projectile and goes with it.
#[derive(Component)]
pub struct ProjectileModelOf(pub Entity);

/// A buff's character effect on `owner` (a child of it), removed with the
/// buff (`Character::AddParticleEffect`, `PFxManager::StartParticleEffect`);
/// placed at its attach point; the renderer
/// moves it with the point's bone ([`BuffFx::point`]).
#[derive(Component)]
pub struct BuffFx {
    pub owner: Entity,
    pub skill: String,
    /// It comes from a toggled skill (`Skill::IsAutoToggle`; for a buff on a
    /// character, its aura's): the `toggledAurasSelf` option hides it on the
    /// player (`gd_game::skill_fx`).
    pub toggled: bool,
    /// The mesh attach point it rides (`Actor::Attach` at the point's parent
    /// bone, posed with the skeleton); `None`: the character's origin.
    pub point: Option<String>,
}

/// A looping `SoundPak` for as long as this entity lives: the renderer loops
/// it at the [`on`](Self::on) entity (`SoundPak::Track`), else at
/// [`at`](Self::at), and when the entity goes fades it out over
/// [`fade`](Self::fade) seconds (`SoundPak::FadeLastPlayedSound`). Hearing
/// range and voices as for any looping 3D sound. The skills' loops, the
/// `CharFxPak` `activeSound`s; a [`ChildOf`] of what it belongs to goes with
/// it. Headless, inert data like the rest of this module.
#[derive(Component, Clone, Debug)]
pub struct SoundLoop {
    pub pak: String,
    pub on: Option<Entity>,
    pub at: Vec3,
    pub fade: f32,
}

/// `CharFxPak::Stop`'s `stopSound`: played at `on` when the effect entity
/// carrying this goes.
#[derive(Component, Clone, Debug)]
pub struct StopSound {
    pub pak: String,
    pub on: Entity,
}

/// A camera shake (game event type 7, `FxPak::OnAddToLevel`):
/// the sending `FxPak` record's `cameraShakeDuration` (s) and `cameraShakeAmplitude` as it is added to the level.
/// `own`: the player's own shake, always applied; else only a position in view shakes the camera
/// (`grimdawn::camera::apply_shake`, gated by the `cameraShake` option).
#[derive(Message, Clone, Copy, Debug)]
pub struct CameraShake {
    pub ms: f32,
    pub amp: f32,
    pub at: Vec3,
    pub own: bool,
}

/// `World::CreatePointDisturbance(pos, radius, strength, falloff, splash)` (Engine.dll): pushes the
/// terrain grass aside and ripples the water there (`grimdawn::render::grass` and `grimdawn::water`, which list the
/// callers). `splash`: the water type's splash effect to start (0 none, 1 only when the point is under the surface,
/// 2, 3).
#[derive(Message, Clone, Copy, Debug)]
pub struct PointDisturbance {
    pub pos: Vec3,
    pub radius: f32,
    pub strength: f32,
    pub falloff: bool,
    pub splash: u8,
}

/// `World::CreateLineDisturbance(from, to, strength, splash)` (Engine.dll): with L = |to − from|, a
/// [`PointDisturbance`] of radius 1, falloff, at `Lerp(from, to, i / L)` for i in 0..int(L)
/// ([`LineDisturbance::points`]). Sent by the waves (`waves.rs`).
#[derive(Message, Clone, Copy, Debug)]
pub struct LineDisturbance {
    pub from: Vec3,
    pub to: Vec3,
    pub strength: f32,
    pub splash: u8,
}

impl LineDisturbance {
    /// The point disturbances it makes (1 m apart from `from`; none under 1 m).
    pub fn points(&self) -> impl Iterator<Item = PointDisturbance> + '_ {
        let len = self.from.distance(self.to);
        let step = 1.0 / len;
        (0..len as i32).map(move |i| PointDisturbance {
            pos: self.from.lerp(self.to, i as f32 * step),
            radius: 1.0,
            strength: self.strength,
            falloff: true,
            splash: self.splash,
        })
    }
}

/// Play one variant of a `SoundPak` record, at a world position (heard from
/// the player) or unpositioned.
#[derive(Message, Clone, Debug)]
pub struct PlaySound {
    pub pak: String,
    pub at: Option<Vec3>,
}

/// Play one variant of a `SoundPak` record at an entity and keep it there
/// while the entity lives (`SoundPak::Track`: the engine moves a tracked
/// sound with its object every frame, `SoundManager::Update`).
#[derive(Message, Clone, Debug)]
pub struct PlaySoundOn {
    pub pak: String,
    pub on: Entity,
    /// Where the entity is now (it may not be placed yet).
    pub at: Vec3,
}

/// When the first effect matching `GD_SHOT_ON` ([`crate::hooks::ShotHooks::on`]) played (the screenshot timer's start).
#[cfg(feature = "dev")]
#[derive(Resource, Default)]
pub struct ShotTrigger(pub Option<std::time::Instant>);

/// `GD_SHOT_ON` (hooks.rs): the first effect whose record matches starts the screenshot timer ([`ShotTrigger`]).
/// Queued, so spawn helpers that only hold `Commands` can call it. Dev builds only: empty otherwise.
#[cfg(feature = "dev")]
pub fn notify(commands: &mut Commands, record: &str) {
    let record = record.to_owned();
    commands.queue(move |world: &mut World| notify_world(world, &record));
}

#[cfg(not(feature = "dev"))]
#[inline(always)]
pub fn notify(_commands: &mut Commands, _record: &str) {}

/// [`notify`] for code that has the world.
#[cfg(feature = "dev")]
pub fn notify_world(world: &mut World, record: &str) {
    let Some(want) = world.resource::<crate::hooks::Hooks>().shot.on.clone() else {
        return;
    };
    let mut trigger = world.get_resource_or_insert_with(ShotTrigger::default);
    if trigger.0.is_none()
        && record
            .to_ascii_lowercase()
            .contains(&want.to_ascii_lowercase())
    {
        info!("GD_SHOT_ON: {record}");
        trigger.0 = Some(std::time::Instant::now());
    }
}

#[cfg(not(feature = "dev"))]
#[inline(always)]
pub fn notify_world(_world: &mut World, _record: &str) {}

/// Spawns an `EffectEntity` record, or every effect of an `FxPak`, once
/// (with its ground decal).
pub fn spawn_fx(
    commands: &mut Commands,
    data: &crate::data::GameData,
    record: &str,
    tf: Transform,
) -> Option<Entity> {
    notify(commands, record);
    let rec = data.db.get(record)?;
    // FX paks: `FxPak` records and the class-less `charfxpak.tpl` ones.
    if rec.class == "FxPak" || rec.get("particleEffectNames").is_some() {
        if let Some(pak) = crate::combat_sounds::fx_pak_sound(rec) {
            commands.write_message(PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
        // `FxPak::OnAddToLevel`: a pak with a shake duration and amplitude shakes the camera once.
        let (secs, amp) = (
            rec.f32("cameraShakeDuration").unwrap_or(0.0),
            rec.f32("cameraShakeAmplitude").unwrap_or(0.0),
        );
        if secs > 0.0001 && amp > 0.0001 {
            commands.write_message(CameraShake {
                ms: secs * 1000.0,
                amp,
                at: tf.translation,
                own: false,
            });
        }
        // `FxPak::OnAddToLevel`: a `disturbanceRadius` above 0.0001 disturbs the grass
        // and water at the pak's coords, strength 5, falloff, no splash.
        let radius = rec.f32("disturbanceRadius").unwrap_or(0.0);
        if radius > 0.0001 {
            commands.write_message(PointDisturbance {
                pos: tf.translation,
                radius,
                strength: 5.0,
                falloff: true,
                splash: 0,
            });
        }
        // `FxPak::OnAddToLevel`: its mesh effects (`FxMesh` records) are placed at the pak's coords.
        for m in rec.strs_iter("meshEffectNames") {
            commands.spawn((FxMeshRequest(m.to_owned()), tf));
        }
        let mut first = None;
        for e in rec.strs_iter("particleEffectNames") {
            let id = spawn_fx(commands, data, e, tf);
            first = first.or(id);
        }
        return first;
    }
    if rec.str("decal").is_some() {
        commands.spawn((PendingDecal(record.to_owned()), tf));
    }
    let mut spec = EffectSpec::record(record);
    spec.despawn_when_complete = true;
    spec.no_loop = true;
    Some(commands.spawn((spec, tf)).id())
}

/// A mesh's trail anchors (`Anchor1`/`Anchor2`, or `Anchor3`/`Anchor4` for
/// `second`) in mesh space, bind pose; `None` without the first one.
pub fn anchors_of(msh: &gd_formats::msh::Mesh, second: bool) -> Option<(Vec3, Vec3)> {
    let (a, b) = if second {
        ("Anchor3", "Anchor4")
    } else {
        ("Anchor1", "Anchor2")
    };
    let at = |name| crate::casting::attach_point_bind(msh, name);
    Some((at(a)?, at(b).unwrap_or(Vec3::ZERO)))
}
