//! Stats and combat against the installed game database
//! ([`gd_sim::testdata`]).
use gd_sim::attributes::{AttrFamily, lookup};
use gd_sim::combat::{self, AttackerScaling, DamageScale, ReductionInfo, Rng, WeaponDamage};
use gd_sim::stats::*;

use gd_sim::testdata::db;

#[test]
fn level1_player() {
    let Some(db) = db() else { return };
    let formulas =
        CombatFormulas::from_record(db.get("records/game/combatformulas.dbr").unwrap()).unwrap();
    let pc = db.get("records/creatures/pc/malepc01.dbr").unwrap();
    let levels =
        LevelTable::from_record(db.get(pc.str("levelFileName").unwrap()).unwrap()).unwrap();
    let rates = levels.rates();
    assert_eq!(
        (
            rates.life_per_physique,
            rates.life_per_cunning,
            rates.life_per_spirit,
            rates.energy_per_spirit
        ),
        (2.5, 1.0, 1.5, 2.0)
    );

    let bio = BioStore::from_record(pc);
    let acc = accumulate(&bio, &rates, &Contributions::default());
    let s = CharacterSheet::compute(&acc, 1, &formulas);
    eprintln!("{s:?}");
    assert_eq!((s.physique, s.cunning, s.spirit), (50.0, 50.0, 50.0));
    assert_eq!((s.life, s.energy), (250.0, 250.0));
    // (65 + 12 + 50*0.5) + 53
    assert_eq!((s.offensive_ability, s.defensive_ability), (155.0, 155.0));

    // Level 2 with one point in Physique: +8 Physique, +20 life.
    let mut bio2 = bio.clone();
    bio2.allocate(
        &levels,
        AllocatedPoints {
            physique: 1,
            ..Default::default()
        },
    );
    let s2 = CharacterSheet::compute(
        &accumulate(&bio2, &rates, &Contributions::default()),
        2,
        &formulas,
    );
    assert_eq!((s2.physique, s2.life), (58.0, 270.0));
    assert_eq!(s2.defensive_ability, 65.0 + 24.0 + 29.0 + 53.0);

    // Starting gear: equipping a +Physique source converts to life (none in the
    // default gear, so just check the armor record is readable as Armor).
    let torso = db.get("records/items/geartorso/a00_torso001.dbr").unwrap();
    let prot = lookup("defensiveProtection").unwrap();
    assert_eq!(prot.family, AttrFamily::Armor);
    assert_eq!(torso.f32("defensiveProtection"), Some(12.0));

    // XP table: the equation at playerLevel 0 is 36.
    assert_eq!(levels.experience_for_level(1), 36);
    assert!(levels.experience_for_level(2) > 36);
    assert_eq!(levels.max_level, 100);
}

#[test]
fn player_hits_boar_with_starting_sword() {
    let Some(db) = db() else { return };
    let formulas =
        CombatFormulas::from_record(db.get("records/game/combatformulas.dbr").unwrap()).unwrap();
    let sword = db
        .get("records/items/gearweapons/swords1h/a00_sword001.dbr")
        .unwrap();
    let (min, max) = (
        sword.f32("offensivePhysicalMin").unwrap(),
        sword.f32("offensivePhysicalMax").unwrap(),
    );
    let ratio = sword.f32("offensivePierceRatioMin").unwrap_or(0.0);
    let mut rng = Rng(42);
    let mut w = WeaponDamage::roll(min, max, ratio, 1.0, &mut rng);
    assert!(w.physical[0] >= min && w.physical[0] <= max);
    let vars = CombatVars {
        dexterity: 50.0,
        ..Default::default()
    };
    w.process(
        &AttackerScaling {
            formulas: &formulas,
            vars,
        },
        &ReductionInfo::default(),
        DamageScale::default(),
        0.0,
    );
    let total = w.physical[0] + w.pierce[0];
    // Cunning 50 -> x(1 + 50/245) on both parts.
    assert!(
        total > min && total <= max * (50.0 / 245.0 + 1.0) + 1e-3,
        "{w:?}"
    );

    let boar = db.get("records/creatures/enemies/boar_a01.dbr").unwrap();
    let mut atk = combat::Attack {
        kind: combat::AttackKind::Melee,
        offensive_ability: 155.0,
        fumble_chance: 0.0,
        projectile_fumbled: false,
        unblockable: false,
        crit_damage_bonus: 0.0,
        energy_cost: 0.0,
        mana_burn_ratio: 0.0,
        damage: vec![combat::Damage::Weapon(w)],
    };
    let defenses: Vec<combat::Defense> = record_resistances(boar)
        .into_iter()
        .map(|(ty, value)| combat::Defense::Resistance {
            ty,
            value,
            modifier: 0.0,
            cap_bonus: 0.0,
            restricted: combat::restricted_resistance(ty),
        })
        .collect();
    let mut def = combat::Defender {
        defensive_ability: 100.0,
        dodge: 0.0,
        deflect: 0.0,
        defenses: &defenses,
        resistance_cap: 100.0,
        resistance_reduction: ReductionInfo::default(),
        armor_reduction: 0.0,
        damage_taken_pct: 100.0,
        reflection_resist: 0.0,
        reflect: 0.0,
        shield: None,
        absorb: Vec::new(),
        link_absorb: 0.0,
        block_cooldown_ms: 0,
        block_recovery_reduction: 0.0,
        block_modifier: 0.0,
        block_amount_modifier: 0.0,
        racial_reduction: 0.0,
        energy_absorption: 0.0,
        energy: 0.0,
        energy_max: 0.0,
        life: 100.0,
    };
    // Seed 42's next draws land the hit (OA 155 vs DA 100).
    let r = combat::take_attack(&formulas, &mut atk, &mut def, &mut rng);
    assert!(r.miss.is_none(), "seed 42 should hit: {r:?}");
    let dealt: f32 = r.applied.iter().map(|a| a.1).sum();
    assert!(
        dealt > 0.0 && (100.0 - def.life - dealt).abs() < 1e-3,
        "{r:?}"
    );
}
