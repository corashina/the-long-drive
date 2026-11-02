//! Draw distances of one world session, resolved like `settingsscript.DrawDistanceChanged`.
use crate::reference::DrawDistanceReference;

macro_rules! distances {($($field:ident),* $(,)?) => {
    /// Resolved source `DDS` fields. Names follow `DrawDistanceSetting`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct DrawDistances {
        $(pub $field: f32,)*
    }
    impl DrawDistances {
        /// Clamp `value` to the source range, then interpolate each field in f32 like `Mathf`.
        pub fn resolve(r: &DrawDistanceReference, value: f32) -> Self {
            let (lo, hi) = (r.minimum.draw_distance, r.maximum.draw_distance);
            let value = value.clamp(lo, hi);
            let t = ((value - lo) / (hi - lo)).clamp(0., 1.);
            Self {
                $($field: r.minimum.$field + (r.maximum.$field - r.minimum.$field) * t,)*
            }
        }
        /// True when `value` resolves to these distances.
        pub fn matches(&self, r: &DrawDistanceReference, value: f32) -> bool {
            Self::resolve(r, value) == *self
        }
        #[cfg(test)]
        fn values(&self) -> [f32; 28] {
            [$(self.$field),*]
        }
    }
}}
distances!(
    draw_distance,
    fog_modifier,
    cam1_far_plane,
    cam2_near_plane,
    cam2_far_plane,
    terrain_gen_dist,
    terrain_remove_dist,
    distant_terrain_gen_dist,
    distant_terrain_remove_dist,
    distant_terrain2_gen_dist,
    distant_terrain2_remove_dist,
    distant_terrain_shader1_near,
    distant_terrain_shader1_far,
    distant_terrain_shader1_amount,
    distant_terrain_shader2_near,
    distant_terrain_shader2_far,
    distant_terrain_shader2_amount,
    obj_gen_dist,
    obj_remove_dist,
    mountain_gen_dist,
    mountain_remove_dist,
    building_gen_dist,
    building_remove_dist,
    road_gen_dist,
    item_remove_dist,
    item_spawn_dist,
    item_freeze_dist,
    item_unfreeze_dist,
);

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMUM: [f32; 28] = [
        500., 3.5, 150., 100., 10000., 200., 300., 1000., 1500., 10000., 15000., 100., 350., -75.,
        150., 1000., -2000., 100., 300., 500., 1500., 500., 1500., 500., 150., 140., 130., 120.,
    ];
    const MAXIMUM: [f32; 28] = [
        25000., 1., 450., 300., 100000., 450., 675., 25000., 37500., 100000., 150000., 400., 500.,
        -75., 20000., 30000., -1000., 450., 900., 25000., 35000., 25000., 35000., 25000., 360.,
        340., 320., 300.,
    ];
    /// Python struct float32 round trips of the same lerp.
    const AT_2500: [f32; 28] = [
        2500.,
        3.295_918_5,
        174.489_79,
        116.326_53,
        17_346.938,
        220.408_16,
        330.612_24,
        2959.183_6,
        4438.775_4,
        17_346.938,
        26_020.408,
        124.489_79,
        362.244_9,
        -75.,
        1770.408_1,
        3367.347,
        -1918.367_3,
        128.571_43,
        348.979_58,
        2500.,
        4234.694,
        2500.,
        4234.694,
        2500.,
        167.142_85,
        156.326_54,
        145.510_21,
        134.693_88,
    ];

    const NAMES: [&str; 28] = [
        "drawDistance",
        "fogModifier",
        "Cam1FarPlane",
        "Cam2NearPlane",
        "Cam2FarPlane",
        "TerrainGenDist",
        "TerrainRemoveDist",
        "DistantTerrainGenDist",
        "DistantTerrainRemoveDist",
        "DistantTerrain2GenDist",
        "DistantTerrain2RemoveDist",
        "DistantTerrainShader1Near",
        "DistantTerrainShader1Far",
        "DistantTerrainShader1Amount",
        "DistantTerrainShader2Near",
        "DistantTerrainShader2Far",
        "DistantTerrainShader2Amount",
        "ObjGenDist",
        "ObjRemoveDist",
        "MountainGenDist",
        "MountainRemoveDist",
        "BuildingGenDist",
        "BuildingRemoveDist",
        "RoadGenDist",
        "ItemRemoveDist",
        "ItemSpawnDist",
        "ItemFreezeDist",
        "ItemUnFreezeDist",
    ];

    fn reference() -> DrawDistanceReference {
        let set = |v: [f32; 28]| -> serde_json::Value {
            NAMES
                .iter()
                .map(|n| n.to_string())
                .zip(v.map(serde_json::Value::from))
                .collect()
        };
        let r: DrawDistanceReference = serde_json::from_value(serde_json::json!({
            "source": {"file": "level0", "path_id": 16865},
            "minimum": set(MINIMUM),
            "maximum": set(MAXIMUM),
            "default": 2500.,
        }))
        .unwrap();
        r.validate().unwrap();
        r
    }

    fn bits(d: DrawDistances) -> [u32; 28] {
        d.values().map(f32::to_bits)
    }

    #[test]
    fn resolve_follows_source_lerp() {
        let r = reference();
        assert_eq!(
            bits(DrawDistances::resolve(&r, 500.)),
            MINIMUM.map(f32::to_bits)
        );
        assert_eq!(
            bits(DrawDistances::resolve(&r, 2500.)),
            AT_2500.map(f32::to_bits)
        );
        assert_eq!(
            bits(DrawDistances::resolve(&r, 25000.)),
            MAXIMUM.map(f32::to_bits)
        );
        assert_eq!(
            bits(DrawDistances::resolve(&r, 100.)),
            MINIMUM.map(f32::to_bits)
        );
        assert_eq!(
            bits(DrawDistances::resolve(&r, 1e6)),
            MAXIMUM.map(f32::to_bits)
        );
    }

    #[test]
    fn matches_after_clamp() {
        let r = reference();
        let d = DrawDistances::resolve(&r, 2500.);
        assert!(d.matches(&r, 2500.));
        assert!(!d.matches(&r, 2600.));
        assert!(DrawDistances::resolve(&r, 500.).matches(&r, 100.));
    }
}
