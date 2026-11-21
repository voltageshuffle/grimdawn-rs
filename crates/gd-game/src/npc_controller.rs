//! An NPC's controller out of conversation: `GAME::ControllerNpc2`'s Idle ↔ LongIdle cycle (Game.dll).
//! It only picks the character's [`IdleMode`]; `crate::character` plays it.
//!
//! - Every `Npc*`-class character gets the controller: `Npc::Load` @0x1803b9ab0 reads `controller`, default
//!   [`DEFAULT_CONTROLLER`] (stock: only gdx3 `areah_blacksmithb302` names one, `ai_npc_idleloop_01`).
//!   `ControllerNpc2::Load` @0x180153710 reads `npcIdleTimeMin` / `npcIdleTimeMax` (s, `(int)(v * 1000)` ms, default 0;
//!   default02: 5 / 10) and `npcIdleLoop` (default 0) from that record.
//! - Idle (`ControllerNpcStateIdle::OnBegin` @0x1801597a0): a new `IdleAction` ([`IdleMode::Combat`]) and a timer of
//!   min + rand() % (max − min + 1) ms (just min when the unsigned difference is above 0x7FFD). `OnUpdate` @0x180159830:
//!   the timer counts down; at ≤ 0 an NPC whose current animation set has no `Walk` clip goes to LongIdle, one with
//!   it to Wander, whose `OnBegin` @0x180159fc0 goes straight back to Idle when there are no wander points (a new
//!   timer; the combat idle goes on).
//! - LongIdle (`ControllerNpcStateLongIdle::OnBegin` @0x180159c00 / `OnUpdate` @0x180159ce0): `LongIdleAction`
//!   ([`IdleMode::Long`]: transition, loop, fidgets) and the same timer roll; at ≤ 0 back to Idle unless
//!   `npcIdleLoop`.
//! - Chat (both states' `OnUpdate` → "Chat" while the NPC has a partner; `crate::character::npc_chat`) suspends the
//!   cycle; its end (`ControllerNpcStateChat::OnUpdate` @0x180159eb0) re-enters Idle with a new timer.
//! - PORT: the timers use `crate::rng`, not the CRT `rand()` (only the draw order differs). The Walk NPC's re-entry
//!   into Idle keeps the playing `AttackIdle` instead of starting a fresh `IdleAction` (the same clip either way).
//!   Leaving the idle states any other way (a quest `ControllerState` animation) re-enters Idle when the character
//!   idles again (the quest states' ends are not traced).
//! - Startup ([`attach_npc_controllers`]): the Spawn animation once when the spawn animation is enabled (a script's
//!   `Character.Create`: the Shattered Realm keeper swapped in when a run starts or ends plays its activate /
//!   deactivate animation), else Idle at once.
//! - PORT: not run, because stock data never reaches them: Wander and WatchAttraction (`MoveToCurrentWanderPoint`
//!   @0x18015a680, `FindAttraction`
//!   @0x18015aaa0: no `NpcWanderPoint` is placed in any map and no NPC is an `isNpcAttractor`), Alert
//!   (`ControllerNpcStateAlert` exists but `ControllerNpc2::RegisterStates` @0x180153ae0 does not register it). So an
//!   NPC never walks or turns on its own.
use crate::character::{CharState, Character, IdleMode};
use crate::data::GameData;
use bevy::prelude::*;
use gd_sim::anim::AnimType;

/// The controller record of an `Npc` without `controller` (`Npc::Load`).
pub const DEFAULT_CONTROLLER: &str = "records/controllers/npc/ai_npcdefault02.dbr";

/// Which `ControllerNpc2` state the NPC is in (module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcState {
    Idle,
    LongIdle,
    /// Chat or any non-idle character state: the cycle waits; Idle starts again after it.
    Away,
}

/// `ControllerNpc2` (module doc): the controller record's idle fields and the current state's timer.
#[derive(Component, Debug, Clone)]
pub struct NpcController {
    /// `npcIdleTimeMin` / `npcIdleTimeMax` (ms).
    pub idle_ms: (i32, i32),
    /// `npcIdleLoop`: LongIdle never leaves.
    pub idle_loop: bool,
    pub state: NpcState,
    /// Ms left of the state's timer.
    pub left_ms: f32,
}

impl NpcController {
    /// The controller of record `controller` (missing fields 0, as `ControllerNpc2::Load` reads them).
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

    /// The idle timer roll of Idle / LongIdle `OnBegin`.
    fn roll(&self) -> f32 {
        let (min, max) = self.idle_ms;
        let d = max.wrapping_sub(min) as u32;
        let ms = if d > 0x7FFD {
            min
        } else {
            min + crate::rng::u32(0..=d) as i32
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

/// `Npc::Load`: an `Npc*`-class character gets its controller. Its Startup state
/// (`ControllerNpcStateStartup::OnBegin` @0x180159420): with `GetSpawnAnimationEnabled` (an NPC a script created with
/// `Character.Create`, `gd_sim::story::state::Entities::spawn_animation`) it plays the Spawn animation (type 19) once,
/// then Idle; else Idle at once.
pub fn attach_npc_controllers(
    mut commands: Commands,
    data: Res<GameData>,
    rt: Option<NonSend<crate::story::StoryRuntime>>,
    mut new: Query<
        (Entity, &mut Character, Option<&crate::story::StoryActor>),
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

/// The Idle ↔ LongIdle cycle (module doc).
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
