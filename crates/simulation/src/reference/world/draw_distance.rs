//! Source `settingsscript.DDSmin` and `DDSmax`. The runtime resolves them for the chosen value.
use super::{ReferenceError, SourceId, require};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawDistanceReference {
    pub source: SourceId,
    pub minimum: DrawDistanceSetting,
    pub maximum: DrawDistanceSetting,
    /// Source `S.FDrawDistance` before the player changes it.
    pub default: f32,
}

macro_rules! setting {($($field:ident = $name:literal),* $(,)?) => {
    /// One source `DrawDistanceSetting` record.
    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct DrawDistanceSetting {
        $(#[serde(rename = $name)] pub $field: f32,)*
    }
    impl DrawDistanceSetting {
        /// Source names and values in source order.
        pub fn values(&self) -> [(&'static str, f32); 28] {
            [$(($name, self.$field)),*]
        }
    }
}}
setting!(
    draw_distance = "drawDistance",
    fog_modifier = "fogModifier",
    cam1_far_plane = "Cam1FarPlane",
    cam2_near_plane = "Cam2NearPlane",
    cam2_far_plane = "Cam2FarPlane",
    terrain_gen_dist = "TerrainGenDist",
    terrain_remove_dist = "TerrainRemoveDist",
    distant_terrain_gen_dist = "DistantTerrainGenDist",
    distant_terrain_remove_dist = "DistantTerrainRemoveDist",
    distant_terrain2_gen_dist = "DistantTerrain2GenDist",
    distant_terrain2_remove_dist = "DistantTerrain2RemoveDist",
    distant_terrain_shader1_near = "DistantTerrainShader1Near",
    distant_terrain_shader1_far = "DistantTerrainShader1Far",
    distant_terrain_shader1_amount = "DistantTerrainShader1Amount",
    distant_terrain_shader2_near = "DistantTerrainShader2Near",
    distant_terrain_shader2_far = "DistantTerrainShader2Far",
    distant_terrain_shader2_amount = "DistantTerrainShader2Amount",
    obj_gen_dist = "ObjGenDist",
    obj_remove_dist = "ObjRemoveDist",
    mountain_gen_dist = "MountainGenDist",
    mountain_remove_dist = "MountainRemoveDist",
    building_gen_dist = "BuildingGenDist",
    building_remove_dist = "BuildingRemoveDist",
    road_gen_dist = "RoadGenDist",
    item_remove_dist = "ItemRemoveDist",
    item_spawn_dist = "ItemSpawnDist",
    item_freeze_dist = "ItemFreezeDist",
    item_unfreeze_dist = "ItemUnFreezeDist",
);

impl DrawDistanceReference {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        for (set, setting) in [("minimum", &self.minimum), ("maximum", &self.maximum)] {
            for (name, value) in setting.values() {
                // The shader amounts are signed. All other fields are distances or scales.
                require(
                    value.is_finite() && (value > 0. || name.ends_with("Amount")),
                    &format!("world draw_distance.{set}.{name}"),
                )?;
            }
        }
        require(
            self.minimum.draw_distance < self.maximum.draw_distance
                && (self.minimum.draw_distance..=self.maximum.draw_distance)
                    .contains(&self.default),
            "world draw_distance range",
        )
    }
}
