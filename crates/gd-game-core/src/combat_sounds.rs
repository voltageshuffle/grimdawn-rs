//! The sound records of FX paks, resolved from the database (the combat
//! sounds themselves are gd-game's `combat_sounds` systems).

/// The sound an FX pak record plays as it starts: an `FxPak`'s
/// `activateSound`, a class-less `charfxpak.tpl` record's `startSound`.
pub fn fx_pak_sound(pak: &gd_formats::arz::Record) -> Option<String> {
    let k = if pak.class == "FxPak" {
        "activateSound"
    } else {
        "startSound"
    };
    pak.str(k).filter(|s| !s.is_empty()).map(str::to_owned)
}
