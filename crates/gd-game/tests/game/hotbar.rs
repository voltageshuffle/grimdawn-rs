//! Hotbar slot binding (`gd_game::hotbar`): the pick list's contents and
//! order, binding a potion and a scroll, using the scroll slot, and the
//! save / load of both options.
use crate::common::BLITZ;
use gd_formats::save::gdc::{Gdc, HotSlot};
use gd_formats::save::item::Item;
use gd_game::casting::{CastRequest, Caster, DEFAULT_ATTACK, HEALTH_POTION, SLOT_LEFT};
use gd_game::hotbar::{BindSlot, DEFAULT_SKILLS, choices};
use gd_game::inventory::Inventory;
use gd_game::player::{EffectiveSkills, Loadout};
use gd_game::testing::TestGame;

const OIL: &str = "records/items/crafting/consumables/oil_stoneheart.dbr";

#[test]
fn pick_list_bind_use_and_save() {
    let Some(mut g) = TestGame::new_character("Hot Bar", false, false) else {
        return;
    };
    let p = g.spawn_player();
    g.steps(2);
    // A learned skill.
    g.get_mut::<Loadout>(p)
        .skills
        .skills
        .push((BLITZ.to_owned(), 1));
    g.app
        .world_mut()
        .resource_mut::<Inventory>()
        .add_to_bags(Item {
            base_name: OIL.into(),
            stack_count: 3,
            ..Default::default()
        });
    g.step();
    let db = g.data().db;
    let list = {
        let w = g.app.world();
        choices(
            &db,
            g.get::<Loadout>(p),
            g.get::<EffectiveSkills>(p),
            g.get::<Caster>(p),
            w.get_resource::<Inventory>(),
            3,
        )
    };
    // The default skills first, in order; the scroll; the empty pick last.
    let names: Vec<&str> = list.iter().map(|o| o.as_deref().unwrap_or("")).collect();
    assert_eq!(&names[..6], &DEFAULT_SKILLS[..]);
    assert_eq!(list.last(), Some(&None));
    assert!(names.contains(&OIL));
    for sk in list.iter().flatten().filter(|s| !s.contains("/items/")) {
        let class = db.get(sk).unwrap().class.clone();
        let (pri, sec, auto) = gd_sim::skills::hot_slot_flags(&class);
        assert!((pri || sec) && !auto, "{sk} ({class}) listed");
    }
    assert!(
        names.contains(&BLITZ),
        "the learned skill is listed: {names:?}"
    );
    // The left mouse slot: primary skills only, no scroll, no empty pick.
    let left = choices(
        &db,
        g.get::<Loadout>(p),
        g.get::<EffectiveSkills>(p),
        g.get::<Caster>(p),
        g.app.world().get_resource::<Inventory>(),
        SLOT_LEFT,
    );
    assert!(left.iter().all(|o| o.is_some()) && left.iter().flatten().any(|s| s == DEFAULT_ATTACK));
    assert!(
        !left
            .iter()
            .flatten()
            .any(|s| s == HEALTH_POTION || s == OIL)
    );

    g.app.world_mut().write_message(BindSlot {
        slot: 3,
        option: Some(HEALTH_POTION.into()),
    });
    g.app.world_mut().write_message(BindSlot {
        slot: 4,
        option: Some(OIL.into()),
    });
    g.app.world_mut().write_message(BindSlot {
        slot: 5,
        option: None,
    });
    g.steps(2);
    assert_eq!(g.get::<Caster>(p).slots[4].as_deref(), Some(OIL));
    assert_eq!(g.get::<Caster>(p).slots[5], None);
    let before = g.app.world().resource::<Inventory>().count_by_record(OIL);
    g.app.world_mut().write_message(CastRequest {
        slot: 4,
        target: None,
        point: bevy::math::Vec3::ZERO,
        force: false,
        scripted: true,
    });
    g.steps(3);
    assert_eq!(
        g.app.world().resource::<Inventory>().count_by_record(OIL),
        before - 1,
        "the scroll slot uses one oil"
    );

    let written = g.save_now();
    let saved = Gdc::parse(&std::fs::read(&written).expect("player.gdc written")).unwrap();
    let bar = &saved.ui.bars[0].slots;
    assert_eq!(bar[3], HotSlot::Plain(2));
    assert!(matches!(&bar[4], HotSlot::Scroll { a, .. } if a == OIL));
    assert_eq!(bar[5], HotSlot::Empty);

    let mut g2 = TestGame::with_save(&written).expect("loads");
    let p2 = g2.spawn_player();
    g2.steps(2);
    assert_eq!(
        g2.get::<Caster>(p2).slots[3].as_deref(),
        Some(HEALTH_POTION)
    );
    assert_eq!(g2.get::<Caster>(p2).slots[4].as_deref(), Some(OIL));
}
