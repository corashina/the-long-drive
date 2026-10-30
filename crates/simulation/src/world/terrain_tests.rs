use super::*;
use crate::world::RoadSequence;
use ::terrain::{TerrainMesh as WorldSurface, TerrainSurfaceJob};

fn reference() -> &'static WorldReference {
    static REFERENCE: std::sync::OnceLock<WorldReference> = std::sync::OnceLock::new();
    REFERENCE.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct Manifest {
            world: WorldReference,
        }
        serde_json::from_slice::<Manifest>(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/assets/reference/manifest.json"
            ))
            .unwrap(),
        )
        .unwrap()
        .world
    })
}
fn fingerprint(surface: &WorldSurface) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut add = |v: u64| {
        hash = (hash ^ v).wrapping_mul(0x100000001b3);
    };
    for v in surface.origin {
        add(v.to_bits());
    }
    for group in [&surface.vertices, &surface.normals] {
        add(group.len() as u64);
        for v in group.iter().flatten() {
            add(v.to_bits() as u64);
        }
    }
    add(surface.tangents.len() as u64);
    for v in surface.tangents.iter().flatten() {
        add(v.to_bits() as u64);
    }
    add(surface.uv.len() as u64);
    for v in surface.uv.iter().flatten() {
        add(v.to_bits() as u64);
    }
    add(surface.colors.len() as u64);
    for v in surface.colors.iter().flatten() {
        add(v.to_bits() as u64);
    }
    add(surface.triangles.len() as u64);
    for v in surface.triangles.iter().flatten() {
        add(*v as u64);
    }
    // The worker client adds one material slot when it reads the finished mesh.
    add(1);
    add(0);
    add(surface.triangles.len() as u64);
    hash
}
#[test]
#[ignore = "requires the installed source export"]
fn terrain_source_fingerprints() {
    use crate::world::{TerrainContext, TerrainFoundation, TerrainRoadPoint};
    let r = reference();
    // Fingerprints were recorded at the 500 m source minimum.
    let distances = crate::world::DrawDistances::resolve(
        &r.draw_distance,
        r.draw_distance.minimum.draw_distance,
    );
    let evaluator = terrain_evaluator(r, 42);
    let mut sequence = RoadSequence::new(r, 42, None).unwrap();
    let mut roads = Vec::new();
    for _ in 0..r.road.gen_at_start {
        let segment = sequence.advance(&evaluator).unwrap();
        roads.extend(
            segment
                .source_coords
                .into_iter()
                .map(|point| TerrainRoadPoint {
                    position: std::array::from_fn(|a| {
                        (segment.source_origin[a] + segment.source_position[a] + point[a] as f64)
                            as f32
                    }),
                    segment_origin: [
                        segment.source_position[0] as f32,
                        segment.source_position[2] as f32,
                    ],
                    range: r.road.def_point_range,
                }),
        );
    }
    roads.retain(|p| p.segment_origin[0].powi(2) + p.segment_origin[1].powi(2) < 1_000_000.);
    let foundations = [TerrainFoundation {
        position: [1., 4., 3.],
        range_squared: [16., 100.],
    }];
    let holes = [
        [[-2., 0., 2.], [2., 0., 2.], [-2., 0., -2.], [2., 0., -2.]],
        [
            [10., 1., 14.],
            [14., 1., 14.],
            [10., 1., 10.],
            [14., 1., 10.],
        ],
    ];
    for tier in 0..3 {
        let tile = terrain_tile(r, &distances, tier).unwrap();
        let context = TerrainContext {
            evaluator: &evaluator,
            tile,
            roads: &roads,
            road_radius_squared: None,
            foundations: &foundations,
            holes: &holes,
            biome_override: None,
        };
        let expected = [0xc222622fd9f7ef28, 0xcf1954b7e178dc9b, 0x44ba67bdb21ff3a9][tier];
        // Zero uses the live 1 ms driving deadline. Other values force repeatable yields.
        for stop_every in [usize::MAX, 1, 3, 0] {
            let mut job = TerrainSurfaceJob::default();
            let mut surface = WorldSurface::default();
            job.begin([0, 0], context, &mut surface);
            let capacities = (
                job.retained_bytes(),
                surface.vertices.capacity(),
                surface.normals.capacity(),
                surface.tangents.capacity(),
                surface.colors.capacity(),
                surface.uv.capacity(),
                surface.triangles.capacity(),
            );
            let mut samples = Vec::new();
            let mut checks = 0;
            let mut previous_work = 0;
            loop {
                let start = std::time::Instant::now();
                let done = job.advance(context, &mut surface, || {
                    checks += 1;
                    if stop_every == 0 {
                        start.elapsed().as_secs_f64() * 1000. >= 1.
                    } else {
                        checks % stop_every == 0
                    }
                });
                let work = job.prepared_vertices() - previous_work;
                assert!(work > 0 && work <= if stop_every == 1 { 38 } else { 4096 });
                previous_work = job.prepared_vertices();
                assert_eq!(
                    capacities,
                    (
                        job.retained_bytes(),
                        surface.vertices.capacity(),
                        surface.normals.capacity(),
                        surface.tangents.capacity(),
                        surface.colors.capacity(),
                        surface.uv.capacity(),
                        surface.triangles.capacity(),
                    )
                );
                samples.push(start.elapsed().as_secs_f64() * 1000.);
                if done {
                    break;
                }
            }
            assert_eq!(
                fingerprint(&surface),
                expected,
                "tier={tier} stop_every={stop_every}"
            );
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "terrain stop_every={stop_every} tier={tier} roads={} resolution={} calls={} hash={:016x} native_ms_p50={:.3} p95={:.3} max={:.3}",
                roads.len(),
                tile.resolution,
                samples.len(),
                fingerprint(&surface),
                samples[samples.len() / 2],
                samples[(samples.len() - 1) * 95 / 100],
                samples.last().unwrap()
            );
        }
    }
}
#[test]
fn game_mode_forces_terrain_color() {
    use crate::world::{GameMode, forced_color};
    let raw = [0.7, 0.2, 0.1, 0.4];
    let winter = GameMode {
        biome: 2,
        ..GameMode::default()
    }
    .biome_override();
    assert_eq!(winter, Some(1));
    assert_eq!(forced_color(raw, winter), [0., 0., 1., 0.4]);
    assert_eq!(forced_color(raw, GameMode::default().biome_override()), raw);
}
