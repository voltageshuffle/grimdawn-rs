//! Area triggers (`BoxTrigger` records), ported from Game.dll
//! and hooks from `AreaTriggerHookPack::LoadHooks`: `shouldTrigger`,
//! `onEnter`, `onLeave`, `onInside`, each called with the entity's object id.
//!
//! State machine (`+136`): 0 disabled, 1 armed, 2 triggered, 3 resetting.
//! While armed or triggered a countdown runs ([`AreaTrigger::tick`]); when it
//! expires the trigger collects the entities inside ([`AreaTrigger::collect`])
//! and filters them (`AreaTrigger::Filter` @0x1800273e0): only a
//! trigger that names `detectableNames` filters at all, and then keeps an
//! entity whose record name is one of them (compared by FNV-1a hash in the
//! engine) and that `AreaTrigger::ShouldTrigger` @0x180027720 accepts: true
//! without a `shouldTrigger` hook, else the Lua function is called with the
//! entity's object id and must return the boolean `true` (hook call
//! sub_1800295A0: any other result, a Lua error or a function that does not
//! exist is false). The hook is Lua, so the script host asks it between
//! `collect` and [`AreaTrigger::fire`] ([`AreaTrigger::hook`]). Then:
//! * armed: each entity not inside last time fires `onEnter` and the trigger
//!   becomes *triggered* (if `resetPeriod >= 0`); `singular` stops after one;
//! * every entity inside last time fires `onInside` if still inside, else
//!   `onLeave`;
//! * triggered with someone inside → countdown = `updatePeriod`; with nobody
//!   inside → *resetting* for `resetPeriod`, or the trigger is destroyed when
//!   `resetPeriod <= 0` (one-shot);
//! * resetting → armed when the countdown expires.
//!
//! Shape: `extentsX/Y/Z` as half sizes of a box in the entity's frame.
//! The unnamed `BoxTrigger` (Load 0x180049e20, collect
//! 0x180049700): candidates come from a sphere of radius |extents|, then the
//! entity's position must lie within ±extents along each of the trigger's
//! axes (OBB test 0x1805932c0).
use super::state::Coords;

#[derive(Debug, Clone)]
pub struct AreaTrigger {
    pub id: u32,
    pub record: String,
    pub coords: Coords,
    pub half_extents: [f32; 3],
    pub detectable: Vec<String>,
    /// `detects` (`AreaTrigger::ResolveEnum_Detection`): 0 `AnyEntity`, 1 `Monsters` (the collect
    /// 0x180049700 keeps the entities of class `Monster` in the sphere), 2 `Players`, else 3 `LocalPlayer` (the
    /// default: no stock trigger names `Players` / `LocalPlayer`).
    pub detects: u8,
    pub on_enter: Option<String>,
    pub on_leave: Option<String>,
    pub on_inside: Option<String>,
    pub should_trigger: Option<String>,
    pub update_ms: i32,
    pub reset_ms: i32,
    pub singular: bool,
    pub state: u8,
    pub timer: i32,
    pub inside: Vec<u32>,
    pub destroyed: bool,
}

/// One detectable candidate: (object id, record, world position).
pub type Candidate<'a> = (u32, &'a str, [f32; 3]);

#[derive(Debug, Clone, PartialEq)]
pub struct TriggerCall {
    pub trigger: u32,
    pub function: String,
    pub entity: u32,
}

/// A trigger whose countdown expired with `inside` collected and a `shouldTrigger` hook (`function`) to ask
/// for each of them before its step ends (`Story::update_triggers`).
#[derive(Debug, Clone, PartialEq)]
pub struct Hooked {
    pub trigger: u32,
    pub function: String,
    pub inside: Vec<u32>,
}

impl AreaTrigger {
    /// From a `BoxTrigger` record's fields (`get(name)` returns the raw value).
    pub fn from_record(
        id: u32,
        record: &str,
        coords: Coords,
        get: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        let f = |k: &str| get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let s = |k: &str| get(k).filter(|v| !v.is_empty());
        let enabled = get("enabled").is_some_and(|v| v != "0");
        Self {
            id,
            record: record.to_string(),
            coords,
            half_extents: [f("extentsX"), f("extentsY"), f("extentsZ")],
            detectable: get("detectableNames")
                .map(|v| {
                    v.split(';')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.replace('\\', "/").to_ascii_lowercase())
                        .collect()
                })
                .unwrap_or_default(),
            detects: match get("detects").as_deref() {
                Some("AnyEntity") => 0,
                Some("Monsters") => 1,
                Some("Players") => 2,
                _ => 3,
            },
            on_enter: s("onEnter"),
            on_leave: s("onLeave"),
            on_inside: s("onInside"),
            should_trigger: s("shouldTrigger"),
            update_ms: (f("updatePeriod") * 1000.0) as i32,
            reset_ms: (f("resetPeriod") * 1000.0) as i32,
            singular: get("singular").is_some_and(|v| v != "0"),
            state: enabled as u8,
            timer: 0,
            inside: Vec::new(),
            destroyed: false,
        }
    }

    pub fn contains(&self, p: [f32; 3]) -> bool {
        let d = [
            p[0] - self.coords.origin[0],
            p[1] - self.coords.origin[1],
            p[2] - self.coords.origin[2],
        ];
        (0..3).all(|i| {
            let a = self.coords.axes[i];
            (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]).abs() <= self.half_extents[i]
        })
    }

    /// Whether a candidate of this kind (`player`: the player, else a monster) is collected ([`Self::detects`]).
    pub fn detects_kind(&self, player: bool) -> bool {
        match self.detects {
            0 => true,
            1 => !player,
            _ => player,
        }
    }

    fn detectable(&self, record: &str) -> bool {
        self.detectable.is_empty()
            || self
                .detectable
                .iter()
                .any(|d| d.eq_ignore_ascii_case(record))
    }

    /// The `shouldTrigger` function the collected entities must pass (module doc): asked only by a
    /// trigger that names `detectableNames`.
    pub fn hook(&self) -> Option<&str> {
        self.should_trigger
            .as_deref()
            .filter(|_| !self.detectable.is_empty())
    }

    /// Advances the countdown by `dt_ms`; true when it expired: the trigger collects now
    /// ([`Self::collect`], then [`Self::fire`]).
    pub fn tick(&mut self, dt_ms: i32) -> bool {
        match self.state {
            1 | 2 => {}
            3 => {
                self.timer -= dt_ms;
                if self.timer <= 0 {
                    self.state = 1;
                }
                return false;
            }
            _ => return false,
        }
        // An armed trigger with nobody inside collects every frame and never resets
        // its timer: saturate so a long session can't overflow it.
        self.timer = self.timer.saturating_sub(dt_ms);
        self.timer < 0
    }

    /// The ids of the candidates the trigger collects: of the kind it detects ([`Self::detects_kind`];
    /// `player` is the player's id), inside the box and detectable by name.
    pub fn collect<'a>(
        &self,
        candidates: impl Iterator<Item = Candidate<'a>>,
        player: u32,
    ) -> Vec<u32> {
        candidates
            .filter(|(id, rec, p)| {
                self.detects_kind(*id == player) && self.detectable(rec) && self.contains(*p)
            })
            .map(|(id, _, _)| id)
            .collect()
    }

    /// The expired countdown's step with the entities `now` inside (collected, and accepted by the
    /// `shouldTrigger` hook): the hook calls to make are pushed to `calls` in order.
    pub fn fire(&mut self, now: Vec<u32>, calls: &mut Vec<TriggerCall>) {
        let mut call = |f: &Option<String>, entity: u32| {
            if let Some(f) = f {
                calls.push(TriggerCall {
                    trigger: self.id,
                    function: f.clone(),
                    entity,
                });
            }
        };
        if self.state == 1 {
            for &id in &now {
                if !self.inside.contains(&id) {
                    if self.reset_ms >= 0 {
                        self.state = 2;
                    }
                    call(&self.on_enter, id);
                    if self.singular {
                        break;
                    }
                }
            }
        }
        for &id in &self.inside {
            if now.contains(&id) {
                call(&self.on_inside, id)
            } else {
                call(&self.on_leave, id)
            }
        }
        self.inside = now;
        if self.state == 2 {
            if !self.inside.is_empty() {
                self.timer = self.update_ms;
            } else if self.reset_ms > 0 {
                self.state = 3;
                self.timer = self.reset_ms;
            } else {
                self.destroyed = true;
                self.state = 0;
            }
        }
    }
}
