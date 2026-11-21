//! The challenge-area window (`hudChallengeAreaWindow`, `records/ui/challengearea/challengehud_table.dbr`): the
//! area's name on a banner at the top right while the player stands in a challenge area that shows the HUD
//! (`gd_game::challenge_areas`, [`super::HudValues::challenge_area`]).
//!
//! Grim Dawn.exe: the HUD update sub_14020D550 makes the
//! window visible exactly when the area under the player has `ChallengeArea::GetDisplayUI` and hands it the area
//! id; the window (vtable 0x140314a10) loads, after the common window fields (`sub_1402617D0`),
//! `challengeBackground` (a `BitmapSingle`), `challengeAreaName` (a `TextString`) and `challengeAreaRollover` (a
//! `RolloverLine2`) (Load sub_1401398D0); its update (sub_1401397B0) sets the text to the area's `nameTag` and
//! keeps `nameTag` / `descriptionTag` for the rollover; render (sub_140139640) draws the bitmap then the text at
//! the window origin; hover (sub_1401396D0) tests the text box and shows the rollover (line 1 the name, line 2
//! the description, the record's styles). The window is shifted with the Crucible / Shattered Realm panes for the
//! minimap (`survival.rs` module doc). The same update shows the info icon window with the area's mutators
//! (`survival.rs` `collect_mutators`).
use super::survival::{PaneRollovers, Zone, compass_mode, hud_shift};
use super::{HudValues, UiAssets, Widget};
use crate::data::GameData;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};

/// What the window shows: the area's id, its name and description tags and the mutator paks it rolled (the info
/// icon window's list).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChallengeHud {
    pub id: [i32; 4],
    pub name_tag: String,
    pub description_tag: String,
    pub mutators: Vec<String>,
}

impl ChallengeHud {
    /// Refreshes `cur` to the window's view of the player's area (`None`: none, or one that does not show the
    /// HUD); true when it changed. The same area (by id) only compares its mutators; the record is read when the
    /// area changes.
    pub fn refresh(
        cur: &mut Option<Self>,
        data: &GameData,
        area: Option<&gd_game::challenge_areas::InChallengeArea>,
        areas: &gd_game::challenge_areas::ChallengeAreas,
    ) -> bool {
        let mutators = |id: &[i32; 4]| areas.areas.get(id).map_or(&[][..], |a| &a.mutators[..]);
        if let (Some(c), Some(a)) = (cur.as_mut(), area)
            && c.id == a.id
        {
            let m = mutators(&a.id);
            if c.mutators == m {
                return false;
            }
            c.mutators = m.to_vec();
            return true;
        }
        let next = area
            .filter(|a| gd_game::challenge_areas::displays_ui(data, &a.record))
            .map(|area| {
                let rec = data.db.get(&area.record);
                let tag = |k: &str| rec.and_then(|r| r.str(k)).unwrap_or("").to_owned();
                Self {
                    id: area.id,
                    name_tag: tag("nameTag"),
                    description_tag: tag("descriptionTag"),
                    mutators: mutators(&area.id).to_vec(),
                }
            });
        if *cur == next {
            return false;
        }
        *cur = next;
        true
    }
}

#[derive(Component)]
struct ChallengeRoot;

pub struct ChallengeUiPlugin;

impl Plugin for ChallengeUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, build.in_set(crate::ui::UiPlaySet));
    }
}

/// The window's table record in the HUD master.
fn table(ui: &super::Ui) -> Option<gd_formats::arz::Record> {
    ui.record(super::hud::MASTER)
        .and_then(|m| m.str("hudChallengeAreaWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
}

/// Builds the window when the area (or the window size) changes; nothing while the player is in no displayed area.
#[allow(clippy::too_many_arguments)]
fn build(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    values: Res<HudValues>,
    roots: Query<Entity, With<ChallengeRoot>>,
    mut built: Local<Option<(UVec4, Option<ChallengeHud>)>>,
    mut assets: UiAssets,
    mut zones: ResMut<PaneRollovers>,
    // Option: created once the player is in (`survival::compass_mode`).
    compass: Option<Res<gd_game::compass::CompassMode>>,
) {
    if fresh.begun() {
        *built = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let mode = compass_mode(compass.as_deref());
    let view = &values.challenge_area;
    // The UI scale is a layout input too (an applied UI Scale option rebuilds).
    let sk = size
        .extend(mode as u32)
        .extend(assets.scale(size).to_bits());
    if size.x == 0 || built.as_ref().is_some_and(|(s, v)| *s == sk && v == view) {
        return;
    }
    *built = Some((sk, view.clone()));
    zones.challenge.clear();
    super::window::despawn_all(&mut commands, &roots);
    let Some(view) = view else { return };
    let root = crate::ui::spawn_hud_overlay(&mut commands, ChallengeRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let Some(table) = table(&ui) else { return };
    let field = |k: &str| table.str(k).unwrap_or("").to_owned();
    let pane = WindowDef::from_record(&table).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let origin = Vec2::new(pane.x, pane.y) + hud_shift(ui.s, mode);
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&field("challengeBackground")) {
        ui.bitmap(&bitmap, x, y, origin);
    }
    let name = ui.fonts.tag(&view.name_tag).to_owned();
    if let Some(r) = ui.record(&field("challengeAreaName")) {
        let tb = TextBox::from_record(&r);
        let b = tb.rect;
        ui.text(&tb, origin, &name);
        // The rollover: the name text's box (`sub_1401396D0`), lines from the `RolloverLine2` record's styles.
        let rollover = field("challengeAreaRollover");
        let style = |k: &str, or: &str| {
            ui.record(&rollover)
                .and_then(|r| r.str(k).map(str::to_owned))
                .unwrap_or_else(|| or.to_owned())
        };
        let mut lines = vec![(
            style(
                "Line1Style",
                "records/ui/styles/text/style_textwhite_sizen_bold.dbr",
            ),
            name.clone(),
            false,
        )];
        if !view.description_tag.is_empty() {
            lines.push((
                style(
                    "Line2Style",
                    "records/ui/styles/text/style_textgray_sizen.dbr",
                ),
                ui.fonts.tag(&view.description_tag).to_owned(),
                true,
            ));
        }
        zones.challenge.push(Zone {
            field: "challengeAreaRollover",
            rect: URect::new(
                origin.x + ui.sc(b.x),
                origin.y + ui.sc(b.y),
                ui.sc(b.w),
                ui.sc(b.h),
            ),
            record: rollover,
            lines: Some(lines),
        });
    }
}
