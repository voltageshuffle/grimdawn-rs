//! Devotion shrines, presentation side: models, the dormant / restored
//! look (animation and looping effect), and the story bridge
//! (`shrine:SetLocked` on a story object → `gd_game::devotion::ShrineLock`).
//! The rules are `gd_game::devotion`; the window is `ui::shrine`.
use bevy::prelude::*;
pub use gd_game::devotion::*;
use gd_render::character::CharAssets;
use gd_render::model::{Animator, Ready};

pub struct DevotionViewPlugin;

impl Plugin for DevotionViewPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Shrine, Visibility>()
            .add_systems(
                Update,
                (
                    (dress_shrines, shrine_looks).chain().after(shrine_fights),
                    story_locks.before(story_devotion),
                ),
            );
    }
}

/// The shown look of a shrine: restored or not, and its looping effect.
#[derive(Component, Default)]
pub struct ShrineLook {
    shown: Option<bool>,
    fx: Option<Entity>,
}

/// A new shrine's model and what its mesh creates (`Actor::CreateAttachmentsFromMesh`, every
/// actor: `fx::spawn_mesh_attachments`). The Mogdrogen shrine's rune (`FX_Rune` on `BN_Restored`) follows
/// its bone: the dormant animation holds it 15 m under the shrine, the restore animation raises it
/// (`model::BoneFx`). The model loads in the background (`CharAssets::model_ready`): a shrine is dressed (and
/// given its [`ShrineLook`]) once it is built.
fn dress_shrines(
    mut commands: Commands,
    new: Query<(Entity, &Shrine), Without<ShrineLook>>,
    mut a: CharAssets,
) {
    for (e, sh) in &new {
        let db = a.data.db.clone();
        let model = match db
            .get(&sh.record)
            .and_then(|r| r.str("mesh"))
            .map(|m| a.model_ready(m))
        {
            Some(Ready::Loading) => continue,
            Some(Ready::Done(model)) => Some(model),
            Some(Ready::Failed) | None => None,
        };
        commands.entity(e).try_insert(ShrineLook::default());
        if let Some(model) = model {
            gd_render::model::spawn_model(&mut commands, e, &model);
            gd_render::fx::spawn_mesh_attachments(&mut commands, &a.data, &model, e);
        }
    }
}

/// A script's `shrine:SetLocked` names a story object: its record and spot.
/// A riftgate's `SetLocked` likewise (`gd_game::riftgates::RiftgateLock`).
fn story_locks(
    mut events: MessageReader<crate::story::StoryEffect>,
    rt: Option<NonSend<crate::story::StoryRuntime>>,
    mut out: MessageWriter<ShrineLock>,
    mut gates: MessageWriter<gd_game::riftgates::RiftgateLock>,
) {
    use gd_sim::story::ObjClass;
    for ev in events.read() {
        let gd_sim::story::Effect::SetLocked { id, locked } = ev.0 else {
            continue;
        };
        let Some(rt) = rt.as_ref() else { continue };
        let story = rt.story.borrow();
        let Some(o) = story.entities.objects.get(&id) else {
            continue;
        };
        let (record, at) = (o.record.clone(), Vec3::from(o.coords.origin));
        match o.class {
            ObjClass::Shrine => {
                out.write(ShrineLock { record, at, locked });
            }
            ObjClass::Riftgate => {
                gates.write(gd_game::riftgates::RiftgateLock { record, at, locked });
            }
            _ => {}
        }
    }
}

/// Dormant or restored animation and looping effect.
fn shrine_looks(
    mut commands: Commands,
    state: Option<Res<DevotionState>>,
    mut shrines: Query<(Entity, &Shrine, &mut ShrineLook)>,
    mut assets: CharAssets,
) {
    let Some(s) = state else { return };
    for (e, sh, mut look) in &mut shrines {
        let restored = s.restored.contains(&sh.uid) || s.totems_done.contains(&sh.uid);
        if look.shown == Some(restored) {
            continue;
        }
        let first = look.shown.is_none();
        look.shown = Some(restored);
        let Some(rec) = assets.data.db.get(&sh.record) else {
            continue;
        };
        let (anim, speed) = match (restored, first) {
            (false, _) => ("dormantAnimation", "dormantAnimationSpeed"),
            (true, true) => ("restoredAnimation", "restoredAnimationSpeed"),
            (true, false) => (
                "dormantToRestoredAnimation",
                "dormantToRestoredAnimationSpeed",
            ),
        };
        if let Some(an) = rec
            .str(anim)
            .and_then(|p| assets.cache.anim(&assets.data, p))
        {
            commands.entity(e).insert(Animator {
                anim: an,
                time: 0.0,
                speed: rec.f32(speed).unwrap_or(1.0),
                looped: !restored || first,
            });
        }
        if let Some(fx) = look.fx.take() {
            commands.entity(fx).try_despawn();
        }
        let key = if restored {
            "restoredLoopingEffect"
        } else {
            "dormantLoopingEffect"
        };
        if let Some(r) = rec.str(key).filter(|r| !r.is_empty()) {
            let fx = gd_render::fx::spawn_effect_record(&mut commands, r, Transform::IDENTITY);
            commands.entity(e).add_child(fx);
            look.fx = Some(fx);
        }
        if restored
            && !first
            && let Some(snd) = rec.str("dormantToRestoredSound")
        {
            debug!("devotion: shrine restored sound {snd}");
        }
    }
}
