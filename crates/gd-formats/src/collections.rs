//! Hash maps and sets for the port's trusted game data.
//!
//! Keys come from the game install and our own ids, never untrusted input, so std's SipHash is not
//! needed. `clippy.toml` bans the std types; use these aliases instead.
//!
//! The hasher is [`foldhash::fast::FixedState`]: its fixed seed makes iteration order reproducible
//! across runs for the same insertion sequence, which the determinism tests rely on.
//!
//! Constructors: these maps have no `new()`/`with_capacity()` (those are std-`RandomState`-only); use
//! `HashMap::default()` / `HashMap::with_capacity_and_hasher(n, Default::default())`. Sorted iteration
//! still wants `BTreeMap`/`BTreeSet`.
#![allow(clippy::disallowed_types)] // the aliases themselves name the std types

use std::hash::{BuildHasher, Hash};

/// A [`std::collections::HashMap`] with the port's fast hasher.
pub type HashMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::FixedState>;

/// A [`std::collections::HashSet`] with the port's fast hasher.
pub type HashSet<K> = std::collections::HashSet<K, foldhash::fast::FixedState>;

/// Hash one value with the port's hasher: stable, process-local identity keys that are not
/// themselves map keys (e.g. an item instance's DoT owner id). Never saved.
pub fn hash_one<T: Hash>(value: T) -> u64 {
    foldhash::fast::FixedState::default().hash_one(value)
}
