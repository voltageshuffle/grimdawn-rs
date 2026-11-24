//! The audio devices the options window's Audio tab lists: `soundDevice` (output) and `captureDevice` (input).
//! Index 0 is the system default — not an entry; index i >= 1 is entry i - 1 of the list, sorted by name for
//! stable indices. An out-of-range index falls back to the default ([`selected_output`]).
//!
//! PORT: bevy_audio 0.20 has no device hook (`AudioOutput` is pub(crate) and always opens the default sink), so
//! the port owns the bus's rodio sink and opens it on the selected device (`mixer::start_bus`,
//! `mixer::switch_device`). The engine stores a device index too; hotplug shifts the enumeration under a saved
//! index — the same weakness: an index whose device disappeared falls back to the default.
//!
//! PORT: the engine (FMOD) lists one entry per physical endpoint. cpal's ALSA host instead enumerates every
//! PCM in the ALSA configuration — plugins/virtual devices (null, pulse, pipewire, ...), per-endpoint channel
//! variants (surround51, iec958, usbstream, ...) and raw hw:/plughw: duplicates of the same endpoints — so the
//! port keeps only the physical front:/hdmi: endpoints plus the Default row (index 0). On a pure-ALSA machine
//! Default still routes through the system default PCM.
use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait};

/// One listed device: the cpal id a selection matches on, and its readable name (the dropdown row).
pub struct AudioDeviceEntry {
    pub id: cpal::DeviceId,
    pub name: String,
}

/// The output / input device lists for the options Audio tab (index 0 = system default, i >= 1 = vec[i - 1]).
/// Refreshed at startup ([`init_devices`]) and by the options window when it opens.
#[derive(Resource, Default)]
pub struct AudioDevices {
    pub output: Vec<AudioDeviceEntry>,
    pub input: Vec<AudioDeviceEntry>,
}

impl AudioDevices {
    /// Re-enumerates both lists. Infallible in effect: a host or device error leaves the lists empty (a headless
    /// machine has no devices), logged once.
    pub fn refresh(&mut self) {
        self.output = enumerate(false);
        self.input = enumerate(true);
    }
}

/// The device lists at startup; the options window refreshes them when it opens.
pub(super) fn init_devices(mut devices: ResMut<AudioDevices>) {
    devices.refresh();
}

/// The output device a `soundDevice` index selects: None = the system default (index 0, out of range, or a listed
/// device that disappeared). Re-queries the host and matches the id, so a stale index cannot open another device.
pub fn selected_output(devices: &AudioDevices, index: i32) -> Option<cpal::Device> {
    let entry = devices.output.get(index.checked_sub(1)? as usize)?;
    cpal::default_host()
        .output_devices()
        .ok()?
        .find(|d| d.id().ok().as_ref() == Some(&entry.id))
}

/// The host's output / input devices with a readable name, sorted by name; ALSA keeps only physical endpoints
/// ([`is_listed_endpoint`]).
fn enumerate(input: bool) -> Vec<AudioDeviceEntry> {
    let host = cpal::default_host();
    let devices = if input {
        host.input_devices()
    } else {
        host.output_devices()
    };
    let devices = match devices {
        Ok(d) => d,
        Err(e) => {
            warn_once(&format!("audio device enumeration failed: {e}"));
            return Vec::new();
        }
    };
    let mut entries: Vec<AudioDeviceEntry> = devices
        .filter_map(|d| {
            let id = d.id().ok()?;
            if !is_listed_endpoint(&id) {
                return None;
            }
            let name = d.description().ok()?.name().to_string();
            (!name.is_empty()).then_some(AudioDeviceEntry { id, name })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Whether an enumerated device belongs in the list. The ALSA host exposes plugins, channel variants and raw
/// duplicates alongside the physical endpoints, so filter on the stable id's PCM name: keep front:/hdmi: only.
/// Other hosts already list one entry per endpoint and pass through unchanged.
fn is_listed_endpoint(id: &cpal::DeviceId) -> bool {
    if id.0 == cpal::platform::HostId::Alsa {
        id.1.starts_with("front:") || id.1.starts_with("hdmi:")
    } else {
        true
    }
}

/// Enumeration errors are logged once: a headless machine fails every refresh.
fn warn_once(msg: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        warn!("{msg}");
    }
}
