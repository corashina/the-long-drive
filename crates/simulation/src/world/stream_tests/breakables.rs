//! X02: broken lamp bits save. A tumbleweed breaks on a hard drop. A spawned copy takes the hit
//! sound of its pickup. `roadmap_tests::weapons` breaks a bottle, a vase, a box and a lamp.
use super::kit::*;
use super::*;
use crate::damage::BreakKind;
use crate::reference::ItemId;
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
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let r = &reference.weather.tumbleweeds;
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.item_id == Some(r.item_id))
        .expect("tumbleweed prototype");
    let weed = spawn_near_player(&mut run, &reference, prototype);
    seconds(&mut run, &reference, 2.);
    assert!(
        run.stream
            .breakables
            .targets()
            .iter()
            .any(|t| t.kind == BreakKind::Item(weed)),
        "the spawned tumbleweed binds its breakablescript"
    );
    assert!(
        run.area.spawned.contains(weed),
        "a soft landing keeps the weed"
    );
    drop_from(&mut run, weed, 10.);
    seconds(&mut run, &reference, 3.);
    assert!(
        !run.area.spawned.contains(weed),
        "the broken weed leaves the spawned list"
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn spawned_copy_takes_the_hit_sound_of_its_pickup() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
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
