//! The presentation side of `gd_game::ragdoll`: the camera's view the death throw's gate reads as the preload
//! frustum (`Entity::InRenderPreLoadFrustum`, [`gd_game::ragdoll::RagdollView`]), and the decals the thrown bodies
//! drop ([`gd_game::ragdoll::BodyDecal`], growing in like the gibs': `Actor::CollisionCallback`'s flag 0x40000).
use bevy::camera::primitives::Frustum;
use bevy::prelude::*;
use gd_game::ragdoll::{BodyDecal, RagdollView};

pub struct RagdollViewPlugin;

impl Plugin for RagdollViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                write_view.before(gd_game_core::combat::CombatSet),
                body_decals.after(gd_game::ragdoll::RagdollStage::Throw),
            ),
        );
    }
}

/// The active game camera's frustum planes (last frame's: Bevy updates the frusta in `PostUpdate`).
fn write_view(
    mut view: ResMut<RagdollView>,
    cams: Query<(&Camera, &Frustum), With<gd_render::camera::GameCamera>>,
) {
    let planes = cams
        .iter()
        .find(|(c, _)| c.is_active)
        .map(|(_, f)| std::array::from_fn(|i| f.half_spaces[i].normal_d()));
    if view.0 != planes {
        view.0 = planes;
    }
}

/// A thrown body's ground decal (`casting::drop_decals` places it).
fn body_decals(mut commands: Commands, mut decals: MessageReader<BodyDecal>) {
    for d in decals.read() {
        commands.spawn((
            gd_render::fx::DropDecal(d.record.clone().into()),
            gd_render::decal::GrowIn::default(),
            Transform::from_translation(d.at).with_rotation(d.rotation),
        ));
    }
}
