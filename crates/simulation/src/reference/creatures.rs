//! Source rabbit prefab (`aispawn.AIPrefab`) and the `stimulscript` sources that rabbits sense.
//! Positions and directions use the converted space. Angles are in degrees. Ranges are [min, max].
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreaturesReference {
    pub rabbit: RabbitReference,
    /// Every `stimulscript`, sorted by source.
    pub stimuli: Vec<StimulusSource>,
    /// `Stimul` on the player Head.
    pub player_stimulus: SourceId,
    /// The stimulus of the `mainscript.Audio` prefab. `PlayClipAtPoint` sets it to the volume.
    pub sound_stimulus: SourceId,
}

/// Source `stimulType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StimulusKind {
    Nothing,
    Food,
    Water,
    Threat,
    Light,
    Command,
}

/// Source `stimul`: a kind and a value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StimulusValue {
    pub kind: StimulusKind,
    pub value: f32,
}

/// One `stimulscript`. Its collider position is the stimulus position.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StimulusSource {
    pub source: SourceId,
    /// Transform of the stimulus GameObject.
    pub node: SourceId,
    /// Top Transform of the owner hierarchy.
    pub root: SourceId,
    pub visible: bool,
    pub sound: bool,
    pub smell: bool,
    pub stimuli: Vec<StimulusValue>,
    /// `CheckAudioSources` and `SetAudioValue` set stimulus 0 to volume times this value.
    pub max_sound_value: f32,
    /// False: `DontStartCheckingAudioSources`. Code calls `SetAudioValue`.
    pub check_audio_sources: bool,
    pub audio_sources: Vec<SourceId>,
}

/// Source `aispawn` on the Player.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnerReference {
    pub source: SourceId,
    pub chance: f32,
    /// Seconds between spawn tries.
    pub time_range: [f32; 2],
    /// `Random.Range(minCount, maxCount + 1)` is read again each loop step.
    pub count_range: [f32; 2],
    pub max_count: u32,
    pub distance_range: [f32; 2],
    /// Player-local offset of the ring center, through `InverseTransformDirection`.
    pub offset: [f32; 3],
    pub radius: f32,
    pub half_fov: f32,
    pub surface_mask: u32,
    pub sphere_mask: u32,
}

/// Source `aiscript` values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrainReference {
    pub source: SourceId,
    /// Velocity change per frame. `Start` squares it.
    pub acceleration_death: f32,
    pub go_to_target: bool,
    pub flee: bool,
    pub pre_calc_flee: bool,
    pub avoid_obstacles: bool,
    pub roam: bool,
    pub attack: bool,
    pub jump: bool,
    pub jump_if_speed: f32,
    pub jump_sense_distance: f32,
    pub jump_mask: u32,
    pub enable_invert: bool,
    pub go_to_player_target: bool,
    pub target_pre_calc_distance: f32,
    pub target_distance_goal: f32,
    pub target_angle_goal: f32,
    pub target_angle_move: f32,
    pub walk_distance: f32,
    pub run_distance: f32,
    pub attack_speed: f32,
    pub flee_distance: f32,
    pub min_flee_speed: f32,
    pub flee_time: f32,
    pub walk_speed_modifier: f32,
    pub move_towards_target_modifier: f32,
    pub move_forward_modifier: f32,
    pub roam_x: [f32; 2],
    pub roam_z: [f32; 2],
    pub roam_time: [f32; 2],
    pub invert_flight_if_above: f32,
    pub preferences: Vec<StimulusValue>,
}

/// Source `movecreaturescript` values. `nyulscript` replaces speed and turn rate by size.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionReference {
    pub source: SourceId,
    pub acceleration: f32,
    pub speed: f32,
    /// Degrees per second.
    pub rot_speed: f32,
    pub jump_force: f32,
    pub grounded_ray_distance: f32,
    pub grounded_mask: u32,
}

/// One `sensoryscript.visionRay`. The direction is in eye space; the mirror ray negates x.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisionRay {
    pub enabled: bool,
    pub inverted: bool,
    pub symmetry: bool,
    /// Source `BOOL`: full value on any hit.
    pub boolean: bool,
    /// Unit, from `Quaternion.Euler(rayDir) * Vector3.forward`.
    pub direction: [f32; 3],
    pub length: f32,
    pub mask: u32,
    pub max_value: f32,
    /// `Start` adds a random value in [-spread, spread] to `max_value`.
    pub spread: f32,
}

/// Source `sensoryscript` values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensesReference {
    pub source: SourceId,
    pub scan_interval: f32,
    /// Eye Transform node.
    pub eyes: NodeId,
    /// `LooseEyes` hides these nodes.
    pub eye_objects: Vec<NodeId>,
    /// Full X and Y cone angles and the vision distance.
    pub fov: [f32; 3],
    pub hear_distance: f32,
    pub min_hear_distance: f32,
    pub min_hear_value: f32,
    pub hear_modifier: f32,
    pub stimulus_mask: u32,
    pub vision_mask: u32,
    pub rays: Vec<VisionRay>,
}

/// Source `nyulscript.SetBasedOnSize` ranges. Each value is `lerp(range[0], range[1], size)`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RabbitSizeReference {
    pub source: SourceId,
    pub scale: [f32; 2],
    pub mass: [f32; 2],
    pub health: [f32; 2],
    pub shoot_health: [f32; 2],
    pub speed: [f32; 2],
    /// From `maxRotSpeed` to `minRotSpeed`: big rabbits turn slower.
    pub rot_speed: [f32; 2],
    /// `RunValue` lerp rate.
    pub run_lerp: f32,
    /// Eye renderer nodes.
    pub eyes: Vec<NodeId>,
}

/// One `breakchilds` of the rabbit breakable.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SenseChild {
    pub source: SourceId,
    pub node: NodeId,
    pub lose_eye_chance: f32,
    pub lose_ear_chance: f32,
    pub lose_sense_force: f32,
    /// True: shots can take the senses.
    pub senses: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureDamage {
    /// The rabbit `breakablescript`. It has no owners: the creature runtime binds it.
    pub rules: BreakableReference,
    /// In `rules.children` order.
    pub children: Vec<SenseChild>,
}

/// One anim collider of the live rabbit. Geometry is in the space of its node.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCollider {
    pub node: NodeId,
    pub source: SourceId,
    pub sensor: bool,
    pub membership: u32,
    pub filter: u32,
    pub friction: f32,
    pub restitution: f32,
    pub geometry: ColliderGeometry,
    pub step_surface: Option<StepSurface>,
    pub tag: Option<ColliderTag>,
}

/// The live root Rigidbody. `SetMass` sets its mass to the size mass over the bone count.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureBody {
    pub source: SourceId,
    pub mass_kg: f32,
    /// Unity `RigidbodyConstraints` bits.
    pub constraints: u32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub colliders: Vec<LiveCollider>,
}

/// `ragdollactivatescript.SetRagdol(true)` bodies, colliders, and joints.
/// Body poses are in prefab root space. Collider geometry and joint anchors are in body space.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureRagdoll {
    pub source: SourceId,
    pub bodies: Vec<BodyRecord>,
    pub colliders: Vec<ColliderRecord>,
    pub joints: Vec<RagdollJoint>,
    /// The `pickupable` of each body, in body order. `SetRagdol(true)` clears `cant_pickup`.
    pub pickups: Vec<PickupReference>,
}

/// The base layer default state: a looping 1D blend tree. Track nodes are manifest node ids.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendReference {
    pub source: SourceId,
    pub parameter: String,
    /// Ascending, one per clip.
    pub thresholds: Vec<f32>,
    pub clips: Vec<MotionClip>,
    pub overlay: OverlayLayer,
    pub actions: AnimatorActions,
}

/// Base layer states that `nyulscript.Roar` and `Attack` start through their triggers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorActions {
    /// Any state goes here on the `Roar` trigger.
    pub roar: ActionState,
    /// `Idle,move` goes here on the `Attack` trigger at its exit time.
    pub attack: ActionState,
}

/// One single-clip state and its transitions in and back to `Idle,move`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionState {
    pub clip: MotionClip,
    pub speed: f32,
    pub r#loop: bool,
    pub enter: ActionTransition,
    pub leave: ActionTransition,
}

/// A fixed-duration transition. `exit_time` is the normalized source state time; none means
/// the transition waits only for its trigger. `offset` is the normalized start time of the target.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionTransition {
    pub exit_time: Option<f32>,
    pub duration: f32,
    pub offset: f32,
}

/// Source `nyulscript` evil behaviour: `GetEvil`, `SeeTarget`, `Roar`, `Attack`, the idle growl.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvilReference {
    pub source: SourceId,
    /// Degrees.
    pub attack_angle: f32,
    pub attack_distance: f32,
    pub attack_duration: f32,
    pub attack_cooldown: f32,
    /// `move.modifierValue` while attacking.
    pub attack_modifier: f32,
    pub roar_duration: f32,
    pub target_forget_time: f32,
    /// Seconds between idle growls.
    pub idle_time: [f32; 2],
    /// Compared with the squared player distance, as in the source.
    pub idle_distance: f32,
    pub roar_pitch: f32,
    /// Audio clip ids. They play through `die_voice`, the `PlayClipAtPoint` template.
    pub roar_clips: Vec<u32>,
    pub idle_clips: Vec<u32>,
    pub attack_clips: Vec<u32>,
    pub eye_materials: EyeMaterials,
    pub trigger: AttackTrigger,
}

/// `EyeEvil` and `EyeGreen` in the rabbit asset.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EyeMaterials {
    pub evil: LightingMaterialLink,
    pub green: LightingMaterialLink,
}

/// The `PlayerDeathTrigger` sphere and its `playerdeathtriggerscript` damage per second.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttackTrigger {
    pub damage_player: f32,
    pub damage_breakable: f32,
    pub collider: LiveCollider,
}

/// Animator layer 1 `piszmog`: an override layer that loops one clip.
/// The clip keeps only the tracks of bones in the layer mask.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayLayer {
    pub weight: f32,
    pub clip: MotionClip,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RabbitReference {
    /// Prefab root Transform.
    pub source: SourceId,
    pub asset: AssetId,
    pub root: NodeId,
    /// `tosaveitemscript` id.
    pub item_id: u32,
    pub spawner: SpawnerReference,
    pub brain: BrainReference,
    pub motion: MotionReference,
    pub senses: SensesReference,
    pub size: RabbitSizeReference,
    pub damage: CreatureDamage,
    pub body: CreatureBody,
    pub ragdoll: CreatureRagdoll,
    pub animator: BlendReference,
    pub evil: EvilReference,
    /// `Sdie` audio clip ids.
    pub die_clips: Vec<u32>,
    /// One-shot car voice template of `mainscript.PlayClipAtPoint`.
    pub die_voice: u32,
    pub evidence: ManagedEvidence,
}

fn range(r: &[f32; 2]) -> bool {
    finite(r) && 0. <= r[0] && r[0] <= r[1]
}

impl CreaturesReference {
    pub fn stimulus(&self, source: &SourceId) -> Option<&StimulusSource> {
        self.stimuli.iter().find(|s| &s.source == source)
    }
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        self.rabbit.validate(m)?;
        require(
            self.stimuli.iter().all(|s| {
                s.max_sound_value.is_finite() && s.stimuli.iter().all(|v| v.value.is_finite())
            }) && self
                .stimulus(&self.player_stimulus)
                .is_some_and(|s| s.visible && !s.stimuli.is_empty())
                && self.stimulus(&self.sound_stimulus).is_some_and(|s| {
                    s.sound
                        && !s.check_audio_sources
                        && s.audio_sources.is_empty()
                        && !s.stimuli.is_empty()
                }),
            "stimulus sources",
        )
    }
}

impl RabbitReference {
    /// A node of the rabbit asset.
    pub fn local(&self, m: &ReferenceManifest, node: NodeId) -> bool {
        m.nodes
            .iter()
            .any(|n| n.id == node && n.asset == self.asset)
    }

    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            m.assets.iter().any(|a| {
                a.id == self.asset
                    && a.usage == AssetUsage::CreaturePrototype
                    && a.root == self.source
            }) && m
                .nodes
                .iter()
                .any(|n| n.id == self.root && n.source == self.source && n.parent.is_none()),
            "rabbit asset",
        )?;
        let s = &self.spawner;
        require(
            range(&s.time_range)
                && range(&s.count_range)
                && range(&s.distance_range)
                && s.max_count > 0
                && finite(&s.offset)
                && finite(&[s.chance, s.radius, s.half_fov])
                && s.radius > 0.,
            "rabbit spawner",
        )?;
        let b = &self.brain;
        require(
            finite(&[
                b.acceleration_death,
                b.jump_if_speed,
                b.jump_sense_distance,
                b.target_pre_calc_distance,
                b.target_distance_goal,
                b.target_angle_goal,
                b.target_angle_move,
                b.walk_distance,
                b.run_distance,
                b.attack_speed,
                b.flee_distance,
                b.min_flee_speed,
                b.flee_time,
                b.walk_speed_modifier,
                b.move_towards_target_modifier,
                b.move_forward_modifier,
                b.invert_flight_if_above,
            ]) && finite(&b.roam_x)
                && finite(&b.roam_z)
                && range(&b.roam_time)
                && b.preferences.iter().all(|p| p.value.is_finite()),
            "rabbit brain",
        )?;
        let mo = &self.motion;
        require(
            finite(&[
                mo.acceleration,
                mo.speed,
                mo.rot_speed,
                mo.jump_force,
                mo.grounded_ray_distance,
            ]) && mo.acceleration > 0.,
            "rabbit motion",
        )?;
        let se = &self.senses;
        require(
            se.scan_interval > 0.
                && finite(&se.fov)
                && finite(&[
                    se.hear_distance,
                    se.min_hear_distance,
                    se.min_hear_value,
                    se.hear_modifier,
                ])
                && self.local(m, se.eyes)
                && se.eye_objects.iter().all(|n| self.local(m, *n))
                && se.rays.iter().all(|r| {
                    finite(&r.direction)
                        && finite(&[r.length, r.max_value, r.spread])
                        && r.length > 0.
                }),
            "rabbit senses",
        )?;
        let z = &self.size;
        require(
            [
                z.scale,
                z.mass,
                z.health,
                z.shoot_health,
                z.speed,
                z.rot_speed,
            ]
            .iter()
            .all(|r| finite(r) && r[0] >= 0. && r[1] >= 0.)
                && z.scale[0] > 0.
                && z.run_lerp.is_finite()
                && z.eyes.iter().all(|n| self.local(m, *n)),
            "rabbit size",
        )?;
        require(
            self.damage.children.len() == self.damage.rules.children.len()
                && self.damage.children.iter().all(|c| {
                    self.local(m, c.node)
                        && finite(&[c.lose_eye_chance, c.lose_ear_chance, c.lose_sense_force])
                })
                && self
                    .damage
                    .rules
                    .children
                    .iter()
                    .all(|c| c.modifier.is_finite()),
            "rabbit damage",
        )?;
        require(
            self.body.mass_kg > 0.
                && !self.body.colliders.is_empty()
                && self.body.colliders.iter().all(|c| self.local(m, c.node)),
            "rabbit body",
        )?;
        let r = &self.ragdoll;
        let bodies = r.bodies.len() as u32;
        require(
            !r.bodies.is_empty()
                && r.pickups.len() == r.bodies.len()
                && r.bodies.iter().enumerate().all(|(i, b)| {
                    b.id.0 == i as u32 + 1 && self.local(m, b.node) && b.mass_kg > 0.
                })
                && r.colliders
                    .iter()
                    .all(|c| (1..=bodies).contains(&c.body.0) && self.local(m, c.node))
                && r.joints.iter().all(|j| {
                    (1..=bodies).contains(&j.body.0)
                        && (1..=bodies).contains(&j.connected_body.0)
                        && j.body != j.connected_body
                }),
            "rabbit ragdoll",
        )?;
        let a = &self.animator;
        require(
            a.clips.len() >= 2
                && a.thresholds.len() == a.clips.len()
                && finite(&a.thresholds)
                && a.thresholds.windows(2).all(|w| w[0] < w[1])
                && a.overlay.weight > 0.
                && a.overlay.weight <= 1.
                && !a.overlay.clip.tracks.is_empty()
                && a.clips.iter().chain([&a.overlay.clip]).all(|c| {
                    c.r#loop
                        && c.start_time < c.stop_time
                        && c.tracks
                            .iter()
                            .all(|t| self.local(m, NodeId(t.node)) && t.keys_valid(c.stop_time))
                }),
            "rabbit blend tree",
        )?;
        require(
            [&a.actions.roar, &a.actions.attack].iter().all(|s| {
                s.speed > 0.
                    && s.clip.start_time < s.clip.stop_time
                    && s.clip
                        .tracks
                        .iter()
                        .all(|t| self.local(m, NodeId(t.node)) && t.keys_valid(s.clip.stop_time))
                    && [s.enter, s.leave].iter().all(|t| {
                        t.duration >= 0.
                            && t.offset.is_finite()
                            && t.exit_time.is_none_or(|e| (0. ..=1.).contains(&e))
                    })
                    && s.leave.exit_time.is_some()
            }),
            "rabbit actions",
        )?;
        let e = &self.evil;
        let clips = m.audio["clips"].as_array();
        let clip = |id: &u32| {
            clips.is_some_and(|c| c.iter().any(|c| c["id"].as_u64() == Some(u64::from(*id))))
        };
        require(
            finite(&[
                e.attack_angle,
                e.attack_distance,
                e.attack_cooldown,
                e.attack_modifier,
                e.target_forget_time,
                e.idle_distance,
                e.roar_pitch,
                e.trigger.damage_player,
                e.trigger.damage_breakable,
            ]) && e.attack_duration > 0.
                && e.roar_duration > 0.
                && range(&e.idle_time)
                && [&e.roar_clips, &e.idle_clips, &e.attack_clips]
                    .iter()
                    .all(|c| !c.is_empty() && c.iter().all(clip))
                && [&e.eye_materials.evil, &e.eye_materials.green]
                    .iter()
                    .all(|l| l.asset == self.asset)
                && e.trigger.collider.sensor
                && self.local(m, e.trigger.collider.node),
            "rabbit evil",
        )?;
        require(
            !self.die_clips.is_empty()
                && self.die_clips.iter().all(|id| {
                    clips.is_some_and(|c| c.iter().any(|c| c["id"].as_u64() == Some(*id as u64)))
                })
                && m.audio["car_voices"].as_array().is_some_and(|v| {
                    v.iter().any(|v| {
                        v["id"].as_u64() == Some(u64::from(self.die_voice)) && v["one_shot"] == true
                    })
                }),
            "rabbit die sounds",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires exported Steam assets"]
    fn creatures_reference_loads() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .unwrap();
        let m = ReferenceManifest::from_runtime_json(&bytes).unwrap();
        let r = &m.creatures.rabbit;
        assert_eq!(r.item_id, 103);
        assert_eq!(r.spawner.max_count, 10);
        assert_eq!(r.ragdoll.bodies.len(), 14);
        assert_eq!(r.animator.parameter, "RunValue");
        assert!(m.creatures.stimuli.iter().any(|s| s.visible));
    }
}
