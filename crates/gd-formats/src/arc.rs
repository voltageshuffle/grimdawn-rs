//! `.arc` resource archives (version 3). Files are split into LZ4-block parts
//! of up to 256 KiB ([`MAX_PART`]); a part whose compressed size equals its decompressed size
//! is stored raw. A part that claims more than [`MAX_PART`] is refused (every stock part fits;
//! the 255× LZ4 bound alone would let a hostile mod's 1 MiB part allocate 255 MiB).
//!
//! [`Arc::read`] decodes a whole entry; [`Arc::open_entry`] gives a seekable reader ([`EntryReader`]) that
//! decodes only the parts a read touches, as the engine's `FileSystem` serves a map to `World::Load`
//! (maps are read region by region as the world streams in, never whole).
use crate::util::{Reader, latin1, lz4_max_size};
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub offset: u32,
    pub compressed: u32,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    /// 1 = stored, 3 = LZ4 parts.
    pub kind: u32,
    pub offset: u32,
    pub compressed: u32,
    pub size: u32,
    pub hash: u32,
    pub filetime: u64,
    pub num_parts: u32,
    pub first_part: u32,
}

/// The most decoded bytes of one part (the format's chunk size; the largest stock part).
pub const MAX_PART: u32 = 256 << 10;

pub struct Arc {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub parts: Vec<Part>,
    /// File length: every offset of the tables is checked against it before a read.
    len: u64,
    /// Shared by concurrent [`Arc::read`]s through positional reads (no cursor, no lock).
    file: File,
}

impl Arc {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let len = file.metadata()?.len();
        let mut hdr = [0u8; 28];
        file.read_exact(&mut hdr)?;
        let mut r = Reader::new(&hdr);
        if r.bytes(4)? != b"ARC\0" {
            bail!("{}: not an ARC file", path.display());
        }
        let version = r.u32()?;
        if version != 3 {
            bail!("unsupported ARC version {version}");
        }
        let num_files = r.u32()? as usize;
        let num_parts = r.u32()? as usize;
        let parts_size = r.u32()? as usize;
        let strings_size = r.u32()? as usize;
        let toc_offset = r.u32()? as u64;
        let toc_len = parts_size as u64 + strings_size as u64 + num_files as u64 * 44;
        if toc_offset + toc_len > len {
            bail!(
                "{}: tables ({toc_len} bytes at {toc_offset:#x}) past end of file",
                path.display()
            );
        }
        let mut toc = vec![0u8; toc_len as usize];
        file.seek(SeekFrom::Start(toc_offset))?;
        file.read_exact(&mut toc)?;
        let mut r = Reader::new(&toc[..parts_size]);
        let mut parts = Vec::with_capacity(r.capacity(num_parts, 12));
        for _ in 0..num_parts {
            parts.push(Part {
                offset: r.u32()?,
                compressed: r.u32()?,
                size: r.u32()?,
            });
        }
        let strings = &toc[parts_size..parts_size + strings_size];
        let mut r = Reader::at(&toc, parts_size + strings_size);
        let mut entries = Vec::with_capacity(r.capacity(num_files, 44));
        for _ in 0..num_files {
            let kind = r.u32()?;
            let offset = r.u32()?;
            let compressed = r.u32()?;
            let size = r.u32()?;
            let hash = r.u32()?;
            let filetime = r.u64()?;
            let num_parts = r.u32()?;
            let first_part = r.u32()?;
            let name_len = r.u32()? as usize;
            let name_off = r.u32()? as usize;
            let Some(name) = strings
                .get(name_off..)
                .and_then(|s| s.get(..name_len))
                .map(latin1)
            else {
                bail!(
                    "{}: entry name ({name_len} bytes at {name_off:#x}) outside the string table",
                    path.display()
                );
            };
            entries.push(Entry {
                name,
                kind,
                offset,
                compressed,
                size,
                hash,
                filetime,
                num_parts,
                first_part,
            });
        }
        Ok(Self {
            path,
            entries,
            parts,
            len,
            file,
        })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// `e`'s parts, checked against the archive (`None`: stored whole at `e.offset`).
    fn parts_of(&self, e: &Entry) -> Result<Option<&[Part]>> {
        let past_end = |offset: u32, len: u32| offset as u64 + len as u64 > self.len;
        if e.kind == 1 || e.num_parts == 0 {
            if past_end(e.offset, e.size) {
                bail!("{}: stored data past end of archive", e.name);
            }
            return Ok(None);
        }
        let first = e.first_part as usize;
        let Some(parts) = first
            .checked_add(e.num_parts as usize)
            .and_then(|end| self.parts.get(first..end))
        else {
            bail!(
                "{}: parts {first}+{} outside the part table ({})",
                e.name,
                e.num_parts,
                self.parts.len()
            );
        };
        let mut total = 0u64;
        for p in parts {
            if past_end(p.offset, p.compressed)
                || p.size > MAX_PART
                || p.size as usize > lz4_max_size(p.compressed as usize)
            {
                bail!("{}: corrupt part at {:#x}", e.name, p.offset);
            }
            total += p.size as u64;
        }
        // The parts must decode to exactly the header size; a table that re-references one
        // region would otherwise amplify without bound.
        if total != e.size as u64 {
            bail!(
                "{}: parts decode to {total}, header says {}",
                e.name,
                e.size
            );
        }
        Ok(Some(parts))
    }

    pub fn read(&self, e: &Entry) -> Result<Vec<u8>> {
        let parts = self.parts_of(e)?;
        let mut out = vec![0; e.size as usize];
        let Some(parts) = parts else {
            read_exact_at(&self.file, &mut out, e.offset as u64)?;
            return Ok(out);
        };
        let (mut buf, mut at) = (Vec::new(), 0);
        for p in parts {
            let end = at + p.size as usize;
            decode_part(&self.file, p, &mut buf, &mut out[at..end])
                .with_context(|| format!("part of {}", e.name))?;
            at = end;
        }
        Ok(out)
    }

    /// A seekable reader of `e` on a file handle of its own (no contention with [`Arc::read`]).
    pub fn open_entry(&self, e: &Entry) -> Result<EntryReader> {
        let parts = self.parts_of(e)?;
        let file =
            File::open(&self.path).with_context(|| format!("open {}", self.path.display()))?;
        let layout = match parts {
            None => Layout::Stored {
                offset: e.offset as u64,
            },
            Some(parts) => {
                let mut starts = Vec::with_capacity(parts.len());
                let mut at = 0u64;
                for p in parts {
                    starts.push(at);
                    at += p.size as u64;
                }
                Layout::Parts {
                    parts: parts.into(),
                    starts: starts.into(),
                    recent: Vec::with_capacity(RECENT_PARTS),
                    compressed: Vec::new(),
                }
            }
        };
        Ok(EntryReader {
            file,
            size: e.size as u64,
            pos: 0,
            layout,
        })
    }
}

/// One positional read (no cursor): the primitive behind [`read_exact_at`] and [`EntryReader`].
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::read_at(file, buf, offset);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_read(file, buf, offset);
}

/// Reads exactly `buf.len()` bytes at `offset` without touching the file cursor, so concurrent
/// readers share one handle without a lock.
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = read_at(file, buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

/// Decodes part `p` of `file` into `out` (`p.size` bytes), reading its compressed bytes through `buf`.
fn decode_part(file: &File, p: &Part, buf: &mut Vec<u8>, out: &mut [u8]) -> io::Result<()> {
    if p.compressed == p.size {
        return read_exact_at(file, out, p.offset as u64);
    }
    buf.resize(p.compressed as usize, 0);
    read_exact_at(file, buf, p.offset as u64)?;
    let n = lz4_flex::block::decompress_into(buf, out).map_err(io::Error::other)?;
    if n != p.size as usize {
        return Err(io::Error::other(format!(
            "part at {:#x} decompressed to {n}, expected {}",
            p.offset, p.size
        )));
    }
    Ok(())
}

/// The decoded parts an [`EntryReader`] keeps: a read that ends inside a part and the next one that
/// continues it decode that part once.
const RECENT_PARTS: usize = 2;

enum Layout {
    Stored {
        offset: u64,
    },
    Parts {
        parts: Box<[Part]>,
        /// Each part's offset in the decoded entry.
        starts: Box<[u64]>,
        /// The last decoded LZ4 parts, most recent last: (part index, bytes).
        recent: Vec<(usize, Vec<u8>)>,
        compressed: Vec<u8>,
    },
}

/// One archive entry as a `Read + Seek` stream ([`Arc::open_entry`]). A read decodes only the parts it
/// covers: a whole LZ4 part straight into the caller's buffer, a raw part read in place, and a part read in
/// pieces through a small cache of the last [`RECENT_PARTS`] decoded parts.
pub struct EntryReader {
    file: File,
    size: u64,
    pos: u64,
    layout: Layout,
}

impl Read for EntryReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.size.saturating_sub(self.pos);
        let n = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        if n == 0 {
            return Ok(0);
        }
        let n = match &mut self.layout {
            Layout::Stored { offset } => read_at(&self.file, &mut buf[..n], *offset + self.pos)?,
            Layout::Parts {
                parts,
                starts,
                recent,
                compressed,
            } => {
                // The last part starting at or before `pos` (zero-size parts share the next one's start).
                let i = starts.partition_point(|&s| s <= self.pos) - 1;
                let p = &parts[i];
                let within = (self.pos - starts[i]) as usize;
                let n = n.min(p.size as usize - within);
                let out = &mut buf[..n];
                if p.compressed == p.size {
                    read_exact_at(&self.file, out, p.offset as u64 + within as u64)?;
                } else if let Some(k) = recent.iter().position(|(j, _)| *j == i) {
                    out.copy_from_slice(&recent[k].1[within..within + n]);
                } else if n == p.size as usize {
                    decode_part(&self.file, p, compressed, out)?;
                } else {
                    let mut bytes = if recent.len() == RECENT_PARTS {
                        recent.remove(0).1
                    } else {
                        Vec::new()
                    };
                    bytes.resize(p.size as usize, 0);
                    decode_part(&self.file, p, compressed, &mut bytes)?;
                    out.copy_from_slice(&bytes[within..within + n]);
                    recent.push((i, bytes));
                }
                n
            }
        };
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for EntryReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.size.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = pos
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        Ok(self.pos)
    }
}
