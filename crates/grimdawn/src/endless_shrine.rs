//! The Shattered Realm's buff shrines, presentation side (`gd_game::endless::EndlessShrine`, which holds the
//! rules; `EndlessBuffShrine`, a `StaticShrine` subclass).
//!
//! Stock records (`records/endlessdungeon/shrines/buffshrine_01..05`): the model is `mesh` with the
//! `baseTextures` of the colour (`CharAssets::spawn_record_model`); an unused shrine loops `dormantAnimation` and
//! the `dormantLoopingEffect` (`endlessdungeonshrine_loop<colour>01`); using it plays `dormantToRestoredAnimation`
//! and `dormantToRestoredSound`, then it stays in `restoredAnimation` (`shrine_endlessdungeon_used01a`); a
//! shrine whose level streams back in used starts in that animation. The proxy's `proxyActivateSound` plays
//! at the shrine's own coordinates (`StaticShrine::SetShrineState`, state 2, set by
//! `StaticShrine::RunProxy` right after it runs its proxy: here, where the shrine appears).
//! `EndlessBuffShrine::AppendDetailMapData`: the minimap's Custom nugget (type 19) is
//! offered while the shrine's dungeon stamp is the running dungeon's and the record's hide flag
//! (`EndlessBuffShrine.hideOnMap`) is
//! clear, used or not: the symbol does not hide on use.
//!
//! The looping effects sit at the mesh attach point the record names (`dormantLoopingAttachment` /
//! `restoredLoopingAttachment`, stock `FX`, 5.4 m up on the shrine mesh; the point has no parent bone). The stock
//! records have no `dormantToRestoredEffect`, so `dormantToRestoredEffectAttachment` is unused.
//! Engine.dll `Actor::Load` / `GraphicsMeshInstance::SetShaderParameters` /
//! `GraphicsShader2::SetTexture`: the record's `glowTexture`
//! overrides the glow of every material, bound by parameter name, so it shows only on a material whose shader has
//! a `glowTexture`: on `shrine_endlessdungeon01a.msh` the first material (`standardglowskinned`, stock
//! `_red_glo`) takes the colour's `_glo`; the energy (`distortadditiveskinned`) and used (`standardskinned`)
//! materials have none. `spawn_record_model` applies it that way (`MaterialAssets::reshade`).
//! The minimap symbol is `ui/localmap.rs`'.
use bevy::prelude::*;
use gd_game::endless::{EndlessShrine, ShrineProxyRan};
use gd_render::character::CharAssets;
use gd_render::model::Animator;

pub struct EndlessShrineViewPlugin;

impl Plugin for EndlessShrineViewPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<EndlessShrine, Visibility>()
            .add_systems(Update, (dress, looks, proxy_sound).chain());
    }
}

/// The shown look of a shrine: used or not, and its looping effect.
#[derive(Component, Default)]
struct Look {
    shown: Option<bool>,
    fx: Option<Entity>,
}

/// Dresses a new shrine once its model is built (loaded in the background, `CharAssets::models_ready`): its
/// [`Look`] goes on with the model, so [`looks`] starts its animation on a dressed shrine.
fn dress(
    mut commands: Commands,
    new: Query<(Entity, &EndlessShrine), Without<Look>>,
    mut a: CharAssets,
) {
    for (e, sh) in &new {
        let db = a.data.db.clone();
        let rec = db.get(&sh.record);
        if let Some(mesh) = rec.and_then(|r| r.str("mesh"))
            && !a.models_ready([mesh])
        {
            continue;
        }
        commands.entity(e).insert(Look::default());
        if let Some(rec) = rec {
            a.spawn_record_model(&mut commands, e, rec);
        }
    }
}

/// Dormant or restored animation and looping effect; the use plays `dormantToRestoredSound`.
fn looks(
    mut commands: Commands,
    mut shrines: Query<(
        Entity,
        &EndlessShrine,
        &GlobalTransform,
        &mut Look,
        Option<&gd_render::model::Skeleton>,
    )>,
    mut assets: CharAssets,
    mut sounds: ResMut<crate::audio::Sounds>,
) {
    for (e, sh, gt, mut look, skel) in &mut shrines {
        if look.shown == Some(sh.used) {
            continue;
        }
        let first = look.shown.is_none();
        look.shown = Some(sh.used);
        let Some(rec) = assets.data.db.get(&sh.record) else {
            continue;
        };
        let (anim, speed) = match (sh.used, first) {
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
                looped: !sh.used || first,
            });
        }
        if let Some(fx) = look.fx.take() {
            commands.entity(fx).try_despawn();
        }
        let (key, attach) = if sh.used {
            ("restoredLoopingEffect", "restoredLoopingAttachment")
        } else {
            ("dormantLoopingEffect", "dormantLoopingAttachment")
        };
        if let Some(r) = rec.str(key).filter(|r| !r.is_empty()) {
            // The record names the mesh attach point (`FX`); the shrine meshes' point has no parent bone.
            let at = rec
                .str(attach)
                .and_then(|name| skel?.msh.attach_point(name))
                .map_or(Transform::IDENTITY, |ap| {
                    let c = &ap.coords;
                    let axes = Mat3::from_cols(
                        Vec3::from(c.x).normalize_or(Vec3::X),
                        Vec3::from(c.y).normalize_or(Vec3::Y),
                        Vec3::from(c.z).normalize_or(Vec3::Z),
                    );
                    Transform::from_translation(Vec3::from(c.origin))
                        .with_rotation(Quat::from_mat3(&axes).normalize())
                });
            let fx = gd_render::fx::spawn_effect_record(&mut commands, r, at);
            commands.entity(e).add_child(fx);
            look.fx = Some(fx);
        }
        if sh.used
            && !first
            && let Some(snd) = rec.str("dormantToRestoredSound").filter(|s| !s.is_empty())
        {
            crate::audio::play_pak(
                &mut commands,
                &assets.data,
                &mut sounds,
                snd,
                Some(gt.translation()),
            );
        }
    }
}

/// `proxyActivateSound`, where a shrine proxy ran.
fn proxy_sound(
    mut commands: Commands,
    mut ran: MessageReader<ShrineProxyRan>,
    data: Res<gd_game_core::data::GameData>,
    mut sounds: ResMut<crate::audio::Sounds>,
) {
    for r in ran.read() {
        if let Some(snd) = data
            .db
            .get(&r.record)
            .and_then(|rec| rec.str("proxyActivateSound").map(str::to_owned))
            .filter(|s| !s.is_empty())
        {
            crate::audio::play_pak(&mut commands, &data, &mut sounds, &snd, Some(r.at));
        }
    }
}
