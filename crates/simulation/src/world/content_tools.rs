//! Source drotkefescript brushes and sprayscript sprays on generated items (loot tools).
use super::WorldStream;
use crate::interaction::InteractionState;
use crate::interaction::cleaning::{BrushPoses, valid_tool_state};
use crate::interaction::items::ItemStore;
use crate::reference::{
    ItemId, PrototypeTool, ReferenceError, ReferenceManifest, SourceId, SprayTool,
    WorldPrototypeReference,
};
use crate::save::ToolStateSnapshot;
use crate::vehicle::to_rapier;
use crate::world::unity_random::UnityRandom;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::math::Pose;
use std::collections::HashMap;

pub struct ToolBinding {
    pub(super) parent: GeneratedId,
    pub(super) item: ItemId,
    /// The active content slot.
    slot: usize,
    /// The spawn state. A save writes only a state that differs from it.
    initial: ToolStateSnapshot,
    /// Prototype nodes: the brush model, the brush scrub part, the spray color renderer.
    model: Option<usize>,
    scrub: Option<usize>,
    renderer: Option<usize>,
    own: Option<crate::reference::ConditionId>,
    /// The presented look.
    pub look: ToolLook,
}
/// What the renderer shows of a loot tool.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolLook {
    /// The brush model node and its pose in stream space.
    pub model: Option<(usize, crate::vehicle::VehiclePose)>,
    /// The spray renderer node and its color.
    pub color: Option<(usize, [f32; 4])>,
    /// The scrub node of a worn brush.
    pub hidden: Option<usize>,
}

#[derive(Default)]
pub struct Tools {
    pub(super) bindings: Vec<ToolBinding>,
    /// Prototype tool index by prototype.
    prototypes: Option<HashMap<SourceId, usize>>,
    scan: Option<(u64, u64)>,
}
impl Tools {
    pub fn heap_bytes(&self) -> usize {
        self.bindings.capacity() * std::mem::size_of::<ToolBinding>()
            + self
                .bindings
                .iter()
                .map(|b| crate::world::contents::identity_heap(&b.parent))
                .sum::<usize>()
    }
}

/// Source `sprayscript.SelectRandomColor`. None keeps the authored color.
fn random_color(t: &SprayTool, random: &mut UnityRandom) -> Option<[f32; 4]> {
    let mut own = -1;
    let mut palette = None;
    if !t.color_types.is_empty() {
        own = i32::from(!t.colors.is_empty() && random.range_int(0, 10) <= 1);
        let name = &t.color_types[random.range_int(0, t.color_types.len() as i32) as usize];
        let colors = &t.color_palettes[name];
        palette = Some(colors[random.range_int(0, colors.len() as i32) as usize]);
    }
    let mut chosen = None;
    if !t.colors.is_empty() {
        if t.color_types.is_empty() {
            own = 1;
        }
        chosen = Some(t.colors[random.range_int(0, t.colors.len() as i32) as usize]);
    }
    let mut color = if own == 0 { palette } else { chosen }?;
    color[3] = 1.;
    Some(color)
}

/// The pose of prototype node `node` relative to the node of `root`.
fn local(
    prototype: &WorldPrototypeReference,
    placement: &crate::world::Placement,
    root: usize,
    node: &SourceId,
) -> Result<Pose, ReferenceError> {
    let place = |index| {
        crate::world::placement::place_pose(
            placement.pose,
            placement.scale,
            crate::world::placement::node_pose(prototype, index),
        )
    };
    let index = super::content_small_items::node_index(prototype, node)?;
    Ok(super::content_small_items::relative(
        place(root),
        place(index),
    ))
}

impl WorldStream {
    /// Binds the tools of new active loot items and removes the tools of gone ones.
    pub(super) fn sync_tools(
        &mut self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &mut InteractionState,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.tools.scan == Some(revision) {
            return Ok(());
        }
        if self.tools.prototypes.is_none() {
            let tools = &reference.conditions.prototype_tools;
            self.tools.prototypes = Some(
                tools
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (t.prototype.clone(), i))
                    .collect(),
            );
        }
        let active = &self.contents.active;
        self.tools.bindings.retain_mut(|b| {
            let slot = active
                .iter()
                .position(|c| c.content.placement.id == b.parent && c.item == Some(b.item));
            match slot {
                Some(slot) => b.slot = slot,
                None => interaction.cleaning.remove_tool(b.item),
            }
            slot.is_some()
        });
        let mut complete = true;
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let prototypes = self.tools.prototypes.as_ref().expect("built above");
            let (Some(item), Some(&tool)) = (
                content.item,
                prototypes.get(&content.content.placement.prototype),
            ) else {
                continue;
            };
            if self.tools.bindings.iter().any(|b| b.item == item) {
                continue;
            }
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let binding = self.bind_tool(reference, items, interaction, slot, item, tool)?;
            self.tools.bindings.push(binding);
        }
        self.tools.scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_tool(
        &self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &mut InteractionState,
        slot: usize,
        item: ItemId,
        tool: usize,
    ) -> Result<ToolBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let placement = &content.content.placement;
        let prototype = self
            .reference
            .prototypes
            .iter()
            .find(|p| p.source == placement.prototype)
            .ok_or_else(|| ReferenceError("Tool prototype missing".into()))?;
        let rules = &self.contents.rules;
        let pickup = rules
            .component(&self.reference, &placement.prototype, "pickupable")?
            .ok_or_else(|| ReferenceError("Loot tool has no pickup".into()))?;
        let root = super::content_small_items::node_index(
            prototype,
            rules.root(&self.reference, pickup.source)?,
        )?;
        let item_pose = to_rapier(items.get(item).expect("checked tool item").pose);
        let pose = |node: &SourceId| local(prototype, placement, root, node).map(|p| item_pose * p);
        let saved = self
            .changes
            .get(ObjectKey::Generated(placement.id.clone()))
            .and_then(|c| c.snapshot.as_ref())
            .and_then(|s| s.tool);
        let invalid = || ReferenceError("Invalid saved loot tool state".into());
        let cleaning = &mut interaction.cleaning;
        let index = |n: Option<&SourceId>| {
            n.map(|n| super::content_small_items::node_index(prototype, n))
                .transpose()
        };
        let mut binding = ToolBinding {
            parent: placement.id.clone(),
            item,
            slot,
            initial: ToolStateSnapshot {
                quantity: None,
                color: None,
                disabled: false,
            },
            model: None,
            scrub: None,
            renderer: None,
            own: None,
            look: ToolLook::default(),
        };
        binding.initial = match &reference.conditions.prototype_tools[tool].kind {
            PrototypeTool::Brush {
                own_condition,
                tool: t,
                nodes,
                ..
            } => {
                let own = content
                    .conditions
                    .iter()
                    .find(|&&(_, i, _)| {
                        &content.content.inputs.conditions[i].source == own_condition
                    })
                    .map(|&(entry, ..)| entry);
                let optional = |n: &Option<SourceId>| n.as_ref().map(pose).transpose();
                let poses = BrushPoses {
                    root: item_pose,
                    model: optional(&nodes.model)?,
                    equip: optional(&nodes.model_equip)?,
                    default: optional(&nodes.model_default)?,
                };
                if saved.is_some_and(|s| !valid_tool_state(None, s.quantity, s.color, s.disabled)) {
                    return Err(invalid());
                }
                let disabled = saved.map_or(t.disabled, |s| s.disabled);
                cleaning.add_prototype_brush(item, tool, own, poses, disabled, reference);
                binding.model = index(nodes.model.as_ref())?;
                binding.scrub = index(nodes.scrub_node.as_ref())?;
                binding.own = own;
                ToolStateSnapshot {
                    quantity: None,
                    color: None,
                    disabled: t.disabled,
                }
            }
            PrototypeTool::Spray {
                nozzle,
                renderer_node,
                tool: t,
                ..
            } => {
                binding.renderer = index(Some(renderer_node))?;
                let key = ObjectKey::Generated(placement.id.clone());
                let mut random = UnityRandom::new(key.seed() as i32);
                let startup = if t.authored_color_set {
                    None
                } else {
                    random_color(t, &mut random)
                }
                .unwrap_or(t.startup_color);
                let initial = ToolStateSnapshot {
                    quantity: Some(t.startup_quantity),
                    color: Some(startup),
                    disabled: false,
                };
                let state = saved.unwrap_or(initial);
                if !valid_tool_state(Some(t), state.quantity, state.color, state.disabled) {
                    return Err(invalid());
                }
                let (Some(quantity), Some(color)) = (state.quantity, state.color) else {
                    return Err(invalid());
                };
                cleaning.add_prototype_spray(
                    item,
                    tool,
                    item_pose,
                    pose(nozzle)?,
                    quantity,
                    color,
                    reference,
                );
                initial
            }
        };
        Ok(binding)
    }
    /// Updates the looks of loot tools and queues the changed ones for presentation.
    pub fn sync_tool_looks(
        &mut self,
        cleaning: &crate::interaction::cleaning::CleaningTools,
        conditions: &crate::conditions::ConditionStore,
        items: &ItemStore,
    ) {
        for b in &mut self.tools.bindings {
            let look = ToolLook {
                model: b.model.map(|n| (n, cleaning.model_pose(b.item, items))),
                color: b
                    .renderer
                    .and_then(|n| Some((n, cleaning.get(b.item)?.color?))),
                hidden: b
                    .scrub
                    .filter(|_| b.own.is_some_and(|id| conditions.get(id).state == 5)),
            };
            if look != b.look {
                b.look = look;
                let slot = super::content_index::ContentSlot::Active(b.slot);
                if !self.presentation_changes.contains(&slot) {
                    self.presentation_changes.push(slot);
                }
            }
        }
    }
    /// The saved state of a bound loot tool: None at its spawn state. None when `item` has no tool.
    pub(super) fn tool_value(
        &self,
        item: ItemId,
        interaction: &InteractionState,
    ) -> Option<Option<ToolStateSnapshot>> {
        let b = self.tools.bindings.iter().find(|b| b.item == item)?;
        let s = interaction.cleaning.get(item)?;
        let state = ToolStateSnapshot {
            quantity: s.quantity,
            color: s.color,
            disabled: s.disabled,
        };
        Some((state != b.initial).then_some(state))
    }
    /// The look of the loot tool of a generated item.
    pub fn tool_look(&self, parent: &GeneratedId) -> Option<&ToolLook> {
        self.tools
            .bindings
            .iter()
            .find(|b| &b.parent == parent)
            .map(|b| &b.look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::SprayTool;
    use std::collections::BTreeMap;

    fn spray(types: &[&str], colors: Vec<[f32; 4]>) -> SprayTool {
        let mut t: SprayTool = serde_json::from_value(serde_json::json!({
            "source": {"file": "f", "path_id": 1}, "ammo": {"file": "f", "path_id": 2},
            "color_source": {"file": "f", "path_id": 3}, "float_quantity": true,
            "hide_look_at_count": false, "hide_carried_count": false, "no_per_show": false,
            "startup_quantity": 1.0, "maximum_quantity": 1.0, "startup_color": [0.0, 0.0, 0.0, 1.0],
            "authored_color_set": false, "startup_palette": null, "startup_color_index": null,
            "color_types": [], "color_palettes": {}, "colors": [],
            "renderer": {"file": "f", "path_id": 4}, "distance": 2.0, "mask": 1, "color_lerp": 1.0,
            "paint_loss": 1.0, "valve_speed": 1.0, "startup_valve": 0.0, "rate": 1.0,
            "max_volume": 1.0, "text": 0, "start_speed": 1.0, "voice_source": null,
            "voice": null, "particles": []
        }))
        .unwrap();
        t.color_types = types.iter().map(|s| s.to_string()).collect();
        t.color_palettes = types
            .iter()
            .map(|s| (s.to_string(), vec![[0.5, 0.5, 0.5, 0.2]]))
            .collect::<BTreeMap<_, _>>();
        t.colors = colors;
        t
    }

    #[test]
    fn random_color_picks_a_palette_or_own_color_with_full_alpha() {
        let mut random = UnityRandom::new(7);
        assert_eq!(random_color(&spray(&[], vec![]), &mut random), None);
        let own = random_color(&spray(&[], vec![[1., 0., 0., 0.]]), &mut random);
        assert_eq!(own, Some([1., 0., 0., 1.]));
        for _ in 0..20 {
            let c = random_color(&spray(&["a"], vec![[1., 0., 0., 0.]]), &mut random).unwrap();
            assert!(c == [0.5, 0.5, 0.5, 1.] || c == [1., 0., 0., 1.]);
        }
    }
}
