//! Texture / font / shader / localisation tools.
//!
//! Inputs are either a plain file or `ARCHIVE.arc::entry/name`.
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use gd_formats::{arc::Arc, fnt::Font, ssh, tex::Tex, text::TagTable};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(about = "Grim Dawn .tex/.fnt/.ssh/tags tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse every .tex/.fnt/.ssh/tags*.txt in the given .arc files or directories
    Scan {
        inputs: Vec<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    /// Print a texture's header
    TexInfo { input: String },
    /// Convert a texture (frame/mip) or a bitmap-font atlas to PNG
    Png {
        input: String,
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        frame: usize,
        #[arg(long, default_value_t = 0)]
        mip: u32,
    },
    /// Print a font's styles and the first glyphs
    FntInfo { input: String },
    /// Dump an .ssh profile: params, values, techniques with decoded states, DXBC reflection
    SshDump {
        input: String,
        #[arg(long, default_value = "DirectX11")]
        profile: String,
    },
    /// Write each DirectX11 shader blob of an .ssh to <out>/<value>_<vs|ps>.dxbc
    SshExtract {
        input: String,
        out: PathBuf,
        #[arg(long, default_value = "DirectX11")]
        profile: String,
    },
    /// Look up a tag in tags*.txt files (dir or Text_XX.arc)
    Text { input: PathBuf, tag: Option<String> },
}

fn load(spec: &str) -> Result<(String, Vec<u8>)> {
    if let Some((arc, entry)) = spec.split_once("::") {
        let a = Arc::open(arc)?;
        let e = a
            .find(entry)
            .with_context(|| format!("{entry} not in {arc}"))?;
        Ok((e.name.clone(), a.read(e)?))
    } else {
        Ok((
            spec.to_string(),
            std::fs::read(spec).with_context(|| spec.to_string())?,
        ))
    }
}

/// Visit every (name, bytes) in .arc files / directories.
fn each_file(inputs: &[PathBuf], f: impl FnMut(&str, &[u8])) -> Result<()> {
    let wanted = |n: &str| {
        let l = n.to_ascii_lowercase();
        l.ends_with(".tex")
            || l.ends_with(".fnt")
            || l.ends_with(".ssh")
            || (l.ends_with(".txt") && TagTable::is_tag_file(&l))
    };
    gd_tools::each_input(inputs, wanted, f)
}

fn add(c: &mut BTreeMap<String, usize>, k: impl Into<String>, n: usize) {
    *c.entry(k.into()).or_default() += n;
}

fn scan(inputs: &[PathBuf], verbose: bool) -> Result<()> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors = Vec::new();
    each_file(inputs, |name, data| {
        let l = name.to_ascii_lowercase();
        let res: Result<()> = (|| {
            if l.ends_with(".tex") {
                let t = Tex::parse(data)?;
                add(&mut counts, "tex: files", 1);
                if t.frames.len() > 1 {
                    add(&mut counts, "tex: animated (multi-frame)", 1);
                }
                for d in &t.frames {
                    add(&mut counts, format!("tex frame: {:?}", d.format), 1);
                    add(
                        &mut counts,
                        format!(
                            "tex frame: magic {}",
                            if d.reversed { "DDSR" } else { "DDS " }
                        ),
                        1,
                    );
                    if d.faces == 6 {
                        add(&mut counts, "tex frame: cubemap", 1)
                    }
                    if d.generate_mips {
                        add(&mut counts, "tex frame: mipcount 0 (GenerateMips)", 1)
                    }
                    if d.truncated {
                        add(&mut counts, "tex frame: truncated mip chain", 1)
                    }
                    if d.mip_levels == 1 {
                        add(&mut counts, "tex frame: single mip", 1)
                    }
                    if !d.width.is_power_of_two() || !d.height.is_power_of_two() {
                        add(&mut counts, "tex frame: non-pow2", 1)
                    }
                }
            } else if l.ends_with(".fnt") {
                match Font::parse(data)? {
                    Font::Bitmap(b) => {
                        add(
                            &mut counts,
                            format!("fnt: FNTX atlas {:?}", b.atlas_format),
                            1,
                        );
                        *counts.entry("fnt: FNTX styles (sum)".into()).or_default() +=
                            b.styles.len();
                    }
                    Font::Dynamic(d) => {
                        add(
                            &mut counts,
                            format!(
                                "fnt: FNTD embedded {}",
                                match &d.font_file.get(..4) {
                                    Some(b"OTTO") => "OpenType/CFF",
                                    Some([0, 1, 0, 0]) => "TrueType",
                                    Some(b"ttcf") => "TT collection",
                                    _ => "other",
                                }
                            ),
                            1,
                        );
                    }
                }
            } else if l.ends_with(".ssh") {
                let s = ssh::Ssh::parse(data)?;
                add(&mut counts, "ssh: files", 1);
                for p in &s.profiles {
                    add(&mut counts, format!("ssh profile: {}", p.name), 1);
                }
                if let Some(p) = s.profile("DirectX11") {
                    for v in &p.values {
                        if let ssh::Value::VertexShader(o) | ssh::Value::PixelShader(o) = v {
                            if o.is_dxbc() {
                                ssh::Dxbc::parse(&o.bytecode)?;
                                add(&mut counts, "ssh dx11: shader blobs (DXBC reflected ok)", 1);
                            } else {
                                add(
                                    &mut counts,
                                    "ssh dx11: D3D9 bytecode left in DirectX11 profile",
                                    1,
                                );
                            }
                        }
                    }
                } else {
                    add(&mut counts, "ssh: no DirectX11 profile", 1);
                }
            } else {
                let mut t = TagTable::default();
                t.add_file(data);
                add(&mut counts, "text: tag files", 1);
                *counts.entry("text: tags".into()).or_default() += t.tags.len();
                *counts
                    .entry("text: duplicate keys (ignored)".into())
                    .or_default() += t.duplicates;
                *counts.entry("text: lines without '='".into()).or_default() += t.malformed;
            }
            Ok(())
        })();
        if let Err(e) = res {
            errors.push(format!("{name}: {e:#}"));
        } else if verbose {
            println!("ok {name}");
        }
    })?;
    for (k, v) in &counts {
        println!("{v:>8}  {k}");
    }
    println!("{:>8}  ERRORS", errors.len());
    for e in errors.iter().take(50) {
        println!("  {e}");
    }
    Ok(())
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Scan { inputs, verbose } => scan(&inputs, verbose)?,
        Cmd::TexInfo { input } => {
            let (_, b) = load(&input)?;
            let t = Tex::parse(&b)?;
            println!(
                "version {} fps {} frames {}",
                t.version,
                t.fps,
                t.frames.len()
            );
            for (i, d) in t.frames.iter().enumerate() {
                println!(
                    "frame {i}: {}x{} {:?} ({}) mips {} faces {} reversed {} genmips {} truncated {} bytes {}",
                    d.width,
                    d.height,
                    d.format,
                    d.format.wgpu_name(),
                    d.mip_levels,
                    d.faces,
                    d.reversed,
                    d.generate_mips,
                    d.truncated,
                    d.data.len()
                );
            }
        }
        Cmd::Png {
            input,
            out,
            frame,
            mip,
        } => {
            let (name, b) = load(&input)?;
            if name.to_ascii_lowercase().ends_with(".fnt") {
                let Font::Bitmap(f) = Font::parse(&b)? else {
                    bail!("dynamic (TTF) font has no atlas")
                };
                write_png(&out, f.atlas_width, f.atlas_height, &f.atlas_rgba8()?)?;
            } else {
                let t = Tex::parse(&b)?;
                let d = t.frames.get(frame).context("no such frame")?;
                // Decode first: it rejects a mip the frame lacks before `level_dims` shifts by it.
                let rgba = d.decode_rgba8(mip)?;
                let (w, h) = d.level_dims(mip);
                write_png(&out, w, h, &rgba)?;
            }
            println!("wrote {}", out.display());
        }
        Cmd::FntInfo { input } => {
            let (_, b) = load(&input)?;
            match Font::parse(&b)? {
                Font::Dynamic(d) => println!(
                    "FNTD header {:?}, embedded font {} bytes",
                    d.header,
                    d.font_file.len()
                ),
                Font::Bitmap(f) => {
                    println!(
                        "FNTX atlas {}x{} {:?} ({} bytes), {} styles",
                        f.atlas_width,
                        f.atlas_height,
                        f.atlas_format,
                        f.atlas.len(),
                        f.styles.len()
                    );
                    for s in &f.styles {
                        println!(
                            "  size {} flags {} ascent {} line {} extra {} glyphs {} kerning {}",
                            s.size,
                            s.flags,
                            s.ascent,
                            s.line_height,
                            s.extra,
                            s.glyphs.len(),
                            s.kerning.len()
                        );
                    }
                    if let Some(s) = f.styles.first() {
                        for g in s.glyphs.iter().take(8) {
                            println!(
                                "    {:?} {:?} adv {}",
                                char::from_u32(g.code as u32),
                                g,
                                g.advance()
                            );
                        }
                    }
                }
            }
        }
        Cmd::SshDump { input, profile } => {
            let (_, b) = load(&input)?;
            let s = ssh::Ssh::parse(&b)?;
            println!(
                "profiles: {:?}",
                s.profiles
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
            );
            let p = s.profile(&profile).context("no such profile")?;
            println!("== params");
            for (i, pa) in p.params.iter().enumerate() {
                println!(
                    "  [{i}] {} {:?}{}{}",
                    pa.name,
                    pa.ty,
                    if pa.material { " (material)" } else { "" },
                    pa.default
                        .map(|d| format!(" default=v{d}"))
                        .unwrap_or_default()
                );
            }
            println!("== values");
            for (i, v) in p.values.iter().enumerate() {
                match v {
                    ssh::Value::VertexShader(o) | ssh::Value::PixelShader(o) => {
                        let kind = if matches!(v, ssh::Value::VertexShader(_)) {
                            "VS"
                        } else {
                            "PS"
                        };
                        let r = ssh::Dxbc::parse(&o.bytecode).ok();
                        println!(
                            "  v{i} {kind} {} bytes {}",
                            o.bytecode.len(),
                            r.as_ref()
                                .map(|r| r.target.as_str())
                                .unwrap_or("(not DXBC)")
                        );
                        for bd in &o.bindings {
                            let n = p
                                .params
                                .get(bd.param as usize)
                                .map(|p| p.name.as_str())
                                .unwrap_or("?");
                            println!(
                                "      const {n}: cb{} +{} ({} bytes)",
                                bd.slot, bd.offset, bd.size
                            );
                        }
                        if let Some(r) = r {
                            for res in &r.resources {
                                println!(
                                    "      bind {} kind {} slot {}",
                                    res.name, res.kind, res.bind_point
                                );
                            }
                            for i in &r.inputs {
                                println!(
                                    "      in  {}{} -> v{} mask {:#x}",
                                    i.semantic, i.index, i.register, i.mask
                                );
                            }
                        }
                    }
                    v => println!("  v{i} {v:?}"),
                }
            }
            println!("== techniques");
            for t in &p.techniques {
                println!("  {} (sort {})", t.name, t.sort_order);
                for (pi, pass) in t.passes.iter().enumerate() {
                    println!("    pass {pi}");
                    for st in &pass.states {
                        println!("      {}", p.describe(st));
                    }
                }
            }
        }
        Cmd::SshExtract {
            input,
            out,
            profile,
        } => {
            let (_, b) = load(&input)?;
            let s = ssh::Ssh::parse(&b)?;
            let p = s.profile(&profile).context("no such profile")?;
            std::fs::create_dir_all(&out)?;
            for (i, v) in p.values.iter().enumerate() {
                match v {
                    ssh::Value::VertexShader(o) => {
                        std::fs::write(out.join(format!("{i}_vs.dxbc")), &o.bytecode)?
                    }
                    ssh::Value::PixelShader(o) => {
                        std::fs::write(out.join(format!("{i}_ps.dxbc")), &o.bytecode)?
                    }
                    _ => {}
                }
            }
        }
        Cmd::Text { input, tag } => {
            let mut t = TagTable::default();
            each_file(&[input], |name, data| {
                if name.to_ascii_lowercase().ends_with(".txt") {
                    t.add_file(data)
                }
            })?;
            println!(
                "{} tags, {} duplicates, {} malformed",
                t.tags.len(),
                t.duplicates,
                t.malformed
            );
            if let Some(tag) = tag {
                let v = t.get(&tag).unwrap_or("<missing>");
                println!("{tag} = {v}");
                println!("stripped: {}", gd_formats::text::strip_markup(v));
            }
        }
    }
    Ok(())
}
