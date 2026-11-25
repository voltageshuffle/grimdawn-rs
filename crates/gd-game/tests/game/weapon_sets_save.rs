//! Both weapon sets survive a save and a load (`gd_game::weapon_sets`): the
//! active set, the inactive one flagged and detached, the mouse slots in set
//! order.
use gd_formats::save::gdc::{Gdc, HotSlot};
use gd_formats::save::item::Item;
use gd_game::testing::{TempDir, TestGame};
use gd_game::weapon_sets::SwapWeapons;
use gd_game_core::casting::{Caster, DEFAULT_ATTACK, SLOT_LEFT};
use gd_game_core::inventory::Inventory;

// Level 1: a new character can wear it, so the active set II attaches it.
const SWORD: &str = "records/items/gearweapons/swords1h/a00_sword001.dbr";
const SHIELD: &str = "records/items/gearweapons/shields/a00_shield01.dbr";
const DAGGER: &str = "records/items/gearweapons/caster/c202_dagger.dbr";

fn slot_skill(g: &Gdc, i: usize) -> Option<String> {
    match g.ui.bars.first()?.slots.get(i)? {
        HotSlot::Skill { skill, .. } => Some(skill.clone()),
        _ => None,
    }
}

#[test]
fn both_sets_save_and_load() {
    let Some(mut new) = TestGame::new_character("Two Sets", false, false) else {
        return;
    };
    new.spawn_player();
    let tmp = TempDir::new("gd-weapon-sets-save");
    let dir = tmp.path();
    let mut src = Gdc::parse(&std::fs::read(new.save_now()).unwrap()).unwrap();
    drop(new);
    let item = |base: &str, seed| Item {
        base_name: base.into(),
        seed,
        stack_count: 1,
        ..Default::default()
    };
    let sword = item(SWORD, 5);
    let d = src.inventory.data.as_mut().unwrap();
    d.weapons[0].slots[0].item = item(DAGGER, 3);
    d.weapons[0].slots[1].item = Item {
        stack_count: 1,
        ..Default::default()
    };
    d.weapons[0].slots[1].attached = 0;
    d.weapons[1].slots[0].item = sword.clone();
    d.weapons[1].slots[1].item = item(SHIELD, 1);
    let path = dir.join("player.gdc");
    std::fs::write(&path, src.encode().unwrap()).unwrap();
    let left = slot_skill(&src, SLOT_LEFT);

    let mut g = TestGame::with_save(&path).expect("the game is installed");
    g.spawn_player();
    g.steps(2);
    g.app
        .world_mut()
        .write_message(SwapWeapons { from_window: true });
    g.steps(3);
    assert!(g.app.world().resource::<Inventory>().alternate);
    let written = g.save_now();
    let saved = Gdc::parse(&std::fs::read(&written).expect("player.gdc written")).unwrap();
    let d = saved.inventory.data.as_ref().unwrap();
    assert_eq!(d.use_alternate, 1);
    assert_eq!(
        (
            saved.info.alternate_config,
            saved.info.alternate_config_enabled
        ),
        (1, 1)
    );
    assert_eq!((d.weapons[0].flag, d.weapons[1].flag), (1, 0));
    assert_eq!(d.weapons[0].slots[0].item.base_name, DAGGER);
    assert_eq!(
        d.weapons[0].slots[0].attached, 0,
        "the inactive set is detached"
    );
    assert_eq!(d.weapons[1].slots[0].item, sword);
    assert_eq!(d.weapons[1].slots[1].item.base_name, SHIELD);
    assert_eq!(d.weapons[1].slots[0].attached, 1);
    // The mouse slots stay in set order: 10 = set I's left skill.
    assert_eq!(slot_skill(&saved, SLOT_LEFT), left);
    assert_eq!(
        slot_skill(&saved, SLOT_LEFT + 1).as_deref(),
        Some(DEFAULT_ATTACK)
    );

    // Loading it back: set II worn, set I kept aside, set II's mouse slots shown.
    let mut g2 = TestGame::with_save(&written).expect("loads");
    let p2 = g2.spawn_player();
    g2.steps(2);
    let inv = g2.app.world().resource::<Inventory>();
    assert!(inv.alternate && inv.alternate_enabled);
    assert_eq!(inv.equipped[12].as_ref(), Some(&sword));
    assert_eq!(
        inv.other_weapons[0].as_ref().map(|i| i.base_name.as_str()),
        Some(DAGGER)
    );
    assert_eq!(
        g2.get::<Caster>(p2).slots[SLOT_LEFT].as_deref(),
        Some(DEFAULT_ATTACK)
    );
    assert_eq!(g2.get::<Caster>(p2).slots[SLOT_LEFT + 1], left);
}
