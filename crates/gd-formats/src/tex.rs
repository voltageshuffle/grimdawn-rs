//! `.tex` textures. A TEX file is a list of DDS frames (more than one = a
//! flipbook animated at `fps`). Each frame is a legacy DDS (no DX10 header)
//! whose magic is either `"DDS "` (mips largest-first, standard) or `"DDSR"`
//! (same header, mip chain stored smallest-first so it can be streamed).
//!
//! Loader: `GraphicsTexture::Initialize` (Engine.dll) splits
//! frames; `Direct3DTexture` ctor (Direct3D11.dll) maps the pixel
//! format. All formats are created as UNORM/SNORM, never `_SRGB`.
use crate::util::Reader;
use anyhow::{Result, bail, ensure};

/// Pixel formats the D3D11 renderer accepts (anything else is rejected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// FourCC DXT1 → DXGI BC1_UNORM (1-bit punch-through alpha honoured).
    Bc1,
    /// FourCC DXT3 → BC2_UNORM.
    Bc2,
    /// FourCC DXT5 → BC3_UNORM.
    Bc3,
    /// RGB 32 bpp → B8G8R8A8_UNORM (channel masks are ignored by the engine).
    Bgra8,
    /// RGB 24 bpp (B,G,R bytes) → expanded by the engine to B8G8R8X8_UNORM.
    /// `Dds::data` is already expanded to 4 bytes/pixel BGRA with A=255.
    Bgrx8,
    /// DDPF_BUMPDUDV 32 bpp → R8G8B8A8_SNORM.
    Rgba8Snorm,
    /// DDPF_BUMPDUDV 16 bpp → R8G8_SNORM.
    Rg8Snorm,
    /// DDPF_ALPHA → A8_UNORM (no wgpu equivalent; upload as R8 and swizzle).
    A8,
}

impl PixelFormat {
    pub fn is_compressed(self) -> bool {
        matches!(self, Self::Bc1 | Self::Bc2 | Self::Bc3)
    }
    /// Bytes per 4x4 block (compressed) or per pixel (uncompressed, after expansion).
    pub fn unit_bytes(self) -> usize {
        match self {
            Self::Bc1 => 8,
            Self::Bc2 | Self::Bc3 => 16,
            Self::Bgra8 | Self::Bgrx8 | Self::Rgba8Snorm => 4,
            Self::Rg8Snorm => 2,
            Self::A8 => 1,
        }
    }
    /// Size of one mip level as stored in the file (24-bit RGB = 3 bytes/pixel).
    fn file_level_size(self, w: u32, h: u32) -> usize {
        match self {
            Self::Bgrx8 => w.max(1) as usize * h.max(1) as usize * 3,
            f => f.level_size(w, h),
        }
    }
    pub fn level_size(self, w: u32, h: u32) -> usize {
        let (w, h) = (w.max(1) as usize, h.max(1) as usize);
        if self.is_compressed() {
            w.div_ceil(4) * h.div_ceil(4) * self.unit_bytes()
        } else {
            w * h * self.unit_bytes()
        }
    }
    /// Name of the matching `wgpu::TextureFormat` variant (A8 has none → R8Unorm).
    pub fn wgpu_name(self) -> &'static str {
        match self {
            Self::Bc1 => "Bc1RgbaUnorm",
            Self::Bc2 => "Bc2RgbaUnorm",
            Self::Bc3 => "Bc3RgbaUnorm",
            Self::Bgra8 | Self::Bgrx8 => "Bgra8Unorm",
            Self::Rgba8Snorm => "Rgba8Snorm",
            Self::Rg8Snorm => "Rg8Snorm",
            Self::A8 => "R8Unorm",
        }
    }
}

/// One DDS frame, normalised for upload: `data` holds every face (1, or 6 for
/// a cubemap, in D3D order +X -X +Y -Y +Z -Z), each face's mips largest-first
/// and tightly packed (wgpu `TextureDataOrder::LayerMajor`).
#[derive(Debug, Clone)]
pub struct Dds {
    pub width: u32,
    pub height: u32,
    /// Mip levels present in `data` (>= 1).
    pub mip_levels: u32,
    /// Header `dwMipMapCount` was 0: engine creates 1 level and calls
    /// `GenerateMips` (D3D11_RESOURCE_MISC_GENERATE_MIPS).
    pub generate_mips: bool,
    /// 1 or 6.
    pub faces: u32,
    pub format: PixelFormat,
    /// `DDSR` magic (mips were stored smallest-first; already reordered here).
    pub reversed: bool,
    /// Stored mips were fewer than the header claimed (data ended early).
    pub truncated: bool,
    pub data: Vec<u8>,
}

/// One frame's kept mip levels in upload order, as slices of the file bytes.
type LevelSlices<'a> = Vec<Vec<&'a [u8]>>;

impl Dds {
    pub fn level_dims(&self, level: u32) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }
    /// Byte range of (face, level) in `data`.
    pub fn level_range(&self, face: u32, level: u32) -> std::ops::Range<usize> {
        let face_size: usize = (0..self.mip_levels)
            .map(|l| {
                let (w, h) = self.level_dims(l);
                self.format.level_size(w, h)
            })
            .sum();
        let mut off = face as usize * face_size;
        for l in 0..level {
            let (w, h) = self.level_dims(l);
            off += self.format.level_size(w, h);
        }
        let (w, h) = self.level_dims(level);
        off..off + self.format.level_size(w, h)
    }
    pub fn level(&self, face: u32, level: u32) -> &[u8] {
        &self.data[self.level_range(face, level)]
    }

    pub fn parse(b: &[u8]) -> Result<Self> {
        let (mut dds, levels, _) = Self::gather(b)?;
        dds.data = Self::copy_levels(&dds, &levels);
        Ok(dds)
    }

    /// [`parse`](Self::parse) on an owned buffer: when the payload is already
    /// the upload data (not `DDSR`, no 24-bit expansion), the header is dropped
    /// in place instead of copying the pixels into a new allocation.
    pub fn parse_owned(b: Vec<u8>) -> Result<Self> {
        Self::parse_owned_at(b, 0)
    }

    /// `parse_owned` with the DDS header at `hdr` (a TEX frame's sits after
    /// the TEX header and the frame size).
    fn parse_owned_at(mut b: Vec<u8>, hdr: usize) -> Result<Self> {
        let (mut dds, levels, used) = Self::gather(&b[hdr..])?;
        // Reversed mips need reordering and 24-bit RGB expansion; a truncated
        // cube's kept bytes are not contiguous (the first faces hold levels
        // the later ones lack). Everything else is the payload as stored.
        if dds.reversed || dds.format == PixelFormat::Bgrx8 || (dds.faces > 1 && dds.truncated) {
            dds.data = Self::copy_levels(&dds, &levels);
            return Ok(dds);
        }
        drop(levels);
        b.drain(..hdr + 128);
        b.truncate(used);
        dds.data = b;
        Ok(dds)
    }

    /// The header fields (`data` empty), the kept levels in upload order as
    /// file slices, and the payload bytes they consumed.
    fn gather(b: &[u8]) -> Result<(Self, LevelSlices<'_>, usize)> {
        ensure!(b.len() >= 128, "DDS too short ({} bytes)", b.len());
        let reversed = match &b[..4] {
            b"DDS " => false,
            b"DDSR" => true,
            m => bail!("bad DDS magic {m:?}"),
        };
        let mut r = Reader::at(b, 8);
        let flags = r.u32()?;
        let height = r.u32()?;
        let width = r.u32()?;
        r.skip(8)?; // pitch, depth
        let mip_count = r.u32()?;
        // `width >> l` below: a shift of 32 or more panics.
        ensure!(mip_count <= 32, "DDS mip count {mip_count}");
        // A crafted header can claim u32::MAX dims; the level-size math would overflow.
        ensure!(
            width <= 16384 && height <= 16384,
            "DDS dimensions {width}x{height}"
        );
        ensure!(flags & 0x1000 != 0, "DDSD_PIXELFORMAT not set");
        let format = Self::pixel_format(b)?;
        let caps2 = u32::from_le_bytes(b[112..116].try_into().unwrap());
        let cube = caps2 & 0x200 != 0;
        if cube {
            ensure!(caps2 & 0xFC00 == 0xFC00, "partial cubemap");
            ensure!(width == height, "non-square cubemap");
        }
        let faces = if cube { 6 } else { 1 };
        let generate_mips = mip_count == 0;
        let want = mip_count.max(1);
        let payload = &b[128..];
        let size = |l: u32| format.file_level_size(width >> l, height >> l);

        // Gather (face, level) slices in file order. A `DDSR` mip chain is stored smallest-first;
        // a cubemap or a chain without mips is gathered largest-first whatever its magic.
        let small_first = reversed && !cube && mip_count != 0;
        let mut levels: Vec<Vec<&[u8]>> = vec![Vec::new(); faces as usize];
        let mut off = 0usize;
        let mut truncated = false;
        if small_first {
            // Smallest level first; a short file simply lacks the big levels.
            let mut rev = Vec::new();
            for l in (0..want).rev() {
                let n = size(l);
                if off + n > payload.len() {
                    truncated = true;
                    break;
                }
                rev.push(&payload[off..off + n]);
                off += n;
            }
            rev.reverse();
            levels[0] = rev;
        } else {
            'outer: for face in levels.iter_mut() {
                for l in 0..want {
                    let n = size(l);
                    if off + n > payload.len() {
                        truncated = true;
                        break 'outer;
                    }
                    face.push(&payload[off..off + n]);
                    off += n;
                }
            }
        }
        let have = levels.iter().map(|f| f.len()).min().unwrap_or(0) as u32;
        ensure!(have > 0, "DDS has no complete mip level");
        // With a smallest-first, truncated chain the surviving levels are the small
        // ones: the top level is then smaller than the header size.
        let skip = if small_first { want - have } else { 0 };
        let (width, height) = ((width >> skip).max(1), (height >> skip).max(1));
        for face in &mut levels {
            face.truncate(have as usize);
        }
        Ok((
            Self {
                width,
                height,
                mip_levels: have,
                generate_mips,
                faces,
                format,
                reversed,
                truncated,
                data: Vec::new(),
            },
            levels,
            off,
        ))
    }

    /// The pixel format from the DDS header's pixel-format block (offset 80).
    fn pixel_format(b: &[u8]) -> Result<PixelFormat> {
        let mut p = Reader::at(b, 80);
        let pf_flags = p.u32()?;
        let fourcc = p.bytes(4)?;
        let bits = p.u32()?;
        Ok(if pf_flags & 0x40 != 0 {
            match bits {
                24 => PixelFormat::Bgrx8,
                32 => PixelFormat::Bgra8,
                n => bail!("unsupported RGB bit count {n}"),
            }
        } else if pf_flags & 0x4 != 0 {
            match fourcc {
                b"DXT1" => PixelFormat::Bc1,
                b"DXT3" => PixelFormat::Bc2,
                b"DXT5" => PixelFormat::Bc3,
                f => bail!("unsupported FourCC {f:?}"),
            }
        } else if pf_flags & 0x80000 != 0 {
            match bits {
                32 => PixelFormat::Rgba8Snorm,
                16 => PixelFormat::Rg8Snorm,
                n => bail!("unsupported BUMPDUDV bit count {n}"),
            }
        } else if pf_flags & 0x2 != 0 {
            PixelFormat::A8
        } else {
            bail!("unsupported pixel format flags {pf_flags:#x}");
        })
    }

    /// The upload payload copied from the kept level slices (24-bit RGB
    /// expanded to BGRA with A=255).
    fn copy_levels(dds: &Self, levels: &[Vec<&[u8]>]) -> Vec<u8> {
        let mut data = Vec::new();
        for face in levels {
            for lvl in face {
                if dds.format == PixelFormat::Bgrx8 {
                    for px in lvl.as_chunks::<3>().0 {
                        data.extend_from_slice(&[px[0], px[1], px[2], 255]);
                    }
                } else {
                    data.extend_from_slice(lvl);
                }
            }
        }
        data
    }

    /// Decode (face 0, `level`) to straight RGBA8, rows top to bottom.
    pub fn decode_rgba8(&self, level: u32) -> Result<Vec<u8>> {
        self.decode_level_rgba8(0, level)
    }

    /// Top mip of one cubemap face (D3D order +X −X +Y −Y +Z −Z) as RGBA8.
    pub fn decode_face_rgba8(&self, face: u32) -> Result<Vec<u8>> {
        self.decode_level_rgba8(face, 0)
    }

    /// Decode (`face`, `level`) to straight RGBA8, rows top to bottom; an error for a face or
    /// level the frame does not have.
    pub fn decode_level_rgba8(&self, face: u32, level: u32) -> Result<Vec<u8>> {
        ensure!(
            face < self.faces && level < self.mip_levels,
            "no face {face} / level {level} ({} faces, {} levels)",
            self.faces,
            self.mip_levels
        );
        let (w, h) = self.level_dims(level);
        let src = self.level(face, level);
        let n = (w * h) as usize;
        let mut out = vec![0u8; n * 4];
        match self.format {
            PixelFormat::Bc1 | PixelFormat::Bc2 | PixelFormat::Bc3 => {
                let (block_bytes, decode): (_, BlockDecoder) = match self.format {
                    PixelFormat::Bc1 => (8, texture2ddecoder::decode_bc1a_block),
                    PixelFormat::Bc2 => (16, texture2ddecoder::decode_bc2_block),
                    _ => (16, texture2ddecoder::decode_bc3_block),
                };
                bc_rgba8_into(src, w as usize, h as usize, block_bytes, decode, &mut out)
                    .map_err(|e| anyhow::anyhow!("BC decode: {e}"))?;
            }
            PixelFormat::Bgra8 | PixelFormat::Bgrx8 => {
                for (o, p) in out
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src.as_chunks::<4>().0)
                {
                    o.copy_from_slice(&[p[2], p[1], p[0], p[3]]);
                }
            }
            PixelFormat::Rgba8Snorm => {
                for (o, p) in out
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src.as_chunks::<4>().0)
                {
                    for i in 0..4 {
                        o[i] = (p[i] as i8 as i32 + 128) as u8;
                    }
                }
            }
            PixelFormat::Rg8Snorm => {
                for (o, p) in out
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src.as_chunks::<2>().0)
                {
                    o.copy_from_slice(&[
                        (p[0] as i8 as i32 + 128) as u8,
                        (p[1] as i8 as i32 + 128) as u8,
                        128,
                        255,
                    ]);
                }
            }
            PixelFormat::A8 => {
                for (o, &a) in out.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                    o.copy_from_slice(&[255, 255, 255, a]);
                }
            }
        }
        Ok(out)
    }
}

pub(crate) type BlockDecoder = fn(&[u8], &mut [u32]);

/// Decode one BC level to straight RGBA8 into `out` (`w * h * 4` bytes, rows top to
/// bottom): each 4×4 block is decoded into a stack buffer and swizzled BGRA→RGBA straight
/// into the output — one heap buffer, one pass, where the bulk decoders write a `u32`
/// image that needs a second buffer and pass to swizzle. Same block decoders and edge
/// clipping as `texture2ddecoder`'s bulk calls, so the bytes are identical.
pub(crate) fn bc_rgba8_into(
    src: &[u8],
    w: usize,
    h: usize,
    block_bytes: usize,
    decode_block: BlockDecoder,
    out: &mut [u8],
) -> Result<(), &'static str> {
    let (bx, by) = (w.div_ceil(4), h.div_ceil(4));
    if src.len() < bx * by * block_bytes {
        return Err("Not enough data to decode image!");
    }
    if out.len() < w * h * 4 {
        return Err("Image buffer is too small!");
    }
    let mut block = [0u32; 16];
    let mut at = 0;
    for y in 0..by {
        for x in 0..bx {
            decode_block(&src[at..], &mut block);
            at += block_bytes;
            for r in 0..4 {
                let py = y * 4 + r;
                if py >= h {
                    break;
                }
                let row = &mut out[(py * w + x * 4) * 4..][..(w - x * 4).min(4) * 4];
                for (o, p) in row.as_chunks_mut::<4>().0.iter_mut().zip(&block[r * 4..]) {
                    let [b, g, r, a] = p.to_le_bytes();
                    o.copy_from_slice(&[r, g, b, a]);
                }
            }
        }
    }
    Ok(())
}

/// A parsed `.tex` file.
#[derive(Debug, Clone)]
pub struct Tex {
    /// Byte 3 of the magic (stock files: 2). The engine only checks `"TEX"`.
    pub version: u8,
    /// Flipbook frame rate (u32 in file, stored as float by the engine;
    /// 0 for static textures; engine default for bare DDS is 15).
    pub fps: u32,
    pub frames: Vec<Dds>,
}

impl Tex {
    /// Parse a `.tex` (or a bare DDS, which the engine also accepts).
    pub fn parse(b: &[u8]) -> Result<Self> {
        if b.starts_with(b"DDS ") {
            return Ok(Self {
                version: 0,
                fps: 15,
                frames: vec![Dds::parse(b)?],
            });
        }
        ensure!(b.len() >= 8 && &b[..3] == b"TEX", "not a TEX file");
        let version = b[3];
        let mut r = Reader::at(b, 4);
        let fps = r.u32()?;
        let mut frames = Vec::new();
        while r.remaining() > 0 {
            let n = r.u32()? as usize;
            frames.push(Dds::parse(r.bytes(n)?)?);
        }
        ensure!(!frames.is_empty(), "TEX without frames");
        Ok(Self {
            version,
            fps,
            frames,
        })
    }

    /// [`parse`](Self::parse) on an owned buffer: a single-frame file hands its
    /// buffer to the frame parser, which drops the headers in place instead of
    /// copying the pixels ([`Dds::parse_owned`]).
    pub fn parse_owned(b: Vec<u8>) -> Result<Self> {
        if b.starts_with(b"DDS ") {
            return Ok(Self {
                version: 0,
                fps: 15,
                frames: vec![Dds::parse_owned(b)?],
            });
        }
        if b.len() >= 12 && &b[..3] == b"TEX" {
            let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
            if n == b.len() - 12 {
                return Ok(Self {
                    version: b[3],
                    fps: u32::from_le_bytes(b[4..8].try_into().unwrap()),
                    frames: vec![Dds::parse_owned_at(b, 12)?],
                });
            }
        }
        Self::parse(&b)
    }

    /// The pixel dims of frame 0 from a header prefix alone — `hdr`: the file's first
    /// bytes (≥ 140 for TEX, ≥ 128 for a bare DDS), `file_len`: the whole entry's size —
    /// when they are provably what [`parse`](Self::parse) returns: a plain `DDS ` frame
    /// (not `DDSR`), one face, a supported format, and the top mip inside the frame's
    /// payload. `None` when the answer needs the payload (reversed mips, cubemap,
    /// multi-frame TEX) or the header is malformed: parse the whole file then.
    pub fn header_dims(hdr: &[u8], file_len: u64) -> Option<(u32, u32)> {
        let (hdr_len, dds) = if hdr.starts_with(b"DDS ") {
            (0, hdr)
        } else {
            // TEX: only a single-frame file, whose frame 0 is the whole payload.
            if hdr.get(..3)? != b"TEX" {
                return None;
            }
            let n = u32::from_le_bytes(hdr.get(8..12)?.try_into().ok()?) as u64;
            if file_len != 12 + n {
                return None;
            }
            (12, hdr.get(12..)?)
        };
        if dds.len() < 128 || !dds.starts_with(b"DDS ") {
            return None;
        }
        let mut r = Reader::at(dds, 8);
        let flags = r.u32().ok()?;
        let height = r.u32().ok()?;
        let width = r.u32().ok()?;
        r.skip(8).ok()?; // pitch, depth
        let mip_count = r.u32().ok()?;
        if mip_count > 32 || width > 16384 || height > 16384 || flags & 0x1000 == 0 {
            return None;
        }
        let format = Dds::pixel_format(dds).ok()?;
        let caps2 = u32::from_le_bytes(dds[112..116].try_into().ok()?);
        if caps2 & 0x200 != 0 {
            return None; // cubemap: the kept levels depend on the payload
        }
        // A plain chain keeps the header dims iff its top mip fits (`gather`'s `have > 0`).
        let payload = file_len.checked_sub(hdr_len + 128)?;
        if payload < format.file_level_size(width, height) as u64 {
            return None;
        }
        Some((width, height))
    }
}
