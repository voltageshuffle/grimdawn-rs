//! The loading screen, shown while the session loads (`AppState::Loading`: the character is chosen,
//! its player is not yet spawned with its progress and inventory applied) and during a teleport (`doors::travel`,
//! faded in and out), and the HUD teardown on leaving `AppState::InGame`.
//!
//! `Grim Dawn.exe` loading screen (constructor sub_1400B7D70, draw sub_1400B8BD0):
//! - the background is picked at random (`rand() % n`) from `ui/loading/loadingbackground01.tex`, plus `02` and
//!   `03` when the second expansion is loaded and `04` with the third; drawn scaled to the screen's width (height
//!   `texture height × W / texture width`, floored), centred vertically, the bands above and below black;
//! - the tip: `GameEngine::GetLoadingTip` (Game.dll 0x1802edb60) picks one of the `loadingTips` tags of the
//!   `tipsRecord` (`records/game/gametips.dbr`, from `gameengine.dbr`, `GameEngine::LoadFromDatabase`) with
//!   `rand() % n`, localised (none: "<Insert Loading Tip Here>"); drawn in `loadingTipStyle`'s font (the unscaled
//!   style, `GetLoadingTipStyle`) at `min(size, size × W / 1920)` px, white (the style's colour is not used), centred
//!   on `W / 2`, its bottom 90 px above the screen's bottom (`GraphicsAlign(center, bottom)`, `RenderText2d` centre /
//!   bottom);
//! - everything is drawn at the screen's fade alpha (1500 ms fades by state), except the hourglass's grey disc;
//! - the progress hourglass ([`PieOmatic`], Engine.dll `GAME::PieOmatic`, built with
//!   `ui/loading/loadinggraphic01grey.tex` and `ui/loading/loadinggraphic01.tex`, updated by the screen's update
//!   sub_1400B85E0 with the machine-time delta and faded in from its first state): centred on `(W − 100, H − 100)`
//!   (`GraphicsAlign(right, bottom)` of (100, 100)), at half the textures' size (+68 = 0.5, no UI scale).
//!
//! PORT: a background missing from the install's archives is left out of the pick (the expansion test: its archives
//! are the expansion); the session's loading cover shows at once (no 1500 ms fade-in; the teleport cover fades with
//! the teleport activity, `doors::travel_screen`); the hourglass never fades out (the engine's `FadeOut` is its last
//! state, the port's cover is gone by then) and its fan is composited on the CPU into one image per frame (texels
//! outside the colour texture, which the fan's corners reach, are left transparent). Not shown for the scene test
//! hooks that never start a player (`GD_CHARS`). A second session after an
//! Exit to Main Menu loads faster than the first: the model, texture, animation and effect caches survive the
//! teardown on purpose (`exit.rs`, its `KEEP` list).
use super::{Built, UiAssets, UiRoot};
use crate::data::GameData;
use crate::session::AppState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Loading),
            show.run_if(|hooks: Res<crate::hooks::Hooks>| hooks.session.chars.is_none()),
        )
        .add_systems(Update, (dress, spin, follow_alpha).chain())
        .add_systems(OnExit(AppState::InGame), teardown_hud);
    }
}

/// Above the HUD and windows, below the cursor and the main menu's layer.
const Z: i32 = 110;

const BACKGROUNDS: [&str; 4] = [
    "ui/loading/loadingbackground01.tex",
    "ui/loading/loadingbackground02.tex",
    "ui/loading/loadingbackground03.tex",
    "ui/loading/loadingbackground04.tex",
];
const ENGINE: &str = "records/game/gameengine.dbr";

/// The cover node; [`dress`] adds its background and tip.
#[derive(Component)]
pub struct LoadingCover;

/// A cover that has its background and tip.
#[derive(Component)]
struct Dressed;

/// Spawns the loading cover. The main menu calls it in the frame it starts the session: the state only becomes `Loading` a frame
/// later and a new UI root is not drawn on its first frame, so waiting for [`show`] left a couple of frames of the world (the
/// previous session's lighting, after an Exit to Main Menu) between the menu and the cover.
/// The cover also registers a full-screen button blocker ([`super::widgets::ButtonSound`] with no sounds): during a
/// teleport fade the HUD buttons under it neither sound nor press.
pub fn spawn_cover(commands: &mut Commands) -> Entity {
    let cover = commands
        .spawn((
            LoadingCover,
            DespawnOnExit(AppState::Loading),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(Z),
        ))
        .id();
    commands.spawn((
        super::widgets::ButtonSound {
            rect: URect::new(0.0, 0.0, f32::MAX, f32::MAX),
            over: None,
            down: None,
        },
        Node {
            display: Display::None,
            ..default()
        },
        ChildOf(cover),
    ));
    cover
}

/// `OnEnter(Loading)`: the cover, unless the menu already spawned it.
fn show(mut commands: Commands, existing: Query<(), With<LoadingCover>>) {
    if existing.is_empty() {
        info!("loading screen");
        spawn_cover(&mut commands);
    }
}

/// `rand() % n` over `items` (the background and the tip picks).
fn pick<T>(items: &[T], r: u32) -> Option<&T> {
    (!items.is_empty()).then(|| &items[r as usize % items.len()])
}

/// The tip tags: `gameengine.dbr`'s `tipsRecord`, its `loadingTips` (module doc).
fn tips(db: &gd_formats::vfs::Database) -> (Vec<String>, Option<String>) {
    let Some(rec) = db
        .get(ENGINE)
        .and_then(|e| e.str("tipsRecord").map(str::to_owned))
        .and_then(|p| db.get(&p))
    else {
        return (Vec::new(), None);
    };
    (
        rec.strs("loadingTips")
            .into_iter()
            .map(str::to_owned)
            .collect(),
        rec.str("loadingTipStyle").map(str::to_owned),
    )
}

/// A new cover gets the background and the tip (module doc), as direct children (their alpha follows the cover's).
fn dress(
    mut commands: Commands,
    covers: Query<Entity, (With<LoadingCover>, Without<Dressed>)>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut assets: UiAssets,
) {
    let Ok(win) = window.single() else { return };
    let screen = Vec2::new(win.physical_width() as f32, win.physical_height() as f32);
    if screen.x == 0.0 {
        return;
    }
    for cover in &covers {
        commands.entity(cover).insert(Dressed);
        let (tags, style) = tips(&data.db);
        let tip = pick(&tags, fastrand::u32(..))
            .map(|t| assets.fonts.tag(t).to_owned())
            .unwrap_or_else(|| "<Insert Loading Tip Here>".to_owned());
        // The tip's font: the unscaled style at min(1, W / 1920).
        let s = (screen.x / 1920.0).min(1.0);
        let pie = PieOmatic::load(&data);
        let pie_img = pie.as_ref().map(|p| assets.images.add(p.image()));
        let mut ui = assets.ui(
            &mut commands,
            &data,
            cover,
            s,
            1.0 / win.scale_factor(),
            screen,
        );
        let present: Vec<(Handle<Image>, UVec2)> =
            BACKGROUNDS.iter().filter_map(|p| ui.image(p)).collect();
        if let Some((img, size)) = pick(&present, fastrand::u32(..)).cloned() {
            let h = (size.y as f32 * screen.x / size.x.max(1) as f32).floor();
            ui.quad_stretch(
                img,
                URect::new(0.0, ((screen.y - h) * 0.5).trunc(), screen.x, h),
                Color::WHITE,
            );
        }
        if let Some(style) = style {
            let lh = ui.line_height(&style);
            let w = ui.text_width(&style, &tip);
            ui.text_line_colored(
                &style,
                Vec2::new(
                    (screen.x * 0.5 - w * 0.5).round(),
                    (screen.y - 90.0 - lh).round(),
                ),
                &tip,
                Color::WHITE,
                Some([1.0; 3]),
            );
        }
        // The hourglass: the grey disc at full alpha, the colour fan over it (`spin`).
        if let (Some(pie), Some(img)) = (pie, pie_img) {
            let (w, h) = (pie.size.x as f32 * PIE_SCALE, pie.size.y as f32 * PIE_SCALE);
            let r = URect::new(screen.x - 100.0 - w * 0.5, screen.y - 100.0 - h * 0.5, w, h);
            if let Some((grey, _)) = ui.image(PIE_GREY) {
                let e = ui.quad_stretch(grey, r, Color::WHITE);
                ui.commands.entity(e).insert(PieNode);
            }
            let e = ui.quad_stretch(img, r, Color::WHITE.with_alpha(0.0));
            ui.commands.entity(e).insert((PieNode, pie));
        }
    }
}

/// The hourglass's nodes: they do not take the cover's alpha from [`follow_alpha`] ([`spin`] sets the fan's).
#[derive(Component)]
struct PieNode;

/// Steps the hourglass by the frame's time and draws its fan into its image, in the fade × the cover's alpha
/// (`PieOmatic::Render`'s `a4`, the screen's alpha).
fn spin(
    time: Res<Time>,
    covers: Query<&BackgroundColor, With<LoadingCover>>,
    mut pies: Query<(&mut PieOmatic, &mut ImageNode, &ChildOf)>,
    mut images: ResMut<Assets<Image>>,
    // The frame's sub-millisecond rest, carried so high frame rates do not slow the hourglass.
    mut carry: Local<f32>,
) {
    let ms = gd_game::session::frame_ms(&mut carry, &time).max(0) as u32;
    for (mut pie, mut node, parent) in &mut pies {
        pie.update(ms);
        let a = covers.get(parent.parent()).map_or(1.0, |bg| bg.0.alpha());
        node.color.set_alpha(pie.fade * a);
        if let Some(mut img) = images.get_mut(&node.image)
            && let Some(px) = img.data.as_mut()
        {
            pie.draw(px);
        }
    }
}

/// The cover's pieces fade with it (the teleport cover's alpha, `doors::travel_screen`).
fn follow_alpha(
    covers: Query<(&BackgroundColor, &Children), (With<LoadingCover>, Changed<BackgroundColor>)>,
    mut images: Query<&mut ImageNode, Without<PieNode>>,
    mut texts: Query<&mut TextColor>,
) {
    for (bg, children) in &covers {
        let a = bg.0.alpha();
        for c in children {
            if let Ok(mut i) = images.get_mut(*c) {
                i.color.set_alpha(a);
            }
            if let Ok(mut t) = texts.get_mut(*c) {
                t.0.set_alpha(a);
            }
        }
    }
}

const PIE_GREY: &str = "ui/loading/loadinggraphic01grey.tex";
const PIE_COLOUR: &str = "ui/loading/loadinggraphic01.tex";
/// `PieOmatic` +68: the textures' draw scale.
const PIE_SCALE: f32 = 0.5;
/// Segments out of wedge mode (`SetWedgeMode(false)`, the loading screen's): 30 (10 in wedge mode).
const PIE_SEGMENTS: usize = 30;

/// `GAME::PieOmatic` (Engine.dll: ctor 0x180188120, `SetWedgeMode` 0x180188390, `Update` 0x1801885C0, `Render`
/// 0x1801887C0) out of wedge mode. There:
/// - the colour texture is drawn as a fan of 30 segments around its centre; vertex k at angle k·2π/30 − π/2
///   (screen-clockwise from the top) at the half diagonal (w·0.7071 px, uv `(cos·√2 + 1) / 2`), each segment two
///   triangles (centre, vertex k, the edge's midpoint) and (centre, midpoint, vertex k + 1), the centre and the
///   midpoint in the mean of the two vertices' alphas; every alpha × the fade × the screen's alpha;
/// - each vertex's alpha (starting at 1) moves toward its target (starting at 1) at 0.001 / 0.5 per ms; every
///   ⌊2000 / 30⌋ = 66 ms the next vertex's target flips between 0 and 1 (cycling from 0); the fade moves toward
///   its target (`FadeIn` 1, from 0) at 0.001 per ms; the step is the elapsed ms, at most 100.
#[derive(Component)]
struct PieOmatic {
    size: UVec2,
    /// The colour texture (RGBA8).
    colour: Vec<u8>,
    /// Per texel: its segment and the weight of the segment's first vertex alpha (the rest is the next one's).
    weights: Vec<(u8, f32)>,
    alpha: [f32; PIE_SEGMENTS],
    target: [f32; PIE_SEGMENTS],
    fade: f32,
    clock: u32,
    next: usize,
}

impl PieOmatic {
    fn load(data: &GameData) -> Option<Self> {
        let b = data.vfs.read(PIE_COLOUR).ok()?;
        let t = gd_formats::tex::Tex::parse_owned(b).ok()?;
        let f = t.frames.first()?;
        Some(Self::new(
            UVec2::new(f.width, f.height),
            f.decode_rgba8(0).ok()?,
        ))
    }

    fn new(size: UVec2, colour: Vec<u8>) -> Self {
        let weights = (0..size.y)
            .flat_map(|y| (0..size.x).map(move |x| (x, y)))
            .map(|(x, y)| fan_weight(size, Vec2::new(x as f32 + 0.5, y as f32 + 0.5)))
            .collect();
        Self {
            size,
            colour,
            weights,
            alpha: [1.0; PIE_SEGMENTS],
            target: [1.0; PIE_SEGMENTS],
            fade: 0.0,
            clock: 0,
            next: 0,
        }
    }

    /// The fan's image (filled by [`Self::draw`]).
    fn image(&self) -> Image {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let mut img = Image::new(
            Extent3d {
                width: self.size.x,
                height: self.size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.colour.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        if let Some(px) = img.data.as_mut() {
            self.draw(px);
        }
        img
    }

    /// `PieOmatic::Update(ms)` with the fade's target 1 (module doc).
    fn update(&mut self, ms: u32) {
        let ms = ms.min(100);
        let step = ms as f32 * 0.001;
        self.fade = toward(self.fade, 1.0, step);
        let v = 0.001 / 0.5 * ms as f32;
        for (a, t) in self.alpha.iter_mut().zip(self.target) {
            *a = toward(*a, t, v);
        }
        let period = (2000.0 / PIE_SEGMENTS as f32) as u32;
        self.clock += ms;
        while self.clock >= period {
            self.clock -= period;
            let t = &mut self.target[self.next];
            *t = if *t <= 0.5 { 1.0 } else { 0.0 };
            self.next = (self.next + 1) % PIE_SEGMENTS;
        }
    }

    /// The colour texture in the fan's per-texel alphas (the fade and the screen's alpha go on the node).
    fn draw(&self, px: &mut [u8]) {
        for (i, (seg, k)) in self.weights.iter().enumerate() {
            let s = *seg as usize;
            let a = self.alpha[s] * k + self.alpha[(s + 1) % PIE_SEGMENTS] * (1.0 - k);
            px[i * 4 + 3] = (self.colour[i * 4 + 3] as f32 * a).round() as u8;
        }
    }
}

/// `v` moved toward `to` by at most `step`.
fn toward(v: f32, to: f32, step: f32) -> f32 {
    if (v - to).abs() < step {
        to
    } else if v < to {
        v + step
    } else {
        v - step
    }
}

/// A texel's segment and its first vertex's weight in the fan ([`PieOmatic`]): the barycentric alpha of the
/// triangle it is in, with the centre and the midpoint at the vertices' mean.
fn fan_weight(size: UVec2, p: Vec2) -> (u8, f32) {
    let d = p - size.as_vec2() * 0.5;
    let n = PIE_SEGMENTS as f32;
    let turn = (d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
    let seg = ((turn / std::f32::consts::TAU * n) as usize).min(PIE_SEGMENTS - 1);
    let r = size.x as f32 * std::f32::consts::FRAC_1_SQRT_2;
    let vert = |k: usize| {
        let a = k as f32 / n * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        Vec2::new(a.cos(), a.sin()) * r
    };
    let (v0, v1) = (vert(seg), vert(seg + 1));
    let m = (v0 + v1) * 0.5;
    // d = a·u + b·m in the triangle holding it; u is v0 (first half) or v1 (second half).
    let solve = |u: Vec2| {
        let det = u.x * m.y - u.y * m.x;
        (d.x * m.y - d.y * m.x) / det
    };
    let a0 = solve(v0);
    let k = if a0 >= 0.0 {
        // (centre, v0, m): v0 has a0, the centre and m the mean.
        (1.0 + a0) * 0.5
    } else {
        let a1 = solve(v1);
        (1.0 - a1) * 0.5
    };
    (seg as u8, k.clamp(0.0, 1.0))
}

/// Leaving the playing state: the HUD roots go and every layout cache the builders wrote is reset, so
/// nothing reads a stale window and the next session builds from scratch.
fn teardown_hud(
    mut commands: Commands,
    roots: Query<Entity, With<UiRoot>>,
    mut built: ResMut<Built>,
    mut panels: ResMut<super::UiPanels>,
) {
    super::window::despawn_all(&mut commands, &roots);
    *built = Built::default();
    *panels = default();
    commands.insert_resource(crate::inventory::InvLayout::default());
    commands.insert_resource(super::skills::SkillLayout::default());
    commands.insert_resource(super::devotion::DevotionLayout::default());
    commands.insert_resource(crate::trade::TradeLayout::default());
}
