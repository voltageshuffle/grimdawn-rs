//! Devotion: constellations of stars taken with devotion points.
//!
//! Data: `devotion_mastertable.dbr` (`devotionpane.tpl`) lists
//! `devotionConstellation1..` (`devotionconstellation.tpl`). A constellation
//! has stars `devotionButtonN` (`skillbutton.tpl`: `skillName` is the star's
//! skill, `bitmapPositionX/Y` its place on the map), `devotionLinksN` (the
//! 1-based star that star N hangs from), affinity requirements
//! `affinityRequiredN` / `affinityRequiredNameN` and the affinity it gives
//! once complete (`affinityGivenN` / `affinityGivenNameN`).
//!
//! Rules (exe `DevotionWindow::UpdateLocks` / `DevotionWindow_ResetAll`): a star with links needs
//! one of its linked stars taken; a constellation is available when the
//! player's affinity of every required type is at least the requirement;
//! affinity is the sum of what completed constellations give
//! (`Character::AddAffinity` on completion).
//!
//! Points (`ScriptableAction_GiveDevotion::Execute` and
//! `GameEngine::HandleShrineReward`): a grant raises both the
//! unspent and the total points, capped so the total stays at most
//! `maxDevotionPoints` (`playerlevels.dbr`, 55).
//!
//! Celestial powers (`SkillManager::AddExperience`):
//! a power bound to a skill gains the experience the character
//! receives and levels up while its experience reaches
//! `skillExperienceLevels[level]`, up to the table's length.
//!
//! Removing a star (exe star lock refresh `DevotionWindow::UpdateLocks` in reclaim mode and its
//! affinity check `DevotionWindow::CheckAffinityLoss`; the aether / gold refund cost is
//! `ui/devotion.rs`): a star is locked while any taken star of its
//! constellation lists it in its links, even when that star has another
//! taken link; and, when its constellation is complete, while the player's
//! affinity less that constellation's bonus no longer covers the
//! requirements of any constellation with a taken star (its own included).
//! The max power level is the length of `skillExperienceLevels` when it has
//! more than one entry (`SkillProfile::LoadProfile`).
use gd_formats::vfs::Database;

pub const MASTER: &str = "records/ui/skills/devotion/devotion_mastertable.dbr";
pub const AFFINITIES: [&str; 5] = ["Ascendant", "Chaos", "Eldritch", "Order", "Primordial"];

pub fn affinity_index(name: &str) -> Option<usize> {
    AFFINITIES
        .iter()
        .position(|a| a.eq_ignore_ascii_case(name.trim()))
}

#[derive(Clone, Debug)]
pub struct Star {
    /// The star's skill record (lower-case).
    pub skill: String,
    /// The button record (bitmaps).
    pub button: String,
    pub pos: [f32; 2],
    /// Indices (0-based) of the stars this one hangs from.
    pub links: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Constellation {
    pub record: String,
    pub name_tag: String,
    pub required: [u32; 5],
    pub given: [u32; 5],
    pub stars: Vec<Star>,
}

#[derive(Clone, Debug, Default)]
pub struct Devotions {
    pub list: Vec<Constellation>,
}

fn affinities(r: &gd_formats::arz::Record, value: &str, name: &str) -> [u32; 5] {
    let mut out = [0; 5];
    for n in 1..=5 {
        let v = r.i32(&format!("{value}{n}")).unwrap_or(0);
        if v > 0
            && let Some(i) = r.str(&format!("{name}{n}")).and_then(affinity_index)
        {
            out[i] += v as u32;
        }
    }
    out
}

impl Devotions {
    pub fn load(db: &Database) -> Self {
        let Some(m) = db.get(MASTER) else {
            return Self::default();
        };
        let mut list = Vec::new();
        for n in 1..=200 {
            let Some(path) = m
                .str(&format!("devotionConstellation{n}"))
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let Some(c) = db.get(path) else { continue };
            // The resolved buttons with their slot numbers: `devotionLinksN` names a slot, which
            // differs from the star's index once a slot is empty or unresolved.
            let buttons: Vec<_> = (1..=40)
                .filter_map(|k| {
                    let b = c
                        .str(&format!("devotionButton{k}"))
                        .filter(|s| !s.is_empty())?;
                    Some((k, b, db.get(b)?))
                })
                .collect();
            let stars = buttons
                .iter()
                .map(|(k, b, br)| Star {
                    skill: br.str("skillName").unwrap_or("").to_ascii_lowercase(),
                    button: (*b).to_owned(),
                    pos: [
                        br.f32("bitmapPositionX").unwrap_or(0.0),
                        br.f32("bitmapPositionY").unwrap_or(0.0),
                    ],
                    links: c
                        .f32s(&format!("devotionLinks{k}"))
                        .iter()
                        .filter(|v| **v >= 1.0)
                        .filter_map(|v| buttons.iter().position(|x| x.0 == *v as usize))
                        .collect(),
                })
                .collect();
            list.push(Constellation {
                record: path.to_owned(),
                name_tag: c.str("constellationDisplayTag").unwrap_or("").to_owned(),
                required: affinities(c, "affinityRequired", "affinityRequiredName"),
                given: affinities(c, "affinityGiven", "affinityGivenName"),
                stars,
            });
        }
        Self { list }
    }

    /// (constellation, star) of a star skill.
    pub fn locate(&self, skill: &str) -> Option<(usize, usize)> {
        self.list.iter().enumerate().find_map(|(c, k)| {
            k.stars
                .iter()
                .position(|s| s.skill.eq_ignore_ascii_case(skill))
                .map(|s| (c, s))
        })
    }

    pub fn complete(&self, c: usize, taken: &dyn Fn(&str) -> bool) -> bool {
        let k = &self.list[c];
        !k.stars.is_empty() && k.stars.iter().all(|s| taken(&s.skill))
    }

    /// Affinity from completed constellations.
    pub fn affinity(&self, taken: &dyn Fn(&str) -> bool) -> [u32; 5] {
        let mut a = [0; 5];
        for (c, k) in self.list.iter().enumerate() {
            if self.complete(c, taken) {
                for i in 0..5 {
                    a[i] += k.given[i];
                }
            }
        }
        a
    }

    pub fn available(&self, c: usize, affinity: &[u32; 5]) -> bool {
        self.list[c]
            .required
            .iter()
            .zip(affinity)
            .all(|(r, a)| a >= r)
    }

    /// Why the star cannot be taken, if it cannot.
    pub fn cannot_take(
        &self,
        c: usize,
        s: usize,
        taken: &dyn Fn(&str) -> bool,
        points: u32,
    ) -> Option<&'static str> {
        self.cannot_take_with(c, s, taken, points, &self.affinity(taken))
    }

    /// [`Self::cannot_take`] with the player's [`Self::affinity`] computed once by the caller.
    pub fn cannot_take_with(
        &self,
        c: usize,
        s: usize,
        taken: &dyn Fn(&str) -> bool,
        points: u32,
        affinity: &[u32; 5],
    ) -> Option<&'static str> {
        let k = &self.list[c];
        let star = &k.stars[s];
        if taken(&star.skill) {
            return Some("already taken");
        }
        if points == 0 {
            return Some("no devotion points");
        }
        if !self.available(c, affinity) {
            return Some("not enough affinity");
        }
        if !star.links.is_empty()
            && !star
                .links
                .iter()
                .any(|&l| k.stars.get(l).is_some_and(|p| taken(&p.skill)))
        {
            return Some("needs a linked star");
        }
        None
    }

    /// Why the star cannot be removed (see the module docs).
    pub fn cannot_remove(
        &self,
        c: usize,
        s: usize,
        taken: &dyn Fn(&str) -> bool,
    ) -> Option<&'static str> {
        let k = &self.list[c];
        let gone = &k.stars[s].skill;
        if !taken(gone) {
            return Some("not taken");
        }
        // Every taken star locks the stars it links to.
        if k.stars
            .iter()
            .enumerate()
            .any(|(i, st)| i != s && taken(&st.skill) && st.links.contains(&s))
        {
            return Some("another star depends on it");
        }
        if !self.affinity_dependents(c, taken).is_empty() {
            return Some("another constellation needs its affinity");
        }
        None
    }

    /// The constellations that would lose their affinity requirement if constellation `c` lost its
    /// completion bonus (GrimDawn.exe `DevotionWindow::CheckAffinityLoss`): only a complete constellation gives
    /// affinity; its bonus is taken off the player's affinity and every constellation with a taken
    /// star is checked, `c` itself included (then it is "self locked",
    /// `FailedConstellationSelfLocked`).
    pub fn affinity_dependents(&self, c: usize, taken: &dyn Fn(&str) -> bool) -> Vec<usize> {
        self.affinity_dependents_with(c, taken, &self.affinity(taken))
    }

    /// [`Self::affinity_dependents`] with the player's [`Self::affinity`] computed once by the caller.
    pub fn affinity_dependents_with(
        &self,
        c: usize,
        taken: &dyn Fn(&str) -> bool,
        affinity: &[u32; 5],
    ) -> Vec<usize> {
        if !self.complete(c, taken) {
            return Vec::new();
        }
        let mut aff = *affinity;
        for (a, g) in aff.iter_mut().zip(&self.list[c].given) {
            *a = a.saturating_sub(*g);
        }
        (0..self.list.len())
            .filter(|&i| {
                self.list[i].stars.iter().any(|st| taken(&st.skill)) && !self.available(i, &aff)
            })
            .collect()
    }
}

/// Devotion points from a grant: (added to unspent and total).
pub fn grant(total: u32, max: u32, amount: u32) -> u32 {
    amount.min(max.saturating_sub(total))
}

/// Celestial power experience: adds `xp` and levels up; returns the new
/// (level, experience).
pub fn add_power_experience(levels: &[i32], level: u32, experience: u32, xp: u32) -> (u32, u32) {
    let max = levels.len() as u32;
    let mut level = level.max(1);
    let experience = experience.saturating_add(xp);
    while level < max && experience >= levels[level as usize].max(0) as u32 {
        level += 1;
    }
    (level, experience)
}
