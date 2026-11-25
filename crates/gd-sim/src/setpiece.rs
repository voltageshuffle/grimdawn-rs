//! Set pieces (`SetPiece` entities: camps, statue groups, barriers, potion
//! chests, ...). A set piece picks one `SetPiecePool` and places a few of its
//! objects (creatures, props, chests) around itself.
//!
//! * `SetPiece::Load`: with `FGenerate(0, 100) <= placementChance`
//!   the pools `pool1..5` with a non-zero `weight1..5` are collected.
//! * `SetPiece::Generate`: a pool by `IGenerate(0, Σw)` (first
//!   cumulative weight ≥ the roll); from it `placementRadius`,
//!   `placementScheme` (Random 0, Grid 1, Camp 2, Ring 3), `minToPlace`,
//!   `maxToPlace` and entries 1..10: `objectN` (a list), `limitN`, `weightN`,
//!   `forceN`, `rotationN`, `scaleMinN/MaxN`, `tagsN`, `bufferN`,
//!   `blockPathingN`.
//! * `SetPiece::Pick`, on the set piece's own Park–Miller stream:
//!   the count is `min + r % (max - min + 1)`; each object is a
//!   `PickEntry`: `r % (Σw + 1)` over the entries with uses left
//!   (`limit` 0 = unlimited; first cumulative weight ≥ the roll), then one of
//!   the entry's objects by `r % len`; an entry whose limit runs out leaves
//!   the total.
//! * Tags (`StringToTag`): main/center 1, secondary/perimeter 2,
//!   collection 4, interior 8. Rotations (`StringToRotation`): None 0, Y 1,
//!   XYZ 2, Aligned 3, FaceOut 4, FaceIn 5.
//!
//! Parts (`SetPiecePart`) are added after the objects (see [`run_set_piece_shared`]).
//!
//! Entity groups (`SetPiece::Load`): the host spends a set piece its `Unique Entities` /
//! `Inverse Unique Entities` group leaves out before the placement-chance roll
//! (`gd_game::unique::Loader::SetPiece`: Unique and not chosen, or Inverse and
//! chosen); the `Unified Entities` flag shares one pool index across the group
//! ([`run_set_piece_shared`]'s `pick`, `SetPiece::Generate`'s
//! `GetEntityUnificationIndex` / `SetEntityUnificationIndex`). Placement itself
//! is the host's (see [`Placement`]); [`SetPieceRun::stream`] is the set piece's
//! own stream after the picks, which the positioning continues (scale and
//! rotation rolls, `SetPiece::Apply*Positioning` / `ApplyRotation`).
use gd_formats::vfs::Database;
use gd_rules::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Random,
    Grid,
    Camp,
    Ring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Y,
    Xyz,
    Aligned,
    FaceOut,
    FaceIn,
}

pub const TAG_MAIN: u32 = 1;
pub const TAG_SECONDARY: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub record: String,
    /// Entry number (1..10) in the pool.
    pub entry: usize,
    pub tags: u32,
    pub rotation: Rotation,
    /// `scaleMinN` / `scaleMaxN` (parts: `minScale` / `maxScale`), rolled
    /// while positioning.
    pub scale: (f32, f32),
    pub buffer: f32,
    pub force: bool,
    pub block_pathing: bool,
    /// The record's `actorRadius` (`Actor::GetRadius` scales it by the
    /// length of the scale vector).
    pub radius: f32,
    /// Parts: `positionX/Y/Z`, the fixed offset from the set piece.
    pub offset: Option<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetPieceRun {
    pub pool: String,
    pub scheme: Scheme,
    pub radius: f32,
    pub objects: Vec<Placement>,
    /// The set piece's own stream after the picks.
    pub stream: Rng,
}

struct Entry<'a> {
    objects: Vec<&'a str>,
    left: i32,
    limit: i32,
    weight: i32,
    template: Placement,
}

fn scheme(s: &str) -> Scheme {
    match s {
        "Grid" => Scheme::Grid,
        "Camp" => Scheme::Camp,
        "Ring" => Scheme::Ring,
        _ => Scheme::Random,
    }
}

fn rotation(s: &str) -> Rotation {
    match s {
        "Y" => Rotation::Y,
        "XYZ" => Rotation::Xyz,
        "Aligned" => Rotation::Aligned,
        "FaceOut" => Rotation::FaceOut,
        "FaceIn" => Rotation::FaceIn,
        _ => Rotation::None,
    }
}

fn tag(s: &str) -> u32 {
    match s {
        "main" | "center" => 1,
        "secondary" | "perimeter" => 2,
        "collection" => 4,
        "interior" => 8,
        _ => 0,
    }
}

/// `r % n` with the engine's guard against n = 0 (`SetPiece::Pick`); a
/// negative span (bad data: max below min) counts as 1
/// instead of wrapping to a huge unsigned modulus. Widened to `i64` so the
/// callers' `hi - lo + 1` / `total + 1` cannot overflow (bad data: counts or
/// weights near `i32::MAX`); identical to the engine's 32-bit maths on stock data.
fn modulo(r: u32, n: i64) -> i64 {
    i64::from(r) % n.max(1)
}

/// Placement / part count ceiling (bad data: a huge `minToPlace` /
/// `minParts`). Stock set pieces place far fewer.
const MAX_PICKS: i64 = 1000;

/// `lo + r % (hi - lo + 1)`, the count roll, capped at [`MAX_PICKS`].
fn count(r: u32, lo: i32, hi: i32) -> i64 {
    (i64::from(lo) + modulo(r, i64::from(hi) - i64::from(lo) + 1)).min(MAX_PICKS)
}

/// Runs a set piece record. `rng` is the engine's shared stream (placement
/// chance, pool choice); `own` the set piece's stream (seeded from it).
/// `totem_chance`: [`crate::ascendant::totem_chance`].
///
/// A set piece of a "Unified Entities" group (`SetPiece::Generate`'s use of
/// `GameEngine::GetEntityUnificationIndex` / `SetEntityUnificationIndex`): `pick` is the
/// group's stored index into this set piece's pools (the non-zero `weight1..5` columns, in
/// order); with `None` the pool is chosen by weight. An index past the pool
/// list falls back to the weighted choice. Either way the chosen index goes
/// back to `pick` (the engine stores it after every choice, overwriting).
pub fn run_set_piece_shared(
    db: &Database,
    record: &str,
    totem_chance: f32,
    pick: &mut Option<usize>,
    rng: &mut Rng,
) -> Option<SetPieceRun> {
    let r = db.get(record)?;
    if rng.unit() * 100.0 > r.f32("placementChance").unwrap_or(0.0) {
        return None;
    }
    let mut own = Rng(rng.next().max(1));
    own.next();
    let pools: Vec<(&str, i32)> = (1..=5)
        .filter_map(|i| {
            Some((
                r.str(&format!("pool{i}"))?,
                r.i32(&format!("weight{i}")).unwrap_or(0),
            ))
        })
        .filter(|p| p.1 != 0)
        .collect();
    let total: i64 = pools.iter().map(|p| i64::from(p.1)).sum();
    if pools.is_empty() {
        return None;
    }
    // The group's stored choice first (it does not roll), else the weighted
    // roll; the chosen index is stored back (`SetEntityUnificationIndex`).
    let chosen = match pick.filter(|i| *i < pools.len()) {
        Some(i) => i,
        None => {
            let roll = modulo(rng.next(), total + 1);
            let mut acc = 0;
            pools.iter().position(|(_, w)| {
                acc += i64::from(*w);
                roll <= acc
            })?
        }
    };
    *pick = Some(chosen);
    // Monster totems: the set piece's `ascendantPool` on the totem roll
    // (`SetPiece::Generate`; `crate::ascendant`).
    let pool = r
        .str("ascendantPool")
        .filter(|_| crate::ascendant::roll(rng, totem_chance))
        .unwrap_or(pools[chosen].0);
    let p = db.get(pool)?;
    let mut entries: Vec<Entry> = (1..=10)
        .filter_map(|i| {
            let objects = p.strs(&format!("object{i}"));
            if objects.is_empty() {
                return None;
            }
            let lo = p.f32(&format!("scaleMin{i}")).unwrap_or(1.0);
            let hi = p.f32(&format!("scaleMax{i}")).unwrap_or(lo).max(lo);
            let limit = p.i32(&format!("limit{i}")).unwrap_or(0);
            Some(Entry {
                objects,
                left: limit,
                limit,
                weight: p.i32(&format!("weight{i}")).unwrap_or(0),
                template: Placement {
                    record: String::new(),
                    entry: i,
                    tags: p
                        .strs_iter(&format!("tags{i}"))
                        .map(tag)
                        .fold(0, |a, b| a | b),
                    rotation: rotation(p.str(&format!("rotation{i}")).unwrap_or("None")),
                    scale: (lo, hi),
                    buffer: p.f32(&format!("buffer{i}")).unwrap_or(0.0),
                    force: p.bool(&format!("force{i}")),
                    block_pathing: p.bool(&format!("blockPathing{i}")),
                    radius: 0.0,
                    offset: None,
                },
            })
        })
        .collect();
    let (lo, hi) = (
        p.i32("minToPlace").unwrap_or(0),
        p.i32("maxToPlace").unwrap_or(0),
    );
    let placed = count(own.next(), lo, hi);
    let mut total: i64 = entries.iter().map(|e| i64::from(e.weight)).sum();
    let mut objects = Vec::new();
    for _ in 0..placed.max(0) {
        let roll = modulo(own.next(), total + 1);
        let mut acc = 0;
        let Some(e) = entries.iter_mut().find(|e| {
            if e.left != 0 || e.limit == 0 {
                acc += i64::from(e.weight);
                roll <= acc
            } else {
                false
            }
        }) else {
            continue;
        };
        let which = modulo(own.next(), e.objects.len() as i64) as usize;
        let mut obj = e.template.clone();
        obj.record = e.objects[which].to_owned();
        obj.radius = db
            .get(&obj.record)
            .and_then(|r| r.f32("actorRadius"))
            .unwrap_or(0.0);
        objects.push(obj);
        if e.limit != 0 {
            e.left -= 1;
            if e.left == 0 {
                total -= i64::from(e.weight);
            }
        }
    }
    // Parts (`SetPiece::Pick`): `minParts +
    // r % (maxParts - minParts + 1)` picks, each `r % (budget + 1)` over the
    // cumulative `weight`s, the picked part's weight leaving the budget
    // (parts may repeat); `CreateParts` creates every one of its `names`.
    // Each name becomes an entry holding the part (`CreateParts`:
    // no tags, rotation or force; the part's `buffer` and
    // `blockPathing`); positioning puts it at the part's `positionX/Y/Z`
    // (`SetPiecePart::Load`) from the set piece.
    let parts: Vec<(&gd_formats::arz::Record, i32)> = p
        .strs_iter("parts")
        .filter_map(|n| db.get(n))
        .map(|r| {
            let w = r.i32("weight").unwrap_or(0);
            (r, w)
        })
        .collect();
    if !parts.is_empty() {
        let lo = p.i32("minParts").unwrap_or(0);
        let hi = p.i32("maxParts").unwrap_or(parts.len() as i32);
        let n = count(own.next(), lo, hi);
        let mut budget: i64 = parts.iter().map(|x| i64::from(x.1)).sum();
        for _ in 0..n.max(0) {
            let roll = modulo(own.next(), budget + 1);
            let mut acc = 0;
            let Some((part, w)) = parts.iter().find(|(_, w)| {
                acc += i64::from(*w);
                roll <= acc
            }) else {
                continue;
            };
            budget -= i64::from(*w);
            for name in part.strs_iter("names") {
                objects.push(Placement {
                    record: name.to_string(),
                    entry: 0,
                    tags: 0,
                    rotation: Rotation::None,
                    scale: (
                        part.f32("minScale").unwrap_or(1.0),
                        part.f32("maxScale").unwrap_or(1.0),
                    ),
                    buffer: part.f32("buffer").unwrap_or(0.0),
                    force: false,
                    block_pathing: part.bool("blockPathing"),
                    radius: db
                        .get(name)
                        .and_then(|r| r.f32("actorRadius"))
                        .unwrap_or(0.0),
                    offset: Some([
                        part.f32("positionX").unwrap_or(0.0),
                        part.f32("positionY").unwrap_or(0.0),
                        part.f32("positionZ").unwrap_or(0.0),
                    ]),
                });
            }
        }
    }
    Some(SetPieceRun {
        pool: pool.to_owned(),
        scheme: scheme(p.str("placementScheme").unwrap_or("")),
        radius: p.f32("placementRadius").unwrap_or(0.0),
        objects,
        stream: own,
    })
}
