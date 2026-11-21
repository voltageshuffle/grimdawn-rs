//! The weather's presentation state: the running simulation (`gd_sim::weather::Weather`) and the
//! ripple / grass-wind requests it makes. The drawing systems read it — `render::env`'s ambience
//! adjustment ([`WeatherState::adjust`]), `render::grass`'s wind ([`WeatherState::wind_strength`] and
//! `grass_winds`), `water`'s ripples — while the app's weather driver (`weather.rs`) writes it each
//! frame. It holds no gameplay state (the rules are `gd_sim::weather`'s).
use bevy::prelude::*;
use gd_sim::weather::{Adjust, Weather};

/// The weather driver's system set: `water::update_ripples` runs after it (the ripples it requests).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WeatherSet;

/// The weather of the running session (reset by `exit.rs`).
#[derive(Resource, Default)]
pub struct WeatherState {
    pub weather: Option<Weather>,
    /// The water ripples (`water::update_ripples` places and draws them).
    pub ripples: crate::water::RippleSet,
    /// This frame's `Terrain::CreateGrassWind` calls (`render::grass::grass_offsets` applies them).
    pub grass_winds: Vec<crate::render::grass::GrassWindPush>,
}

impl WeatherState {
    /// The ambience adjustments now (zero without weather), for `render::env`.
    pub fn adjust(&self) -> Adjust {
        self.weather.as_ref().map(|w| w.adjust).unwrap_or_default()
    }
    /// `WeatherManager::GetWindStrength`: the wind's eased 1..3 factor, 1
    /// without weather (the manager's default), for render/grass.rs.
    pub fn wind_strength(&self) -> f32 {
        self.weather.as_ref().map_or(1.0, |w| w.wind.factor)
    }
}
