//! Source `fpscontroller` seated views: outside view, drop cam and map view.
//! The rig runs once per frame with the frame time, as the source `Update`.
use crate::boundary::{PoseRow, StateBlock};
use crate::reference::CarCameraReference;
use crate::simulation::{FrameInput, keys, player_keys};
use crate::vehicle::VehiclePose;
use glam::{Mat3, Quat, Vec3};

/// Source `dropCamTime >= 0.4f`.
const HOLD_SECONDS: f32 = 0.4;
/// Source `dropCamTime < 0.3f`: a tap leaves the drop cam.
const TAP_SECONDS: f32 = 0.3;
const PITCH_LIMIT: f32 = 89.;
/// Source `scroll * dt * 3f`.
const ZOOM_RATE: f32 = 3.;
const DISTANCE_LIMITS: [f32; 2] = [1., 30.];
/// Source `Time.deltaTime * 20f`: the rig up returns to world up.
const UP_RATE: f32 = 20.;
/// Source `Time.deltaTime * 20f`: map recenter.
const MAP_RECENTER_RATE: f32 = 20.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CarView {
    #[default]
    Cockpit,
    /// Source `camView`.
    Outside,
    /// Source `dropCam`. `outside_before` is `camViewBeforeDropCam`.
    Drop { follow: bool, outside_before: bool },
    /// Source `mapView`.
    Map,
}

/// Seat values of one frame, in stream space.
#[derive(Clone, Copy, Debug)]
pub struct CameraSeat {
    /// `seat.transform.position + up`, or the root with `rootOrigin`.
    pub pivot: Vec3,
    /// `seat.transform.up`, or the root up with `rootOrigin`.
    pub up: Vec3,
    /// `seat.transform.position`: the follow target of the drop cam.
    pub seat: Vec3,
    /// Seat body velocity (`seat.RB.velocity`).
    pub velocity: Vec3,
    /// Player root (`transform`).
    pub player: Vec3,
    pub player_rotation: Quat,
}

impl CameraSeat {
    pub fn lerp(&self, next: &Self, t: f32) -> Self {
        Self {
            pivot: self.pivot.lerp(next.pivot, t),
            up: self.up.lerp(next.up, t).normalize_or(Vec3::Y),
            seat: self.seat.lerp(next.seat, t),
            velocity: self.velocity.lerp(next.velocity, t),
            player: self.player.lerp(next.player, t),
            player_rotation: self.player_rotation.slerp(next.player_rotation, t),
        }
    }
    pub fn shift(&mut self, delta: Vec3) {
        self.pivot -= delta;
        self.seat -= delta;
        self.player -= delta;
    }
}

/// Frame input of the rig. Edges and mouse add up until the next frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraInput {
    pressed: u32,
    released: u32,
    held: u32,
    player_pressed: u16,
    player_held: u16,
    /// Head-look scaled mouse: x is `LOOKX`, y is `-LOOKY`.
    mouse: [f32; 2],
    wheel: f32,
}

impl CameraInput {
    pub fn merge(&mut self, frame: &FrameInput) {
        self.pressed |= frame.pressed;
        self.released |= frame.released;
        self.held = frame.held;
        self.player_pressed |= frame.player_pressed;
        self.player_held = frame.player_held;
        for (sum, delta) in self.mouse.iter_mut().zip(frame.mouse) {
            if delta.is_finite() {
                *sum += delta;
            }
        }
        if frame.wheel.is_finite() {
            self.wheel += frame.wheel;
        }
    }
    /// Keeps the held keys for the next frame.
    fn consume(&mut self) {
        *self = Self {
            held: self.held,
            player_held: self.player_held,
            ..Self::default()
        };
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CarCamera {
    view: CarView,
    /// `dropCamTime`.
    hold: f32,
    /// `switchedCam`.
    switched: bool,
    /// `CarCamParent` rotation relative to the player while it is a player child (Cockpit, Map).
    rig_local: Quat,
    /// `CarCamParent` world rotation in Outside and Drop.
    rig: Quat,
    /// `carcamrotX` (down is positive) and `carcamrotY`.
    orbit: [f32; 2],
    /// `carcampos` in source axes.
    pan: Vec3,
    /// Minus the `CarCam` local z.
    distance: f32,
    /// `cardropcamrotX` as a downward pitch.
    drop_pitch: f32,
    /// `mapViewTime`, `mapViewTimeAdd`, `mapViewX`, `mapViewY`, `mapViewHeightLerp`.
    map_time: f32,
    map_add: f32,
    map_pan: [f32; 2],
    map_height: f32,
    /// `CamParent` world pose of the last frame.
    position: Vec3,
    rotation: Quat,
}

impl CarCamera {
    pub fn new(reference: &CarCameraReference) -> Self {
        Self {
            view: CarView::Cockpit,
            hold: 0.,
            switched: false,
            rig_local: Quat::IDENTITY,
            rig: Quat::IDENTITY,
            orbit: [reference.default_pitch_degrees, 0.],
            pan: Vec3::ZERO,
            distance: reference.default_distance,
            drop_pitch: 0.,
            map_time: 0.,
            map_add: 0.,
            map_pan: [0.; 2],
            map_height: 0.,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        }
    }

    pub fn view(&self) -> CarView {
        self.view
    }

    /// Source `otherView()`.
    pub fn other_view(&self) -> bool {
        self.view != CarView::Cockpit
    }

    /// The camera of an other view.
    pub fn pose(&self) -> Option<VehiclePose> {
        self.other_view().then(|| VehiclePose {
            position: self.position.to_array(),
            rotation: self.rotation.to_array(),
        })
    }

    /// Writes the view camera and its flags.
    pub fn fill(&self, state: &mut StateBlock) {
        let Some(pose) = self.pose() else {
            return;
        };
        state.view_camera = PoseRow::new(pose, PoseRow::ACTIVE);
        state.flags |= StateBlock::OTHER_VIEW
            | match self.view {
                CarView::Drop { .. } => StateBlock::DROP_CAM,
                CarView::Map => StateBlock::MAP_VIEW,
                _ => 0,
            };
    }

    /// The stream origin moved by `delta`.
    pub fn shift(&mut self, delta: Vec3) {
        self.position -= delta;
    }

    /// Back to the cockpit with no key state. Orbit, pan, zoom and map height stay, as in the source.
    pub fn leave(&mut self) {
        self.view = CarView::Cockpit;
        self.hold = 0.;
        self.switched = false;
    }

    /// One frame. `seat` is `None` when the player does not sit. `head` is the player camera.
    pub fn frame(
        &mut self,
        reference: &CarCameraReference,
        input: &mut CameraInput,
        seat: Option<&CameraSeat>,
        head: (Vec3, Quat),
        dt: f32,
    ) {
        let frame = *input;
        input.consume();
        let Some(seat) = seat else {
            self.leave();
            return;
        };
        if self.view == CarView::Cockpit {
            (self.position, self.rotation) = head;
        }
        self.keys(reference, &frame, seat, dt);
        let lean = frame.player_held & player_keys::LEAN != 0;
        let recenter = frame.held & keys::RECENTER != 0;
        match self.view {
            CarView::Cockpit => (self.position, self.rotation) = head,
            CarView::Outside => self.outside(reference, &frame, seat, lean, recenter, dt),
            CarView::Drop { follow, .. } => self.drop_cam(&frame, seat, follow),
            CarView::Map => self.map(reference, &frame, seat, head, lean, recenter, dt),
        }
    }

    /// The `mapDown` and camera key blocks of `Update`.
    fn keys(
        &mut self,
        reference: &CarCameraReference,
        frame: &CameraInput,
        seat: &CameraSeat,
        dt: f32,
    ) {
        if frame.player_pressed & player_keys::MAP != 0 {
            if self.view == CarView::Map {
                self.map_time = 1.;
                self.map_add = -reference.map.time_add;
            } else if !self.other_view() {
                self.view = CarView::Map;
                self.map_time = 0.;
                self.map_add = reference.map.time_add;
            }
        }
        if self.view == CarView::Map {
            return;
        }
        let held = frame.held & keys::CAMERA != 0;
        if frame.pressed & keys::CAMERA != 0 {
            self.hold = 0.;
        }
        if !self.switched {
            if let CarView::Drop {
                follow,
                outside_before,
            } = self.view
            {
                if frame.released & keys::CAMERA != 0 && self.hold < TAP_SECONDS {
                    // SwitchBackFromDropCam.
                    if outside_before {
                        self.view = CarView::Outside;
                        self.switch_to_outside(seat);
                    } else {
                        self.leave_to_cockpit(seat.player_rotation);
                    }
                } else if held {
                    self.hold += dt;
                    if self.hold >= HOLD_SECONDS {
                        self.view = CarView::Drop {
                            follow: !follow,
                            outside_before,
                        };
                        self.switched = true;
                    }
                }
            } else {
                if frame.released & keys::CAMERA != 0 {
                    if self.view == CarView::Outside {
                        self.leave_to_cockpit(seat.player_rotation);
                    } else {
                        self.attach_rig(seat);
                        self.view = CarView::Outside;
                        self.switch_to_outside(seat);
                    }
                }
                if held {
                    self.hold += dt;
                    if self.hold >= HOLD_SECONDS {
                        self.hold = 0.;
                        let outside_before = self.view == CarView::Outside;
                        if !outside_before {
                            self.attach_rig(seat);
                        }
                        self.drop_pitch = pitch_down(self.rotation);
                        self.view = CarView::Drop {
                            follow: false,
                            outside_before,
                        };
                        self.switch_to_outside(seat);
                    }
                }
            }
        }
        if !held {
            self.switched = false;
        }
    }

    /// The rig leaves the player: its world rotation is the player child rotation.
    fn attach_rig(&mut self, seat: &CameraSeat) {
        self.rig = seat.player_rotation * self.rig_local;
    }

    /// `SwitchToOutsideView`: align the rig up with the seat up.
    fn switch_to_outside(&mut self, seat: &CameraSeat) {
        self.switched = true;
        self.rig = Quat::from_rotation_arc(self.rig * Vec3::Y, seat.up) * self.rig;
    }

    /// `SwitchToCockpitView`: the rig becomes a player child again.
    fn leave_to_cockpit(&mut self, player_rotation: Quat) {
        if matches!(self.view, CarView::Outside | CarView::Drop { .. }) {
            self.rig_local = player_rotation.inverse() * self.rig;
        }
        self.view = CarView::Cockpit;
        self.switched = true;
    }

    fn outside(
        &mut self,
        reference: &CarCameraReference,
        frame: &CameraInput,
        seat: &CameraSeat,
        lean: bool,
        recenter: bool,
        dt: f32,
    ) {
        let [mx, my] = frame.mouse;
        if lean {
            let step = reference.pan_sensitivity;
            self.pan.x += mx * step;
            if frame.held & keys::RUN != 0 {
                self.pan.z -= my * step;
            } else {
                self.pan.y -= my * step;
            }
            let limits = Vec3::from_array(reference.pan_limits);
            self.pan = self.pan.clamp(-limits, limits);
        } else {
            self.orbit[0] += my;
            self.orbit[1] += mx;
        }
        self.orbit[0] = self.orbit[0].clamp(-PITCH_LIMIT, PITCH_LIMIT);
        let orbit = euler(self.orbit[0], self.orbit[1]);
        let pan = Vec3::new(self.pan.x, self.pan.y, -self.pan.z);
        self.distance = (self.distance - frame.wheel * dt * ZOOM_RATE)
            .clamp(DISTANCE_LIMITS[0], DISTANCE_LIMITS[1]);
        if recenter {
            let t = (reference.recenter_rate * dt).clamp(0., 1.);
            self.distance += (reference.default_distance - self.distance) * t;
            self.pan = self.pan.lerp(Vec3::ZERO, t);
            self.orbit[0] += (reference.default_pitch_degrees - self.orbit[0]) * t;
            self.orbit[1] = lerp_angle(self.orbit[1], 0., t);
        }
        let upright = Quat::from_rotation_arc(self.rig * Vec3::Y, Vec3::Y) * self.rig;
        self.rig = self.rig.lerp(upright, (dt * UP_RATE).clamp(0., 1.));
        let kmh = seat.velocity.length() * 3.6;
        let forward = self.rig * -Vec3::Z;
        let [speed_min, speed_max] = reference.follow_speed_kmh;
        let [angle_min, angle_max] = reference.follow_angle_speed_kmh;
        let angle = forward.angle_between(seat.velocity).to_degrees();
        let angle = if angle.is_finite() { angle } else { 0. };
        let turning =
            angle * if kmh > 5. { 1. } else { 0. } * inverse_lerp(angle_min, angle_max, kmh);
        let k = dt
            * reference.follow_rate
            * inverse_lerp(speed_min, speed_max, kmh).max(inverse_lerp(10., 90., turning));
        let forward = forward.lerp(seat.velocity.normalize_or_zero(), k.clamp(0., 1.));
        if let Some(rig) = look_rotation(forward, Vec3::Y) {
            self.rig = rig;
        }
        let frame_rotation = self.rig * orbit;
        let target = seat.pivot + frame_rotation * pan;
        self.position = target + frame_rotation * Vec3::new(0., 0., self.distance);
        if let Some(rotation) = look_rotation(target - self.position, Vec3::Y) {
            self.rotation = rotation;
        }
    }

    fn drop_cam(&mut self, frame: &CameraInput, seat: &CameraSeat, follow: bool) {
        if follow {
            if let Some(rotation) = look_rotation(seat.seat - self.position, Vec3::Y) {
                self.rotation = rotation;
            }
            self.drop_pitch = pitch_down(self.rotation);
        } else {
            self.drop_pitch = (self.drop_pitch + frame.mouse[1]).clamp(-PITCH_LIMIT, PITCH_LIMIT);
            self.rotation = euler(self.drop_pitch, yaw(self.rotation) + frame.mouse[0]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn map(
        &mut self,
        reference: &CarCameraReference,
        frame: &CameraInput,
        seat: &CameraSeat,
        head: (Vec3, Quat),
        lean: bool,
        recenter: bool,
        dt: f32,
    ) {
        let map = &reference.map;
        if lean {
            let speed = lerp(map.min_pan_speed, map.max_pan_speed, self.map_height);
            self.map_pan[0] += frame.mouse[0] * dt * speed;
            self.map_pan[1] -= frame.mouse[1] * dt * speed;
        }
        if recenter {
            let t = (dt * MAP_RECENTER_RATE).clamp(0., 1.);
            self.map_pan = self.map_pan.map(|v| v - v * t);
        }
        let border = lerp(map.min_pan, map.max_pan, self.map_height);
        self.map_pan = self.map_pan.map(|v| v.clamp(-border, border));
        self.map_time += self.map_add * dt;
        if self.map_time <= 0. {
            self.view = CarView::Cockpit;
            (self.position, self.rotation) = head;
            return;
        }
        self.map_height = (self.map_height - frame.wheel * dt * map.height_rate).clamp(0., 1.);
        let player_forward = seat.player_rotation * -Vec3::Z;
        let target = seat.player
            + Vec3::Y * lerp(map.min_height, map.max_height, self.map_height)
            + player_forward * self.map_pan[1]
            + seat.player_rotation * Vec3::X * self.map_pan[0];
        let t = self.map_time.clamp(0., 1.);
        self.position = head.0.lerp(target, t);
        let flat = (player_forward - Vec3::Y * player_forward.y).normalize_or(Vec3::NEG_Z);
        let down = look_rotation(Vec3::NEG_Y, flat).unwrap_or(self.rotation);
        let look = inverse_lerp(0., map.look_time, self.map_time);
        self.rotation = if self.map_time < look {
            head.1.lerp(down, look)
        } else {
            self.rotation
                .lerp(down, (dt * map.rotation_rate).clamp(0., 1.))
        };
    }
}

/// Source `Quaternion.Euler(pitch, yaw, 0)` in port axes.
fn euler(pitch_down: f32, yaw: f32) -> Quat {
    Quat::from_rotation_y(-yaw.to_radians()) * Quat::from_rotation_x(-pitch_down.to_radians())
}

/// Source `eulerAngles.y`.
fn yaw(rotation: Quat) -> f32 {
    let forward = rotation * -Vec3::Z;
    forward.x.atan2(-forward.z).to_degrees()
}

/// Degrees below the horizon.
fn pitch_down(rotation: Quat) -> f32 {
    -(rotation * -Vec3::Z).y.clamp(-1., 1.).asin().to_degrees()
}

/// Source `Quaternion.LookRotation(forward, up)`. The camera looks along -Z.
fn look_rotation(forward: Vec3, up: Vec3) -> Option<Quat> {
    let back = -forward.try_normalize()?;
    let right = up.cross(back).try_normalize()?;
    Some(Quat::from_mat3(&Mat3::from_cols(
        right,
        back.cross(right),
        back,
    )))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}

/// Source `Mathf.InverseLerp`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((value - a) / (b - a)).clamp(0., 1.)
    }
}

/// Source `Mathf.LerpAngle`.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut delta = (b - a).rem_euclid(360.);
    if delta > 180. {
        delta -= 360.;
    }
    a + delta * t.clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::MapViewReference;

    const DT: f32 = 0.02;

    fn reference() -> CarCameraReference {
        CarCameraReference {
            default_pitch_degrees: 12.5,
            pan_limits: [1.5; 3],
            pan_sensitivity: 0.0125,
            default_distance: 5.,
            recenter_rate: 5.,
            follow_rate: 3.,
            follow_speed_kmh: [5., 100.],
            follow_angle_speed_kmh: [-30., 50.],
            map: MapViewReference {
                time_add: 3.,
                min_height: 2.5,
                max_height: 200.,
                min_pan: 2.5,
                max_pan: 100.,
                min_pan_speed: 2.5,
                max_pan_speed: 10.,
                height_rate: 1.,
                rotation_rate: 1.5,
                look_time: 0.3,
            },
        }
    }

    fn seat() -> CameraSeat {
        CameraSeat {
            pivot: Vec3::Y,
            up: Vec3::Y,
            seat: Vec3::ZERO,
            velocity: Vec3::ZERO,
            player: Vec3::ZERO,
            player_rotation: Quat::IDENTITY,
        }
    }

    struct Rig {
        camera: CarCamera,
        reference: CarCameraReference,
    }

    impl Rig {
        fn new() -> Self {
            let reference = reference();
            Self {
                camera: CarCamera::new(&reference),
                reference,
            }
        }
        fn run(&mut self, frame: FrameInput) {
            let mut input = CameraInput::default();
            input.merge(&frame);
            self.camera.frame(
                &self.reference,
                &mut input,
                Some(&seat()),
                (Vec3::Y, Quat::IDENTITY),
                DT,
            );
        }
        fn idle(&mut self, frames: usize) {
            for _ in 0..frames {
                self.run(FrameInput::default());
            }
        }
        fn key(&mut self, frames: usize) {
            self.run(FrameInput {
                pressed: keys::CAMERA,
                held: keys::CAMERA,
                ..FrameInput::default()
            });
            for _ in 1..frames {
                self.run(FrameInput {
                    held: keys::CAMERA,
                    ..FrameInput::default()
                });
            }
            self.run(FrameInput {
                released: keys::CAMERA,
                ..FrameInput::default()
            });
        }
        fn map_key(&mut self) {
            self.run(FrameInput {
                player_pressed: player_keys::MAP,
                ..FrameInput::default()
            });
        }
    }

    #[test]
    fn camera_key_taps_outside_and_holds_drop_cam() {
        let mut rig = Rig::new();
        rig.key(1);
        assert_eq!(rig.camera.view(), CarView::Outside);
        let behind = rig.camera.pose().unwrap().position;
        assert!(behind[2] > 4. && behind[1] > 1., "{behind:?}");
        rig.key(1);
        assert_eq!(rig.camera.view(), CarView::Cockpit);
        rig.key(25);
        assert_eq!(
            rig.camera.view(),
            CarView::Drop {
                follow: false,
                outside_before: false
            }
        );
        rig.key(25);
        assert_eq!(
            rig.camera.view(),
            CarView::Drop {
                follow: true,
                outside_before: false
            }
        );
        rig.key(1);
        assert_eq!(rig.camera.view(), CarView::Cockpit);
    }

    #[test]
    fn zoom_clamps_and_recenter_returns() {
        let mut rig = Rig::new();
        rig.key(1);
        for wheel in [-1000., 1000.] {
            rig.run(FrameInput {
                wheel,
                ..FrameInput::default()
            });
            let limit = if wheel < 0. { 30. } else { 1. };
            assert_eq!(rig.camera.distance, limit);
        }
        for _ in 0..200 {
            rig.run(FrameInput {
                held: keys::RECENTER,
                ..FrameInput::default()
            });
        }
        assert!((rig.camera.distance - 5.).abs() < 0.01);
    }

    #[test]
    fn map_rises_and_returns() {
        let mut rig = Rig::new();
        rig.map_key();
        assert_eq!(rig.camera.view(), CarView::Map);
        rig.idle(17);
        let high = rig.camera.pose().unwrap().position;
        assert!((high[1] - 2.5).abs() < 0.01, "{high:?}");
        let down = rig.camera.pose().unwrap().rotation;
        assert!((Quat::from_array(down) * -Vec3::Z).y < -0.99);
        for _ in 0..1000 {
            rig.run(FrameInput {
                wheel: -1.,
                ..FrameInput::default()
            });
        }
        assert!((rig.camera.pose().unwrap().position[1] - 200.).abs() < 0.01);
        rig.map_key();
        rig.idle(17);
        assert_eq!(rig.camera.view(), CarView::Cockpit);
    }

    #[test]
    fn leaving_the_seat_returns_to_cockpit() {
        let mut rig = Rig::new();
        rig.key(1);
        let mut input = CameraInput::default();
        rig.camera.frame(
            &rig.reference,
            &mut input,
            None,
            (Vec3::Y, Quat::IDENTITY),
            DT,
        );
        assert_eq!(rig.camera.view(), CarView::Cockpit);
    }
}
