//! The paper doll's UI builders and state: the `UISceneView` camera/light record ([`SceneView`]), the
//! gamma-space UI material that shows the doll's render target ([`DollMaterial`], `doll.wesl`), the
//! per-view doll state ([`Dolls`]) and the window builders ([`panel`], [`menu_character`],
//! [`rotate_button`]). The binary's `doll.rs` runs the systems that build the bodies, turn the views
//! and ride the cameras (they read the world / menu scene); this module is what the windows draw.
use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use bevy::shader::ShaderRef;
use gd_formats::collections::HashMap;
use gd_formats::ui::Rect as URect;

pub use gd_render::character::{DollBody, DollView};

/// Registers the doll's embedded shader and UI material and its state resource.
pub struct DollUiPlugin;

impl Plugin for DollUiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "doll.wesl");
        app.add_plugins(UiMaterialPlugin::<DollMaterial>::default())
            .init_resource::<Dolls>();
    }
}

/// A `UISceneView` record's camera and lights (module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct SceneView {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub target_point: Vec3,
    pub target_offset: Vec3,
    pub light_dir: Vec3,
    pub light_color: Vec3,
    pub sky: Vec3,
    pub ground: Vec3,
}

impl SceneView {
    pub fn from_record(r: &gd_formats::arz::Record) -> Self {
        let v = |k: &str| {
            Vec3::new(
                r.f32(&format!("{k}.x")).unwrap_or(0.0),
                r.f32(&format!("{k}.y")).unwrap_or(0.0),
                r.f32(&format!("{k}.z")).unwrap_or(0.0),
            )
        };
        let c = |k: &str| {
            Vec3::new(
                r.f32(&format!("{k}.r")).unwrap_or(0.0),
                r.f32(&format!("{k}.g")).unwrap_or(0.0),
                r.f32(&format!("{k}.b")).unwrap_or(0.0),
            )
        };
        Self {
            fov: r.f32("cameraFov").unwrap_or(0.0).to_radians(),
            near: r.f32("cameraNearPlane").unwrap_or(0.0),
            far: r.f32("cameraFarPlane").unwrap_or(0.0),
            target_point: v("targetPoint"),
            target_offset: v("targetOffset"),
            light_dir: v("lightDirection").normalize_or_zero(),
            light_color: c("lightColor"),
            sky: c("skyAmbientColor"),
            ground: c("groundAmbientColor"),
        }
    }

    /// Eye position (relative to the doll) for the view angle `rot`.
    pub fn eye(&self, rot: f32) -> Vec3 {
        self.target_point + Quat::from_rotation_y(rot) * self.target_offset
    }

    /// The light rig for the doll shader (module doc). The light turns with
    /// the camera, so its direction goes in view space (constant per record:
    /// Bevy warns on every change of a `ColorGrading` it does not apply).
    pub fn grading(&self) -> ColorGrading {
        let cam = Transform::from_translation(self.eye(0.0)).looking_at(self.target_point, Vec3::Y);
        let d = cam.rotation.inverse() * self.light_dir;
        let sec = |i: usize| ColorGradingSection {
            gain: self.light_color[i],
            lift: self.sky[i],
            gamma: self.ground[i],
            contrast: d[i],
            saturation: 1.0,
        };
        ColorGrading {
            global: ColorGradingGlobal {
                exposure: 1.0,
                ..default()
            },
            shadows: sec(0),
            midtones: sec(1),
            highlights: sec(2),
        }
    }
}

/// The doll image's UI material: the gamma-space scene colours drawn as they are (`doll.wesl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct DollMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
}

impl UiMaterial for DollMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_ui/doll.wesl".into()
    }
}

#[derive(Default)]
pub struct Doll {
    /// The render target (its size), drawn by `material`; a new pair per
    /// size (a resized or replaced target image stayed black).
    pub target: Option<(UVec2, Handle<Image>, Handle<DollMaterial>)>,
    /// The view rect and rotate buttons (left, right) of the last UI build, screen px.
    pub rect: Option<URect>,
    pub buttons: [Option<RotateButton>; 2],
    /// The drawn rotate-button overlay: (button, down?) and its node.
    pub button_look: Option<((usize, bool), Entity)>,
    pub record: String,
    /// What the window shows (set by the UI build).
    pub shown: bool,
    pub rotation: f32,
    /// Held rotate button (right?) and ms to the next step.
    pub hold: Option<(bool, f32)>,
    pub drag: Option<f32>,
    /// (female, item looks) the body was built for.
    pub look: Option<(bool, Vec<String>)>,
    /// The look given by the UI (the main menu's selected save).
    pub preset: Option<(bool, Vec<String>)>,
    pub body: Option<Entity>,
    pub camera: Option<Entity>,
}

/// The dolls by view.
#[derive(Resource, Default)]
pub struct Dolls(pub HashMap<DollView, Doll>);

impl Dolls {
    /// Every entity the dolls own (body, camera, rotate-button overlay): `exit.rs` despawns them before
    /// the resource is reset, since they may predate the session (the main menu's doll).
    pub fn entities(&self) -> Vec<Entity> {
        self.0
            .values()
            .flat_map(|d| [d.body, d.camera, d.button_look.map(|b| b.1)])
            .flatten()
            .collect()
    }
}

fn target_image(w: u32, h: u32) -> Image {
    Image::new_target_texture(w.max(1), h.max(1), TextureFormat::Rgba8UnormSrgb, None)
}

/// Draws `view`'s doll over `rect` (screen px) and records the view's
/// `UISceneView` record for the doll's camera.
pub fn panel(ui: &mut gd_ui_kit::Ui, view: DollView, record: &str, rect: URect) {
    panel_with(ui, view, record, rect, None);
}

/// [`panel`] for a character that is not the player: (female, item looks).
pub fn panel_with(
    ui: &mut gd_ui_kit::Ui,
    view: DollView,
    record: &str,
    rect: URect,
    preset: Option<(bool, Vec<String>)>,
) {
    let e = ui.commands.spawn(ui.node(rect)).id();
    ui.commands.entity(ui.root).add_child(e);
    let record = record.to_owned();
    ui.commands.queue(move |world: &mut World| {
        let size = UVec2::new(
            rect.w.round().max(1.0) as u32,
            rect.h.round().max(1.0) as u32,
        );
        let stale = world
            .resource::<Dolls>()
            .0
            .get(&view)
            .and_then(|d| d.target.as_ref())
            .is_none_or(|t| t.0 != size);
        let fresh = stale.then(|| {
            let image = world
                .resource_mut::<Assets<Image>>()
                .add(target_image(size.x, size.y));
            let material = world
                .resource_mut::<Assets<DollMaterial>>()
                .add(DollMaterial {
                    image: image.clone(),
                });
            (size, image, material)
        });
        let rotation = world
            .resource::<gd_game_core::hooks::Hooks>()
            .render
            .doll_rot;
        let mut dolls = world.resource_mut::<Dolls>();
        let d = dolls.0.entry(view).or_default();
        if !d.shown {
            // A newly opened window starts facing the doll (reset on close, `InventoryWindow::OnHide`);
            // test hook `GD_DOLL_ROT=<radians>`.
            d.rotation = rotation;
        }
        d.shown = true;
        d.rect = Some(rect);
        d.record = record;
        d.preset = preset;
        if fresh.is_some() {
            d.target = fresh;
        }
        let Some(m) = d.target.as_ref().map(|t| t.2.clone()) else {
            return;
        };
        if let Ok(mut em) = world.get_entity_mut(e) {
            em.insert(MaterialNode(m));
        }
    });
}

/// Stands the main menu's selected character (female, item looks) in the
/// menu scene (`DollView::Menu`).
pub fn menu_character(ui: &mut gd_ui_kit::Ui, preset: (bool, Vec<String>)) {
    ui.commands.queue(move |world: &mut World| {
        let mut dolls = world.resource_mut::<Dolls>();
        let d = dolls.0.entry(DollView::Menu).or_default();
        d.shown = true;
        d.preset = Some(preset);
    });
}

/// A rotate button: its rect (screen px) and its down / in-focus bitmaps.
#[derive(Clone)]
pub struct RotateButton {
    pub rect: URect,
    pub down: Option<Handle<Image>>,
    pub over: Option<Handle<Image>>,
}

/// Draws a rotate button's widget (its up bitmap) at window origin `o` and
/// returns it for [`rotate_buttons`].
pub fn rotate_button(
    ui: &mut gd_ui_kit::Ui,
    button: Option<gd_formats::ui::Widget>,
    o: Vec2,
) -> Option<RotateButton> {
    let Some(gd_formats::ui::Widget::Button {
        up: Some(up),
        down,
        over,
        x,
        y,
        ..
    }) = button
    else {
        return None;
    };
    let rect = ui.bitmap(&up, x, y, o)?.1;
    let down = down.and_then(|d| ui.image(&d)).map(|i| i.0);
    let over = over.and_then(|d| ui.image(&d)).map(|i| i.0);
    Some(RotateButton { rect, down, over })
}

/// Records `view`'s rotate buttons (left, right) for the input.
pub fn rotate_buttons(ui: &mut gd_ui_kit::Ui, view: DollView, buttons: [Option<RotateButton>; 2]) {
    ui.commands.queue(move |world: &mut World| {
        if let Some(mut dolls) = world.get_resource_mut::<Dolls>() {
            dolls.0.entry(view).or_default().buttons = buttons;
        }
    });
}
