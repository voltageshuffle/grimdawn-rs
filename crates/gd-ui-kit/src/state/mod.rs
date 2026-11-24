//! Binary-local state the UI kit shares with the windows and the input modules. Only the pieces the
//! kit itself reads live here today (the settings-file helpers and the UI scale); the per-window
//! open-state resources stay with the binary's windows.
pub mod settings;
