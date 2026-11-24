//! Skill point reclamation: the spirit guide's refunds (the rules; the session
//! state and the requests are `gd_game::reclamation`, the window
//! `grimdawn::ui::skills`).
//!
//! Price (Game.dll `SkillManager::UseReclamationPoints`,
//! `GetCurrentSkillReclamationCost`, `SkillManager::Load` reads
//! the player record's `reclamationPointTiers` / `reclamationPointCosts`): the
//! refund count (`SkillManager.reclamationPointsUsed`, save block 8
//! `skill_reclamation_points_used`) picks the first band
//! `[tiers[i], tiers[i + 1])` holding it, else the last cost ([`cost`]). A
//! refund of `n` points charges the cost at each count before raising it and
//! fails (count unchanged) when the money is short; a negative `n` (the
//! window's undo) lowers the count first and pays the cost at the lowered
//! count back, the money capped at 2,000,000,000. The devotion refunds are
//! the same with the `devotion*` fields (`gd_game::devotion::reclaim_cost`).
//!
//! Which buttons refund (GrimDawn.exe `UISkillPane::GetRequirementFlags` flags, used by
//! the button update `UISkillPane::Update` and the frame draw `UISkillPane::Draw` while
//! the window's reclaim flag `UISkillPane.reclamationMode` is set), [`Flags`]:
//! * disabled (no click) = [`Flags::disabled`]: unlearned (f11), the cost above
//!   the money (f8, the cost at the current count), removing the last point
//!   would orphan a dependant (f10, [`orphans`]) or a celestial power is bound
//!   to it at level 1 (f7: `HasAutocastSkill` without `HasAutocastInDbr`, the
//!   autocast's operation 3, a devotion binding); a mastery also while a
//!   learned skill of its tree needs its level (f13, [`mastery_needed`]: the
//!   skill's tier level `>=` the mastery level) and at level 1 (the click
//!   callback `UISkillPane::OnButtonClick` refuses it as well);
//! * grey (grayscale shader, still clickable) = [`Flags::grayed`]: mastery too
//!   low for the tier (f1), base skill unlearned (f2), mastery slots full (f4),
//!   dependency unmet (f5);
//! * frame: red when f11 / f10 / f7 / f8, else green (none for the mastery,
//!   no blue maxed frame in this mode).
//!
//! Tooltip (`GameEngine::GenerateUISkillText` with the reclaim flag
//! `a5`): the next-level block is left out, then after a blank line the first
//! of [`Flags::tip`] (class 52; `tagReclaimPoint` class 51), nothing while f1.
use crate::skills::{ClassTree, SkillBook, SkillTrees, profile_record};
use gd_formats::vfs::Database;

/// The player record holding the price fields.
pub const PLAYER_RECORD: &str = "records/creatures/pc/malepc01.dbr";

/// The money cap of a refund paid back (`UseReclamationPoints`).
pub const MONEY_CAP: u64 = 2_000_000_000;

/// The cost of a refund at refund count `count` (`GetCurrentSkillReclamationCost`).
pub fn cost(tiers: &[u32], costs: &[u32], count: u32) -> u64 {
    let n = tiers.len();
    let mut i = 0;
    while i + 1 < n {
        if count >= tiers[i] && count < tiers[i + 1] {
            break;
        }
        i += 1;
    }
    costs.get(i).copied().unwrap_or(0) as u64
}

/// The player record's skill reclamation price table: (tiers, costs).
pub fn table(db: &Database) -> (Vec<u32>, Vec<u32>) {
    let Some(r) = db.get(PLAYER_RECORD) else {
        return (Vec::new(), Vec::new());
    };
    let ints = |k: &str| {
        r.f32s(k)
            .into_iter()
            .map(|v| v.max(0.0) as u32)
            .collect::<Vec<_>>()
    };
    (ints("reclamationPointTiers"), ints("reclamationPointCosts"))
}

/// The cost of the next refund after `count` refunds.
pub fn current_cost(db: &Database, count: u32) -> u64 {
    let (t, c) = table(db);
    cost(&t, &c, count)
}

/// Money paid for refunding `n` points from `count` (count after: `count + n`).
pub fn charge(db: &Database, count: u32, n: u32) -> u64 {
    let (t, c) = table(db);
    (0..n).map(|k| cost(&t, &c, count + k)).sum()
}

/// Money paid back for undoing `n` refunds from `count` (count after: `count - n`).
pub fn payback(db: &Database, count: u32, n: u32) -> u64 {
    let (t, c) = table(db);
    (1..=n.min(count)).map(|k| cost(&t, &c, count - k)).sum()
}

/// Money text: `%i` with a comma before each group of three digits (Game.dll `Game::FormatMoney`, the
/// tooltip's cost; the window's numbers use GrimDawn.exe's copy `UI::FormatThousands`).
pub fn money(v: u64) -> String {
    let d = v.to_string();
    let n = d.len();
    d.chars().enumerate().fold(String::new(), |mut s, (i, c)| {
        if i > 0 && (n - i) % 3 == 0 {
            s.push(',');
        }
        s.push(c);
        s
    })
}

fn dependencies(db: &Database, skill: &str) -> (Vec<String>, bool) {
    let Some(rec) = profile_record(db, skill).or_else(|| db.get(skill).cloned()) else {
        return (Vec::new(), false);
    };
    (
        rec.strs("skillDependancy")
            .into_iter()
            .map(str::to_owned)
            .collect(),
        rec.bool("skillDependancyAll"),
    )
}

/// f10, `UISkillPane::WouldOrphanDependant`: taking the last point of `skill` would leave a dependant without it.
/// Only at level 1.
/// The tree's learned skills listing it in `skillDependancy`: an all-mode list blocks; an any-mode list blocks
/// unless another listed skill is learned, and then the scan of further dependants stops (engine order). Then the
/// learned modifiers / secondary skills of `skill` (tree entries whose base it is, [`ClassTree::bases`]) block, unless
/// that stop happened and the child has a dependency list of its own.
pub fn orphans(db: &Database, book: &SkillBook, tree: &ClassTree, skill: &str) -> bool {
    if book.level(skill) != 1 {
        return false;
    }
    let mut alternative = false;
    for x in &tree.skills {
        if x.eq_ignore_ascii_case(skill) || book.level(x) == 0 {
            continue;
        }
        let (deps, all) = dependencies(db, x);
        if !deps.iter().any(|d| d.eq_ignore_ascii_case(skill)) {
            continue;
        }
        if all {
            return true;
        }
        if deps
            .iter()
            .any(|d| !d.eq_ignore_ascii_case(skill) && book.level(d) > 0)
        {
            alternative = true;
            break;
        }
        return true;
    }
    tree.skills
        .iter()
        .zip(&tree.bases)
        .filter(|&(c, b)| {
            b.as_deref().is_some_and(|b| b.eq_ignore_ascii_case(skill)) && book.level(c) > 0
        })
        .any(|(c, _)| !alternative || dependencies(db, c).0.is_empty())
}

/// f13, `UISkillPane::IsMasteryNeeded`: some learned skill of the mastery's tree needs the mastery's level (`tree`
/// one of
/// `trees`).
pub fn mastery_needed(
    db: &Database,
    book: &SkillBook,
    trees: &SkillTrees,
    tree: &ClassTree,
    mastery: &str,
) -> bool {
    let level = book.level(mastery);
    tree.skills
        .iter()
        .skip(1)
        .any(|s| book.level(s) > 0 && trees.tier_requirement(db, s) >= level)
}

/// A skill button's reclaim flags (`UISkillPane::GetRequirementFlags`, module docs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub mastery: bool,
    pub level: u32,
    /// f1, f2, f4, f5.
    pub mastery_low: bool,
    pub base_disabled: bool,
    pub masteries_full: bool,
    pub dependency: bool,
    /// f7, f8, f10, f11, f13.
    pub devotion: bool,
    pub no_gold: bool,
    pub orphans: bool,
    pub unlearned: bool,
    pub mastery_needed: bool,
}

/// A reclaim tooltip line (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    /// `tagReclaimMastery`.
    MasteryNeeded,
    /// `tagDecreaseMasteryError`.
    MasteryLast,
    /// `tagReclaimNoPoints`.
    NoPoints,
    /// `tagReclaimNoGold` with the cost.
    NoGold,
    /// `tagReclaimBase`.
    Base,
    /// `tagReclaimDevotion`.
    Devotion,
    /// `tagReclaimPoint` with the cost.
    Point,
}

impl Flags {
    /// The button takes no click.
    pub fn disabled(&self) -> bool {
        let any = self.unlearned || self.no_gold || self.orphans || self.devotion;
        any || (self.mastery && (self.mastery_needed || self.level == 1))
    }

    /// The icon is drawn grey.
    pub fn grayed(&self) -> bool {
        self.mastery_low || self.masteries_full || self.base_disabled || self.dependency
    }

    /// The red frame (else green; masteries have none).
    pub fn red(&self) -> bool {
        self.unlearned || self.orphans || self.devotion || self.no_gold
    }

    /// The tooltip's reclaim line, if any.
    pub fn tip(&self) -> Option<Tip> {
        Some(if self.mastery_low {
            return None;
        } else if self.mastery && self.mastery_needed {
            Tip::MasteryNeeded
        } else if self.mastery && self.level == 1 {
            Tip::MasteryLast
        } else if self.unlearned {
            Tip::NoPoints
        } else if self.no_gold {
            Tip::NoGold
        } else if self.orphans {
            Tip::Base
        } else if self.devotion {
            Tip::Devotion
        } else {
            Tip::Point
        })
    }
}

/// The flags of `skill` in `tree` (one of `trees`) for a character with `gold`, `count` refunds made,
/// `masteries_full` (allowed <= active) and `bound` (a celestial power is bound to the skill).
#[allow(clippy::too_many_arguments)]
pub fn flags(
    db: &Database,
    book: &SkillBook,
    trees: &SkillTrees,
    tree: &ClassTree,
    skill: &str,
    gold: u64,
    count: u32,
    masteries_full: bool,
    bound: bool,
) -> Flags {
    let mastery = tree
        .mastery()
        .is_some_and(|m| m.eq_ignore_ascii_case(skill));
    let level = book.level(skill);
    let mut f = Flags {
        mastery,
        level,
        unlearned: level == 0,
        no_gold: current_cost(db, count) > gold,
        devotion: level == 1 && bound,
        ..Default::default()
    };
    if mastery {
        f.masteries_full = level == 0 && masteries_full;
        f.mastery_needed = mastery_needed(db, book, trees, tree, skill);
    } else {
        let m = tree.mastery().map_or(0, |m| book.level(m));
        f.mastery_low = m < trees.tier_requirement(db, skill);
        let mut bases = tree.bases_of(skill).peekable();
        f.base_disabled = bases.peek().is_some() && bases.all(|b| book.level(b) == 0);
        f.dependency = book.dependency_unmet(db, skill);
        f.orphans = orphans(db, book, tree, skill);
    }
    f
}
