//! Additive-blend UI quads. The real game draws the devotion map's art
//! (constellations, stars, nebulae, glows) with the `additive` blend style
//! (`GraphicsCanvas::RenderStyledRect`, name "additive" in `DevotionStar::Render`,
//! `DevotionConstellation::Render`, `DevotionWindow::Render`'s render `DevotionWindow::RenderMap`) and the bag grids'
//! new-item glow (`BagWidget::Render`, `ui/newitems.rs`). Bevy's UI has no per-node blend
//! mode, so this is a `UiMaterial` whose pipeline blends source-alpha + one.
//! PORT: source-alpha / one is the blend assumed for the engine's "additive" style (the
//! particle additive state in `fx/render.rs` is the same).
use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
};
use bevy::shader::ShaderRef;
use gd_formats::collections::HashMap;

pub struct AdditivePlugin;

impl Plugin for AdditivePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "additive.wesl");
        app.add_plugins(UiMaterialPlugin::<AdditiveMaterial>::default())
            .init_resource::<AdditiveCache>();
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct AdditiveMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[texture(1)]
    #[sampler(2)]
    pub image: Handle<Image>,
}

impl UiMaterial for AdditiveMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_ui_kit/additive.wesl".into()
    }
    fn specialize(descriptor: &mut RenderPipelineDescriptor, _key: UiMaterialKey<Self>) {
        if let Some(frag) = descriptor.fragment.as_mut() {
            let c = BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState {
                    color: c,
                    alpha: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                });
            }
        }
    }
}

/// Materials shared by (image, 8-bit colour), so a rebuilt tree reuses bind groups.
#[derive(Resource, Default)]
pub struct AdditiveCache(HashMap<(AssetId<Image>, [u8; 4]), Handle<AdditiveMaterial>>);

impl AdditiveCache {
    pub fn get(
        &mut self,
        mats: &mut Assets<AdditiveMaterial>,
        image: &Handle<Image>,
        color: Color,
    ) -> Handle<AdditiveMaterial> {
        if self.0.len() > 4096 {
            self.0.clear();
        }
        let l = color.to_linear();
        let key = (
            image.id(),
            [l.red, l.green, l.blue, l.alpha].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
        );
        self.0
            .entry(key)
            .or_insert_with(|| {
                mats.add(AdditiveMaterial {
                    color: l,
                    image: image.clone(),
                })
            })
            .clone()
    }
}

/// Adds the material to `e` once commands are applied (needs world access).
pub fn queue_additive(commands: &mut Commands, e: Entity, image: Handle<Image>, color: Color) {
    commands.queue(move |world: &mut World| {
        let h = world.resource_scope(|world, mut cache: Mut<AdditiveCache>| {
            let mut mats = world.resource_mut::<Assets<AdditiveMaterial>>();
            cache.get(&mut mats, &image, color)
        });
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(MaterialNode(h));
        }
    });
}

/// A node showing `image` with additive blending over `dst` (screen px).
pub fn quad(
    ui: &mut super::Ui,
    image: Handle<Image>,
    dst: gd_formats::ui::Rect,
    color: Color,
) -> Entity {
    let e = ui.commands.spawn(ui.node(dst)).id();
    ui.commands.entity(ui.root).add_child(e);
    queue_additive(ui.commands, e, image, color);
    e
}
