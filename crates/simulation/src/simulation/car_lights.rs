//! Source `headlightscript` (headlight and cabin) and `rearlights` controllers of one world car
//! or trailer.
//! Outputs name prototype nodes. The presentation maps them to the car instance.
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
    /// `electronicsscript` output. None: the controller has no electronics.
    electronics: Option<f32>,
    /// The part item that holds the controller. None: the car root.
    owner: Option<ItemId>,
    cabin_mode: usize,
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
    pub electronics: Option<f32>,
    pub owner: Option<ItemId>,
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
                cabin_mode: light.cabin.as_ref().map_or(0, |c| c.startup_mode),
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

    /// One light step. `mounted` tells whether a part item is on the car; `door` reads
    /// a cabin door usable value.
    pub fn advance(
        &mut self,
        records: &[WorldLightReference],
        input: CarLightInput,
        mounted: impl Fn(ItemId) -> bool,
        door: impl Fn(ItemId) -> f32,
        dt: f32,
    ) {
        for index in 0..self.controllers.len() {
            let reference = &records[index];
            let c = &mut self.controllers[index];
            match c.kind {
                LightControllerKind::Rear => {
                    let powered = c.electronics.is_none_or(|e| e > ELECTRONICS_CUTOFF);
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
                        let on = match cabin.modes.get(c.cabin_mode).copied().unwrap_or(0) {
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
                    self.refresh_fade(records, index);
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

    fn refresh_fade(&mut self, records: &[WorldLightReference], index: usize) {
        let c = &self.controllers[index];
        let energy = c.state.energy() * c.electronics.unwrap_or(1.);
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
