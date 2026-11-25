//! The Lua bindings' engine behaviour (see `story/lua/`), on a
//! small in-memory script set: `Script.Load` runs a file once, update
//! registrations fire on the next update and every period; and the PORT
//! sandbox (no `os`/`io`/file loading, instruction budget).
use gd_formats::collections::HashMap;
use gd_story::story::lua::{ScriptEnv, ScriptHost};
use gd_story::story::{Dialogs, Player, QuestDefs, Story};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

struct Env {
    files: HashMap<&'static str, &'static str>,
}

impl ScriptEnv for Env {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path).map(|s| s.as_bytes().to_vec())
    }
    fn record_field(&self, _record: &str, _field: &str) -> Option<String> {
        None
    }
}

const MAIN: &str = r#"
loads = (loads or 0) + 1
Script.Load("scripts/b.lua")
Script.Load("Scripts\\B.lua")
ticks = 0
function tick(id) ticks = ticks + 1 end
"#;

fn host() -> (Rc<RefCell<Story>>, ScriptHost) {
    let env = Env {
        files: HashMap::from_iter([
            ("scripts/main.lua", MAIN),
            ("scripts/b.lua", "bloads = (bloads or 0) + 1"),
        ]),
    };
    let story = Rc::new(RefCell::new(Story::new(
        Arc::new(QuestDefs::default()),
        Dialogs::default(),
        Player::new_character(1, "Bot"),
    )));
    let host = ScriptHost::new(story.clone(), Rc::new(env)).unwrap();
    (story, host)
}

fn eval<T: mlua::FromLua>(h: &ScriptHost, src: &str) -> T {
    h.lua.load(src).eval().unwrap()
}

#[test]
fn script_load_runs_once() {
    let (_, h) = host();
    assert_eq!(eval::<i64>(&h, "return bloads"), 1);
    assert!(eval::<bool>(&h, "return Script.Load('scripts/b.lua')"));
    assert_eq!(eval::<i64>(&h, "return bloads"), 1);
    assert!(!eval::<bool>(
        &h,
        "return Script.Load('scripts/missing.lua')"
    ));
}

#[test]
fn update_registrations() {
    let (story, h) = host();
    h.lua
        .load("Script.RegisterForUpdate(7, 'tick', 100) Script.RegisterForUpdate(7, 'nope', 100)")
        .exec()
        .unwrap();
    assert!(
        story
            .borrow_mut()
            .take_log()
            .iter()
            .any(|l| l.contains("function 'nope' does not exist"))
    );
    h.update(16); // countdown starts at 0: runs on the first update
    assert_eq!(eval::<i64>(&h, "return ticks"), 1);
    for _ in 0..6 {
        h.update(16);
    }
    assert_eq!(eval::<i64>(&h, "return ticks"), 1);
    h.update(16); // 7 * 16 > 100
    assert_eq!(eval::<i64>(&h, "return ticks"), 2);
    h.lua
        .load("Script.UnregisterForUpdate(7, 'tick')")
        .exec()
        .unwrap();
    for _ in 0..20 {
        h.update(16);
    }
    assert_eq!(eval::<i64>(&h, "return ticks"), 2);
}

/// PORT: a runaway script raises a Lua error at the instruction budget
/// instead of freezing the game; the VM keeps working afterwards.
#[test]
fn a_runaway_call_errors_at_the_instruction_budget() {
    let (story, h) = host();
    h.lua
        .load("function spin() while true do end end")
        .exec()
        .unwrap();
    assert!(h.call("spin", ()).is_none(), "the call errors out");
    assert!(
        story
            .borrow_mut()
            .take_log()
            .iter()
            .any(|l| l.contains("instruction budget"))
    );
    assert_eq!(eval::<i64>(&h, "return 1 + 1"), 2, "the VM still works");
}

/// PORT sandbox: no `os`, `io` or `package` library and no file loading;
/// `loadstring` stays.
#[test]
fn the_vm_has_no_os_io_or_file_loading() {
    let (_, h) = host();
    assert!(eval::<bool>(
        &h,
        "return os == nil and io == nil and package == nil \
         and dofile == nil and loadfile == nil and require == nil"
    ));
    assert!(
        h.lua.load("os.execute('true')").exec().is_err(),
        "os.execute is unreachable"
    );
    assert_eq!(eval::<i64>(&h, "return loadstring('return 2')()"), 2);
}
