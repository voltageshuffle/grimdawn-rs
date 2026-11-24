//! The life of a channelled beam (`Skill_AttackSpellBeam`: Aether Ray): one `BeamInfo` node of the skill's list, a
//! fan of `beamNumber` rays sharing one state.
//!
//! Game.dll `Skill_AttackSpellBeam::Update`, `UpdateBeams`, `StartEndEffects`, `StopEmitting`, `ActivateNow`,
//! `GetTargets`. States: 0
//! place (the start effect goes to the emit point and starts, the end and ground
//! effects are placed idle), 1 delay (`beamDelay`, the main player only), 2 active, 3 stopping, 4 retract, 5
//! destroy. After the delay the ribbons are added to the world (state 2, or 3 when the stop came during the delay).
//! Active (the node of the held cast only): each ray's length grows `travelSpeed` m/s up to the longest it may be
//! (`GetMaxBeamLength`), and once it is there (clamped: "hit") its end effect emits, else it stops. Stopping (the
//! channel ended): the start effect stops; then the rays retract from the caster's end at the same speed (the far
//! ends stay) until the retract reaches the first ray's length, and the node is destroyed (the end and ground
//! effects stop and remove themselves). The ground effect emits while the ray meets a surface (the caller's test).
//!
//! The fan (`UpdateBeams`): the ray count and spread are the skill profile's `beamNumber` (0 = 1) and `beamAngle`
//! (degrees), each plus the learned modifiers' (`Skill::GetSkillModifierData`; the profile loader
//! `SkillProfile_Modifiers::Load` reads both per level; stock: Aether Ray's 1 ray, the gdx `*_aetherray` item modifiers
//! +1 ray, +25 degrees). Ray
//! k turns about the up axis by `-spread / 2 + k * step`, `step = spread / (n - 1)` (`spread / n` from 360 degrees
//! on), so the fan is symmetric about the heading and a lone ray points straight ([`fan_angles`]).
//!
//! Damage (`Update`, states 2 and 4): the node's attack timer starts at `timeBetweenAttacks` / cast speed ms
//! (`ActivateNow`) and counts down only while the ribbons are in the world; at 0 it is re-armed and every ray's
//! ribbon is a target ray (`GetTargets`: from the ribbon's start, its drawn length + 1.5 m, `beamThickness` wide),
//! so the hits reach only as far as the drawn beam has grown ([`SpellBeam::attack_due`]); the held cast's node then
//! pays the energy.

/// `BeamInfo` state (`BeamInfo.state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Delay,
    Active,
    Retract,
    Done,
}

/// One node of a channelled beam skill: its rays share the state, the delay and the retract.
#[derive(Clone, Debug)]
pub struct SpellBeam {
    pub state: State,
    delay: f32,
    /// The stop was asked (`BeamInfo.stopRequested`).
    ending: bool,
    /// Per ray: metres from the emit point to the far end.
    pub lens: Vec<f32>,
    /// Metres from the emit point to the near end (grows only while retracting).
    pub retract: f32,
    /// Per ray: the end effect is emitting (the "end effect started" bits).
    pub end_on: Vec<bool>,
    /// Seconds to the next attack (`BeamInfo.attackTimer`).
    attack: f32,
}

/// The fan's ray directions as turns about the up axis (radians), ray 0 first (`UpdateBeams`).
pub fn fan_angles(n: usize, spread_deg: f32) -> Vec<f32> {
    if n <= 1 {
        return vec![0.0; n];
    }
    let gaps = if spread_deg < 360.0 { n - 1 } else { n };
    let step = spread_deg / gaps as f32;
    (0..n)
        .map(|k| (-spread_deg * 0.5 + k as f32 * step).to_radians())
        .collect()
}

impl SpellBeam {
    /// A node of `rays` rays placed this frame (state 0 done), with `delay` seconds before it shows and its first
    /// attack `period` seconds after that.
    pub fn new(delay: f32, rays: usize, period: f32) -> Self {
        let n = rays.max(1);
        Self {
            state: State::Delay,
            delay: delay.max(0.0),
            ending: false,
            lens: vec![0.0; n],
            retract: 0.0,
            end_on: vec![false; n],
            attack: period,
        }
    }

    /// `StopEmitting`: the channel ended.
    pub fn stop(&mut self) {
        self.ending = true;
        if self.state == State::Active {
            self.state = State::Retract;
        }
    }

    /// The channel ended (the node no longer grows).
    pub fn stopped(&self) -> bool {
        self.ending
    }

    /// The ribbons are in the world (drawn).
    pub fn shown(&self) -> bool {
        matches!(self.state, State::Active | State::Retract)
    }

    /// One frame of `dt` seconds: `speed` = `travelSpeed`, `max_lens` = each ray's `GetMaxBeamLength` now.
    pub fn update(&mut self, dt: f32, speed: f32, max_lens: &[f32]) {
        match self.state {
            State::Delay => {
                self.delay -= dt;
                if self.delay <= 0.0 {
                    self.state = if self.ending {
                        State::Retract
                    } else {
                        State::Active
                    };
                }
            }
            State::Active => {
                for (k, len) in self.lens.iter_mut().enumerate() {
                    let max_len = max_lens.get(k).copied().unwrap_or(0.0).max(0.0);
                    *len = (*len + dt * speed).min(max_len);
                    self.end_on[k] = *len >= max_len;
                }
            }
            State::Retract => {
                self.retract += dt * speed;
                if self.retract >= self.lens[0] {
                    self.state = State::Done;
                    self.end_on.iter_mut().for_each(|b| *b = false);
                }
            }
            State::Done => {}
        }
    }

    /// The attack timer for this frame of `dt` seconds (call after [`Self::update`]): true when the rays strike now,
    /// the timer re-armed with `period` seconds. It runs only while the ribbons are in the world.
    pub fn attack_due(&mut self, dt: f32, period: f32) -> bool {
        if !self.shown() {
            return false;
        }
        self.attack -= dt;
        if self.attack > 0.0 {
            return false;
        }
        self.attack = period;
        true
    }
}
