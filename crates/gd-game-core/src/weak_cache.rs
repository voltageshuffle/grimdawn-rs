//! Path-keyed caches of shared values that do not keep them alive: an entry is found again while something
//! outside the cache holds its `Arc` (a character its body mesh, an effect its `.pfx`, a playing voice its file),
//! and its memory is freed when the last holder drops it.
//!
//! PORT: the engine's resource managers count references and unload a resource 5 s after its last release
//! (Engine.dll `BaseResourceManager` ctor `BaseResourceManager.unloadDelayMs` = 5000, `UnloadUnreferencedResources`
//! each frame); the port
//! hands out `Arc`s and keeps only `Weak`s, so nothing is swept on a timer and a value goes the moment its last
//! user does. What has to stay warm holds the `Arc` itself (a spawned model, a creature's preloaded sounds), or a
//! cache keeps its last used values in a [`Recent`] set (bounded by count or bytes), so an asset used now and then is
//! not reloaded every time.
use gd_formats::collections::HashMap;
use std::sync::{Arc, Weak};

/// The entries a cache holds before it first drops the dead ones ([`WeakCache::insert`]).
const PRUNE_MIN: usize = 64;

/// Values by normalized path, held weakly, plus the last [`WeakCache::with_recent`] values handed out held strongly
/// (a [`Recent`] set), so a file used now and then is not loaded again each time. A file that failed to load stays
/// failed (`None`) until [`WeakCache::clear`].
pub struct WeakCache<T> {
    map: HashMap<String, Option<Weak<T>>>,
    /// The size at which the next insert drops the dead entries (twice the live ones after a prune).
    prune_at: usize,
    recent: Recent<Arc<T>>,
}

/// How many values a [`WeakCache`] keeps warm by default (a model, a mesh or an effect file each).
pub const RECENT: usize = 64;

impl<T> Default for WeakCache<T> {
    fn default() -> Self {
        Self::with_recent(RECENT)
    }
}

impl<T> WeakCache<T> {
    /// A cache that keeps its last `recent` values loaded.
    pub fn with_recent(recent: usize) -> Self {
        Self {
            map: HashMap::default(),
            prune_at: PRUNE_MIN,
            recent: Recent::new(recent),
        }
    }

    /// The entries still alive (the memory audit).
    pub fn live(&self) -> usize {
        self.map
            .values()
            .filter(|w| w.as_ref().is_some_and(|w| w.strong_count() > 0))
            .count()
    }

    /// Forgets every entry (the mounted mod changed).
    pub fn clear(&mut self) {
        self.map.clear();
        self.recent.clear();
        self.prune_at = PRUNE_MIN;
    }

    /// `Some(value)` for a live entry, `Some(None)` for a file that failed to load, `None` when the path was never
    /// loaded or its value has gone. Does not allocate.
    pub fn get(&self, key: &str) -> Option<Option<Arc<T>>> {
        match self.map.get(key)? {
            None => Some(None),
            Some(w) => w.upgrade().map(Some),
        }
    }

    /// Records `value` (`None`: the load failed) for `key`, reusing the dead entry's key when there is one.
    pub fn insert(&mut self, key: &str, value: Option<&Arc<T>>) {
        let weak = value.map(Arc::downgrade);
        if let Some(slot) = self.map.get_mut(key) {
            *slot = weak;
            return;
        }
        if self.map.len() >= self.prune_at {
            self.map
                .retain(|_, w| w.as_ref().is_none_or(|w| w.strong_count() > 0));
            self.prune_at = (self.map.len() * 2).max(PRUNE_MIN);
        }
        self.map.insert(key.to_owned(), weak);
    }

    /// [`WeakCache::insert`] of a loaded value, kept warm until its user takes it.
    pub fn insert_kept(&mut self, key: &str, value: &Arc<T>) {
        self.insert(key, Some(value));
        self.recent.keep(value);
    }

    /// The value of `key`, made by `load` (and recorded) when it is not alive; kept warm either way.
    pub fn get_or_load(&mut self, key: &str, load: impl FnOnce() -> Option<T>) -> Option<Arc<T>> {
        let value = match self.get(key) {
            Some(hit) => hit,
            None => {
                let value = load().map(Arc::new);
                self.insert(key, value.as_ref());
                value
            }
        };
        if let Some(v) = &value {
            self.recent.keep(v);
        }
        value
    }

    /// [`WeakCache::get`] that keeps a hit warm (a value already in use, about to be used again).
    pub fn get_kept(&mut self, key: &str) -> Option<Option<Arc<T>>> {
        let hit = self.get(key)?;
        if let Some(v) = &hit {
            self.recent.keep(v);
        }
        Some(hit)
    }

    /// Records `value`, loaded elsewhere (a background read), for `key` and keeps it warm until its user takes it.
    pub fn put(&mut self, key: &str, value: Option<T>) -> Option<Arc<T>> {
        let value = value.map(Arc::new);
        self.insert(key, value.as_ref());
        if let Some(v) = &value {
            self.recent.keep(v);
        }
        value
    }
}

/// Frames a [`Fresh`] holds a value loaded in the background (~2 s at 60 fps).
pub const FRESH_FRAMES: u32 = 120;

/// Values a background load just finished, held strongly for [`FRESH_FRAMES`]: the waiter polls each frame and takes
/// its own hold, but a burst of completions (level streaming, a crowd of creatures) would push one out of the
/// [`Recent`] set before then, its load start over and a waiter needing several never have them all at once.
pub struct Fresh<T> {
    held: std::collections::VecDeque<(u32, T)>,
    frame: u32,
}

impl<T> Default for Fresh<T> {
    fn default() -> Self {
        Self {
            held: Default::default(),
            frame: 0,
        }
    }
}

impl<T> Fresh<T> {
    /// Next frame: lets the values held longer than [`FRESH_FRAMES`] go.
    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        while self
            .held
            .front()
            .is_some_and(|(f, _)| self.frame.wrapping_sub(*f) > FRESH_FRAMES)
        {
            self.held.pop_front();
        }
    }

    pub fn hold(&mut self, value: T) {
        self.held.push_back((self.frame, value));
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }
}

/// A shared value [`Recent`] tells apart by identity and charges against its budget.
pub trait Shared: Clone {
    /// The [`Recent`] budget of this kind of value, in [`Shared::cost`] units.
    const BUDGET: usize = RECENT;
    /// The value's identity (the address of its shared allocation).
    fn id(&self) -> usize;
    /// What the value costs against a [`Recent`] budget (default: one entry).
    fn cost(&self) -> usize {
        1
    }
}

impl<T: ?Sized> Shared for Arc<T> {
    fn id(&self) -> usize {
        Arc::as_ptr(self).cast::<()>() as usize
    }
}

/// The values a weak cache handed out last, held strongly up to a budget (summed [`Shared::cost`]): each value
/// once, the least recently used goes first.
pub struct Recent<T> {
    /// The values and their last use (a `clock` tick).
    entries: Vec<(T, u64)>,
    /// Each value's slot in `entries`, by [`Shared::id`].
    slots: HashMap<usize, usize>,
    clock: u64,
    used: usize,
    budget: usize,
}

impl<T: Shared> Default for Recent<T> {
    fn default() -> Self {
        Self::new(T::BUDGET)
    }
}

impl<T> Recent<T> {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: Vec::new(),
            slots: HashMap::default(),
            clock: 0,
            used: 0,
            budget,
        }
    }

    /// Lets every value go.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.slots.clear();
        self.used = 0;
    }
}

impl<T: Shared> Recent<T> {
    /// Keeps `value` warm: marks it the latest, adding it if new and dropping the least recently used values over
    /// the budget (never the latest one). A hit is one hash lookup; an eviction scans the set.
    pub fn keep(&mut self, value: &T) {
        self.clock += 1;
        let id = value.id();
        if let Some(&i) = self.slots.get(&id) {
            self.entries[i].1 = self.clock;
            return;
        }
        self.used += value.cost();
        self.slots.insert(id, self.entries.len());
        self.entries.push((value.clone(), self.clock));
        while self.used > self.budget && self.entries.len() > 1 {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.1)
                .map_or(0, |(i, _)| i);
            let (gone, _) = self.entries.swap_remove(oldest);
            self.slots.remove(&gone.id());
            if let Some(moved) = self.entries.get(oldest) {
                self.slots.insert(moved.0.id(), oldest);
            }
            self.used -= gone.cost();
        }
    }
}
