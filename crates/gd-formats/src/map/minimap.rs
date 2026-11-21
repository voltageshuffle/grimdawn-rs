//! Region minimap images (the `.map` chunk 0x19 TGAs, `Region::GetMinimapImage`).
//!
//! Each region has one 32-bit TGA, uncompressed or run-length encoded (type 10,
//! e.g. the Shattered Realm regions) (stock: 256 × 256, 2 px per metre for a
//! 128 m region), its rows taken in file order (the header's bottom-up origin
//! bit is not applied, as the engine uploads the rows as stored; PORT: matched
//! to the real game's local map): the first row is the region's high-z edge.
//! Drawing them (the fog passes included) is presentation code, kept with the
//! port's local map UI.
use super::MapFile;
use anyhow::{Result, bail};

/// A decoded region minimap: RGBA8 rows in file order, the first row at the
/// region's high-z edge.
#[derive(Clone, Debug)]
pub struct MinimapImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl MinimapImage {
    /// Decodes a 24 / 32-bit TGA, uncompressed (type 2) or run-length
    /// encoded (type 10: the Shattered Realm's `levels/endlessdungeon` regions).
    pub fn from_tga(t: &[u8]) -> Result<Self> {
        if t.len() < 18 || (t[2] != 2 && t[2] != 10) {
            bail!(
                "unsupported minimap TGA type {}",
                t.get(2).copied().unwrap_or(0)
            );
        }
        let idlen = t[0] as usize;
        let w = u16::from_le_bytes([t[12], t[13]]) as u32;
        let h = u16::from_le_bytes([t[14], t[15]]) as u32;
        if w == 0 || h == 0 {
            bail!("minimap TGA {w}x{h}");
        }
        let bpp = t[16] as usize / 8;
        if bpp != 3 && bpp != 4 {
            bail!("minimap TGA {bpp} bytes per pixel");
        }
        // The rows are used in file order whatever the TGA's origin bit (the
        // stock files say bottom-up): the texture's first row is the file's
        // first (checked against the real local map's orientation).
        let n = (w as usize) * (h as usize) * bpp;
        let Some(src) = t.get(18 + idlen..) else {
            bail!("minimap TGA id length {idlen} past end");
        };
        let rle;
        let data = if t[2] == 10 {
            rle = unrle(src, bpp, n)?;
            &rle[..]
        } else {
            src
        };
        if data.len() < n {
            bail!("minimap TGA truncated");
        }
        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        for y in 0..h {
            for x in 0..w {
                let s = ((y * w + x) as usize) * bpp;
                let d = (y * w + x) as usize * 4;
                rgba[d] = data[s + 2];
                rgba[d + 1] = data[s + 1];
                rgba[d + 2] = data[s];
                rgba[d + 3] = if bpp == 4 { data[s + 3] } else { 255 };
            }
        }
        Ok(Self { w, h, rgba })
    }
}

/// A run-length encoded (type 10) TGA's pixel bytes: packets of a header
/// byte (high bit: one pixel repeated, else literal pixels; low 7 bits + 1 =
/// the count).
fn unrle(src: &[u8], bpp: usize, n: usize) -> Result<Vec<u8>> {
    // A packet spends >= 1 + bpp input bytes for <= 128 * bpp output bytes,
    // so the decode can never need more than 128x the input.
    let mut out = Vec::with_capacity(n.min(src.len().saturating_mul(128)));
    let mut i = 0;
    while out.len() < n {
        let Some(&hd) = src.get(i) else {
            bail!("minimap TGA RLE truncated");
        };
        i += 1;
        let count = (hd & 0x7f) as usize + 1;
        let len = if hd & 0x80 != 0 { bpp } else { bpp * count };
        let Some(px) = src.get(i..i + len) else {
            bail!("minimap TGA RLE truncated");
        };
        i += len;
        if hd & 0x80 != 0 {
            for _ in 0..count {
                out.extend_from_slice(px);
            }
        } else {
            out.extend_from_slice(px);
        }
    }
    out.truncate(n);
    Ok(out)
}

impl MapFile {
    /// Region `i`'s minimap image, if it has one.
    pub fn minimap_image(&self, i: usize) -> Result<Option<MinimapImage>> {
        self.minimap_bytes(i)?
            .map(|t| MinimapImage::from_tga(&t))
            .transpose()
    }
}
