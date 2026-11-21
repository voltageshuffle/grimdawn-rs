//! Text with the game's own fonts.
//!
//! FNTX (bitmap) fonts are laid out exactly like `GraphicsFont2`
//! ([`gd_formats::ui::layout_text`]) and rasterised on the CPU into one image
//! per text widget: each glyph quad samples the atlas bilinearly and is shaded
//! with the style's top→bottom colour pair (`fontColor0` → `fontColor1`, per
//! glyph quad as the engine's vertex colours do). FNTD fonts carry a TrueType
//! file which is handed to Bevy as a regular [`Font`].
use crate::data::GameData;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use gd_formats::fnt::{BitmapFont, Font as GdFont};
use gd_formats::text::TagTable;
use gd_formats::ui::{FontStyle, TextLayout, layout_text};
use std::collections::HashMap;
use std::sync::Arc;

pub enum LoadedFont {
    Bitmap { font: BitmapFont, atlas: Vec<u8> },
    Dynamic(Handle<Font>),
}

/// A rasterised text block cached as an uploaded GPU image ([`FontCache::rasterize_cached`]):
/// the same string reuses its image instead of rasterising and uploading again. `image`
/// covers `[offset, offset + size)` relative to the layout origin.
#[derive(Clone)]
pub struct CachedText {
    pub image: Handle<Image>,
    pub offset: Vec2,
    pub size: UVec2,
    pub layout: TextLayout,
}

/// The raster cache key: the font, pixel size, style flags, both glyph colours and the
/// string (colours by bit pattern so equal colours hash equal).
#[derive(Clone, PartialEq, Eq, Hash)]
struct RasterKey {
    font: String,
    size: u32,
    flags: u32,
    color0: [u32; 3],
    color1: [u32; 3],
    text: String,
}

/// An LRU of rasterised texts. The cap bounds the live textures: an evicted entry drops
/// its handle and Bevy frees the image once no node references it any more.
#[derive(Default)]
struct RasterCache {
    entries: HashMap<RasterKey, (CachedText, u64)>,
    tick: u64,
}

/// The raster cache's entry cap.
const RASTER_CAP: usize = 512;

/// Text class → (font style record, indent) from `gameengine.dbr` (`GameEngine::GetGameTextStyleName` /
/// `GetGameTextStyleIndent`: the class's key of `gd_sim::items::tooltip::CLASS_KEYS` and its `{key}Indent`), read on a
/// class's first use. One cache for the item tooltip (`tooltip.rs`) and the skill-style tooltip (`tip.rs`).
#[derive(Default)]
struct ClassStyles(HashMap<u8, (Option<Arc<str>>, f32)>);

/// Fonts, font styles, text-class styles and localisation tags, loaded on demand.
#[derive(Resource, Default)]
pub struct FontCache {
    fonts: HashMap<String, Option<Arc<LoadedFont>>>,
    styles: HashMap<String, Option<FontStyle>>,
    classes: ClassStyles,
    pub tags: Arc<TagTable>,
    raster: RasterCache,
}

impl FontCache {
    /// Takes the mounted data's tags and drops the fonts and styles loaded from other data (a mod can
    /// override font files and style records).
    pub fn load_tags(&mut self, data: &GameData) {
        self.fonts.clear();
        self.styles.clear();
        self.classes.0.clear();
        self.raster = RasterCache::default();
        self.tags = data.tags();
        info!("ui: {} text tags", self.tags.tags.len());
    }

    /// A text class's font style record (`None` when `gameengine.dbr` names none) and indent (unscaled px).
    pub fn class_style(&mut self, data: &GameData, class: u8) -> (Option<Arc<str>>, f32) {
        self.classes
            .0
            .entry(class)
            .or_insert_with(|| {
                let engine = data.db.get("records/game/gameengine.dbr");
                let key = gd_sim::items::tooltip::CLASS_KEYS.get(class as usize);
                match (engine, key) {
                    (Some(r), Some(key)) => (
                        r.str(key).map(Arc::from),
                        r.f32(&format!("{key}Indent")).unwrap_or(0.0),
                    ),
                    _ => (None, 0.0),
                }
            })
            .clone()
    }

    /// [`rasterize`] through the LRU: the same (font, size, flags, colours, string) reuses
    /// its uploaded image instead of rasterising and uploading a new one (the orb numbers,
    /// cooldown seconds and floating numbers change often but repeat their strings).
    pub fn rasterize_cached(
        &mut self,
        images: &mut Assets<Image>,
        font: &BitmapFont,
        atlas: &[u8],
        style: &FontStyle,
        size: u32,
        text: &str,
    ) -> Option<CachedText> {
        let key = RasterKey {
            font: gd_formats::vfs::normalize(&style.font),
            size,
            flags: style.flags,
            color0: style.color0.map(f32::to_bits),
            color1: style.color1.map(f32::to_bits),
            text: text.to_owned(),
        };
        self.raster.tick += 1;
        let tick = self.raster.tick;
        if let Some((cached, used)) = self.raster.entries.get_mut(&key) {
            *used = tick;
            return Some(cached.clone());
        }
        let ti = rasterize(font, atlas, style, size, text)?;
        let cached = CachedText {
            image: images.add(ti.image),
            offset: ti.offset,
            size: ti.size,
            layout: ti.layout,
        };
        if self.raster.entries.len() >= RASTER_CAP
            && let Some(old) = self
                .raster
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            self.raster.entries.remove(&old);
        }
        self.raster.entries.insert(key, (cached.clone(), tick));
        Some(cached)
    }

    pub fn tag<'a>(&'a self, tag: &'a str) -> &'a str {
        self.tags.get(tag).unwrap_or(tag)
    }

    pub fn style(&mut self, data: &GameData, path: &str) -> Option<FontStyle> {
        self.styles
            .entry(path.to_ascii_lowercase())
            .or_insert_with(|| data.db.get(path).map(FontStyle::from_record))
            .clone()
    }

    /// The width of `text` in the style at UI scale `s` (the bitmap font's layout; a rough estimate while the font is not loaded).
    pub fn text_width(
        &mut self,
        data: &GameData,
        assets: &mut Assets<Font>,
        style_path: &str,
        s: f32,
        text: &str,
    ) -> f32 {
        let Some(style) = self.style(data, style_path) else {
            return 0.0;
        };
        let size = style.scaled_size(s);
        match self.font(data, &style.font, assets).as_deref() {
            Some(LoadedFont::Bitmap { font, .. }) => {
                gd_formats::ui::layout_text(
                    font,
                    size,
                    style.flags,
                    text,
                    (style.color0, style.color1),
                )
                .width as f32
            }
            _ => text.chars().count() as f32 * size as f32 * 0.5,
        }
    }

    pub fn font(
        &mut self,
        data: &GameData,
        path: &str,
        fonts: &mut Assets<Font>,
    ) -> Option<Arc<LoadedFont>> {
        let key = gd_formats::vfs::normalize(path);
        if let Some(f) = self.fonts.get(&key) {
            return f.clone();
        }
        let loaded = data
            .vfs
            .read(&key)
            .and_then(|b| GdFont::parse(&b))
            .and_then(|f| match f {
                GdFont::Bitmap(font) => {
                    let atlas = font.atlas_rgba8()?;
                    Ok(LoadedFont::Bitmap { font, atlas })
                }
                GdFont::Dynamic(d) => Ok(LoadedFont::Dynamic(
                    fonts.add(Font::from_bytes(d.font_file)),
                )),
            });
        let v = match loaded {
            Ok(f) => Some(Arc::new(f)),
            Err(e) => {
                warn!("ui font {key}: {e:#}");
                None
            }
        };
        self.fonts.insert(key, v.clone());
        v
    }
}

/// A rasterised text block: `image` covers `[offset, offset + size)` relative
/// to the layout origin.
pub struct TextImage {
    pub image: Image,
    pub offset: Vec2,
    pub size: UVec2,
    pub layout: TextLayout,
}

/// Lays out and rasterises `text` (markup allowed) in `style` at pixel size `size`.
pub fn rasterize(
    font: &BitmapFont,
    atlas: &[u8],
    style: &FontStyle,
    size: u32,
    text: &str,
) -> Option<TextImage> {
    let layout = layout_text(font, size, style.flags, text, (style.color0, style.color1));
    if layout.quads.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for q in &layout.quads {
        x0 = x0.min(q.x0);
        y0 = y0.min(q.y0);
        x1 = x1.max(q.x1);
        y1 = y1.max(q.y1);
    }
    let (w, h) = (
        (x1 - x0).ceil().max(1.0) as u32,
        (y1 - y0).ceil().max(1.0) as u32,
    );
    let mut px = vec![0f32; (w * h * 4) as usize];
    let (aw, ah) = (font.atlas_width as usize, font.atlas_height as usize);
    let sample = |u: f32, v: f32| -> [f32; 4] {
        // bilinear, clamp
        let fx = (u * aw as f32 - 0.5).clamp(0.0, (aw - 1) as f32);
        let fy = (v * ah as f32 - 0.5).clamp(0.0, (ah - 1) as f32);
        let (ix, iy) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let (jx, jy) = ((ix + 1).min(aw - 1), (iy + 1).min(ah - 1));
        let at = |x: usize, y: usize| {
            let i = (y * aw + x) * 4;
            [
                atlas[i] as f32,
                atlas[i + 1] as f32,
                atlas[i + 2] as f32,
                atlas[i + 3] as f32,
            ]
        };
        let (a, b, c, d) = (at(ix, iy), at(jx, iy), at(ix, jy), at(jx, jy));
        let mut o = [0f32; 4];
        for k in 0..4 {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bot = c[k] + (d[k] - c[k]) * tx;
            o[k] = (top + (bot - top) * ty) / 255.0;
        }
        o
    };
    for q in &layout.quads {
        let (qw, qh) = (q.x1 - q.x0, q.y1 - q.y0);
        if qw <= 0.0 || qh <= 0.0 {
            continue;
        }
        let (sx, sy) = ((q.x0 - x0) as i32, (q.y0 - y0) as i32);
        for j in 0..qh as i32 {
            let t = (j as f32 + 0.5) / qh;
            let v = q.uv0[1] + (q.uv1[1] - q.uv0[1]) * t;
            let col = [0, 1, 2].map(|k| q.c0[k] + (q.c1[k] - q.c0[k]) * t);
            for i in 0..qw as i32 {
                let u = q.uv0[0] + (q.uv1[0] - q.uv0[0]) * (i as f32 + 0.5) / qw;
                let s = sample(u, v);
                let (dx, dy) = (sx + i, sy + j);
                if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                    continue;
                }
                let o = ((dy as u32 * w + dx as u32) * 4) as usize;
                let a = s[3];
                let src = [s[0] * col[0], s[1] * col[1], s[2] * col[2]];
                // straight-alpha "over"
                let da = px[o + 3];
                let oa = a + da * (1.0 - a);
                if oa > 0.0 {
                    for k in 0..3 {
                        px[o + k] = (src[k] * a + px[o + k] * da * (1.0 - a)) / oa;
                    }
                }
                px[o + 3] = oa;
            }
        }
    }
    let bytes: Vec<u8> = px
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect();
    let image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        // Engine colours are gamma-space values: store them as sRGB so the
        // UI pass writes them back unchanged.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    Some(TextImage {
        image,
        offset: Vec2::new(x0, y0),
        size: UVec2::new(w, h),
        layout,
    })
}

/// The active `^X` colour at the end of `s` (to carry over a wrap).
pub fn active_color(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '^'
            && let Some(n) = it.next()
        {
            if n == '^' {
                continue;
            }
            out = if n == '-' {
                String::new()
            } else {
                format!("^{n}")
            };
        }
    }
    out
}

/// The laid-out width (px) of a markup string in a bitmap font, as [`wrap_by`] measures it.
pub fn bitmap_width(font: &gd_formats::fnt::BitmapFont, size: u32, flags: u32, text: &str) -> f32 {
    layout_text(font, size, flags, text, ([1.0; 3], [1.0; 3])).width as f32
}

/// Greedy word wrap of a markup string to `max` px wide by `measure` (the one wrap routine): words split
/// on spaces, the active colour code carried to each continuation line.
pub fn wrap_by(text: &str, max: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cand = String::new();
    for word in text.split(' ') {
        if cur.is_empty() {
            cur.push_str(word);
            continue;
        }
        cand.clear();
        cand.push_str(&cur);
        cand.push(' ');
        cand.push_str(word);
        if measure(&cand) > max {
            let carry = active_color(&cur);
            lines.push(std::mem::take(&mut cur));
            cur.push_str(&carry);
            cur.push_str(word);
        } else {
            std::mem::swap(&mut cur, &mut cand);
        }
    }
    lines.push(cur);
    lines
}
