//! Source posture, clearance sensors, and gameplay view settings.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSphereReference {
    pub source: SourceId,
    pub center: [f32; 3],
    pub radius: f32,
    pub membership: u32,
    pub filter: u32,
}

/// A Y axis trigger capsule in the player body frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCapsuleReference {
    pub source: SourceId,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
    pub membership: u32,
    pub filter: u32,
}

/// Source `canwalljump` (wall) and `canwalljump2` (ledge) `sens` triggers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerWallSensorsReference {
    pub wall: PlayerCapsuleReference,
    pub ledge: PlayerSphereReference,
}

/// Source `interiorsens` (`sens2`): an overlap sphere at the head.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerInteriorSensorReference {
    pub source: SourceId,
    pub radius: f32,
    pub mask: u32,
    /// Position in the head frame.
    pub local_position: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerClearanceReference {
    pub posture: f32,
    pub sphere: PlayerSphereReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerPostureReference {
    pub capsule_membership: u32,
    pub capsule_filter: u32,
    pub standing_head: Transform,
    pub prone_head: Transform,
    pub minimum_radius: f32,
    pub maximum_radius: f32,
    pub ctrl_value: f32,
    pub scroll_step: f32,
    pub rise_rate: f32,
    pub feet: PlayerSphereReference,
    pub clearance: [PlayerClearanceReference; 10],
    pub sensor_hold_steps: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerViewReference {
    pub head_lerp: f32,
    pub lean_sensitivity: f32,
    pub lean_distance: f32,
    pub height_limits: [f32; 2],
    pub tap_seconds: f32,
    pub return_window_seconds: f32,
    pub return_rate: f32,
    pub free_look_limit_degrees: f32,
    pub zoom_multiplier: f32,
    pub zoom_in_rate: f32,
    pub zoom_out_rate: f32,
    pub lean_scroll_multiplier: f32,
    pub height_scroll_step: f32,
}

/// Source `fpscontroller` outside view rig values (`CarCam*`, `carCamLerp*`, `defcarcamrotX`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarCameraReference {
    pub default_pitch_degrees: f32,
    /// `CarCamLimitX`, `CarCamLimitY`, `CarCamLimitZ`.
    pub pan_limits: [f32; 3],
    pub pan_sensitivity: f32,
    /// Minus the `CarCam` local z.
    pub default_distance: f32,
    /// `FduckLerp`.
    pub recenter_rate: f32,
    pub follow_rate: f32,
    pub follow_speed_kmh: [f32; 2],
    pub follow_angle_speed_kmh: [f32; 2],
    pub map: MapViewReference,
}

/// Source `mainscript.mapCamView*`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapViewReference {
    pub time_add: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub min_pan: f32,
    pub max_pan: f32,
    pub min_pan_speed: f32,
    pub max_pan_speed: f32,
    pub height_rate: f32,
    pub rotation_rate: f32,
    pub look_time: f32,
}

fn same(actual: f32, expected: f32) -> bool {
    actual.is_finite() && (actual - expected).abs() <= 0.00001
}

fn sphere(
    value: &PlayerSphereReference,
    file: &str,
    path_id: i64,
    layer: u32,
    filter: u32,
    radius: f32,
) -> bool {
    value.source.file == file
        && value.source.path_id == path_id
        && finite(&value.center)
        && same(value.radius, radius)
        && value.membership == 1 << layer
        && value.filter == filter
}

impl PlayerReference {
    pub(super) fn validate_movement(&self) -> Result<(), ReferenceError> {
        let p = &self.posture;
        require(
            same(self.prone_multiplier, 0.3)
                && same(self.duck_jump_multiplier, 1.25)
                && same(self.inverse_duck_multiplier, 0.5)
                && same(self.max_fall_speed, 30.0)
                && same(self.max_depenetration_velocity, 10.0),
            "player posture movement",
        )?;
        require(
            p.capsule_membership == 1 << 12
                && p.capsule_filter == 4_172_190_207
                && transform(&p.standing_head)
                && transform(&p.prone_head)
                && same(p.minimum_radius, 0.1)
                && same(p.maximum_radius, 0.3)
                && same(p.ctrl_value, 0.3)
                && same(p.scroll_step, 0.05)
                && same(p.rise_rate, 5.0)
                && p.sensor_hold_steps == 3
                && sphere(&p.feet, "level1", 57258, 12, 4_172_190_207, 0.3),
            "player posture geometry",
        )?;
        let w = &self.wall_sensors.wall;
        require(
            same(self.air_force, 20000.0)
                && same(self.grip_multiplier, 0.5)
                && same(self.wall_jump_multiplier, 1.0)
                && same(self.ledge_jump_multiplier, 1.25)
                && same(self.grip_loose, 0.0)
                && same(self.max_grip_fall_speed, 5.0)
                && w.source.file == "level1"
                && w.source.path_id == 58453
                && finite(&w.a)
                && finite(&w.b)
                && same(w.radius, 0.2)
                && w.membership == 1 << 14
                && w.filter == 4_037_970_431
                && sphere(
                    &self.wall_sensors.ledge,
                    "level1",
                    57383,
                    14,
                    4_037_970_431,
                    0.2,
                )
                && self.interior_sensor.source.file == "level1"
                && self.interior_sensor.source.path_id == 80121
                && same(self.interior_sensor.radius, 0.05)
                && self.interior_sensor.mask == 8192
                && finite(&self.interior_sensor.local_position),
            "player wall jump and interior sensors",
        )?;
        let low = p.prone_head.translation;
        let distance = p
            .standing_head
            .translation
            .iter()
            .zip(low)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt();
        require(
            distance.is_finite() && distance > 0.0,
            "player head anchors",
        )?;
        for (index, sensor) in p.clearance.iter().enumerate() {
            let fraction = (index + 1) as f32 / 10.0;
            require(
                same(sensor.posture, fraction)
                    && sphere(
                        &sensor.sphere,
                        "sharedassets1.assets",
                        124758,
                        14,
                        4_037_970_431,
                        0.15,
                    )
                    && sensor.sphere.center.iter().zip(low).enumerate().all(
                        |(axis, (actual, start))| {
                            same(
                                *actual,
                                start + if axis == 1 { distance * fraction } else { 0.0 },
                            )
                        },
                    ),
                "player ordered clearance spheres",
            )?;
        }
        let v = &self.view;
        require(
            [
                (v.head_lerp, 15.0),
                (v.lean_sensitivity, 0.15),
                (v.lean_distance, 0.6),
                (v.height_limits[0], -0.1),
                (v.height_limits[1], 0.375),
                (v.tap_seconds, 0.2),
                (v.return_window_seconds, 0.5),
                (v.return_rate, 10.0),
                (v.free_look_limit_degrees, 90.0),
                (v.zoom_multiplier, 0.4),
                (v.zoom_in_rate, 10.0),
                (v.zoom_out_rate, 20.0),
                (v.lean_scroll_multiplier, 5.0),
                (v.height_scroll_step, 0.05),
            ]
            .into_iter()
            .all(|(actual, expected)| same(actual, expected)),
            "player gameplay view",
        )
    }
}
