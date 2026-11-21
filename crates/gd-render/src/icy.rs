//! The icy look of a frozen / petrified character ([`gd_game::afflictions::Icy`], the request; its rules and
//! sounds are in the afflictions module docs, "Icy look").
//!
//! Game.dll `SpecialCharHandler_IcyCharacter::Start` @0x1805634b0: the character's mesh instance and
//! every attached item's get a `GMIEffectIcey` (Engine.dll ctor @0x18008fd10: the override shader
//! `shaders/iceyskinned.ssh` / `iceystatic.ssh`, its `iceyTexture` = the handler's `icyEffectName`) and the
//! instance base texture `icyEffectName` (`GraphicsMeshInstance::SetBaseTexture` @0x1800c1120, the override
//! of every material's `baseTexture`; the old override is kept and put back by `End` @0x180563940). The
//! shader's `deferredPS` alpha-tests the base, then draws `lerp(base, iceyTexture, iceyFactor)` with the
//! mesh's own normal and specular maps (no glow) ([`FLAG_ICEY`]). `GMIEffectIcey::SetShaderParameters`
//! @0x1800900a0, `Update` @0x18008ff60: `iceyFactor` fades 0 → 1 over 500 ms from the start
//! (`t = 1 − max(timer · 0.002, 0)`), then holds 1; `End` puts the base texture back and `BeginFinishing`
//! @0x18008ff40 fades it 1 → 0 over 500 ms, after which the effect (and the shader) is removed. So the
//! character turns into the texture at once (base and icy texture are the same) and thaws over half a
//! second. The stock textures (`damagefreeze_overlay.tex`, `damagepetrify_overlay.tex`) are opaque, so
//! alpha-tested parts draw solid while it is on. The character's scale is not changed (the character's
//! effect is made with `expansionTarget` 0). No ice chunk mesh is drawn: the engine never spawns the
//! handler's `iceCubeName` (afflictions docs).
//!
//! PORT: `iceyAlphaMultiplier` is not ported (the deferred program does not read it); parts dressed while
//! the look is on (a re-dressed body) are not iced, as in the engine (`Start` only covers what is attached
//! then).
use crate::character::Body;
use crate::data::GameData;
use crate::render::material::{
    FLAG_DETAIL, FLAG_DISSOLVE, FLAG_GLOW, FLAG_GLOW_SCROLL, FLAG_GLOW_SCROLL_ALPHA, FLAG_ICEY,
    GdMaterial, KIND_LIT,
};
use crate::textures::Textures;
use bevy::prelude::*;
use gd_game::afflictions::{Icy, icy_texture};

/// `GMIEffectIcey` ctor / `BeginFinishing`: the 500 ms fades.
const FADE_MS: f32 = 500.0;

pub struct IcyPlugin;

impl Plugin for IcyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, draw_icy.run_if(resource_exists::<GameData>));
    }
}

/// The iced parts of a character: (part, its material, the icy copy), the look drawn and its fade timer (ms).
#[derive(Component)]
struct IcyParts {
    petrify: bool,
    parts: Vec<(Entity, Handle<GdMaterial>, Handle<GdMaterial>)>,
    timer: f32,
    /// `End` ran: thawing (base texture back, the factor fading out).
    thawing: bool,
}

/// `iceyFactor` (`GMIEffectIcey::SetShaderParameters`): fading in, `1 − max(timer · 0.002, 0)`; fading out, its
/// complement.
fn icey_factor(timer_ms: f32, thawing: bool) -> f32 {
    let t = 1.0 - (timer_ms / FADE_MS).max(0.0);
    if thawing { 1.0 - t } else { t }
}

#[allow(clippy::too_many_arguments)]
fn draw_icy(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut textures: ResMut<Textures>,
    mut images: ResMut<Assets<Image>>,
    icy: Query<(Entity, &Icy, &Body, Has<IcyParts>)>,
    mut held: Query<(Entity, &mut IcyParts, Option<&Icy>)>,
    children: Query<&Children>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
) {
    let dt = time.delta_secs() * 1000.0;
    // Running looks: fade in, or (the request gone, or switched to the other handler) thaw, then put the
    // originals back.
    for (e, mut ip, req) in &mut held {
        if !ip.thawing && req.is_none_or(|r| r.petrify != ip.petrify) {
            ip.thawing = true;
            ip.timer = FADE_MS;
            for (_, orig, copy) in &ip.parts {
                let base = materials.get(orig).map(|m| m.base.clone());
                if let (Some(base), Some(mut m)) = (base, materials.get_mut(copy)) {
                    m.base = base;
                }
            }
        }
        if ip.timer > 0.0 {
            ip.timer -= dt;
        }
        if ip.thawing && ip.timer <= 0.0 {
            for (part, orig, copy) in &ip.parts {
                if let Ok(mut m) = parts.get_mut(*part)
                    && m.0 == *copy
                {
                    m.0 = orig.clone();
                }
            }
            commands.entity(e).try_remove::<IcyParts>();
            continue;
        }
        // Write only on change: `get_mut` marks the material modified (re-prepared), and a held look sits at 1.
        let f = icey_factor(ip.timer, ip.thawing);
        for (_, _, copy) in &ip.parts {
            if materials.get(copy).is_some_and(|m| m.params.fade.w != f)
                && let Some(mut m) = materials.get_mut(copy)
            {
                m.params.fade.w = f;
            }
        }
    }
    // New looks (a thawing one finishes first).
    for (e, req, body, running) in &icy {
        if running {
            continue;
        }
        let Some(tex) = textures.get(&data, &icy_texture(&data, req.petrify), false, &mut images)
        else {
            continue;
        };
        let roots = body
            .parts
            .iter()
            .copied()
            .chain(body.weapons.iter().flat_map(|w| w.parts.iter().copied()));
        let mut list = Vec::new();
        for root in roots {
            for part in std::iter::once(root).chain(children.iter_descendants(root)) {
                let Ok(mut mat) = parts.get_mut(part) else {
                    continue;
                };
                let Some(mut m) = materials.get(&mat.0).cloned() else {
                    continue;
                };
                if m.params.flags.x != KIND_LIT || m.params.outline.w > 0.0 {
                    continue;
                }
                m.params.flags.y &= !(FLAG_GLOW
                    | FLAG_GLOW_SCROLL
                    | FLAG_GLOW_SCROLL_ALPHA
                    | FLAG_DETAIL
                    | FLAG_DISSOLVE);
                m.params.flags.y |= FLAG_ICEY;
                m.params.fade.w = icey_factor(FADE_MS, false);
                m.base = tex.clone();
                m.detail = tex.clone();
                let copy = materials.add(m);
                list.push((part, mat.0.clone(), copy.clone()));
                mat.0 = copy;
            }
        }
        // A body with no lit part (all distortion / hidden): nothing to ice.
        if list.is_empty() {
            continue;
        }
        debug!("{e}: icy look ({} parts)", list.len());
        commands.entity(e).try_insert(IcyParts {
            petrify: req.petrify,
            parts: list,
            timer: FADE_MS,
            thawing: false,
        });
    }
}
