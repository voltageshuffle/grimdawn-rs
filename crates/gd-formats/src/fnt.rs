//! `.fnt` fonts. Two kinds ship (loader `GraphicsFont2::Initialize`,
//! Engine.dll):
//! - `FNTX` v2: pre-rasterised bitmap font — styles (size + flags) with glyph
//!   metrics, kerning pairs, and the glyph atlas embedded at the end of the
//!   file (stock: DXT5, glyph coverage in alpha).
//! - `FNTD` v1: 28-byte header followed by an embedded TrueType/OpenType file
//!   that the engine rasterises at runtime into a dynamic texture.
use crate::util::{Reader, byte_list};
use anyhow::{Result, bail, ensure};

/// FNTX atlas pixel format code → device call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasFormat {
    /// Codes 2, 3: `CreateRawTexture(DXT1)`.
    Dxt1,
    /// Code 4: DXT3.
    Dxt3,
    /// Code 5 (all stock files): DXT5.
    Dxt5,
    /// Codes 1 / 6: uncompressed `CreateTexture(w, h, RenderTextureFormat 2 / 3)`
    /// (pixel layout unverified; not used by stock data).
    Raw(u32),
}

/// One glyph (28 bytes on disk, copied verbatim into `FNTMetric`).
/// Pixel fields are interpreted as Windows ABC widths: advance = a + b + c.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub code: u16,
    /// Left bearing (ABC "A"), pixels.
    pub a: i16,
    /// Right bearing (ABC "C"), pixels.
    pub c: i16,
    /// Bitmap width (ABC "B"), pixels.
    pub width: i16,
    pub height: i16,
    /// Top of the bitmap below the line top, pixels.
    pub y_offset: i16,
    /// Atlas rectangle, normalised [0,1] (top-left origin).
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
}

impl Glyph {
    pub fn advance(&self) -> i32 {
        self.a as i32 + self.width as i32 + self.c as i32
    }
}

#[derive(Debug, Clone)]
pub struct Style {
    /// Point size this style was rasterised at (lookup key high half).
    pub size: u32,
    /// `FontStyleFlag` (0 normal, 1 bold, 2 italic) — lookup key low half.
    pub flags: u32,
    /// Third header field (looks like the ascent/baseline in pixels).
    pub ascent: u32,
    /// Line height in pixels; `GetLineHeight(sz) = sz / size * line_height`.
    pub line_height: u32,
    /// Seventh header field (stored as u16 by the engine; 0 in stock files).
    pub extra: u32,
    /// Sorted by `code` (stable, so the first of a duplicated code wins).
    pub glyphs: Vec<Glyph>,
    /// (first, second) → kerning amount in pixels, sorted by (first, second).
    pub kerning: Vec<(u16, u16, i32)>,
}

impl Style {
    pub fn glyph(&self, c: u16) -> Option<&Glyph> {
        let i = self.glyphs.partition_point(|g| g.code < c);
        self.glyphs.get(i).filter(|g| g.code == c)
    }
    pub fn kerning(&self, first: u16, second: u16) -> i32 {
        let key = (first, second);
        let i = self.kerning.partition_point(|k| (k.0, k.1) < key);
        self.kerning
            .get(i)
            .filter(|k| (k.0, k.1) == key)
            .map_or(0, |k| k.2)
    }
}

#[derive(Debug, Clone)]
pub struct BitmapFont {
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub atlas_format_code: u32,
    pub atlas_format: AtlasFormat,
    /// Header field at +20 (= atlas byte size in stock files).
    pub atlas_size_field: u32,
    pub styles: Vec<Style>,
    /// Atlas pixels (remainder of the file).
    pub atlas: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DynamicFont {
    /// The five u32 header fields after the version (stock: 1, 1, 0, 0, 0; meaning unknown).
    pub header: [u32; 5],
    /// Embedded `.ttf`/`.otf` bytes.
    pub font_file: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Font {
    Bitmap(BitmapFont),
    Dynamic(DynamicFont),
}

impl Font {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(b.len() >= 8, "font file too short");
        let mut r = Reader::new(b);
        match r.bytes(4)? {
            b"FNTX" => {
                ensure!(r.u32()? == 2, "FNTX version != 2");
                let atlas_width = r.u32()?;
                let atlas_height = r.u32()?;
                let atlas_format_code = r.u32()?;
                let atlas_size_field = r.u32()?;
                let nstyles = r.u32()?;
                r.skip(4)?;
                let mut styles = Vec::new();
                for _ in 0..nstyles {
                    let size = r.u32()?;
                    let flags = r.u32()?;
                    let ascent = r.u32()?;
                    let line_height = r.u32()?;
                    let nglyphs = r.u32()?;
                    let nkern = r.u32()?;
                    let extra = r.u32()?;
                    let mut glyphs = Vec::with_capacity(r.capacity(nglyphs as usize, 28));
                    for _ in 0..nglyphs {
                        let code = r.u16()?;
                        let mut i = || -> Result<i16> { Ok(r.u16()? as i16) };
                        let (a, c, width, height, y_offset) = (i()?, i()?, i()?, i()?, i()?);
                        let uv0 = [r.f32()?, r.f32()?];
                        let uv1 = [r.f32()?, r.f32()?];
                        glyphs.push(Glyph {
                            code,
                            a,
                            c,
                            width,
                            height,
                            y_offset,
                            uv0,
                            uv1,
                        });
                    }
                    let mut kerning = Vec::with_capacity(r.capacity(nkern as usize, 8));
                    for _ in 0..nkern {
                        let second = r.u16()?;
                        let first = r.u16()?;
                        kerning.push((first, second, r.i32()?));
                    }
                    glyphs.sort_by_key(|g| g.code);
                    kerning.sort_by_key(|k| (k.0, k.1));
                    styles.push(Style {
                        size,
                        flags,
                        ascent,
                        line_height,
                        extra,
                        glyphs,
                        kerning,
                    });
                }
                let atlas_format = match atlas_format_code {
                    2 | 3 => AtlasFormat::Dxt1,
                    4 => AtlasFormat::Dxt3,
                    5 => AtlasFormat::Dxt5,
                    1 | 6 => AtlasFormat::Raw(atlas_format_code),
                    c => bail!("unknown FNTX atlas format {c}"),
                };
                let atlas = b[r.pos..].to_vec();
                if let Some(need) = Self::dxt_size(atlas_format, atlas_width, atlas_height) {
                    ensure!(
                        atlas.len() >= need,
                        "atlas truncated: {} < {need}",
                        atlas.len()
                    );
                }
                Ok(Font::Bitmap(BitmapFont {
                    atlas_width,
                    atlas_height,
                    atlas_format_code,
                    atlas_format,
                    atlas_size_field,
                    styles,
                    atlas,
                }))
            }
            b"FNTD" => {
                ensure!(r.u32()? == 1, "FNTD version != 1");
                let header = [r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?];
                Ok(Font::Dynamic(DynamicFont {
                    header,
                    font_file: b[28..].to_vec(),
                }))
            }
            m => bail!("bad font magic {}", byte_list(m, false)),
        }
    }

    /// The bytes a BC atlas of `w` × `h` needs (saturating: a crafted size cannot overflow the
    /// check); `None` for a raw atlas.
    fn dxt_size(f: AtlasFormat, w: u32, h: u32) -> Option<usize> {
        let block_bytes = match f {
            AtlasFormat::Dxt1 => 8,
            AtlasFormat::Dxt3 | AtlasFormat::Dxt5 => 16,
            AtlasFormat::Raw(_) => return None,
        };
        Some(
            (w as usize)
                .div_ceil(4)
                .saturating_mul((h as usize).div_ceil(4))
                .saturating_mul(block_bytes),
        )
    }
}

impl BitmapFont {
    /// Style lookup as `GetStyle`: exact (size, flags) match.
    pub fn style(&self, size: u32, flags: u32) -> Option<&Style> {
        self.styles
            .iter()
            .find(|s| s.size == size && s.flags == flags)
    }

    /// Decode the atlas to straight RGBA8 (BC formats only).
    pub fn atlas_rgba8(&self) -> Result<Vec<u8>> {
        let (w, h) = (self.atlas_width as usize, self.atlas_height as usize);
        let (block_bytes, decode): (_, crate::tex::BlockDecoder) = match self.atlas_format {
            AtlasFormat::Dxt1 => (8, texture2ddecoder::decode_bc1a_block),
            AtlasFormat::Dxt3 => (16, texture2ddecoder::decode_bc2_block),
            AtlasFormat::Dxt5 => (16, texture2ddecoder::decode_bc3_block),
            AtlasFormat::Raw(c) => bail!("raw atlas format {c} not supported"),
        };
        let mut out = vec![0u8; w * h * 4];
        crate::tex::bc_rgba8_into(&self.atlas, w, h, block_bytes, decode, &mut out)
            .map_err(|e| anyhow::anyhow!("atlas decode: {e}"))?;
        Ok(out)
    }
}
