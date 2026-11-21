//! Iteration order of the engine's MSVC `std::unordered_map` (Game.dll's pre-VS2019 `_Hash`).
//!
//! Game.dll, two instantiations of the same `_Hash`: the `SkillManager` buff map
//! `unordered_map<string, _>` (insert 0x1805435a0, rehash check 0x1805433e0) and the
//! `Quest2Repository` map `unordered_map<u32, Quest2*>` (`LoadQuests` 0x180422870 → insert 0x18001be50,
//! rehash check 0x18001bcb0):
//! - hash: 64-bit FNV-1a over the key's bytes (a string's characters; a `u32`'s 4 little-endian bytes);
//!   bucket = hash & (buckets − 1), 8 buckets at first;
//! - all nodes live in one list with a [lo, hi] range per bucket: a new node goes to the list's end when its
//!   bucket is empty, else just before its bucket's lo;
//! - after linking the node (so with the new size), when `size / buckets > max_load_factor` (1.0, compared as
//!   `f32`) the bucket count grows × 8 (× 2 from 512) and every node is inserted again in list order.

/// 64-bit FNV-1a: MSVC's `std::hash` over a key's bytes.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The list order (indices into `keys`) of an MSVC `unordered_map` after inserting the distinct `keys` (their
/// bytes, see [`fnv1a`]) in this order into an empty map.
pub fn msvc_unordered_order<K: AsRef<[u8]>>(keys: &[K]) -> Vec<usize> {
    let hashes: Vec<u64> = keys.iter().map(|k| fnv1a(k.as_ref())).collect();
    let insert = |list: &mut Vec<usize>, i: usize, buckets: u64| {
        let b = hashes[i] & (buckets - 1);
        match list.iter().position(|&j| hashes[j] & (buckets - 1) == b) {
            Some(p) => list.insert(p, i),
            None => list.push(i),
        }
    };
    let mut buckets = 8u64;
    let mut list = Vec::with_capacity(keys.len());
    for i in 0..keys.len() {
        insert(&mut list, i, buckets);
        if list.len() as f32 / buckets as f32 > 1.0 {
            buckets *= if buckets < 512 { 8 } else { 2 };
            for j in std::mem::take(&mut list) {
                insert(&mut list, j, buckets);
            }
        }
    }
    list
}
