use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};

/// Little-endian cursor over a byte slice.
#[derive(Clone)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    /// The capacity to reserve for `count` elements of at least `min_size` bytes each: no more
    /// than the bytes left can hold, so a count from a corrupt file cannot ask for a huge
    /// allocation (the element reads then fail with `Err` at the end of the data).
    pub fn capacity(&self, count: usize, min_size: usize) -> usize {
        count.min(self.remaining() / min_size.max(1))
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            bail!(
                "read of {n} bytes at {:#x} past end ({:#x})",
                self.pos,
                self.data.len()
            );
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    /// u32 length-prefixed string (Latin-1/UTF-8 lossy).
    pub fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        Ok(latin1(self.bytes(n)?))
    }
}

/// Asset text: UTF-8 when valid, else bytes as Latin-1. Not byte-exact; saves that must round-trip
/// use [`crate::save::cipher::bytes_to_string`].
pub fn latin1(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

/// C `atof` (the CRT's `strtod` in the C locale), as the engine reads numbers out of text: leading white space, an
/// optional sign, digits with at most one `.`, an optional exponent (`e` / `E`, a sign, digits; left out when it
/// has no digits). The longest such prefix, else 0. Hex floats, `inf` and `nan` are not read (no text the port
/// parses has them).
pub fn atof(s: &str) -> f64 {
    let s = s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let b = s.as_bytes();
    let digits = |from: usize| {
        b.get(from..)
            .map_or(0, |t| t.iter().take_while(|c| c.is_ascii_digit()).count())
    };
    let mut end = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int = digits(end);
    end += int;
    let mut frac = 0;
    if b.get(end) == Some(&b'.') {
        frac = digits(end + 1);
        end += 1 + frac;
    }
    if int + frac == 0 {
        return 0.0;
    }
    if matches!(b.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(end + 1), Some(b'+' | b'-')));
        let exp = digits(end + 1 + sign);
        if exp > 0 {
            end += 1 + sign + exp;
        }
    }
    s[..end].parse().unwrap_or(0.0)
}

/// The most bytes an LZ4 block of `compressed` bytes can decode to: a match run grows by at
/// most 255 bytes per input byte.
pub fn lz4_max_size(compressed: usize) -> usize {
    compressed.saturating_mul(255)
}

/// Decompresses an LZ4 block whose header claims `size` bytes. `lz4_flex` allocates `size` up
/// front, so a size the block cannot reach (a corrupt header) is refused instead of allocated.
pub fn lz4_block(src: &[u8], size: usize) -> Result<Vec<u8>> {
    if size > lz4_max_size(src.len()) {
        bail!("LZ4 block of {} bytes cannot hold {size} bytes", src.len());
    }
    Ok(lz4_flex::block::decompress(src, size)?)
}

/// Joins an archive / database entry `name` under `base` for extraction,
/// refusing names that would escape it: a root, a drive prefix or a `..`
/// component (`Path::join` replaces the base on an absolute path), so a
/// crafted mod archive cannot overwrite files outside the output directory.
pub fn safe_join(base: &Path, name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.components().any(|c| {
        matches!(
            c,
            Component::RootDir | Component::Prefix(_) | Component::ParentDir
        )
    }) {
        bail!("unsafe entry name {name:?}");
    }
    Ok(base.join(p))
}

/// MD5 (RFC 1321); the engine's `MD5Digest` (Game.dll `MD5Digest::Update` / `MD5Digest::Transform`).
/// Quest, task and conversation uids.
pub fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // floor(|sin(i + 1)| * 2^32)
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    fn block(h: &mut [u32; 4], chunk: &[u8; 64]) {
        let mut m = [0u32; 16];
        for (w, b) in m.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *w = u32::from_le_bytes(*b);
        }
        let [mut a, mut b, mut c, mut d] = *h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    let (full, rest) = data.as_chunks::<64>();
    for c in full {
        block(&mut h, c);
    }
    // Padding: 0x80, zeros, then the bit length, in one or two final blocks.
    let mut tail = [0u8; 128];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let n = if rest.len() < 56 { 64 } else { 128 };
    tail[n - 8..n].copy_from_slice(&(data.len() as u64).wrapping_mul(8).to_le_bytes());
    for c in tail[..n].as_chunks::<64>().0 {
        block(&mut h, c);
    }
    let mut out = [0u8; 16];
    for (o, w) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *o = w.to_le_bytes();
    }
    out
}
