//! The Crucible's difficulty ramp (`Rules::with_survival_ramp`): a monster with a ramp adds the
//! survival adjustment pak at list index `ramp - 1`; ramp 0 changes nothing; players take none.
use gd_formats::testdata;
use gd_formats::vfs::{Database, SURVIVAL_MOD};
use gd_sim::actor::{Rules, Side, monster};

const ZOMBIE: &str = "records/creatures/enemies/zombie_a01.dbr";

#[test]
fn ramp_adds_the_survival_pak() {
    let Some(root) = testdata::game() else { return };
    let db = Database::open(&root).unwrap();
    db.initialize_mod(&root, SURVIVAL_MOD).unwrap();
    let base = Rules::load(&db).unwrap().with_difficulty(0);
    let at = |ramp: u32| {
        let rules = base.with_survival_ramp(&db, ramp);
        monster(&db, &rules, ZOMBIE, 30, Side::Monster).unwrap()
    };
    let (r0, r1, r8) = (at(0), at(1), at(8));
    let life = |p: &gd_sim::actor::Profile| p.sheet.life;
    assert_eq!(
        life(&r0),
        life(&monster(&db, &base, ZOMBIE, 30, Side::Monster).unwrap())
    );
    assert!(
        r8.totals != r0.totals && r1.totals != r0.totals && r8.totals != r1.totals,
        "the pak's columns add attributes, a later ramp a different column"
    );
    assert!(life(&r8) > life(&r1), "a higher ramp reads a later column");
    // The pak is the difficulty's (Normal): `survivalAdjustmentNormal`.
    assert!(base.with_survival_ramp(&db, 1).challenge.is_some());
    assert!(base.with_survival_ramp(&db, 0).challenge.is_none());
    // Elite and Ultimate pick their own paks.
    let elite = Rules::load(&db)
        .unwrap()
        .with_difficulty(1)
        .with_survival_ramp(&db, 1);
    assert_ne!(
        elite.challenge.as_ref().map(|r| r.name.clone()),
        base.with_survival_ramp(&db, 1).challenge.map(|r| r.name)
    );
}
