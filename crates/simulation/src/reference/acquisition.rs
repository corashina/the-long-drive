//! Checked acquisition inputs. Gameplay owns draw execution and publication.
use super::*;
pub(super) fn required_field<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer)
}

macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(#[serde(deserialize_with="required_field")] pub $field: $ty),* }
    };
}
record!(AcquisitionReference {
    stations: Vec<StationAcquisitionReference>, categories: Vec<CategoryReference>,
    r#box: BoxAcquisitionReference, packs: Vec<PackAcquisitionReference>,
    startup_packs: Vec<StartupPackReference>, holiday_pumpkin: SourceId, holiday_startup: HolidayPumpkinReference,
    pack_item_id: u32, cigarette_item_id: u32, evidence: AcquisitionEvidence, rules: AcquisitionRules
});
record!(AcquisitionEvidence { assembly_sha256: String, text_sha256: String, methods: Vec<AcquisitionMethod> });
record!(AcquisitionMethod {
    owner: String,
    signature: String,
    sha256: String
});
record!(AcquisitionMarker {
    source: SourceId,
    node: NodeId,
    transform: Transform
});
record!(AcquisitionPose { source: SourceId, parent: Option<SourceId>, coordinate_space: String, transform: Transform });
record!(AcquisitionDistance {
    source: SourceId,
    draw_distance: [f32; 2],
    item_distance: [f32; 2],
    authored_draw_distance: f32
});
record!(StationAcquisitionReference {
    source: SourceId, building: SourceId, asset: AssetId, root: NodeId, item_spawn_distance: AcquisitionDistance,
    building_chance: [f32;2], initial_items_spawned: bool, to_start: Vec<SourceId>, ordinary: Vec<OrdinaryAcquisitionReference>,
    underground: Vec<UndergroundAcquisitionReference>, misc4: Vec<SourceId>, flyers: StationFlyers
});
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquisitionSelection {
    Category {
        name: String,
        category_id: u32,
    },
    Item {
        name: String,
        item_id: u32,
        #[serde(deserialize_with = "required_field")]
        prototype: Option<PrototypeId>,
    },
    Prefab {
        name: String,
        source: SourceId,
        #[serde(deserialize_with = "required_field")]
        item_id: Option<u32>,
        #[serde(deserialize_with = "required_field")]
        prototype: Option<PrototypeId>,
    },
}
record!(CategoryReference {
    id: u32, children: Vec<u32>, categories: bool, weights: Vec<u32>, constructor_weights: Option<Vec<u32>>,
    depend_on_car: bool, constructor_sha256: String, prototypes: Vec<Option<PrototypeId>>, invalid_children: Vec<InvalidCategory>
});
record!(InvalidCategory {
    ordinal: u32,
    target: u32,
    failure: String
});
record!(StationFlyers {
    force: bool,
    halloween: bool,
    vertical_offset: f32,
    vertical_scale: f32,
    horizontal: [f32; 2],
    prefab: SourceId,
    supported: bool
});
record!(AcquisitionTableEntry {
    selection: AcquisitionSelection,
    weight: f32,
    threshold: f32
});
record!(OrdinaryAcquisitionReference {
    source: SourceId, marker: AcquisitionMarker, dont_spawn: bool, holiday: u32,
    building_affects_this: bool, chance: f32, spawned: bool, entries: Vec<OrdinaryAcquisitionEntry>
});
record!(OrdinaryAcquisitionEntry {
    selection: AcquisitionSelection, weight: u32, threshold: u32, places: Vec<AcquisitionPlace>,
    random_rotation: bool, disable_random: bool, initialization: AcquisitionInitialization
});
record!(AcquisitionPlace { weight: u32, threshold: u32, marker: Option<AcquisitionMarker> });
record!(AcquisitionInitialization {
    integer_range: [u32;2], float_range: [f32;2], override_fluid_amount: bool, fluid_range: [f32;2],
    override_fluid_type: bool, fluids: Vec<AcquisitionFluid>, no_mixed_fluids: bool, override_mix_chance: bool, mix_chance: f32
});
record!(AcquisitionFluid {
    r#type: u32,
    weight: f32,
    authored_threshold: f32
});
record!(UndergroundAcquisitionReference {
    source: SourceId, center: AcquisitionMarker, chance: f32, count: [u32;2], radius: [f32;2], vertical: [f32;2],
    table: Vec<AcquisitionTableEntry>
});
record!(BoxAcquisitionReference {
    prototype: PrototypeId, source: SourceId, table: Vec<AcquisitionTableEntry>, disable_spawn: bool, spawn_before_open: bool,
    spawned: bool, opened: bool, spawn_position: AcquisitionPose, authored_spawn_position: Option<SourceId>,
    breakable: SourceId, save: SourceId, mount: BoxMountReference, release: Vec<BoxRelease>, disabled_starter_boxes: Vec<DisabledBox>,
    content_counts: Vec<BoxContentCount>
});
record!(BoxMountReference {
    source: SourceId,
    parent_save: SourceId,
    collider: SourceId,
    only_manual: bool,
    no_physics_lock: bool,
    dont_lock_if_picked_up: bool
});
record!(BoxRelease {
    owner: SourceId,
    method: String,
    target: SourceId,
    target_method: String
});
record!(DisabledBox {
    source: SourceId,
    instance: SourceId,
    disable_spawn: bool
});
record!(BoxContentCount {
    prototype: PrototypeId,
    source: SourceId,
    maximum: u32
});
record!(AcquisitionAmmo {
    source: SourceId,
    count: u32,
    float_count: f32,
    maximum: f32,
    float_or_int: bool,
    r#type: u32,
    random_range: [f32; 2]
});
record!(PackAcquisitionReference {
    prototype: PrototypeId, source: SourceId, ammo: AcquisitionAmmo, cigarette: PrototypeId,
    cigarette_prefab: SourceId, take_one_order: Vec<String>, wearable_type: u32
});
record!(StartupPackReference {
    source: SourceId, pose: AcquisitionPose, active: bool, save: SourceId, item_id: u32, body: SourceId, pickup: SourceId,
    manual_start: bool, started: bool, loaded: bool, buried: bool, car: Option<SourceId>, fixed_in_car: u32,
    prototype: PrototypeId, group: SourceId, group_pose: AcquisitionPose, group_active: bool, vehicle: SourceId,
    pack: SourceId, cigarette_prefab: SourceId, ammo: AcquisitionAmmo
});
impl StartupPackReference {
    /// The start car field of the pack group.
    pub fn start_car(&self) -> &'static str {
        pack_start_car(self.source.path_id)
    }
}
/// Source `StartCar01` keeps pack 14079, `StartCar02` pack 14082.
fn pack_start_car(pack: i64) -> &'static str {
    if pack == 14079 {
        "StartCar01"
    } else {
        "StartCar02"
    }
}
record!(HolidayPumpkinReference {
    source: SourceId, pose: AcquisitionPose, active: bool, save: SourceId, item_id: u32, body: SourceId, pickup: SourceId,
    manual_start: bool, started: bool, loaded: bool, buried: bool, car: Option<SourceId>, fixed_in_car: u32,
    prototype: PrototypeId, wearable: SourceId, terrain: SourceId, start: [u32;2], end: [u32;2],
    inclusive: bool, local_calendar: bool, new_game_only: bool, multiplayer: bool
});
pub(super) fn source_is(s: &SourceId, file: &str, id: i64) -> bool {
    s.file == file && s.path_id == id
}
fn valid_source(s: &SourceId) -> bool {
    s.path_id > 0
        && [
            "level0",
            "level1",
            "sharedassets0.assets",
            "sharedassets1.assets",
        ]
        .contains(&s.file.as_str())
}
fn valid_pose(p: &AcquisitionPose) -> bool {
    valid_source(&p.source)
        && p.parent.as_ref().is_none_or(valid_source)
        && p.coordinate_space == "source_parent_local"
        && valid_transform(&p.transform)
        && checked_pose(p)
}
fn valid_transform(t: &Transform) -> bool {
    t.translation
        .iter()
        .chain(&t.rotation)
        .chain(&t.scale)
        .all(|v| v.is_finite())
        && t.scale.iter().all(|v| *v != 0.)
        && (t.rotation.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.002
}
fn same_transform(a: &Transform, b: &Transform) -> bool {
    a.translation == b.translation && a.rotation == b.rotation && a.scale == b.scale
}
fn valid_marker(p: &AcquisitionMarker, asset: AssetId, m: &ReferenceManifest) -> bool {
    valid_source(&p.source)
        && valid_transform(&p.transform)
        && m.world
            .prototypes
            .iter()
            .find(|v| v.asset == asset)
            .and_then(|v| v.nodes.get(p.node.0 as usize))
            .is_some_and(|n| n.source == p.source && same_transform(&n.transform, &p.transform))
}
fn prototype<'a>(
    id: &PrototypeId,
    item: Option<u32>,
    m: &'a ReferenceManifest,
) -> Result<&'a ItemPrototype, ReferenceError> {
    m.item_prototypes
        .iter()
        .find(|p| p.id == *id && (item.is_none() || p.item_id == item))
        .ok_or_else(|| ReferenceError("acquisition prototype link".into()))
}

impl AcquisitionReference {
    fn selection(
        &self,
        s: &AcquisitionSelection,
        m: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        match s {
            AcquisitionSelection::Category { name, category_id } => require(
                self.categories.iter().any(|c| c.id == *category_id) && !name.is_empty(),
                "acquisition category link",
            ),
            AcquisitionSelection::Item {
                name,
                item_id,
                prototype,
            } => require(
                name == "box01"
                    && *item_id == 62
                    && m.item_prototypes
                        .iter()
                        .find(|p| p.item_id == Some(*item_id))
                        .map(|p| &p.id)
                        == prototype.as_ref(),
                "acquisition item selector",
            ),
            AcquisitionSelection::Prefab {
                name,
                source,
                item_id,
                prototype,
            } => {
                require(
                    source.file == "sharedassets1.assets"
                        && prefab_identity(source.path_id) == Some((name.as_str(), *item_id)),
                    "acquisition prefab source item link",
                )?;
                let expected = prefab_root(source)
                    .and_then(|id| {
                        m.item_prototypes
                            .iter()
                            .find(|p| source_is(&p.id.0, "sharedassets1.assets", id))
                    })
                    .map(|p| &p.id);
                require(
                    expected == prototype.as_ref(),
                    "acquisition prefab prototype",
                )
            }
        }
    }
    fn table(
        &self,
        entries: &[AcquisitionTableEntry],
        m: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        require(!entries.is_empty(), "acquisition table empty")?;
        let mut threshold = 0.;
        for row in entries {
            self.selection(&row.selection, m)?;
            threshold += row.weight;
            require(
                row.weight.is_finite()
                    && row.weight > 0.
                    && row.threshold.is_finite()
                    && row.threshold == threshold,
                "acquisition table threshold",
            )?;
        }
        Ok(())
    }
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        self.rules.validate()?;
        require(
            self.pack_item_id == 74
                && self.cigarette_item_id == 230
                && source_is(&self.holiday_pumpkin, "level1", 82),
            "acquisition identities",
        )?;
        require(
            self.categories.len() == CATEGORY_IDS.len()
                && self
                    .categories
                    .iter()
                    .map(|c| c.id)
                    .eq(CATEGORY_IDS.iter().copied()),
            "acquisition category closure",
        )?;
        for c in &self.categories {
            require(
                category_contract(c) && c.prototypes.len() == c.children.len(),
                "acquisition checked category",
            )?;
            let mut invalid = Vec::new();
            for (i, (child, p)) in c.children.iter().zip(&c.prototypes).enumerate() {
                if c.categories {
                    require(p.is_none(), "category cannot be prototype")?;
                    if !self.categories.iter().any(|c| c.id == *child) {
                        let known = match c.id {
                            12 => [186, 265, 303, 387].contains(child),
                            13 => [187, 266, 304, 386].contains(child),
                            _ => false,
                        };
                        require(known, "missing category link")?;
                        invalid.push((i as u32, *child));
                    }
                } else {
                    require(
                        m.item_prototypes
                            .iter()
                            .find(|p| p.item_id == Some(*child))
                            .map(|p| &p.id)
                            == p.as_ref(),
                        "acquisition leaf prototype",
                    )?;
                }
            }
            require(
                c.invalid_children
                    .iter()
                    .map(|v| (v.ordinal, v.target))
                    .eq(invalid)
                    && c.invalid_children
                        .iter()
                        .all(|v| v.failure == "category_index_out_of_range"),
                "native category error terminal",
            )?;
        }
        for category in &self.categories {
            let mut pending = category.children.clone();
            let mut visited = HashSet::new();
            while let Some(id) = pending.pop() {
                require(
                    id != category.id || !category.categories,
                    "acquisition category cycle",
                )?;
                if visited.insert(id) && category.categories {
                    if let Some(child) = self.categories.iter().find(|c| c.id == id && c.categories)
                    {
                        pending.extend(&child.children);
                    }
                }
            }
        }
        require(self.stations.len() == 3, "acquisition station count")?;
        for (index, s) in self.stations.iter().enumerate() {
            let station = m.world.prototypes.iter().find(|p| p.asset == s.asset);
            require(
                source_is(
                    &s.source,
                    "sharedassets1.assets",
                    [26573, 27344, 41123][index],
                ) && s.root.0 == 0
                    && station.is_some_and(|p| m.world.records.iter().find(|r| r.source == p.source)
                        .is_some_and(|r| matches!(r.fields.get("m_GameObject"), Some(WorldValue::Link(owner)) if owner == &s.source)))
                    && source_is(
                        &s.building,
                        "sharedassets1.assets",
                        [158348, 155446, 156988][index],
                    ),
                "acquisition station link",
            )?;
            let f = &s.flyers;
            require(
                !f.force
                    && f.halloween
                    && f.vertical_offset == 10.
                    && f.vertical_scale == 5.
                    && f.horizontal == [6., 16.]
                    && source_is(&f.prefab, "sharedassets1.assets", 32394)
                    && f.supported,
                "station flyer contract",
            )?;
            let d = &s.item_spawn_distance;
            require(
                source_is(&d.source, "level1", 65886)
                    && d.draw_distance == [500., 25000.]
                    && d.item_distance == [140., 340.]
                    && d.authored_draw_distance == 2500.,
                "acquisition distance settings",
            )?;
            require(
                s.building_chance == [0., 0.] && !s.initial_items_spawned && s.to_start.is_empty(),
                "acquisition building startup",
            )?;
            require(
                s.ordinary.len() == [33, 37, 236][index]
                    && s.ordinary
                        .iter()
                        .map(|o| &o.source)
                        .collect::<HashSet<_>>()
                        .len()
                        == s.ordinary.len(),
                "acquisition spawner count or duplicate",
            )?;
            let mut misc = Vec::new();
            for o in &s.ordinary {
                require(
                    valid_source(&o.source)
                        && valid_marker(&o.marker, s.asset, m)
                        && o.holiday <= 2
                        && (0.0..=1.).contains(&o.chance)
                        && !o.spawned,
                    "ordinary acquisition source",
                )?;
                let mut threshold = 0u32;
                for e in &o.entries {
                    self.selection(&e.selection, m)?;
                    threshold = threshold
                        .checked_add(e.weight.max(1))
                        .ok_or_else(|| ReferenceError("acquisition threshold overflow".into()))?;
                    require(e.threshold == threshold, "ordinary item threshold")?;
                    let mut place_threshold = 0u32;
                    for p in &e.places {
                        place_threshold = place_threshold
                            .checked_add(p.weight.max(1))
                            .ok_or_else(|| ReferenceError("acquisition place overflow".into()))?;
                        require(
                            p.threshold == place_threshold
                                && p.marker
                                    .as_ref()
                                    .is_none_or(|p| valid_marker(p, s.asset, m)),
                            "ordinary place threshold or marker",
                        )?;
                    }
                    let v = &e.initialization;
                    require(
                        v.float_range
                            .iter()
                            .chain(&v.fluid_range)
                            .chain([&v.mix_chance])
                            .all(|v| v.is_finite())
                            && v.fluids
                                .iter()
                                .all(|v| v.weight.is_finite() && v.authored_threshold.is_finite()),
                        "acquisition initialization numeric fields",
                    )?;
                }
                if o.entries.iter().any(|e| {
                    matches!(
                        e.selection,
                        AcquisitionSelection::Category {
                            category_id: 35,
                            ..
                        }
                    )
                }) {
                    misc.push(&o.source);
                }
            }
            require(
                s.misc4.iter().eq(misc)
                    && s.misc4.len() == [18, 6, 38][index]
                    && s.underground.len() == 1,
                "misc4 and underground counts",
            )?;
            let u = &s.underground[0];
            require(
                source_is(
                    &u.source,
                    "sharedassets1.assets",
                    [154992, 132529, 157204][index],
                ) && valid_marker(&u.center, s.asset, m)
                    && u.chance == 0.4
                    && u.count == [1, 5]
                    && u.radius == [[3., 9.], [5., 11.], [13., 20.]][index]
                    && u.vertical == [-0.15, -0.4],
                "underground source rules",
            )?;
            self.table(&u.table, m)?;
        }
        self.validate_objects(m)?;
        require(
            self.evidence.assembly_sha256
                == "9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e"
                && self.evidence.text_sha256
                    == "22ddc62b91954205d90ff513232f39becebbc58a71b718685075f91f64b1f3ee"
                && self.evidence.methods.len() == 46
                && self
                    .evidence
                    .methods
                    .iter()
                    .all(|e| !e.owner.is_empty() && !e.signature.is_empty() && hash(&e.sha256))
                && self
                    .evidence
                    .methods
                    .iter()
                    .map(|e| (&e.owner, &e.signature))
                    .collect::<HashSet<_>>()
                    .len()
                    == 46,
            "acquisition checked methods",
        )?;
        Ok(())
    }
}

impl AcquisitionReference {
    fn validate_objects(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let b = &self.r#box;
        let p = prototype(&b.prototype, Some(62), m)?;
        let behavior = &p.behavior;
        require(
            behavior.spawn.as_ref() == Some(&b.source)
                && behavior.breakable.as_ref() == Some(&b.breakable),
            "Box behavior links",
        )?;
        self.table(&b.table, m)?;
        require(
            !b.disable_spawn
                && b.spawn_before_open
                && !b.spawned
                && !b.opened
                && b.authored_spawn_position.is_none()
                && (source_is(&b.spawn_position.source, "sharedassets1.assets", 51722)
                    && valid_pose(&b.spawn_position)),
            "Box spawn rules",
        )?;
        let box_gib = |id: &Option<PrototypeId>, path: i64| {
            id.as_ref()
                .is_some_and(|id| source_is(&id.0, "sharedassets1.assets", path))
        };
        let r = m.breakable(&b.breakable);
        require(
            r.is_some_and(|r| {
                r.health == 7.
                    && !r.decrease_after_attack
                    && !r.only_decrease_above
                    && r.decrease_above_health == 0.
                    && r.mass_modifier == 1.
                    && !r.only_velocity
                    && r.no_trigger
                    && !r.no_collider
                    && !r.no_destroy
                    && !r.no_gib
                    && box_gib(&r.gib, item_prototypes::BOX_GIBS[0])
                    && box_gib(&r.reduced_gib, item_prototypes::BOX_GIBS[1])
                    && r.clips.iter().map(|c| c.path_id).eq([4223, 4140])
            }),
            "Box breakables record",
        )?;
        let mount = &b.mount;
        require(
            behavior.mount.as_ref() == Some(&mount.source)
                && mount.parent_save == b.save
                && source_is(&b.save, "sharedassets1.assets", 145573)
                && p.colliders.iter().any(|c| c.source == mount.collider)
                && mount.only_manual
                && !mount.no_physics_lock
                && !mount.dont_lock_if_picked_up,
            "Box mount links",
        )?;
        require(
            b.release.len() == 2
                && b.release[0].owner == b.breakable
                && b.release[0].method == "Break"
                && b.release[0].target == b.save
                && b.release[0].target_method == "PreDestroy"
                && b.release[1].owner == b.save
                && b.release[1].method == "PreDestroy"
                && b.release[1].target == mount.source
                && b.release[1].target_method == "Unstore",
            "Box release operation links",
        )?;
        require(
            b.disabled_starter_boxes.len() == 8,
            "Box disabled instances",
        )?;
        for (d, (source, instance)) in b.disabled_starter_boxes.iter().zip(DISABLED_BOXES) {
            require(
                source_is(&d.source, "level1", *source)
                    && source_is(&d.instance, "level1", *instance)
                    && d.disable_spawn,
                "disabled starter Box",
            )?;
        }
        require(
            b.content_counts.len() == CONTENT_COUNTS.len(),
            "Box content count table",
        )?;
        for (v, (root, source, maximum)) in b.content_counts.iter().zip(CONTENT_COUNTS) {
            prototype(&v.prototype, None, m)?;
            require(
                source_is(&v.prototype.0, "sharedassets1.assets", *root)
                    && source_is(&v.source, "sharedassets1.assets", *source)
                    && v.maximum == *maximum,
                "Box content count source",
            )?;
        }
        // Source DestroyOthers keeps the pack of each start car group with a car placement.
        let start_car = |group: &str| {
            m.world
                .startup
                .iter()
                .any(|s| s.prototype.is_some() && s.group == group)
        };
        let startup_packs = STARTUP_PACKS
            .iter()
            .filter(|expected| start_car(pack_start_car(expected[0])));
        require(
            self.packs.len() == 1 && self.startup_packs.len() == startup_packs.clone().count(),
            "selected packs",
        )?;
        let pack = &self.packs[0];
        let p = prototype(&pack.prototype, Some(74), m)?;
        prototype(&pack.cigarette, Some(230), m)?;
        require(
            p.behavior.pack.as_ref() == Some(&pack.source)
                && p.behavior.cigarette_prefab.as_ref() == Some(&pack.cigarette_prefab)
                && pack.wearable_type == 4
                && pack.take_one_order.iter().map(String::as_str).eq([
                    "decrement_integer_count",
                    "instantiate_at_pack_pose",
                    "initialize",
                    "wear_if_slot_empty",
                ]),
            "pack source rules",
        )?;
        validate_ammo(&pack.ammo, "sharedassets1.assets", 141757)?;
        for (v, expected) in self.startup_packs.iter().zip(startup_packs) {
            prototype(&v.prototype, Some(74), m)?;
            require(
                [
                    &v.source, &v.save, &v.body, &v.pickup, &v.group, &v.vehicle, &v.pack,
                ]
                .iter()
                .zip(expected)
                .all(|(s, id)| source_is(s, "level1", *id)),
                "selected startup pack sources",
            )?;
            validate_ammo(&v.ammo, "level1", expected[7])?;
            require(
                source_is(
                    &v.pose.source,
                    "level1",
                    if expected[0] == 14079 { 33859 } else { 33862 },
                ) && source_is(
                    &v.group_pose.source,
                    "level1",
                    if expected[0] == 14079 { 21655 } else { 21567 },
                ) && valid_pose(&v.pose)
                    && valid_pose(&v.group_pose)
                    && v.pose.parent.as_ref() == Some(&v.group_pose.source)
                    && v.active
                    && !v.group_active
                    && v.manual_start
                    && !v.started
                    && !v.loaded
                    && !v.buried
                    && v.car.is_none()
                    && v.fixed_in_car == 0
                    && v.item_id == 74
                    && v.cigarette_prefab == pack.cigarette_prefab,
                "startup pack initialization",
            )?;
        }
        let h = &self.holiday_startup;
        prototype(&h.prototype, Some(92), m)?;
        require(
            h.source == self.holiday_pumpkin
                && source_is(&h.wearable, "level1", 79857)
                && source_is(&h.terrain, "level1", 64898)
                && source_is(&h.pose.source, "level1", 21449)
                && valid_pose(&h.pose)
                && h.start == [10, 15]
                && h.end == [11, 15]
                && h.inclusive
                && h.local_calendar
                && h.new_game_only
                && !h.multiplayer
                && !h.started
                && !h.loaded
                && !h.buried
                && h.car.is_none()
                && h.fixed_in_car == 0
                && h.item_id == 92
                && source_is(&h.save, "level1", 72331)
                && source_is(&h.body, "level1", 50268)
                && source_is(&h.pickup, "level1", 80624)
                && h.active
                && h.manual_start,
            "holiday startup source",
        )?;
        Ok(())
    }
}
fn validate_ammo(v: &AcquisitionAmmo, file: &str, id: i64) -> Result<(), ReferenceError> {
    require(
        source_is(&v.source, file, id)
            && v.count == 20
            && v.float_count == 0.
            && v.maximum == 20.
            && !v.float_or_int
            && v.r#type == 5
            && v.random_range == [20., 1.],
        "pack count and reversed range",
    )
}

const CATEGORY_IDS: &[u32] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51,
    53, 54, 55, 56, 57, 58, 59, 60, 61, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
    78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89,
];
fn prefab_root(source: &SourceId) -> Option<i64> {
    if source.file != "sharedassets1.assets" {
        return None;
    }
    match source.path_id {
        32825 => Some(75443),
        9085 => Some(51722),
        18963 => Some(61587),
        39185 => Some(81642),
        39188 => Some(81645),
        43845 => Some(70812),
        24481 => Some(53838),
        24480 => Some(53837),
        20981 => Some(63605),
        20649 => Some(63259),
        11985 => Some(54495),
        41097 => Some(83430),
        _ => None,
    }
}
fn category_contract(c: &CategoryReference) -> bool {
    !c.children.is_empty()
        && c.children.len() == c.weights.len()
        && c.weights.iter().all(|w| *w > 0)
        && c.constructor_weights
            .as_ref()
            .is_none_or(|w| w.len() == c.children.len())
        && c.weights.iter().enumerate().all(|(i, w)| {
            *w == if c.categories {
                c.constructor_weights.as_ref().map_or(1, |v| v[i])
            } else {
                1
            }
        })
        && hash(&c.constructor_sha256)
}
const DISABLED_BOXES: &[(i64, i64)] = &[
    (79792, 1904),
    (79793, 1902),
    (79794, 1903),
    (79795, 1909),
    (79796, 1906),
    (79797, 1905),
    (79798, 1908),
    (79799, 1907),
];
const CONTENT_COUNTS: &[(i64, i64, u32)] = &[
    (75443, 132605, 10),
    (51722, 136544, 0),
    (61587, 156405, 0),
    (70812, 134682, 5),
    (53838, 147662, 3),
    (53837, 147661, 3),
    (63605, 134243, 3),
    (63259, 141432, 0),
    (54495, 134663, 0),
    (51557, 137709, 0),
    (51878, 132739, 0),
];
const STARTUP_PACKS: &[[i64; 8]] = &[
    [14079, 72692, 50432, 75069, 298, 69517, 70704, 70998],
    [14082, 72693, 50434, 75072, 206, 77839, 70705, 70999],
];
record!(AcquisitionRules {integer_upper_exclusive:bool,float_upper_inclusive:bool,car_type_to_item_index:Vec<u32>});
impl AcquisitionRules {
    fn validate(&self) -> Result<(), ReferenceError> {
        require(
            self.integer_upper_exclusive
                && self.float_upper_inclusive
                && self.car_type_to_item_index
                    == [
                        1, 1, 15, 98, 29, 94, 105, 122, 140, 153, 173, 178, 222, 243, 257, 275,
                        309, 0, 337, 356, 371, 390, 396,
                    ],
            "acquisition range or vehicle mapping",
        )
    }
}

fn prefab_identity(id: i64) -> Option<(&'static str, Option<u32>)> {
    match id {
        32825 => Some(("", Some(74))),
        39986 => Some(("", Some(54))),
        6496 => Some(("", Some(71))),
        42570 => Some(("", Some(60))),
        12981 => Some(("", Some(86))),
        30814 => Some(("", Some(72))),
        35111 => Some(("", Some(76))),
        39989 => Some(("", Some(57))),
        20979 => Some(("", Some(75))),
        20649 => Some(("", Some(92))),
        30548 => Some(("", Some(254))),
        16468 => Some(("", Some(80))),
        28749 => Some(("", Some(66))),
        26645 => Some(("", Some(85))),
        32753 => Some(("", Some(83))),
        41097 => Some(("", None)),
        9085 => Some(("", Some(62))),
        9086 => Some(("", Some(231))),
        24105 => Some(("", Some(87))),
        26970 => Some(("", Some(88))),
        35853 => Some(("", Some(70))),
        36799 => Some(("", Some(81))),
        35272 => Some(("", Some(82))),
        7370 => Some(("", Some(84))),
        _ => None,
    }
}

fn checked_pose(p: &AcquisitionPose) -> bool {
    match (p.source.file.as_str(), p.source.path_id) {
        ("sharedassets1.assets", 51722) => {
            p.parent.is_none()
                && p.transform.translation == [0.0, 0.0, -0.0]
                && p.transform.rotation == [-0.0, -0.0, 0.0, 1.0]
                && p.transform.scale == [1.0, 1.0, 1.0]
        }
        ("level1", 21449) => {
            p.parent
                .as_ref()
                .is_some_and(|v| source_is(v, "level1", 21481))
                && p.transform.translation
                    == [1.4539967775344849, 6.00600004196167, 14.700002670288086]
                && p.transform.rotation == [-0.0, -1.0, 0.0, 0.0]
                && p.transform.scale == [1.0, 1.0, 1.0]
        }
        ("level1", 33859) => {
            p.parent
                .as_ref()
                .is_some_and(|v| source_is(v, "level1", 21655))
                && p.transform.translation
                    == [
                        -0.15660084784030914,
                        -0.27971217036247253,
                        -0.5606002807617188,
                    ]
                && p.transform.rotation
                    == [
                        0.7058812379837036,
                        0.7058789730072021,
                        0.04163286089897156,
                        0.04163298010826111,
                    ]
                && p.transform.scale == [100.00006103515625, 100.00003051757812, 100.00003051757812]
        }
        ("level1", 21655) => {
            p.parent
                .as_ref()
                .is_some_and(|v| source_is(v, "level1", 21481))
                && p.transform.translation
                    == [1.4890013933181763, 2.002000093460083, 11.198002815246582]
                && p.transform.rotation == [0.0, 0.0, -0.7071056962013245, 0.7071078419685364]
                && p.transform.scale == [1.0, 1.0, 1.0]
        }
        ("level1", 33862) => {
            p.parent
                .as_ref()
                .is_some_and(|v| source_is(v, "level1", 21567))
                && p.transform.translation
                    == [0.3570023775100708, 0.35050010681152344, -1.0168991088867188]
                && p.transform.rotation
                    == [
                        0.0,
                        0.058877840638160706,
                        -0.998265266418457,
                        9.126959810146218e-08,
                    ]
                && p.transform.scale == [100.00015258789062, 100.0001449584961, 100.0001220703125]
        }
        ("level1", 21567) => {
            p.parent
                .as_ref()
                .is_some_and(|v| source_is(v, "level1", 21481))
                && p.transform.translation
                    == [1.565999984741211, 1.8350000381469727, 9.581000328063965]
                && p.transform.rotation == [0.0, -1.0, -0.0, 0.0]
                && p.transform.scale == [1.0, 1.0, 1.0]
        }
        _ => false,
    }
}
