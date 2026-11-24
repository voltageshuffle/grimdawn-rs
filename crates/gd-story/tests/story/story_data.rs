//! Data test: every quest/conversation of base+gdx1-3 loads, the game's Lua
//! scripts run, and a new character plays the first steps of the main quest
//! ("Waking to Misery") the way the game wires it: spawn trigger → jailor
//! conversation → Bourbon (spawned by a ScriptEntity's Lua) → Burial Cave
//! token → Reanimator kill. Needs the install ([`gd_sim::testdata`]).
use gd_formats::arz::{RawValue, Record};
use gd_formats::collections::HashMap;
use gd_formats::vfs::{Database, Vfs};
use gd_story::story::lua::{ScriptEnv, ScriptHost};
use gd_story::story::quest::STATE_COMPLETE;
use gd_story::story::{Coords, Effect, Player, Story};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const WAKING: &str = "quests/mq_wakingtomisery.qst";
const T_INTRO: u32 = 0xb037_1500;
const T_CAVE: u32 = 0x92a0_c700;
const T_REANIMATOR: u32 = 0xa497_f000;
const T_RETURN: u32 = 0x3f73_dec0;

/// The install's story data as the script host sees it. Mirrors gd-game's
/// `DataEnv` (which lives above this crate): the loaders are gd-story's, and
/// the item footprint resolves through gd-sim's item rules (a dev-dependency).
struct Env {
    vfs: Arc<Vfs>,
    db: Arc<Database>,
    cache: RefCell<HashMap<String, Option<Arc<Record>>>>,
}

impl Env {
    fn record(&self, name: &str) -> Option<Arc<Record>> {
        let key = gd_formats::vfs::normalized(name);
        if let Some(r) = self.cache.borrow().get(key.as_ref()) {
            return r.clone();
        }
        let r = self.db.get_owned(&key).map(Arc::new);
        self.cache.borrow_mut().insert(key.into_owned(), r.clone());
        r
    }

    fn quest_defs(&self) -> anyhow::Result<gd_story::story::quest::QuestDefs> {
        gd_story::story::load::quest_defs(&self.vfs, "en")
    }

    fn dialogs(&self) -> anyhow::Result<gd_story::story::dialog::Dialogs> {
        gd_story::story::load::dialogs(&self.vfs, "en")
    }
}

impl ScriptEnv for Env {
    fn database(&self) -> Option<&Database> {
        Some(&self.db)
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.vfs.read(path).ok()
    }

    fn record_field(&self, record: &str, field: &str) -> Option<String> {
        let r = self.record(record)?;
        if field.eq_ignore_ascii_case("Class") {
            return Some(r.class.clone());
        }
        let vals = r.get(field)?;
        let parts: Vec<String> = vals
            .iter()
            .map(|v| match v {
                RawValue::Str(s) => s.to_string(),
                RawValue::Int(i) => i.to_string(),
                RawValue::Float(f) => f.to_string(),
                RawValue::Bool(b) => (*b as i32).to_string(),
            })
            .collect();
        Some(parts.join(";"))
    }

    fn item_footprint(&self, record: &str) -> Option<gd_formats::inventory::Footprint> {
        use gd_formats::inventory::{Footprint, bitmap_footprint};
        let item = gd_formats::save::item::Item {
            base_name: record.to_string(),
            ..Default::default()
        };
        let r =
            gd_sim::items::resolve::resolve(&gd_sim::items::ItemDb::new(self.db.clone()), &item)?;
        Some(bitmap_footprint(&self.vfs, &r.bitmap).unwrap_or(Footprint { w: 1, h: 1 }))
    }
}

/// The install's story data (archives and database opened once per binary).
fn env() -> Option<Env> {
    Some(Env {
        vfs: gd_sim::testdata::vfs()?,
        db: gd_sim::testdata::db()?,
        cache: RefCell::new(HashMap::default()),
    })
}

fn task_state(story: &Story, uid: u32) -> u32 {
    let qi = story.quests.defs.by_path(WAKING).unwrap();
    let ti = story.quests.task_index(qi, uid).unwrap();
    story.quests.task_state(qi, ti)
}

/// Talks to `npc`, always taking the first Accept, else the first response,
/// until the conversation ends. Returns the speech texts seen.
fn converse(host: &ScriptHost, npc: u32) -> Vec<String> {
    let mut seen = Vec::new();
    let mut view = host.talk(npc);
    for _ in 0..20 {
        let Some(v) = view else { break };
        seen.push(v.text.clone());
        let pick = v
            .responses
            .iter()
            .find(|r| r.2 == "tagAccept")
            .or(v.responses.first());
        let Some(&(idx, _, _)) = pick else { break };
        view = host.choose(idx);
    }
    seen
}

#[test]
fn opening_of_the_main_quest() {
    let Some(env) = env() else { return };
    let env = Rc::new(env);
    let defs = env.quest_defs().unwrap();
    let dialogs = env.dialogs().unwrap();
    println!(
        "{} quests, {} conversations",
        defs.list.len(),
        dialogs.defs.len()
    );
    assert!(defs.list.len() >= 400 && dialogs.defs.len() >= 550);

    let mut player = Player::new_character(1, "RefBot");
    // Devil's Crossing spawn point (world001.map chunk 0x10).
    player.position = [59.175232, 7.7474318, 97.206215];
    let story = Rc::new(RefCell::new(Story::new(Arc::new(defs), dialogs, player)));
    let host = ScriptHost::new(story.clone(), env.clone()).unwrap();
    let log = story.borrow_mut().take_log();
    let failures: Vec<_> = log
        .iter()
        .filter(|l| l.contains("Failed to load") || l.contains("Lua error"))
        .collect();
    assert!(failures.is_empty(), "script load failures: {failures:#?}");
    // main.lua defines the quest tables.
    assert!(
        host.function("gd.quests.devilsCrossing.johnBourbon.introOnAddToWorld")
            .is_some()
    );
    assert!(host.function("QuestEventDispatch").is_some());

    // Level objects around the spawn (positions from Region0A001.lvl).
    host.add_object(
        100,
        "records/creatures/npcs/flavornpcs/devilscrossing/jailor01.dbr",
        Coords::at([58.46, 7.97, 92.23]),
    );
    host.add_object(
        101,
        "records/triggervolumes/devilscrossing_spawnarea.dbr",
        Coords::at([58.67, 7.75, 97.21]),
    );
    host.add_object(
        102,
        "records/storyelements/scriptentity/dc_bourbonintro.dbr",
        Coords::at([81.78, 7.87, 44.32]),
    );

    // Bourbon's ScriptEntity swapped in the Bourbon NPC through Lua.
    let effects = story.borrow_mut().take_effects();
    let bourbon = effects
        .iter()
        .find_map(|e| match e {
            Effect::Spawn { id, record, .. } if record.contains("npc_johnbourbon_01") => Some(*id),
            _ => None,
        })
        .expect("Bourbon spawned by introOnAddToWorld");
    // The host would now register the spawned NPC with its record data.
    let coords = story.borrow().entities.get(bourbon).unwrap().coords;
    host.add_object(
        bourbon,
        "records/creatures/npcs/questnpcs/npc_johnbourbon_01.dbr",
        coords,
    );

    // Spawn area trigger: the salt necklace for a level-1 character on Normal.
    for _ in 0..5 {
        host.update(100);
    }
    assert!(story.borrow().player.tokens.contains("SALT_NECKLACE"));
    assert_eq!(
        story
            .borrow()
            .player
            .item_count("records/items/gearaccessories/necklaces/a00_necklace.dbr"),
        1
    );

    // Jailor: "Still drawing breath..." → accept → Intro task.
    let seen = converse(&host, 100);
    println!("jailor: {seen:#?}");
    assert!(seen[0].starts_with("Still drawing breath"));
    assert_eq!(task_state(&story.borrow(), T_INTRO), 2);
    let journal = story.borrow().quests.journal();
    println!("journal: {journal:#?}");
    assert!(journal.iter().any(|j| {
        j.quest == "Waking to Misery"
            && j.objectives
                .iter()
                .any(|(o, _)| o == "Speak to Captain John Bourbon at Devil's Crossing")
    }));

    // Bourbon: completes Intro, starts "Find The Burial Cave".
    let seen = converse(&host, bourbon);
    println!("bourbon: {seen:#?}");
    assert_eq!(task_state(&story.borrow(), T_INTRO), STATE_COMPLETE);
    assert_eq!(task_state(&story.borrow(), T_CAVE), 2);

    // Entering the cave grants the token; the 500 ms quest update completes the task.
    story.borrow_mut().give_token("WM_BURIAL_CAVE_FOUND");
    host.update(600);
    assert_eq!(task_state(&story.borrow(), T_CAVE), STATE_COMPLETE);
    assert_eq!(task_state(&story.borrow(), T_REANIMATOR), 2);
    let effects = story.borrow_mut().take_effects();
    assert!(effects.contains(&Effect::CinematicText(
        "tagNotification_WightmireBurialCaveFound".into()
    )));

    // Kill Kyzogg the Reanimator.
    host.add_object(
        200,
        "records/creatures/enemies/boss&quest/reanimator.dbr",
        Coords::at([0.0; 3]),
    );
    host.on_kill(200);
    host.update(600);
    assert_eq!(task_state(&story.borrow(), T_REANIMATOR), STATE_COMPLETE);
    assert_eq!(task_state(&story.borrow(), T_RETURN), 2);

    let log = story.borrow_mut().take_log();
    let errors: Vec<_> = log
        .iter()
        .filter(|l| l.contains("Lua error") || l.contains("does not exist"))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    println!("log: {log:#?}");
}

#[test]
fn quest_save_round_trip() {
    let Some(env) = env() else { return };
    let defs = Arc::new(env.quest_defs().unwrap());
    let mut a = Story::new(
        defs.clone(),
        env.dialogs().unwrap(),
        Player::new_character(1, "RefBot"),
    );
    let qi = defs.by_path(WAKING).unwrap();
    a.give_token("TEST_TOKEN");
    a.begin_task(qi, T_INTRO, false);
    a.complete_task(qi, T_INTRO);
    a.begin_task(qi, T_REANIMATOR, false);
    a.on_kill(&gd_story::story::Kill::record_only(
        "records/creatures/enemies/boss&quest/reanimator.dbr",
    ));
    let saved = a.save_quests(0, None);
    let bytes = saved.encode().unwrap();
    let parsed = gd_formats::save::gdd::Quests::parse(&bytes).unwrap();
    assert_eq!(parsed, saved);
    let mut b = Story::new(
        defs.clone(),
        env.dialogs().unwrap(),
        Player::new_character(1, "RefBot"),
    );
    b.load_quests(&parsed, 0);
    // Saved per task: state, the two flags and the counters (objective
    // satisfaction is re-evaluated by the game, not saved).
    let view = |s: &Story| -> Vec<_> {
        s.quests
            .rt
            .iter()
            .flat_map(|q| {
                q.tasks.iter().map(|t| {
                    (
                        t.state,
                        t.finished,
                        t.rewarded,
                        t.objectives
                            .iter()
                            .map(|o| o.counters.clone())
                            .collect::<Vec<_>>(),
                    )
                })
            })
            .collect()
    };
    assert_eq!(view(&a), view(&b));
    assert_eq!(a.player.tokens, b.player.tokens);
    assert_eq!(b.quests.active.len(), 1);

    // Saving with the loaded file as template is byte-identical.
    assert_eq!(b.save_quests(0, Some(&parsed)).encode().unwrap(), bytes);
}

#[test]
fn every_script_compiles() {
    let Some(dir) = gd_sim::testdata::game() else {
        return;
    };
    let mut total = 0;
    let mut bad = Vec::new();
    for layer in gd_formats::vfs::LAYERS {
        let p = dir.join(layer).join("resources/Scripts.arc");
        let Ok(arc) = gd_formats::arc::Arc::open(&p) else {
            continue;
        };
        let lua = mlua::Lua::new();
        for e in &arc.entries {
            if !e.name.ends_with(".lua") {
                continue;
            }
            total += 1;
            let src =
                gd_story::story::syntax::to_lua51(&String::from_utf8_lossy(&arc.read(e).unwrap()));
            if let Err(err) = lua.load(&src).set_name(e.name.as_str()).into_function() {
                bad.push(format!("{layer}/{}: {err}", e.name));
            }
        }
    }
    println!("{total} scripts");
    assert!(total >= 280);
    assert!(bad.is_empty(), "{bad:#?}");
}
