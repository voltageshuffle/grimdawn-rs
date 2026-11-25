//! Session audit: `GD_CENSUS` writes, `GD_CENSUS_SECS` (default 25) seconds after the session began, what the
//! session added to the world since then: resources created, resources that existed before and were mutated,
//! and the entity kinds spawned (archetype census). Component and resource names need Bevy's `debug` feature:
//! `cargo dev-build --features audit`.
use bevy::ecs::component::ComponentId;
use bevy::prelude::*;
use gd_formats::collections::HashSet;
use gd_ui::state::session::AppState;
use std::collections::BTreeMap;

pub struct AuditPlugin;

impl Plugin for AuditPlugin {
    fn build(&self, app: &mut App) {
        // `main` inserts the hooks before the plugins.
        if app
            .world()
            .resource::<gd_game_core::hooks::Hooks>()
            .audit
            .census
            .is_some()
        {
            app.add_systems(
                First,
                baseline.run_if(resource_added::<gd_game_core::session::SessionReady>),
            )
            .add_systems(Update, report.run_if(in_state(AppState::InGame)));
        }
    }
}

#[derive(Resource)]
struct Baseline {
    resources: HashSet<ComponentId>,
    entities: HashSet<Entity>,
    tick: bevy::ecs::change_detection::Tick,
    at: std::time::Instant,
    reported: bool,
}

fn baseline(world: &mut World) {
    let resources = world.iter_resources().map(|(id, _, _)| id).collect();
    let entities = world.iter_entities().map(|e| e.id()).collect();
    let tick = world.read_change_tick();
    world.insert_resource(Baseline {
        resources,
        entities,
        tick,
        at: std::time::Instant::now(),
        reported: false,
    });
}

fn report(world: &mut World) {
    let Some(b) = world.get_resource::<Baseline>() else {
        return;
    };
    if b.reported {
        return;
    }
    let audit = world.resource::<gd_game_core::hooks::Hooks>().audit.clone();
    let Some(b) = world.get_resource::<Baseline>() else {
        return;
    };
    if b.at.elapsed().as_secs_f32() < audit.census_secs {
        return;
    }
    world.resource_mut::<Baseline>().reported = true;
    let Some(b) = world.get_resource::<Baseline>() else {
        return;
    };
    let now = world.read_change_tick();
    let mut out = String::new();
    let mut created = Vec::new();
    let mut mutated = Vec::new();
    for (id, info, _) in world.iter_resources() {
        let name = info.name().to_string();
        if !b.resources.contains(&id) {
            created.push(name);
        } else if world
            .get_resource_change_ticks_by_id(id)
            .is_some_and(|t| t.changed.is_newer_than(b.tick, now))
        {
            mutated.push(name);
        }
    }
    created.sort();
    mutated.sort();
    out += &format!(
        "== resources created in the session ({})\n{}\n\n== resources that existed before and were mutated ({})\n{}\n\n",
        created.len(),
        created.join("\n"),
        mutated.len(),
        mutated.join("\n")
    );
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0;
    for e in world.iter_entities() {
        if b.entities.contains(&e.id()) {
            continue;
        }
        total += 1;
        let mut names: Vec<String> = e
            .archetype()
            .components()
            .iter()
            .copied()
            .filter_map(|c| world.components().get_info(c).map(|i| i.name().to_string()))
            .filter(|n| {
                !n.contains("Visibility")
                    && !n.contains("bevy_transform")
                    && !n.contains("ViewVisibility")
            })
            .collect();
        names.sort();
        *kinds.entry(names.join(", ")).or_default() += 1;
    }
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
    out += &format!("== entities spawned in the session ({total}), by component set\n");
    for (k, n) in kinds {
        out += &format!("{n}\t{k}\n");
    }
    if let Some(path) = &audit.census {
        drop(std::fs::write(path, out));
    }
}
