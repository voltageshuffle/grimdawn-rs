//! An NPC's controller out of conversation: `GAME::ControllerNpc2`'s Idle ↔ LongIdle cycle (Game.dll).
//! It only picks the character's [`IdleMode`]; `crate::character` plays it.
//!
//! - Every `Npc*`-class character gets the controller: `Npc::Load` reads `controller`, default
//!   [`DEFAULT_CONTROLLER`]. `ControllerNpc2::Load` reads `npcIdleTimeMin` / `npcIdleTimeMax` (s,
//!   `(int)(v * 1000)` ms, default 0) and `npcIdleLoop` (default 0) from that record.
//! - Idle (`ControllerNpcStateIdle::OnBegin`): a new `IdleAction` ([`IdleMode::Combat`]) and a timer of
//!   min + rand() % (max − min + 1) ms (just min when the unsigned difference is above 0x7FFD). At ≤ 0 an
//!   NPC whose animation set has no `Walk` clip goes to LongIdle, one with it to Wander, which goes
//!   straight back to Idle without wander points.
//! - LongIdle (`ControllerNpcStateLongIdle`): `LongIdleAction` ([`IdleMode::Long`]) and the same timer
//!   roll; at ≤ 0 back to Idle unless `npcIdleLoop`.
//! - Chat (`crate::character::npc_chat`) suspends the cycle; its end (`ControllerNpcStateChat::OnUpdate`)
//!   re-enters Idle with a new timer.
//! - PORT: the Walk NPC's re-entry into Idle keeps the playing `AttackIdle` (the same clip). Leaving the
//!   idle states any other way (a quest `ControllerState` animation) re-enters Idle when the character
//!   idles again (the quest states' ends are not traced).
//! - Startup ([`attach_npc_controllers`]): the Spawn animation once when enabled, else Idle at once.
//! - PORT: Wander, WatchAttraction (no `NpcWanderPoint` / `isNpcAttractor` in stock data) and Alert (not
//!   registered by `ControllerNpc2::RegisterStates`) are not ported: an NPC never walks or turns on its own.
use bevy::prelude::*;
use gd_game_core::character::{CharState, Character, IdleMode};
use gd_game_core::data::GameData;
use gd_sim::anim::AnimType;

/// `Npc::Load`'s default.
pub const DEFAULT_CONTROLLER: &str = "records/controllers/npc/ai_npcdefault02.dbr";

/// Which `ControllerNpc2` state the NPC is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcState {
    Idle,
    LongIdle,
    /// Chat or any non-idle character state: Idle starts again after it.
    Away,
}

/// `ControllerNpc2`: the controller record's idle fields and the current state's timer.
#[derive(Component, Debug, Clone)]
pub struct NpcController {
    /// `npcIdleTimeMin` / `npcIdleTimeMax` (ms).
    pub idle_ms: (i32, i32),
    /// `npcIdleLoop`: LongIdle never leaves.
    pub idle_loop: bool,
    pub state: NpcState,
    pub left_ms: f32,
}

impl NpcController {
    /// Missing fields 0, as `ControllerNpc2::Load` reads them.
    pub fn load(data: &GameData, controller: &str) -> Self {
        let rec = data.db.get(controller);
        let f = |k: &str| rec.as_ref().and_then(|r| r.f32(k)).unwrap_or(0.0);
        NpcController {
            idle_ms: (
                (f("npcIdleTimeMin") * 1000.0) as i32,
                (f("npcIdleTimeMax") * 1000.0) as i32,
            ),
            idle_loop: f("npcIdleLoop") != 0.0,
            state: NpcState::Away,
            left_ms: 0.0,
        }
    }

    fn roll(&self) -> f32 {
        let (min, max) = self.idle_ms;
        let d = max.wrapping_sub(min) as u32;
        let ms = if d > 0x7FFD {
            min
        } else {
            min + gd_game_core::rng::u32(0..=d) as i32
        };
        ms as f32
    }

    fn enter(&mut self, state: NpcState, c: &mut Character) {
        self.state = state;
        self.left_ms = self.roll();
        c.idle = if state == NpcState::LongIdle {
            IdleMode::Long
        } else {
            IdleMode::Combat
        };
    }
}

/// `Npc::Load`: an `Npc*`-class character gets its controller. `ControllerNpcStateStartup::OnBegin`: with
/// `GetSpawnAnimationEnabled` it plays the Spawn animation (type 19) once, then Idle; else Idle at once.
pub fn attach_npc_controllers(
    mut commands: Commands,
    data: Res<GameData>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut new: Query<
        (
            Entity,
            &mut Character,
            Option<&gd_game_core::story::StoryActor>,
        ),
        (Added<Character>, Without<NpcController>),
    >,
) {
    for (e, mut c, actor) in &mut new {
        let Some(rec) = data
            .db
            .get(&c.record)
            .filter(|r| r.class.starts_with("Npc"))
        else {
            continue;
        };
        if c.is_player {
            continue;
        }
        let spawn_anim = actor
            .zip(rt.as_ref())
            .is_some_and(|(a, rt)| rt.story.borrow().entities.spawn_animation.contains(&a.0));
        if spawn_anim && c.play_scripted(AnimType::Spawn) {
            debug!("{}: npc spawn animation", c.record);
        }
        let ctl = rec.str("controller").unwrap_or(DEFAULT_CONTROLLER);
        commands
            .entity(e)
            .try_insert(NpcController::load(&data, ctl));
    }
}

pub fn npc_idle(time: Res<Time>, mut npcs: Query<(&mut NpcController, &mut Character)>) {
    let dt = time.delta_secs() * 1000.0;
    for (mut n, mut c) in &mut npcs {
        let (n, c) = (&mut *n, &mut *c);
        if c.idle == IdleMode::Chat || c.state != CharState::Idle {
            n.state = NpcState::Away;
            continue;
        }
        if n.state == NpcState::Away {
            n.enter(NpcState::Idle, c);
        }
        n.left_ms -= dt;
        if n.left_ms > 0.0 {
            continue;
        }
        match n.state {
            NpcState::Idle if c.anims.has(AnimType::Walk) => n.enter(NpcState::Idle, c),
            NpcState::Idle => {
                debug!("{}: npc long idle", c.record);
                n.enter(NpcState::LongIdle, c);
            }
            NpcState::LongIdle if !n.idle_loop => n.enter(NpcState::Idle, c),
            _ => {}
        }
    }
}
