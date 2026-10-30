use super::*;
use crate::reference::ItemId;
use glam::Quat;

#[test]
fn frame_round_trips() {
    let row = |x: f32| PoseRow {
        position: [x, 2., 3.],
        flags: PoseRow::VISIBLE,
        rotation: [0., 0., 0., 1.],
    };
    let frame = SimFrame {
        seq_consumed: 42,
        state: StateBlock {
            simulation_seconds: 12.5,
            origin: [1., 2., 3.],
            stream_origin: [4., 5., 6.],
            cycle_elapsed_seconds: 40.,
            session: 3,
            selection_revision: 9,
            alpha: 0.25,
            time_scale: 2.,
            flags: StateBlock::STARTED | StateBlock::SEATED,
            fov_degrees: 60.,
            stored_items: [0, 8, 0, 3],
            body_pose: row(7.),
            body_root: row(8.),
            joint_scale: 0.01,
            sky_anchor: [0., 300., 0.],
            ember_light: 8,
            dither_light: 4,
            pad: 0,
            dead_weight: 0.5,
            motion_revision: 2,
            far_plane: 1500.,
            sleep_camera: row(9.),
            view_camera: PoseRow::default(),
            prepared_roads: 12,
            glasses: GlassesState::encode(Some(GlassesState {
                profile: 3,
                hide_flare: true,
            })),
            focus_distance: 0.4,
            binocular_focus: 0.,
            look: LookBlock {
                yaw: 30.,
                gain: 1.,
                ..Default::default()
            },
            perf: PerfBlock {
                physics_ms: 3.5,
                frame_steps: 2,
                ..Default::default()
            },
        },
        hud: HudBlock {
            hp: 80.,
            slots: [0, 7, 0, 0],
            absolute: [1e6, 2., 3.],
            flags: HudBlock::AUTOSAVE_SAFE,
            ..HudBlock::default()
        },
        previous: vec![row(1.), row(4.)],
        current: vec![row(2.), row(5.)],
        items: vec![
            ItemRow::default(),
            ItemRow {
                scale: 0.5,
                bite: 2,
            },
        ],
        joints: vec![row(9.)],
        remotes: vec![RemoteRow {
            slot: 2,
            flags: RemoteRow::VISIBLE,
            body_pose: row(11.),
            body_root: row(12.),
        }],
        remote_joints: vec![row(13.)],
        ragdoll: vec![row(10.)],
        cigarettes: vec![CigaretteRow {
            item: 7,
            blend: 0.5,
            color: [1., 0.5, 0., 1.],
            light_intensity: 2.,
        }],
        particle_groups: vec![ParticleGroupRow {
            kind: ParticleGroupRow::CAR,
            a: ParticleGroupRow::CAR_WHEEL,
            b: 2,
            c: 9,
            source: 17,
            start: 0,
            count: 1,
            fluid: 0,
        }],
        particles: vec![ParticleRow {
            position: [1., 2., 3.],
            size: 0.5,
            age: 1.,
            lifetime: 2.,
            ..Default::default()
        }],
        decals: vec![DecalRow {
            previous: row(4.),
            current: row(5.),
            kind: 3,
            scale: 0.25,
        }],
        planes: vec![PlaneRow {
            id: 3,
            prefab: 1,
            offset: [10., 0., 5.],
            heading: [0., 0., 0., 1.],
        }],
        creatures: vec![CreatureRow {
            id: 2,
            flags: PoseRow::VISIBLE,
            position: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
            scale: 0.8,
            phase: 0.25,
            value: 0.5,
            twitch: 0.1,
            action: CreatureRow::ATTACK,
            action_time: 0.3,
            action_weight: 0.5,
            bones: [0, 1],
        }],
        creature_bones: vec![CreatureBoneRow {
            node: 4,
            position: [0., 0.1, 0.],
            rotation: [0., 0., 0., 1.],
        }],
        conditions: vec![ConditionRow {
            id: 4,
            state: 2,
            state2: 1,
            state3: 0,
            repair_progress: -0.5,
            color: [0.2, 0.3, 0.4, 1.],
        }],
        washes: vec![1, 0],
        brush_poses: vec![PoseRow::default()],
        spray_colors: vec![[1., 0., 0., 1.]],
        audio: vec![
            AudioCall::Begin {
                session: 1,
                started: true,
                paused: false,
                seconds: 3.5,
            },
            AudioCall::Listener,
            AudioCall::Radio {
                emitter: 40,
                frame: crate::simulation::radio::RadioFrame {
                    power: true,
                    frequency: 98.5,
                    ..Default::default()
                },
            },
            AudioCall::End,
        ],
        budgets: vec![BudgetRow {
            used: 3.,
            limit: 8.,
        }],
        lighting: FrameLighting {
            cycle: crate::lighting::CycleOutput {
                progress: 0.3,
                grading_temperature: Some(5.),
                ..Default::default()
            },
            road_lamp: LightOutput {
                id: crate::reference::LightId(4),
                enabled: true,
                intensity: 1.5,
            },
        },
        lights: vec![LightOutput::default()],
        lamp_materials: Vec::new(),
        control_poses: Vec::new(),
        node_visibility: vec![crate::lighting::NodeVisibilityOutput {
            node: crate::reference::NodeId(2),
            visible: true,
        }],
        valves: vec![ValveRow {
            opening: 4,
            valve: 0.5,
        }],
        structures: vec![StructureRow {
            slot: 3,
            flags: StructureRow::READY | StructureRow::LAMP,
            lamp: [1., 0.5, 0., 1.],
            display_map: 0,
            poses: [0, 1],
            hidden: [0, 1],
            materials: [0, 1],
            colors: [0, 1],
        }],
        structure_poses: vec![NodePoseRow {
            node: 2,
            kind: NodePoseRow::BODY,
            translation: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        }],
        hidden_nodes: vec![5],
        node_materials: vec![MaterialRow { node: 1, source: 0 }],
        node_colors: vec![NodeColorRow {
            node: 1,
            color: [0.5, 0.25, 0.125, 1.],
        }],
        changes: vec![
            Change::Spawn {
                handle: Handle(1),
                key: PoseKey::Item(ItemId(7)),
                data: SpawnData::Spawned(crate::starting_area::spawned::SpawnedItem {
                    id: ItemId(7),
                    serial: 2,
                    prototype: 3,
                    pickup: 1,
                }),
            },
            Change::PlayerRig(vec![crate::reference::NodeId(5)]),
            Change::Retire(Handle(0)),
            Change::HomeCable(vec![RopeAim {
                node: crate::reference::NodeId(9),
                rotation: [0., 0., 0., 1.],
                scale_z: 0.4,
            }]),
            Change::Feedback(Box::new(Feedback {
                name: "Jerry can".into(),
                amount: 4.5,
                quantities: [4.5, 0., 0., 0., 0., 0.],
                ..Feedback::default()
            })),
            Change::StructureRemove(3),
            Change::MaterialSource(crate::reference::SourceId {
                file: "level0".into(),
                path_id: 8,
            }),
            Change::Plates {
                id: crate::world::GeneratedId {
                    layer: crate::world::WorldLayer::Startup,
                    generator: crate::reference::SourceId {
                        file: "level0".into(),
                        path_id: 7,
                    },
                    unit: [0, 0],
                    ordinal: 1,
                    children: Vec::new(),
                },
                texts: vec!["ABC 123".into()],
            },
            Change::Failure("stream".into()),
            Change::SaveResult {
                id: 4,
                kind: "Capture".into(),
                payload: "{}".into(),
                error: String::new(),
            },
        ],
        world: crate::world::WorldDelta::default(),
    };
    let mut bytes = Vec::new();
    encode(&frame, &mut bytes);
    let mut decoded = SimFrame::default();
    decode(&bytes, &mut decoded).unwrap();
    assert_eq!(decoded, frame);
    assert_eq!(
        decoded.state.glasses(),
        Some(GlassesState {
            profile: 3,
            hide_flare: true
        })
    );
    assert!(decode(&bytes[..20], &mut decoded).is_err());
}

#[test]
fn command_round_trips() {
    let mut command = SimCommand {
        seq: 9,
        delta_seconds: 1. / 60.,
        ..Default::default()
    };
    command.input.pressed = 5;
    command.input.mouse = [0.5, -1.];
    command.input.carry.owner = Some(ItemId(3));
    command.input.typing.text[..2].copy_from_slice(b"hi");
    command.input.typing.len = 2;
    command.input.gamepad.axes[31] = 0.75;
    command.actions = SimCommand::PAUSE;
    command.assists = Some(crate::reference::DrivingAssists {
        automatic_clutch: true,
        automatic_gears: false,
    });
    command
        .world
        .facts
        .push(crate::world::RendererFact::ConfigureStaging {
            reserve: 4,
            metadata: 2,
        });
    command.world.report = Some(crate::world::PresentationReport::default());
    command.world.lamps = LampAdmission::Pending;
    command.save = vec![
        SaveCommand::Request {
            json: r#"{"id":1,"kind":"PrepareNewGame"}"#.into(),
            mode: crate::world::GameMode::default(),
            epoch: 3,
        },
        SaveCommand::Cancel(1),
    ];
    command.listener = Some([1., 2., 3.]);
    let mut bytes = Vec::new();
    encode_command(&command, &mut bytes);
    assert_eq!(decode_command(&bytes).unwrap(), command);
}

#[test]
fn command_merge_keeps_edges_and_sums_deltas() {
    let mut first = SimCommand {
        seq: 1,
        delta_seconds: 0.25,
        actions: SimCommand::PAUSE,
        ..Default::default()
    };
    first.input.held = 1;
    first.input.pressed = 1;
    first.input.mouse = [1., 2.];
    let mut second = SimCommand {
        seq: 2,
        delta_seconds: 0.5,
        ready: true,
        ..Default::default()
    };
    second.input.held = 2;
    second.input.released = 1;
    second.input.mouse = [0.5, -1.];
    first.sandbox.push("[1]".into());
    second.sandbox.push("[2]".into());
    first.merge(second.clone());
    assert_eq!(first.sandbox, ["[1]", "[2]"]);
    assert_eq!(
        (first.seq, first.delta_seconds, first.ready),
        (2, 0.75, true)
    );
    assert_eq!(first.actions, SimCommand::PAUSE);
    assert_eq!(
        (first.input.held, first.input.pressed, first.input.released),
        (2, 1, 1)
    );
    assert_eq!(first.input.mouse, [1.5, 1.]);
    second.hold = true;
    first.merge(second);
    assert!(first.hold);
    assert_eq!(first.input, FrameInput::default());
}

#[test]
fn world_merge_keeps_facts_and_released_credit() {
    let report = |used: usize, release: usize| {
        let mut report = crate::world::PresentationReport::default();
        report.used.jobs = used;
        report.release.jobs = release;
        report
    };
    let fact = |source: i64| {
        crate::world::RendererFact::HoldPrototype(SourceId {
            file: "a".into(),
            path_id: source,
        })
    };
    let mut first = WorldCommand {
        report: Some(report(3, 1)),
        lamps: LampAdmission::Ready(Vec::new()),
        ..Default::default()
    };
    first.facts.push(fact(1));
    let mut second = WorldCommand {
        report: Some(report(2, 4)),
        ..Default::default()
    };
    second.facts.push(fact(2));
    first.merge(second);
    let report = first.report.unwrap();
    assert_eq!((report.used.jobs, report.release.jobs), (2, 5));
    assert!(first.facts.contains(&fact(1)) && first.facts.contains(&fact(2)));
    // No lamp change keeps the earlier admission.
    assert_eq!(first.lamps, LampAdmission::Ready(Vec::new()));
}

#[test]
fn handle_is_reused_only_after_release() {
    let mut handles = Handles::default();
    let (a, new) = handles.assign(PoseKey::Item(ItemId(1)));
    assert!(new);
    assert_eq!(handles.assign(PoseKey::Item(ItemId(1))), (a, false));
    handles.retire(a);
    let (b, _) = handles.assign(PoseKey::Item(ItemId(2)));
    assert_ne!(a, b);
    handles.release(a);
    let (c, _) = handles.assign(PoseKey::Item(ItemId(3)));
    assert_eq!(c, a);
    assert_eq!(handles.len(), 2);
}

fn look_frame(yaw: f32, pitch: f32, seq_consumed: u64) -> SimFrame {
    let mut frame = SimFrame {
        seq_consumed,
        ..Default::default()
    };
    frame.state.alpha = 1.;
    frame.state.look = LookBlock {
        position: [[0., 1., 0.], [1., 2., 3.]],
        parent: [[0., 0., 0., 1.], Quat::from_rotation_y(0.5).to_array()],
        camera: [0., 0., 0., 1.],
        yaw,
        pitch,
        gain: 2.,
        yaw_limits: [-90., 90.],
        pitch_limits: [-60., 60.],
        pad: 0,
    };
    frame
}

#[test]
fn mouse_look_adds_unconsumed_mouse_within_limits() {
    let mut look = MouseLook::default();
    look.send(1, [5., 0.]);
    look.send(2, [1., 40.]);
    let pose = look.camera(&look_frame(10., 0., 1));
    let expected = Quat::from_rotation_y(0.5)
        * Quat::from_rotation_y(-12f32.to_radians())
        * Quat::from_rotation_x(-60f32.to_radians());
    assert!(Quat::from_array(pose.rotation).abs_diff_eq(expected, 1e-5));
    assert_eq!(pose.position, [1., 2., 3.]);
}

#[test]
fn mouse_look_matches_the_frame_after_consume() {
    let mut look = MouseLook::default();
    look.send(1, [3., -2.]);
    let before = look.camera(&look_frame(10., 5., 0));
    // The step applies the mouse with the same gain.
    let after = look.camera(&look_frame(16., 1., 1));
    assert!(Quat::from_array(before.rotation).abs_diff_eq(Quat::from_array(after.rotation), 1e-5));
    assert_eq!(look.camera(&look_frame(16., 1., 1)), after);
}

/// A stream that leaves out unchanged pose rows decodes to the same frames.
#[test]
fn pose_stream_sends_changed_rows_only() {
    let row = |x: f32| PoseRow {
        position: [x, 0., 0.],
        flags: PoseRow::VISIBLE,
        rotation: [0., 0., 0., 1.],
    };
    let mut frame = SimFrame {
        previous: vec![row(1.), row(2.), row(3.)],
        current: vec![row(1.), row(2.), row(3.)],
        items: vec![ItemRow::default(); 3],
        ..Default::default()
    };
    let (mut encoder, mut bytes, mut decoded) =
        (FrameEncoder::default(), Vec::new(), SimFrame::default());
    encoder.encode(&frame, &mut bytes);
    decode(&bytes, &mut decoded).unwrap();
    assert_eq!(decoded, frame);
    let whole = bytes.len();

    frame.current[1] = row(-0.);
    frame.items[2].bite = 1;
    frame.previous.push(row(4.));
    frame.current.push(row(4.));
    frame.items.push(ItemRow::default());
    encoder.encode(&frame, &mut bytes);
    decode(&bytes, &mut decoded).unwrap();
    assert_eq!(decoded, frame);
    assert_eq!(decoded.current[1].position[0].to_bits(), (-0f32).to_bits());

    encoder.encode(&frame, &mut bytes);
    assert!(bytes.len() < whole, "an unchanged frame sends no pose rows");
    decode(&bytes, &mut decoded).unwrap();
    assert_eq!(decoded, frame);
}
