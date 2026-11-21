//! How a container throws its drops onto the ground: start point, toss
//! velocity, ballistic flight and the drop timing (the presentation and the
//! ECS side: `gd_game::loot`).
//!
//! Game.dll loot dropper (`sub_180358B20`, shared by monsters,
//! destructibles, chests and shrines):
//! - Timing: after the loot job, wait [`FIRST_DELAY_MS`], then release a
//!   batch per [`BATCH_INTERVAL_MS`]` / players`. Monsters and destructibles
//!   release everything at once; chests (`lootGen` event) and shrines one
//!   item per batch.
//! - Start: the drop point moved by `(r cos a, r, r sin a)`, `r` =
//!   [`drop_radius`] (monsters 0), `a` = 0 (no stock caller changes it).
//! - Toss: velocity [`toss_velocity`], yaw [`random_yaw`]. Untossed items
//!   (a monster's death-animation loot event) appear at the drop point and fall.
//! - Flight: a rigid body ([`crate::drop_body`], terrain and static scenery
//!   only; drops do not collide with each other, so there is no scatter). It
//!   lands at first contact or when its centre of mass reaches the path mesh,
//!   then is lifted onto the mesh and gets its beam and (unless played on
//!   contact) its drop sound.
//! - Drop points: a monster's centre of mass (or its controller's
//!   `lootDropAttachPointName`); a chest's, destructible's or shrine's
//!   `LootDrop` attach point else centre of mass, raised to
//!   [`MIN_DROP_HEIGHT`] above its floor ([`raise_drop`]).
//! - Pose: identity, or tossed a pure yaw; no lay-flat turn (ground meshes
//!   are modelled lying). The body spins at 20 × its Z axis and rests in the
//!   pose it first touched the world in.

/// Toss speed (m/s), `sub_180358B20`.
pub const TOSS_SPEED: f32 = 3.0;
/// Wait after the loot job before the first batch (ms).
pub const FIRST_DELAY_MS: f32 = 250.0;
/// Interval between batches for one player (ms; divided by the players).
pub const BATCH_INTERVAL_MS: f32 = 200.0;
/// The lowest a chest's or destructible's drop point sits above its floor.
pub const MIN_DROP_HEIGHT: f32 = 1.0;
/// The least loot drop radius of chests and destructibles.
pub const MIN_DROP_RADIUS: f32 = 0.4;

/// `GetLootDropRadius` of a chest, destructible or shrine whose record's
/// `actorRadius` is `actor_radius`.
pub fn drop_radius(actor_radius: f32) -> f32 {
    actor_radius.max(MIN_DROP_RADIUS)
}

/// The height of a chest's, destructible's or shrine's drop point at `y`
/// over a floor at `floor`: at least [`MIN_DROP_HEIGHT`] above it.
pub fn raise_drop(y: f32, floor: f32) -> f32 {
    y.max(floor + MIN_DROP_HEIGHT)
}

/// Velocity of an item tossed at angle `a` (radians, from +X towards +Z):
/// `TOSS_SPEED · normalize(cos a, 2, sin a)`.
pub fn toss_velocity(a: f32) -> [f32; 3] {
    /// `1 / |(cos a, 2, sin a)|` = `1 / √5`.
    const N: f32 = 0.447_213_6;
    let (s, c) = a.sin_cos();
    [N * c * TOSS_SPEED, N * 2.0 * TOSS_SPEED, N * s * TOSS_SPEED]
}

/// Where a tossed item starts: the drop point moved by the radius along the
/// angle, and up by the radius.
pub fn toss_start(drop: [f32; 3], radius: f32, a: f32) -> [f32; 3] {
    let (s, c) = a.sin_cos();
    [drop[0] + radius * c, drop[1] + radius, drop[2] + radius * s]
}

/// The item's yaw from one `rand()` value (0..=32767).
pub fn random_yaw(rand: u32) -> f32 {
    rand as f32 * 0.000_191_753_45
}

/// When the release of item `i` (0-based) comes, in ms after the loot job:
/// `batch` items per release, `players` sharing the interval.
pub fn release_ms(i: usize, batch: usize, players: u32) -> f32 {
    let interval = (BATCH_INTERVAL_MS as u32 / players.max(1)) as f32;
    FIRST_DELAY_MS + (i / batch.max(1)) as f32 * interval
}
