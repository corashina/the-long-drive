//! Main-view TAA jitter and camera motion history. Source `pplayermelle.TurnOn` enables TAA with antialiasing.
//! Motion blur reads the same history without jitter.
use super::view_frame::ViewSnapshot;
use crate::reference::{postprocessing::TaaSettings, temporal::TaaSequence};
use bevy::{ecs as bevy_ecs, prelude::*};

/// Camera matrices without jitter. Motion vectors and TAA reprojection use them.
#[derive(Clone, Copy)]
pub struct MainMotion {
    /// Jitter in uv. The scene image at `uv - jitter` is the unjittered image at `uv`.
    pub jitter: Vec2,
    pub clip_from_world: Mat4,
    pub previous_clip_from_world: Mat4,
    /// True when no valid history exists: first frame, scene restart, camera cut, or resize.
    pub reset: bool,
}

struct Previous {
    clip_from_world: Mat4,
    origin: [f64; 3],
    revision: u32,
    scene: u32,
    extent: UVec2,
}

#[derive(Resource, Default)]
pub struct TemporalView {
    sequence: TaaSequence,
    previous: Option<Previous>,
}
impl TemporalView {
    /// Return the motion state of this frame. With TAA, jitter the main raster projection.
    pub fn advance(
        &mut self,
        view: &mut ViewSnapshot,
        taa: Option<&TaaSettings>,
        origin: [f64; 3],
        revision: u32,
        scene: u32,
    ) -> MainMotion {
        let clip_from_world = view.raster_projection * view.view_from_world;
        let jitter = taa.map_or(Vec2::ZERO, |settings| {
            Vec2::from_array(self.sequence.advance(settings.jitter_spread)) / view.extent.as_vec2()
        });
        view.raster_projection = jitter_clip(jitter) * view.raster_projection;
        let previous = self.previous.replace(Previous {
            clip_from_world,
            origin,
            revision,
            scene,
            extent: view.extent,
        });
        let previous = previous
            .filter(|p| p.revision == revision && p.scene == scene && p.extent == view.extent);
        MainMotion {
            jitter,
            clip_from_world,
            // Runtime poses subtract the origin. Move the old matrix into the current origin.
            previous_clip_from_world: previous.as_ref().map_or(clip_from_world, |p| {
                p.clip_from_world
                    * Mat4::from_translation(Vec3::from_array(std::array::from_fn(|i| {
                        (origin[i] - p.origin[i]) as f32
                    })))
            }),
            reset: previous.is_none(),
        }
    }
    pub fn clear(&mut self) {
        self.previous = None;
    }
}

/// Clip-space shift that moves the image by `-jitter` in uv. Uv y points down.
pub fn jitter_clip(jitter: Vec2) -> Mat4 {
    Mat4::from_cols(
        Vec4::X,
        Vec4::Y,
        Vec4::Z,
        Vec4::new(-2. * jitter.x, 2. * jitter.y, 0., 1.),
    )
}
