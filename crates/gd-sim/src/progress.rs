//! Experience, levelling and gold drops (Game.dll).
//!
//! - `Monster::GetExperienceReward` → `GameEngine::CalculateExperienceReward`:
//!   equation from `records/game/experienceformulas.dbr` by
//!   difficulty (`experienceEquation`, `eliteExperienceEquation`,
//!   `ultimateExperienceEquation`; challenge mode `challengeExperienceEquation`),
//!   variables `monsterLevel`, `monsterExperience`, `averagePartyLevel`;
//!   reward `(int)(max(v, 0) + 0.5)`. Only monsters with `giveXP`.
//! - `Character::ReceiveExperience`: `+ (int)(bonus% * 0.01 * xp + 0.5)`
//!   (CharAttr 55), then `ExperienceLevelControl::AddExperience`: the total is cumulative; while
//!   `level < maxLevel` and `total >= (int)(eq(playerLevel = level) + 0.5)`,
//!   level up, `+characterModifierPoints` attribute points and
//!   `skillModifierPoints[min(newLevel - 2, len - 1)]` skill points.
//! - `GoldGenerator::RunGenerator`: with chance `goldGeneratorChance`
//!   (default 100, ≤ 0 = always), amount `IGenerate` between the two
//!   `goldAmountEquation`s (each `(int)(v + 0.5)` clamped to
//!   `[goldValueMin, goldValueMax]`), split by `GoldGenerator::SplitGold` into
//!   `rand[goldSplitMin, goldSplitMax]` piles of at least `goldSplitSizeMin`.
//! - [`class_tag`]: the class name tag (`Player::GetClassTag`).
use crate::stats::LevelTable;
use gd_formats::vfs::Database;
use gd_rules::Rng;
use gd_rules::expr::Expr;

fn irange(rng: &mut Rng, lo: i32, hi: i32) -> i32 {
    if hi <= lo {
        return lo;
    }
    // Span in i64: `goldValueMax` defaults to i32::MAX, so an i32 span can overflow.
    let span = hi as i64 - lo as i64 + 1;
    (lo as i64 + rng.next() as i64 % span) as i32
}

/// Experience a monster gives a player of `player_level`.
pub fn experience_reward(
    db: &Database,
    monster: &str,
    monster_level: i32,
    player_level: i32,
    mode: gd_rules::mode::Mode,
) -> u32 {
    let difficulty = mode.difficulty;
    let Some(rec) = db.get(monster) else { return 0 };
    if !rec.bool("giveXP") {
        return 0;
    }
    let Some(f) = db.get("records/game/experienceformulas.dbr") else {
        return 0;
    };
    // Veteran on Normal: `challengeExperienceEquation`
    // (`GameEngine::CalculateExperienceReward`).
    let key = if mode.veteran() {
        "challengeExperienceEquation"
    } else {
        [
            "experienceEquation",
            "eliteExperienceEquation",
            "ultimateExperienceEquation",
        ][difficulty.min(2)]
    };
    let Some(eq) = f
        .str(key)
        .or(f.str("experienceEquation"))
        .and_then(|s| Expr::parse(s).ok())
    else {
        return 0;
    };
    let mexp = rec.f32("experiencePoints").unwrap_or(0.0);
    let v = eq.eval(&|n| match n.to_ascii_lowercase().as_str() {
        "monsterlevel" => Some(monster_level as f32),
        "monsterexperience" => Some(mexp),
        "averagepartylevel" => Some(player_level as f32),
        _ => None,
    });
    (v.max(0.0) + 0.5) as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub level: i32,
    /// Cumulative experience.
    pub experience: u64,
    pub attribute_points: i32,
    pub skill_points: i32,
}

impl Progress {
    /// Total experience needed to leave `level`.
    pub fn threshold(levels: &LevelTable, level: i32) -> u64 {
        let pl = level as f32;
        let v = levels
            .experience_equation
            .eval(&|n| n.eq_ignore_ascii_case("playerLevel").then_some(pl));
        (v + 0.5).max(0.0) as u64
    }

    /// Adds experience (after the bonus %); returns the levels gained.
    pub fn add_experience(&mut self, levels: &LevelTable, xp: u32, bonus_pct: f32) -> i32 {
        let mut xp = xp as i64;
        if bonus_pct != 0.0 {
            xp += (bonus_pct * 0.01 * xp as f32 + 0.5) as i64;
        }
        self.grant(levels, xp.max(0) as u64)
    }

    /// Adds raw experience and applies the level-ups; returns the levels gained.
    fn grant(&mut self, levels: &LevelTable, xp: u64) -> i32 {
        self.experience = self.experience.saturating_add(xp);
        let mut gained = 0;
        while self.level < levels.max_level
            && self.experience >= Self::threshold(levels, self.level)
        {
            self.level += 1;
            gained += 1;
            self.attribute_points += levels.attribute_points_per_level;
            if !levels.skill_points.is_empty() {
                let i = ((self.level - 2).max(0) as usize).min(levels.skill_points.len() - 1);
                self.skill_points = (self.skill_points + levels.skill_points[i]).max(0);
            }
        }
        gained
    }

    /// `ScriptableAction_GiveLevel::Execute`: `n` times, experience is raised
    /// to the next level's threshold. Returns the levels gained.
    pub fn give_levels(&mut self, levels: &LevelTable, n: u32) -> i32 {
        let mut gained = 0;
        for _ in 0..n {
            let need = Self::threshold(levels, self.level).saturating_sub(self.experience);
            let g = self.grant(levels, need);
            if g == 0 {
                // Max level: the remaining iterations change nothing.
                break;
            }
            gained += g;
        }
        gained
    }

    /// Fraction of the way from this level's start to the next (HUD bar).
    pub fn fraction(&self, levels: &LevelTable) -> f32 {
        let lo = if self.level <= 1 {
            0
        } else {
            Self::threshold(levels, self.level - 1)
        };
        let hi = Self::threshold(levels, self.level);
        if hi <= lo {
            1.0
        } else {
            ((self.experience.saturating_sub(lo)) as f32 / (hi - lo) as f32).clamp(0.0, 1.0)
        }
    }
}

/// Gold piles dropped by a generator record for a monster of `level`.
pub fn gold_drop(
    db: &Database,
    generator: &str,
    chance: f32,
    level: i32,
    rng: &mut Rng,
) -> Vec<u32> {
    let Some(g) = db.get(generator) else {
        return Vec::new();
    };
    if chance > 0.0 && rng.unit() * 100.0 > chance {
        return Vec::new();
    }
    let lvl = level as f32;
    let vars = |n: &str| match n.to_ascii_lowercase().as_str() {
        "generatorlevel" => Some(lvl),
        "numberofplayers" => Some(1.0),
        "averageplayerlevel" | "currentplayerlevel" | "minplayerlevel" | "maxplayerlevel" => {
            Some(lvl)
        }
        _ => Some(0.0),
    };
    let (vmin, vmax) = (
        g.i32("goldValueMin").unwrap_or(0),
        g.i32("goldValueMax").unwrap_or(i32::MAX),
    );
    let amount = |k: &str| {
        g.str(k)
            .and_then(|s| Expr::parse(s).ok())
            .map(|e| (e.eval(&vars) + 0.5) as i32)
    };
    let a = amount("goldAmountEquation").unwrap_or(0);
    let b = amount("goldAmountEquation2").unwrap_or(a);
    let (a, b) = (a.max(vmin).min(vmax), b.max(vmin).min(vmax));
    let mut total = irange(rng, a.min(b), a.max(b));
    let (smin, smax) = (
        g.i32("goldSplitMin").unwrap_or(1),
        g.i32("goldSplitMax").unwrap_or(1),
    );
    let size_min = g.i32("goldSplitSizeMin").unwrap_or(1).max(1);
    let piles = irange(rng, smin, smax) - 1;
    let mut out = Vec::new();
    let mut n = 0;
    while total > 0 && n < piles {
        let part = if total > size_min.saturating_mul(2) {
            irange(rng, size_min, total - size_min)
        } else {
            total
        };
        total -= part;
        out.push(part as u32);
        n += 1;
    }
    if total > 0 {
        out.push(total as u32);
    }
    out
}

/// The character's class name tag (`Player::GetClassTag`): the masteries with a
/// point (`SkillManager::GetSkillMasteries`, each `Skill_Mastery`'s `MasteryEnumeration`, 0-based), looked
/// up in the table `GameEngine::FillClassArray` fills: one mastery `tagSkillClassName<NN>`, two
/// `tagSkillClassName<NN><MM>` (the table is symmetric, the lower number first; NN = enumeration + 1); none
/// an empty tag.
pub fn class_tag(db: &Database, book: &crate::skills::SkillBook) -> String {
    let masteries: Vec<u32> = crate::skills::class_trees(db)
        .iter()
        .filter_map(|(_, tree)| {
            let skills = crate::skills::tree_skills(db, tree);
            (book.level(skills.first()?) > 0)
                .then(|| crate::skills::tree_enumeration(db, &skills))
                .flatten()
        })
        .collect();
    class_tag_of(&masteries)
}

/// [`class_tag`] of the masteries' enumerations (`GameEngine::GetClassTag(a)` / `GetClassTag(a, b)`).
pub fn class_tag_of(masteries: &[u32]) -> String {
    match *masteries {
        [] => String::new(),
        [a] => format!("tagSkillClassName{:02}", a + 1),
        [a, b, ..] if a == b => format!("tagSkillClassName{:02}", a + 1),
        [a, b, ..] => format!("tagSkillClassName{:02}{:02}", a.min(b) + 1, a.max(b) + 1),
    }
}
