//! Masked UI quads: a texture multiplied by another texture's alpha laid
//! over a screen rect. The bar graphs' cap (`GameBarGraph`'s cap draw,
//! Grim Dawn.exe `StatusBar::Render`) uses it: shader technique "blend2" with
//! `baseTexture` = the cap bitmap over the cap quad and `maskTexture` = the
//! bar's empty bitmap, its UVs (0..1) spanning the bar's widget rect, so the
//! cap only shows inside the empty bitmap's shape (the round orbs).
//! PORT: "blend2" taken as base × mask alpha; outside the widget rect the
//! mask is 0 (the engine's sampler addressing there is not checked).
use bevy::asset::{AssetId, embedded_asset};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use gd_formats::collections::HashMap;

pub struct MaskedPlugin;

impl Plugin for MaskedPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "masked.wesl");
        app.add_plugins(UiMaterialPlugin::<MaskedMaterial>::default())
            .init_resource::<MaskedCache>();
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct MaskedMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    /// The mask's rect in framebuffer px (x, y, w, h).
    #[uniform(0)]
    pub mask_rect: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub image: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub mask: Handle<Image>,
}

impl UiMaterial for MaskedMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_ui_kit/masked.wesl".into()
    }
}

/// Materials shared by (image, mask, mask rect bits): a rebuild of the same masked quad reuses its
/// material instead of minting one per build.
#[derive(Resource, Default)]
#[allow(clippy::type_complexity)]
pub struct MaskedCache(HashMap<(AssetId<Image>, AssetId<Image>, [u32; 4]), Handle<MaskedMaterial>>);

/// A node showing `image` over `dst` (screen px), masked by `mask`'s alpha
/// stretched over `mask_rect` (screen px).
pub fn quad(
    ui: &mut super::Ui,
    image: Handle<Image>,
    mask: Handle<Image>,
    dst: gd_formats::ui::Rect,
    mask_rect: gd_formats::ui::Rect,
) -> Entity {
    let e = ui.commands.spawn(ui.node(dst)).id();
    ui.commands.entity(ui.root).add_child(e);
    let key = (
        image.id(),
        mask.id(),
        [mask_rect.x, mask_rect.y, mask_rect.w, mask_rect.h].map(f32::to_bits),
    );
    let mat = MaskedMaterial {
        color: LinearRgba::WHITE,
        mask_rect: Vec4::new(mask_rect.x, mask_rect.y, mask_rect.w, mask_rect.h),
        image,
        mask,
    };
    ui.commands.queue(move |world: &mut World| {
        let h = world.resource_scope(|world, mut cache: Mut<MaskedCache>| {
            if cache.0.len() > 4096 {
                cache.0.clear();
            }
            cache
                .0
                .entry(key)
                .or_insert_with(|| world.resource_mut::<Assets<MaskedMaterial>>().add(mat))
                .clone()
        });
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(MaterialNode(h));
        }
    });
    e
}
