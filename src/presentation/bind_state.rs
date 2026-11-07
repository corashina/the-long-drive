//! Load phase of a presentation scene instance.

/// Order: Loading, then Loaded (scene spawned), then Bound or Failed (bind), then Shown
/// (first show). A failed instance stays hidden and binding does not run again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BindState {
    Loading,
    Loaded,
    Bound,
    Shown,
    Failed,
}
