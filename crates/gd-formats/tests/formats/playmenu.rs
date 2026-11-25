//! `playmenu.cpn` round trip.
use gd_formats::save::cpn::PlayMenu;

#[test]
fn fields_round_trip() {
    let m = PlayMenu {
        character: "Tester".into(),
        game_name: "Grim Dawn Server".into(),
        host_name: "Grim Dawn Server".into(),
        level_name: "levels/world001.map".into(),
        difficulty: 2,
        challenge: 1,
        mode: 1,
        mod_name: "survivalmode".into(),
        level_name2: "maps/survivalworld_c.map".into(),
        survival_index: 3,
        ..Default::default()
    };
    let b = m.encode().unwrap();
    assert_eq!(PlayMenu::parse(&b).unwrap(), m);
}
