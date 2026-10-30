//! Source particle callbacks select one nearby cap per struck GameObject.
use super::fluid_contact::FluidCollision;
use super::*;
use crate::fluids::{FluidContainer, FluidSnapshot};
use std::ops::ControlFlow;

pub(super) struct ReceiptTank {
    item: ItemId,
    node: NodeId,
    caps: Vec<OpeningId>,
}

impl ReceiptTank {
    pub(super) fn retained_cap_bytes(&self) -> usize {
        super::memory::vector(&self.caps)
    }
}

pub(super) fn tank_nodes(reference: &ReferenceManifest) -> Vec<ReceiptTank> {
    let mut tanks: Vec<_> = reference
        .tanks
        .iter()
        .map(|tank| ReceiptTank {
            item: tank.item,
            node: reference
                .items
                .iter()
                .find(|i| i.id == tank.item)
                .unwrap()
                .node,
            caps: tank.openings.clone(),
        })
        .collect();
    tanks.sort_unstable_by_key(|tank| tank.node.0);
    tanks
}

fn group_callbacks(
    nodes: &mut Vec<NodeId>,
    events: &[FluidCollision],
    collider_nodes: &HashMap<ColliderId, NodeId>,
    physics: &PhysicsWorld,
) {
    nodes.clear();
    for event in events {
        if let Some(node) = collision_node(event, collider_nodes, physics)
            && !nodes.contains(&node)
        {
            nodes.push(node);
        }
    }
    nodes.sort_unstable_by_key(|node| node.0);
}

fn collision_node(
    event: &FluidCollision,
    nodes: &HashMap<ColliderId, NodeId>,
    physics: &PhysicsWorld,
) -> Option<NodeId> {
    let collider = physics.rapier().colliders.get(event.collider)?;
    nodes
        .get(&ColliderId((collider.user_data >> 64) as u32))
        .copied()
}

fn select_cap(
    source: OpeningId,
    position: Vector,
    caps: impl Iterator<Item = (OpeningId, Vector, f32)>,
) -> ControlFlow<Option<OpeningId>> {
    for (cap, center, radius) in caps {
        if cap == source {
            return ControlFlow::Break(None);
        }
        if (position - center).length() < radius * 0.1 {
            return ControlFlow::Break(Some(cap));
        }
    }
    ControlFlow::Continue(())
}

#[derive(Clone, Copy)]
enum ReceiptSource<'a> {
    Tank(&'a FluidContainer),
    Destination,
}

fn add_receipt(source: ReceiptSource<'_>, destination: &mut FluidContainer, liters: f32) {
    let entries = FluidSnapshot::from(match source {
        ReceiptSource::Tank(tank) => tank,
        ReceiptSource::Destination => &*destination,
    });
    for entry in entries.as_slice() {
        let current_source = match source {
            ReceiptSource::Tank(tank) => tank,
            ReceiptSource::Destination => &*destination,
        };
        let fraction =
            current_source.mixture().amount(entry.fluid_type as usize) / current_source.total();
        destination.change_one(liters * fraction, entry.fluid_type as usize);
    }
    destination.cleanup_one();
}

impl InteractionState {
    fn receipt_target(
        &self,
        source: OpeningId,
        game_object: NodeId,
        events: &[FluidCollision],
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> Option<OpeningId> {
        let root = self.mechanisms.root_node(game_object)?;
        for tank in &self.receipt_tanks {
            if self.mechanisms.root_node(tank.node) != Some(root)
                || items
                    .get(tank.item)
                    .is_none_or(|i| i.inactive_pose.is_some())
            {
                continue;
            }
            for event in events {
                if collision_node(event, &self.light_nodes, physics) != Some(game_object) {
                    continue;
                }
                let caps = tank.caps.iter().map(|cap| {
                    let opening = self
                        .openings
                        .iter()
                        .find(|o| o.reference.id == *cap)
                        .unwrap();
                    let pose = to_rapier(items.get(tank.item).unwrap().pose) * opening.local_pose;
                    (*cap, pose.translation, opening.reference.flow_radius)
                });
                if let ControlFlow::Break(target) = select_cap(source, event.position, caps) {
                    return target;
                }
            }
        }
        None
    }

    fn receive_fluid(
        &mut self,
        source: OpeningId,
        destination: OpeningId,
        raw_poured: f32,
        fluid_scale: f32,
        items: &mut ItemStore,
    ) -> bool {
        let source_index = self
            .openings
            .iter()
            .position(|o| o.reference.id == source)
            .unwrap();
        let destination_index = self
            .openings
            .iter()
            .position(|o| o.reference.id == destination)
            .unwrap();
        let source_tank = self.openings[source_index].fluid_owner();
        let destination_tank = self.openings[destination_index].reference.tank;
        let Some(destination_fluid) = items.get(destination_tank).and_then(|i| i.fluid.as_ref())
        else {
            return false;
        };
        let receiver = &self.openings[destination_index];
        let valve = receiver.valve;
        let accepts = destination_fluid.total() < destination_fluid.capacity_liters && valve > 0.;
        let response = receiver.particles.as_ref().map(|p| (p.bounce, p.dampen));
        let emitter = &mut self.openings[source_index];
        if let Some(contact) = emitter.contact.as_mut() {
            if accepts {
                if let Some((bounce, dampen)) = response {
                    contact.receive_response(bounce, dampen, valve);
                }
            } else if let Some(particles) = emitter.particles.as_ref() {
                contact.restore_response(particles);
            }
        }
        // Old particles still change the response after the current spill stops.
        if !accepts || !(raw_poured > 0.) {
            return false;
        }
        let Some(_) = items.get(source_tank).and_then(|i| i.fluid.as_ref()) else {
            return false;
        };
        let liters = raw_poured * fluid_scale;
        if source_tank == destination_tank {
            add_receipt(
                ReceiptSource::Destination,
                items
                    .get_mut(destination_tank)
                    .unwrap()
                    .fluid
                    .as_mut()
                    .unwrap(),
                liters,
            );
        } else {
            let (source, destination) = items.get_pair_mut(source_tank, destination_tank).unwrap();
            add_receipt(
                ReceiptSource::Tank(source.fluid.as_ref().unwrap()),
                destination.fluid.as_mut().unwrap(),
                liters,
            );
        }
        self.openings[destination_index].flow = Some((true, 1.));
        self.openings[destination_index].fill_called = true;
        true
    }

    pub(super) fn apply_fluid_receipts(
        &mut self,
        items: &mut ItemStore,
        physics: &PhysicsWorld,
        fluid_scale: f32,
    ) {
        for index in 0..self.openings.len() {
            let Some(contact) = self.openings[index].contact.as_ref() else {
                continue;
            };
            let raw_poured = contact.last_poured;
            group_callbacks(
                &mut self.callback_nodes,
                contact.collisions(),
                &self.light_nodes,
                physics,
            );
            let source = self.openings[index].reference.id;
            for callback in 0..self.callback_nodes.len() {
                let contact = self.openings[index].contact.as_ref().unwrap();
                if let Some(destination) = self.receipt_target(
                    source,
                    self.callback_nodes[callback],
                    contact.collisions(),
                    items,
                    physics,
                ) {
                    self.receive_fluid(source, destination, raw_poured, fluid_scale, items);
                }
            }
        }
    }
}
