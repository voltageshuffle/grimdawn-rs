//! Path-finding probe on real levels: renders walkable cells (by height) and
//! a path from the map spawn point to a target as a PNG.
//! Usage: gd-nav <world001.map> <out.png> <target_x> <target_z> [radius]
use gd_formats::map::MapFile;
use gd_sim::nav::{NavLayer, NavWorld};

fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        a.len() >= 5,
        "usage: gd-nav <world001.map> <out.png> <target_x> <target_z> [radius]"
    );
    let map = MapFile::open(&a[1])?;
    let sp = *map.spawn_points.first().context("map has no spawn point")?;
    let r0 = &map.regions[sp.region as usize];
    let start = [
        r0.position[0] as f32 + sp.position[0],
        r0.position[1] as f32 + sp.position[1],
        r0.position[2] as f32 + sp.position[2],
    ];
    let target = [
        a[3].parse::<f32>().context("target_x")?,
        start[1],
        a[4].parse::<f32>().context("target_z")?,
    ];
    let radius: f32 = match a.get(5) {
        Some(s) => s.parse().context("radius")?,
        None => 150.0,
    };
    let mut nav = NavWorld::default();
    for (i, r) in map.regions.iter().enumerate() {
        let c = [
            r.position[0] as f32 + r.bbox.center[0],
            r.position[2] as f32 + r.bbox.center[2],
        ];
        if (c[0] - start[0]).abs() > radius || (c[1] - start[2]).abs() > radius || r.level_size == 0
        {
            continue;
        }
        let lvl = map.level(i)?;
        let Some(p) = lvl.pathing else { continue };
        let all: Vec<_> = p.tiles.into_iter().flatten().collect();
        let ch = NavWorld::cell_height(&all);
        nav.add(
            i as u64,
            all.iter()
                .filter_map(|t| NavLayer::from_tile(t, ch))
                .collect(),
        );
    }
    let t = std::time::Instant::now();
    let mut scratch = gd_sim::nav::PathScratch::default();
    let path = nav.find_path(&mut scratch, start, target, 2_000_000);
    println!(
        "start {start:?} target {target:?} path {:?} in {:?}",
        path.as_ref().map(|p| p.len()),
        t.elapsed()
    );
    // Render a window around start..target at 4 px/m.
    let (minx, minz) = (
        start[0].min(target[0]) - 40.0,
        start[2].min(target[2]) - 40.0,
    );
    let (maxx, maxz) = (
        start[0].max(target[0]) + 40.0,
        start[2].max(target[2]) + 40.0,
    );
    let ppm = 4.0;
    let (w, h) = (((maxx - minx) * ppm) as u32, ((maxz - minz) * ppm) as u32);
    let mut img = vec![20u8; (w * h * 3) as usize];
    for py in 0..h {
        for px in 0..w {
            let q = [
                minx + (px as f32 + 0.5) / ppm,
                start[1],
                minz + (py as f32 + 0.5) / ppm,
            ];
            if let Some(g) = nav.ground([q[0], 1000.0, q[2]]).or_else(|| nav.ground(q)) {
                let v = (((g - start[1]) * 8.0) + 128.0).clamp(40.0, 255.0) as u8;
                let o = ((py * w + px) * 3) as usize;
                img[o..o + 3].copy_from_slice(&[v / 2, v, v / 2]);
            }
        }
    }
    let mut plot = |p: [f32; 3], c: [u8; 3]| {
        let (px, py) = (((p[0] - minx) * ppm) as i32, ((p[2] - minz) * ppm) as i32);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (px + dx, py + dy);
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    let o = ((y as u32 * w + x as u32) * 3) as usize;
                    img[o..o + 3].copy_from_slice(&c);
                }
            }
        }
    };
    if let Some(p) = &path {
        for s in p.windows(2) {
            for k in 0..=100 {
                let t = k as f32 / 100.0;
                plot(
                    [
                        s[0][0] + (s[1][0] - s[0][0]) * t,
                        0.0,
                        s[0][2] + (s[1][2] - s[0][2]) * t,
                    ],
                    [255, 40, 40],
                );
            }
        }
    }
    plot(start, [40, 120, 255]);
    plot(target, [255, 255, 0]);
    let f = std::fs::File::create(&a[2])?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.write_header()?.write_image_data(&img)?;
    Ok(())
}
