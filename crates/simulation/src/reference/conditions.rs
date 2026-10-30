use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionRecord {
    pub surfaces: Vec<ConditionSurface>,
    pub brushes: Vec<BrushReference>,
    /// Source `fpscontroller.pointKefe`: the player hand brush of pointing.
    /// `fpscontroller.Start` already set its `mask` (`pointingLayer`) and `distance` (`pointRange`).
    pub player_brush: PlayerBrushReference,
    pub sprays: Vec<SprayReference>,
    /// Brush and spray tools of world prototypes (loot items).
    pub prototype_tools: Vec<PrototypeToolReference>,
    pub washes: Vec<WashReference>,
    pub collider_bindings: Vec<ConditionColliderBinding>,
    pub repair_tables: Vec<[[bool; 7]; 5]>,
    pub clean_tables: Vec<[bool; 7]>,
    pub repair_rate: f32,
    pub repair_decay_rate: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionStartup {
    pub state: u8,
    pub state2: u32,
    pub state3: u32,
    pub repair_progress: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionMaterialState {
    pub materials: Vec<MaterialBindingReference>,
    pub color: Option<[f32; 4]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionMeshReference {
    pub asset: AssetId,
    pub gltf_mesh: u32,
    pub source: SourceId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionRenderer {
    pub source: SourceId,
    pub node: NodeId,
    pub states: Vec<ConditionMaterialState>,
    pub meshes: Vec<ConditionMeshReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionSurface {
    pub id: ConditionId,
    pub source: SourceId,
    pub node: NodeId,
    pub item: Option<ItemId>,
    pub startup: ConditionStartup,
    pub permission_index: u32,
    pub selected_type: String,
    pub material_type: String,
    pub colors: Vec<[f32; 4]>,
    pub use_material_type: bool,
    pub disable_color: bool,
    pub force_color: bool,
    pub use_mat_list: bool,
    pub dont_use_worn: bool,
    pub shared_mat_list: bool,
    pub use_mesh_list: bool,
    pub use_object_list: bool,
    pub objects: Vec<NodeId>,
    pub children: Vec<ConditionId>,
    pub is_child: bool,
    pub no_inherit_state: bool,
    pub same_state_as_parent: bool,
    pub child_new_color: bool,
    pub random_start_state3: bool,
    pub loaded: bool,
    pub renderers: Vec<ConditionRenderer>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// A `drotkefescript` brush item.
pub struct BrushReference {
    pub item: ItemId,
    pub body: BodyId,
    pub pickup: SourceId,
    pub save: SourceId,
    /// Source `drotkefescript.ownCondition`.
    pub own_condition: ConditionId,
    pub tool: BrushTool,
    pub nodes: BrushNodes<NodeId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerBrushReference {
    pub tool: BrushTool,
    pub nodes: BrushNodes<NodeId>,
}

/// The nodes of a brush: reference nodes, or prototype node sources.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushNodes<N> {
    pub scrub_node: Option<N>,
    pub model: Option<N>,
    /// None: the slide has no equip or default position (the player point slide).
    pub model_equip: Option<N>,
    pub model_default: Option<N>,
}

/// The tool of a world prototype.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypeToolReference {
    pub prototype: SourceId,
    pub kind: PrototypeTool,
}

/// The loop of a prototype tool: a `car_voices` template at a root-relative position.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSound {
    pub voice: u32,
    pub position: [f32; 3],
}

/// Node fields are prototype node sources. `tool.voice` is None: `sound` plays the loop.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrototypeTool {
    Brush {
        /// Source `drotkefescript.ownCondition`: a partconditionscript record.
        own_condition: SourceId,
        tool: BrushTool,
        nodes: BrushNodes<SourceId>,
        sound: Option<ToolSound>,
    },
    Spray {
        nozzle: SourceId,
        renderer_node: SourceId,
        tool: SprayTool,
        sound: ToolSound,
    },
}

/// Source `drotkefescript` and its `objSlideOnColliderScript` slide.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushTool {
    pub source: SourceId,
    pub slide: SourceId,
    pub cleaning_type: u8,
    pub can_ruin: bool,
    pub repair_modifier: f32,
    pub min_force: f32,
    pub wash_radius: f32,
    pub wash_mask: u32,
    pub text: u32,
    pub show_text: bool,
    pub distance: f32,
    pub mask: u32,
    pub position_lerp: f32,
    pub rotation_lerp: f32,
    pub contact_offset: f32,
    pub minimum_displacement: f32,
    pub maximum_displacement: f32,
    pub disabled: bool,
    pub both_work: bool,
    pub sound_when_disabled: bool,
    pub has_sound: bool,
    pub max_volume: f32,
    pub volume_lerp: f32,
    pub voice_source: Option<SourceId>,
    pub voice: Option<u32>,
    pub particles: Vec<ParticleReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SprayReference {
    pub item: ItemId,
    pub body: BodyId,
    pub pickup: SourceId,
    pub save: SourceId,
    pub nozzle: NodeId,
    pub renderer_node: NodeId,
    pub tool: SprayTool,
}

/// Source `sprayscript`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SprayTool {
    pub source: SourceId,
    pub ammo: SourceId,
    pub color_source: SourceId,
    pub float_quantity: bool,
    pub hide_look_at_count: bool,
    pub hide_carried_count: bool,
    pub no_per_show: bool,
    pub startup_quantity: f32,
    pub maximum_quantity: f32,
    pub startup_color: [f32; 4],
    pub authored_color_set: bool,
    pub startup_palette: Option<String>,
    pub startup_color_index: Option<u32>,
    pub color_types: Vec<String>,
    pub color_palettes: BTreeMap<String, Vec<[f32; 4]>>,
    pub colors: Vec<[f32; 4]>,
    pub renderer: SourceId,
    pub distance: f32,
    pub mask: u32,
    pub color_lerp: f32,
    pub paint_loss: f32,
    pub valve_speed: f32,
    pub startup_valve: f32,
    pub rate: f32,
    pub max_volume: f32,
    pub text: u32,
    pub start_speed: f32,
    pub voice_source: Option<SourceId>,
    pub voice: Option<u32>,
    pub particles: Vec<ParticleReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WashReference {
    pub id: WashId,
    pub source: SourceId,
    pub node: NodeId,
    pub colliders: Vec<ColliderId>,
    pub objects: Vec<NodeId>,
    pub permission_index: u32,
    pub repair_progress: f32,
    pub active: bool,
    pub on: bool,
    pub never_repair: bool,
    pub destroy_on_repair: bool,
    pub wash_on_shoot: bool,
    pub chance_to_start_disabled: f32,
    pub particles: Vec<ParticleReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionColliderBinding {
    pub collider: ColliderId,
    pub source: SourceId,
    pub ancestor_condition: Option<ConditionId>,
    pub pickup_condition: Option<ConditionId>,
    pub forwarded_condition: Option<ConditionId>,
    pub installed_slot_source: Option<SourceId>,
    pub installed_part_slot: Option<PartSlotId>,
    pub installed_tire_slot: Option<ItemId>,
    pub wash: Option<WashId>,
}

fn check(source: &SourceId, valid: bool, field: &str) -> Result<(), ReferenceError> {
    if valid && !source.file.is_empty() && source.path_id != 0 {
        Ok(())
    } else {
        Err(ReferenceError(format!(
            "Invalid condition {}:{} field {field}",
            source.file, source.path_id
        )))
    }
}

impl ConditionRecord {
    pub fn validate(&self, r: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.repair_tables.len() == 8 && self.clean_tables.len() == 4,
            "condition permission tables",
        )?;
        require(
            self.repair_rate.is_finite()
                && self.repair_rate > 0.
                && self.repair_decay_rate.is_finite()
                && self.repair_decay_rate >= 0.,
            "condition repair_rate",
        )?;
        let nodes: HashSet<_> = r.nodes.iter().map(|n| n.id).collect();
        let surfaces: HashMap<_, _> = self.surfaces.iter().map(|s| (s.id, s)).collect();
        let washes: HashSet<_> = self.washes.iter().map(|w| w.id).collect();
        let mut sources = HashSet::new();
        let mut surface_ids = HashSet::new();
        let mut wash_ids = HashSet::new();
        for s in &self.surfaces {
            check(
                &s.source,
                s.id.0 > 0 && surface_ids.insert(s.id) && sources.insert(&s.source),
                "id/source",
            )?;
            check(&s.source, nodes.contains(&s.node), "node")?;
            check(
                &s.source,
                s.item
                    .is_none_or(|id| r.items.iter().any(|i| i.id == id && i.node == s.node)),
                "item",
            )?;
            check(
                &s.source,
                s.permission_index < self.repair_tables.len() as u32,
                "permission_index",
            )?;
            let tool_disabled = self
                .brushes
                .iter()
                .any(|b| b.own_condition == s.id && b.tool.disabled);
            check(
                &s.source,
                s.startup.state <= 4 || (s.startup.state == 5 && tool_disabled),
                "startup.state",
            )?;
            check(
                &s.source,
                finite(&s.startup.color)
                    && s.colors.iter().all(|c| finite(c))
                    && s.startup.repair_progress.is_finite(),
                "startup.color/colors/repair_progress",
            )?;
            check(
                &s.source,
                s.children.iter().all(|id| surfaces.contains_key(id))
                    && s.children.iter().collect::<HashSet<_>>().len() == s.children.len(),
                "children",
            )?;
            check(
                &s.source,
                s.objects.iter().all(|n| nodes.contains(n)),
                "objects",
            )?;
            check(
                &s.source,
                !s.use_object_list
                    || s.objects.is_empty()
                    || (s.startup.state3 as usize) < s.objects.len(),
                "startup.state3",
            )?;
            for renderer in &s.renderers {
                check(&renderer.source, true, "renderers.source")?;
                check(
                    &s.source,
                    nodes.contains(&renderer.node) && renderer.states.len() == 5,
                    "renderers.states",
                )?;
                check(
                    &s.source,
                    !renderer.meshes.is_empty()
                        && (!s.use_mesh_list
                            || (s.startup.state2 as usize) < renderer.meshes.len()),
                    "startup.state2",
                )?;
                for state in &renderer.states {
                    check(
                        &s.source,
                        !state.materials.is_empty()
                            && state.color.as_ref().is_none_or(|c| finite(c)),
                        "renderers.states.materials/color",
                    )?;
                    for b in &state.materials {
                        check(
                            &s.source,
                            r.rendering.materials.iter().any(|m| {
                                m.asset == b.asset
                                    && m.gltf_material == b.gltf_material
                                    && m.source == b.source
                            }),
                            "renderers.states.materials",
                        )?;
                    }
                }
                for b in &renderer.meshes {
                    check(
                        &s.source,
                        r.assets
                            .iter()
                            .any(|a| a.id == b.asset && b.gltf_mesh < a.mesh_count)
                            && !b.source.file.is_empty()
                            && b.source.path_id != 0,
                        "renderers.meshes",
                    )?;
                }
            }
        }
        fn visit(
            id: ConditionId,
            surfaces: &HashMap<ConditionId, &ConditionSurface>,
            visiting: &mut HashSet<ConditionId>,
            done: &mut HashSet<ConditionId>,
        ) -> Result<(), ReferenceError> {
            if done.contains(&id) {
                return Ok(());
            }
            let s = surfaces[&id];
            check(&s.source, visiting.insert(id), "children cycle")?;
            for child in &s.children {
                visit(*child, surfaces, visiting, done)?;
            }
            visiting.remove(&id);
            done.insert(id);
            Ok(())
        }
        let mut visiting = HashSet::new();
        let mut done = HashSet::new();
        for s in &self.surfaces {
            visit(s.id, &surfaces, &mut visiting, &mut done)?;
        }
        let descendant = |mut node: NodeId, ancestor: NodeId| {
            loop {
                if node == ancestor {
                    return true;
                }
                let Some(parent) = r.nodes.iter().find(|n| n.id == node).and_then(|n| n.parent)
                else {
                    return false;
                };
                node = parent;
            }
        };
        let mut tool_items = HashSet::new();
        let owner = |source: &SourceId, item: ItemId, body: BodyId, pickup: &SourceId| {
            check(
                source,
                r.items.iter().any(|i| {
                    i.id == item
                        && i.body == body
                        && i.pickup.as_ref().is_some_and(|p| &p.source == pickup)
                }),
                "item/body/pickup",
            )
        };
        let voice = |source: &SourceId, id: Option<u32>, identity: &Option<SourceId>| {
            check(
                source,
                match (id, identity) {
                    (None, None) => true,
                    (Some(id), Some(identity)) => r.audio["voices"].as_array().is_some_and(|v| {
                        v.iter().any(|v| {
                            v["id"].as_u64() == Some(id as u64)
                                && serde_json::from_value::<SourceId>(v["source"].clone())
                                    .is_ok_and(|s| s == *identity)
                        })
                    }),
                    _ => false,
                },
                "voice",
            )
        };
        // A fixed tool plays an audio voice. A prototype tool plays a `sound` instead.
        let tool = |t: &BrushTool, fixed: bool| -> Result<(), ReferenceError> {
            check(&t.source, t.particles.len() <= 2, "particles channels")?;
            check(
                &t.source,
                !t.slide.file.is_empty() && t.slide.path_id != 0,
                "slide",
            )?;
            check(&t.source, t.cleaning_type < 7, "cleaning_type")?;
            check(
                &t.source,
                finite(&[
                    t.repair_modifier,
                    t.min_force,
                    t.wash_radius,
                    t.distance,
                    t.position_lerp,
                    t.rotation_lerp,
                    t.contact_offset,
                    t.minimum_displacement,
                    t.maximum_displacement,
                    t.max_volume,
                    t.volume_lerp,
                ]) && t.distance > 0.
                    && t.minimum_displacement >= 0.
                    && t.maximum_displacement > t.minimum_displacement
                    && t.repair_modifier >= 0.
                    && (0. ..=1.).contains(&t.min_force)
                    && t.wash_radius >= 0.,
                "parameters",
            )?;
            if fixed {
                voice(&t.source, t.voice, &t.voice_source)?;
                check(
                    &t.source,
                    !t.has_sound || t.voice.is_some(),
                    "voice/has_sound",
                )?;
            } else {
                check(&t.source, t.voice.is_none(), "prototype voice")?;
            }
            for p in &t.particles {
                validate_particle(&t.source, p, r)?;
            }
            Ok(())
        };
        let spray = |t: &SprayTool, fixed: bool| -> Result<(), ReferenceError> {
            check(
                &t.source,
                !t.ammo.file.is_empty() && t.ammo.path_id != 0,
                "ammo",
            )?;
            check(
                &t.source,
                !t.color_source.file.is_empty() && t.color_source.path_id != 0,
                "color_source",
            )?;
            check(
                &t.source,
                !t.renderer.file.is_empty() && t.renderer.path_id != 0,
                "renderer",
            )?;
            check(
                &t.source,
                finite(&[
                    t.startup_quantity,
                    t.maximum_quantity,
                    t.distance,
                    t.color_lerp,
                    t.paint_loss,
                    t.valve_speed,
                    t.startup_valve,
                    t.rate,
                    t.max_volume,
                    t.start_speed,
                ]) && t.maximum_quantity > 0.
                    && (0. ..=t.maximum_quantity).contains(&t.startup_quantity)
                    && t.distance > 1.
                    && t.color_lerp >= 0.
                    && t.paint_loss > 0.
                    && t.valve_speed > 0.
                    && (0. ..=1.).contains(&t.startup_valve)
                    && t.rate >= 0.,
                "parameters/quantity",
            )?;
            check(
                &t.source,
                finite(&t.startup_color)
                    && t.colors.iter().all(|c| finite(c))
                    && t.color_palettes.values().flatten().all(|c| finite(c))
                    && t.color_types.iter().all(|name| {
                        t.color_palettes
                            .get(name)
                            .is_some_and(|colors| !colors.is_empty())
                    })
                    && if t.authored_color_set || !fixed {
                        // The runtime selects the startup color of a prototype spray.
                        t.startup_palette.is_none() && t.startup_color_index.is_none()
                    } else {
                        let palette = t
                            .startup_palette
                            .as_ref()
                            .and_then(|name| t.color_palettes.get(name))
                            .or_else(|| t.startup_palette.is_none().then_some(&t.colors));
                        palette
                            .zip(t.startup_color_index)
                            .is_some_and(|(colors, index)| {
                                colors.get(index as usize).is_some_and(|color| {
                                    color[..3] == t.startup_color[..3] && t.startup_color[3] == 1.
                                })
                            })
                    },
                "color/palette/index",
            )?;
            if fixed {
                voice(&t.source, t.voice, &t.voice_source)?;
                check(&t.source, t.voice.is_some(), "voice")?;
            } else {
                check(
                    &t.source,
                    t.voice.is_none() && t.voice_source.is_some(),
                    "prototype voice",
                )?;
            }
            check(&t.source, t.particles.len() == 1, "spray particle owner")?;
            for p in &t.particles {
                validate_particle(&t.source, p, r)?;
            }
            Ok(())
        };
        let reference_nodes = |source: &SourceId, n: &BrushNodes<NodeId>| {
            check(
                source,
                [n.model_equip, n.model_default, n.model, n.scrub_node]
                    .iter()
                    .flatten()
                    .all(|n| nodes.contains(n)),
                "model",
            )
        };
        let mut prototypes = HashSet::new();
        for b in &self.brushes {
            let t = &b.tool;
            check(
                &t.source,
                sources.insert(&t.source) && tool_items.insert(b.item),
                "id/source",
            )?;
            owner(&t.source, b.item, b.body, &b.pickup)?;
            check(
                &t.source,
                !b.save.file.is_empty() && b.save.path_id != 0,
                "save",
            )?;
            check(
                &t.source,
                surfaces.get(&b.own_condition).is_some_and(|s| {
                    r.items
                        .iter()
                        .find(|i| i.id == b.item)
                        .is_some_and(|i| descendant(s.node, i.node))
                }),
                "own_condition",
            )?;
            check(
                &t.source,
                [1, 2].contains(&t.cleaning_type),
                "cleaning_type",
            )?;
            tool(t, true)?;
            reference_nodes(&t.source, &b.nodes)?;
        }
        let t = &self.player_brush.tool;
        check(
            &t.source,
            sources.insert(&t.source),
            "player brush id/source",
        )?;
        tool(t, true)?;
        reference_nodes(&t.source, &self.player_brush.nodes)?;
        for s in &self.sprays {
            let t = &s.tool;
            check(
                &t.source,
                sources.insert(&t.source) && tool_items.insert(s.item),
                "id/source",
            )?;
            owner(&t.source, s.item, s.body, &s.pickup)?;
            check(
                &t.source,
                !s.save.file.is_empty() && s.save.path_id != 0,
                "save",
            )?;
            check(
                &t.source,
                nodes.contains(&s.nozzle) && nodes.contains(&s.renderer_node),
                "nozzle/renderer_node",
            )?;
            spray(t, true)?;
        }
        let car_voices = r.audio["car_voices"].as_array().map_or(0, |v| v.len()) as u32;
        let sound = |t: &SourceId, s: &ToolSound| {
            check(
                t,
                (1..=car_voices).contains(&s.voice) && finite(&s.position),
                "sound",
            )
        };
        for p in &self.prototype_tools {
            let prototype = r.world.prototypes.iter().find(|w| w.source == p.prototype);
            let inside =
                |n: &SourceId| prototype.is_some_and(|w| w.nodes.iter().any(|x| &x.source == n));
            let source = match &p.kind {
                PrototypeTool::Brush { tool, .. } => &tool.source,
                PrototypeTool::Spray { tool, .. } => &tool.source,
            };
            check(
                source,
                prototype.is_some() && prototypes.insert(&p.prototype) && sources.insert(source),
                "prototype/source",
            )?;
            match &p.kind {
                PrototypeTool::Brush {
                    own_condition,
                    tool: t,
                    nodes: n,
                    sound: s,
                } => {
                    check(
                        &t.source,
                        [1, 2].contains(&t.cleaning_type)
                            && !own_condition.file.is_empty()
                            && own_condition.path_id != 0,
                        "cleaning_type/own_condition",
                    )?;
                    tool(t, false)?;
                    check(
                        &t.source,
                        [&n.model_equip, &n.model_default, &n.model, &n.scrub_node]
                            .into_iter()
                            .flatten()
                            .all(inside),
                        "prototype nodes",
                    )?;
                    check(
                        &t.source,
                        s.is_some() == (t.has_sound && t.voice_source.is_some()),
                        "sound/has_sound",
                    )?;
                    if let Some(s) = s {
                        sound(&t.source, s)?;
                    }
                }
                PrototypeTool::Spray {
                    nozzle,
                    renderer_node,
                    tool: t,
                    sound: s,
                } => {
                    check(
                        &t.source,
                        inside(nozzle) && inside(renderer_node),
                        "nozzle/renderer_node",
                    )?;
                    spray(t, false)?;
                    sound(&t.source, s)?;
                }
            }
        }
        for w in &self.washes {
            check(
                &w.source,
                w.id.0 > 0 && wash_ids.insert(w.id) && sources.insert(&w.source),
                "id/source",
            )?;
            check(
                &w.source,
                nodes.contains(&w.node)
                    && w.objects.iter().all(|n| nodes.contains(n))
                    && w.colliders.iter().all(|id| {
                        r.colliders
                            .iter()
                            .any(|c| c.id == *id && descendant(c.node, w.node))
                    }),
                "node/objects/colliders",
            )?;
            check(
                &w.source,
                (w.permission_index as usize) < self.clean_tables.len()
                    && w.repair_progress.is_finite()
                    && w.chance_to_start_disabled.is_finite()
                    && (0. ..=1.).contains(&w.chance_to_start_disabled),
                "permission_index/parameters",
            )?;
            for p in &w.particles {
                validate_particle(&w.source, p, r)?;
            }
        }
        let mut bound = HashSet::new();
        for b in &self.collider_bindings {
            check(
                &b.source,
                bound.insert(b.collider)
                    && r.colliders
                        .iter()
                        .any(|c| c.id == b.collider && c.source == b.source),
                "collider",
            )?;
            let slot_kinds = usize::from(b.installed_part_slot.is_some())
                + usize::from(b.installed_tire_slot.is_some());
            check(
                &b.source,
                slot_kinds <= 1
                    && b.installed_slot_source.is_some() == (slot_kinds == 1)
                    && b.installed_slot_source
                        .as_ref()
                        .is_none_or(|source| !source.file.is_empty() && source.path_id != 0),
                "installed_slot_source/kinds",
            )?;
            check(
                &b.source,
                b.installed_tire_slot.is_none_or(|id| {
                    r.wheel_parts
                        .iter()
                        .any(|part| part.item == id && part.tire_slot_anchor.is_some())
                }),
                "installed_tire_slot",
            )?;
            check(
                &b.source,
                [
                    b.ancestor_condition,
                    b.pickup_condition,
                    b.forwarded_condition,
                ]
                .iter()
                .flatten()
                .all(|id| surfaces.contains_key(id))
                    && b.installed_part_slot.is_none_or(|id| {
                        r.part_slots.iter().any(|s| {
                            s.id == id && Some(&s.source) == b.installed_slot_source.as_ref()
                        })
                    })
                    && b.wash.is_none_or(|id| {
                        washes.contains(&id)
                            && self
                                .washes
                                .iter()
                                .any(|w| w.id == id && w.colliders.contains(&b.collider))
                    }),
                "owners",
            )?;
        }
        Ok(())
    }
}

pub(super) fn validate_particle(
    owner: &SourceId,
    p: &ParticleReference,
    r: &ReferenceManifest,
) -> Result<(), ReferenceError> {
    check(
        owner,
        !p.source.file.is_empty()
            && p.source.path_id != 0
            && p.capacity > 0
            && p.duration.is_finite()
            && p.duration > 0.
            && finite(&[p.rate_over_time, p.rate_over_distance])
            && p.rate_over_time >= 0.
            && p.rate_over_distance >= 0.
            && p.simulation_space <= 2
            && transform(&p.local_pose)
            && p.scalars.values().all(|v| v.is_finite())
            && p.vectors.values().all(|v| finite(v))
            && p.curves.values().all(|c| {
                c.mode <= 3
                    && finite(&[c.scalar, c.min_scalar])
                    && particles::curve_valid(&c.min_curve)
                    && particles::curve_valid(&c.max_curve)
            })
            && p.colors.values().all(|c| {
                c.mode <= 4
                    && finite(&c.min_color)
                    && finite(&c.max_color)
                    && particles::gradient_valid(&c.min_gradient)
                    && particles::gradient_valid(&c.max_gradient)
            })
            && !p.materials.is_empty()
            && p.materials.iter().all(|m| {
                m.floats.values().all(|v| v.is_finite())
                    && m.colors.values().all(|v| finite(v))
                    && m.textures.iter().all(|t| {
                        finite(&t.scale)
                            && finite(&t.offset)
                            && r.lighting
                                .textures
                                .iter()
                                .any(|texture| texture.id == t.texture)
                    })
            }),
        "particles",
    )?;
    p.validate_motion()
}
