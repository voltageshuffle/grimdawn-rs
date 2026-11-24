//! Masked UI quads: a texture multiplied by another texture's alpha laid
//! over a screen rect. The bar graphs' cap (`GameBarGraph`'s cap draw,
//! Grim Dawn.exe `StatusBar::Render`) uses it: shader technique "blend2" with
//! `baseTexture` = the cap bitmap over the cap quad and `maskTexture` = the
//! bar's empty bitmap, its UVs (0..1) spanning the bar's widget rect, so the
//! cap only shows inside the empty bitmap's shape (the round orbs).
//! PORT: "blend2" taken as base × mask alpha; outside the widget rect the
//! mask is 0 (the engine's sampler addressing there is not checked).
use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub struct MaskedPlugin;

impl Plugin for MaskedPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "masked.wesl");
        app.add_plugins(UiMaterialPlugin::<MaskedMaterial>::default());
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
    let mat = MaskedMaterial {
        color: LinearRgba::WHITE,
        mask_rect: Vec4::new(mask_rect.x, mask_rect.y, mask_rect.w, mask_rect.h),
        image,
        mask,
    };
    ui.commands.queue(move |world: &mut World| {
        let h = world.resource_mut::<Assets<MaskedMaterial>>().add(mat);
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(MaterialNode(h));
        }
    });
    e
}
