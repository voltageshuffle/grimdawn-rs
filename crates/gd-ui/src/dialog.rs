//! The conversation window (`records/ingameui/conversationwindow/
//! conversationwindow.dbr`): the NPC's name and speech and the list of
//! replies. It shows [`gd_game::conversation::Conversation`] and sends
//! [`ConversationRequest`]s; the rules (talk range, walking away, what is
//! said) are in `gd_game::conversation` and `gd_story::story::dialog`.
//!
//! Layout, `Grim Dawn.exe` (loader `ConversationWindow::Load`, layout `ConversationWindow::Layout`,
//! reply rows `ConversationWindow::BuildReplyRows`, draw `ConversationWindow::Render`):
//! - the record's window rect (640 × 170, centred, 150 above the bottom)
//!   keeps its bottom edge and grows upwards to fit the content:
//!   `content = S + name line + text lines × line height + S + replies`
//!   (`S` = `sectionSpacing`), `inner = max(170 − portrait y − paddingBottom,
//!   content)`, height = `paddingTop` (20) + inner + `paddingBottom` (15);
//! - the content is centred vertically in the window: the name at
//!   (`textBox` x, `h/2 − content/2`), the text one `S` + name line below,
//!   the replies one `S` below the text, `optionSpacing` (0) apart;
//! - a reply row starts at the text box's x and spans the rest of the
//!   window; its height is the larger of its text and its bitmap; the bitmap
//!   is at the row's left, the text after it, centred on the row. The
//!   bitmap follows the reply's step type: Accept `acceptBitmap`, Decline
//!   `declineBitmap`, End `endBitmap`, any other (Continue, link)
//!   `continueBitmap`. A reply without text shows its type's tag;
//! - a page before the last lists only "Continue" (`tagContinue`,
//!   `ConversationWindow::AddContinueButton`); a speech without replies gets "End Conversation"
//!   (`ConversationWindow::AddEndButton`);
//! - the background is a `bgColor` rect over the window, the border the
//!   eight `*Texture` bitmaps around it ([`Ui::border`]); the close button
//!   (`closeButton`) sits `bitmapPosition` from the top-right corner and plays
//!   its record's `soundNameDown` on press (the shared `ButtonSound`,
//!   `ui/widgets.rs`); the reply rows are drawn rows, not button records, and
//!   have no click sound.
//!
//! - fade (update `ConversationWindow::Update`, draw `ConversationWindow::Render`): opening fades the
//!   background, border and reply rows in at 0.002 alpha per ms, closing
//!   fades them out (the name and speech text are drawn opaque); no input
//!   for 150 s (`Engine::IsUserIdle`) closes the conversation.
//!   PORT (a performance choice): the fading parts are built once in a [`FadeGroup`] whose alpha the
//!   live pass moves, and every reply row's hover rect is built and shown in place: no rebuild per
//!   fade step or hover change.
//!
//! - name line (layout `ConversationWindow::Layout`): the NPC's faction icon
//!   ([`Conversation::faction_icon`], shown when its visible faction is not
//!   -1000) at the text box's x, centred on the name line, then the name
//!   3 px (unscaled) after it. With a faction-locked quest step in the
//!   NPC's conversation ([`Conversation::locked_quests`],
//!   `Story::locked_quest_faction`) the locked-quests icon
//!   ([`LOCKED_ICON`], loader `ConversationWindow::Load`) is drawn there instead: the
//!   layout skips the faction icon when the lock value is set.
//! - portrait (`portrait`, a `UISceneView`, loader `UISceneView::Load`,
//!   render `UISceneView::Render`, open `ConversationWindow::BeginConversation` / `UISceneView::SetTarget`): its rect
//!   keeps its distance from the window's bottom (y = window h − (record h −
//!   `y`)), drawn after the replies. The target = the NPC's `targetPointName`
//!   point (`HeadEffect`), re-evaluated from its current pose every frame (the portrait
//!   follows the NPC sitting/standing/animating); eye = target + R·`targetOffset`,
//!   R = rotation about Y by `portraitAngle` (degrees) + π/2 −
//!   atan2(forward.z, forward.x), recomputed every frame from the NPC's
//!   current facing (update `ConversationWindow::Update`, `UISceneView.targetFacing`), so the
//!   camera stays in front of the NPC while it turns to the player; looking
//!   at the target with up +Y, `cameraFov` / near / far. The render clears
//!   the NPC's highlight flag first (`Entity::SetHighlight(false)`): no
//!   hover highlight in the portrait (PORT: the mesh tag's flag bit, which
//!   a doll view ignores, `render/light_rig.rs`). The scene holds only the NPC (`GraphicsScene::AddRegion(region, false)`
//!   adds no region content, then the entity adds itself), live: it plays
//!   its animation inside the fixed frame. `worldLighting` 0: the record's
//!   light rig (one directional light along R·`lightDirection`, sky /
//!   ground ambient), no shadows, fog, post effects or light rigs;
//!   `worldBackground` 1: background mode 1 = a vertical gradient over the
//!   view from the world's sky ambient to its ground ambient
//!   (`AmbianceManager::GetSky/GroundAmbientColor`,
//!   `GraphicsMTRenderer::RenderBackground`).
//!
//! Port: the portrait is a camera on its own render layer ([`PORTRAIT_LAYER`],
//! added to the NPC's meshes and to the emitter meshes of the effects attached
//! to it, its gear's, while the window is open) rendering into a small image
//! drawn by the doll's material (its nodes stay hidden until the camera has
//! run two frames, [`Portrait::warm`]: the target is empty before its first
//! render) (`crate::doll`, its light rig in the camera's `ColorGrading`); the
//! camera is inactive and the NPC untagged while no conversation is open (no
//! cost then). The target point is the attach point in the NPC's current pose each frame
//! (PORT: tracked live, not frozen at open, so a sitting NPC keeps its head framed); the camera and background move with the facing. The background is drawn into the target like the
//! engine's `RenderBackground`: a quad behind the NPC with the gradient as an
//! unlit texture ([`background_assets`]), so the target is opaque and blended
//! parts (Mazaan's hood) and particles composite on it. The scene glow runs in the portrait too
//! (Engine.dll `GraphicsDeferredRenderer::RenderFinalScene` calls `RenderSceneGlow`
//! unconditionally; `SetPostEffectsEnabled(false)` only stops `RenderPostEffects`): its own glow camera and the
//! main view's composite ([`portrait_glow`]). PORT: the portrait target is sRGB, so the composite adds in linear
//! values there (the engine adds gamma values).
//! PORT: the gradient's top is the sky colour (assumed from the argument
//! order); the portrait has the window's border around it; the
//! faction icon's hover tooltip ([`faction_lines`]) is drawn by `rollover.rs`.
//! `ConversationReplyRow::Render` (row draw) / `ConversationWindow::Render` / `ConversationWindow::OnButtonClicked`:
//! the hovered reply row (the first whose rect contains the cursor) gets a `hoverColor` rect over the whole row rect,
//! alpha times the fade; nothing else changes on hover. `ConversationWindow::UpdateStep` /
//! `ConversationWindow::CollectSpeeches` / `ScriptableConditionCollection::Evaluate`:
//! a reply with an unmet condition is not listed at all (no grey text), see
//! `Story::advance`. The speech and reply texts keep their `^X` colour markup: `Ui::wrap_lines` carries the active
//! colour over line breaks and `Ui::text_line` draws it (`gd_formats::ui::layout_text`).
use super::text::FontCache;
use super::window::Rebuild;
use crate::fade::{Fader, fade_group, seal_fade_group};

/// `ConversationWindow::Load`: the window's locked-quests icon.
const LOCKED_ICON: &str = "ui/conversationwindow/icon_lockedquests.tex";
use super::contains;
use crate::doll::{DollMaterial, SceneView};
use crate::state::inventory::UiCapture;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};
use gd_game::conversation::{Conversation, ConversationRequest};
use gd_game_core::data::GameData;
use gd_render::render::material::GdMaterial;
use gd_story::story::DialogView;

pub const RECORD: &str = "records/ingameui/conversationwindow/conversationwindow.dbr";

pub struct DialogUiPlugin;

impl Plugin for DialogUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialogLayout>()
            .init_resource::<Portrait>()
            .add_systems(
                Update,
                (portrait, portrait_reveal)
                    .chain()
                    .after(build)
                    .in_set(gd_ui_kit::UiPlaySet),
            );
        // Live: a conversation is open, or the window is still fading out (its root exists).
        super::window::add_window(
            app,
            build,
            clicks,
            |conv: Res<Conversation>, roots: Query<(), With<DialogRoot>>| {
                conv.view.is_some() || !roots.is_empty()
            },
        );
    }
}

/// What a click on a reply row does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    Choose(u32),
    NextPage,
}

/// The built window (physical px): its rect, the reply rows and the close
/// button. Empty while no conversation is open.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct DialogLayout {
    pub window: Option<URect>,
    pub rows: Vec<(URect, Row)>,
    pub close: Option<URect>,
    /// The name line's faction / locked-quests icon (its hover shows the faction tooltip).
    pub icon: Option<URect>,
}

#[derive(Component)]
struct DialogRoot;

/// The portrait's gradient and view nodes: hidden until the camera has
/// rendered the NPC once ([`Portrait::warm`]).
#[derive(Component)]
struct PortraitNode;

/// The reply rows of a view: (text, bitmap field, action).
fn rows(view: &DialogView, fonts: &FontCache) -> Vec<(String, &'static str, Row)> {
    if !view.on_last_page() {
        return vec![(
            fonts.tag("tagContinue").to_string(),
            "continueBitmap",
            Row::NextPage,
        )];
    }
    view.responses
        .iter()
        .map(|(step, text, tag)| {
            let bitmap = match *tag {
                "tagAccept" => "acceptBitmap",
                "tagDecline" => "declineBitmap",
                "tagEndConversation" => "endBitmap",
                _ => "continueBitmap",
            };
            let text = if text.is_empty() {
                fonts.tag(tag).to_string()
            } else {
                text.clone()
            };
            (text, bitmap, Row::Choose(*step))
        })
        .collect()
}

/// Speech text as the window shows it: `{^n}` / `^n` become line breaks, the other markup stays for the text
/// drawing.
fn plain(text: &str) -> String {
    text.replace("{^n}", "\n").replace("^n", "\n")
}

/// The window's fade (module doc): its alpha and the view it shows (kept
/// while fading out after the conversation ended).
#[derive(Default)]
struct Fade {
    alpha: f32,
    view: Option<DialogView>,
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    conv: Res<Conversation>,
    time: Res<Time<Real>>,
    mut fade: Local<Fade>,
    roots: Query<Entity, With<DialogRoot>>,
    mut layout: ResMut<DialogLayout>,
    mut built: Local<Rebuild<(u64, UVec2, u32)>>,
    // The built fade group and its rows' hover rects.
    mut live: Local<Option<(Entity, Vec<Entity>)>>,
    (mut portrait, mut mats): (ResMut<Portrait>, ResMut<Assets<DollMaterial>>),
    mut assets: gd_ui_kit::UiAssets,
    mut fader: Fader,
    mut vis: Query<&mut Visibility>,
    (pause, popup): (
        Res<gd_game_core::pause::GamePause>,
        Res<super::questpopup::QuestPopup>,
    ),
) {
    if fresh.begun() {
        *fade = Default::default();
        built.reset();
        *live = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    // No row hover under the game menu / Options or under the quest reward window (it sits over this one).
    let cursor = super::cursor_px(&assets.hooks, win).filter(|p| !pause.menu && !popup.covers(*p));
    let hover = cursor.and_then(|p| layout.rows.iter().position(|(r, _)| contains(r, p)));
    // Fade in on opening, out after closing, 0.002 per ms (`ConversationWindow::Update`).
    let step = time.delta_secs() * 1000.0 * 0.002;
    match &conv.view {
        Some(v) => {
            if fade.view.is_none() {
                fade.alpha = 0.0;
            }
            if fade.view.as_ref() != Some(v) {
                fade.view = Some(v.clone());
            }
            fade.alpha = (fade.alpha + step).min(1.0);
        }
        None if fade.view.is_some() => {
            fade.alpha -= step;
            if fade.alpha <= 0.0 {
                *fade = Fade::default();
            }
        }
        None => {}
    }
    let closing = conv.view.is_none();
    let a = fade.alpha;
    if fade.view.is_none() {
        // The fade-out is over: the window goes (the live gate stops this system next frame).
        super::window::despawn_all(&mut commands, &roots);
        *layout = DialogLayout::default();
        built.reset();
        *live = None;
        return;
    }
    if size.x == 0 {
        return;
    }
    // The fade and the hover are not in the signature: they move in place (the fade group's alpha, the
    // hover rects' visibility), no rebuild per fade step or hover change. The UI scale is: an applied UI
    // Scale rebuilds.
    let sig = (conv.version, size, assets.scale(size).to_bits());
    if !built.changed(sig) {
        if let Some((group, hovers)) = live.as_ref() {
            fader.set(*group, a);
            for (i, e) in hovers.iter().enumerate() {
                if let Ok(mut v) = vis.get_mut(*e) {
                    v.set_if_neq(if hover == Some(i) {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    });
                }
            }
        }
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    *layout = DialogLayout::default();
    *live = None;
    let (Some(view), Some(rec)) = (&fade.view, data.db.get(RECORD)) else {
        return;
    };
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(&mut commands, DialogRoot, super::window::Z_WINDOW);
    let faction_icon = conv
        .locked_quests
        .then(|| LOCKED_ICON.to_string())
        .or_else(|| conv.faction_icon.clone())
        .or_else(|| {
            if closing {
                portrait.faction_icon.clone()
            } else {
                None
            }
        });
    portrait.faction_icon = faction_icon.clone();
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    let int = |k: &str, d: f32| rec.f32(k).unwrap_or(d) * s;
    let base = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, s);
    let name_box = ui
        .record(&field("nameBox"))
        .map(|r| TextBox::from_record(&r))
        .unwrap_or_default();
    let text_box = ui
        .record(&field("textBox"))
        .map(|r| TextBox::from_record(&r))
        .unwrap_or_default();
    let name_style = name_box.style.clone().unwrap_or_default();
    let text_style = text_box.style.clone().unwrap_or_default();
    let option_style = field("optionStyle");
    let (section, option_gap) = (
        (int("sectionSpacing", 0.0)).trunc(),
        (int("optionSpacing", 0.0)).trunc(),
    );
    let (pad_top, pad_bottom) = (int("paddingTop", 20.0), int("paddingBottom", 15.0));
    let portrait_y = ui
        .record(&field("portrait"))
        .and_then(|r| r.f32("y"))
        .unwrap_or(0.0)
        * s;
    let text_x = ui.sc(text_box.rect.x);
    // Content heights.
    // `ConversationWindow::Layout`: the content height counts each text line at the style object's (UI-scaled)
    // `fontSize` (`StyleManager::LoadStyle`), not the font's glyph line height. The body and the faction icon sit
    // from the name widget's box height (`UITextBox.boxHeight` of `ConversationWindow.nameText`, loader
    // `UITextBox::Load`: `textBoxYSize` × UI scale, rounded `floor(x + 0.5)`; 30 in `conversationname.dbr`).
    let name_h = ui.font_px(&name_style);
    let name_lh = ui.sc(name_box.rect.h);
    let text_lines = ui.wrap_lines(
        &text_style,
        ui.sc(text_box.rect.w),
        &plain(view.page_text()),
    );
    let text_lh = ui.font_px(&text_style);
    let text_h = text_lines.len() as f32 * text_lh;
    let opt_lh = ui.line_height(&option_style);
    let mut replies = Vec::new();
    let mut replies_h = 0.0;
    for (i, (text, bitmap, row)) in rows(view, ui.fonts).into_iter().enumerate() {
        let icon = ui.image(&field(bitmap));
        let icon_w = icon.as_ref().map_or(0.0, |(_, sz)| ui.sc(sz.x as f32));
        let icon_h = icon.as_ref().map_or(0.0, |(_, sz)| ui.sc(sz.y as f32));
        let lines = ui.wrap_lines(&option_style, base.w - text_x - icon_w, &plain(&text));
        let h = (lines.len() as f32 * opt_lh).max(icon_h);
        if i > 0 {
            replies_h += option_gap;
        }
        replies_h += h;
        replies.push((lines, icon, icon_w, h, row));
    }
    let content = section + name_h + text_h + section + replies_h;
    let inner = (base.h - portrait_y - pad_bottom).max(content);
    let h = (pad_top + inner + pad_bottom).round();
    let r = URect::new(base.x, base.y + base.h - h, base.w, h);
    // The fading parts (background, border, reply rows, portrait border) in one fade group, built as if
    // the window were opaque; the group's alpha is the fade. The name, the speech, the faction icon, the
    // portrait view and the close button are drawn outside it, opaque (module doc).
    let group = fade_group(&mut ui, root, a);
    // Background and border.
    let c = |k: &str| rec.f32(k).unwrap_or(0.0) / 255.0;
    ui.solid(
        r,
        Color::srgba(
            c("bgColor.R"),
            c("bgColor.G"),
            c("bgColor.B"),
            c("bgColor.A"),
        ),
    );
    ui.border(rec, "", r, 1.0);
    let top = (r.y + h * 0.5 - content * 0.5).round();
    let hover_color = Color::srgba(
        c("hoverColor.R"),
        c("hoverColor.G"),
        c("hoverColor.B"),
        c("hoverColor.A"),
    );
    let mut hovers = Vec::new();
    let mut y = top + section + name_lh + text_h + section;
    for (i, (lines, icon, icon_w, rh, row)) in replies.into_iter().enumerate() {
        if i > 0 {
            y += option_gap;
        }
        let row_r = URect::new(r.x + text_x, y, r.w - text_x, rh);
        // Every row's hover rect is built; the live pass shows the hovered one (`ConversationReplyRow::Render`: its
        // alpha is the hover colour's times the fade).
        let he = ui.solid(row_r, hover_color);
        ui.commands.entity(he).insert(if hover == Some(i) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        hovers.push(he);
        if let Some((img, sz)) = icon {
            let (iw, ih) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
            ui.quad(
                img,
                URect::new(row_r.x, row_r.y, iw, ih),
                None,
                Color::WHITE,
            );
        }
        let mut ty = (row_r.y + rh * 0.5 - lines.len() as f32 * opt_lh * 0.5).round();
        for l in &lines {
            ui.text_line(&option_style, Vec2::new(row_r.x + icon_w, ty), l, 1.0);
            ty += opt_lh;
        }
        if !closing {
            layout.rows.push((row_r, row));
        }
        y += rh;
    }
    // Portrait: the border fades with the window; the view itself is opaque.
    let node_vis = portrait_visibility(portrait.warm);
    let mut portrait_rect = None;
    if let Some(pr) = ui.record(&field("portrait")) {
        let f = |k: &str| pr.f32(k).unwrap_or(0.0);
        let pw = ui.sc(f("xSize")).round().max(1.0);
        let ph = ui.sc(f("ySize")).round().max(1.0);
        let rect = URect::new(
            (r.x + ui.sc(f("x"))).round(),
            (r.y + h - (base.h - portrait_y)).round(),
            pw,
            ph,
        );
        let size = UVec2::new(pw as u32, ph as u32);
        // PORT: the black ragged frame of the real game: the window's border textures
        // around the portrait rect (the engine's draw of it was not decompiled).
        ui.border(rec, "", rect, 1.0);
        if portrait.target.as_ref().is_none_or(|t| t.0 != size) {
            let image = ui.images.add(Image::new_target_texture(
                size.x,
                size.y,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            let material = mats.add(DollMaterial {
                image: image.clone(),
            });
            portrait.target = Some((size, image, material));
        }
        portrait_rect = Some(rect);
    }
    seal_fade_group(ui.commands, group);
    ui.root = root;
    *live = Some((group, hovers));
    // Faction icon before the name, centred on the name line (module doc), then the name and the speech.
    let mut name_x = r.x + text_x;
    let mut icon_rect = None;
    if let Some((img, sz)) = faction_icon.and_then(|p| ui.image(&p)) {
        let (iw, ih) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
        let ir = URect::new(
            name_x,
            (top + (name_lh * 0.5).trunc() - ih * 0.5).round(),
            iw,
            ih,
        );
        ui.quad(img, ir, None, Color::WHITE);
        icon_rect = Some(ir);
        name_x += iw + 3.0;
    }
    ui.text_line(&name_style, Vec2::new(name_x, top), &view.speaker, 1.0);
    let mut y = top + section + name_lh;
    for l in &text_lines {
        ui.text_line(&text_style, Vec2::new(r.x + text_x, y), l, 1.0);
        y += text_lh;
    }
    if let (Some(rect), Some((_, _, m))) = (portrait_rect, &portrait.target) {
        let e = ui
            .commands
            .spawn((
                ui.node(rect),
                MaterialNode(m.clone()),
                PortraitNode,
                node_vis,
            ))
            .id();
        ui.commands.entity(ui.root).add_child(e);
    }
    // Close button: `bitmapPosition` in from the top-right corner.
    if let Some(b) = ui.record(&field("closeButton"))
        && let Some((img, sz)) = b.str("bitmapNameUp").and_then(|p| ui.image(p))
    {
        let (bw, bh) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
        let br = URect::new(
            r.x + r.w - bw - ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
            r.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
            bw,
            bh,
        );
        ui.quad(img, br, None, Color::WHITE);
        if !closing {
            ui.record_button_sound(br, &b);
        }
        layout.close = (!closing).then_some(br);
    }
    layout.window = Some(r);
    layout.icon = icon_rect.filter(|_| !closing);
}

/// The faction icon's rollover lines, `ConversationWindow::GetFactionRollover`: `tagFaction` (the faction's name) in
/// the record's `rolloverTitleStyle`, `tagFactionStanding` (the player's rank) in `rolloverTextStyle`, and, with a
/// faction-locked quest, a space and `tagFactionQuestReq` (the missing standing, signed) in the same style. Shown by the
/// shared rollover window (`rollover.rs`: the record's `rolloverStyle` box, mode 2 at the cursor like the other window
/// widgets' rollovers, `UIWindow::DrawRollover`).
pub fn faction_lines(
    ui: &super::Ui,
    rec: &gd_formats::arz::Record,
    tip: &gd_game::factions::FactionTip,
) -> Vec<(String, String, bool)> {
    use gd_game_core::floating::TextArg;
    let fmt = |tag: &str, a: Vec<TextArg>| super::floating::format_text(ui.fonts, tag, &a);
    let mut standing = fmt(
        "tagFactionStanding",
        vec![TextArg::Tag(tip.rank_tag.clone())],
    );
    if let Some(req) = tip.quest_req {
        standing = format!(
            "{standing} {}",
            fmt("tagFactionQuestReq", vec![TextArg::Num(req as f32)])
        );
    }
    vec![
        (
            rec.str("rolloverTitleStyle").unwrap_or("").to_string(),
            fmt("tagFaction", vec![TextArg::Tag(tip.name_tag.clone())]),
            false,
        ),
        (
            rec.str("rolloverTextStyle").unwrap_or("").to_string(),
            standing,
            false,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
/// Clicks on the window: a reply row answers, the close button closes; the
/// window takes the click (no walking). `GD_DIALOG_CLICK` (hooks.rs).
pub fn clicks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<DialogLayout>,
    conv: Res<Conversation>,
    time: Res<Time>,
    mut capture: ResMut<UiCapture>,
    mut out: MessageWriter<ConversationRequest>,
    mut opened: Local<Option<(u64, f32)>>,
    // The conversations the test hook has seen open (`GD_DIALOG_CLICK` rows by conversation).
    mut seen: Local<(bool, usize)>,
    (keys, motion, real, mut idle): (
        Res<ButtonInput<KeyCode>>,
        Res<bevy::input::mouse::AccumulatedMouseMotion>,
        Res<Time<Real>>,
        Local<f32>,
    ),
    (pause, popup): (
        Res<gd_game_core::pause::GamePause>,
        Res<super::questpopup::QuestPopup>,
    ),
    mut fresh: gd_game_core::session::NewSession,
) {
    // The hook's conversation count and click state describe the last session's conversations.
    if fresh.begun() {
        *opened = None;
        *seen = Default::default();
        *idle = 0.0;
    }
    // Idle close: no input for 150 s closes it (`Engine::IsUserIdle(150000)`,
    // `ConversationWindow::Update`).
    if keys.get_pressed().next().is_some()
        || buttons.get_pressed().next().is_some()
        || motion.delta != Vec2::ZERO
        || conv.view.is_none()
    {
        *idle = 0.0;
    } else {
        *idle += real.delta_secs();
        if *idle >= 150.0 {
            *idle = 0.0;
            out.write(ConversationRequest::Close);
        }
    }
    // The conversations opened so far (the hook's rows go by conversation).
    let open_now = conv.view.is_some();
    if open_now && !seen.0 {
        seen.1 += 1;
    }
    seen.0 = open_now;
    let Some(wr) = layout.window else {
        *opened = None;
        return;
    };
    // `GD_DIALOG_CLICK`: a scripted click on a row, once per window state.
    if let Some((rows, secs)) = &hooks.ui.dialog_click {
        let (secs, nth) = (*secs, seen.1.saturating_sub(1));
        let row = rows[nth.min(rows.len() - 1)];

        let since = opened.get_or_insert((conv.version, time.elapsed_secs()));
        if since.0 != conv.version {
            *since = (conv.version, time.elapsed_secs());
        } else if time.elapsed_secs() - since.1 >= secs && since.1 >= 0.0 {
            // A missing row (fewer replies on this page) is retried every frame, not dropped.
            if let Some((_, r)) = layout.rows.get(row) {
                since.1 = -1.0;
                info!("dialog test: clicking row {row}");
                out.write(match r {
                    Row::Choose(s) => ConversationRequest::Choose(*s),
                    Row::NextPage => ConversationRequest::NextPage,
                });
            } else if time.elapsed_secs() - since.1 < secs + 0.1 {
                warn!("dialog test: no row {row} ({} rows)", layout.rows.len());
            }
        }
    }
    let Ok(win) = window.single() else { return };
    // The game menu / Options holds the mouse; the quest reward window over this one takes its own presses.
    let Some(p) = super::cursor_px(&hooks, win).filter(|p| !pause.menu && !popup.covers(*p)) else {
        return;
    };
    if !contains(&wr, p) && !layout.close.is_some_and(|c| contains(&c, p)) {
        return;
    }
    capture.0 = true;
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if layout.close.is_some_and(|c| contains(&c, p)) {
        out.write(ConversationRequest::Close);
    } else if let Some((_, r)) = layout.rows.iter().find(|(r, _)| contains(r, p)) {
        out.write(match r {
            Row::Choose(s) => ConversationRequest::Choose(*s),
            Row::NextPage => ConversationRequest::NextPage,
        });
    }
}

fn portrait_visibility(warm: u8) -> Visibility {
    if warm >= 2 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Shows the portrait nodes once the camera has rendered.
fn portrait_reveal(p: Res<Portrait>, mut nodes: Query<&mut Visibility, With<PortraitNode>>) {
    for mut v in &mut nodes {
        v.set_if_neq(portrait_visibility(p.warm));
    }
}

/// The portrait's render layer: the NPC's meshes carry it while the window
/// is open (module doc).
pub const PORTRAIT_LAYER: usize = 24;
/// The portrait's glow view: the tagged meshes that the main glow view draws ([`PortraitGlow`]).
const PORTRAIT_GLOW_LAYER: usize = 25;
/// The portrait's glow composite triangle (the portrait camera only).
const PORTRAIT_COMPOSITE_LAYER: usize = 26;

/// The portrait's scene glow (module doc): its glow camera, composite triangle and target.
struct PortraitGlow {
    camera: Entity,
    composite: Entity,
    size: UVec2,
}

/// The conversation portrait's offscreen view (module doc).
#[derive(Resource, Default)]
pub struct Portrait {
    /// The NPC shown (entity, story id); its target point is re-evaluated live each frame.
    shot: Option<(Entity, u32)>,
    /// The render target (its size) and its UI material.
    target: Option<(UVec2, Handle<Image>, Handle<DollMaterial>)>,
    camera: Option<Entity>,
    /// Meshes given [`PORTRAIT_LAYER`].
    tagged: Vec<Entity>,
    /// The background quad (`worldBackground`) and its gradient image, with the (sky, ground)
    /// texels it holds.
    background: Option<(Entity, Handle<Image>, [[u8; 4]; 2])>,
    /// Frames the camera has run for the current NPC; the portrait nodes show
    /// from the second on (the target image is empty before its first render:
    /// a grey square for the window's first frames).
    warm: u8,
    /// The icon shown, kept while the window fades out.
    faction_icon: Option<String>,
    glow: Option<PortraitGlow>,
}

/// The target point of a `UISceneView` on a character, in its current pose, world space (module doc).
fn portrait_target(
    sv: &SceneView,
    rec: &gd_formats::arz::Record,
    gt: &GlobalTransform,
    ch: &gd_render::character::Character,
) -> Vec3 {
    // The point in the NPC's current pose (module doc).
    let point = rec
        .str("targetPointName")
        .filter(|n| !n.is_empty())
        .and_then(|n| ch.posed_point(n))
        .unwrap_or(sv.target_point);
    gt.transform_point(point)
}

/// The camera frame of a `UISceneView` on a character, each frame from its target and the NPC's current
/// facing (module doc).
fn portrait_frame(sv: &SceneView, target: Vec3, angle: f32, gt: &GlobalTransform) -> Transform {
    let f = gt.affine().transform_vector3(Vec3::Z);
    let rot = angle + std::f32::consts::FRAC_PI_2 - f.z.atan2(f.x);
    Transform::from_translation(target + Quat::from_rotation_y(rot) * sv.target_offset)
        .looking_at(target, Vec3::Y)
}

/// Takes [`PORTRAIT_LAYER`] and [`PORTRAIT_GLOW_LAYER`] off the `tagged` meshes still alive.
fn untag(
    commands: &mut Commands,
    layers: &Query<Option<&RenderLayers>, With<Mesh3d>>,
    tagged: Vec<Entity>,
) {
    for t in tagged {
        if let Ok(Some(l)) = layers.get(t) {
            commands.entity(t).try_insert(
                l.clone()
                    .without(PORTRAIT_LAYER)
                    .without(PORTRAIT_GLOW_LAYER),
            );
        }
    }
}

/// Keeps the portrait camera on the conversation's NPC while the window is
/// open; turns it off and untags the NPC once it closes or the conversation moves to another NPC.
#[allow(clippy::too_many_arguments)]
fn portrait(
    mut commands: Commands,
    conv: Res<Conversation>,
    data: Res<GameData>,
    mut p: ResMut<Portrait>,
    actors: Query<(
        Entity,
        &gd_game_core::story::StoryActor,
        &GlobalTransform,
        &gd_render::character::Character,
        Option<&gd_render::character::Body>,
    )>,
    children: Query<&Children>,
    layers: Query<Option<&RenderLayers>, With<Mesh3d>>,
    effects: Query<&gd_render::fx::Effect>,
    (mut materials, mut images, mut meshes, defaults, amb, mut glow_mats): (
        ResMut<Assets<GdMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<Mesh>>,
        Res<gd_render::render::GdDefaults>,
        Option<Res<gd_render::render::env::Ambiance>>,
        ResMut<Assets<gd_render::render::post::GlowComposite>>,
    ),
    mut cams: Query<(&mut Camera, &RenderTarget)>,
    mut placed: Query<&mut Transform>,
) {
    let npc = conv.view.as_ref().map(|v| v.npc);
    let found = npc.and_then(|id| actors.iter().find(|a| a.1.0 == id));
    let Some((e, actor, gt, ch, body)) = found else {
        let glow_cam = p.glow.as_ref().map(|g| g.camera);
        for c in p.camera.into_iter().chain(glow_cam) {
            if let Ok((mut cam, _)) = cams.get_mut(c)
                && cam.is_active
            {
                cam.is_active = false;
            }
        }
        untag(&mut commands, &layers, std::mem::take(&mut p.tagged));
        if let Some((bg, ..)) = p.background.take() {
            commands.entity(bg).try_despawn();
        }
        p.shot = None;
        p.warm = 0;
        return;
    };
    let (Some(win), Some(target)) = (data.db.get(RECORD), p.target.clone()) else {
        return;
    };
    let Some(rec) = win.str("portrait").and_then(|r| data.db.get(r)) else {
        return;
    };
    let sv = SceneView::from_record(rec);
    let fresh = p.shot.as_ref().is_none_or(|s| s.0 != e || s.1 != actor.0);
    if fresh {
        // Another NPC: the previous one's meshes leave the portrait.
        let old = std::mem::take(&mut p.tagged);
        untag(&mut commands, &layers, old);
        p.shot = Some((e, actor.0));
        p.warm = 0;
    }
    // Tag the NPC's meshes (parts spawned later included) and the emitter meshes of the effects
    // attached to it (`fx::items`: its gear's; they are top-level entities of their own).
    let tagged = &mut p.tagged;
    let mut tag = |m: Entity| {
        let Ok(l) = layers.get(m) else { return };
        let l = l.cloned().unwrap_or_default();
        // The hover outline's copies (glow view only) stay out.
        if l == RenderLayers::layer(gd_render::render::post::GLOW_LAYER)
            || l.intersects(&RenderLayers::layer(PORTRAIT_LAYER))
        {
            return;
        }
        // A mesh the main glow view draws is in the portrait's glow view too.
        let glows = l.intersects(&RenderLayers::layer(gd_render::render::post::GLOW_LAYER));
        let l = l.with(PORTRAIT_LAYER);
        let l = if glows {
            l.with(PORTRAIT_GLOW_LAYER)
        } else {
            l
        };
        commands.entity(m).try_insert(l);
        tagged.push(m);
    };
    let roots = std::iter::once(e).chain(
        body.into_iter()
            .flat_map(|b| b.weapons.iter().map(|w| w.root)),
    );
    for r in roots {
        for m in std::iter::once(r).chain(children.iter_descendants(r)) {
            tag(m);
            if let Ok(fx) = effects.get(m) {
                for d in fx.emitters.iter().filter_map(|em| em.draw.as_ref()) {
                    tag(d.entity);
                }
            }
        }
    }
    let angle = win.f32("portraitAngle").unwrap_or(0.0).to_radians();
    let tf = portrait_frame(&sv, portrait_target(&sv, rec, gt, ch), angle, gt);
    let proj = Projection::Perspective(PerspectiveProjection {
        fov: sv.fov,
        near: sv.near,
        far: sv.far,
        aspect_ratio: target.0.x as f32 / target.0.y as f32,
        ..default()
    });
    match p.camera.and_then(|c| cams.get_mut(c).ok()) {
        Some((mut cam, rt)) => {
            if cam.is_active && !fresh {
                p.warm = p.warm.saturating_add(1);
            }
            let retarget = !matches!(rt, RenderTarget::Image(i) if i.handle == target.1);
            if !cam.is_active || fresh || retarget {
                cam.is_active = true;
                commands.entity(p.camera.unwrap()).insert((
                    tf,
                    proj.clone(),
                    RenderTarget::Image(target.1.clone().into()),
                ));
            }
        }
        None => {
            p.camera = Some(
                commands
                    .spawn((
                        Camera3d::default(),
                        Camera {
                            order: -3,
                            clear_color: ClearColorConfig::Custom(Color::NONE),
                            ..default()
                        },
                        RenderTarget::Image(target.1.clone().into()),
                        Tonemapping::None,
                        DebandDither::Disabled,
                        Msaa::Off,
                        // No point lights in the portrait (`shade_doll`): one cluster.
                        bevy::light::cluster::ClusterConfig::Single,
                        RenderLayers::layer(PORTRAIT_LAYER).with(PORTRAIT_COMPOSITE_LAYER),
                        proj.clone(),
                        // Constant per record (Bevy warns on every change).
                        sv.grading(),
                        tf,
                        Name::new("conversation portrait camera"),
                    ))
                    .id(),
            );
        }
    }
    portrait_glow(
        &mut commands,
        &mut p,
        (target.0, tf, &proj, fresh),
        (&mut images, &mut meshes, &mut glow_mats),
        &mut cams,
    );
    // The frame follows the NPC's facing (module doc).
    let glow_cam = p.glow.as_ref().map(|g| g.camera);
    for c in p.camera.into_iter().chain(glow_cam) {
        if let Ok(mut t) = placed.get_mut(c) {
            t.set_if_neq(tf);
        }
    }
    // `worldBackground`: the sky-to-ground gradient drawn into the target behind the NPC.
    if rec.f32("worldBackground").unwrap_or(0.0) == 0.0 {
        return;
    }
    let (sky, ground) = amb
        .as_deref()
        .map_or((Vec3::ZERO, Vec3::ZERO), |a| (a.sky, a.ground));
    let px = |v: Vec3| {
        let c = (v.clamp(Vec3::ZERO, Vec3::ONE) * 255.0).round();
        [c.x as u8, c.y as u8, c.z as u8, 255]
    };
    let texels = [px(sky), px(ground)];
    // Far enough to be behind the NPC, inside the far plane.
    let dist = sv.far * 0.8;
    let place = tf * Transform::from_xyz(0.0, 0.0, -dist);
    match &mut p.background {
        Some((bg, img, held)) => {
            if *held != texels {
                *held = texels;
                if let Some(mut i) = images.get_mut(&*img) {
                    i.data = Some(texels.concat());
                }
            }
            if let Ok(mut t) = placed.get_mut(*bg) {
                t.set_if_neq(place);
            }
        }
        None => {
            let (image, quad, material) = background_assets(&sv, target.0, dist, texels, &defaults);
            let image = images.add(image);
            let material = materials.add(GdMaterial {
                base: image.clone(),
                ..material
            });
            let bg = commands
                .spawn((
                    Mesh3d(meshes.add(quad)),
                    MeshMaterial3d(material),
                    place,
                    Visibility::default(),
                    RenderLayers::layer(PORTRAIT_LAYER),
                    bevy::camera::visibility::NoFrustumCulling,
                    bevy::light::NotShadowCaster,
                    Name::new("conversation portrait background"),
                ))
                .id();
            p.background = Some((bg, image, texels));
        }
    }
}

/// The portrait's scene glow (module doc): a glow camera like the main one (`render::post`'s, the
/// materials' glow-only exposure marker) on [`PORTRAIT_GLOW_LAYER`] into a target of the portrait's
/// size, following the portrait camera, and the same glow composite drawn by the portrait camera.
fn portrait_glow(
    commands: &mut Commands,
    p: &mut Portrait,
    (size, tf, proj, fresh): (UVec2, Transform, &Projection, bool),
    (images, meshes, mats): (
        &mut Assets<Image>,
        &mut Assets<Mesh>,
        &mut Assets<gd_render::render::post::GlowComposite>,
    ),
    cams: &mut Query<(&mut Camera, &RenderTarget)>,
) {
    use bevy::camera::{Exposure, Hdr};
    if p.glow.as_ref().is_some_and(|g| g.size != size) {
        let g = p.glow.take().expect("checked");
        commands.entity(g.camera).try_despawn();
        commands.entity(g.composite).try_despawn();
    }
    let Some(g) = &p.glow else {
        let image = images.add(Image::new_target_texture(
            size.x.max(1),
            size.y.max(1),
            TextureFormat::Rgba16Float,
            None,
        ));
        let camera = commands
            .spawn((
                Camera3d::default(),
                // Alpha 0: no outline mask in the portrait.
                Camera {
                    order: -4,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Hdr,
                Tonemapping::None,
                DebandDither::Disabled,
                Msaa::Off,
                // The glow-only marker (`render::post::setup_glow`).
                Exposure { ev100: 0.0 },
                bevy::light::cluster::ClusterConfig::Single,
                RenderLayers::layer(PORTRAIT_GLOW_LAYER),
                proj.clone(),
                tf,
                Name::new("conversation portrait glow camera"),
            ))
            .id();
        let composite = commands
            .spawn((
                Mesh3d(meshes.add(gd_render::render::post::fullscreen_triangle())),
                MeshMaterial3d(
                    mats.add(gd_render::render::post::GlowComposite::scene(image.clone())),
                ),
                bevy::camera::visibility::NoFrustumCulling,
                RenderLayers::layer(PORTRAIT_COMPOSITE_LAYER),
                Transform::default(),
                Name::new("conversation portrait glow composite"),
            ))
            .id();
        p.glow = Some(PortraitGlow {
            camera,
            composite,
            size,
        });
        return;
    };
    if let Ok((mut cam, _)) = cams.get_mut(g.camera) {
        let wake = !cam.is_active;
        if wake {
            cam.is_active = true;
        }
        if fresh || wake {
            commands.entity(g.camera).insert((tf, proj.clone()));
        }
    }
}

/// The portrait background (module doc, `worldBackground`): a 1 × 2 gradient image (sky over
/// ground, sampled between the texel centres so the blend spans the whole view), a quad `dist` in
/// front of the camera covering its view, and its material: [`FLAG_UNLIT`] (a doll view draws it
/// as the texel), opaque, no culling. The material's `base` is the caller's handle of the image.
fn background_assets(
    sv: &SceneView,
    size: UVec2,
    dist: f32,
    texels: [[u8; 4]; 2],
    d: &gd_render::render::GdDefaults,
) -> (Image, Mesh, GdMaterial) {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::ImageSampler;
    use bevy::render::render_resource::{Extent3d, TextureDimension};
    use gd_render::render::material::{FLAG_UNLIT, GdParams, KIND_LIT, PassStates};
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texels.concat(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    let h = 2.0 * dist * (sv.fov * 0.5).tan() * 1.05;
    let w = h * size.x as f32 / size.y.max(1) as f32;
    let mut quad = Mesh::from(Rectangle::new(w, h));
    if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
        quad.attribute_mut(Mesh::ATTRIBUTE_UV_0)
    {
        for t in uv.iter_mut() {
            t[0] = 0.5;
            t[1] = 0.25 + 0.5 * t[1];
        }
    }
    let material = GdMaterial {
        params: GdParams {
            diffuse: Vec4::new(0.5, 0.5, 0.5, 0.0),
            specular: Vec4::new(0.0, 0.0, 0.0, 1.0),
            scroll: Vec4::ZERO,
            snow: Vec4::ZERO,
            flags: UVec4::new(KIND_LIT, FLAG_UNLIT, 0, 0),
            fade: Vec4::new(1.0, 0.5, 1.0, 1.0),
            outline: Vec4::ZERO,
            wind: Vec4::ZERO,
            ripple_scale: Vec4::ZERO,
            ripple_amplitude: Vec4::ZERO,
            ..GdParams::default()
        },
        base: d.white.clone(),
        bump: d.flat_normal.clone(),
        glow: d.black.clone(),
        detail: d.white.clone(),
        snow_base: d.white.clone(),
        snow_normal: d.flat_normal.clone(),
        env: d.env.clone(),
        grass: d.grass.clone(),
        states: PassStates {
            cull: 0,
            alpha_test: false,
            ..PassStates::default()
        },
    };
    (image, quad, material)
}
