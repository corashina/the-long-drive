//! Source `headlightscript` (headlight and cabin) and `rearlights` controllers of one world car
//! or trailer.
//! Outputs name prototype nodes. The presentation maps them to the car instance.
use crate::electronics::NodeElectronics;
use crate::lighting::HeadlightState;
use crate::reference::{
    ItemId, LampSignal, LightControllerKind, MaterialLink, WorldLampStateReference,
    WorldLightReference,
};

/// Source electronics cutoff of the rear lights and the high beam.
const ELECTRONICS_CUTOFF: f32 = 0.2;

/// One source Light of a car lamp state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarLightOutput {
    /// Prototype node of the Light component.
    pub node: u32,
    pub enabled: bool,
    pub intensity: f32,
}

/// The material that a lamp renderer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialChoice {
    pub light: u16,
    pub state: u16,
    pub on: bool,
}

/// One lamp renderer of the car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarMaterialOutput {
    /// Prototype node of the renderer.
    pub node: u32,
    /// None: the renderer keeps its authored material.
    pub choice: Option<MaterialChoice>,
}

impl MaterialChoice {
    /// The material link of this choice in `records`.
    pub fn link(self, records: &[WorldLightReference]) -> Option<&MaterialLink> {
        let lamp = &records[self.light as usize].states[self.state as usize];
        if self.on {
            lamp.on_material.as_ref().or(lamp.off_material.as_ref())
        } else {
            lamp.off_material.as_ref()
        }
    }
}

/// Inputs of one light step.
#[derive(Clone, Copy, Debug, Default)]
pub struct CarLightInput {
    /// Source `carscript` requested headlight state: off, low or high.
    pub requested: u8,
    pub ignition: bool,
    pub brake: f32,
    pub gear_ratio: f32,
}

struct Controller {
    state: HeadlightState,
    kind: LightControllerKind,
    /// Index of its `electronicsscript` in the car electronics. None: no electronics.
    electronics: Option<usize>,
    /// The part item that holds the controller. None: the car root.
    owner: Option<ItemId>,
    /// The cabin switch usable item.
    switch: Option<ItemId>,
    /// Usable items of the cabin door slots.
    doors: Vec<Option<ItemId>>,
    /// Output indices per state: lights, then renderers.
    lights: Vec<Vec<usize>>,
    renderers: Vec<Vec<usize>>,
}

/// Light state of one world car.
pub struct CarLights {
    controllers: Vec<Controller>,
    lights: Vec<CarLightOutput>,
    materials: Vec<CarMaterialOutput>,
    /// Static `Working` and `nWorking` node visibility.
    visibility: Vec<(u32, bool)>,
}

/// Per-controller links that the binder resolves on the car copy.
pub struct CarLightLinks {
    pub electronics: Option<usize>,
    pub owner: Option<ItemId>,
    /// Item of the `inputconnect.turnOverride` usable. None: the controller has no cabin switch.
    pub switch: Option<ItemId>,
    pub doors: Vec<Option<ItemId>>,
}

impl CarLights {
    /// `links` follow `records`.
    pub fn new(records: &[WorldLightReference], links: Vec<CarLightLinks>) -> Self {
        let mut lights: Vec<CarLightOutput> = Vec::new();
        let mut materials: Vec<CarMaterialOutput> = Vec::new();
        let mut visibility = Vec::new();
        let mut controllers = Vec::with_capacity(records.len());
        for (index, (light, links)) in records.iter().zip(links).enumerate() {
            let mut light_slots = Vec::with_capacity(light.states.len());
            let mut renderer_slots = Vec::with_capacity(light.states.len());
            for (state_index, lamp) in light.states.iter().enumerate() {
                light_slots.push(
                    lamp.lights
                        .iter()
                        .map(|&node| {
                            lights
                                .iter()
                                .position(|o| o.node == node)
                                .unwrap_or_else(|| {
                                    lights.push(CarLightOutput {
                                        node,
                                        enabled: false,
                                        intensity: 0.,
                                    });
                                    lights.len() - 1
                                })
                        })
                        .collect(),
                );
                let startup = MaterialChoice {
                    light: index as u16,
                    state: state_index as u16,
                    on: light.startup_on,
                };
                let initial = startup.link(records).map(|_| startup);
                renderer_slots.push(
                    lamp.renderers
                        .iter()
                        .map(|&node| {
                            let i = materials
                                .iter()
                                .position(|o| o.node == node)
                                .unwrap_or_else(|| {
                                    materials.push(CarMaterialOutput { node, choice: None });
                                    materials.len() - 1
                                });
                            if initial.is_some() {
                                materials[i].choice = initial;
                            }
                            i
                        })
                        .collect(),
                );
                visibility.extend(lamp.working_nodes.iter().map(|&n| (n, !lamp.broken)));
                visibility.extend(lamp.broken_nodes.iter().map(|&n| (n, lamp.broken)));
            }
            controllers.push(Controller {
                state: HeadlightState::world(light),
                kind: light.kind,
                electronics: links.electronics,
                owner: links.owner,
                switch: links.switch,
                doors: links.doors,
                lights: light_slots,
                renderers: renderer_slots,
            });
        }
        Self {
            controllers,
            lights,
            materials,
            visibility,
        }
    }

    /// One light step. `mounted` tells whether a part item is on the car. `door` reads
    /// a cabin door usable value. `switch` reads a cabin switch turn state.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        records: &[WorldLightReference],
        input: CarLightInput,
        electronics: &[NodeElectronics],
        mounted: impl Fn(ItemId) -> bool,
        door: impl Fn(ItemId) -> f32,
        switch: impl Fn(ItemId) -> Option<u8>,
        dt: f32,
    ) {
        for index in 0..self.controllers.len() {
            let reference = &records[index];
            let c = &mut self.controllers[index];
            let output = c.electronics.map(|i| electronics[i].state.output());
            match c.kind {
                LightControllerKind::Rear => {
                    let powered = output.is_none_or(|e| e > ELECTRONICS_CUTOFF);
                    self.refresh_rear(records, index, input, input.ignition && powered);
                }
                LightControllerKind::Headlight | LightControllerKind::Cabin => {
                    if !dt.is_finite() || dt <= 0. {
                        continue;
                    }
                    if c.kind == LightControllerKind::Headlight {
                        if c.owner.is_some_and(|owner| !mounted(owner)) {
                            c.state.unmount();
                            self.refresh_dark(records, index);
                            continue;
                        }
                        c.state.request(input.requested);
                    } else if let Some(cabin) = &reference.cabin {
                        // Source `inputconnect` reads `turnOverride.currentTurnState` each frame.
                        let mode = c
                            .switch
                            .and_then(&switch)
                            .map_or(cabin.startup_mode, usize::from);
                        let on = match cabin.modes.get(mode).copied().unwrap_or(0) {
                            1 => true,
                            2 => false,
                            _ => c
                                .doors
                                .iter()
                                .flatten()
                                .any(|&item| mounted(item) && door(item) > cabin.threshold),
                        };
                        c.state.request(u8::from(on));
                    }
                    c.state.update(dt);
                    self.refresh_fade(records, index, output.unwrap_or(1.));
                }
                _ => {}
            }
        }
    }

    /// A hash of the lamp materials and node visibility. It changes when the presentation must sync.
    pub fn signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        for m in &self.materials {
            m.choice.map(|c| (c.light, c.state, c.on)).hash(&mut h);
        }
        self.visibility.hash(&mut h);
        h.finish()
    }
    pub fn lights(&self) -> &[CarLightOutput] {
        &self.lights
    }
    pub fn materials(&self) -> &[CarMaterialOutput] {
        &self.materials
    }
    pub fn visibility(&self) -> &[(u32, bool)] {
        &self.visibility
    }

    fn refresh_rear(
        &mut self,
        records: &[WorldLightReference],
        index: usize,
        input: CarLightInput,
        ignition: bool,
    ) {
        let states = &records[index].states;
        let count = states.len();
        for step in 0..count {
            // Source order: forward with ignition, backward without.
            let state = if ignition { step } else { count - 1 - step };
            let lamp = &states[state];
            let on = ignition
                && match lamp.signal {
                    LampSignal::Tail => input.requested != 0,
                    LampSignal::Brake => input.brake > 0.01,
                    LampSignal::Reverse => input.gear_ratio < 0.,
                    _ => false,
                };
            for &slot in &self.controllers[index].lights[state] {
                self.lights[slot].enabled = on;
                self.lights[slot].intensity = lamp.intensity;
            }
            if on || !ignition || !lamp.dont_overwrite {
                self.select(index, state, lamp, on);
            }
        }
    }

    fn refresh_dark(&mut self, records: &[WorldLightReference], index: usize) {
        for (state, lamp) in records[index].states.iter().enumerate() {
            for &slot in &self.controllers[index].lights[state] {
                self.lights[slot].enabled = false;
                self.lights[slot].intensity = 0.;
            }
            self.select(index, state, lamp, false);
        }
    }

    /// Source `RefreshLights`. `output` is the `electronicsscript` output, 1 without electronics.
    fn refresh_fade(&mut self, records: &[WorldLightReference], index: usize, output: f32) {
        let c = &self.controllers[index];
        let energy = c.state.energy() * output;
        let current = c.state.refreshed_state();
        let states = &records[index].states;
        // Clear other states first. Shared source renderers then select the current state.
        for (state, lamp) in states.iter().enumerate() {
            if state == current {
                continue;
            }
            for &slot in &self.controllers[index].lights[state] {
                self.lights[slot].enabled = false;
            }
            self.select(index, state, lamp, false);
        }
        let Some(lamp) = states.get(current) else {
            return;
        };
        for &slot in &self.controllers[index].lights[current] {
            self.lights[slot].enabled = true;
            self.lights[slot].intensity = lamp.intensity * energy;
        }
        self.select(index, current, lamp, energy > 0.);
    }

    fn select(&mut self, index: usize, state: usize, lamp: &WorldLampStateReference, on: bool) {
        let has = if on {
            lamp.on_material.is_some() || lamp.off_material.is_some()
        } else {
            lamp.off_material.is_some()
        };
        if !has {
            return;
        }
        let choice = MaterialChoice {
            light: index as u16,
            state: state as u16,
            on,
        };
        for &slot in &self.controllers[index].renderers[state] {
            self.materials[slot].choice = Some(choice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{SourceId, WorldCabinInputReference};

    fn lamp(signal: LampSignal, light: u32) -> WorldLampStateReference {
        WorldLampStateReference {
            signal,
            lights: vec![light],
            intensity: 1.,
            renderers: Vec::new(),
            on_material: None,
            off_material: None,
            broken: false,
            working_nodes: Vec::new(),
            broken_nodes: Vec::new(),
            dont_overwrite: false,
        }
    }

    /// The cabin lamp follows the switch: door mode, always on, always off.
    #[test]
    fn cabin_lamp_follows_switch_state() {
        let source = SourceId {
            file: "car".into(),
            path_id: 1,
        };
        let records = [WorldLightReference {
            source: source.clone(),
            kind: LightControllerKind::Cabin,
            node: 0,
            electronics: None,
            startup_on: false,
            current_state: 0,
            requested_state: 0,
            energy: 1.,
            energy_move: 100.,
            enabled: true,
            reverse_switch: false,
            states: vec![lamp(LampSignal::Off, 1), lamp(LampSignal::Cabin, 2)],
            cabin: Some(WorldCabinInputReference {
                source,
                switch_node: 3,
                modes: vec![0, 1, 2],
                startup_mode: 0,
                door_slots: vec![4],
                threshold: 0.5,
                include_children: false,
                missing_slot_is_same: false,
            }),
        }];
        let (switch, door) = (ItemId(1), ItemId(2));
        let mut lights = CarLights::new(
            &records,
            vec![CarLightLinks {
                electronics: None,
                owner: None,
                switch: Some(switch),
                doors: vec![Some(door)],
            }],
        );
        let mut lit = |state: u8, open: f32| {
            for _ in 0..20 {
                lights.advance(
                    &records,
                    CarLightInput::default(),
                    &[],
                    |_| true,
                    |_| open,
                    |item| (item == switch).then_some(state),
                    0.05,
                );
            }
            lights
                .lights()
                .iter()
                .find(|l| l.node == 2)
                .unwrap()
                .enabled
        };
        assert!(!lit(0, 0.));
        assert!(lit(0, 1.));
        assert!(lit(1, 0.));
        assert!(!lit(2, 1.));
    }
}
