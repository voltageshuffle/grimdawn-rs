//! Crucible sessions (`TestGame::crucible`): their own test binary, since a Crucible session turns the data
//! overlay on for the binary's shared `GameData`.
use bevy::prelude::*;
use gd_game::conversation::{Conversation, ConversationRequest};
use gd_game::testing::TestGame;
use gd_game_core::combat::Combatant;
use gd_game_core::skill_buffs::ReceivedBuffs;
use gd_game_core::story::{StoryActor, StoryRuntime};
use gd_story::story::state::{Coords, ObjClass, WorldObject};

mod arenas;
mod defenses;
mod difficulty;
mod restart;
mod stats;

const ARENA: &str = "maps/survivalworld_a.map";
const NPC: u32 = 900_200;
const POWERUPS: &str = "records/creatures/npcs/merchants/powerups_s01.dbr";
const HOUND: &str = "records/skills/playerclass03/summon_hellhound1.dbr";
const ZOMBIE: &str = "records/creatures/enemies/zombie_a01.dbr";

/// A Crucible game with the player, a Hellhound and Torralia (the power-ups NPC) 2 m away.
fn setup() -> Option<(TestGame, Entity, Entity)> {
    let mut g = TestGame::crucible(ARENA)?;
    g.flat_ground(Vec3::ZERO, 30.0);
    let p = g.spawn_player();
    g.steps(2);
    let conversation = g
        .data()
        .db
        .get(POWERUPS)
        .and_then(|r| r.str("conversation").map(str::to_owned))
        .expect("the Crucible's power-ups NPC (overlay record)");
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let mut story = rt.story.borrow_mut();
        story.entities.objects.insert(
            NPC,
            WorldObject {
                id: NPC,
                record: POWERUPS.into(),
                class: ObjClass::Npc,
                coords: Coords {
                    origin: [2.0, 0.0, 0.0],
                    axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                },
                in_world: true,
                alive: true,
                locked: false,
                open: false,
                broken: false,
                conversation: Some(conversation),
                available_for_conversation: true,
                scripted: false,
            },
        );
        story.player.tribute = 150;
    }
    g.app
        .world_mut()
        .spawn((Transform::from_xyz(2.0, 0.0, 0.0), StoryActor(NPC)));
    g.app
        .world_mut()
        .write_message(gd_game_core::casting::SpawnPet {
            owner: p,
            skill: HOUND.into(),
            level_index: 0,
            point: Vec3::new(-2.0, 0.0, 2.0),
            target: None,
        });
    g.steps(3);
    let mut q = g.app.world_mut().query::<(Entity, &gd_game::pets::Pet)>();
    let hound = q.iter(g.app.world()).next().expect("the hound").0;
    Some((g, p, hound))
}

fn tribute(g: &TestGame) -> u32 {
    g.app
        .world()
        .non_send::<StoryRuntime>()
        .story
        .borrow()
        .player
        .tribute
}

/// Opens Torralia's conversation and clicks the rows whose text contains each of `rows` in turn.
fn click(g: &mut TestGame, rows: &[&str]) {
    g.app
        .world_mut()
        .write_message(ConversationRequest::Talk(NPC));
    g.step();
    for want in rows {
        let v = g
            .app
            .world()
            .resource::<Conversation>()
            .view
            .clone()
            .expect("the conversation is open");
        let row = v
            .responses
            .iter()
            .find(|r| r.1.contains(want))
            .unwrap_or_else(|| panic!("no row {want:?} in {:?}", v.responses))
            .0;
        g.app
            .world_mut()
            .write_message(ConversationRequest::Choose(row));
        g.steps(3);
    }
}

fn has_buff(g: &TestGame, e: Entity, buff: &str) -> bool {
    g.app
        .world()
        .get::<ReceivedBuffs>(e)
        .is_some_and(|b| b.has(buff))
}

/// Each of the four blessings, bought from Torralia: 12 tributes, the buff record on the player and on the pet
/// (`Skill_BuffOther`: every friend of the NPC in `skillTargetRadius`), the profile changed, the power-up token.
#[test]
fn the_four_blessings() {
    let Some((mut g, p, hound)) = setup() else {
        return;
    };
    let blessings = [
        ("Blessing of Ulo", "blessingulo"),
        ("Empyrion's Guidance", "empyrionguidance"),
        ("Might of Amatok", "mightamatok"),
        ("Ulzuin's Pact", "ulzuinpact"),
    ];
    for (i, (row, name)) in blessings.iter().enumerate() {
        let buff = format!("records/skills/powerups/{name}_buff.dbr");
        let before = g.get::<Combatant>(p).profile.clone();
        let t = tribute(&g);
        click(&mut g, &["What are my options?", row]);
        g.steps(3);
        assert_eq!(tribute(&g), t - 12, "{row}: 12 tributes spent");
        assert!(has_buff(&g, p, &buff), "{row}: the player is blessed");
        assert!(has_buff(&g, hound, &buff), "{row}: the pet is blessed too");
        let after = g.get::<Combatant>(p).profile.clone();
        let (b, a) = (&before.sheet, &after.sheet);
        match i {
            0 => assert!(
                format!("{:?}", before.resistances) != format!("{:?}", after.resistances),
                "Ulo raises the resistances"
            ),
            1 => assert!(
                a.life > b.life * 1.5,
                "Empyrion: +80% life ({} -> {})",
                b.life,
                a.life
            ),
            2 => assert!(
                a.offensive_ability > b.offensive_ability + 70.0,
                "Amatok: +75 OA ({} -> {})",
                b.offensive_ability,
                a.offensive_ability
            ),
            _ => assert!(
                a.defensive_ability > b.defensive_ability + 30.0,
                "Ulzuin: +30 DA +5% ({} -> {})",
                b.defensive_ability,
                a.defensive_ability
            ),
        }
        let token = g
            .app
            .world()
            .non_send::<StoryRuntime>()
            .story
            .borrow()
            .player
            .tokens
            .contains("SURVIVALMODE_POWERUPACTIVE");
        assert!(token, "{row}: the power-up token");
    }
}

/// "I wish to prove my Devotion": one devotion point for the tier's price (3 / 5 / 10 / 18 ... tributes by the
/// points ever earned, `TotalDevotion`, the character's).
#[test]
fn the_devotion_purchase() {
    let Some((mut g, _, _)) = setup() else {
        return;
    };
    let devotion = |g: &TestGame| {
        let d = g.app.world().resource::<gd_game::devotion::DevotionState>();
        (d.points, d.total)
    };
    // Pin the price tier: a character past 40 points ever earned would silently skip the asserts.
    g.app
        .world_mut()
        .resource_mut::<gd_game::devotion::DevotionState>()
        .total = 0;
    let (points, total) = devotion(&g);
    let price = match total {
        0..5 => 3,
        5..10 => 5,
        10..25 => 10,
        25..40 => 18,
        _ => 0,
    };
    assert!(price > 0, "the tier price for {total} points ever earned");
    let t = tribute(&g);
    click(
        &mut g,
        &["prove my Devotion", &format!("({price} Tributes)")],
    );
    g.steps(2);
    assert_eq!(devotion(&g), (points + 1, total + 1), "one point");
    assert_eq!(tribute(&g), t - price, "{price} tributes spent");
}
