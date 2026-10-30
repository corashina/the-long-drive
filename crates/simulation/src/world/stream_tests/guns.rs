//! X02 part 3: gun shots. A revolver shot breaks a bottle. Shotgun pellets push an item.
//! Reload and unload keep the rounds. A magazine swap saves and loads.
use super::drive_out::{frame, settle};
use super::melee::{CROWBAR, place, spawn, weapon};
use super::*;
use crate::damage::BreakKind;
use crate::interaction::guns::{AmmoItem, GunSnapshot, GunState, fr_allows};
use crate::interaction::items::ItemLocation;
use crate::interaction::spray::Ammo;
use crate::reference::{GunKind, GunReference, ItemId};
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::vehicle::to_rapier;
use rapier3d::math::{Pose, Vector};

/// Frames from one trigger press to the next.
const PRESS_FRAMES: usize = 20;

/// Spawns the gun prototype at the player, puts it in the right hand and fills its ammo.
fn equip(run: &mut GarageRuntime, reference: &ReferenceManifest, gun: &GunReference) -> ItemId {
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.id == gun.prototype)
        .unwrap();
    let pickup = reference.item_prototypes[prototype]
        .pickups
        .iter()
        .position(|p| p.pickup.source == gun.pickup)
        .unwrap();
    let pose = run.area.player.body_pose(&run.physics);
    let item = crate::starting_area::spawned::spawn_prototype(
        reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        pose,
    )
    .unwrap()[pickup];
    super::g08_pickup(run, item);
    assert!(run.area.equip_carried(&mut run.physics).unwrap());
    settle(run, reference, 1.);
    super::g08_frame(run, reference, FrameInput::default(), FIXED_DT);
    let rt = run
        .area
        .interaction
        .guns
        .runtime_mut(item)
        .expect("gun runtime");
    rt.ammo.count = rt.ammo.maximum.max(1.);
    item
}

/// `Th` in physics space.
fn view(run: &GarageRuntime) -> Pose {
    to_rapier(run.area.player.view(&run.physics).head_pose)
}

/// One frame of repeated trigger presses.
fn trigger_frame(run: &mut GarageRuntime, reference: &ReferenceManifest, index: usize) {
    let input = if index % PRESS_FRAMES == 0 {
        FrameInput {
            held: keys::PRIMARY,
            pressed: keys::PRIMARY,
            ..Default::default()
        }
    } else {
        FrameInput::default()
    };
    super::g08_frame(run, reference, input, FIXED_DT);
}

#[test]
#[ignore = "requires the installed source export"]
fn revolver_shot_breaks_a_bottle() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let records = &reference.breakables;
    let (target, bottle) = run
        .stream
        .breakables
        .targets()
        .iter()
        .enumerate()
        .filter_map(|(i, t)| match t.kind {
            BreakKind::Item(item) => Some((i, t.record, item)),
            _ => None,
        })
        .filter(|(_, record, item)| {
            let r = &records[*record];
            r.source != reference.acquisition.r#box.breakable
                && !(r.no_destroy || r.no_collider || r.no_shoot)
                && r.health <= 6.
                && run
                    .area
                    .items
                    .get(*item)
                    .is_some_and(|s| s.location == ItemLocation::World && !s.buried)
        })
        .map(|(i, _, item)| (i, item))
        .next()
        .expect("bottle near the start");
    let revolver = reference
        .guns
        .weapons
        .iter()
        .find(|w| matches!(w.kind, GunKind::Revolver { .. }))
        .expect("revolver");
    let item = equip(&mut run, &reference, revolver);
    let pickup = &run.area.interaction.carry.rule(item).unwrap().source;
    assert!(run.area.interaction.guns.weapon_for(pickup).is_some());
    for i in 0..5 * PRESS_FRAMES {
        if run.stream.breakables.targets()[target].destroyed {
            break;
        }
        let view = view(&run);
        place(&mut run, bottle, view * Vector::new(0., 0., -1.5));
        trigger_frame(&mut run, &reference, i);
        frame(&mut run, &reference);
    }
    assert!(
        run.stream.breakables.targets()[target].destroyed,
        "the revolver shot breaks the bottle"
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn shotgun_pellets_push_an_item() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let shotgun = reference
        .guns
        .weapons
        .iter()
        .find(|w| w.shot_count > 1)
        .expect("shotgun");
    equip(&mut run, &reference, shotgun);
    let target = spawn(&mut run, &reference, weapon(&reference, CROWBAR));
    let body = run.area.items.get(target).unwrap().body.unwrap();
    let mut pushed = false;
    for i in 0..5 * PRESS_FRAMES {
        let view = view(&run);
        if i % PRESS_FRAMES == 0 {
            place(&mut run, target, view * Vector::new(0., 0., -1.));
        }
        trigger_frame(&mut run, &reference, i);
        let forward = view.rotation * -Vector::Z;
        let velocity = run.physics.rapier().bodies[body].linvel();
        if i % PRESS_FRAMES == 0 && velocity.dot(forward) > 0.1 {
            pushed = true;
            break;
        }
        frame(&mut run, &reference);
    }
    assert!(pushed, "the pellets push the item away from the gun");
}

/// A large box of the gun's ammo type.
fn ammo_box(gun: &GunReference, count: f32) -> AmmoItem {
    AmmoItem {
        ammo: Ammo {
            count,
            maximum: 1000.,
            float: gun.ammo.float_or_int,
            kind: gun.ammo.r#type,
        },
        is_mag: false,
    }
}

/// An FR value where the gun's reload rule holds.
fn reload_fr(gun: &GunReference) -> f32 {
    [0., 1., gun.fr.reload.value, 0.5]
        .into_iter()
        .find(|v| fr_allows(&gun.fr.reload, *v))
        .expect("reload FR value")
}

#[test]
#[ignore = "requires the installed source export"]
fn reload_and_unload_keep_round_totals() {
    let reference = super::reference();
    let mut guns = GunState::new(&reference).unwrap();
    for (i, w) in reference.guns.weapons.iter().enumerate() {
        if matches!(w.kind, GunKind::Magazine { .. } | GunKind::None) {
            continue;
        }
        let item = ItemId(1_000_000 + i as u32);
        assert!(guns.bind(item, i, None, Some(0.)));
        guns.runtime_mut(item).unwrap().fr = reload_fr(w);
        let mut other = ammo_box(w, 100.);
        let total = |g: &GunState, o: &AmmoItem| g.runtime(item).unwrap().ammo.count + o.ammo.count;
        let start = total(&guns, &other);
        let mut loads = 0;
        while guns.can_reload(item, &other) {
            guns.reload(item, &mut other, false, false, [0.; 3]);
            assert_eq!(total(&guns, &other), start, "a reload keeps the rounds");
            loads += 1;
            assert!(loads < 1000, "each reload moves rounds");
        }
        let rt = guns.runtime(item).unwrap();
        assert_eq!(rt.ammo.count, rt.ammo.maximum, "reload fills the gun");
        if let GunKind::Revolver { chambers, .. } = &w.kind {
            assert_eq!(
                rt.chambers.count_ones(),
                (rt.ammo.count as u32).min(chambers.len() as u32),
                "one full chamber per round"
            );
        }
        while guns.can_unload(item, &other) {
            guns.reload(item, &mut other, true, false, [0.; 3]);
            assert_eq!(total(&guns, &other), start, "an unload keeps the rounds");
            loads += 1;
            assert!(loads < 2000, "each unload moves rounds");
        }
        let rt = guns.runtime(item).unwrap();
        assert_eq!(rt.ammo.count, 0., "unload empties the gun");
        if matches!(w.kind, GunKind::Revolver { .. }) {
            assert_eq!(rt.chambers, 0, "unload empties the chambers");
        }
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn magazine_swap_saves_and_reloads() {
    let reference = super::reference();
    let (index, w) = reference
        .guns
        .weapons
        .iter()
        .enumerate()
        .find(|(_, w)| matches!(w.kind, GunKind::Magazine { .. }))
        .expect("magazine gun");
    let item = ItemId(1_000_000);
    let mut guns = GunState::new(&reference).unwrap();
    assert!(guns.bind(item, index, None, Some(5.)));
    let rt = guns.runtime_mut(item).unwrap();
    rt.fr = reload_fr(w);
    let (had, before) = (rt.has_mag, rt.ammo.count);
    let mut mag = AmmoItem {
        ammo: Ammo {
            count: 7.,
            maximum: rt.ammo.maximum,
            float: false,
            kind: w.ammo.r#type,
        },
        is_mag: true,
    };
    assert!(guns.can_reload(item, &mag), "a magazine fits its gun");
    let used = guns.reload(item, &mut mag, false, false, [0.; 3]).used;
    let rt = guns.runtime(item).unwrap();
    assert_eq!(used, !had, "a gun without a magazine uses the item up");
    assert!(rt.has_mag);
    assert_eq!(rt.ammo.count, 7.);
    if had {
        assert_eq!(
            mag.ammo.count, before,
            "the old magazine keeps the gun's rounds"
        );
    }
    let saved = guns.snapshot(item).expect("changed gun state");
    let json = serde_json::to_string(&saved).unwrap();
    let loaded: GunSnapshot = serde_json::from_str(&json).unwrap();
    let mut restored = GunState::new(&reference).unwrap();
    assert!(restored.bind(item, index, Some(loaded), Some(5.)));
    assert_eq!(restored.runtime(item).unwrap().snapshot(), saved);
}

#[test]
#[ignore = "requires the installed source export"]
fn x02_leftovers_smg_unload_drops_the_magazine() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let smg = reference
        .guns
        .weapons
        .iter()
        .find(|w| matches!(w.kind, GunKind::Magazine { .. }))
        .expect("magazine gun");
    let GunKind::Magazine { mag_prefab, .. } = &smg.kind else {
        unreachable!()
    };
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| &p.source == mag_prefab)
        .expect("magazine item prototype");
    let gun = equip(&mut run, &reference, smg);
    run.area
        .interaction
        .guns
        .runtime_mut(gun)
        .unwrap()
        .ammo
        .count = 17.;
    let pose = run.area.player.body_pose(&run.physics);
    let loose = crate::starting_area::spawned::spawn_prototype(
        &reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        pose,
    )
    .unwrap()[0];
    let full = run.area.spawned.ammo(loose).unwrap().ammo.count;
    let mags = |run: &GarageRuntime| -> Vec<ItemId> {
        run.area
            .spawned
            .iter()
            .filter(|s| s.prototype == prototype)
            .map(|s| s.id)
            .collect()
    };

    // E at loose ammo: the magazine drops with the gun rounds.
    run.reload_gun(&reference, gun, loose, true);
    let rt = run.area.interaction.guns.runtime(gun).unwrap();
    assert!(!rt.has_mag && rt.ammo.count == 0., "the gun is empty");
    assert_eq!(run.area.spawned.ammo(loose).unwrap().ammo.count, full);
    let dropped = *mags(&run)
        .iter()
        .find(|id| **id != loose)
        .expect("a dropped magazine");
    assert_eq!(run.area.spawned.ammo(dropped).unwrap().ammo.count, 17.);

    // R at the dropped magazine: it goes into the gun and is used up.
    run.reload_gun(&reference, gun, dropped, false);
    let rt = run.area.interaction.guns.runtime(gun).unwrap();
    assert!(rt.has_mag && rt.ammo.count == 17.);
    assert!(
        !run.area.spawned.contains(dropped),
        "the used magazine is gone"
    );

    // A dropped magazine saves its rounds.
    run.reload_gun(&reference, gun, loose, true);
    let dropped = *mags(&run).iter().find(|id| **id != loose).unwrap();
    let pose = run.area.items.get(dropped).unwrap().pose;
    let loaded = super::explosions::save_and_load(&run, &reference);
    let counts: Vec<f32> = mags(&loaded)
        .iter()
        .filter_map(|id| loaded.area.spawned.ammo(*id))
        .map(|a| a.ammo.count)
        .collect();
    assert!(
        counts.contains(&17.),
        "the dropped magazine keeps 17 rounds after load"
    );
    assert!(pose.position.iter().all(|v| v.is_finite()));
}
