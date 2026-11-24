//! In-game UI definitions (database records under `records/ui/...`) and the
//! engine's layout rules. Verified against `Grim Dawn.exe` (the UI widgets live
//! in the executable, not in Engine/Game.dll) and Engine.dll.
//!
//! There is no generic widget tree in the data: every window class reads a fixed
//! set of named fields from its own record (`hudHealthBar`, `equipHead`, ...),
//! each naming a child record. The child's `templateName` tells which widget
//! class the field is expected to hold; [`Widget::from_record`] dispatches on it.
//!
//! Units: records are authored in pixels at UI scale 1.0. Every position and
//! size is multiplied by [`ui_scale_factor`] and rounded with [`round_px`];
//! bitmap sizes come from the texture (`texture px × scale`).
use crate::arz::Record;

/// The engine's rounding for UI rects: `floor(x + 0.5)`.
pub fn round_px(x: f32) -> f32 {
    (x + 0.5).floor()
}

/// `GraphicsEngine::GetUIScaleFactor` (Engine.dll).
///
/// `option` is the UI-scale slider in [0, 1] (`Options` float, default 0.5; 0.5 also when the engine flag that
/// enables it is off). At the default the factor is 1.0 from ~1440×1015 pixels up, smaller below
/// (e.g. 0.856 at 1600×900).
pub fn ui_scale_factor(width: u32, height: u32, option: f32) -> f32 {
    let v = option;
    let t = (v - 0.5) + (v - 0.5);
    if t < 0.0 {
        return t * 0.3 + 1.0;
    }
    // 2.1701389e-7 = 1 / (2560 × 1800)
    let area = ((width * height) as f32 * 0.000_000_217_013_89).clamp(0.0, 1.0);
    if area > 0.75 {
        return t + 1.0;
    }
    let mut base = area * 0.500_000_06 + 0.7;
    if (1.0 - base).abs() <= 0.1 {
        base = 1.0;
    }
    (area * 0.7 + 0.1) * t + base
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// Window screen alignment (`windowScreenAlignmentX/Y`, parser `Ui::ParseScreenAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Unknown/empty string: position used as-is, unscaled.
    #[default]
    None,
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

impl Align {
    pub fn parse(s: &str) -> Self {
        match s {
            "Left" => Align::Left,
            "Right" => Align::Right,
            "Center" => Align::Center,
            "Top" => Align::Top,
            "Bottom" => Align::Bottom,
            _ => Align::None,
        }
    }
}

/// Text-box alignment (`textAlignmentX/Y`, parsers `Ui::ParseTextAlignmentX` / `Ui::ParseTextAlignmentY`):
/// Left|Top = 0, Right|Bottom = 1, anything else (incl. the data's "Botton"
/// typo) = Center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    Start,
    End,
    #[default]
    Center,
}

impl TextAlign {
    pub fn parse_x(s: &str) -> Self {
        match s {
            "Left" => TextAlign::Start,
            "Right" => TextAlign::End,
            _ => TextAlign::Center,
        }
    }
    pub fn parse_y(s: &str) -> Self {
        match s {
            "Top" => TextAlign::Start,
            "Bottom" => TextAlign::End,
            _ => TextAlign::Center,
        }
    }
    /// `GraphicsCanvas::RenderText2d(Rect, ...)` (Engine): start
    /// coordinate of a block of `extent` pixels in `[pos, pos + size)`.
    pub fn place(self, pos: f32, size: f32, extent: i32) -> f32 {
        let v = match self {
            TextAlign::Start => pos.trunc(),
            TextAlign::End => (pos + size - extent as f32).trunc(),
            TextAlign::Center => (pos + size * 0.5 - (extent / 2) as f32).trunc(),
        };
        round_px(v)
    }
}

/// A top-level UI window position (`windowDefault*` fields; generic loader
/// `UIWindow::LoadBase`, rect transform virtual `Ui::TransformWindowRect`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowDef {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub align_x: Align,
    pub align_y: Align,
}

impl WindowDef {
    pub fn from_record(r: &Record) -> Self {
        Self {
            x: r.f32("windowDefaultX").unwrap_or(0.0),
            y: r.f32("windowDefaultY").unwrap_or(0.0),
            w: r.f32("windowDefaultExtentX").unwrap_or(0.0),
            h: r.f32("windowDefaultExtentY").unwrap_or(0.0),
            align_x: Align::parse(r.str("windowScreenAlignmentX").unwrap_or("")),
            align_y: Align::parse(r.str("windowScreenAlignmentY").unwrap_or("")),
        }
    }

    /// Screen rect in pixels: size × s; x per alignment Left `s·x`,
    /// Right `W − (s·x + w)`, Center `W/2 + s·x − w/2`; y likewise with
    /// Top/Bottom/Center. Unaligned axes keep the raw value.
    pub fn screen_rect(&self, screen_w: f32, screen_h: f32, s: f32) -> Rect {
        let (w, h) = (round_px(self.w * s), round_px(self.h * s));
        let (sx, sy) = (self.x * s, self.y * s);
        let x = match self.align_x {
            Align::Left => sx,
            Align::Right => screen_w - (sx + w),
            Align::Center => screen_w * 0.5 + sx - w * 0.5,
            _ => self.x,
        };
        let y = match self.align_y {
            Align::Top => sy,
            Align::Bottom => screen_h - (sy + h),
            Align::Center => screen_h * 0.5 + sy - h * 0.5,
            _ => self.y,
        };
        Rect::new(round_px(x), round_px(y), w, h)
    }
}

/// `records/ui/styles/text/*.dbr` (`fontstyle.tpl`, loader
/// `StyleManager::LoadStyle` Engine).
#[derive(Debug, Clone, PartialEq)]
pub struct FontStyle {
    pub font: String,
    /// Point size from the record (default 16).
    pub size: u32,
    /// Engine minimum after UI scaling (`lowResMinimumFontSize`, default 8).
    pub min_size: u32,
    /// Top / bottom vertex colour of every glyph quad (RGB, gamma space).
    pub color0: [f32; 3],
    pub color1: [f32; 3],
    /// FontStyleFlag: 1 bold, 2 italic.
    pub flags: u32,
}

impl FontStyle {
    pub fn from_record(r: &Record) -> Self {
        let c = |k: &str| r.f32(k).unwrap_or(1.0);
        let color0 = [c("fontColor0.R"), c("fontColor0.G"), c("fontColor0.B")];
        // `StyleManager::LoadStyle` (Engine.dll): each
        // `fontColor1` channel defaults to the style's own `fontColor0`
        // channel (the loader passes it as the getter's default), not 1.0:
        // a style without `fontColor1` is one flat colour.
        let c1 = |k: &str, i: usize| r.f32(k).unwrap_or(color0[i]);
        Self {
            font: r.str("fontName").unwrap_or("fonts/jura.fnt").to_string(),
            size: r.i32("fontSize").unwrap_or(16).max(1) as u32,
            min_size: r.i32("lowResMinimumFontSize").unwrap_or(8).max(1) as u32,
            color0,
            color1: [
                c1("fontColor1.R", 0),
                c1("fontColor1.G", 1),
                c1("fontColor1.B", 2),
            ],
            flags: r.bool("fontAttribBold") as u32 | (r.bool("fontAttribItalic") as u32) << 1,
        }
    }

    /// Requested pixel size under UI scale `s`: `max(ceil(size·s), min)` when s ≠ 1.
    pub fn scaled_size(&self, s: f32) -> u32 {
        if s == 1.0 {
            return self.size;
        }
        ((self.size as f32 * s).ceil() as u32).max(self.min_size)
    }
}

/// Bar graph (`bargraph.tpl` / `bargraphanimated.tpl`, loaders `BarGraph::Load` /
/// `BarGraphAnimated::Load`, draw `BarGraph::Render`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BarGraph {
    pub empty: Option<String>,
    pub full: Option<String>,
    pub cap: Option<String>,
    pub font_style: Option<String>,
    pub x: f32,
    pub y: f32,
    pub vertical: bool,
    pub reverse: bool,
    /// Draw the empty bitmap behind the fill (animated bars only read this;
    /// plain bars default to drawing it).
    pub render_background: bool,
    pub anim: Option<BarAnim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BarAnim {
    pub x_frames: u32,
    pub y_frames: u32,
    pub total_frames: u32,
    pub x_spacing: i32,
    pub y_spacing: i32,
    pub fps: f32,
    pub text_offset_y: f32,
}

impl BarGraph {
    /// Widget size before scaling: the empty bitmap's size if there is one,
    /// else the full bitmap's (`BarGraph::SetBitmaps`).
    pub fn base_size(&self, empty: Option<(u32, u32)>, full: Option<(u32, u32)>) -> (f32, f32) {
        let (w, h) = empty.or(full).unwrap_or((0, 0));
        (w as f32, h as f32)
    }

    /// Source rect in the full bitmap (texture px) for fill `ratio` at
    /// animation `frame` (`BarGraph::GetFillSourceRect` plain, `BarGraphAnimated::GetFillSourceRect` animated).
    pub fn fill_src(&self, tex_w: u32, tex_h: u32, ratio: f32, frame: u32) -> Rect {
        let (ox, oy, cw, ch, sx, sy) = match self.anim {
            Some(a) => {
                let (xf, yf) = (a.x_frames.max(1), a.y_frames.max(1));
                let (cw, ch) = ((tex_w / xf) as f32, (tex_h / yf) as f32);
                let f = frame % a.total_frames.max(1);
                (
                    (f % xf) as f32 * cw,
                    (f / xf) as f32 * ch,
                    cw,
                    ch,
                    a.x_spacing as f32,
                    a.y_spacing as f32,
                )
            }
            None => (0.0, 0.0, tex_w as f32, tex_h as f32, 0.0, 0.0),
        };
        let r = ratio.clamp(0.0, 1.0);
        match (self.vertical, self.reverse) {
            // fill from the bottom
            (true, false) => Rect::new(ox, oy + (1.0 - r) * ch, cw - sx, ch * r - sy),
            // fill from the top
            (true, true) => Rect::new(ox, oy, cw - sx, ch * r - sy),
            // fill from the right
            (false, true) => Rect::new(ox + (1.0 - r) * cw, oy, cw * r - sx, ch - sy),
            (false, false) => Rect::new(ox, oy, cw * r - sx, ch - sy),
        }
    }

    /// Destination rect on screen for `ratio`, given the widget's screen rect
    /// (`BarGraph::GetFillDestRect`).
    pub fn fill_dst(&self, r: Rect, ratio: f32) -> Rect {
        let k = ratio.clamp(0.0, 1.0);
        match (self.vertical, self.reverse) {
            (true, false) => Rect::new(r.x, r.y + (1.0 - k) * r.h, r.w, r.h * k),
            (true, true) => Rect::new(r.x, r.y, r.w, r.h * k),
            (false, true) => Rect::new(r.x + (1.0 - k) * r.w, r.y, r.w * k, r.h),
            (false, false) => Rect::new(r.x, r.y, r.w * k, r.h),
        }
    }
}

/// Text box common to `textstring/textstaticstring/textnumber/hotslot` text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextBox {
    pub style: Option<String>,
    pub rect: Rect,
    pub align_x: TextAlign,
    pub align_y: TextAlign,
    pub tag: Option<String>,
    /// Additive glow drawn behind the text (`additiveBitmapName` + padding).
    pub glow: Option<(String, f32, f32)>,
}

impl TextBox {
    pub fn from_record(r: &Record) -> Self {
        let f = |k: &str| r.f32(k).unwrap_or(0.0);
        Self {
            style: r.str("style").map(str::to_string),
            rect: Rect::new(
                f("textBoxX"),
                f("textBoxY"),
                f("textBoxXSize"),
                f("textBoxYSize"),
            ),
            align_x: TextAlign::parse_x(r.str("textAlignmentX").unwrap_or("")),
            align_y: TextAlign::parse_y(r.str("textAlignmentY").unwrap_or("")),
            tag: r.str("textTag").map(str::to_string),
            glow: r.str("additiveBitmapName").map(|b| {
                (
                    b.to_string(),
                    f("additiveBitmapXPadding"),
                    f("additiveBitmapYPadding"),
                )
            }),
        }
    }
}

/// One widget record, classified by its template.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    /// `bitmapsingle.tpl`: one texture at a position, sized by the texture.
    Bitmap {
        bitmap: String,
        x: f32,
        y: f32,
    },
    BarGraph(BarGraph),
    /// `buttonstatic.tpl` / `buttonhidden.tpl` / `checkbox.tpl` ...: up/down/
    /// over/disabled bitmaps; hidden buttons only have a hit box.
    Button {
        up: Option<String>,
        down: Option<String>,
        over: Option<String>,
        disabled: Option<String>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// `hotslotbutton.tpl`: a skill/potion slot (icon size `bitmapWidth/Height`).
    HotSlot {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        circular: bool,
        key_text: Option<String>,
        /// `cooldownTextTable`: the slot's own cooldown-seconds text record.
        cooldown_text: Option<String>,
        text: TextBox,
        /// `disableCooldownRadial` (`HotSlotButton.disableCooldownRadial`): with a mask bitmap the cooldown is a
        /// masked fill.
        disable_radial: bool,
    },
    Text(TextBox),
    /// `itembox.tpl` / `itemboxhand.tpl`: equipment slot with a silhouette.
    ItemBox {
        rect: Rect,
        silhouette: Option<String>,
    },
    /// `inventorygrid.tpl`.
    InventoryGrid {
        rect: Rect,
    },
    /// `sceneview.tpl`: 3D character preview.
    SceneView {
        rect: Rect,
        background: Option<String>,
    },
    /// `includes/widgetwindow.tpl`-style window position record.
    Window(WindowDef),
    Other {
        template: String,
    },
}

impl Widget {
    /// Template base name, e.g. `bitmapsingle`.
    pub fn template(r: &Record) -> String {
        let t = r.str("templateName").unwrap_or("");
        let base = t.rsplit(['/', '\\']).next().unwrap_or(t);
        base.trim_end_matches(".tpl").to_ascii_lowercase()
    }

    pub fn from_record(r: &Record) -> Self {
        let f = |k: &str| r.f32(k).unwrap_or(0.0);
        let s = |k: &str| r.str(k).map(str::to_string);
        let t = Self::template(r);
        match t.as_str() {
            "bitmapsingle" => Widget::Bitmap {
                bitmap: s("bitmapName").unwrap_or_default(),
                x: f("bitmapPositionX"),
                y: f("bitmapPositionY"),
            },
            "bargraph" | "bargraphanimated" => {
                let anim = (t == "bargraphanimated").then(|| BarAnim {
                    x_frames: r.i32("xFrames").unwrap_or(1).max(1) as u32,
                    y_frames: r.i32("yFrames").unwrap_or(1).max(1) as u32,
                    total_frames: r.i32("totalFrames").unwrap_or(1).max(1) as u32,
                    x_spacing: r.i32("xSpacing").unwrap_or(0),
                    y_spacing: r.i32("ySpacing").unwrap_or(0),
                    fps: f("framesPerSecond").max(1.0),
                    text_offset_y: f("textOffsetY"),
                });
                Widget::BarGraph(BarGraph {
                    empty: s("bitmapEmptyName"),
                    full: s("bitmapFullName"),
                    cap: s("bitmapCapName"),
                    font_style: s("FontStyle"),
                    x: f("bitmapPositionX"),
                    y: f("bitmapPositionY"),
                    vertical: r.bool("vertical"),
                    reverse: r.bool("reverse"),
                    render_background: if anim.is_some() {
                        r.bool("renderBackground")
                    } else {
                        true
                    },
                    anim,
                })
            }
            "buttonstatic"
            | "buttonstatictext"
            | "buttonstatictext2"
            | "buttonhidden"
            | "buttonhiddentext"
            | "buttonmultistate"
            | "checkbox"
            | "skillbutton"
            | "buttonskilltemplate" => Widget::Button {
                up: s("bitmapNameUp"),
                down: s("bitmapNameDown"),
                over: s("bitmapNameInFocus"),
                disabled: s("bitmapNameDisabled"),
                x: f("bitmapPositionX"),
                y: f("bitmapPositionY"),
                w: f("buttonXSize"),
                h: f("buttonYSize"),
            },
            "hotslotbutton" => Widget::HotSlot {
                x: f("bitmapPositionX"),
                y: f("bitmapPositionY"),
                w: r.f32("bitmapWidth").unwrap_or(32.0),
                h: r.f32("bitmapHeight").unwrap_or(32.0),
                circular: r.bool("isCircular"),
                disable_radial: r.bool("disableCooldownRadial"),
                key_text: s("keyTextTable"),
                cooldown_text: s("cooldownTextTable"),
                text: TextBox::from_record(r),
            },
            "textstring" | "textstaticstring" | "textnumber" | "textbox" | "textwindow"
            | "textboxmeasureable" => Widget::Text(TextBox::from_record(r)),
            "itembox" | "itemboxhand" => Widget::ItemBox {
                rect: Rect::new(f("itemX"), f("itemY"), f("itemXSize"), f("itemYSize")),
                silhouette: s("silhouette"),
            },
            "inventorygrid" => Widget::InventoryGrid {
                rect: Rect::new(
                    f("inventoryX"),
                    f("inventoryY"),
                    f("inventoryXSize"),
                    f("inventoryYSize"),
                ),
            },
            "sceneview" => Widget::SceneView {
                rect: Rect::new(f("x"), f("y"), f("xSize"), f("ySize")),
                background: s("backgroundTexture"),
            },
            _ if r.get("windowDefaultExtentX").is_some() => {
                Widget::Window(WindowDef::from_record(r))
            }
            _ => Widget::Other { template: t },
        }
    }
}

// Text layout (GraphicsFont2 / GraphicsCanvas, Engine.dll).

/// One glyph quad in pixels relative to the text origin (top-left of the
/// first line). `c0` colours the top edge, `c1` the bottom edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphQuad {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub c0: [f32; 3],
    pub c1: [f32; 3],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextLayout {
    pub quads: Vec<GlyphQuad>,
    /// Block size as measured by the engine (`GraphicsFont2::GetTextExtent` / `GraphicsFont2::MeasureText`).
    pub width: i32,
    pub height: i32,
}

/// Markup pass of `GraphicsFont2::ProcessCharacter`: returns
/// printable chars with the colour pair in effect for each.
fn process_markup(text: &str, base: ([f32; 3], [f32; 3])) -> Vec<(char, [f32; 3], [f32; 3])> {
    let (mut c0, mut c1) = base;
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('^', Some('^')) | ('&', Some('&')) => {
                out.push((c, c0, c1));
                chars.next();
            }
            ('^', Some('-')) => {
                (c0, c1) = base;
                chars.next();
            }
            ('^', Some(x)) => {
                // Colour letters set both colours; other letters are swallowed.
                if let Some(rgb) = crate::text::color_code(x) {
                    c0 = [rgb[0], rgb[1], rgb[2]];
                    c1 = c0;
                }
                chars.next();
            }
            ('&', Some('b' | 'B' | 'i' | 'I' | 's' | 'S')) => {
                chars.next();
            }
            ('{' | '}', _) => {}
            _ => out.push((c, c0, c1)),
        }
    }
    out
}

/// Lays out `text` with an FNTX font at pixel `size` (style chosen like
/// `GraphicsFont2::GetStyle`: exact size+flags, else nearest size preferring
/// matching flags; metrics scaled by `size / style.size`).
pub fn layout_text(
    font: &crate::fnt::BitmapFont,
    size: u32,
    flags: u32,
    text: &str,
    colors: ([f32; 3], [f32; 3]),
) -> TextLayout {
    let Some(style) = pick_style(font, size, flags).filter(|s| s.size > 0) else {
        return TextLayout::default();
    };
    let scale = size as f32 / style.size as f32;
    let line_h = (style.line_height as f32 * scale) as i32;
    let glyph = |c: char| -> Option<&crate::fnt::Glyph> {
        let code = c as u32;
        if code > 0xffff {
            return None;
        }
        style
            .glyph(code as u16)
            .or_else(|| style.glyph(b'?' as u16))
    };
    let wpx = |g: &crate::fnt::Glyph| round_px(g.width as f32 * scale) as i32;

    let mut out = TextLayout {
        quads: Vec::new(),
        width: 0,
        height: 0,
    };
    for line in process_markup(text, colors).split(|c| c.0 == '\n') {
        // Measure (`GraphicsFont2::MeasureText`).
        let mut x = 0i32;
        let mut prev: Option<(char, &crate::fnt::Glyph)> = None;
        for &(c, ..) in line {
            let Some(g) = glyph(c) else { continue };
            if let Some((pc, pg)) = prev {
                if c != ' ' || pc == ' ' {
                    x += pg.c as i32;
                }
                x += style.kerning(pc as u16, c as u16);
                if g.a < 0 {
                    x = x.max(-(g.a as i32));
                }
                x += g.a as i32;
            }
            x += wpx(g);
            prev = Some((c, g));
        }
        if let Some((_, pg)) = prev {
            x += pg.c as i32;
        }
        out.width = out.width.max(x);

        // Draw (`GraphicsFont2::DrawText`, left-to-right branch).
        let y = out.height as f32;
        let mut pen = 0i32;
        let mut prev_c = 0i32;
        for (n, &(c, c0, c1)) in line.iter().enumerate() {
            let Some(g) = glyph(c) else { continue };
            if n > 0 {
                pen += g.a as i32;
                if c == ' ' && line[n - 1].0 != ' ' {
                    pen -= prev_c;
                }
            }
            if g.width != 0 {
                let w = wpx(g);
                let top = (g.y_offset as f32 * scale) as i32;
                let bottom = ((g.y_offset + g.height) as f32 * scale) as i32;
                out.quads.push(GlyphQuad {
                    x0: pen as f32,
                    y0: y + top as f32,
                    x1: (pen + w) as f32,
                    y1: y + bottom as f32,
                    uv0: g.uv0,
                    uv1: g.uv1,
                    c0,
                    c1,
                });
            }
            pen += wpx(g) + g.c as i32;
            if let Some(&(nc, ..)) = line.get(n + 1) {
                pen += style.kerning(c as u16, nc as u16);
            }
            prev_c = g.c as i32;
        }
        out.height += line_h;
    }
    out
}

/// `GraphicsFont2::GetStyle` fallback (`GraphicsFont2::FindFallbackStyle`, Engine).
pub fn pick_style(
    font: &crate::fnt::BitmapFont,
    size: u32,
    flags: u32,
) -> Option<&crate::fnt::Style> {
    if let Some(s) = font.style(size, flags) {
        return Some(s);
    }
    let mut best: Option<(&crate::fnt::Style, i32, i32)> = None;
    for s in &font.styles {
        let d = (size as i32 - s.size as i32).abs();
        let m = if s.flags == flags {
            i32::MAX
        } else {
            (s.flags & flags & 0x1f).count_ones() as i32
        };
        best = match best {
            Some((_, bd, bm)) if d > bd || (d == bd && m <= bm) => best,
            _ => Some((s, d, m)),
        };
    }
    best.map(|b| b.0)
}
