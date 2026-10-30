//! X02: a source bottle stays whole after a soft drop, breaks after a hard drop, spawns its gib
//! once and stays removed after save and load. A broken lamp stays broken after save and load.
use super::drive_out::{frame, settle};
use super::*;
use crate::damage::BreakKind;
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::simulation::FIXED_DT;
use rapier3d::math::Vector;

/// Height above the rest pose. The drop speed adds to the fall from here.
const LIFT: f32 = 0.2;

/// Lifts the item and gives it the speed of a fall from `height` m.
fn drop_from(run: &mut GarageRuntime, item: ItemId, height: f32) {
    let state = run.area.items.get(item).unwrap();
    let (body, p) = (state.body.unwrap(), state.pose.position);
    let speed = (2. * run.physics.rapier().gravity.y.abs() * height).sqrt();
    run.physics.edit_body(body, |b| {
        b.set_translation(Vector::new(p[0], p[1] + LIFT, p[2]), true);
        b.set_linvel(Vector::new(0., -speed, 0.), true);
        b.set_angvel(Vector::ZERO, true);
    });
    run.physics.refresh_queries();
}

/// Spawn serials of the gib prototype.
fn gib_serials(run: &GarageRuntime, gib: usize, serials: &mut Vec<u32>) {
    for spawned in run.area.spawned.iter() {
        if spawned.prototype == gib && !serials.contains(&spawned.serial) {
            serials.push(spawned.serial);
        }
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn bottle_breaks_on_a_hard_drop_once_and_stays_removed_after_reload() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let records = &reference.breakables;
    // A beer bottle (health 6), else the vase (health 5), near the start.
    let (record, item) = run
        .stream
        .breakables
        .targets()
        .iter()
        .filter_map(|t| match t.kind {
            BreakKind::Item(item) => Some((t.record, item)),
            _ => None,
        })
        .filter(|(record, item)| {
            let r = &records[*record];
            r.source != reference.acquisition.r#box.breakable
                && r.gib.is_some()
                && !(r.no_gib || r.no_destroy || r.no_collider || r.only_decrease_above)
                && r.health <= 6.
                && run
                    .area
                    .items
                    .get(*item)
                    .is_some_and(|s| s.location == ItemLocation::World && !s.buried)
        })
        .max_by(|a, b| records[a.0].health.total_cmp(&records[b.0].health))
        .expect("bottle or vase near the start");
    let rules = &records[record];
    let gib = reference
        .item_prototypes
        .iter()
        .position(|p| Some(&p.id) == rules.gib.as_ref())
        .unwrap();
    let key = run.area.items.object_key(item);
    let body = run.area.items.get(item).unwrap().body.unwrap();
    assert!(run.physics.rapier().bodies[body].is_dynamic());

    // A 1 m fall is about 4.4 m/s, below health 5 and 6.
    drop_from(&mut run, item, 1.);
    settle(&mut run, &reference, 2.);
    let state = run.area.items.get(item).unwrap();
    assert!(!state.removed, "a 1 m drop keeps health {}", rules.health);
    let mut serials = Vec::new();
    gib_serials(&run, gib, &mut serials);
    let before = serials.len();

    // A 5 m fall is about 9.9 m/s.
    drop_from(&mut run, item, 5.);
    for _ in 0..(3. / FIXED_DT) as usize {
        frame(&mut run, &reference);
        gib_serials(&run, gib, &mut serials);
    }
    assert!(run.area.items.get(item).is_none_or(|s| s.removed));
    assert_eq!(serials.len(), before + 1, "the break spawns its gib once");
    assert!(
        run.stream
            .changes
            .get(&key)
            .is_some_and(|c| c.snapshot.is_none()),
        "the removal is a world change"
    );

    // Save and load.
    let stamp = roadside_stamp(&reference);
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-26T12:00:00Z")
        .unwrap();
    let saved: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    for _ in 0..2000 {
        prepare_ready(&mut loaded, &reference);
        if loaded.advance_saved_restore(&reference).unwrap() && !loaded.restore_pending() {
            break;
        }
    }
    assert!(!loaded.restore_pending());
    loaded.started = true;
    loaded.paused = false;
    settle(&mut loaded, &reference, 1.);
    assert!(
        loaded
            .area
            .items
            .resolve_key(&key)
            .and_then(|id| loaded.area.items.get(id))
            .is_none_or(|s| s.removed),
        "the broken item stays removed"
    );
    assert!(
        loaded.area.spawned.iter().all(|s| s.prototype != gib),
        "gibs are not saved"
    );
}

#[test]
fn broken_lamp_bits_survive_the_change_record() {
    use crate::world::spawn_state::SpawnInitialization;
    use crate::world::{ChangeRecords, GeneratedId, ObjectKey, WorldLayer};
    let source = SourceId {
        file: "sharedassets1.assets".into(),
        path_id: 1,
    };
    let parent = GeneratedId {
        layer: WorldLayer::Building,
        generator: source.clone(),
        unit: [0; 2],
        ordinal: 0,
        children: Vec::new(),
    };
    let mut key = parent.clone();
    key.children.push((source.clone(), 0));
    let key = ObjectKey::Generated(key);
    let mut baseline = super::replay_snapshot([0.; 3]);
    baseline.lamp = Some(true);
    let mut broken = baseline.clone();
    broken.lamp_broken = Some(1 << 1);
    let mut changes = ChangeRecords::default();
    changes.record(key.clone(), broken.clone(), &baseline);
    let changes: ChangeRecords =
        serde_json::from_slice(&serde_json::to_vec(&changes).unwrap()).unwrap();
    let saved = changes.get(&key).and_then(|c| c.snapshot.as_ref()).unwrap();
    assert_eq!(saved, &broken);
    // The rebind applies the saved bits to the spawn inputs.
    let mut init = SpawnInitialization::default();
    init.lamps.push((parent.clone(), source.clone(), true));
    init.break_lamps(&parent, &source, saved.lamp_broken.unwrap());
    assert!(!init.lamp_broken(&parent, &source, 0));
    assert!(init.lamp_broken(&parent, &source, 1));
}

#[test]
#[ignore = "requires the installed source export"]
fn shovel_broken_lamp_stays_broken_after_reload() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let records = &reference.breakables;
    // A bound building lamp trigger that Break destroys.
    let target = run
        .stream
        .breakables
        .targets()
        .iter()
        .position(|t| {
            matches!(t.kind, BreakKind::Lamp { .. })
                && !records[t.record].no_destroy
                && !records[t.record].lamp.as_ref().unwrap().lamps.is_empty()
        })
        .expect("lamp trigger near the start");
    let BreakKind::Lamp { parent, control } = run.stream.breakables.targets()[target].kind.clone()
    else {
        unreachable!()
    };
    let record = run.stream.breakables.targets()[target].record;
    let lamps = records[record]
        .lamp
        .as_ref()
        .unwrap()
        .lamps
        .iter()
        .fold(0u64, |bits, i| bits | 1 << i);
    let source = run
        .stream
        .lamp_bindings
        .iter()
        .find(|b| b.control == control && b.parent == parent)
        .unwrap()
        .source
        .clone();
    assert_eq!(
        run.stream.contents.broken_lamps(&parent, &source) & lamps,
        0
    );

    // Shovel hits until the trigger breaks.
    for _ in 0..100 {
        if run.stream.breakables.targets()[target].destroyed {
            break;
        }
        run.stream
            .breakables
            .try_break_melee(records, target, 10., FIXED_DT);
        frame(&mut run, &reference);
    }
    settle(&mut run, &reference, 0.5);
    let dark =
        |run: &GarageRuntime| run.stream.contents.broken_lamps(&parent, &source) & lamps == lamps;
    assert!(dark(&run), "the hit lamps are broken");

    // Save and load.
    let stamp = roadside_stamp(&reference);
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-26T12:00:00Z")
        .unwrap();
    let saved: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    for _ in 0..2000 {
        prepare_ready(&mut loaded, &reference);
        if loaded.advance_saved_restore(&reference).unwrap() && !loaded.restore_pending() {
            break;
        }
    }
    assert!(!loaded.restore_pending());
    loaded.started = true;
    loaded.paused = false;
    settle(&mut loaded, &reference, 1.);
    assert!(dark(&loaded), "the lamps stay broken");
    let target = loaded
        .stream
        .breakables
        .targets()
        .iter()
        .position(|t| {
            t.record == record
                && matches!(&t.kind, BreakKind::Lamp { parent: p, .. } if p == &parent)
        })
        .expect("rebound lamp trigger");
    let state = &loaded.stream.breakables.targets()[target];
    assert!(state.destroyed && state.removed);
    for collider in loaded.stream.breakables.colliders_of(target) {
        assert!(!loaded.physics.rapier().colliders[collider].is_enabled());
    }
    // The destroyed trigger does not break again.
    loaded
        .stream
        .breakables
        .try_break_melee(records, target, 1e6, FIXED_DT);
    assert!(loaded.stream.take_break().is_none());
}

/// Spawns one copy of item prototype `prototype` 2 m above the player.
fn spawn_near_player(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    prototype: usize,
) -> ItemId {
    let mut pose = run.area.player.body_pose(&run.physics);
    pose.position[1] += 2.;
    pose.rotation = [0., 0., 0., 1.];
    crate::starting_area::spawned::spawn_prototype(
        reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        pose,
    )
    .unwrap()[0]
}

#[test]
#[ignore = "requires the installed source export"]
fn tumbleweed_breaks_on_a_hard_drop_and_leaves_the_spawned_list() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let r = &reference.weather.tumbleweeds;
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.item_id == Some(r.item_id))
        .expect("tumbleweed prototype");
    let weed = spawn_near_player(&mut run, &reference, prototype);
    settle(&mut run, &reference, 2.);
    let record = run
        .stream
        .breakables
        .targets()
        .iter()
        .find(|t| t.kind == BreakKind::Item(weed))
        .expect("the spawned tumbleweed binds its breakablescript")
        .record;
    let gib = reference.breakables[record].gib.as_ref().map(|g| {
        reference
            .item_prototypes
            .iter()
            .position(|p| &p.id == g)
            .unwrap()
    });
    assert!(
        run.area.spawned.contains(weed),
        "a soft landing keeps the weed"
    );
    let mut serials = Vec::new();
    if let Some(gib) = gib {
        gib_serials(&run, gib, &mut serials);
    }
    let before = serials.len();
    drop_from(&mut run, weed, 10.);
    for _ in 0..(3. / FIXED_DT) as usize {
        frame(&mut run, &reference);
        if let Some(gib) = gib {
            gib_serials(&run, gib, &mut serials);
        }
    }
    assert!(
        !run.area.spawned.contains(weed),
        "the broken weed leaves the spawned list"
    );
    if gib.is_some() {
        assert_eq!(serials.len(), before + 1, "the break spawns its gib once");
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn spawned_copy_takes_the_hit_sound_of_its_pickup() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let (prototype, clip) = reference
        .item_prototypes
        .iter()
        .enumerate()
        .find_map(|(i, p)| {
            let clip = p.pickups.first()?.collision_clip.as_ref()?;
            Some((i, reference.clip_id(clip)?))
        })
        .expect("a prototype pickup with a hit clip");
    let item = spawn_near_player(&mut run, &reference, prototype);
    let state = run.area.items.get(item).unwrap();
    assert_eq!(state.hit_sound.map(|h| h.clip), Some(clip));
    let body = state.body.unwrap();
    let rapier = run.physics.rapier();
    assert!(rapier.bodies[body].colliders().iter().all(|c| {
        rapier.colliders[*c]
            .active_events()
            .contains(rapier3d::prelude::ActiveEvents::COLLISION_EVENTS)
    }));
}
