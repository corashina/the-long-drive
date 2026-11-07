//! The change of an ordered list: kept runs of the old list and new items, in the new order.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum Run {
    /// `count` old items from `start`.
    Keep { start: u32, count: u32 },
    /// The next `count` new items.
    Add { count: u32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListDelta<T> {
    changed: bool,
    runs: Vec<Run>,
    added: Vec<T>,
    /// Old positions that the diff keeps. Reused by each diff.
    #[serde(skip)]
    used: Vec<bool>,
}
impl<T> Default for ListDelta<T> {
    fn default() -> Self {
        Self {
            changed: false,
            runs: Vec::new(),
            added: Vec::new(),
            used: Vec::new(),
        }
    }
}
impl<T: Clone> ListDelta<T> {
    pub fn clear(&mut self) {
        self.changed = false;
        self.runs.clear();
        self.added.clear();
    }
    pub fn changed(&self) -> bool {
        self.changed
    }
    /// Writes the change from `old` to `new` and applies it to `old`. `find` gives the old
    /// position of an item that is the same as the new item. `make` makes a new item.
    pub fn diff<N>(
        &mut self,
        old: &mut Vec<T>,
        new: &[N],
        mut find: impl FnMut(&[T], &N) -> Option<usize>,
        make: impl Fn(&N) -> T,
    ) {
        self.clear();
        self.changed = true;
        self.used.clear();
        self.used.resize(old.len(), false);
        for item in new {
            match find(old, item).filter(|&i| !self.used[i]) {
                Some(i) => {
                    self.used[i] = true;
                    self.keep(i as u32);
                }
                None => {
                    self.added.push(make(item));
                    self.add();
                }
            }
        }
        self.apply(old);
    }
    fn keep(&mut self, i: u32) {
        if let Some(Run::Keep { start, count }) = self.runs.last_mut()
            && *start + *count == i
        {
            *count += 1;
            return;
        }
        self.runs.push(Run::Keep { start: i, count: 1 });
    }
    fn add(&mut self) {
        if let Some(Run::Add { count }) = self.runs.last_mut() {
            *count += 1;
            return;
        }
        self.runs.push(Run::Add { count: 1 });
    }
    /// Applies the change to the old list. It moves kept items and clones only new items.
    pub fn apply(&self, list: &mut Vec<T>) {
        if !self.changed {
            return;
        }
        let mut old: Vec<Option<T>> = std::mem::take(list).into_iter().map(Some).collect();
        let mut added = self.added.iter();
        for run in &self.runs {
            match *run {
                Run::Keep { start, count } => list.extend(
                    old[start as usize..(start + count) as usize]
                        .iter_mut()
                        .map(|item| item.take().expect("delta keeps an old item once")),
                ),
                Run::Add { count } => list.extend(added.by_ref().take(count as usize).cloned()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_gives_the_new_list() {
        let mut sent = vec![1, 2, 3, 4, 5];
        let mut copy = sent.clone();
        let new = [5, 2, 3, 9, 1, 8, 8];
        let mut delta = ListDelta::default();
        delta.diff(
            &mut sent,
            &new,
            |old, n| old.iter().position(|o| o == n),
            |n| *n,
        );
        assert_eq!(sent, new);
        let delta: ListDelta<i32> =
            postcard::from_bytes(&postcard::to_allocvec(&delta).unwrap()).unwrap();
        delta.apply(&mut copy);
        assert_eq!(copy, new);
    }
}
