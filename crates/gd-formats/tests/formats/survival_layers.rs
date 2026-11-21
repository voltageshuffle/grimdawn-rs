//! The Crucible's packs are a mod mounted over the install and only visible while mounted
//! (`vfs::SURVIVAL_LAYERS`, `Vfs::initialize_mod`).
use gd_formats::testdata;
use gd_formats::vfs::{Database, SURVIVAL_MOD, Vfs};

#[test]
fn survival_mod_mounts_and_unmounts() {
    let Some(root) = testdata::game() else { return };
    let vfs = Vfs::open(&root).unwrap();
    let waves = "scripts/game/survival/tier01waves.lua";
    let quest_events = "scripts/game/questevents.lua";
    // No mod (the campaign): the survival scripts are invisible, the stock ones read.
    assert!(!vfs.exists(waves));
    let campaign = vfs.read(quest_events).unwrap();
    vfs.initialize_mod(SURVIVAL_MOD).unwrap();
    assert!(vfs.exists(waves));
    // The later pack's script (survivalmode3) and lower-case archive names (survivalmode2) mount too.
    assert!(vfs.exists("scripts/game/survival/tier20waves.lua"));
    assert_ne!(
        vfs.read(quest_events).unwrap(),
        campaign,
        "mod script replaces the stock one"
    );
    assert!(vfs.paths().any(|p| p == waves));
    vfs.remove_mod();
    assert!(!vfs.exists(waves));
    assert_eq!(vfs.read(quest_events).unwrap(), campaign);

    let db = Database::open(&root).unwrap();
    let stock_len = db.len();
    let stock_rec = db
        .get("records/controllers/enemy/controller_a1m_001.dbr")
        .unwrap();
    db.initialize_mod(&root, SURVIVAL_MOD).unwrap();
    assert!(db.len() > stock_len);
    assert_ne!(
        db.get("records/controllers/enemy/controller_a1m_001.dbr")
            .unwrap()
            .fields,
        stock_rec.fields,
        "mod record replaces the stock one"
    );
    db.remove_mod();
    assert_eq!(db.len(), stock_len);
}
