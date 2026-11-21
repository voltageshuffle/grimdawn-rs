//! A stock map (a Crucible arena, `survivalworld_i.map`, read through the mounted mod) parses with all of
//! its levels, including water paths: `WaterPath::Load`'s layout (gd-formats `map/water.rs`).
use gd_formats::map::MapFile;
use gd_formats::testdata;
use gd_formats::vfs::{SURVIVAL_MOD, Vfs};

#[test]
fn stock_map_parses() {
    let Some(root) = testdata::game() else { return };
    let vfs = Vfs::open(&root).unwrap();
    vfs.initialize_mod(SURVIVAL_MOD).unwrap();
    let map = MapFile::open_resource(&vfs, "maps/survivalworld_i.map").unwrap();
    assert!(!map.regions.is_empty());
    let (mut paths, mut points) = (0, 0);
    for i in 0..map.regions.len() {
        let level = map.level(i).unwrap();
        let Some(w) = &level.water else { continue };
        for p in &w.paths {
            paths += 1;
            assert!(p.points.len() >= 2, "a path is at least one segment");
            assert!(
                w.types
                    .iter()
                    .any(|t| t.name.eq_ignore_ascii_case(&p.water_type)),
                "a path's water type is one of its level's"
            );
            for c in p.control_points() {
                points += 1;
                assert!(c.width > 0.0);
                // The side is the unit left normal of the handle.
                let t = (c.tangent[0].hypot(c.tangent[1]), c.side[0].hypot(c.side[1]));
                assert!((t.1 - 1.0).abs() < 1e-3, "unit side {c:?}");
                let n = [-c.tangent[1] / t.0, c.tangent[0] / t.0];
                assert!(
                    (n[0] - c.side[0]).abs() < 1e-3 && (n[1] - c.side[1]).abs() < 1e-3,
                    "{c:?}"
                );
            }
        }
    }
    assert!(paths > 10 && points > 50, "{paths} paths, {points} points");
}
