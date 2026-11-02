//! Source `tosaveitemscript.Claim`: the peer that holds an item owns it and sends its state.
use crate::reference::ItemId;
use std::collections::{HashMap, HashSet};

/// Items this player holds: the carried item, the right-hand item and the 4 inventory slots.
pub const HELD: usize = 6;

/// Source `tosaveitemscript.unClaim` real-time wait.
pub const UNCLAIM_DELAY: f64 = 0.05;

#[derive(Default)]
pub struct Claims {
    /// Source `claimed`.
    mine: HashSet<ItemId>,
    /// Source `otherClaimed`, with the slot of the owner.
    others: HashMap<ItemId, u8>,
    held: [Option<ItemId>; HELD],
    releasing: Vec<(ItemId, f64)>,
}

/// A claim change of this peer, to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimChange {
    Claim(ItemId),
    /// Send the last pose and velocity, then the release.
    Release(ItemId),
}

impl Claims {
    /// Source `(isServer || claimed) && !otherClaimed`.
    pub fn owns(&self, item: ItemId, host: bool) -> bool {
        (host || self.mine.contains(&item)) && !self.others.contains_key(&item)
    }
    pub fn claimed(&self, item: ItemId) -> bool {
        self.mine.contains(&item)
    }
    pub fn other(&self, item: ItemId) -> Option<u8> {
        self.others.get(&item).copied()
    }

    /// Source `pickupable.Pick` claims, `Drop` releases after `UNCLAIM_DELAY`.
    /// `held`: the carried item and the right-hand item of this player.
    pub fn observe(&mut self, held: [Option<ItemId>; HELD], now: f64, out: &mut Vec<ClaimChange>) {
        for item in self.held.into_iter().flatten() {
            if !held.contains(&Some(item)) && self.mine.contains(&item) {
                self.releasing.push((item, now + UNCLAIM_DELAY));
            }
        }
        for item in held.into_iter().flatten() {
            self.releasing.retain(|(i, _)| *i != item);
            // A new pick claims, even from another owner. A later claim by another peer wins.
            if !self.held.contains(&Some(item)) && !self.mine.contains(&item) {
                // Source `DoClaim(true)`.
                self.mine.insert(item);
                self.others.remove(&item);
                out.push(ClaimChange::Claim(item));
            }
        }
        self.held = held;
        let mut i = 0;
        while i < self.releasing.len() {
            let (item, due) = self.releasing[i];
            if due > now {
                i += 1;
                continue;
            }
            self.releasing.swap_remove(i);
            if self.mine.remove(&item) {
                out.push(ClaimChange::Release(item));
            }
        }
    }

    /// Source `snItemsync.Claim`.
    pub fn got(&mut self, item: ItemId, slot: u8, claim: bool) {
        if claim {
            self.others.insert(item, slot);
            self.mine.remove(&item);
            self.releasing.retain(|(i, _)| *i != item);
        } else if self.others.get(&item) == Some(&slot) {
            self.others.remove(&item);
        }
    }

    /// A peer left: its claims end.
    pub fn leave(&mut self, slot: u8) {
        self.others.retain(|_, s| *s != slot);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(carried: Option<ItemId>, hand: Option<ItemId>) -> [Option<ItemId>; HELD] {
        [carried, hand, None, None, None, None]
    }

    #[test]
    fn pick_claims_and_drop_releases_after_the_delay() {
        let (mut claims, mut out, item) = (Claims::default(), Vec::new(), ItemId(4));
        claims.observe(held(Some(item), None), 0., &mut out);
        assert_eq!(out, [ClaimChange::Claim(item)]);
        assert!(claims.owns(item, false));
        out.clear();
        claims.observe(held(None, None), 1., &mut out);
        assert!(out.is_empty(), "the release waits");
        claims.observe(held(None, None), 1.04, &mut out);
        assert!(out.is_empty());
        claims.observe(held(None, None), 1.06, &mut out);
        assert_eq!(out, [ClaimChange::Release(item)]);
        assert!(!claims.owns(item, false));
        assert!(claims.owns(item, true), "the host owns unclaimed items");
    }

    #[test]
    fn a_pick_before_the_delay_keeps_the_claim() {
        let (mut claims, mut out, item) = (Claims::default(), Vec::new(), ItemId(4));
        claims.observe(held(Some(item), None), 0., &mut out);
        claims.observe(held(None, None), 1., &mut out);
        claims.observe(held(None, Some(item)), 1.01, &mut out);
        claims.observe(held(None, Some(item)), 2., &mut out);
        assert_eq!(out, [ClaimChange::Claim(item)]);
    }

    #[test]
    fn another_claim_takes_the_item() {
        let (mut claims, mut out, item) = (Claims::default(), Vec::new(), ItemId(4));
        claims.observe(held(Some(item), None), 0., &mut out);
        claims.got(item, 2, true);
        assert!(!claims.owns(item, true), "not even the host owns it");
        assert_eq!(claims.other(item), Some(2));
        out.clear();
        claims.observe(held(Some(item), None), 1., &mut out);
        assert!(out.is_empty(), "a held item does not claim again");
        claims.got(item, 3, false);
        assert_eq!(claims.other(item), Some(2), "only the owner releases");
        claims.leave(2);
        assert!(claims.owns(item, true));
    }
}
