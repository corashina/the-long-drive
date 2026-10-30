//! Conservative container payload bounds for shared interaction ownership.
use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
};
pub(super) fn map<K, V>(m: &HashMap<K, V>) -> usize {
    table(m.capacity(), size_of::<(K, V)>())
}
pub fn platform_map<K, V>(m: &bevy_platform::collections::HashMap<K, V>) -> usize {
    table(m.capacity(), size_of::<(K, V)>())
}
pub(super) fn set<K>(m: &HashSet<K>) -> usize {
    table(m.capacity(), size_of::<K>())
}
fn table(capacity: usize, width: usize) -> usize {
    if capacity == 0 {
        0
    } else {
        capacity.next_power_of_two() * (width + 1) + 16
    }
}
pub(super) fn vector<T>(v: &Vec<T>) -> usize {
    v.capacity() * size_of::<T>()
}
pub(super) fn strings(v: &Vec<String>) -> usize {
    vector(v) + v.iter().map(String::capacity).sum::<usize>()
}
pub fn mechanism(r: &crate::reference::MechanismReference) -> usize {
    r.source.file.capacity()
        + vector(&r.driven_nodes)
        + r.rotation.as_ref().map_or(0, |r| vector(&r.targets))
        + r.slide.as_ref().map_or(0, |s| {
            s.animation_source.as_ref().map_or(0, |s| s.file.capacity())
                + vector(&s.paths)
                + s.paths.iter().map(|p| vector(&p.points)).sum::<usize>()
                + vector(&s.rotations)
        })
        + r.turn.as_ref().map_or(0, |t| {
            if let crate::reference::TurnAction::ToggleHomeLight { source } = &t.action {
                source.file.capacity()
            } else {
                0
            }
        })
}
