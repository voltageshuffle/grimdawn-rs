//! The keyed XOR stream used by `GAME::CheckedReader` / `GAME::CheckedWriter`
//! (Game.dll) for `player.gdc`, `*.gdd`, `transfer.gst`, `transmutes.gst` and
//! `reagents.gst`.
//!
//! Per the decompiled stream helpers:
//! * file starts with `u32 seed ^ 0x55555555` (writer seeds from `timeGetTime()`);
//! * a 256-entry table: `t = seed; for i in 0..256 { t = ror(t,1) * 39916801; table[i] = t }`;
//! * running `key` starts at `seed`;
//! * encrypted u32: `cipher = plain ^ key` (all four bytes against the same key),
//!   then `key ^= table[c]` for each of the four cipher bytes;
//! * encrypted u16 (conversations only): `cipher = plain ^ (key & 0xffff)`, then two updates;
//! * encrypted u8: `cipher = plain ^ (key & 0xff)`, then `key ^= table[cipher]`;
//! * "raw" u32s (block length field, block-end / header-end check words) are
//!   stored without touching `key`;
//! * blocks: `u32 id` (encrypted), `u32 len ^ key` (raw), payload, then `u32 key` (raw)
//!   where `len` counts the payload bytes only.
use anyhow::{Result, bail};

pub const SEED_XOR: u32 = 0x5555_5555;

pub fn make_table(seed: u32) -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut v = seed;
    for e in t.iter_mut() {
        v = v.rotate_right(1).wrapping_mul(39_916_801);
        *e = v;
    }
    t
}

/// Bytes are kept 1:1 in `String` chars (Latin-1 style) so any byte string round-trips.
pub fn bytes_to_string(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

pub fn string_to_bytes(s: &str) -> Result<Vec<u8>> {
    s.chars()
        .map(|c| {
            let v = c as u32;
            if v > 0xff {
                bail!("character {c:?} does not fit a byte string");
            }
            Ok(v as u8)
        })
        .collect()
}

pub struct Decoder<'a> {
    data: &'a [u8],
    pub pos: usize,
    pub key: u32,
    pub seed: u32,
    table: [u32; 256],
}

impl<'a> Decoder<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 4 {
            bail!("file too short for a cipher seed");
        }
        let seed = u32::from_le_bytes(data[0..4].try_into().unwrap()) ^ SEED_XOR;
        Ok(Self {
            data,
            pos: 4,
            key: seed,
            seed,
            table: make_table(seed),
        })
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn at_end(&self) -> bool {
        self.pos == self.data.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
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

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        let v = u32::from_le_bytes(b.try_into().unwrap()) ^ self.key;
        for &c in b {
            self.key ^= self.table[c as usize];
        }
        Ok(v)
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        let v = u16::from_le_bytes(b.try_into().unwrap()) ^ (self.key as u16);
        for &c in b {
            self.key ^= self.table[c as usize];
        }
        Ok(v)
    }

    pub fn u8(&mut self) -> Result<u8> {
        let c = self.take(1)?[0];
        let v = c ^ (self.key as u8);
        self.key ^= self.table[c as usize];
        Ok(v)
    }

    /// Byte-wise decrypt of a 16-byte `UniqueId`.
    pub fn uid(&mut self) -> Result<[u8; 16]> {
        let mut out = [0u8; 16];
        for o in out.iter_mut() {
            *o = self.u8()?;
        }
        Ok(out)
    }

    /// `u32 len` + `len` encrypted bytes.
    pub fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n > self.data.len() {
            bail!("implausible string length {n} at {:#x}", self.pos);
        }
        let mut b = Vec::with_capacity(n);
        for _ in 0..n {
            b.push(self.u8()?);
        }
        Ok(bytes_to_string(&b))
    }

    /// `u32 len` + `len` UTF-16 units, each as two encrypted bytes.
    pub fn wstring(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n * 2 > self.data.len() {
            bail!("implausible wide string length {n} at {:#x}", self.pos);
        }
        let mut u = Vec::with_capacity(n);
        for _ in 0..n {
            let lo = self.u8()? as u16;
            let hi = self.u8()? as u16;
            u.push(lo | (hi << 8));
        }
        match String::from_utf16(&u) {
            Ok(s) => Ok(s),
            Err(_) => bail!("invalid UTF-16 in wide string at {:#x}", self.pos),
        }
    }

    /// Unencrypted u32 that does not advance the key.
    pub fn raw_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// A raw word that must equal the current key (header end, `*.gst` block heads).
    pub fn check_key(&mut self, what: &str) -> Result<()> {
        let at = self.pos;
        let v = self.raw_u32()?;
        if v != self.key {
            bail!(
                "{what}: check word at {at:#x} is {v:#010x}, key is {:#010x}",
                self.key
            );
        }
        Ok(())
    }

    /// `CheckedReader::BeginBlock` (Game.dll). Returns the end offset.
    pub fn begin_block(&mut self, expected: Option<u32>) -> Result<(u32, usize)> {
        let at = self.pos;
        let id = self.u32()?;
        if let Some(e) = expected
            && id != e
        {
            bail!("expected block {e} at {at:#x}, found {id}");
        }
        let len = self.raw_u32()? ^ self.key;
        let end = self.pos + len as usize;
        if end > self.data.len() {
            bail!("block {id} at {at:#x}: length {len} runs past end of file");
        }
        Ok((id, end))
    }

    /// `CheckedReader::EndBlock`: payload must be fully consumed,
    /// then a raw word equal to the key.
    pub fn end_block(&mut self, id: u32, end: usize) -> Result<()> {
        if self.pos != end {
            bail!(
                "block {id}: parsed to {:#x} but block ends at {end:#x}",
                self.pos
            );
        }
        self.check_key(&format!("end of block {id}"))
    }
}

pub struct Encoder {
    pub buf: Vec<u8>,
    pub key: u32,
    table: [u32; 256],
    stack: Vec<usize>,
}

impl Encoder {
    pub fn new(seed: u32) -> Self {
        let mut buf = Vec::with_capacity(0x2000);
        buf.extend_from_slice(&(seed ^ SEED_XOR).to_le_bytes());
        Self {
            buf,
            key: seed,
            table: make_table(seed),
            stack: Vec::new(),
        }
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        if !self.stack.is_empty() {
            bail!("{} unterminated block(s)", self.stack.len());
        }
        Ok(self.buf)
    }

    pub fn u32(&mut self, v: u32) {
        let c = (v ^ self.key).to_le_bytes();
        for &b in &c {
            self.key ^= self.table[b as usize];
        }
        self.buf.extend_from_slice(&c);
    }

    pub fn i32(&mut self, v: i32) {
        self.u32(v as u32)
    }

    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits())
    }

    pub fn u16(&mut self, v: u16) {
        let c = (v ^ self.key as u16).to_le_bytes();
        for &b in &c {
            self.key ^= self.table[b as usize];
        }
        self.buf.extend_from_slice(&c);
    }

    pub fn u8(&mut self, v: u8) {
        let c = v ^ self.key as u8;
        self.key ^= self.table[c as usize];
        self.buf.push(c);
    }

    pub fn uid(&mut self, v: &[u8; 16]) {
        for &b in v {
            self.u8(b);
        }
    }

    pub fn string(&mut self, s: &str) -> Result<()> {
        let b = string_to_bytes(s)?;
        self.u32(b.len() as u32);
        for c in b {
            self.u8(c);
        }
        Ok(())
    }

    pub fn wstring(&mut self, s: &str) {
        let u: Vec<u16> = s.encode_utf16().collect();
        self.u32(u.len() as u32);
        for c in u {
            self.u8(c as u8);
            self.u8((c >> 8) as u8);
        }
    }

    pub fn raw_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn key_word(&mut self) {
        let k = self.key;
        self.raw_u32(k);
    }

    pub fn begin_block(&mut self, id: u32) {
        self.u32(id);
        self.stack.push(self.buf.len());
        self.key_word();
    }

    /// `CheckedWriter::EndBlock`: patch `len` into the placeholder
    /// (which holds the key), then append the key.
    pub fn end_block(&mut self) -> Result<()> {
        let Some(at) = self.stack.pop() else {
            bail!("end_block without begin_block")
        };
        let len = (self.buf.len() - at - 4) as u32;
        let old = u32::from_le_bytes(self.buf[at..at + 4].try_into().unwrap());
        self.buf[at..at + 4].copy_from_slice(&(old ^ len).to_le_bytes());
        self.key_word();
        Ok(())
    }
}
