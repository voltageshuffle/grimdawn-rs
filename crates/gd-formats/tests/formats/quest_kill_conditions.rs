//! The gdx3 bounties' kill conditions 28 / 29 (`qst::Condition::KillClassification` / `KillFaction`) as stored.
use gd_formats::qst::{Condition, Quest};
use gd_formats::testdata;
use gd_formats::vfs::Vfs;

fn first_kill_condition(vfs: &Vfs, name: &str) -> Condition {
    let q = Quest::parse(
        &vfs.read(&format!("quests/bountiesgdx3/{name}.qst"))
            .unwrap(),
    )
    .unwrap();
    q.tasks
        .iter()
        .flat_map(|t| &t.objectives)
        .flat_map(|o| o.conditions.iter().flat_map(|c| &c.list))
        .find(|c| matches!(c.kind, 28 | 29))
        .unwrap_or_else(|| panic!("{name}: no condition 28 / 29"))
        .cond
        .clone()
}

#[test]
fn gdx3_bounty_kill_conditions_decode() {
    let Some(root) = testdata::game() else { return };
    let vfs = Vfs::open(&root).unwrap();
    // Boss 3 / Quest 4 (`ResolveMonsterClassificationEnum`), 8 kills.
    assert_eq!(
        first_kill_condition(&vfs, "bq_av06"),
        Condition::KillClassification {
            classes: [3, 4],
            count: 8
        }
    );
    // Aetherials 2 / User12 18 (the Aetherial Vanguard pack's `myFaction`; `FactionPack::GetFactionFromString`),
    // 200 kills.
    assert_eq!(
        first_kill_condition(&vfs, "bq_av07"),
        Condition::KillFaction {
            factions: [2, 18],
            count: 200
        }
    );
    // Cthonians 3 / User18 24 (`factiongdx3_chthonickurn.dbr`).
    assert_eq!(
        first_kill_condition(&vfs, "bq_av08"),
        Condition::KillFaction {
            factions: [3, 24],
            count: 200
        }
    );
    // One faction, User16 22 (`factiongdx2_eldritch.dbr`): the second is -1000 (none).
    assert_eq!(
        first_kill_condition(&vfs, "bq_av09"),
        Condition::KillFaction {
            factions: [22, -1000i32 as u32],
            count: 200
        }
    );
}
