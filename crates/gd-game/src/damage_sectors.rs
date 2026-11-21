//! Damage sectors: the cells of the map's damage layer (sector layer 7, `DamageSectorData`,
//! [`gd_formats::map::sector::DamageSector`]) hurt a player standing in them. Stock: 18 entries in `world001.map`
//! (aether, poison, fire, chaos, cold, physical; 1 % to 30 %), such as the aether fields north of Homestead, which the
//! Shattered Realm's floors reuse.
//!
//! Game.dll `TickManager` (engine-wide, ctor @0x180583e80: 1000 ms period, factor 1.0; `Update`
//! @0x180583ea0 counts the frame time down and runs `Tick` @0x180583ee0 every 1000 ms, not while the engine is
//! paused): for each player, the layer-7 id of its cell in its own region's level (`SectorLayers::GetTargetId`);
//! with an entry whose damage × factor is above 0, a player that is not invincible (`Player::IsInvincible`) and
//! without an active purity skill (`SkillManager::IsPurityActive`: one of `gameengine.dbr` `puritySkillCheck`
//! toggled or running; stock lists none, so never):
//! - the controlled player takes `CombatManager::ApplyDamage(max life × damage, type)`: the combat filters
//!   (absorbs) but no `TakeAttack`, so no resistance ([`crate::afflictions::Afflictions::apply_damage`]), and
//!   `CancelAcceleratedLifeRegen` (the life loss stops the constitution regen, `crate::constitution`);
//! - the entry's effect record (stock: `records/fx/damagedefault/dmgabs_*_fx01.dbr`, `EffectEntity`) starts
//!   emitting at the player's centre of mass (placed, not attached). PORT: the `FxPak` branch of a record that is
//!   no `EffectEntity` is not ported (no stock use).
use crate::afflictions::Afflictions;
use crate::combat::{Combatant, Dead, Invincible};
use crate::data::GameData;
use crate::levels::Levels;
use crate::player::Player;
use bevy::prelude::*;

/// The sector layer of the damage sectors.
const DAMAGE_LAYER: usize = 7;

/// `TickManager` +0: ms to the next tick (module doc).
#[derive(Resource)]
pub struct TickManager(pub i32);

impl Default for TickManager {
    fn default() -> Self {
        Self(1000)
    }
}

pub struct DamageSectorsPlugin;

impl Plugin for DamageSectorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TickManager>().add_systems(
            Update,
            damage_tick
                .run_if(resource_exists::<Levels>)
                .run_if(resource_exists::<GameData>)
                .before(crate::afflictions::AfflictionStage::Tick),
        );
    }
}

/// `TickManager::Update` / `Tick` (module doc).
fn damage_tick(
    mut commands: Commands,
    (time, mut carry): (Res<Time>, Local<f32>),
    data: Res<GameData>,
    levels: Res<Levels>,
    mut ticks: ResMut<TickManager>,
    rt: Option<NonSend<crate::story::StoryRuntime>>,
    mut players: Query<
        (
            &Transform,
            &Combatant,
            &crate::character::Character,
            &mut Afflictions,
            Has<Invincible>,
        ),
        (With<Player>, Without<Dead>),
    >,
) {
    ticks.0 -= crate::session::frame_ms(&mut carry, &time);
    if ticks.0 >= 0 {
        return;
    }
    // One tick per elapsed period.
    let mut n = 0;
    while ticks.0 < 0 {
        ticks.0 += 1000;
        n += 1;
    }
    let region = crate::riftgates::player_region(rt.as_deref());
    for (tf, cb, ch, mut afflictions, invincible) in &mut players {
        let Some(sector) = sector_at(&levels, region, tf.translation) else {
            continue;
        };
        if sector.damage <= 0.0 || invincible {
            continue;
        }
        let Some(ty) = u8::try_from(sector.kind)
            .ok()
            .and_then(gd_sim::attributes::CombatType::from_id)
        else {
            warn!("damage sector: combat type {}", sector.kind);
            continue;
        };
        let at = tf.translation + tf.rotation * crate::monster_skills::character_center(&data, ch);
        for _ in 0..n {
            afflictions.apply_damage(ty, cb.profile.sheet.life * sector.damage);
            if !sector.record.is_empty() {
                crate::visuals::notify(&mut commands, &sector.record);
                commands.spawn((
                    crate::visuals::EffectSpec {
                        despawn_when_complete: true,
                        ..crate::visuals::EffectSpec::record(sector.record.clone())
                    },
                    Transform::from_translation(at),
                ));
            }
        }
    }
}

/// The damage sector at `pos`: the layer-7 entry of the cell in `region`'s level (else whichever loaded level
/// covers `pos`).
fn sector_at(
    levels: &Levels,
    region: Option<usize>,
    pos: Vec3,
) -> Option<&gd_formats::map::sector::DamageSector> {
    let guid = match region.filter(|r| *r != usize::MAX) {
        Some(r) => levels.sector_guid_in(r, pos, DAMAGE_LAYER)?,
        None => levels.sector_guid(pos, DAMAGE_LAYER)?,
    };
    levels.damages.iter().find(|d| d.base.ints == guid)
}
