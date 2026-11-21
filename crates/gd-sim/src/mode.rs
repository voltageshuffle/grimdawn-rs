//! The session's game mode (`GameInfo`): difficulty and challenge.

/// Difficulty 0..=2 (Normal, Elite, Ultimate) and challenge
/// (`GameInfo::GetChallenge`: 0 none, 1 Veteran on Normal, 3 Ultimate
/// Veteran on Ultimate).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mode {
    pub difficulty: usize,
    pub challenge: u32,
    /// The Crucible's difficulty ramp (`Game.SetSurvivalDifficulty`, `GameEngine::GetDifficultyRamp`): `Some` in a
    /// Crucible session, where the challenge adjustment is the survival one and its entries are read at the ramp.
    pub ramp: Option<u32>,
}

impl Mode {
    /// The menu's choice: `veteran` is Veteran on Normal, Ultimate Veteran
    /// on Ultimate (`IsUltimateVeteran`) and nothing on Elite.
    pub fn new(difficulty: usize, veteran: bool) -> Self {
        let challenge = match difficulty {
            0 if veteran => 1,
            2 if veteran => 3,
            _ => 0,
        };
        Self {
            difficulty: difficulty.min(2),
            challenge,
            ramp: None,
        }
    }

    /// Veteran mode (Normal with the challenge flag, `GameEngine::SetDifficultyRamp`).
    pub fn veteran(self) -> bool {
        self.difficulty == 0 && self.challenge == 1
    }

    /// Ultimate Veteran: challenge 3 on Ultimate (`GameEngine::IsUltimateVeteran`
    /// @0x1802e90e0): monsters get the challenge stats of
    /// `gameascendant.dbr` `ultimateChallangeAdjustment`
    /// (`actor::Rules::with_veteran`; proxy pools take its spawn offsets on all
    /// of Ultimate, see [`crate::proxy::run_pool`]), containers take
    /// `lootTable[3]` (`sub_180358340`) and shrines `ascendantLootTable`
    /// (`StaticShrine::Load`).
    pub fn ultimate_veteran(self) -> bool {
        self.difficulty == 2 && self.challenge == 3
    }

    /// The difficulty index of the Ascendant tables and the endless ruleset
    /// slots: the difficulty (0..=2, clamped), +1 on Ultimate Veteran.
    pub fn ascendant_index(self) -> usize {
        self.difficulty.min(2) + self.ultimate_veteran() as usize
    }

    /// The tags of the HUD's game-mode label at the upper left
    /// (`hudGameModeText`), joined with spaces. Grim Dawn.exe
    /// sub_140218AE0: `tagHardcore` for hardcore, then challenge 3 →
    /// `tagChallengeDifficulty03`, another challenge → `tagChallengeDifficulty`
    /// ("Veteran"), else Elite / Ultimate → `tagRDifficultyTitle02` / `03`
    /// (the campaign's titles; `GameInfo::GetMode` 1 uses the `tagS…` ones);
    /// plain Normal shows nothing. The Crucible (`GameInfo::GetMode() == 1`, `survival`) titles its
    /// difficulties `tagSDifficultyTitle02` ("Challenger") / `03` ("Gladiator").
    /// At most two tags; unused slots are `""` (no allocation for the HUD's per-change label).
    pub fn label_tags_in(self, hardcore: bool, survival: bool) -> [&'static str; 2] {
        let first = if hardcore { "tagHardcore" } else { "" };
        let second = match (self.challenge, self.difficulty) {
            (3, _) => "tagChallengeDifficulty03",
            (c, _) if c != 0 => "tagChallengeDifficulty",
            (_, 1) if survival => "tagSDifficultyTitle02",
            (_, 2) if survival => "tagSDifficultyTitle03",
            (_, 1) => "tagRDifficultyTitle02",
            (_, 2) => "tagRDifficultyTitle03",
            _ => "",
        };
        if first.is_empty() {
            [second, ""]
        } else {
            [first, second]
        }
    }
}
