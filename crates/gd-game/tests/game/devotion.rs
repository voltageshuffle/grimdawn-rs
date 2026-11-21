//! Headless checks of devotion: the saved state, story grants, taking and
//! refunding stars, the shrine window (offerings, accept / cancel).
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game::devotion::{
    DevotionState, RemoveStar, Shrine, ShrineRequest, ShrineWindow, TakeStar, offering_slots,
    spawn_shrine,
};
use gd_game::inventory::Inventory;
use gd_game::player::Loadout;
use gd_game::story::StoryEffect;
use gd_game::testing::TestGame;

fn game() -> Option<(TestGame, Entity)> {
    Some(started(TestGame::new_character("Tester", false, false)?))
}

/// The player on flat ground, its derived state up.
fn started(mut g: TestGame) -> (TestGame, Entity) {
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    g.steps(2);
    (g, p)
}

fn state(g: &TestGame) -> &DevotionState {
    g.app.world().resource::<DevotionState>()
}

/// Puts the devotion window in its reclaim mode: a spirit guide beside the player opens it at once (the
/// character has no mastery, a devotion point is spent).
fn reclaim_mode(g: &mut TestGame) {
    use gd_game::reclamation::{ReclaimRequest, SkillReclamation};
    let guide = g.spawn_monster(
        "records/creatures/npcs/merchants/reallocatora01.dbr",
        1,
        Vec3::new(2.0, 0.0, 0.0),
    );
    g.step();
    g.app.world_mut().write_message(ReclaimRequest::Open(guide));
    g.step();
    assert!(g.app.world().resource::<SkillReclamation>().devotion_mode());
}

/// A story grant adds a point (capped); the points and the restored shrines (save block 17) come back from
/// the save.
#[test]
fn save_state_and_story_grants() {
    let Some((mut g, _)) = game() else { return };
    assert!(state(&g).restored.is_empty(), "a new character");
    let (points, total) = (state(&g).points, state(&g).total);
    g.app
        .world_mut()
        .write_message(StoryEffect(gd_sim::story::Effect::GiveDevotion(1)));
    g.step();
    let s = state(&g);
    let max = g.rules().levels.max_devotion_points.max(0) as u32;
    assert!(
        (s.points, s.total) == (points + 1, total + 1) || s.total == max,
        "capped at {max}"
    );
    let uid = [1, 2, 3, 4];
    g.app
        .world_mut()
        .resource_mut::<DevotionState>()
        .restored
        .push(uid);
    let (points, total) = (state(&g).points, state(&g).total);
    let save = g.save_now();
    let (h, _) = started(TestGame::with_save(&save).expect("the saved character"));
    let s = state(&h);
    assert_eq!((s.points, s.total), (points, total));
    assert_eq!(s.restored, vec![uid]);
}

/// A free star can be taken with a point (its skill is learned), and
/// refunded for gold and aether only in the reclaim mode (a spirit guide's),
/// where nothing is taken.
#[test]
fn take_and_refund_a_star() {
    let Some((mut g, p)) = game() else { return };
    // Five points earned, none spent.
    {
        let mut s = g.app.world_mut().resource_mut::<DevotionState>();
        s.points = 5;
        s.total = 5;
    }
    let found = {
        let s = state(&g);
        let book = &g.get::<Loadout>(p).skills;
        let taken = |n: &str| book.level(n) > 0;
        (0..s.data.list.len())
            .flat_map(|c| (0..s.data.list[c].stars.len()).map(move |st| (c, st)))
            .find(|&(c, st)| s.data.cannot_take(c, st, &taken, s.points).is_none())
    };
    let (c, st) = found.expect("some star is takeable");
    let skill = state(&g).data.list[c].stars[st].skill.clone();
    g.app.world_mut().write_message(TakeStar {
        constellation: c,
        star: st,
    });
    g.step();
    assert_eq!(g.get::<Loadout>(p).skills.level(&skill), 1);
    assert_eq!(state(&g).points, 4);
    // Refund: enough gold and aether crystals in the bags.
    let crystal = g
        .data()
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|r| r.str("aetherCrystal").map(str::to_owned))
        .unwrap();
    g.app
        .world_mut()
        .resource_mut::<gd_game::progress::PlayerProgress>()
        .gold += 10_000_000;
    for _ in 0..50 {
        g.app
            .world_mut()
            .resource_mut::<Inventory>()
            .add_to_bags(Item {
                base_name: crystal.clone(),
                stack_count: 1,
                ..Default::default()
            });
    }
    let remove = RemoveStar {
        constellation: c,
        star: st,
    };
    // The plain window refunds nothing.
    g.app.world_mut().write_message(remove);
    g.step();
    assert_eq!(g.get::<Loadout>(p).skills.level(&skill), 1);
    reclaim_mode(&mut g);
    g.app.world_mut().write_message(remove);
    g.step();
    assert_eq!(g.get::<Loadout>(p).skills.level(&skill), 0);
    assert_eq!(state(&g).points, 5);
    // In the mode a star is not taken.
    g.app.world_mut().write_message(TakeStar {
        constellation: c,
        star: st,
    });
    g.step();
    assert_eq!(g.get::<Loadout>(p).skills.level(&skill), 0);
    // Hiding the window leaves the mode (and autosaves).
    g.app
        .world_mut()
        .write_message(gd_game::reclamation::ReclaimRequest::Close);
    g.step();
    let r = g
        .app
        .world()
        .resource::<gd_game::reclamation::SkillReclamation>();
    assert!(!r.active() && !r.devotion);
}

/// Using a shrine only opens its window; "Offer" without the offerings
/// does nothing, Cancel closes it, and "Offer" with them takes one of each
/// and restores it (points, the restored list).
#[test]
fn shrine_window_takes_the_offerings() {
    let Some((mut g, _)) = game() else { return };
    let rec = "records/interactive/devotionshrinec02.dbr";
    let uid = [1, 2, 3, 4];
    let data = g.data();
    let e = {
        let w = g.app.world_mut();
        let e = spawn_shrine(
            &mut w.commands(),
            &data.db,
            Transform::from_xyz(1.0, 0.0, 0.0),
            rec,
            uid,
            gd_sim::mode::Mode::default(),
        )
        .expect("shrine");
        w.flush();
        e
    };
    let open = |g: &TestGame| g.app.world().resource::<ShrineWindow>().shrine;
    let use_it = |g: &mut TestGame| {
        g.interact(e);
        g.step();
    };
    let answer = |g: &mut TestGame, r: ShrineRequest| {
        g.app.world_mut().write_message(r);
        g.step();
    };
    let slots = |g: &TestGame| {
        let w = g.app.world();
        offering_slots(
            g.get::<Shrine>(e),
            w.resource::<Inventory>(),
            w.get_resource::<gd_game::stash::Stash>(),
        )
    };
    let count = |g: &TestGame, rec: &str| {
        let w = g.app.world();
        let names = [rec.to_owned()];
        w.get_resource::<gd_game::stash::Stash>().map_or_else(
            || w.resource::<Inventory>().count_by_record(rec),
            |st| gd_game::stash::count(&names, w.resource::<Inventory>(), st),
        )
    };
    use_it(&mut g);
    assert_eq!(open(&g), Some(e), "using the shrine opens its window");
    if slots(&g).iter().flatten().any(|(_, have)| !have) {
        answer(&mut g, ShrineRequest::Accept);
        g.step();
        assert!(
            !state(&g).restored.contains(&uid),
            "restored without offerings"
        );
        assert_eq!(
            open(&g),
            Some(e),
            "Offer without offerings keeps the window"
        );
    }
    answer(&mut g, ShrineRequest::Cancel);
    assert_eq!(open(&g), None);
    let offerings: Vec<String> = g
        .get::<Shrine>(e)
        .offerings
        .iter()
        .flatten()
        .cloned()
        .collect();
    assert!(!offerings.is_empty());
    for o in &offerings {
        g.app
            .world_mut()
            .resource_mut::<Inventory>()
            .add_to_bags(Item {
                base_name: o.clone(),
                stack_count: 2,
                ..Default::default()
            });
    }
    assert!(slots(&g).iter().flatten().all(|(_, have)| *have));
    let before = count(&g, &offerings[0]);
    let points = state(&g).points;
    use_it(&mut g);
    assert!(!state(&g).restored.contains(&uid), "opening takes nothing");
    answer(&mut g, ShrineRequest::Accept);
    g.step();
    assert!(state(&g).restored.contains(&uid));
    assert!(state(&g).points >= points);
    assert_eq!(open(&g), None);
    assert_eq!(
        count(&g, &offerings[0]),
        before - offerings.iter().filter(|o| *o == &offerings[0]).count() as u32,
        "one of each offering taken"
    );
}
