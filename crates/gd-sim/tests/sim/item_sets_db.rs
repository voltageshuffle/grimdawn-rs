//! Item sets' skill bonuses (`gd_sim::items::sets` docs) against the
//! installed game database: Mogdrogen's Tranquility (`itemset_c027`).
use gd_formats::save::item::Item;
use gd_sim::items::sets::set_skill_bonuses;
use gd_sim::testdata::db;

const PIECES: [&str; 4] = [
    "records/items/gearhead/c032_head.dbr",
    "records/items/geartorso/c032_torso.dbr",
    "records/items/gearaccessories/necklaces/c039_necklace.dbr",
    "records/items/gearweapons/melee2h/c021_axe2h.dbr",
];
const BRIARTHORN: &str = "records/skills/playerclass06/summon_briarthorn1.dbr";

fn worn(n: usize) -> Vec<Item> {
    PIECES[..n]
        .iter()
        .map(|b| Item {
            base_name: (*b).into(),
            stack_count: 1,
            ..Default::default()
        })
        .collect()
}

/// `augmentSkillLevel1 0;0;3;3` (+3 Briarthorn from 3 pieces),
/// `augmentMasteryLevel1 0;0;0;1` (+1 Shaman with 4).
#[test]
fn set_skill_bonuses_follow_the_count() {
    let Some(db) = db() else { return };
    let (sk, ms, _) = set_skill_bonuses(&db, &worn(2));
    assert!(sk.is_empty() && ms.is_empty());
    let (sk, ms, _) = set_skill_bonuses(&db, &worn(3));
    assert!(sk.contains(&(BRIARTHORN.into(), 3)), "{sk:?}");
    assert!(ms.is_empty());
    let (_, ms, _) = set_skill_bonuses(&db, &worn(4));
    assert_eq!(
        ms,
        vec![(
            "records/skills/playerclass06/_classtraining_class06.dbr".into(),
            1
        )]
    );
    // On top of the pieces' own bonuses.
    let briar = |v: &[(String, u32)]| {
        v.iter()
            .filter(|(s, _)| s == BRIARTHORN)
            .map(|s| s.1)
            .sum::<u32>()
    };
    let pieces: u32 = worn(3)
        .iter()
        .map(|i| briar(&gd_sim::skills::item_skill_bonuses(&db, i).0))
        .sum();
    let (sk, _, _) = gd_sim::skills::item_augments(&db, &worn(3));
    assert_eq!(briar(&sk), pieces + 3);
}
