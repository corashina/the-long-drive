//! Content kinds bound to active generated contents. One scan binds and drops every kind.
use super::WorldStream;
use super::content_busdoors::{BusDoorBinding, BusDoorKind};
use super::content_detectors::{DetectorBinding, DetectorKind};
use super::content_dials::DialBinding;
use super::content_electronics::ElectronicsBinding;
use super::content_followers::{FollowerBinding, FollowerKind};
use super::content_levels::{LevelBinding, LevelKind};
use super::content_musicboxes::{MusicBoxBinding, MusicBoxKind};
use super::content_radios::{RadioBinding, RadioKind};
use super::content_seats::{PrototypeSeats, SeatBinding, SeatKind};
use super::content_sleds::{SledBinding, SledKind};
use super::content_trailers::{HitchKind, HitchOwner, TrailerBinding, TrailerKind};
use crate::interaction::InteractionState;
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, ReferenceError, ReferenceManifest, SourceId, WorldReference};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::spawn_rules::SourceRules;

/// One binding of a content kind.
pub(super) trait Bound {
    fn parent(&self) -> &GeneratedId;
    /// The bound content item. None: the binding belongs to its parent only.
    fn item(&self) -> Option<ItemId>;
    /// The binding's content moved to this active slot.
    fn set_slot(&mut self, _slot: usize) {}
    fn heap_bytes(&self) -> usize;
}

/// The bindings of one kind and the session and content revision they cover.
pub(super) struct KindBindings<B> {
    pub list: Vec<B>,
    pub scan: Option<(u64, u64)>,
}
impl<B: Bound> KindBindings<B> {
    fn new() -> Self {
        Self {
            list: Vec::new(),
            scan: None,
        }
    }
    fn heap_bytes(&self) -> usize {
        self.list.capacity() * std::mem::size_of::<B>()
            + self.list.iter().map(B::heap_bytes).sum::<usize>()
    }
}

/// The world state a kind binds into.
pub(super) struct BindContext<'a> {
    pub reference: &'a ReferenceManifest,
    pub items: &'a mut ItemStore,
    pub interaction: &'a mut InteractionState,
    pub physics: &'a mut PhysicsWorld,
}

pub(super) enum Ready {
    Bind,
    /// Scan again on the next call.
    Wait,
    Skip,
}

/// One content kind: which contents it binds and how.
pub(super) trait ContentKind {
    type Binding: Bound;
    type Script;
    /// Keyed by the content item. False: keyed by the content parent, with or without an item.
    const BY_ITEM: bool = true;
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<Self::Binding>;
    /// The kind's script on the active content. None: the content is not of this kind.
    fn script(
        stream: &mut WorldStream,
        cx: &BindContext,
        slot: usize,
    ) -> Result<Option<Self::Script>, ReferenceError>;
    fn ready(stream: &WorldStream, items: &ItemStore, slot: usize) -> Ready {
        let content = &stream.contents.active()[slot];
        if content.restored && has_body(items, content.item) {
            Ready::Bind
        } else {
            Ready::Wait
        }
    }
    /// None: not ready yet.
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        item: Option<ItemId>,
        script: Self::Script,
    ) -> Result<Option<Self::Binding>, ReferenceError>;
    /// Called after the binding at `index` is added.
    fn bound(_stream: &mut WorldStream, _cx: &mut BindContext, _index: usize) {}
    fn unbind(_binding: &Self::Binding, _interaction: &mut InteractionState) {}
}

pub(super) fn has_body(items: &ItemStore, item: Option<ItemId>) -> bool {
    item.and_then(|i| items.get(i))
        .and_then(|i| i.body)
        .is_some()
}

/// A restored content waits. A removed item has no body and nothing to bind.
pub(super) fn ready_unless_removed(stream: &WorldStream, items: &ItemStore, slot: usize) -> Ready {
    let content = &stream.contents.active()[slot];
    if !content.restored {
        Ready::Wait
    } else if has_body(items, content.item) {
        Ready::Bind
    } else {
        Ready::Skip
    }
}

/// Every bound content kind with its prototype lookups.
pub(super) struct ContentKinds {
    pub dials: KindBindings<DialBinding>,
    pub levels: KindBindings<LevelBinding>,
    pub radios: KindBindings<RadioBinding>,
    /// Prototype radioscript lookups.
    pub radio_scripts: Vec<(SourceId, Option<SourceId>)>,
    pub music_boxes: KindBindings<MusicBoxBinding>,
    pub electronics: KindBindings<ElectronicsBinding>,
    pub detectors: KindBindings<DetectorBinding>,
    pub bus_doors: KindBindings<BusDoorBinding>,
    /// `busdoorscripts` and `dinamiccolliderforbus` sources by prototype.
    pub bus_door_scripts: Vec<(SourceId, Option<SourceId>, Vec<SourceId>)>,
    pub seats: KindBindings<SeatBinding>,
    /// Seats of world prototypes, read and checked on world load.
    pub seat_prototypes: Vec<PrototypeSeats>,
    pub sleds: KindBindings<SledBinding>,
    /// Indices of world prototypes with a sled.
    pub sled_prototypes: Vec<usize>,
    pub hitches: KindBindings<HitchOwner>,
    pub trailers: KindBindings<TrailerBinding>,
    /// Indices of world prototypes with a trailer or hitch targets.
    pub trailer_prototypes: Vec<usize>,
    pub followers: KindBindings<FollowerBinding>,
    /// Indices of world prototypes with followers.
    pub follower_prototypes: Vec<usize>,
}

impl ContentKinds {
    pub fn new(world: &WorldReference, rules: &SourceRules) -> Result<Self, ReferenceError> {
        let prototypes = |with: fn(&crate::reference::WorldPrototypeReference) -> bool| {
            (world.prototypes.iter().enumerate())
                .filter(|(_, p)| with(p))
                .map(|(i, _)| i)
                .collect()
        };
        Ok(Self {
            dials: KindBindings::new(),
            levels: KindBindings::new(),
            radios: KindBindings::new(),
            radio_scripts: Vec::new(),
            music_boxes: KindBindings::new(),
            electronics: KindBindings::new(),
            detectors: KindBindings::new(),
            bus_doors: KindBindings::new(),
            bus_door_scripts: Vec::new(),
            seats: KindBindings::new(),
            seat_prototypes: super::content_seats::world_seats(world, rules)?,
            sleds: KindBindings::new(),
            sled_prototypes: prototypes(|p| p.sled.is_some()),
            hitches: KindBindings::new(),
            trailers: KindBindings::new(),
            trailer_prototypes: prototypes(|p| p.trailer.is_some() || !p.hitches.is_empty()),
            followers: KindBindings::new(),
            follower_prototypes: prototypes(|p| p.followers.is_some()),
        })
    }

    pub fn heap_bytes(&self) -> usize {
        self.dials.heap_bytes()
            + self.levels.heap_bytes()
            + self.radios.heap_bytes()
            + self.radio_scripts.capacity() * std::mem::size_of::<(SourceId, Option<SourceId>)>()
            + self.music_boxes.heap_bytes()
            + self.electronics.heap_bytes()
            + self.detectors.heap_bytes()
            + self.bus_doors.heap_bytes()
            + self.bus_door_scripts.capacity()
                * std::mem::size_of::<(SourceId, Option<SourceId>, Vec<SourceId>)>()
            + self.seats.heap_bytes()
            + self.seat_prototypes.capacity() * std::mem::size_of::<PrototypeSeats>()
            + super::content_seats::prototype_seats_heap(&self.seat_prototypes)
            + self.sleds.heap_bytes()
            + self.sled_prototypes.capacity() * std::mem::size_of::<usize>()
            + self.hitches.heap_bytes()
            + self.trailers.heap_bytes()
            + self.trailer_prototypes.capacity() * std::mem::size_of::<usize>()
            + self.followers.heap_bytes()
            + self.follower_prototypes.capacity() * std::mem::size_of::<usize>()
    }
}

impl WorldStream {
    /// The kinds bound after the computers, in their sync order.
    pub(super) fn sync_content_kinds(
        &mut self,
        cx: &mut BindContext,
    ) -> Result<(), ReferenceError> {
        self.sync_kind::<LevelKind>(cx)?;
        self.sync_kind::<RadioKind>(cx)?;
        self.sync_kind::<MusicBoxKind>(cx)?;
        self.sync_electronics_bindings()?;
        self.sync_kind::<DetectorKind>(cx)?;
        self.sync_kind::<BusDoorKind>(cx)?;
        self.sync_kind::<SeatKind>(cx)?;
        self.sync_kind::<SledKind>(cx)?;
        self.sync_kind::<HitchKind>(cx)?;
        self.sync_kind::<TrailerKind>(cx)?;
        self.sync_kind::<FollowerKind>(cx)
    }

    /// Drops the bindings of gone contents and binds new contents of kind `K`.
    pub(super) fn sync_kind<K: ContentKind>(
        &mut self,
        cx: &mut BindContext,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        let kind = K::kind(&mut self.kinds);
        if kind.scan == Some(revision) {
            return Ok(());
        }
        let active = self.contents.active();
        kind.list.retain_mut(|b| {
            let slot = active.iter().position(|c| {
                &c.content.placement.id == b.parent() && b.item().is_none_or(|i| c.item == Some(i))
            });
            match slot {
                Some(slot) => b.set_slot(slot),
                None => K::unbind(b, cx.interaction),
            }
            slot.is_some()
        });
        let mut complete = true;
        for slot in 0..self.contents.active().len() {
            let content = &self.contents.active()[slot];
            let item = content.item;
            if K::BY_ITEM && item.is_none() {
                continue;
            }
            let parent = &content.content.placement.id;
            if K::kind(&mut self.kinds)
                .list
                .iter()
                .any(|b| match b.item() {
                    Some(bound) if K::BY_ITEM => item == Some(bound),
                    _ => b.parent() == parent,
                })
            {
                continue;
            }
            let Some(script) = K::script(self, cx, slot)? else {
                continue;
            };
            match K::ready(self, cx.items, slot) {
                Ready::Bind => {}
                Ready::Wait => {
                    complete = false;
                    continue;
                }
                Ready::Skip => continue,
            }
            let Some(binding) = K::bind(self, cx, slot, item, script)? else {
                complete = false;
                continue;
            };
            let list = &mut K::kind(&mut self.kinds).list;
            list.push(binding);
            let index = list.len() - 1;
            K::bound(self, cx, index);
        }
        K::kind(&mut self.kinds).scan = complete.then_some(revision);
        Ok(())
    }
}
