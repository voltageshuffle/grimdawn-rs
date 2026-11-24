//! The cinematic text (`Engine::ShowCinematicText`, [`gd_game_core::story::CinematicText`]): the big announcement near the
//! top of the screen (new mutators, bonus texts, lore bonus, difficulty unlocked, ...).
//!
//! Grim Dawn.exe update `CinematicText::Update` / draw `CinematicText::Render`: texts wait in a FIFO
//! and show one at a time. Per type (scale of the headline font, hold ms): 1 and 7: 1.5, 3000, and the camera shakes
//! `ShakeCamera(0.5, 0.5)` when the hold starts; 2 to 6: 1.5, 3000; 8 and 9: 1.25, 3000; 10: 1.6, 3000 (pushed to the
//! front); every type grows in. Fade-in 150 ms (alpha 0 to 1, the size shrinks from 2x to 1x), the hold, fade-out 250 ms
//! (alpha 1 to 0); the next text starts the frame the last one ends. Headline: `style_cinematictext` (32 px x scale),
//! centred, wrapped at 0.45 x the screen width, the block centred at 15 % of the height, colour from the caller (white;
//! (0.9, 0.9, 1) for types 2, 8, 9), each line over a `skills_leveltextshadow01.tex` strip ((line width + 200) x (line
//! height + 40), white at 0.7 alpha). The subline (32 px unscaled, 0.75 alpha, same strips) appears under the headline
//! once the fade-in is over (its `^X` colour codes apply). No sound.
//! Headline glow (`CinematicText::Render`, Engine.dll `BlurFrameBuffer2` / `GraphicsCanvas_Blur8Tap`): the lines are
//! drawn white (one colour, not the style's) and centred into a temporary surface cleared to opaque black, (wrap
//! width + 20) x (lines x line height + 20), line i at y = 10 + i x line height; the surface is blurred (a vertical then a
//! horizontal pass of eight taps 0.1, 0.11, 0.14, 0.15, 0.15, 0.14, 0.11, 0.1 at -3.5 ... 3.5 texels, into half-size
//! surfaces) and drawn additive tinted by the event colour at 0.75 alpha, then the sharp surface additive at 0.35, both
//! stretched over the block size x the grow factor centred at (W/2, 0.15 H). The strips are sized by the grow factor
//! too, their centres stepped by the unscaled line height from the scaled block's top + 10. Here the surface is built
//! on the CPU once per headline ([`headline_surface`], [`blur`]) and drawn with `ui/additive.rs`.
//! Type 0 (the challenge-area name, `gd_game::challenge_areas`, and the area name, [`area_names`]): only the frame's last type-0 text is kept, queued
//! only into an empty queue (none showing) and dropped while a big window is open (`UIPlayerHud::IsBigWindowOpen` /
//! `UIPlayerHud::IsBigWindowOpenEx`, [`BigWindows`]).
//! PORT: the 10 / 20 / 200 / 40 px paddings are multiplied by the UI scale like the font size; a TrueType (FNTD) font
//! is drawn as plain text (no CPU glyphs to blur).
use super::UiAssets;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;
use gd_game_core::data::GameData;
use std::collections::VecDeque;

const STYLE: &str = "records/ui/styles/text/style_cinematictext.dbr";
const STRIP: &str = "ui/skills/skillallocation/skills_leveltextshadow01.tex";
const FADE_IN: f32 = 150.0;
const FADE_OUT: f32 = 250.0;

/// The caller's `(scale, hold ms)` and whether the camera shakes (the table in the module doc).
fn timing(kind: u32) -> (f32, f32, bool) {
    match kind {
        1 | 7 => (1.5, 3000.0, true),
        8 | 9 => (1.25, 3000.0, false),
        10 => (1.6, 3000.0, false),
        0 => (1.5, 1500.0, false),
        _ => (1.5, 3000.0, false),
    }
}

fn colour(kind: u32) -> Color {
    match kind {
        2 | 8 | 9 => Color::srgb(0.9, 0.9, 1.0),
        _ => Color::WHITE,
    }
}

#[derive(Resource, Default)]
pub struct CinematicQueue {
    queue: VecDeque<gd_game_core::story::CinematicText>,
    /// Ms into the current text (the head of the queue).
    age: f32,
    shaken: bool,
}

#[derive(Component)]
struct CinematicRoot;

/// The big windows that drop a type-0 text (Grim Dawn.exe `UIPlayerHud::IsBigWindowOpen` /
/// `UIPlayerHud::IsBigWindowOpenEx`: the
/// HUD's party,
/// character, quest, market / faction market / smith, trade, caravan, map, skills, factions, achievements, shrine,
/// devotion, transmuter, altar, potions, item ascension and loot filter windows, each `IsVisible`). The port's
/// windows: the character / skills / devotion panels, any NPC trade window (markets, smiths, caravan,
/// transmuter, illusionist, altar), the journal, the map, the loot filter, the devotion shrine and the potions
/// window.
#[derive(bevy::ecs::system::SystemParam)]
pub struct BigWindows<'w> {
    panels: Res<'w, gd_ui::UiPanels>,
    trade: Res<'w, gd_ui::state::trade::Trade>,
    journal: Res<'w, gd_ui::quests::QuestJournal>,
    map: Res<'w, gd_ui::state::map::MapWindow>,
    filter: Res<'w, gd_ui::lootfilter::LootFilterView>,
    shrine: Res<'w, gd_game::devotion::ShrineWindow>,
    potions: Res<'w, gd_game::cauldron::PotionWindow>,
}

impl BigWindows<'_> {
    pub fn any(&self) -> bool {
        self.panels.character
            || self.panels.skills
            || self.panels.devotion
            || self.trade.open.is_some()
            || self.journal.open
            || self.map.open
            || self.filter.open
            || self.shrine.shrine.is_some()
            || self.potions.open
    }
}

pub struct CinematicPlugin;

impl Plugin for CinematicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CinematicQueue>().add_systems(
            Update,
            (area_names, show).chain().in_set(gd_ui_kit::UiPlaySet),
        );
    }
}

/// The area name's banner: a type-0 cinematic text when the name sector (layer 1) under the ambiance target changes.
/// Engine.dll `AmbianceManager::Update`: the name sector at the target (`NameSectorData`: tag,
/// `NameSectorData.show` = the entry's first flag, `NameSectorData.once` = the second, 1 when the level's sector
/// version has none, `SectorDataManager::ReadNameData`) is compared with the last name seen; when it differs it becomes
/// the last one, and it counts as new when not yet in the session's list of shown names (added then) or when "once" is
/// 0. A new name of a sector with "show" set queues `ShowCinematicText(Localize(tag), "", 0, white)` (not in editor
/// mode); every update also writes the area name (`Engine.areaName`: the sector's tag, empty with no name sector; the
/// last name seen is kept: `ui/view.rs`). `AmbianceManager::Reset` (world
/// unload) clears the list and the last name. Type 0 draws per the module doc (dropped while a big window is open or
/// another text is queued). PORT: the target is the player in its own region (the engine's is the camera's target,
/// `Grim Dawn.exe` `CameraController::FollowPlayer`, which follows the player).
#[allow(clippy::too_many_arguments)]
fn area_names(
    mut fresh: gd_game_core::session::NewSession,
    world: Option<Res<crate::world::StreamedMap>>,
    levels: Res<crate::world::Levels>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    player: Query<&Transform, With<gd_game_core::player::Player>>,
    mut seen: Local<(String, Vec<String>)>,
    mut out: MessageWriter<gd_game_core::story::CinematicText>,
) {
    if fresh.begun() {
        *seen = Default::default();
    }
    let (Some(world), Ok(tf)) = (world, player.single()) else {
        return;
    };
    let region = rt.as_ref().and_then(|rt| rt.story.borrow().player.region);
    let entry = match region {
        Some(r) => levels.sector_entry_in(&world.map, r, tf.translation, 1),
        None => levels.sector_entry(&world.map, tf.translation, 1),
    };
    let Some(gd_formats::map::sector::SectorEntry::Name {
        tag,
        flag_a,
        flag_b,
        ..
    }) = entry
    else {
        return;
    };
    let (last, shown) = &mut *seen;
    if last == tag {
        return;
    }
    last.clone_from(tag);
    let mut new = !shown.contains(tag);
    if new {
        shown.push(tag.clone());
    }
    if *flag_b == Some(0) {
        new = true;
    }
    if new && *flag_a != 0 {
        info!("area name: {tag}");
        out.write(gd_game_core::story::CinematicText {
            head: tag.clone(),
            sub: String::new(),
            sub_args: Vec::new(),
            kind: 0,
            front: false,
        });
    }
}

/// A headline node (strip / text) whose rect the grow factor scales about the block centre
/// ([`show`]); built at grow 1.
#[derive(Component)]
struct CinematicGrow {
    base: URect,
    centre: Vec2,
}

/// An additive glow node: its own material (the fade alpha moves its colour in place, the shared
/// [`super::additive::AdditiveCache`] would mint a material per alpha step) and its grow rect.
#[derive(Component)]
struct CinematicGlow {
    mat: Handle<super::additive::AdditiveMaterial>,
    rgb: LinearRgba,
    base_alpha: f32,
    base: URect,
    centre: Vec2,
}

/// A subline node: shown once the fade-in is over ([`show`]).
#[derive(Component)]
struct CinematicSub;

/// `base` scaled by `grow` about `centre` (the banner's grow-in; PORT: a scale about the centre, the
/// engine steps the strips by the unscaled line height).
fn grow_rect(base: URect, centre: Vec2, grow: f32) -> URect {
    URect::new(
        centre.x + (base.x - centre.x) * grow,
        centre.y + (base.y - centre.y) * grow,
        base.w * grow,
        base.h * grow,
    )
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    mut state: ResMut<CinematicQueue>,
    mut events: MessageReader<gd_game_core::story::CinematicText>,
    mut levels: MessageReader<gd_game_core::progress::LeveledUp>,
    mut shake: MessageWriter<gd_game_core::visuals::CameraShake>,
    clock: Res<Time<Real>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    roots: Query<Entity, With<CinematicRoot>>,
    mut assets: UiAssets,
    (mut drawn, mut glow, mut group): (
        Local<
            Option<(
                UVec2,
                u32,
                String,
                String,
                Vec<gd_game_core::floating::TextArg>,
                u32,
            )>,
        >,
        Local<Option<((String, u32, UVec2), Glow)>>,
        Local<Option<Entity>>,
    ),
    windows: BigWindows,
    mut mats: ResMut<Assets<super::additive::AdditiveMaterial>>,
    mut fader: gd_ui::fade::Fader,
    (mut nodes, mut subs): (
        Query<
            (&mut Node, Option<&CinematicGrow>, Option<&CinematicGlow>),
            Or<(With<CinematicGrow>, With<CinematicGlow>)>,
        >,
        Query<&mut Visibility, With<CinematicSub>>,
    ),
) {
    if fresh.begun() {
        *state = CinematicQueue::default();
        *drawn = None;
        *glow = None;
        *group = None;
    }
    // `Character::ReceiveExperience`: type 5, `tagLevelUp` over `tagLevelReached(level)`, once per gain.
    for l in levels.read() {
        state.queue.push_back(gd_game_core::story::CinematicText {
            head: "tagLevelUp".into(),
            sub: "tagLevelReached".into(),
            sub_args: vec![gd_game_core::floating::TextArg::Num(l.level as f32)],
            kind: 5,
            front: false,
        });
    }
    // Type 0 (a challenge area's name): only the frame's last one, and only into an empty queue.
    let mut type0 = None;
    for e in events.read() {
        if e.kind == 0 {
            type0 = Some(e.clone());
            continue;
        }
        if e.front {
            state.queue.push_front(e.clone());
            state.age = 0.0;
            state.shaken = false;
        } else {
            state.queue.push_back(e.clone());
        }
    }
    if let Some(e) = type0.filter(|_| state.queue.is_empty() && !windows.any()) {
        state.queue.push_back(e);
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let clear = |commands: &mut Commands| gd_ui::window::despawn_all(commands, &roots);
    state.age += clock.delta_secs() * 1000.0;
    // The head ends: the next starts this frame.
    while let Some(head) = state.queue.front() {
        let (_, hold, _) = timing(head.kind);
        if state.age < FADE_IN + hold + FADE_OUT {
            break;
        }
        state.age -= FADE_IN + hold + FADE_OUT;
        state.shaken = false;
        state.queue.pop_front();
    }
    let Some(head) = state.queue.front().cloned() else {
        if drawn.take().is_some() {
            clear(&mut commands);
        }
        state.age = 0.0;
        return;
    };
    debug!("cinematic: {:?} age {}", head.head, state.age);
    let (scale, hold, shakes) = timing(head.kind);
    let age = state.age;
    if age >= FADE_IN && shakes && !state.shaken {
        state.shaken = true;
        // `ShakeCamera(0.5, 0.5)` at the start of the hold.
        shake.write(gd_game_core::visuals::CameraShake {
            ms: 500.0,
            amp: 0.5,
            at: Vec3::ZERO,
            own: true,
        });
    }
    let (alpha, grow) = if age < FADE_IN {
        let t = 1.0 - age / FADE_IN;
        (1.0 - t, 1.0 + t)
    } else if age < FADE_IN + hold {
        (1.0, 1.0)
    } else {
        ((FADE_IN + hold + FADE_OUT - age) / FADE_OUT, 1.0)
    };
    // Build once per (screen, scale, kind, headline, subline); the fade alpha and the grow move in place, not by a
    // rebuild per step: the strips / text in a [`gd_ui::fade::FadeGroup`], the additive glow's own
    // material alpha, the grow scaling the rects about the block centre.
    let phase = if age < FADE_IN {
        0
    } else if age < FADE_IN + hold {
        1
    } else {
        2
    };
    let base = assets.scale(size);
    // The subline is in the key: two Level Up banners in a row differ only there.
    let key = (
        size,
        head.kind,
        head.head.clone(),
        head.sub.clone(),
        head.sub_args.clone(),
        base.to_bits(),
    );
    let (w, h) = (size.x as f32, size.y as f32);
    let max_w = (0.45 * w).floor();
    let inv = 1.0 / win.scale_factor();
    let screen = Vec2::new(w, h);
    let centre = Vec2::new(w * 0.5, 0.15 * h);
    if drawn.as_ref() != Some(&key) {
        *drawn = Some(key);
        clear(&mut commands);
        *group = None;
        let root = gd_ui_kit::spawn_root(&mut commands, CinematicRoot, 206);
        commands.entity(root).insert(gd_ui::hud::KeepsShown);
        // Headline (the font size is 32 x scale x `scale`), measured at its final size; the grow factor
        // only stretches the drawn rects.
        let head_text = gd_formats::text::strip_markup(assets.fonts.tag(&head.head));
        let mut ui = assets.ui(&mut commands, &data, root, base * scale, inv, screen);
        let lines = ui.wrap_lines(STYLE, max_w, &head_text);
        let lh = ui.line_height(STYLE);
        let widths: Vec<f32> = lines.iter().map(|l| ui.text_width(STYLE, l)).collect();
        let pad = 10.0 * base;
        let block = Vec2::new(max_w + 2.0 * pad, lh * lines.len() as f32 + 2.0 * pad);
        let block_h = block.y;
        let g = gd_ui::fade::fade_group(&mut ui, root, alpha);
        let top = centre.y - block.y * 0.5;
        let first = (top + pad).floor();
        // The strips: built at grow 1, faded by the group, grown in place.
        for (i, lw) in widths.iter().enumerate() {
            let cy = first + lh * i as f32 + lh * 0.5;
            if let Some((e, r)) = strip(&mut ui, centre.x, cy, *lw, lh, base, 1.0, 1.0) {
                ui.commands
                    .entity(e)
                    .insert(CinematicGrow { base: r, centre });
            }
        }
        let gkey = (head_text.clone(), (base * scale * 1000.0) as u32, size);
        if glow.as_ref().is_none_or(|gg| gg.0 != gkey) {
            *glow = headline_surface(&mut ui, &lines, &widths, lh, block, pad).map(|gg| (gkey, gg));
        }
        let dst = URect::new(
            (centre.x - block.x * 0.5).round(),
            (centre.y - block.y * 0.5).round(),
            block.x.round(),
            block.y.round(),
        );
        let rgb = colour(head.kind);
        match glow.as_ref() {
            // The blurred copy at 0.75 alpha, then the sharp one at 0.35, both additive, each with its
            // own material so the fade moves its alpha in place.
            Some((_, gg)) => {
                for (image, base_alpha) in [(gg.blur.clone(), 0.75), (gg.sharp.clone(), 0.35)] {
                    let e = ui.commands.spawn(ui.node(dst)).id();
                    ui.commands.entity(ui.root).add_child(e);
                    let l = rgb.to_linear();
                    let mat = mats.add(super::additive::AdditiveMaterial {
                        color: LinearRgba {
                            alpha: base_alpha,
                            ..l
                        },
                        image,
                    });
                    ui.commands.entity(e).insert((
                        MaterialNode(mat.clone()),
                        CinematicGlow {
                            mat,
                            rgb: l,
                            base_alpha,
                            base: dst,
                            centre,
                        },
                    ));
                }
            }
            // PORT: a TrueType (FNTD) font is drawn as plain text (faded by the group).
            None => {
                for (i, (l, lw)) in lines.iter().zip(&widths).enumerate() {
                    let y = first + lh * i as f32;
                    ui.text_line_tinted(
                        STYLE,
                        Vec2::new((centre.x - lw * 0.5).round(), y.round()),
                        l,
                        rgb,
                    );
                }
            }
        }
        // Subline: under the headline block, shown once the fade-in is over (32 px, unscaled). Built
        // hidden; `subs` shows it. Per `CinematicText::Render`, its `^X` colour codes apply (over the event
        // colour at 0.75 alpha).
        if !head.sub.is_empty() {
            let sub = gd_ui::floating::format_text(ui.fonts, &head.sub, &head.sub_args);
            // The subline is unscaled (32 px); the headline nodes are already spawned, so reuse the
            // same builder at the base scale (a second `Ui` would double-borrow the assets).
            ui.s = base;
            let sub_lines = ui.wrap_lines(STYLE, max_w, &sub);
            let slh = ui.line_height(STYLE);
            let mut y = 0.15 * h + block_h * 0.5;
            for l in &sub_lines {
                let lw = ui.text_width(STYLE, l);
                if let Some((e, _)) =
                    strip(&mut ui, w * 0.5, y + slh * 0.5, lw, slh, base, 1.0, 1.0)
                {
                    ui.commands
                        .entity(e)
                        .insert((CinematicSub, Visibility::Hidden));
                }
                if let Some(e) = ui.text_line_tinted(
                    STYLE,
                    Vec2::new((w * 0.5 - lw * 0.5).round(), y.round()),
                    l,
                    colour(head.kind).with_alpha(0.75),
                ) {
                    ui.commands
                        .entity(e)
                        .insert((CinematicSub, Visibility::Hidden));
                }
                y += slh;
            }
        }
        gd_ui::fade::seal_fade_group(ui.commands, g);
        *group = Some(g);
    }
    // In place: the fade alpha, the grow, the subline's visibility.
    if let Some(g) = *group {
        fader.set(g, alpha);
    }
    for (mut node, gr, gl) in &mut nodes {
        if let Some(gr) = gr {
            super::set_node_rect(&mut node, grow_rect(gr.base, gr.centre, grow), inv);
        }
        if let Some(gl) = gl {
            if let Some(mut m) = mats.get_mut(&gl.mat) {
                m.color = LinearRgba {
                    alpha: gl.base_alpha * alpha,
                    ..gl.rgb
                };
            }
            super::set_node_rect(&mut node, grow_rect(gl.base, gl.centre, grow), inv);
        }
    }
    for mut vis in &mut subs {
        vis.set_if_neq(if phase != 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// The dark strip behind a line: `(line width + 200) x (line height + 40)` UI px times the grow factor `grow`, white
/// at 0.7 alpha (`s`: the UI scale).
#[allow(clippy::too_many_arguments)]
fn strip(
    ui: &mut super::Ui,
    cx: f32,
    cy: f32,
    lw: f32,
    lh: f32,
    s: f32,
    grow: f32,
    alpha: f32,
) -> Option<(Entity, URect)> {
    let (img, _) = ui.image(STRIP)?;
    let (sw, sh) = ((lw + 200.0 * s) * grow, (lh + 40.0 * s) * grow);
    let r = URect::new((cx - sw * 0.5).round(), (cy - sh * 0.5).round(), sw, sh);
    let e = ui.quad(img, r, None, Color::srgba(1.0, 1.0, 1.0, 0.7 * alpha));
    Some((e, r))
}

/// The headline's surfaces (`CinematicText::Render`): the text and its blur.
struct Glow {
    sharp: Handle<Image>,
    blur: Handle<Image>,
}

/// The blur's eight taps (`GraphicsCanvas::BlurFrameBuffer2` -> Engine.dll `GraphicsCanvas_Blur8Tap`).
const TAPS: [f32; 8] = [0.1, 0.11, 0.14, 0.15, 0.15, 0.14, 0.11, 0.1];

/// The temporary surface the headline is drawn into: opaque black, `block` px, each line white (the one-colour
/// `RenderText2d`, not the style's colours), centred in the block width at `pad + i x lh`; and its blur. None for a
/// TrueType font (no CPU glyphs).
fn headline_surface(
    ui: &mut super::Ui,
    lines: &[String],
    widths: &[f32],
    lh: f32,
    block: Vec2,
    pad: f32,
) -> Option<Glow> {
    let mut style = ui.fonts.style(ui.data, STYLE)?;
    let font = ui.fonts.font(ui.data, &style.font, ui.font_assets)?;
    let super::text::LoadedFont::Bitmap { font, atlas } = &*font else {
        return None;
    };
    style.color0 = [1.0; 3];
    style.color1 = [1.0; 3];
    let size = style.scaled_size(ui.s);
    let (bw, bh) = (
        block.x.round().max(2.0) as usize,
        block.y.round().max(2.0) as usize,
    );
    // Gamma rgb (the engine's UNORM surfaces), alpha 1.
    let mut px = vec![[0f32; 3]; bw * bh];
    for (i, (l, lw)) in lines.iter().zip(widths).enumerate() {
        let Some(ti) = super::text::rasterize(font, atlas, &style, size, l) else {
            continue;
        };
        let Some(src) = ti.image.data.as_ref() else {
            continue;
        };
        let ox = ((block.x - lw) * 0.5).round() + ti.offset.x.round();
        let oy = (pad + lh * i as f32).round() + ti.offset.y.round();
        for y in 0..ti.size.y as usize {
            for x in 0..ti.size.x as usize {
                let (dx, dy) = (ox as isize + x as isize, oy as isize + y as isize);
                if dx < 0 || dy < 0 || dx >= bw as isize || dy >= bh as isize {
                    continue;
                }
                let o = (y * ti.size.x as usize + x) * 4;
                let a = src[o + 3] as f32 / 255.0;
                let d = &mut px[dy as usize * bw + dx as usize];
                for k in 0..3 {
                    d[k] = src[o + k] as f32 / 255.0 * a + d[k] * (1.0 - a);
                }
            }
        }
    }
    let sharp = ui.images.add(rgb_image(&px, bw, bh));
    let (blurred, w2, h2) = blur(&px, bw, bh);
    let blur = ui.images.add(rgb_image(&blurred, w2, h2));
    Some(Glow { sharp, blur })
}

/// `GraphicsCanvas_Blur8Tap` (`BlurFrameBuffer`, scale 1): a vertical pass into a (w, h/2) surface, then a horizontal
/// one into (w/2, h/2), each eight taps of [`TAPS`] at -3.5 ... 3.5 texels of the full-size surface. A half-size
/// pixel's centre lies between two source texels, so the taps fall on texels 2j-3 ... 2j+4 (clamped).
fn blur(px: &[[f32; 3]], w: usize, h: usize) -> (Vec<[f32; 3]>, usize, usize) {
    let (w2, h2) = ((w / 2).max(1), (h / 2).max(1));
    let tap = |i: usize, k: usize, n: usize| (2 * i + k).saturating_sub(3).min(n - 1);
    let mut v = vec![[0f32; 3]; w * h2];
    for j in 0..h2 {
        for x in 0..w {
            let mut c = [0f32; 3];
            for (k, wt) in TAPS.iter().enumerate() {
                let s = px[tap(j, k, h) * w + x];
                (0..3).for_each(|n| c[n] += s[n] * wt);
            }
            v[j * w + x] = c;
        }
    }
    let mut out = vec![[0f32; 3]; w2 * h2];
    for j in 0..h2 {
        for i in 0..w2 {
            let mut c = [0f32; 3];
            for (k, wt) in TAPS.iter().enumerate() {
                let s = v[j * w + tap(i, k, w)];
                (0..3).for_each(|n| c[n] += s[n] * wt);
            }
            out[j * w2 + i] = c;
        }
    }
    (out, w2, h2)
}

/// An opaque image of gamma rgb values (stored as sRGB so the additive pass reads them back unchanged).
fn rgb_image(px: &[[f32; 3]], w: usize, h: usize) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let bytes: Vec<u8> = px
        .iter()
        .flat_map(|c| {
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            [b(c[0]), b(c[1]), b(c[2]), 255]
        })
        .collect();
    Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
