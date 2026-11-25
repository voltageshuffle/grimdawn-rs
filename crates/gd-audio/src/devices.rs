//! The audio devices the options window's Audio tab lists: `soundDevice` (output) and `captureDevice` (input).
//! Index 0 is the system default — not an entry; index i >= 1 is entry i - 1 of the list, sorted by name. An
//! out-of-range index, or one whose device disappeared, falls back to the default.
//!
//! PORT: bevy_audio has no device hook, so the port owns the bus's rodio sink and opens it on the selected device.
//!
//! PORT: on ALSA the list keeps only the physical front:/hdmi: endpoints (the engine lists one entry per
//! physical endpoint).
use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait};

pub struct AudioDeviceEntry {
    pub id: cpal::DeviceId,
    pub name: String,
}

/// The output / input device lists for the options Audio tab (index 0 = system default, i >= 1 = vec[i - 1]).
#[derive(Resource, Default)]
pub struct AudioDevices {
    pub output: Vec<AudioDeviceEntry>,
    pub input: Vec<AudioDeviceEntry>,
}

impl AudioDevices {
    /// Re-enumerates both lists. A host or device error leaves the lists empty, logged once.
    pub fn refresh(&mut self) {
        self.output = enumerate(false);
        self.input = enumerate(true);
    }
}

pub(super) fn init_devices(mut devices: ResMut<AudioDevices>) {
    devices.refresh();
}

/// The output device a `soundDevice` index selects: None = the system default (index 0, out of range, or a listed
/// device that disappeared).
pub fn selected_output(devices: &AudioDevices, index: i32) -> Option<cpal::Device> {
    let entry = devices.output.get(index.checked_sub(1)? as usize)?;
    cpal::default_host()
        .output_devices()
        .ok()?
        .find(|d| d.id().ok().as_ref() == Some(&entry.id))
}

/// The host's output / input devices with a readable name, sorted by name.
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

/// Whether an enumerated device belongs in the list: on ALSA, keep front:/hdmi: PCMs only.
fn is_listed_endpoint(id: &cpal::DeviceId) -> bool {
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    if id.0 == cpal::platform::HostId::Alsa {
        return id.1.starts_with("front:") || id.1.starts_with("hdmi:");
    }
    let _ = id;
    true
}

fn warn_once(msg: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        warn!("{msg}");
    }
}
