//! The render adapters the options window's Video tab lists: `device` (int 7). Index i is entry i of the
//! list (0-based, no Default row), sorted best-first ([`enumerate`]) so `device` 0 is the GPU Bevy would
//! pick itself. An out-of-range index falls back to Bevy's default adapter.
//!
//! PORT: the engine lists the D3D adapters (`OptionsWindow::Load`); the port lists wgpu's adapters. The
//! choice applies at the next launch only (`main` sets `WgpuSettings` `adapter_name` / `backends`).
use bevy::prelude::*;

/// One listed adapter (`[i] name (Backend)` in the dropdown).
pub struct RenderAdapter {
    pub name: String,
    pub backend: String,
}

#[derive(Resource)]
pub struct RenderAdapters {
    pub entries: Vec<RenderAdapter>,
}

impl RenderAdapters {
    pub fn refresh(&mut self) {
        self.entries = enumerate();
    }
}

/// The host's wgpu adapters, best first and stable across launches: by device type (software last), then
/// backend (Vulkan, Metal, Dx12, Gl), then name. Identical (name, backend) pairs deduped.
pub fn enumerate() -> Vec<RenderAdapter> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut infos: Vec<wgpu::AdapterInfo> =
        bevy::tasks::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
            .into_iter()
            .map(|a| a.get_info())
            .collect();
    infos.sort_by(|a, b| {
        (type_rank(a.device_type), backend_rank(a.backend), &a.name).cmp(&(
            type_rank(b.device_type),
            backend_rank(b.backend),
            &b.name,
        ))
    });
    let mut entries: Vec<RenderAdapter> = infos
        .into_iter()
        .map(|info| RenderAdapter {
            name: info.name,
            backend: backend_name(info.backend).to_owned(),
        })
        .collect();
    entries.dedup_by(|a, b| a.name == b.name && a.backend == b.backend);
    entries
}

fn type_rank(t: wgpu::DeviceType) -> u8 {
    match t {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

/// [`enumerate`]'s backend order: the order wgpu's adapter request tries, GL last.
fn backend_rank(b: wgpu::Backend) -> u8 {
    match b {
        wgpu::Backend::Vulkan => 0,
        wgpu::Backend::Metal => 1,
        wgpu::Backend::Dx12 => 2,
        wgpu::Backend::Gl => 3,
        _ => 4,
    }
}

const BACKENDS: [wgpu::Backend; 6] = [
    wgpu::Backend::Vulkan,
    wgpu::Backend::Metal,
    wgpu::Backend::Dx12,
    wgpu::Backend::Gl,
    wgpu::Backend::BrowserWebGpu,
    wgpu::Backend::Noop,
];

/// The backend's name as the dropdown row shows it and as `device` settings / `main` match it.
pub fn backend_name(b: wgpu::Backend) -> &'static str {
    match b {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Dx12 => "Dx12",
        wgpu::Backend::Gl => "Gl",
        wgpu::Backend::BrowserWebGpu => "BrowserWebGpu",
        wgpu::Backend::Noop => "Noop",
    }
}

/// The `wgpu::Backend` a listed backend string names ([`backend_name`]); None
/// for an unknown string.
pub fn backend_of(backend: &str) -> Option<wgpu::Backend> {
    BACKENDS.into_iter().find(|b| backend_name(*b) == backend)
}
