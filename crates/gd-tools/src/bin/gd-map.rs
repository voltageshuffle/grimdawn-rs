//! World map tools: validate `.map` files, dump levels, render top-down PNGs.
//! A map path may be `<archive.arc>:<entry>` to read a map inside an arc without extracting it.
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use gd_formats::arc::Arc;
use gd_formats::collections::HashMap;
use gd_formats::map::{Level, MapFile};
use gd_formats::vfs::Database;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse the header and every level of each map; print stats
    Check {
        maps: Vec<PathBuf>,
        /// Game root for record class lookup (database layers)
        #[arg(long)]
        game: Option<PathBuf>,
    },
    /// Header summary: regions, spawn points, sector names
    Info {
        map: PathBuf,
        #[arg(long, default_value_t = 20)]
        regions: usize,
    },
    /// Instance groups whose name or kind contains a substring (members, linked groups)
    Groups {
        map: PathBuf,
        filter: String,
        /// Print the members' uids and the regions of the level entities they name
        #[arg(long)]
        members: bool,
    },
    /// Connected clusters of the regions whose level name contains a substring (`map::links`)
    Clusters { map: PathBuf, filter: String },
    /// Summary of one level (by index or name substring)
    Level {
        map: PathBuf,
        level: String,
        #[arg(long, default_value_t = 0)]
        entities: usize,
    },
    /// Top-down render of the levels around one level: shaded heightfield,
    /// water, entity dots, level outlines, spawn point
    Area {
        map: PathBuf,
        level: String,
        out: PathBuf,
        /// Extra world units around the chosen level
        #[arg(long, default_value_t = 256)]
        margin: i32,
        /// Pixels per world unit
        #[arg(long, default_value_t = 2.0)]
        scale: f32,
    },
    /// Find level entities by uid (16 bytes as in a save dump, comma-separated)
    Uid { map: PathBuf, uid: Vec<String> },
    /// Find level entities whose record path contains a substring (world positions)
    Find { map: PathBuf, record: String },
    /// Overview of the whole world: every level's heightfield + outlines
    World {
        map: PathBuf,
        out: PathBuf,
        /// Output width in pixels (height follows aspect)
        #[arg(long, default_value_t = 4096)]
        width: u32,
    },
    /// Write region i's minimap TGA to a PNG
    Minimap {
        map: PathBuf,
        level: String,
        out: PathBuf,
    },
    /// Lighting sector data: environment (fog layer) modifiers, day/night
    /// cycles, bloom and adjustment entries
    Env { map: PathBuf },
    /// Sector-layer ids at a world point (x, z) and each layer's painted share
    Sector { map: PathBuf, x: f32, z: f32 },
    /// Where challenge areas (sector layer 13) are painted: per level and entry whose name or record contains
    /// `filter`, the cell count and the world centre of its cells
    Challenge { map: PathBuf, filter: String },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Check { maps, game } => {
            let db = game.map(Database::open).transpose()?;
            for m in maps {
                check(&m, db.as_ref())?;
            }
            Ok(())
        }
        Cmd::Info { map, regions } => info(&map, regions),
        Cmd::Clusters { map, filter } => {
            let m = open_map(&map)?;
            let n = gd_formats::map::links::neighbours(&m.regions);
            let f = filter.to_lowercase();
            let mut seen = vec![false; m.regions.len()];
            let mut clusters = 0;
            for r in 0..m.regions.len() {
                if seen[r] || !m.regions[r].name.to_lowercase().contains(&f) {
                    continue;
                }
                let set = gd_formats::map::links::connected_set(&n, r, 50);
                for &s in &set {
                    seen[s] = true;
                }
                clusters += 1;
                let names: Vec<&str> = set
                    .iter()
                    .map(|i| m.regions[*i].name.rsplit('/').next().unwrap_or(""))
                    .collect();
                println!(
                    "cluster {clusters}: {} regions: {}",
                    set.len(),
                    names.join(" ")
                );
            }
            Ok(())
        }
        Cmd::Groups {
            map,
            filter,
            members,
        } => {
            let m = open_map(&map)?;
            let f = filter.to_lowercase();
            for g in m.instance_groups.iter() {
                if !g.name.to_lowercase().contains(&f) && !g.kind.to_lowercase().contains(&f) {
                    continue;
                }
                println!(
                    "{:?} kind {:?}: {} entries, {} links",
                    g.name,
                    g.kind,
                    g.entries.len(),
                    g.links.len()
                );
                if members {
                    for e in &g.entries {
                        println!("    {:08x?} {:?} {:?}", e.uid, e.name, e.ints);
                    }
                }
            }
            Ok(())
        }
        Cmd::Level {
            map,
            level,
            entities,
        } => level_cmd(&map, &level, entities),
        Cmd::Area {
            map,
            level,
            out,
            margin,
            scale,
        } => area(&map, &level, &out, margin, scale),
        Cmd::World { map, out, width } => world(&map, &out, width),
        Cmd::Uid { map, uid } => find_uid(&map, &uid),
        Cmd::Find { map, record } => find_record(&map, &record),
        Cmd::Minimap { map, level, out } => minimap(&map, &level, &out),
        Cmd::Env { map } => env(&map),
        Cmd::Sector { map, x, z } => sector_at(&map, x, z),
        Cmd::Challenge { map, filter } => challenge_cells(&map, &filter),
    }
}

/// A loose `.map` path, or `<archive.arc>:<entry>` (entry separators `/` or `\`).
fn open_map(path: &Path) -> Result<MapFile> {
    let s = path.to_string_lossy();
    if let Some((arc, entry)) = s.split_once(':')
        && Path::new(arc)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("arc"))
    {
        let a = Arc::open(arc).with_context(|| format!("open {arc}"))?;
        let e = a
            .find(entry)
            .or_else(|| a.find(&entry.replace('/', "\\")))
            .with_context(|| format!("{entry} not found in {arc}"))?;
        return MapFile::from_source(s.into_owned(), a.open_entry(e)?);
    }
    MapFile::open(path)
}

fn pick(m: &MapFile, s: &str) -> Result<usize> {
    if let Ok(i) = s.parse::<usize>()
        && i < m.regions.len()
    {
        return Ok(i);
    }
    m.find_region(s)
        .map(|r| r.index)
        .with_context(|| format!("no level matching {s}"))
}

/// World-space rectangle of a level: [x0, z0, x1, z1] (terrain extent if known,
/// else header bbox).
fn level_rect(m: &MapFile, i: usize, lvl: Option<&Level>) -> [f32; 4] {
    let r = &m.regions[i];
    let (px, pz) = (r.position[0] as f32, r.position[2] as f32);
    if let Some(t) = lvl.and_then(|l| l.terrain.as_ref()) {
        return [
            px,
            pz,
            px + (t.width - 1) as f32,
            pz + (t.height - 1) as f32,
        ];
    }
    let (mn, mx) = (r.bbox.min(), r.bbox.max());
    [px + mn[0], pz + mn[2], px + mx[0], pz + mx[2]]
}

fn check(path: &Path, db: Option<&Database>) -> Result<()> {
    let t0 = std::time::Instant::now();
    let m = open_map(path)?;
    println!(
        "{}: MAP v{} {} bytes, {} regions, {} quests, {} spawn points, {} instance groups, {} sector-data layers ({} name sectors), other header chunks {:?}, minimap blob {:?}",
        path.display(),
        m.version,
        m.file_len,
        m.regions.len(),
        m.quests.len(),
        m.spawn_points.len(),
        m.instance_groups.len(),
        m.sector_data.layer_count,
        m.sector_data.names().count(),
        m.other_chunks
            .iter()
            .map(|c| (c.id, c.len))
            .collect::<Vec<_>>(),
        m.minimap_blob
    );
    let mut errors = 0usize;
    let mut n_ent = 0usize;
    let mut n_uid = 0usize;
    let mut n_extra = 0usize;
    let mut records: HashMap<String, usize> = HashMap::default();
    let mut terrain_sizes: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut layer_counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<u32, usize> = BTreeMap::new();
    let (mut water, mut water_layers, mut water_paths, mut nav, mut nav_layers, mut sectors) =
        (0, 0, 0, 0, 0, 0);
    let mut rects = vec![];
    let mut ground_err = vec![];
    for i in 0..m.regions.len() {
        let lvl = match m.level(i) {
            Ok(l) => l,
            Err(e) => {
                errors += 1;
                eprintln!("ERROR: {e:#}");
                continue;
            }
        };
        n_ent += lvl.entities.len();
        for e in &lvl.entities {
            *records
                .entry(lvl.record_of(e).to_ascii_lowercase())
                .or_default() += 1;
            n_uid += e.uid.is_some() as usize;
            n_extra += e.extra.is_some() as usize;
        }
        if let Some(t) = &lvl.terrain {
            *terrain_sizes.entry((t.width, t.height)).or_default() += 1;
            *layer_counts.entry(t.layers.len()).or_default() += 1;
            // Diagnostic: entity y vs terrain height under it.
            for e in lvl.entities.iter().take(400) {
                let p = e.position;
                if p[0] >= 0.0
                    && p[2] >= 0.0
                    && p[0] <= (t.width - 1) as f32
                    && p[2] <= (t.height - 1) as f32
                {
                    ground_err.push((p[1] - t.sample(p[0], p[2])).abs());
                }
            }
        }
        if let Some(w) = &lvl.water {
            water += 1;
            water_layers += w.layers.len();
            water_paths += w.paths.len();
        }
        if let Some(p) = &lvl.pathing {
            nav += 1;
            nav_layers += p.layer_count();
        }
        sectors += lvl.sector_layers.is_some() as usize;
        for u in &lvl.unknown {
            *unknown.entry(u.id).or_default() += 1;
        }
        rects.push((i, level_rect(&m, i, Some(&lvl)), m.regions[i].position[1]));
    }
    // Overlap check in xz (only among levels at the same y offset).
    let mut overlaps = 0;
    for a in 0..rects.len() {
        for b in a + 1..rects.len() {
            let (ia, ra, ya) = rects[a];
            let (ib, rb, yb) = rects[b];
            if ya != yb {
                continue;
            }
            if ra[0] < rb[2] && rb[0] < ra[2] && ra[1] < rb[3] && rb[1] < ra[3] {
                overlaps += 1;
                if overlaps <= 5 {
                    println!(
                        "  overlap: {} {:?} / {} {:?}",
                        m.regions[ia].name, ra, m.regions[ib].name, rb
                    );
                }
            }
        }
    }
    ground_err.sort_by(|a, b| a.total_cmp(b));
    let med = ground_err.get(ground_err.len() / 2).copied().unwrap_or(0.0);
    let p25 = ground_err.get(ground_err.len() / 4).copied().unwrap_or(0.0);
    println!(
        "  levels parsed: {} errors: {errors}; entities {n_ent} (uid {n_uid}, extra {n_extra}); distinct records {}",
        m.regions.len(),
        records.len()
    );
    println!("  terrain sizes {terrain_sizes:?}; layer counts {layer_counts:?}");
    println!(
        "  water: {water} levels, {water_layers} layers, {water_paths} paths; pathing: {nav} levels, {nav_layers} DTLR layers; sector layers: {sectors}; unknown lvl chunks {unknown:?}"
    );
    println!(
        "  |entity.y - terrain(x,z)|: p25 {p25:.3} median {med:.3} (n={}); xz overlaps at same y: {overlaps}",
        ground_err.len()
    );
    let mut top: Vec<_> = records.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1));
    println!("  top records:");
    for (r, n) in top.iter().take(15) {
        println!("    {n:7} {r}");
    }
    if let Some(db) = db {
        let mut classes: HashMap<String, usize> = HashMap::default();
        for (r, n) in &records {
            let c = db
                .get(r)
                .and_then(|rec| rec.str("Class").map(|s| s.to_string()))
                .unwrap_or_else(|| "<missing record>".into());
            *classes.entry(c).or_default() += n;
        }
        let mut top: Vec<_> = classes.into_iter().collect();
        top.sort_by_key(|t| std::cmp::Reverse(t.1));
        println!("  top classes:");
        for (c, n) in top.iter().take(20) {
            println!("    {n:7} {c}");
        }
    }
    println!("  time {:.1}s", t0.elapsed().as_secs_f32());
    if errors > 0 {
        bail!("{errors} level errors");
    }
    Ok(())
}

fn info(path: &Path, nreg: usize) -> Result<()> {
    let m = open_map(path)?;
    println!(
        "MAP v{} regions {} quests {}",
        m.version,
        m.regions.len(),
        m.quests.len()
    );
    for s in &m.spawn_points {
        let r = &m.regions[s.region as usize];
        let w = [0, 1, 2].map(|i| r.position[i] as f32 + s.position[i]);
        println!(
            "spawn: region {} {} local {:?} world {:?} ({})",
            s.region, r.name, s.position, w, r.location_record
        );
    }
    for r in m.regions.iter().take(nreg) {
        println!(
            "{:4} {:45} pos {:?} bbox c{:?} h{:?} lvl@{:#x}+{} map@{:#x}+{} {} {} {}",
            r.index,
            r.name,
            r.position,
            r.bbox.center,
            r.bbox.half,
            r.level_offset,
            r.level_size,
            r.minimap_offset,
            r.minimap_size,
            r.location_record,
            r.shrine_record,
            r.extra2
        );
    }
    let e1: Vec<_> = m
        .regions
        .iter()
        .filter(|r| !r.shrine_record.is_empty())
        .map(|r| (&r.name, &r.shrine_record))
        .take(5)
        .collect();
    let e2: Vec<_> = m
        .regions
        .iter()
        .filter(|r| !r.extra2.is_empty())
        .map(|r| (&r.name, &r.extra2))
        .take(5)
        .collect();
    println!("non-empty shrine_record: {e1:?}\nnon-empty extra2: {e2:?}");
    for (n, t) in m.sector_data.names().take(10) {
        println!("sector name: {n:?} -> {t:?}");
    }
    let mut kinds: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for g in m.instance_groups.iter() {
        let k = kinds.entry(g.kind.as_str()).or_default();
        k.0 += 1;
        k.1 += (!g.links.is_empty()) as usize;
    }
    println!("instance group kinds (groups, with links): {kinds:?}");
    if std::env::var_os("GD_IG_NAMES").is_some() {
        for g in m
            .instance_groups
            .iter()
            .filter(|g| g.kind == "Dungeon Entrances")
        {
            let w: Vec<_> = g
                .entries
                .iter()
                .map(|e| {
                    let rid = e.ints.map(|i| i as u32);
                    m.regions.iter().find(|r| r.id == rid).map(|r| {
                        let p = [0, 1, 2].map(|k| r.position[k] as f32 + e.floats[k]);
                        format!(
                            "{} ({:.0},{:.0},{:.0})",
                            r.name.rsplit('/').next().unwrap_or(""),
                            p[0],
                            p[1],
                            p[2]
                        )
                    })
                })
                .collect();
            println!("DE {:?} links={} {w:?}", g.name, g.links.len());
        }
    }
    if let Ok(kind) = std::env::var("GD_IG_KIND") {
        for g in m.instance_groups.iter().filter(|g| g.kind == kind) {
            let w: Vec<_> = g
                .entries
                .iter()
                .map(|e| {
                    let rid = e.ints.map(|i| i as u32);
                    m.regions.iter().find(|r| r.id == rid).map(|r| {
                        let p = [0, 1, 2].map(|k| r.position[k] as f32 + e.floats[k]);
                        format!("{:?} ({:.0},{:.0},{:.0})", e.uid, p[0], p[1], p[2])
                    })
                })
                .collect();
            println!(
                "{kind} {:?} id {:?} links {:?} {w:?}",
                g.name, g.group_id, g.links
            );
        }
    }
    for g in m
        .instance_groups
        .iter()
        .filter(|g| !g.links.is_empty())
        .take(3)
    {
        println!(
            "linked group {:?} kind {:?} id {:?} links {:?}",
            g.name, g.kind, g.group_id, g.links
        );
        for e in g.entries.iter().take(3) {
            println!(
                "  entry {:?} {:?} ints {:?} floats {:?}",
                e.uid, e.name, e.ints, e.floats
            );
        }
    }
    println!(
        "sector-data tail (layers 4..): {}",
        match m.sector_data.tail_layers() {
            Ok(t) => format!("{} day/night entries", t.daynight.len()),
            Err(e) => format!("undecodable: {e:#}"),
        }
    );
    Ok(())
}

fn level_cmd(path: &Path, sel: &str, nent: usize) -> Result<()> {
    let m = open_map(path)?;
    let i = pick(&m, sel)?;
    let r = &m.regions[i];
    let l = m.level(i)?;
    println!(
        "{} pos {:?} LVL v{} bbox {:?}",
        r.name, r.position, l.version, l.bbox
    );
    println!("records {} entities {}", l.records.len(), l.entities.len());
    if let Some(t) = &l.terrain {
        let (mn, mx) = t
            .heights
            .iter()
            .fold((f32::MAX, f32::MIN), |a, &h| (a.0.min(h), a.1.max(h)));
        println!(
            "terrain {}x{} h0 {} rest {:?} heights {mn:.2}..{mx:.2} trailer {} layers:",
            t.width, t.height, t.header0, t.header_rest, t.trailer
        );
        for ly in &t.layers {
            println!(
                "  {} weights {}",
                ly.name,
                ly.weights.as_ref().map_or(0, |w| w.len())
            );
        }
        let mut hist = BTreeMap::new();
        for b in &t.layer_mask {
            *hist.entry(*b).or_insert(0) += 1;
        }
        println!(
            "  layer_mask byte histogram (top): {:?}",
            hist.iter().take(12).collect::<Vec<_>>()
        );
    }
    if let Some(w) = &l.water {
        println!(
            "water v{} types {:?}",
            w.version,
            w.types.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        for ly in &w.layers {
            println!(
                "  layer {} h {} uv {:?} blocks {}",
                ly.water_type,
                ly.height,
                ly.uv,
                ly.blocks.len()
            );
            if let Some(b) = ly.blocks.first() {
                println!("    block0 idx {} {:?}", b.index, b.data);
            }
        }
        for p in &w.paths {
            println!(
                "  path {} {} pts {} first {:?}",
                p.water_type,
                p.value,
                p.points.len(),
                p.points.first()
            );
        }
    }
    if let Some(s) = &l.sector_layers {
        println!(
            "sector layers {}x{} x{} palettes {:?}",
            s.width,
            s.height,
            s.palettes.len(),
            s.palettes.iter().map(|p| p.len()).collect::<Vec<_>>()
        );
    }
    if let Some(p) = &l.pathing {
        println!(
            "pathing tiles {:?}..{:?} groups {} layers {}",
            p.tile_min,
            p.tile_max,
            p.tiles.len(),
            p.layer_count()
        );
        if let Some(t) = p.tiles.first().and_then(|t| t.first()) {
            println!("  first layer {:?}", t.header);
        }
    }
    for u in &l.unknown {
        println!("unknown chunk {} {} bytes", u.id, u.len);
    }
    for e in l.entities.iter().take(nent) {
        println!(
            "  {} pos {:?} rot {:?} scale {:?} uid {:?} extra {}",
            l.record_of(e),
            e.position,
            e.rotation,
            e.scale(),
            e.uid,
            e.extra.as_ref().map_or(0, |x| x.len())
        );
    }
    Ok(())
}

struct Canvas {
    w: u32,
    h: u32,
    px: Vec<u8>,
    /// world -> pixel: px = (x - x0) * s
    x0: f32,
    z0: f32,
    s: f32,
}

impl Canvas {
    fn new(x0: f32, z0: f32, x1: f32, z1: f32, s: f32) -> Self {
        let w = ((x1 - x0) * s).ceil().max(1.0) as u32;
        let h = ((z1 - z0) * s).ceil().max(1.0) as u32;
        Canvas {
            w,
            h,
            px: vec![16; (w * h * 3) as usize],
            x0,
            z0,
            s,
        }
    }
    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let i = ((y as u32 * self.w + x as u32) * 3) as usize;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }
    fn to_px(&self, x: f32, z: f32) -> (i64, i64) {
        (
            ((x - self.x0) * self.s) as i64,
            ((z - self.z0) * self.s) as i64,
        )
    }
    fn dot(&mut self, x: f32, z: f32, r: i64, c: [u8; 3]) {
        let (cx, cy) = self.to_px(x, z);
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.put(cx + dx, cy + dy, c);
                }
            }
        }
    }
    fn rect(&mut self, r: [f32; 4], c: [u8; 3]) {
        let (ax, ay) = self.to_px(r[0], r[1]);
        let (bx, by) = self.to_px(r[2], r[3]);
        for x in ax..=bx {
            self.put(x, ay, c);
            self.put(x, by, c);
        }
        for y in ay..=by {
            self.put(ax, y, c);
            self.put(bx, y, c);
        }
    }
    /// Shaded heightfield of a level whose origin is at world (ox, oy, oz).
    fn terrain(&mut self, l: &Level, ox: f32, oy: f32, oz: f32, hmin: f32, hmax: f32) {
        let Some(t) = &l.terrain else { return };
        let (ax, ay) = self.to_px(ox, oz);
        let (bx, by) = self.to_px(ox + (t.width - 1) as f32, oz + (t.height - 1) as f32);
        for py in ay.max(0)..by.min(self.h as i64) {
            for px in ax.max(0)..bx.min(self.w as i64) {
                let x = self.x0 + (px as f32 + 0.5) / self.s - ox;
                let z = self.z0 + (py as f32 + 0.5) / self.s - oz;
                let h = t.sample(x, z);
                // Hillshade from the north-west.
                let dx = t.sample(x + 1.0, z) - t.sample(x - 1.0, z);
                let dz = t.sample(x, z + 1.0) - t.sample(x, z - 1.0);
                let shade = (0.75 - 0.35 * (dx + dz)).clamp(0.15, 1.3);
                let k = ((h + oy - hmin) / (hmax - hmin).max(1.0)).clamp(0.0, 1.0);
                let base = [60.0 + 150.0 * k, 90.0 + 110.0 * k, 50.0 + 90.0 * k];
                let c = base.map(|v| (v * shade).clamp(0.0, 255.0) as u8);
                self.put(px, py, c);
            }
        }
    }
    fn save(&self, path: &Path) -> Result<()> {
        let f = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(f, self.w, self.h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&self.px)?;
        println!("wrote {} ({}x{})", path.display(), self.w, self.h);
        Ok(())
    }
}

fn entity_color(rec: &str) -> Option<[u8; 3]> {
    let r = rec.to_ascii_lowercase();
    Some(if r.contains("proxies") || r.contains("/spawn") {
        [255, 40, 40] // monster spawners
    } else if r.contains("riftgate") || r.contains("teleport") || r.contains("portal") {
        [255, 0, 255]
    } else if r.contains("creatures/npcs") || r.contains("/npc") {
        [255, 255, 0]
    } else if r.contains("interactive")
        || r.contains("chest")
        || r.contains("shrine")
        || r.contains("lore")
    {
        [0, 220, 255]
    } else if r.contains("/fx/") || r.contains("sound") || r.contains("ambien") {
        return None;
    } else {
        [235, 235, 235]
    })
}

fn area(path: &Path, sel: &str, out: &Path, margin: i32, scale: f32) -> Result<()> {
    let m = open_map(path)?;
    let i = pick(&m, sel)?;
    let center = m.level(i)?;
    let cr = level_rect(&m, i, Some(&center));
    let y0 = m.regions[i].position[1];
    let win = [
        cr[0] - margin as f32,
        cr[1] - margin as f32,
        cr[2] + margin as f32,
        cr[3] + margin as f32,
    ];
    // Levels whose coarse rect intersects the window, same vertical layer.
    let mut sel_levels = vec![];
    for j in 0..m.regions.len() {
        if m.regions[j].position[1] != y0 {
            continue;
        }
        let r = level_rect(&m, j, None);
        if r[0] < win[2] && win[0] < r[2] && r[1] < win[3] && win[1] < r[3] {
            sel_levels.push((j, m.level(j)?));
        }
    }
    let (mut hmin, mut hmax) = (f32::MAX, f32::MIN);
    for (j, l) in &sel_levels {
        if let Some(t) = &l.terrain {
            let oy = m.regions[*j].position[1] as f32;
            for h in &t.heights {
                hmin = hmin.min(h + oy);
                hmax = hmax.max(h + oy);
            }
        }
    }
    let mut c = Canvas::new(win[0], win[1], win[2], win[3], scale);
    for (j, l) in &sel_levels {
        let p = m.regions[*j].position.map(|v| v as f32);
        c.terrain(l, p[0], p[1], p[2], hmin, hmax);
    }
    for (j, l) in &sel_levels {
        let p = m.regions[*j].position.map(|v| v as f32);
        if let Some(w) = &l.water {
            for ly in &w.layers {
                // Water blocks: draw as blue tint where the terrain is under the surface.
                if let Some(t) = &l.terrain {
                    // Blocks form a 16x16 grid over the level (inferred from
                    // max stock index 255 and uv extent 128).
                    let bs = ((t.width - 1) / 16).max(1);
                    let mut on = vec![false; 256];
                    for b in &ly.blocks {
                        if (b.index as usize) < on.len() {
                            on[b.index as usize] = true;
                        }
                    }
                    for z in 0..t.height - 1 {
                        for x in 0..t.width - 1 {
                            let bi = ((z / bs) * 16 + x / bs) as usize;
                            if on.get(bi).copied().unwrap_or(false)
                                && t.sample(x as f32 + 0.5, z as f32 + 0.5) < ly.height
                            {
                                let (px, py) =
                                    c.to_px(p[0] + x as f32 + 0.5, p[2] + z as f32 + 0.5);
                                for dy in 0..scale.ceil() as i64 {
                                    for dx in 0..scale.ceil() as i64 {
                                        c.put(px + dx, py + dy, [40, 80, 200]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for e in &l.entities {
            if let Some(col) = entity_color(l.record_of(e)) {
                let r = if col == [235, 235, 235] { 0 } else { 2 };
                c.dot(p[0] + e.position[0], p[2] + e.position[2], r, col);
            }
        }
        c.rect(
            level_rect(&m, *j, Some(l)),
            if *j == i { [255, 128, 0] } else { [0, 0, 0] },
        );
    }
    for s in &m.spawn_points {
        let r = &m.regions[s.region as usize];
        c.dot(
            r.position[0] as f32 + s.position[0],
            r.position[2] as f32 + s.position[2],
            5,
            [0, 255, 0],
        );
    }
    println!(
        "{} levels drawn around {} ({:?})",
        sel_levels.len(),
        m.regions[i].name,
        cr
    );
    c.save(out)
}

fn world(path: &Path, out: &Path, width: u32) -> Result<()> {
    let m = open_map(path)?;
    let rects: Vec<[f32; 4]> = (0..m.regions.len())
        .map(|i| level_rect(&m, i, None))
        .collect();
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for r in &rects {
        b = [
            b[0].min(r[0]),
            b[1].min(r[1]),
            b[2].max(r[2]),
            b[3].max(r[3]),
        ];
    }
    let s = width as f32 / (b[2] - b[0]);
    let mut c = Canvas::new(b[0], b[1], b[2], b[3], s);
    let levels: Vec<Level> = (0..m.regions.len())
        .map(|i| m.level(i))
        .collect::<Result<_>>()?;
    let (mut hmin, mut hmax) = (f32::MAX, f32::MIN);
    for (i, l) in levels.iter().enumerate() {
        if let Some(t) = &l.terrain {
            let oy = m.regions[i].position[1] as f32;
            for h in t.heights.iter().step_by(17) {
                hmin = hmin.min(h + oy);
                hmax = hmax.max(h + oy);
            }
        }
    }
    for (i, l) in levels.iter().enumerate() {
        let p = m.regions[i].position.map(|v| v as f32);
        c.terrain(l, p[0], p[1], p[2], hmin, hmax);
    }
    for (i, l) in levels.iter().enumerate() {
        c.rect(level_rect(&m, i, Some(l)), [0, 0, 0]);
    }
    for sp in &m.spawn_points {
        let r = &m.regions[sp.region as usize];
        c.dot(
            r.position[0] as f32 + sp.position[0],
            r.position[2] as f32 + sp.position[2],
            6,
            [0, 255, 0],
        );
    }
    println!(
        "world bounds x {}..{} z {}..{}, heights {hmin}..{hmax}",
        b[0], b[2], b[1], b[3]
    );
    c.save(out)
}

fn minimap(path: &Path, sel: &str, out: &Path) -> Result<()> {
    let m = open_map(path)?;
    let i = pick(&m, sel)?;
    // Rows in file order, as the engine uploads them.
    let img = m.minimap_image(i)?.context("no minimap")?;
    let mut c = Canvas::new(0.0, 0.0, img.w as f32, img.h as f32, 1.0);
    for y in 0..img.h {
        for x in 0..img.w {
            let p = ((y * img.w + x) * 4) as usize;
            c.put(
                x as i64,
                y as i64,
                [img.rgba[p], img.rgba[p + 1], img.rgba[p + 2]],
            );
        }
    }
    c.save(out)
}

fn find_record(path: &Path, needle: &str) -> Result<()> {
    let m = open_map(path)?;
    let needle = needle.to_ascii_lowercase();
    for (i, r) in m.regions.iter().enumerate() {
        let Ok(l) = m.level(i) else { continue };
        for e in &l.entities {
            let rec = l.record_of(e);
            if rec.to_ascii_lowercase().contains(&needle) {
                let w = [
                    r.position[0] as f32 + e.position[0],
                    r.position[1] as f32 + e.position[1],
                    r.position[2] as f32 + e.position[2],
                ];
                println!("{} {rec} local {:?} world {:?}", r.name, e.position, w);
            }
        }
    }
    Ok(())
}

fn find_uid(path: &Path, uids: &[String]) -> Result<()> {
    let m = open_map(path)?;
    let want = uids
        .iter()
        .map(|s| {
            let b: Vec<u8> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let b: [u8; 16] = b
                .try_into()
                .map_err(|b: Vec<u8>| anyhow::anyhow!("uid {s:?}: {} bytes, want 16", b.len()))?;
            Ok(gd_formats::map::uid_words(&b))
        })
        .collect::<Result<Vec<_>>>()?;
    for (i, r) in m.regions.iter().enumerate() {
        let Ok(l) = m.level(i) else { continue };
        for e in &l.entities {
            if let Some(u) = e.uid.filter(|u| want.contains(u)) {
                println!("{u:?} {} {} {:?}", r.name, l.record_of(e), e.position);
            }
        }
    }
    Ok(())
}

fn env(path: &Path) -> Result<()> {
    use gd_formats::map::sector::{EnvironmentFields, SectorEntry};
    let m = open_map(path)?;
    if let Some((_, entries)) = m.sector_data.layers.first() {
        for e in entries {
            if let SectorEntry::Fog(b, v) = e
                && let Some(f) = EnvironmentFields::from_dwords(v)
            {
                println!("environment {:?} {:?}: {f:?}", b.name, b.ints);
            }
        }
    }
    let t = m.sector_data.tail_layers()?;
    for (b, d) in &t.daynight {
        println!(
            "daynight {:?} {:?}: v{} sunset {} sunrise {} night {:?} sun {} {} keys {:?}",
            b.name,
            b.ints,
            d.version,
            d.sunset,
            d.sunrise,
            d.night,
            d.sun_azimuth,
            d.sun_min_angle,
            d.keys
        );
    }
    for c in &t.later.challenge {
        println!(
            "challenge {:?} {:?}: {}",
            c.base.name, c.base.ints, c.record
        );
    }
    for b in &t.later.bloom {
        println!("bloom {:?} {:?}: {:?}", b.base.name, b.base.ints, b.params);
    }
    for a in &t.later.adjustment {
        println!(
            "adjustment {:?} {:?}: brightness {} contrast {} saturation {} tint {:?}",
            a.base.name, a.base.ints, a.brightness, a.contrast, a.saturation, a.tint
        );
    }
    Ok(())
}

fn sector_at(path: &Path, x: f32, z: f32) -> Result<()> {
    use gd_formats::map::sector::SECTOR_LAYER_NAMES;
    let m = open_map(path)?;
    for i in 0..m.regions.len() {
        let [x0, z0, x1, z1] = level_rect(&m, i, None);
        if x < x0 || z < z0 || x >= x1 || z >= z1 {
            continue;
        }
        let Ok(l) = m.level(i) else { continue };
        let Some(s) = &l.sector_layers else { continue };
        let r = &m.regions[i];
        let (lx, lz) = (x - r.position[0] as f32, z - r.position[2] as f32);
        println!(
            "{} local ({lx:.1}, {lz:.1}) grid {}x{}",
            r.name, s.width, s.height
        );
        for (li, name) in SECTOR_LAYER_NAMES.iter().enumerate().take(s.palettes.len()) {
            let (cx, cz) = (
                (lx as i32).clamp(0, s.width as i32 - 1) as u32,
                (lz as i32).clamp(0, s.height as i32 - 1) as u32,
            );
            let painted = (0..s.width)
                .flat_map(|a| (0..s.height).map(move |b| (a, b)))
                .filter(|&(a, b)| s.get(li, a, b) != 0)
                .count();
            let id = s.get(li, cx, cz);
            let guid = s.palettes[li]
                .iter()
                .find(|(v, _)| *v == id)
                .map(|(_, g)| *g);
            println!(
                "  {li:2} {name:12} id {id:3} guid {guid:?} painted {:.0}%",
                100.0 * painted as f32 / (s.width * s.height) as f32
            );
        }
    }
    Ok(())
}

fn challenge_cells(path: &Path, filter: &str) -> Result<()> {
    let m = open_map(path)?;
    let entries = m.sector_data.tail_layers()?.later.challenge;
    let f = filter.to_lowercase();
    for i in 0..m.regions.len() {
        let Ok(l) = m.level(i) else { continue };
        let Some(s) = &l.sector_layers else { continue };
        let Some(pal) = s.palettes.get(13) else {
            continue;
        };
        let r = &m.regions[i];
        for (v, guid) in pal {
            let Some(e) = entries.iter().find(|e| e.base.ints == *guid) else {
                continue;
            };
            if !e.base.name.to_lowercase().contains(&f) && !e.record.contains(&f) {
                continue;
            }
            let cells: Vec<(u32, u32)> = (0..s.width)
                .flat_map(|a| (0..s.height).map(move |b| (a, b)))
                .filter(|&(a, b)| s.get(13, a, b) == *v)
                .collect();
            if cells.is_empty() {
                continue;
            }
            let n = cells.len() as f32;
            let (cx, cz) = cells.iter().fold((0.0, 0.0), |(x, z), (a, b)| {
                (x + *a as f32 / n, z + *b as f32 / n)
            });
            // The painted cell nearest the centre (a point inside the area).
            let (bx, bz) = cells
                .iter()
                .min_by(|p, q| {
                    let d = |c: &(u32, u32)| (c.0 as f32 - cx).powi(2) + (c.1 as f32 - cz).powi(2);
                    d(p).total_cmp(&d(q))
                })
                .copied()
                .unwrap_or_default();
            println!(
                "{} {:?}: {} cells, inside at {:.1},{:.1} ({})",
                r.name,
                e.base.name,
                cells.len(),
                r.position[0] as f32 + bx as f32 + 0.5,
                r.position[2] as f32 + bz as f32 + 0.5,
                e.record
            );
        }
    }
    Ok(())
}
