//! The render adapters the options window's Video tab lists: `device` (int 7). Index i is entry i of the
//! list (0-based, no Default row — the engine's device index selects a listed adapter directly), sorted by
//! (name, backend) for stable indices. An out-of-range index falls back to Bevy's default adapter.
//!
//! PORT: the engine lists the D3D adapters (`OptionsWindow::Load` fills the render-device dropdown from the
//! graphics engine's adapter list); the port lists wgpu's adapters instead. The choice is applied at the NEXT
//! launch only — `main` reads `device` before `DefaultPlugins` and sets `WgpuSettings.adapter_name` /
//! `backends` — there is no hot-swap. A stale index (the adapter disappeared) falls back to Bevy's default
//! adapter selection.
use bevy::prelude::*;

/// One listed adapter: its name and backend as the dropdown row shows them (`[i] name (Backend)`).
pub struct RenderAdapter {
    pub name: String,
    pub backend: String,
}

/// The adapter list for the options Video tab (index i = entries[i], 0-based). `main` enumerates it once
/// before the plugins build (the launch adapter choice reads it) and inserts it; [`init_adapters`] reuses
/// that list at startup, and the options window refreshes it when it opens.
#[derive(Resource, Default)]
pub struct RenderAdapters {
    pub entries: Vec<RenderAdapter>,
}

impl RenderAdapters {
    /// Re-enumerates the list.
    pub fn refresh(&mut self) {
        self.entries = enumerate();
    }
}

pub struct RenderDevicesPlugin;

impl Plugin for RenderDevicesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderAdapters>()
            .add_systems(Startup, init_adapters);
    }
}

/// The adapter list at startup: `main` already enumerated it to pick the launch adapter, so reuse that
/// list; enumerate only when it is empty (a headless machine has no adapters). The options window
/// refreshes the list when it opens.
fn init_adapters(mut adapters: ResMut<RenderAdapters>) {
    if adapters.entries.is_empty() {
        adapters.refresh();
    }
}

/// The host's wgpu adapters, sorted by (name, backend) for stable indices, identical (name, backend)
/// pairs deduped.
pub fn enumerate() -> Vec<RenderAdapter> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut entries: Vec<RenderAdapter> =
        bevy::tasks::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
            .into_iter()
            .map(|a| {
                let info = a.get_info();
                RenderAdapter {
                    name: info.name,
                    backend: format!("{:?}", info.backend),
                }
            })
            .collect();
    entries.sort_by(|a, b| (&a.name, &a.backend).cmp(&(&b.name, &b.backend)));
    entries.dedup_by(|a, b| a.name == b.name && a.backend == b.backend);
    entries
}

/// The `wgpu::Backend` a listed backend string names (the `Debug` spelling [`enumerate`] stores); None
/// for an unknown string. `main` restricts `WgpuSettings.backends` to it so a substring name match
/// cannot pick an adapter of another backend.
pub fn backend_of(backend: &str) -> Option<wgpu::Backend> {
    match backend {
        "Vulkan" => Some(wgpu::Backend::Vulkan),
        "Metal" => Some(wgpu::Backend::Metal),
        "Dx12" => Some(wgpu::Backend::Dx12),
        "Gl" => Some(wgpu::Backend::Gl),
        "BrowserWebGpu" => Some(wgpu::Backend::BrowserWebGpu),
        "Noop" => Some(wgpu::Backend::Noop),
        _ => None,
    }
}
