//! The player's experience and gold this session.
use bevy::prelude::*;
use gd_sim::progress::Progress;

/// The player gained levels (profile rebuilt at `level` with full life; the game refills energy).
#[derive(Message, Clone, Debug)]
pub struct LeveledUp {
    pub player: Entity,
    pub level: i32,
    pub gained: u32,
}

/// The player was granted unspent points (events 49 / 50); several grants of one frame / level-up are one message.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointsGained {
    pub skill: bool,
    pub attribute: bool,
}

#[derive(Resource)]
pub struct PlayerProgress {
    pub progress: Progress,
    pub gold: u64,
}

impl PlayerProgress {
    /// The loaded character's progress, or a new character's.
    pub fn from_save(loaded: Option<&crate::session::LoadedCharacter>) -> Self {
        let (level, xp, gold, attr_pts, skill_pts) = match loaded {
            Some(c) => (
                c.0.bio.level as i32,
                c.0.bio.experience as u64,
                c.0.info.money as u64,
                c.0.bio.attribute_points as i32,
                c.0.bio.skill_points as i32,
            ),
            None => (1, 0, 0, 0, 0),
        };
        PlayerProgress {
            progress: Progress {
                level: level.max(1),
                experience: xp,
                attribute_points: attr_pts,
                skill_points: skill_pts,
            },
            gold,
        }
    }
}
