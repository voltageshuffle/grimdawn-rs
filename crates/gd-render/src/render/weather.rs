//! The weather's presentation state: the running simulation (`gd_sim::weather::Weather`) and the
//! ripple / grass-wind requests it makes. It holds no gameplay state.
use bevy::prelude::*;
use gd_sim::weather::{Adjust, Weather};

/// The weather driver's system set: `water::update_ripples` runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WeatherSet;

/// The weather of the running session.
#[derive(Resource, Default)]
pub struct WeatherState {
    pub weather: Option<Weather>,
    pub ripples: crate::water::RippleSet,
    /// This frame's `Terrain::CreateGrassWind` calls.
    pub grass_winds: Vec<crate::render::grass::GrassWindPush>,
}

impl WeatherState {
    /// The ambience adjustments now (zero without weather).
    pub fn adjust(&self) -> Adjust {
        self.weather.as_ref().map(|w| w.adjust).unwrap_or_default()
    }
    /// `WeatherManager::GetWindStrength`: the wind's eased 1..3 factor, 1 without weather.
    pub fn wind_strength(&self) -> f32 {
        self.weather.as_ref().map_or(1.0, |w| w.wind.factor)
    }
}
