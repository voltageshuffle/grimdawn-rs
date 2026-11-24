//! Memory audit (hooks.rs `GD_MEM_*`): what holds the process' memory, and a scripted session that moves it.
//! - `GD_MEM_LOG=<secs>` logs `mem:` every `secs`: the resident set, glibc's heap (bytes in use, free bytes the
//!   allocator keeps, mmapped blocks), the Bevy asset counts (images with their CPU bytes and an estimate of their GPU
//!   bytes), the loaded levels and the caches' live entries (textures, models, meshes, animations, sounds, effects,
//!   terrain layers). `GD_MEM_TRIM`
//!   returns the allocator's free memory first (`malloc_trim`), which separates retention from live data. With
//!   jemalloc preloaded and its profiler on (`LD_PRELOAD=/usr/lib/libjemalloc.so MALLOC_CONF=prof:true,...`), each
//!   log also dumps a heap profile (`prof.dump`, read with `jeprof`).
//! - `GD_MEM_TOUR=<n>@<secs>` travels to `n` riftgates spread over the map's index, `secs` after each arrival.
//! - `GD_MEM_EXIT=<secs>`: the first session exits to the main menu `secs` after it began (after the tour), and the
//!   same character starts again from the menu 5 s later.
//!
//! The resident set reads `/proc`, the heap figures and the trim are glibc's and the profile dump needs `dlsym`:
//! elsewhere (Windows) they log 0 / are skipped.
use bevy::prelude::*;
use gd_game_core::hooks::Hooks;
use gd_ui::state::session::AppState;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            log.run_if(|h: Res<Hooks>| h.audit.mem_log.is_some()),
            tour.run_if(|h: Res<Hooks>| h.audit.mem_tour.is_some()),
            exit_and_restart.run_if(|h: Res<Hooks>| h.audit.mem_exit.is_some()),
        ),
    );
}

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(0.0, |pages| pages * 4096.0 / 1.0e6)
}

/// jemalloc's `mallctl` when it is the process' allocator (preloaded).
#[cfg(unix)]
fn jemalloc_dump() -> bool {
    type Mallctl = unsafe extern "C" fn(
        *const libc::c_char,
        *mut libc::c_void,
        *mut usize,
        *mut libc::c_void,
        usize,
    ) -> libc::c_int;
    // SAFETY: `dlsym` only reads the symbol table (`RTLD_DEFAULT` searches the loaded images);
    // a null result is checked below.
    let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"mallctl".as_ptr()) };
    if sym.is_null() {
        return false;
    }
    // SAFETY: `sym` is non-null (checked above) and is jemalloc's `mallctl`, whose C ABI matches
    // the `Mallctl` fn type.
    let mallctl: Mallctl = unsafe { std::mem::transmute(sym) };
    // SAFETY: `mallctl` is the resolved symbol; the name is a static NUL-terminated string and the
    // out pointers/sizes follow the jemalloc API (null where the value is unused).
    unsafe {
        mallctl(
            c"prof.dump".as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        ) == 0
    }
}

#[cfg(not(unix))]
fn jemalloc_dump() -> bool {
    false
}

/// glibc's heap: bytes in use, free bytes the allocator keeps, mmapped blocks.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn heap_bytes() -> (usize, usize, usize) {
    // SAFETY: plain libc allocator-statistics call with no memory-safety invariants.
    let heap = unsafe { libc::mallinfo2() };
    (heap.uordblks, heap.fordblks, heap.hblkhd)
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn heap_bytes() -> (usize, usize, usize) {
    (0, 0, 0)
}

/// Returns glibc's free heap pages to the system (`malloc_trim`); false where there is no such call.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim() -> bool {
    // SAFETY: plain libc call with no memory-safety invariants (releases free heap pages).
    unsafe { libc::malloc_trim(0) };
    true
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim() -> bool {
    false
}

/// An image's GPU bytes: every mip of every layer at its format's block size.
fn gpu_bytes(img: &Image) -> u64 {
    let d = &img.texture_descriptor;
    let (bw, bh) = d.format.block_dimensions();
    let block = d.format.block_copy_size(None).unwrap_or(4) as u64;
    let layers = d.size.depth_or_array_layers.max(1) as u64;
    (0..d.mip_level_count.max(1))
        .map(|m| {
            let w = (d.size.width >> m).max(1).div_ceil(bw) as u64;
            let h = (d.size.height >> m).max(1).div_ceil(bh) as u64;
            w * h * block * layers
        })
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn log(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    mut last: Local<f32>,
    entities: Query<()>,
    assets: (
        Res<Assets<Mesh>>,
        Res<Assets<Image>>,
        Res<Assets<StandardMaterial>>,
        Res<Assets<crate::world::TerrainMaterial>>,
    ),
    caches: (
        Res<gd_render::textures::Textures>,
        Res<gd_render::model::ModelCache>,
        Res<gd_game_core::creature::BodyCache>,
        Res<gd_game_core::character::AnimCache>,
        Res<crate::audio::Sounds>,
        Option<Res<gd_render::fx::FxAssets>>,
        Res<gd_render::render::terrain::TerrainLayers>,
    ),
    levels: Res<crate::world::Levels>,
) {
    let every = hooks.audit.mem_log.unwrap_or(5.0);
    let now = time.elapsed_secs();
    if now - *last < every {
        return;
    }
    *last = now;
    if hooks.audit.mem_trim {
        let (before, t) = (rss_mb(), std::time::Instant::now());
        if trim() {
            info!(
                "mem: malloc_trim {:.1} ms, rss {before:.0} -> {:.0} MB",
                t.elapsed().as_secs_f64() * 1e3,
                rss_mb()
            );
        }
    }
    let (heap_used, heap_free, heap_mmap) = heap_bytes();
    let mb = |b: usize| b as f64 / 1.0e6;
    let (meshes, images, std_mats, terrain_mats) = assets;
    let (img_cpu, img_gpu) = images.iter().fold((0usize, 0u64), |(c, g), (_, i)| {
        (c + i.data.as_ref().map_or(0, Vec::len), g + gpu_bytes(i))
    });
    let (textures, models, bodies, anims, sounds, fx, layers) = caches;
    let (sound_n, sound_bytes) = sounds.footprint();
    let (pfx, fx_tex) = fx.map_or((0, 0), |f| f.count(&images));
    let dumped = jemalloc_dump();
    info!(
        "mem: {:?} t {now:.0}s rss {:.0} MB; heap used {:.0} free {:.0} mmap {:.0} MB{}; {} entities, {} levels; \
         meshes {}, images {} (cpu {:.0} MB, gpu {:.0} MB), std mats {}, terrain mats {}; caches: textures {}, \
         models {}, bodies {}, anims {}, sounds {} ({:.0} MB), pfx {}, fx textures {}, terrain layers {}",
        state.get(),
        rss_mb(),
        mb(heap_used),
        mb(heap_free),
        mb(heap_mmap),
        if dumped { ", profile dumped" } else { "" },
        entities.iter().len(),
        levels.loaded.len(),
        meshes.len(),
        images.len(),
        mb(img_cpu),
        img_gpu as f64 / 1.0e6,
        std_mats.len(),
        terrain_mats.len(),
        textures.count(&images),
        models.count(),
        bodies.count(),
        anims.count(),
        sound_n,
        mb(sound_bytes),
        pfx,
        fx_tex,
        layers.count(),
    );
}

#[derive(Default)]
struct Tour {
    gates: Vec<Vec3>,
    next: usize,
    /// When the last travel ended (or the session began).
    since: Option<f32>,
}

fn tour(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    index: Option<Res<gd_game::riftgates::WaypointIndex>>,
    pending: Res<crate::doors::PendingTravel>,
    mut out: MessageWriter<gd_game::riftgates::GateTravel>,
    mut local: Local<Tour>,
) {
    let Some((n, secs)) = hooks.audit.mem_tour else {
        return;
    };
    if *state.get() != AppState::InGame || local.next >= n {
        return;
    }
    let Some(index) = index else { return };
    if local.gates.is_empty() {
        let mut gates: Vec<_> = index
            .0
            .iter()
            .filter(|(_, p)| p.record.contains("riftgate"))
            .map(|(uid, p)| (*uid, p.pos))
            .collect();
        gates.sort_by_key(|g| g.0);
        local.gates = (0..n)
            .filter_map(|i| gates.get(i * gates.len() / n.max(1)).map(|g| g.1))
            .collect();
        info!("mem tour: {} riftgates", local.gates.len());
    }
    let now = time.elapsed_secs();
    if pending.0.is_some() {
        local.since = None;
        return;
    }
    let since = *local.since.get_or_insert(now);
    if now - since < secs {
        return;
    }
    if let Some(&to) = local.gates.get(local.next) {
        info!("mem tour: travel {} to {to}", local.next + 1);
        out.write(gd_game::riftgates::GateTravel(to));
    }
    local.next += 1;
    local.since = None;
}

#[derive(Default)]
struct Restart {
    /// When the first session began; when the menu showed.
    since: Option<f32>,
    exited: Option<(gd_game_core::session::Choice, gd_rules::mode::Mode)>,
    done: bool,
}

#[allow(clippy::too_many_arguments)]
fn exit_and_restart(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    choice: Option<Res<gd_game_core::session::Choice>>,
    mode: Option<Res<gd_game_core::session::GameMode>>,
    pending: Res<crate::doors::PendingTravel>,
    mut exit: MessageWriter<gd_ui::state::exit::ExitToMenu>,
    mut commands: Commands,
    mut local: Local<Restart>,
) {
    let Some(secs) = hooks.audit.mem_exit else {
        return;
    };
    if local.done {
        return;
    }
    let now = time.elapsed_secs();
    let tour_left = hooks.audit.mem_tour.is_some_and(|_| pending.0.is_some());
    match (state.get(), local.exited.take()) {
        (AppState::InGame, None) => {
            let since = *local.since.get_or_insert(now);
            if now - since >= secs && !tour_left {
                let (Some(choice), Some(mode)) = (choice, mode) else {
                    return;
                };
                info!("mem: exiting to the main menu");
                local.exited = Some((choice.clone(), mode.0));
                local.since = None;
                exit.write(gd_ui::state::exit::ExitToMenu);
            }
        }
        (AppState::Menu, Some((choice, mode))) => {
            let since = *local.since.get_or_insert(now);
            if now - since < 5.0 {
                local.exited = Some((choice, mode));
                return;
            }
            let gdc = choice.save.as_ref().and_then(|p| {
                std::fs::read(p)
                    .ok()
                    .and_then(|b| gd_formats::save::gdc::Gdc::parse(&b).ok())
            });
            info!("mem: starting the session again");
            crate::menu::start(
                &mut commands,
                choice,
                gdc,
                mode.difficulty,
                mode.veteran() || mode.ultimate_veteran(),
            );
            local.done = true;
        }
        (_, exited) => local.exited = exited,
    }
}
