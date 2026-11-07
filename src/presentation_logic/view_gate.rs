//! The rule that decides when a surface view can draw this frame. `frame_graph` gathers the facts.

/// Readiness of the frame-wide passes. Every view needs all of them.
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameFacts {
    /// The frame has this view.
    pub view: bool,
    /// The loading proof lets this frame draw.
    pub allowed: bool,
    pub camera: bool,
    pub display: bool,
    pub effects: bool,
    pub preprocess: bool,
    /// The sky drew this frame.
    pub sky: bool,
    /// The world view presented this frame.
    pub presented: bool,
}

/// Readiness of the passes that one view kind needs.
pub(crate) enum ViewFacts {
    World,
    /// The mirror copy pipeline exists.
    Mirror {
        copy: bool,
    },
    /// The frame captures this probe face and the probe takes it.
    Probe {
        face: bool,
        probe: bool,
    },
}

/// True when the view may draw. Mirrors and probes draw only after the world view presented.
pub(crate) fn view_may_draw(frame: FrameFacts, view: ViewFacts) -> bool {
    frame_blocker(frame).is_none()
        && match view {
            ViewFacts::World => true,
            ViewFacts::Mirror { copy } => frame.presented && copy,
            ViewFacts::Probe { face, probe } => frame.presented && face && probe,
        }
}

/// The first frame-wide fact that holds every view, or `None`. F3 shows it for the world view.
pub(crate) fn frame_blocker(frame: FrameFacts) -> Option<&'static str> {
    [
        (frame.view, "no view"),
        (frame.allowed, "loading frame"),
        (frame.camera, "camera"),
        (frame.display, "display"),
        (frame.effects, "effects"),
        (frame.preprocess, "preprocess"),
        (frame.sky, "sky"),
    ]
    .into_iter()
    .find_map(|(ready, gate)| (!ready).then_some(gate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> FrameFacts {
        FrameFacts {
            view: true,
            allowed: true,
            camera: true,
            display: true,
            effects: true,
            preprocess: true,
            sky: true,
            presented: false,
        }
    }

    #[test]
    fn mirrors_and_probes_wait_for_the_world_view() {
        assert!(view_may_draw(ready(), ViewFacts::World));
        assert!(!view_may_draw(ready(), ViewFacts::Mirror { copy: true }));
        let presented = FrameFacts {
            presented: true,
            ..ready()
        };
        assert!(view_may_draw(presented, ViewFacts::Mirror { copy: true }));
        assert!(!view_may_draw(presented, ViewFacts::Mirror { copy: false }));
        assert!(view_may_draw(
            presented,
            ViewFacts::Probe {
                face: true,
                probe: true
            }
        ));
        assert!(!view_may_draw(
            presented,
            ViewFacts::Probe {
                face: false,
                probe: true
            }
        ));
        let skipped = FrameFacts {
            allowed: false,
            ..presented
        };
        assert!(!view_may_draw(skipped, ViewFacts::World));
    }
}
