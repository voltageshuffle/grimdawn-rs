//! Headless check of the ascendant altar: offering a formula's reagents
//! activates it (reagents taken, the formula installed); with too few it
//! stays dormant; the installed formulas reach the monster rules.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game::ascendant::activate_altar;
use gd_game::testing::TestGame;
use gd_game_core::ascendant::AltarState;
use gd_game_core::inventory::Inventory;

#[test]
fn altar_takes_reagents_and_installs() {
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    g.spawn_player();
    g.steps(2);
    let db = g.data().db;
    let ga = db
        .get(gd_sim::ascendant::GAME_ASCENDANT)
        .expect("gameascendant");
    let formula = ga
        .strs("altarInclusiveRecipes")
        .first()
        .map(|s| s.to_string())
        .expect("a formula");
    let f = db.get(&formula).unwrap();
    let reagent = f.str("reagentBaseBaseName").unwrap().to_owned();
    let q = f.i32("reagentBaseQuantity").unwrap_or(1).max(1) as u32;
    let mode = g
        .app
        .world()
        .resource::<gd_game_core::session::GameMode>()
        .0;
    let w = g.app.world_mut();
    w.resource_scope(|w, mut inv: Mut<Inventory>| {
        let mut state = w.resource_mut::<AltarState>();
        let have = inv.count_by_record(&reagent);
        inv.remove_by_record(&reagent, have);
        assert!(
            activate_altar(
                &db,
                mode,
                std::slice::from_ref(&reagent),
                &mut inv,
                &mut state
            )
            .is_err(),
            "no reagents"
        );
        assert!(!state.activated);
        for _ in 0..q {
            inv.add_to_bags(Item {
                base_name: reagent.clone(),
                stack_count: 1,
                seed: 1,
                ..Default::default()
            });
        }
        let levels = activate_altar(
            &db,
            mode,
            std::slice::from_ref(&reagent),
            &mut inv,
            &mut state,
        )
        .expect("activates");
        assert_eq!(levels, vec![(formula.clone(), 1)]);
        assert!(state.activated);
        assert_eq!(
            state.reagents.placed(),
            vec![reagent.clone()],
            "the boxes as activated"
        );
        assert_eq!(inv.count_by_record(&reagent), 0);
    });
    g.steps(1);
    let paks = g
        .app
        .world()
        .resource::<AltarState>()
        .formulas
        .monster_paks
        .clone();
    assert_eq!(
        g.app
            .world()
            .resource::<gd_game_core::rules::CombatRules>()
            .0
            .ascendant_paks,
        paks,
        "monster rules take the formulas' paks"
    );
}
