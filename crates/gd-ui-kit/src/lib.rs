//! The shared UI kit: the record-driven widget builder ([`Ui`], [`UiAssets`]) and common chrome.
//!
//! Layout is computed in physical pixels with the engine's rules (`gd_formats::ui`): UI scale factor
//! from the resolution, window alignment, `round(pos × scale)` for child positions and `texture size ×
//! scale` for bitmaps. Every widget becomes an absolutely positioned Bevy UI node under one root,
//! spawned in the engine's draw order.
//!
//! Blending: the engine blends UI in gamma space (its UNORM back buffer); so does the port, so
//! colours, tints and textures' own alpha need no correction.
//!
//! The kit reads gameplay data only; it sends no requests and mutates no gameplay state.
pub mod additive;
pub mod confirm;
pub mod keys;
pub mod layout;
pub mod masked;
pub mod state;
pub mod text;
pub mod tip;
pub mod widgets;

use bevy::prelude::*;
use gd_formats::ui::{Rect as URect, TextBox, Widget, round_px};
use gd_game_core::data::GameData;
use gd_render::textures::Textures;
use std::sync::Arc;
use text::{FontCache, LoadedFont};

/// The HUD master table of the orb layout (the default).
pub const MASTER: &str = "records/ui/hud/hud_orbmastertable.dbr";
/// The HUD master table of the classic layout (`standardHUD`).
pub const CLASSIC_MASTER: &str = "records/ui/hud/hud_mastertable.dbr";

pub fn master(classic: bool) -> &'static str {
    if classic { CLASSIC_MASTER } else { MASTER }
}

/// `standardHUD`; the default (no options resource) is the orb HUD.
pub fn classic(options: Option<&gd_game_core::options::GameOptions>) -> bool {
    options.is_some_and(|o| o.flag("standardHUD"))
}

/// Window input systems: before the player's click-to-move and world clicks, so a click on a window
/// never reaches the world.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiInputSet;

/// Everything that shows or reads the playing session (`AppState::InGame`); [`UiInputSet`] and
/// [`HudSet`] are inside it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiPlaySet;

/// The HUD chain, after [`gd_game_core::inventory::InventorySet`] so a click's result is applied
/// before the rebuild reads it. What reads or extends the built HUD runs `.after(HudSet)`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HudSet;

/// Whether the left mouse button was released this frame, from the raw messages: after
/// `ButtonInput::reset_all` the release is never recorded.
pub fn left_released(clicks: &mut MessageReader<bevy::input::mouse::MouseButtonInput>) -> bool {
    // Reads every message (no short circuit), so none carries over a frame.
    clicks
        .read()
        .filter(|e| e.button == MouseButton::Left && e.state == bevy::input::ButtonState::Released)
        .count()
        > 0
}

/// Whether physical-px point `p` is inside `r` (half-open: the right and bottom edges are outside).
pub fn contains(r: &gd_formats::ui::Rect, p: Vec2) -> bool {
    p.x >= r.x && p.y >= r.y && p.x < r.x + r.w && p.y < r.y + r.h
}

/// The pointer in physical px, `None` while it is outside the window.
pub fn cursor_px(win: &Window) -> Option<Vec2> {
    win.cursor_position().map(|c| c * win.scale_factor())
}

/// [`spawn_root`] for an overlay rebuilt every frame: stamped with the frame it was spawned ([`RootFrame`], [`retire_roots`]).
pub fn spawn_frame_root(
    commands: &mut Commands,
    marker: impl Component,
    frame: u32,
    z: i32,
) -> Entity {
    let root = spawn_root(commands, marker, z);
    commands.entity(root).insert(RootFrame(frame));
    root
}

/// Places an absolutely positioned node at `r` (screen px; `inv`: physical → logical), writing only what changed.
pub fn set_node_rect(node: &mut Mut<Node>, r: URect, inv: f32) {
    let v = [r.x, r.y, r.w.max(0.0), r.h.max(0.0)].map(|p| Val::Px(p * inv));
    if [node.left, node.top, node.width, node.height] != v {
        let n = &mut **node;
        [n.left, n.top, n.width, n.height] = v;
    }
}

/// Sets an image node's source rect (texture px) when it changed.
pub fn set_image_src(img: &mut Mut<ImageNode>, src: URect) {
    let r = Some(Rect::new(
        src.x,
        src.y,
        src.x + src.w.max(0.0),
        src.y + src.h.max(0.0),
    ));
    if img.rect != r {
        img.rect = r;
    }
}

/// Where a [`LiveText`] draws: a line with its top-left at `at` (with `centre`, `at.x` is the centre), or
/// a record text box at `origin`.
#[derive(Clone)]
pub enum TextAt {
    Line {
        style: String,
        at: Vec2,
        centre: bool,
        tint: Color,
    },
    Box {
        tb: TextBox,
        origin: Vec2,
    },
    /// A style and an optional colour override.
    Colored {
        tb: TextBox,
        style: String,
        origin: Vec2,
        color: Option<[f32; 3]>,
    },
}

/// A text redrawn in place when it differs (nothing while empty); spawned by [`Ui::live_text`].
#[derive(Component)]
pub struct LiveText {
    at: TextAt,
    s: f32,
    inv: f32,
    screen: Vec2,
    shown: String,
}

impl LiveText {
    /// Draws `text` under `me` (this text's node) when it differs from the one shown.
    pub fn set(
        &mut self,
        me: Entity,
        text: &str,
        commands: &mut Commands,
        data: &GameData,
        assets: &mut UiAssets,
    ) {
        if self.shown == text {
            return;
        }
        self.shown.clear();
        self.shown.push_str(text);
        commands.entity(me).despawn_related::<Children>();
        let mut ui = assets.ui(commands, data, me, self.s, self.inv, self.screen);
        ui.draw_text(&self.at, text);
    }
}

impl Ui<'_, '_, '_> {
    /// A [`LiveText`] showing `text` at `at`: its node, under the root.
    pub fn live_text(&mut self, at: TextAt, text: &str) -> Entity {
        let node = self
            .commands
            .spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            })
            .id();
        self.commands.entity(self.root).add_child(node);
        let root = std::mem::replace(&mut self.root, node);
        self.draw_text(&at, text);
        self.root = root;
        self.commands.entity(node).insert(LiveText {
            at,
            s: self.s,
            inv: self.inv,
            screen: self.screen,
            shown: text.to_owned(),
        });
        node
    }

    fn draw_text(&mut self, at: &TextAt, text: &str) {
        match at {
            TextAt::Line {
                style,
                at,
                centre,
                tint,
            } => {
                let x = if *centre {
                    at.x - (self.text_width(style, text).trunc() * 0.5).trunc()
                } else {
                    at.x
                };
                self.text_line_tinted(style, Vec2::new(x, at.y), text, *tint);
            }
            TextAt::Box { tb, origin } => {
                self.text(tb, *origin, text);
            }
            TextAt::Colored {
                tb,
                style,
                origin,
                color,
            } => {
                self.text_colored(tb, style, *origin, text, *color);
            }
        }
    }
}

/// The frame a rebuilt overlay's root was spawned: a fresh UI root is not drawn on its first frame,
/// so [`retire_roots`] keeps the root being replaced until its successor is drawn.
#[derive(Component)]
pub struct RootFrame(pub u32);

/// Despawns an overlay's old roots: all of them when `clear`; else every root but the newest, once
/// the newest has been drawn (spawned before `frame`) or `replacing`. Returns the root that stays;
/// an overlay updating parts in place must only touch parts under it (the others' despawn is queued).
pub fn retire_roots(
    commands: &mut Commands,
    roots: impl Iterator<Item = (Entity, u32)>,
    frame: u32,
    replacing: bool,
    clear: bool,
) -> Option<Entity> {
    let roots: Vec<(Entity, u32)> = roots.collect();
    let newest = roots.iter().max_by_key(|r| (r.1, r.0)).copied();
    for (e, f) in &roots {
        let keep = !clear && Some((*e, *f)) == newest;
        if !keep && (clear || replacing || newest.is_some_and(|n| n.1 < frame)) {
            commands.entity(*e).try_despawn();
        }
    }
    newest.filter(|_| !clear).map(|n| n.0)
}

/// The asset resources every [`Ui`] builder needs, as one system parameter.
#[derive(bevy::ecs::system::SystemParam)]
pub struct UiAssets<'w> {
    pub tex: ResMut<'w, Textures>,
    pub images: ResMut<'w, Assets<Image>>,
    pub fonts: ResMut<'w, FontCache>,
    pub font_assets: ResMut<'w, Assets<Font>>,
    options: Option<Res<'w, gd_game_core::options::GameOptions>>,
    scale: Res<'w, state::settings::UiScaleOption>,
}

impl UiAssets<'_> {
    /// `standardHUD`.
    pub fn classic_hud(&self) -> bool {
        classic(self.options.as_deref())
    }

    /// The UI scale factor at a `size` (physical px) screen ([`state::settings::UiScaleOption`]).
    pub fn scale(&self, size: UVec2) -> f32 {
        self.scale.factor(size.x, size.y)
    }

    /// The build context under `root`: UI scale `s`, physical → logical factor `inv`, the screen's physical size.
    pub fn ui<'a, 'w, 's>(
        &'a mut self,
        commands: &'a mut Commands<'w, 's>,
        data: &'a GameData,
        root: Entity,
        s: f32,
        inv: f32,
        screen: Vec2,
    ) -> Ui<'a, 'w, 's> {
        Ui {
            commands,
            root,
            data,
            tex: &mut self.tex,
            images: &mut self.images,
            fonts: &mut self.fonts,
            font_assets: &mut self.font_assets,
            classic_hud: classic(self.options.as_deref()),
            s,
            inv,
            screen,
            docked: None,
            req: Default::default(),
        }
    }
}

/// A full-screen, click-through root node for a window or overlay at `z` (`GlobalZIndex`), tagged with `marker`.
pub fn spawn_root(commands: &mut Commands, marker: impl Bundle, z: i32) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(z),
        ))
        .id()
}

/// The HUD overlays' local [`ZIndex`]: over the HUD group, under the HUD-group windows (even local
/// z) and every `GlobalZIndex` window.
pub const HUD_OVERLAY_Z: i32 = 1;

/// [`spawn_root`] for a HUD overlay: no `GlobalZIndex`, stacked among the HUD groups at [`HUD_OVERLAY_Z`].
pub fn spawn_hud_overlay(commands: &mut Commands, marker: impl Bundle) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ZIndex(HUD_OVERLAY_Z),
        ))
        .id()
}

/// Build context: spawns absolutely positioned nodes (physical px) under the root.
pub struct Ui<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub root: Entity,
    pub data: &'a GameData,
    pub tex: &'a mut Textures,
    pub images: &'a mut Assets<Image>,
    pub fonts: &'a mut FontCache,
    pub font_assets: &'a mut Assets<Font>,
    /// `standardHUD`: the classic master table ([`master`]).
    pub classic_hud: bool,
    /// UI scale factor.
    pub s: f32,
    /// Physical → logical pixels.
    pub inv: f32,
    pub screen: Vec2,
    /// x of the open NPC window and of the inventory when docked together.
    pub docked: Option<(f32, f32)>,
    /// The player's equip requirements for the item cells' unusable marks; the default (level 1, no
    /// attributes) elsewhere.
    pub req: gd_game_core::inventory::Requirements,
}

impl Ui<'_, '_, '_> {
    /// The HUD master table of the chosen layout.
    pub fn hud_master(&self) -> &'static str {
        master(self.classic_hud)
    }

    pub fn record(&self, path: &str) -> Option<gd_formats::arz::Record> {
        if path.is_empty() {
            return None;
        }
        let r = self.data.db.get_owned(path);
        if r.is_none() {
            warn!("ui: missing record {path}");
        }
        r
    }

    pub fn widget(&self, path: &str) -> Option<Widget> {
        self.record(path).map(|r| Widget::from_record(&r))
    }

    /// Texture handle + pixel size.
    pub fn image(&mut self, path: &str) -> Option<(Handle<Image>, UVec2)> {
        let h = self.tex.get(self.data, path, true, self.images)?;
        let size = self.images.get(&h).map(|i| i.size())?;
        Some((h, size))
    }

    /// [`Self::image`] desaturated.
    pub fn image_gray(&mut self, path: &str) -> Option<(Handle<Image>, UVec2)> {
        let h = self.tex.get_gray(self.data, path, self.images)?;
        let size = self.images.get(&h).map(|i| i.size())?;
        Some((h, size))
    }

    /// `round(v × s)`.
    pub fn sc(&self, v: f32) -> f32 {
        round_px(v * self.s)
    }

    pub fn node(&self, r: URect) -> Node {
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(r.x * self.inv),
            top: Val::Px(r.y * self.inv),
            width: Val::Px(r.w * self.inv),
            height: Val::Px(r.h * self.inv),
            ..default()
        }
    }

    /// Draws `image` (optionally a sub-rect in texture px) into `dst` (screen px).
    pub fn quad(
        &mut self,
        image: Handle<Image>,
        dst: URect,
        src: Option<URect>,
        color: Color,
    ) -> Entity {
        let mut img = ImageNode::new(image).with_color(color);
        img.rect = src.map(|r| Rect::new(r.x, r.y, r.x + r.w, r.y + r.h));
        let e = self.commands.spawn((self.node(dst), img)).id();
        self.commands.entity(self.root).add_child(e);
        e
    }

    /// Like [`Self::quad`] but the image fills `dst` whatever its aspect.
    pub fn quad_stretch(&mut self, image: Handle<Image>, dst: URect, color: Color) -> Entity {
        let img = ImageNode::new(image)
            .with_color(color)
            .with_mode(bevy::ui::widget::NodeImageMode::Stretch);
        let e = self.commands.spawn((self.node(dst), img)).id();
        self.commands.entity(self.root).add_child(e);
        e
    }

    pub fn solid(&mut self, dst: URect, color: Color) -> Entity {
        let e = self
            .commands
            .spawn((self.node(dst), BackgroundColor(color)))
            .id();
        self.commands.entity(self.root).add_child(e);
        e
    }

    /// A bitmap at record position (`x`,`y`) relative to `origin`, sized by its texture.
    pub fn bitmap(&mut self, path: &str, x: f32, y: f32, origin: Vec2) -> Option<(Entity, URect)> {
        let (h, size) = self.image(path)?;
        let r = URect::new(
            origin.x + self.sc(x),
            origin.y + self.sc(y),
            self.sc(size.x as f32),
            self.sc(size.y as f32),
        );
        Some((self.quad(h, r, None, Color::WHITE), r))
    }

    /// A record's `Widget::Bitmap` field drawn at `origin` (`None` when the field is no bitmap or its texture
    /// is missing).
    pub fn bitmap_field(&mut self, rec: &str, origin: Vec2) -> Option<(Entity, URect)> {
        let Widget::Bitmap { bitmap, x, y } = self.widget(rec)? else {
            return None;
        };
        self.bitmap(&bitmap, x, y, origin)
    }

    /// Draws `text` (markup allowed) in a record text box relative to `origin`.
    pub fn text(&mut self, tb: &TextBox, origin: Vec2, text: &str) -> Option<Entity> {
        self.text_with_style(tb, tb.style.as_deref()?, origin, text)
    }

    /// Line height (px) of a font style (the height of one laid-out line).
    pub fn line_height(&mut self, style_path: &str) -> f32 {
        let Some(style) = self.fonts.style(self.data, style_path) else {
            return 0.0;
        };
        let size = style.scaled_size(self.s);
        match self
            .fonts
            .font(self.data, &style.font, self.font_assets)
            .as_deref()
        {
            Some(LoadedFont::Bitmap { font, .. }) => {
                gd_formats::ui::layout_text(
                    font,
                    size,
                    style.flags,
                    "A",
                    (style.color0, style.color1),
                )
                .height as f32
            }
            Some(LoadedFont::Dynamic(_, face)) => face
                .as_ref()
                .map(|f| text::dynamic_line_height(f, size as f32))
                .unwrap_or((size as f32 * 1.2).round()),
            _ => (size as f32 * 1.2).round(),
        }
    }

    /// The style's pixel size under the UI scale (`StyleManager::LoadStyle`: the style object's `fontSize`, which the
    /// conversation layout `ConversationWindow::Layout` uses as the line height).
    pub fn font_px(&mut self, style_path: &str) -> f32 {
        self.fonts
            .style(self.data, style_path)
            .map_or(0.0, |st| st.scaled_size(self.s) as f32)
    }

    /// Width (px) of one line of text in a style.
    pub fn text_width(&mut self, style_path: &str, text: &str) -> f32 {
        self.fonts
            .text_width(self.data, self.font_assets, style_path, self.s, text)
    }

    /// One line of `text` cut to `max_w` px with "..." (`UITextBox::TruncateLine`).
    /// PORT: the cut is per character; it never splits a `^X` colour code.
    pub fn truncate_line(&mut self, style: &str, max_w: f32, text: &str) -> String {
        if self.text_width(style, text) <= max_w {
            return text.to_owned();
        }
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return String::new();
        }
        // A prefix ending in `^` steps back to the code's start.
        let cut = |mut n: usize| {
            while n > 0 && chars[n - 1] == '^' {
                n -= 1;
            }
            n
        };
        // Binary-search the longest prefix that fits with the dots.
        let mut fits = |n: usize| {
            let t = format!(
                "{}...",
                chars[..cut(n)].iter().collect::<String>().trim_end()
            );
            self.text_width(style, &t) <= max_w
        };
        if !fits(0) {
            return String::new();
        }
        let (mut lo, mut hi) = (0, chars.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if fits(mid) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let n = cut(lo);
        format!("{}...", chars[..n].iter().collect::<String>().trim_end())
    }

    /// The wrapped lines of `text` (markup kept; `\n` breaks) in a style at
    /// most `max_w` px wide.
    pub fn wrap_lines(&mut self, style_path: &str, max_w: f32, text: &str) -> Vec<String> {
        let Some(style) = self.fonts.style(self.data, style_path) else {
            return Vec::new();
        };
        let size = style.scaled_size(self.s);
        let font = self.fonts.font(self.data, &style.font, self.font_assets);
        let mut out = Vec::new();
        let mut carry = String::new();
        for part in text.split('\n') {
            let part_c = format!("{carry}{part}");
            carry = text::active_color(&part_c);
            match font.as_deref() {
                Some(LoadedFont::Bitmap { font, .. }) => {
                    out.extend(text::wrap_by(&part_c, max_w.trunc(), |t| {
                        text::bitmap_width(font, size, style.flags, t)
                    }))
                }
                Some(LoadedFont::Dynamic(_, Some(f))) => {
                    out.extend(text::wrap_by(&part_c, max_w.trunc(), |t| {
                        text::dynamic_width(f, size as f32, t)
                    }))
                }
                Some(LoadedFont::Dynamic(_, None)) | None => out.push(part_c),
            }
        }
        out
    }

    /// One line of text (markup allowed) with its top-left at `pos` (screen
    /// px); `alpha` scales its opacity.
    pub fn text_line(
        &mut self,
        style_path: &str,
        pos: Vec2,
        text: &str,
        alpha: f32,
    ) -> Option<Entity> {
        self.text_line_tinted(style_path, pos, text, Color::srgba(1.0, 1.0, 1.0, alpha))
    }

    /// [`Self::text_line`] multiplied by `tint` (a text box's colour).
    pub fn text_line_tinted(
        &mut self,
        style_path: &str,
        pos: Vec2,
        text: &str,
        tint: Color,
    ) -> Option<Entity> {
        self.text_line_colored(style_path, pos, text, tint, None)
    }

    /// [`Self::text_line_tinted`] with the text widget's colour override (`color`, as [`Self::text_colored`]).
    pub fn text_line_colored(
        &mut self,
        style_path: &str,
        pos: Vec2,
        text: &str,
        tint: Color,
        color: Option<[f32; 3]>,
    ) -> Option<Entity> {
        if text.is_empty() {
            return None;
        }
        let mut style = self.fonts.style(self.data, style_path)?;
        if let Some(c) = color {
            (style.color0, style.color1) = (c, c);
        }
        let font: Arc<LoadedFont> = self.fonts.font(self.data, &style.font, self.font_assets)?;
        let size = style.scaled_size(self.s);
        match &*font {
            LoadedFont::Bitmap { font, atlas } => {
                let ti =
                    self.fonts
                        .rasterize_cached(self.images, font, atlas, &style, size, text)?;
                let dst = URect::new(
                    (pos.x + ti.offset.x).round(),
                    (pos.y + ti.offset.y).round(),
                    ti.size.x as f32,
                    ti.size.y as f32,
                );
                let h = ti.image.clone();
                Some(self.quad(h, dst, None, tint))
            }
            LoadedFont::Dynamic(handle, _) => {
                let (c, t) = (style.color0, tint.to_srgba());
                let e = self
                    .commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(pos.x * self.inv),
                            top: Val::Px(pos.y * self.inv),
                            ..default()
                        },
                        Text::new(gd_formats::text::strip_markup(text)),
                        TextFont {
                            font: FontSource::Handle(handle.clone()),
                            font_size: FontSize::Px(size as f32 * self.inv),
                            ..default()
                        },
                        TextColor(Color::srgba(
                            c[0] * t.red,
                            c[1] * t.green,
                            c[2] * t.blue,
                            t.alpha,
                        )),
                        TextLayout {
                            linebreak: LineBreak::NoWrap,
                            ..default()
                        },
                    ))
                    .id();
                self.commands.entity(self.root).add_child(e);
                Some(e)
            }
        }
    }

    /// A window border of eight bitmaps (record fields `upperLeftTexture`,
    /// `upperRightTexture`, `lowerLeftTexture`, `lowerRightTexture`,
    /// `topTexture`, `bottomTexture`, `leftTexture`, `rightTexture`, with an
    /// optional `prefix` such as `obj`). `Grim Dawn.exe`
    /// `UIWindow::RenderBorder`: every piece lies outside `r`, offset by the upper-left
    /// corner's size: the corners at (x − ulw, y − ulh), (x + w, y − ulh),
    /// (x − ulw, y + h), (x + w, y + h); the top / bottom edges span `w` at
    /// y − ulh / y + h; the left / right edges span `h` at x − ulw / x + w.
    pub fn border(&mut self, rec: &gd_formats::arz::Record, prefix: &str, r: URect, alpha: f32) {
        let key = |k: &str| {
            let k = if prefix.is_empty() {
                k.to_string()
            } else {
                format!("{prefix}{}{}", k[..1].to_uppercase(), &k[1..])
            };
            rec.str(&k).unwrap_or("").to_owned()
        };
        let keys = [
            "upperLeftTexture",
            "upperRightTexture",
            "lowerLeftTexture",
            "lowerRightTexture",
            "topTexture",
            "bottomTexture",
            "leftTexture",
            "rightTexture",
        ];
        let paths = keys.map(key);
        self.border_paths(&paths.each_ref().map(String::as_str), r, alpha);
    }

    /// [`Self::border`] from the eight bitmaps' paths (upper left, upper right, lower left, lower
    /// right, top, bottom, left, right).
    pub fn border_paths(&mut self, paths: &[&str; 8], r: URect, alpha: f32) {
        let tex = self.border_textures(paths);
        let Some((_, uls)) = &tex[0] else {
            return;
        };
        let (cw, ch) = (self.sc(uls.x as f32), self.sc(uls.y as f32));
        let rects: [URect; 8] = std::array::from_fn(|i| {
            let (iw, ih) = tex[i].as_ref().map_or((0.0, 0.0), |(_, s)| {
                (self.sc(s.x as f32), self.sc(s.y as f32))
            });
            match i {
                0 => URect::new(r.x - cw, r.y - ch, cw, ch),
                1 => URect::new(r.x + r.w, r.y - ch, iw, ih),
                2 => URect::new(r.x - cw, r.y + r.h, iw, ih),
                3 => URect::new(r.x + r.w, r.y + r.h, iw, ih),
                4 => URect::new(r.x, r.y - ch, r.w, ih),
                5 => URect::new(r.x, r.y + r.h, r.w, ih),
                6 => URect::new(r.x - cw, r.y, iw, r.h),
                _ => URect::new(r.x + r.w, r.y, iw, r.h),
            }
        });
        self.draw_border(&tex, rects, alpha);
    }

    /// The border drawn OVER `r`'s edges (the item tooltip's rollover box, `RolloverWindow`): the
    /// corners sit inside the box's corners at their own size, the edges span between them (top /
    /// bottom `w − left − right`, left / right `h − top − bottom`), each edge's thickness its own
    /// bitmap's. `corr` is the style's 1 px border correction (`borderCorrectionOffsetX/Y` shift the
    /// edges' start, `…XSize/YSize` grow them), rounded as the rollover draw rounds it.
    pub fn border_over(
        &mut self,
        paths: &[&str; 8],
        r: URect,
        alpha: f32,
        corr: (f32, f32, f32, f32),
    ) {
        let tex = self.border_textures(paths);
        let th = |i: usize, w: bool| {
            tex[i].as_ref().map_or(0.0, |(_, s)| {
                (if w { s.x } else { s.y } as f32 * self.s).round()
            })
        };
        let (bt, bb, bl, br) = (th(4, false), th(5, false), th(6, true), th(7, true));
        let (ex, ey, ew, eh) = (
            corr.0.round(),
            corr.1.round(),
            corr.2.round(),
            corr.3.round(),
        );
        let (w, h) = (r.w, r.h);
        let rects: [URect; 8] = std::array::from_fn(|i| {
            let (cw, ch) = tex[i].as_ref().map_or((0.0, 0.0), |(_, s)| {
                ((s.x as f32 * self.s).round(), (s.y as f32 * self.s).round())
            });
            match i {
                0 => URect::new(r.x, r.y, cw, ch),
                1 => URect::new(r.x + w - cw, r.y, cw, ch),
                2 => URect::new(r.x, r.y + h - ch, cw, ch),
                3 => URect::new(r.x + w - cw, r.y + h - ch, cw, ch),
                4 => URect::new(r.x + bl + ex, r.y, w - bl - br + ew, bt),
                5 => URect::new(r.x + bl + ex, r.y + h - bb, w - bl - br + ew, bb),
                6 => URect::new(r.x, r.y + bt + ey, bl, h - bt - bb + eh),
                _ => URect::new(r.x + w - br, r.y + bt + ey, br, h - bt - bb + eh),
            }
        });
        self.draw_border(&tex, rects, alpha);
    }

    /// The eight border bitmaps (an empty path → `None`).
    fn border_textures(&mut self, paths: &[&str; 8]) -> [Option<(Handle<Image>, UVec2)>; 8] {
        std::array::from_fn(|i| {
            let p = paths[i];
            (!p.is_empty()).then(|| self.image(p)).flatten()
        })
    }

    /// Stretches each present border piece into its rect (the edges fill, the corners keep their
    /// size), tinted white at `alpha`.
    fn draw_border(
        &mut self,
        tex: &[Option<(Handle<Image>, UVec2)>; 8],
        rects: [URect; 8],
        alpha: f32,
    ) {
        let tint = Color::srgba(1.0, 1.0, 1.0, alpha);
        for i in 0..8 {
            if let Some((h, _)) = &tex[i] {
                self.quad_stretch(h.clone(), rects[i], tint);
            }
        }
    }

    pub fn text_with_style(
        &mut self,
        tb: &TextBox,
        style_path: &str,
        origin: Vec2,
        text: &str,
    ) -> Option<Entity> {
        self.text_colored(tb, style_path, origin, text, None)
    }

    /// [`Self::text_with_style`] with the text widget's colour override (`TextWidget.overrideColor` /
    /// `TextWidget.color` replaces both glyph colours).
    pub fn text_colored(
        &mut self,
        tb: &TextBox,
        style_path: &str,
        origin: Vec2,
        text: &str,
        color: Option<[f32; 3]>,
    ) -> Option<Entity> {
        if text.is_empty() {
            return None;
        }
        let mut style = self.fonts.style(self.data, style_path)?;
        if let Some(c) = color {
            (style.color0, style.color1) = (c, c);
        }
        let font: Arc<LoadedFont> = self.fonts.font(self.data, &style.font, self.font_assets)?;
        let size = style.scaled_size(self.s);
        let bx = URect::new(
            origin.x + self.sc(tb.rect.x),
            origin.y + self.sc(tb.rect.y),
            self.sc(tb.rect.w),
            self.sc(tb.rect.h),
        );
        match &*font {
            LoadedFont::Bitmap { font, atlas } => {
                let ti =
                    self.fonts
                        .rasterize_cached(self.images, font, atlas, &style, size, text)?;
                let x = tb.align_x.place(bx.x, bx.w, ti.layout.width);
                let y = tb.align_y.place(bx.y, bx.h, ti.layout.height);
                let dst = URect::new(
                    x + ti.offset.x,
                    y + ti.offset.y,
                    ti.size.x as f32,
                    ti.size.y as f32,
                );
                let h = ti.image.clone();
                Some(self.quad(h, dst, None, Color::WHITE))
            }
            LoadedFont::Dynamic(handle, _) => {
                let plain = gd_formats::text::strip_markup(text);
                let c = style.color0;
                let font_comp = TextFont {
                    font: FontSource::Handle(handle.clone()),
                    font_size: FontSize::Px(size as f32 * self.inv),
                    ..default()
                };
                let color = TextColor(Color::srgb(c[0], c[1], c[2]));
                let lh = self.line_height(style_path);
                let e = if bx.h <= lh * 1.5 {
                    // One line: an auto-width node never word-wraps, so ellipsised values stay on
                    // one line and centred titles stay centred.
                    let w = self.text_width(style_path, text);
                    let x = tb.align_x.place(bx.x, bx.w, w as i32);
                    let y = tb.align_y.place(bx.y, bx.h, lh as i32);
                    self.commands
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(x * self.inv),
                                top: Val::Px(y * self.inv),
                                ..default()
                            },
                            Text::new(plain),
                            font_comp,
                            color,
                            TextLayout {
                                linebreak: LineBreak::NoWrap,
                                ..default()
                            },
                        ))
                        .id()
                } else {
                    let justify = match tb.align_x {
                        gd_formats::ui::TextAlign::Start => Justify::Left,
                        gd_formats::ui::TextAlign::End => Justify::Right,
                        gd_formats::ui::TextAlign::Center => Justify::Center,
                    };
                    self.commands
                        .spawn((
                            self.node(bx),
                            Text::new(plain),
                            font_comp,
                            color,
                            TextLayout::justify(justify),
                        ))
                        .id()
                };
                self.commands.entity(self.root).add_child(e);
                Some(e)
            }
        }
    }
}
