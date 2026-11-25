//! `FxMesh` effects (Game.dll `GAME::FxMesh`, an FxPak's `meshEffectNames`: e.g. the devotion ground effects, bone
//! spikes, ice spikes): an animated model placed at the pak's coords.
//!
//! `FxMesh::Load`, `Start`, `UpdateSelf`: the
//! record's `meshName`, up to three animations (`animationName`, `animationName2`, `animationName3` with `speed`,
//! `speed2`, `speed3` and `animationWeight`, ...: one is picked by weight when it starts), `loop`, `scale` (uniform),
//! `fadeInTime` / `sustainTime` / `fadeOutTime` (seconds); the effect removes itself once its animation has ended (a
//! looping one after its fade-in, sustain and fade-out). With any of the three times set, `Start` enters the fade-in
//! and `UpdateSelf` sets the mesh instance's diffuse colour to a grey `c` ([`grey`]). Game.dll:
//! `Load` stores the three times as whole ms (`FxMesh.fadeInMs`, `fadeOutMs`, `sustainMs`); `Start`
//! arms the fade-in when any is > 0; `UpdateSelf` counts the phase down by the frame's ms: fade-in
//! `c = sat(1 - left / fadeInTime)`, then the sustain (no colour write: the fade-in's last 1 stays), then, when
//! `fadeOutTime` > 0, `c = sat(left / fadeOutTime)`, after which the colour keeps its last value (0) and the effect
//! goes once its animation has ended. `GraphicsMeshInstance::SetDiffuseColor` (Engine.dll) writes
//! `GraphicsMeshInstance.diffuseColor`, which `FxMesh::AddToScene` leaves alone (no `Actor` tint over it) and
//! `SetShaderParameters` sends as `diffuseColor` = 0.5 x `c` (here: the parts' [`GdMaterial`] copies): a darkening,
//! not an alpha fade. A character pak's mesh rides the record's `attachPoint`
//! (`PFxManager::AddMeshEffect`: `Attach(effect, identity, attachPoint)`); with `stationaryAttachment`
//! ([`Stationary`]) it does not turn with the character. A worn item's mesh creates its `FxMesh`es like its effects
//! (`Actor::CreateAttachmentsFromMesh`), which play the record's weapon-set animations as the player animates
//! ([`sync_meshes`]).
//! PORT: an effect with no animation lasts the 3 s of the FxPak.
use crate::character::CharAssets;
use crate::render::material::GdMaterial;
use bevy::prelude::*;
use gd_game_core::visuals::FxMeshRequest;
use gd_sim::anim::AnimType;

pub struct FxMeshPlugin;

impl Plugin for FxMeshPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (start_meshes, sync_meshes).chain(),
                expire_meshes,
                fade_meshes,
            ),
        );
    }
}

/// `stationaryAttachment`: attached to a character, the mesh follows its position, not its turn.
#[derive(Component)]
pub struct Stationary;

/// The `AnimationSet_Type` slots `FxMesh::Load` fills from the record (melee / ranged prefix):
/// AttackIdle, Run, Attack, Fidget, MenuIdle, MenuFidget.
const SYNC_SLOTS: [(AnimType, &str); 6] = [
    (AnimType::AttackIdle, "AttackIdleAnim"),
    (AnimType::Run, "RunAnim"),
    (AnimType::Attack, "AttackAnim"),
    (AnimType::Fidget, "FidgetAnim"),
    (AnimType::MenuIdle, "AttackIdleAnim"),
    (AnimType::MenuFidget, "FidgetAnim"),
];

/// A worn item's mesh effect's weapon-set animations (`meleeAttackAnim`, `rangedRunAnim`, ...: path, speed per slot)
/// and the wearer's animation start last seen (the clip's address, its time).
#[derive(Component)]
struct Synced {
    melee: Vec<(AnimType, String, f32)>,
    ranged: Vec<(AnimType, String, f32)>,
    last: Option<(usize, f32)>,
}

impl Synced {
    fn of(rec: &gd_formats::arz::Record) -> Option<Self> {
        let slots = |prefix: &str| -> Vec<(AnimType, String, f32)> {
            SYNC_SLOTS
                .iter()
                .filter_map(|(t, k)| {
                    let path = rec.str(&format!("{prefix}{k}")).filter(|p| !p.is_empty())?;
                    let speed = rec.f32(&format!("{prefix}{k}Speed")).unwrap_or(1.0);
                    Some((*t, path.to_owned(), speed))
                })
                .collect()
        };
        let (melee, ranged) = (slots("melee"), slots("ranged"));
        (!melee.is_empty() || !ranged.is_empty()).then_some(Synced {
            melee,
            ranged,
            last: None,
        })
    }
}

/// `FxMesh::PlaySyncedAnimation`: every animation the player starts (`Player::PlayAnimation`
/// passes its type, `EquipManager::GetHandState` and the loop flag down the attached entities, so to a
/// worn item's mesh effects) plays the effect's ranged set's clip for that type when the hand state is ranged
/// (`Ranged1h`, `Ranged2h`, `DualRanged`, `Ranged1hShield`, `Ranged1hOffhand`: mask 0x12B00), else the melee set's,
/// with the clip's speed and the player's loop flag; a type without a clip leaves the effect's animation as it is.
/// Only the player passes it on (monsters' `PlayAnimation` does not).
fn sync_meshes(
    mut commands: Commands,
    mut q: Query<(Entity, &mut Synced, &super::items::ItemFx)>,
    players: Query<&gd_game_core::character::Character, With<gd_game_core::player::Player>>,
    mut assets: CharAssets,
) {
    use gd_sim::equip::HandState as H;
    for (e, mut s, f) in &mut q {
        let Ok(c) = players.get(f.owner) else {
            continue;
        };
        let Some(cur) = &c.channel.current else {
            continue;
        };
        let key = std::sync::Arc::as_ptr(&cur.anim) as usize;
        let started = s.last.is_none_or(|(k, t)| k != key || cur.time_ms < t);
        s.last = Some((key, cur.time_ms));
        if !started {
            continue;
        }
        let ranged = matches!(
            c.hand,
            H::Ranged1h | H::Ranged2h | H::DualRanged | H::Ranged1hShield | H::Ranged1hOffhand
        );
        let set = if ranged { &s.ranged } else { &s.melee };
        let Some((_, path, speed)) = set.iter().find(|(t, ..)| *t == c.playing) else {
            continue;
        };
        if let Some(anim) = assets.cache.anim(&assets.data, path) {
            commands.entity(e).insert(crate::model::Animator {
                anim,
                time: 0.0,
                speed: *speed,
                looped: cur.looped,
            });
        }
    }
}

/// A shown mesh effect: seconds left.
#[derive(Component)]
struct FxMeshLife(f32);

/// A mesh effect's grey fade: the record's times (s), the seconds since it started, and the parts' own material copies.
#[derive(Component)]
struct FxMeshFade {
    times: [f32; 3],
    t: f32,
    owned: Vec<Entity>,
}

/// `FxMesh::UpdateSelf`'s grey `t` seconds after `Start` for `[fadeInTime, sustainTime, fadeOutTime]`.
fn grey(t: f32, [fade_in, sustain, fade_out]: [f32; 3]) -> f32 {
    if t < fade_in {
        (t / fade_in).clamp(0.0, 1.0)
    } else if t < fade_in + sustain || fade_out <= 0.0 {
        1.0
    } else {
        (1.0 - (t - fade_in - sustain) / fade_out).clamp(0.0, 1.0)
    }
}

/// Applies [`grey`] to the effect's parts (each gets its own copy of its [`GdMaterial`]).
fn fade_meshes(
    time: Res<Time>,
    mut q: Query<(&mut FxMeshFade, &Children)>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
) {
    for (mut f, children) in &mut q {
        f.t += time.delta_secs();
        let c = grey(f.t, f.times);
        for p in children.iter() {
            let Ok(mut m) = parts.get_mut(p) else {
                continue;
            };
            if !f.owned.contains(&p) {
                let Some(copy) = materials.get(&m.0).cloned() else {
                    continue;
                };
                m.0 = materials.add(copy);
                f.owned.push(p);
            }
            // Write only on change: `get_mut` marks the material modified.
            let diffuse = Vec3::splat(c * 0.5);
            if materials
                .get(&m.0)
                .is_some_and(|mat| mat.params.diffuse.truncate() != diffuse)
                && let Some(mut mat) = materials.get_mut(&m.0)
            {
                let w = mat.params.diffuse.w;
                mat.params.diffuse = diffuse.extend(w);
            }
        }
    }
}

fn start_meshes(
    mut commands: Commands,
    new: Query<(
        Entity,
        &FxMeshRequest,
        &Transform,
        Has<gd_game_core::visuals::BuffFx>,
        Has<super::items::ItemFx>,
    )>,
    mut assets: CharAssets,
) {
    for (e, req, tf, buff, item) in &new {
        // The database by its own handle: the record stays borrowed while `assets` loads models.
        let db = assets.data.db.clone();
        let Some(rec) = db.get(&req.0) else {
            commands.entity(e).remove::<FxMeshRequest>();
            commands.entity(e).try_despawn();
            continue;
        };
        // A model not built yet loads in the background: the mesh effect starts on a later frame.
        let mesh = match rec.str("meshName").map(|m| assets.model_ready(m)) {
            Some(crate::model::Ready::Loading) => continue,
            Some(crate::model::Ready::Done(m)) => Some(m),
            _ => None,
        };
        commands.entity(e).remove::<FxMeshRequest>();
        let Some(mesh) = mesh else {
            commands.entity(e).try_despawn();
            continue;
        };
        // The animations with their speeds and weights; one is picked by weight.
        let mut anims: Vec<(String, f32, f32)> = Vec::new();
        for (k, sk, wk) in [
            ("animationName", "speed", "animationWeight"),
            ("animationName2", "speed2", "animationWeight2"),
            ("animationName3", "speed3", "animationWeight3"),
        ] {
            if let Some(a) = rec.str(k).filter(|a| !a.is_empty()) {
                anims.push((
                    a.to_owned(),
                    rec.f32(sk).unwrap_or(1.0),
                    rec.f32(wk).unwrap_or(1.0).max(0.0),
                ));
            }
        }
        let total: f32 = anims.iter().map(|a| a.2).sum();
        let mut roll = gd_game_core::rng::f32() * total;
        let pick = anims
            .iter()
            .find(|a| {
                roll -= a.2;
                roll <= 0.0
            })
            .or(anims.first())
            .cloned();
        let looped = rec.bool("loop");
        let (fade_in, sustain, fade_out) = (
            rec.f32("fadeInTime").unwrap_or(0.0),
            rec.f32("sustainTime").unwrap_or(0.0),
            rec.f32("fadeOutTime").unwrap_or(0.0),
        );
        let mut life = 3.0;
        if let Some((path, speed, _)) = &pick
            && let Some(anim) = assets.cache.anim(&assets.data, path)
        {
            let len = anim.num_frames as f32 / anim.frame_rate.max(1) as f32 / speed.max(0.01);
            life = if looped && (buff || item) {
                // A character effect pak's mesh runs until the buff removes it, a worn item's while it is worn.
                f32::INFINITY
            } else if looped {
                (fade_in + sustain + fade_out).max(1.0)
            } else {
                // Removed once the animation has ended and the fade is over.
                len.max(fade_in + sustain + fade_out)
            };
            commands.entity(e).insert(crate::model::Animator {
                anim,
                time: 0.0,
                speed: *speed,
                looped,
            });
        }
        debug!(
            "fx mesh {} life {life:.2}s anim {:?}",
            req.0,
            pick.as_ref().map(|p| &p.0)
        );
        let scale = rec.f32("scale").unwrap_or(1.0);
        commands.entity(e).insert((
            Transform {
                scale: tf.scale * scale,
                ..*tf
            },
            Visibility::default(),
            FxMeshLife(life),
        ));
        if rec.bool("stationaryAttachment") {
            commands.entity(e).insert(Stationary);
        }
        if item && let Some(s) = Synced::of(rec) {
            commands.entity(e).insert(s);
        }
        if fade_in > 0.0 || sustain > 0.0 || fade_out > 0.0 {
            commands.entity(e).insert(FxMeshFade {
                times: [fade_in, sustain, fade_out],
                t: 0.0,
                owned: Vec::new(),
            });
        }
        crate::model::spawn_model(&mut commands, e, &mesh);
    }
}

fn expire_meshes(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut FxMeshLife)>) {
    for (e, mut l) in &mut q {
        l.0 -= time.delta_secs();
        if l.0 <= 0.0 {
            commands.entity(e).try_despawn();
        }
    }
}
