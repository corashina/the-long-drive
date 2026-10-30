//! Source seatscript seats on world prototypes and their hit colliders on spawned items.
use super::WorldStream;
use crate::interaction::InteractionState;
use crate::interaction::items::ItemStore;
use crate::player::SeatOwner;
use crate::player::body::SeatFrames;
use crate::reference::{
    ItemId, ReferenceError, SourceId, Transform, WorldPrototypeReference, WorldReference,
};
use crate::vehicle::{PhysicsWorld, VehiclePose, from_rapier, to_rapier};
use crate::world::spawn_rules::*;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::prelude::ColliderHandle;
use rapier3d::prelude::RigidBodyHandle;
use std::sync::Arc;

/// Player view pitch clamp of the source seat look (`FyRot = Mathf.Clamp(FyRot, -89f, 89f)`).
const SEAT_PITCH_LIMITS_DEGREES: [f32; 2] = [-89.0, 89.0];
/// Length of each player sit pose array (`LHSitPose` and the others).
const SIT_POSE_COUNT: u32 = 3;

/// One main seatscript of a world prototype. Node values are prototype node indices.
/// The exit distance is `PlayerReference::exit_distance`.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSeatReference {
    /// The main seatscript. It is the save key of the seat.
    pub source: SourceId,
    /// `sitPos`, or the seatscript node.
    pub sit_node: u32,
    /// Nodes of the main seatscript and of all seatscripts whose `mainseat` is this seat.
    /// The first node is the main seatscript node.
    pub hit_nodes: Vec<u32>,
    pub yaw_limits_degrees: [f32; 2],
    /// `limitLessRot`: no yaw clamp.
    pub yaw_unlimited: bool,
    /// `FcantSitAngle` when `BcantSitAngle` is set.
    pub cant_sit_angle_degrees: Option<f32>,
    /// `lookInsteadOfLean`.
    pub look_instead_of_lean: bool,
    /// `rootOrigin`: the outside view pivots on the root, not on the seat.
    pub root_origin: bool,
    pub pitch_limits_degrees: [f32; 2],
    pub default_pitch_degrees: f32,
    pub min_exit_angle_degrees: f32,
    pub exit_height: f32,
    pub show_sleep_text: bool,
    /// `driverSeat0` and `driverSeat1` of a car seat, from the prototype vehicle record.
    pub driver: bool,
    pub driving2: bool,
    pub body_pose: WorldSeatBodyPose,
}

/// Seated player body inputs of a main seatscript. Nodes are prototype node indices.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSeatBodyPose {
    /// `defPose`: the index into the player sit poses.
    pub def_pose: u32,
    /// `LHpos`, `RHpos`, `LLpos`, `RLpos`. `None` for a null link.
    pub left_hand: Option<u32>,
    pub right_hand: Option<u32>,
    pub left_leg: Option<u32>,
    pub right_leg: Option<u32>,
}

/// Active seatscript seats of a prototype, in source hierarchy order.
pub fn prototype_seats(
    reference: &WorldReference,
    rules: &SourceRules,
    prototype: &WorldPrototypeReference,
) -> Result<Vec<WorldSeatReference>, ReferenceError> {
    let mut scripts = Vec::new();
    rules.children(
        reference,
        &prototype.source,
        "seatscript",
        false,
        &mut scripts,
    )?;
    if scripts.is_empty() {
        return Ok(Vec::new());
    }
    let node_of = |object: &SourceId| -> Result<u32, ReferenceError> {
        let transform = rules.root(reference, object)?;
        prototype
            .nodes
            .iter()
            .position(|n| &n.source == transform)
            .map(|i| i as u32)
            .ok_or_else(|| ReferenceError("node outside its prototype".into()))
    };
    let mut seats = Vec::new();
    let mut secondary = Vec::new();
    for &script in &scripts {
        let seat = |e: ReferenceError| {
            ReferenceError(format!(
                "Unreadable seat {}:{} of prototype {}:{}: {}",
                script.file, script.path_id, prototype.source.file, prototype.source.path_id, e.0
            ))
        };
        let f = rules.get(reference, script).map_err(seat)?.fields;
        let node = node_of(script).map_err(seat)?;
        if let Some(main) = optional(f, "mainseat").map_err(seat)?
            && main != script
        {
            secondary.push((main, node));
            continue;
        }
        seats.push(main_seat(script, f, node, &node_of).map_err(seat)?);
    }
    for (main, node) in secondary {
        let seat = seats
            .iter_mut()
            .find(|s| &s.source == main)
            .ok_or_else(|| {
                ReferenceError(format!(
                    "Seat main seat {}:{} is not an active main seat of prototype {}:{}",
                    main.file, main.path_id, prototype.source.file, prototype.source.path_id
                ))
            })?;
        seat.hit_nodes.push(node);
    }
    Ok(seats)
}

fn main_seat(
    script: &SourceId,
    f: Fields<'_>,
    node: u32,
    node_of: &impl Fn(&SourceId) -> Result<u32, ReferenceError>,
) -> Result<WorldSeatReference, ReferenceError> {
    let sit_node = match optional(f, "sitPos")? {
        Some(sit) => node_of(sit)?,
        None => node,
    };
    let rot_limit = number(f, "rotLimit")?;
    let frame = |n| optional(f, n)?.map(node_of).transpose();
    let def_pose = u32::try_from(integer(f, "defPose")?)
        .ok()
        .filter(|p| *p < SIT_POSE_COUNT)
        .ok_or_else(|| ReferenceError("invalid seat defPose".into()))?;
    let seat = WorldSeatReference {
        source: script.clone(),
        sit_node,
        hit_nodes: vec![node],
        yaw_limits_degrees: [-rot_limit, rot_limit],
        yaw_unlimited: flag(f, "limitLessRot")?,
        cant_sit_angle_degrees: if flag(f, "BcantSitAngle")? {
            Some(number(f, "FcantSitAngle")?)
        } else {
            None
        },
        look_instead_of_lean: flag(f, "lookInsteadOfLean")?,
        root_origin: flag(f, "rootOrigin")?,
        pitch_limits_degrees: SEAT_PITCH_LIMITS_DEGREES,
        default_pitch_degrees: number(f, "defYrot")?,
        min_exit_angle_degrees: number(f, "minGetUpAnlge")?,
        exit_height: number(f, "getOutHeight")?,
        show_sleep_text: flag(f, "showSleepText")?,
        driver: false,
        driving2: false,
        body_pose: WorldSeatBodyPose {
            def_pose,
            left_hand: frame("LHpos")?,
            right_hand: frame("RHpos")?,
            left_leg: frame("LLpos")?,
            right_leg: frame("RLpos")?,
        },
    };
    let values = [
        rot_limit,
        seat.default_pitch_degrees,
        seat.min_exit_angle_degrees,
        seat.exit_height,
        seat.cant_sit_angle_degrees.unwrap_or(0.),
    ];
    if values.iter().any(|v| !v.is_finite())
        || rot_limit < 0.
        || !(SEAT_PITCH_LIMITS_DEGREES[0]..=SEAT_PITCH_LIMITS_DEGREES[1])
            .contains(&seat.default_pitch_degrees)
    {
        return Err(ReferenceError("invalid seat limits".into()));
    }
    Ok(seat)
}

/// One seat of a spawned world item. A car copy owns its seats as a vehicle.
pub struct ItemSeat {
    pub item: ItemId,
    /// The nearest item that holds the main seatscript node: a seat part or the item itself.
    pub seat_item: ItemId,
    pub owner: SeatOwner,
    pub body: RigidBodyHandle,
    /// sitPos relative to the item body.
    pub pose: VehiclePose,
    /// Main seatscript node relative to the item body.
    pub node_pose: VehiclePose,
    pub seat: Arc<WorldSeatReference>,
    /// Player body frames relative to the item body. Car seats add the car drive frames.
    pub frames: SeatFrames,
}
impl ItemSeat {
    /// Source `BcantSitAngle` enter check on the main seatscript node.
    pub fn can_sit(&self, physics: &PhysicsWorld) -> bool {
        let node = *physics.rapier().bodies[self.body].position() * to_rapier(self.node_pose);
        sit_angle_allows(
            node.rotation * rapier3d::math::Vector::Y,
            self.seat.cant_sit_angle_degrees,
        )
    }
}

/// `!BcantSitAngle || Vector3.Angle(seat.up, Vector3.up) < FcantSitAngle`.
fn sit_angle_allows(up: rapier3d::math::Vector, cant_sit_angle_degrees: Option<f32>) -> bool {
    cant_sit_angle_degrees.is_none_or(|limit| {
        up.normalize_or_zero()
            .dot(rapier3d::math::Vector::Y)
            .clamp(-1., 1.)
            .acos()
            .to_degrees()
            < limit
    })
}

/// The seats of one spawned world item and their hit colliders.
pub struct SeatBinding {
    parent: GeneratedId,
    item: ItemId,
    seats: Vec<(Arc<ItemSeat>, Vec<ColliderHandle>)>,
}
impl SeatBinding {
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.seats.capacity() * std::mem::size_of::<(Arc<ItemSeat>, Vec<ColliderHandle>)>()
            + self
                .seats
                .iter()
                .map(|(seat, colliders)| {
                    std::mem::size_of::<ItemSeat>()
                        + colliders.capacity() * std::mem::size_of::<ColliderHandle>()
                        + match &seat.owner {
                            SeatOwner::Vehicle(ObjectKey::Generated(id))
                            | SeatOwner::Item(ObjectKey::Generated(id)) => {
                                crate::world::contents::identity_heap(id)
                            }
                            _ => 0,
                        }
                })
                .sum::<usize>()
    }
}

/// Prototype seats with shared records.
pub type PrototypeSeats = (SourceId, Vec<Arc<WorldSeatReference>>);

pub fn prototype_seats_heap(seats: &[PrototypeSeats]) -> usize {
    seats
        .iter()
        .map(|(source, seats)| {
            source.file.capacity()
                + seats.capacity() * std::mem::size_of::<Arc<WorldSeatReference>>()
                + seats
                    .iter()
                    .map(|s| {
                        std::mem::size_of::<WorldSeatReference>()
                            + s.source.file.capacity()
                            + s.hit_nodes.capacity() * std::mem::size_of::<u32>()
                    })
                    .sum::<usize>()
        })
        .sum()
}

/// The seats of every world prototype that has seats. The world load stops on an unreadable seat.
/// Car seats take their driver roles from the prototype vehicle record.
pub fn world_seats(
    reference: &WorldReference,
    rules: &SourceRules,
) -> Result<Vec<PrototypeSeats>, ReferenceError> {
    let mut seats = Vec::new();
    for prototype in &reference.prototypes {
        let mut records = prototype_seats(reference, rules, prototype)?;
        if let Some(vehicle) = &prototype.vehicle {
            for seat in &mut records {
                if let Some(record) = vehicle.seats.iter().find(|r| r.source == seat.source) {
                    seat.driver = record.driver;
                    seat.driving2 = record.driving2;
                }
            }
        }
        if !records.is_empty() {
            seats.push((
                prototype.source.clone(),
                records.into_iter().map(Arc::new).collect(),
            ));
        }
    }
    Ok(seats)
}

impl WorldStream {
    /// The seats of a prototype. A prototype without seats has none.
    fn seats_of(&self, prototype: &SourceId) -> &[Arc<WorldSeatReference>] {
        self.seat_prototypes
            .iter()
            .find(|(p, _)| p == prototype)
            .map_or(&[], |(_, seats)| seats)
    }
    pub(super) fn sync_seat_bindings(
        &mut self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.seat_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.seat_bindings.len() {
            let b = &self.seat_bindings[i];
            if self
                .contents
                .active
                .iter()
                .any(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
            {
                i += 1;
            } else {
                for (_, colliders) in &b.seats {
                    for handle in colliders {
                        interaction.remove_item_seat(*handle);
                    }
                }
                self.seat_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            if self.seat_bindings.iter().any(|b| b.item == item) {
                continue;
            }
            let seats = self.seats_of(&content.content.placement.prototype);
            if seats.is_empty() {
                continue;
            }
            if !content.restored {
                complete = false;
                continue;
            }
            // A removed item has no body and no seats to bind.
            if items.get(item).and_then(|i| i.body).is_none() {
                continue;
            }
            let seats = seats.to_vec();
            let binding = self.bind_seats(items, interaction, physics, slot, item, seats)?;
            self.seat_bindings.push(binding);
        }
        self.seat_scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_seats(
        &self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &PhysicsWorld,
        slot: usize,
        item: ItemId,
        seats: Vec<Arc<WorldSeatReference>>,
    ) -> Result<SeatBinding, ReferenceError> {
        let body = items
            .get(item)
            .and_then(|i| i.body)
            .ok_or_else(|| ReferenceError("Seat item has no body".into()))?;
        let placement = &self.contents.active[slot].content.placement;
        let prototype = self
            .reference
            .prototypes
            .iter()
            .find(|p| p.source == placement.prototype)
            .ok_or_else(|| ReferenceError("Seat prototype missing".into()))?;
        let instance = self
            .structures
            .iter()
            .find(|s| s.placement.id == placement.id)
            .ok_or_else(|| ReferenceError("Seat item has no published structure".into()))?;
        // A body that the item binding adds for its controls sits at the item root.
        let body_node = instance
            .bodies
            .iter()
            .position(|b| *b == body)
            .and_then(|index| prototype.bodies.get(index))
            .map_or(0, |b| b.node as usize);
        let scale = placement.scale;
        let place = |t: Transform| {
            to_rapier(VehiclePose {
                position: std::array::from_fn(|i| t.translation[i] * scale[i]),
                rotation: t.rotation,
            })
        };
        let body_pose = place(crate::world::placement::node_pose(prototype, body_node));
        let frame = |node: Option<u32>| {
            node.map(|n| {
                from_rapier(
                    &(body_pose.inverse()
                        * place(crate::world::placement::node_pose(prototype, n as usize))),
                )
            })
        };
        let drive = prototype.vehicle.as_ref().map(|v| &v.drive_frames);
        let owner = match physics.vehicle_of(body) {
            Some(vehicle) => SeatOwner::Vehicle(vehicle.clone()),
            None => SeatOwner::Item(items.object_key(item)),
        };
        let mut bound = Vec::with_capacity(seats.len());
        for seat in seats {
            let mut colliders = Vec::new();
            for (source, handle) in &instance.colliders {
                let on_seat = prototype
                    .colliders
                    .iter()
                    .find(|c| &c.source == source)
                    .is_some_and(|c| seat.hit_nodes.contains(&c.node));
                if on_seat && physics.rapier().colliders[*handle].parent() == Some(body) {
                    colliders.push(*handle);
                }
            }
            if colliders.is_empty() {
                continue;
            }
            let sit = place(crate::world::placement::node_pose(
                prototype,
                seat.sit_node as usize,
            ));
            let node = place(crate::world::placement::node_pose(
                prototype,
                seat.hit_nodes[0] as usize,
            ));
            let b = &seat.body_pose;
            let frames = SeatFrames {
                def_pose: b.def_pose as usize,
                driving: seat.driver,
                hands: [b.left_hand, b.right_hand].map(frame),
                legs: [b.left_leg, b.right_leg].map(frame),
                driving_hands: drive.map_or([None; 2], |d| [d.left_hand, d.right_hand].map(frame)),
                driving_legs: drive.map_or([None; 2], |d| [d.left_leg, d.right_leg].map(frame)),
            };
            let content = &self.contents.active[slot];
            let mut cursor = Some(seat.hit_nodes[0]);
            let mut seat_item = item;
            while let Some(n) = cursor {
                if let Some(found) = crate::world::content_mechanisms::node_item(
                    content,
                    prototype,
                    &self.reference,
                    &self.contents.rules,
                    n as usize,
                ) {
                    seat_item = found;
                    break;
                }
                cursor = prototype.nodes[n as usize].parent;
            }
            let target = Arc::new(ItemSeat {
                item,
                seat_item,
                owner: owner.clone(),
                body,
                pose: from_rapier(&(body_pose.inverse() * sit)),
                node_pose: from_rapier(&(body_pose.inverse() * node)),
                seat,
                frames,
            });
            for handle in &colliders {
                interaction.bind_item_seat(*handle, target.clone());
            }
            bound.push((target, colliders));
        }
        Ok(SeatBinding {
            parent: placement.id.clone(),
            item,
            seats: bound,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Fields, ReferenceError, SourceId, main_seat, sit_angle_allows};
    use crate::reference::{WorldInitField, WorldValue};
    use rapier3d::math::Vector;

    #[test]
    fn seat_body_pose_reads_frames_and_checks_def_pose() {
        // Level1 seatscript 79163 has defPose 1. Two frames are null here to check None.
        let id = |path_id| SourceId {
            file: "level1".into(),
            path_id,
        };
        let fields = |def_pose| {
            [
                ("defPose", WorldValue::Integer(def_pose)),
                ("LHpos", WorldValue::Link(id(26412))),
                ("RHpos", WorldValue::Empty),
                ("LLpos", WorldValue::Empty),
                ("RLpos", WorldValue::Link(id(27202))),
                ("sitPos", WorldValue::Empty),
                ("rotLimit", WorldValue::Number(90.)),
                ("limitLessRot", WorldValue::Integer(0)),
                ("BcantSitAngle", WorldValue::Integer(0)),
                ("lookInsteadOfLean", WorldValue::Integer(0)),
                ("rootOrigin", WorldValue::Integer(0)),
                ("defYrot", WorldValue::Number(0.)),
                ("minGetUpAnlge", WorldValue::Number(65.)),
                ("getOutHeight", WorldValue::Number(1.)),
                ("showSleepText", WorldValue::Integer(0)),
            ]
            .into_iter()
            .map(|(name, value)| WorldInitField {
                name: name.into(),
                value,
            })
            .collect::<Vec<_>>()
        };
        let node_of = |s: &SourceId| -> Result<u32, ReferenceError> {
            Ok(if s.path_id == 26412 { 4 } else { 7 })
        };
        let f = fields(1);
        let seat = main_seat(&id(79163), Fields::Compact(&f), 2, &node_of).unwrap();
        let p = seat.body_pose;
        assert_eq!(
            (
                p.def_pose,
                p.left_hand,
                p.right_hand,
                p.left_leg,
                p.right_leg
            ),
            (1, Some(4), None, None, Some(7))
        );
        let f = fields(3);
        assert!(main_seat(&id(79163), Fields::Compact(&f), 2, &node_of).is_err());
    }

    #[test]
    fn cant_sit_angle_limits_tilted_seats() {
        let tilted = Vector::new(1., 1., 0.);
        assert!(sit_angle_allows(tilted, None));
        assert!(sit_angle_allows(tilted, Some(50.)));
        assert!(!sit_angle_allows(tilted, Some(40.)));
        assert!(!sit_angle_allows(-Vector::Y, Some(90.)));
    }
}
