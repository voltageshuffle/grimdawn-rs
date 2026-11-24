//! Hash maps and sets for the port's trusted game data.
//!
//! `std`'s `HashMap`/`HashSet` default to SipHash-1-3, a DoS-resistant hasher we don't need: keys come
//! from the game install and our own ids, never from untrusted input. On the port's key mix (record-path
//! strings, u32/u64 ids, `[u32; 4]` uids, cell tuples) foldhash measures 2–4× faster per operation
//! (45-byte path miss: 8.7 ns vs 27.6 ns; u64 hit: 5.9 ns vs 17.5 ns). `clippy.toml` bans the std types;
//! use these aliases instead.
//!
//! The hasher is [`foldhash::fast::FixedState`]:
//! - `fast`: the speed-optimized variant; its weaker collision resistance only matters for adversarial
//!   keys, which we never hash;
//! - fixed seed: iteration order is reproducible across runs for the same insertion sequence (std
//!   randomizes per map), which keeps headless runs and the determinism tests stable.
//!
//! Constructors: these maps have no `new()`/`with_capacity()` (those are std-`RandomState`-only); use
//! `HashMap::default()` / `HashMap::with_capacity_and_hasher(n, Default::default())`. Sorted iteration
//! still wants `BTreeMap`/`BTreeSet`.
#![allow(clippy::disallowed_types)] // the aliases themselves name the std types

use std::hash::{BuildHasher, Hash};

/// A [`std::collections::HashMap`] with the port's fast hasher (module docs).
pub type HashMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::FixedState>;

/// A [`std::collections::HashSet`] with the port's fast hasher (module docs).
pub type HashSet<K> = std::collections::HashSet<K, foldhash::fast::FixedState>;

/// Hash one value with the port's hasher (the aliases' `FixedState`): stable identity keys that are
/// not themselves map keys (e.g. an item instance's DoT owner id). The value is process-local —
/// never saved, never read from game files — so only equality and determinism matter, not the
/// number itself.
pub fn hash_one<T: Hash>(value: T) -> u64 {
    foldhash::fast::FixedState::default().hash_one(value)
}
