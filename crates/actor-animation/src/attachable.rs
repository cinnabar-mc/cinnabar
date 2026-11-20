//! Animated item models use the same compiled scripts, controllers and pose VM as actors.
use super::*;

mod preview;
mod selection;
pub(super) mod static_draw;
use preview::Preview;
type AttachableKey = (ActorLifetimeId, bool, bool, bool, u8);

// Every hand, perspective, worn-state and worn-slot key fits for all admitted owners.
const STATES_PER_OWNER: usize = 1 << 5;
const MAX_ATTACHABLE_STATES: usize = MAX_ANIMATED_ACTORS * STATES_PER_OWNER;

/// Native item-render inputs; duration values are ticks, not the actor VM's seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct AttachableAnimationInput<'a> {
    pub first_person: bool,
    /// Whether the owner belongs to the current game window.
    pub is_local_player: bool,
    pub off_hand: bool,
    /// Sampled owner main-hand scale, applied by equipment placement outside the item VM.
    pub held_item_scale: Option<f32>,
    /// A worn model has independent controller state from either held item.
    pub worn: bool,
    /// Armour slot index of a worn model, so each worn piece keeps its own state.
    pub worn_slot: u8,
    pub is_paperdoll: bool,
    /// Marks a UI draw even when its owner rig was sampled in the world.
    pub is_in_ui: bool,
    pub frame_alpha: f32,
    /// Elapsed render time for clip application; absent inputs use the actor timestep.
    pub delta_seconds: Option<f32>,
    /// Animation frame of the owner's main-hand item, visible to either rendered hand.
    pub animation_frame: u32,
    /// Owner's elapsed main-hand use ticks, also visible to offhand attachables.
    pub use_elapsed_ticks: Option<u32>,
    pub max_use_ticks: u32,
    pub hand_charged: bool,
    /// Both owner hands remain visible to item-name queries while one attachable is rendered.
    pub owner_main_hand: Option<&'a str>,
    pub owner_off_hand: Option<&'a str>,
    /// Additional owner variables copied before the item's scripts run.
    pub owner_variables: &'a [(&'a str, f32)],
}

impl AttachableAnimationInput<'_> {
    /// Index of the separately replicated body slot in worn-item query context.
    pub const BODY_SLOT: u8 = 4;

    /// Returns the model binding name assigned to the rendered equipment slot.
    pub fn item_slot(self) -> &'static str {
        item_slot(self.worn, self.worn_slot, self.off_hand)
    }

    /// Keeps owner item scaling independent of offhand and worn item models.
    pub fn main_hand_scale(self) -> f32 {
        if self.off_hand || self.worn {
            1.0
        } else {
            self.held_item_scale
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(1.0)
        }
    }

    /// Selects the rendered hand from the owner's first-person use frame.
    pub fn for_hand(self, off_hand: bool) -> Self {
        if off_hand {
            Self {
                off_hand,
                hand_charged: false,
                ..self
            }
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AttachableQueryContext {
    pub first_person: bool,
    pub off_hand: bool,
    pub worn: bool,
    pub worn_slot: u8,
    pub is_paperdoll: bool,
    pub frame_alpha: f32,
    pub delta_seconds: Option<f32>,
    pub animation_frame: u32,
    pub use_elapsed_ticks: Option<u32>,
    pub max_use_ticks: u32,
    pub owner_life_tick: u64,
}

impl AttachableQueryContext {
    /// Returns the model binding name assigned to the rendered equipment slot.
    pub(super) fn item_slot(self) -> &'static str {
        item_slot(self.worn, self.worn_slot, self.off_hand)
    }
}

/// Shares the slot context between query evaluation and owner-bone placement.
fn item_slot(worn: bool, worn_slot: u8, off_hand: bool) -> &'static str {
    if worn {
        match worn_slot {
            0 => "head",
            1 => "torso",
            AttachableAnimationInput::BODY_SLOT => "body",
            _ => "",
        }
    } else if off_hand {
        "off_hand"
    } else {
        "main_hand"
    }
}

/// Sets equipment-only context values after inheriting the owner's script variables.
pub(super) fn apply_context_variables(
    engine: &EngineSlots,
    variables: &mut MolangVariables,
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    input: &ActorTickInput,
) {
    if let Some(attachable) = context.attachable {
        variables.set_actor_reference(engine.context_owning_entity, actor.runtime_id());
        variables.set(
            engine.context_first_person,
            f32::from(attachable.first_person),
        );
        variables.set(engine.context_paperdoll, f32::from(attachable.is_paperdoll));
        variables.set_string(engine.context_item_slot, attachable.item_slot());
        variables.set(engine.attack_time, input.attack_time);
    }
}

/// Evaluated geometry and layers in the item's own model frame, before the owner bone.
#[derive(Clone, Copy, Debug)]
pub struct AttachableRigSnapshot<'a> {
    pub geometry: u32,
    pub pose: &'a [BoneTransform],
    pub bone_names: &'a [Box<str>],
    pub render: &'a [RenderTextureLayer],
    pub scale: f32,
    pub axis_scale: [f32; 3],
    bones: &'a [RuntimeBone],
    layer_skeletons: &'a BTreeMap<u32, Option<Arc<render::LayerSkeleton>>>,
}

/// The skeletal frame inherited by an attachable root and all its descendants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AttachableBoneParent<'a> {
    Actor,
    OwnerNamed(&'a str),
    BindingExpression,
}

impl<'a> AttachableRigSnapshot<'a> {
    /// Resolves the selected geometry's root without allocating or copying its pose.
    pub fn bone_parent(&self, geometry: u32, index: usize) -> Option<AttachableBoneParent<'a>> {
        let (bones, names) = if geometry == self.geometry {
            (self.bones, self.bone_names)
        } else {
            let skeleton = self.layer_skeletons.get(&geometry)?.as_ref()?;
            (skeleton.bones.as_slice(), skeleton.names.as_slice())
        };
        let mut root = index;
        for _ in 0..bones.len() {
            let bone = bones.get(root)?;
            if let Some(parent) = bone.parent {
                root = parent;
                continue;
            }
            return Some(match bone.attachable_root {
                AttachableRootFrame::Actor => AttachableBoneParent::Actor,
                AttachableRootFrame::MatchingOwnerName => {
                    AttachableBoneParent::OwnerNamed(names.get(root)?)
                }
                AttachableRootFrame::BindingExpression => AttachableBoneParent::BindingExpression,
            });
        }
        None
    }
}

/// World-only evaluator outcome. The zero-root sample is preliminary evidence, not a final
/// placement-level no-draw decision; consumers must validate both finite owner endpoints.
#[derive(Debug)]
pub enum WorldAttachableEvaluation<'a> {
    Drawable(AttachableRigSnapshot<'a>),
    AuthoredZeroRootSample(render::AuthoredZeroRootSample<'a>),
    Unavailable,
}

fn retained_authored_zero_root_sample<'a>(
    state: &'a AttachableState,
    assets: &RuntimeEntityAssets,
    binding: usize,
    owner: ActorLifetimeId,
    candidate: render::AuthoredZeroRootCandidate,
    drawable: Option<AttachableRigSnapshot<'a>>,
) -> Option<render::AuthoredZeroRootSample<'a>> {
    let root_name = state.rig.bone_names.first()?.as_ref();
    let root = state.rig.bones.first()?;
    let root_parent = match root.attachable_root {
        AttachableRootFrame::Actor => AttachableBoneParent::Actor,
        AttachableRootFrame::MatchingOwnerName => AttachableBoneParent::OwnerNamed(root_name),
        AttachableRootFrame::BindingExpression => AttachableBoneParent::BindingExpression,
    };
    let rig_scale = drawable.map_or_else(
        || {
            state
                .rig
                .scale
                .unwrap_or_else(|| [assets.rig_bindings()[binding].scale.get(), 1.0, 1.0, 1.0])
        },
        |snapshot| {
            [
                snapshot.scale,
                snapshot.axis_scale[0],
                snapshot.axis_scale[1],
                snapshot.axis_scale[2],
            ]
        },
    );
    Some(render::AuthoredZeroRootSample {
        proof: render::AuthoredZeroRootProof {
            layer: candidate.layer,
            geometry: candidate.geometry,
            effective_scale: candidate.effective_scale,
            root_transform: candidate.root_transform,
            rig_scale,
            root_parent,
            root_bone_name: root_name,
            owner,
            attachable_identifier: state.identifier.as_ref(),
        },
        drawable,
    })
}

#[derive(Debug)]
struct AttachableState {
    identifier: Arc<str>,
    rig: ActorRigState,
    last_used: u64,
    preview: Option<Preview>,
    static_draw: Option<static_draw::StaticDraw>,
}

/// Retained script/controller state per owner, hand and render perspective.
#[derive(Debug)]
pub struct AttachablesRuntime {
    assets: Arc<RuntimeEntityAssets>,
    layout: VariableLayout,
    states: BTreeMap<AttachableKey, AttachableState>,
    evaluations: u64,
    previewing: bool,
    previewed: Vec<AttachableKey>,
    selection_variables: MolangVariables,
    selection_stack: Vec<evaluation::MolangValue>,
    selections: selection::SelectionCache,
    #[cfg(test)]
    commits: u64,
}

impl AttachablesRuntime {
    pub fn new(assets: Arc<RuntimeEntityAssets>) -> Self {
        let layout = VariableLayout::new(&assets);
        let selection_variables = layout.fresh(1);
        Self {
            layout,
            selection_variables,
            selection_stack: Vec::new(),
            selections: selection::SelectionCache::default(),
            assets,
            states: BTreeMap::new(),
            evaluations: 0,
            previewing: false,
            previewed: Vec::new(),
            #[cfg(test)]
            commits: 0,
        }
    }

    /// Discards owner lifetimes when a session ends or its active asset catalog changes.
    pub fn clear(&mut self) {
        self.layout.plans.clear();
        self.states.clear();
        self.selections = selection::SelectionCache::default();
        self.previewed.clear();
        self.previewing = false;
    }

    /// Starts exact readiness draws while keeping their authored VM results uncommitted.
    pub fn begin_preview(&mut self) {
        self.finish_preview(false);
        self.previewing = true;
    }

    /// Leaves preview evaluation mode while retaining the bounded hand results for selection.
    pub fn end_preview(&mut self) {
        self.previewing = false;
    }

    /// Commits reused readiness results once, or restores discarded geometry before final drawing.
    pub fn finish_preview(&mut self, commit: bool) {
        self.previewing = false;
        for key in self.previewed.drain(..) {
            if let Some(state) = self.states.get_mut(&key) {
                if commit {
                    let committed = state.commit_preview();
                    #[cfg(test)]
                    {
                        self.commits += u64::from(committed);
                    }
                    #[cfg(not(test))]
                    let _ = committed;
                } else {
                    state.discard_preview();
                }
            }
        }
    }

    /// Evaluates the exact non-preview world main-hand path. The zero-root sample is
    /// preliminary only; consumers still must validate both finite owner endpoints.
    pub fn evaluate_world_main_hand(
        &mut self,
        identifier: &str,
        owner: &dyn AnimatedActor,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> WorldAttachableEvaluation<'_> {
        if self.previewing
            || input.first_person
            || input.off_hand
            || input.worn
            || input.is_paperdoll
            || input.is_in_ui
            || input.owner_main_hand != Some(identifier)
        {
            return WorldAttachableEvaluation::Unavailable;
        }
        let mut sample = None;
        let drawable =
            self.evaluate_inner(identifier, owner, owner_rig, input, true, Some(&mut sample));
        if let Some(sample) = sample {
            return WorldAttachableEvaluation::AuthoredZeroRootSample(sample);
        }
        match drawable {
            Some(drawable) => WorldAttachableEvaluation::Drawable(drawable),
            None => WorldAttachableEvaluation::Unavailable,
        }
    }

    /// Runs authored item scripts with owner queries and native render-time item inputs.
    /// The returned pose has already sampled `frame_alpha`; do not interpolate it again.
    pub fn evaluate(
        &mut self,
        identifier: &str,
        owner: &dyn AnimatedActor,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> Option<AttachableRigSnapshot<'_>> {
        self.evaluate_inner(identifier, owner, owner_rig, input, false, None)
    }

    fn evaluate_inner<'a>(
        &'a mut self,
        identifier: &str,
        owner: &dyn AnimatedActor,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        authored_no_draw: bool,
        authored_zero_root_sample: Option<&mut Option<render::AuthoredZeroRootSample<'a>>>,
    ) -> Option<AttachableRigSnapshot<'a>> {
        let key = (
            owner_rig.actor,
            input.off_hand,
            input.first_person,
            input.worn,
            if input.worn { input.worn_slot } else { 0 },
        );
        if self
            .states
            .first_key_value()
            .is_some_and(|((actor, ..), _)| actor.session_id != owner_rig.actor.session_id)
        {
            self.states.clear();
            self.previewed.clear();
        }
        if !self.states.contains_key(&key) {
            let first = ActorLifetimeId {
                spawn_revision: 0,
                ..owner_rig.actor
            };
            let last = ActorLifetimeId {
                spawn_revision: u64::MAX,
                ..owner_rig.actor
            };
            let mut stale = [None; STATES_PER_OWNER];
            for (slot, (&key, _)) in stale.iter_mut().zip(
                self.states
                    .range((first, false, false, false, 0)..=(last, true, true, true, u8::MAX)),
            ) {
                if key.0 != owner_rig.actor {
                    *slot = Some(key);
                }
            }
            for key in stale.into_iter().flatten() {
                self.states.remove(&key);
            }
        }
        let binding = self.assets.attachable_rig_binding(identifier)?;
        self.evaluations += 1;
        // Departed owners are never announced here; the least recently drawn state makes room.
        if !self.states.contains_key(&key)
            && self.states.len() >= MAX_ATTACHABLE_STATES
            && let Some(stale) = self
                .states
                .iter()
                .min_by_key(|(_, state)| state.last_used)
                .map(|(key, _)| *key)
        {
            self.states.remove(&stale);
        }
        if self
            .states
            .get(&key)
            .is_none_or(|state| state.identifier.as_ref() != identifier)
        {
            let rig = resolve_binding(
                &self.assets,
                &self.layout,
                owner,
                owner_rig.completed_tick,
                binding,
            )?;
            self.states.insert(
                key,
                AttachableState {
                    identifier: Arc::from(identifier),
                    rig,
                    last_used: 0,
                    preview: None,
                    static_draw: None,
                },
            );
        }
        let entry = self.states.get_mut(&key)?;
        entry.discard_preview();
        entry.last_used = self.evaluations;
        let static_input = static_draw::Inputs::new(owner, owner_rig, input);
        if !self.previewing
            && entry.static_draw.as_ref().is_some_and(|cached| {
                cached.input == static_input
                    && cached.owner_names == owner_rig.bone_names
                    && cached
                        .owner_layers
                        .iter()
                        .copied()
                        .eq(static_draw::owner_layer_models(owner_rig))
            })
        {
            entry.rig.completed_tick = owner_rig.completed_tick;
            return entry.snapshot(&self.assets, binding);
        }
        entry.static_draw = None;
        let state = &mut entry.rig;
        let mut context = selection::owner_context(owner_rig, input);
        if !input.worn && input.off_hand {
            context
                .off_hand
                .get_or_insert_with(|| Arc::from(identifier));
        } else if !input.worn {
            context
                .main_hand
                .get_or_insert_with(|| Arc::from(identifier));
        }
        state.history.clear();
        state
            .history
            .push_back(selection::owner_frame(owner, owner_rig, input));
        let allowance = molang_tick_budget(&self.assets, binding);
        let mut world_left = allowance;
        let mut budget = EvalBudget {
            actor_left: allowance,
            world_left: &mut world_left,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
            transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
            used: 0,
            stack: OperandStack::lease(),
            static_draw: (!self.previewing)
                .then_some(self.assets.rig_bindings()[binding].geometry_count == 1),
        };
        let geometry = if self.previewing {
            geometry::reselect_geometry_preview(
                &self.assets,
                &self.layout,
                state,
                owner,
                &context,
                &mut budget,
            )
        } else {
            geometry::reselect_geometry(
                &self.assets,
                &self.layout,
                state,
                owner,
                &context,
                &mut budget,
            );
            None
        };
        render::bind_attachable_roots(
            state,
            crate::attachment::OwnerSkeleton::body(&[], &[], owner_rig.bone_names)
                .with_layers(owner_rig),
        );
        let evaluated = match evaluate_state(
            &self.assets,
            &self.layout,
            state,
            owner,
            &context,
            owner_rig.completed_tick,
            &mut budget,
            true,
            Some(tick::EvaluationInheritance {
                variables: owner_rig.animation_variables,
                overrides: input.owner_variables,
                authored_no_draw,
            }),
        ) {
            Ok(evaluated) => evaluated,
            Err(_) => {
                if let Some(geometry) = geometry {
                    geometry.restore(state);
                }
                return None;
            }
        };
        let retain_static = budget.static_draw == Some(true)
            && evaluated.render.is_some()
            && evaluated.clip_clocks.is_empty()
            && static_draw::eligible(state, &evaluated.controllers);
        let candidate = (!self.previewing && authored_no_draw)
            .then_some(evaluated.authored_zero_root_sample)
            .flatten();
        if self.previewing {
            entry.preview = Some(Preview {
                evaluated,
                tick: owner_rig.completed_tick,
                geometry,
            });
            if !self.previewed.contains(&key) {
                self.previewed.push(key);
            }
        } else {
            let drawable = entry.commit(evaluated, owner_rig.completed_tick);
            #[cfg(test)]
            {
                self.commits += 1;
            }
            if retain_static {
                entry.static_draw = Some(static_draw::StaticDraw {
                    input: static_input,
                    owner_names: owner_rig.bone_names.to_vec(),
                    owner_layers: static_draw::owner_layer_models(owner_rig).collect(),
                });
            }
            if let (Some(candidate), Some(output)) = (candidate, authored_zero_root_sample) {
                let snapshot = drawable
                    .then(|| entry.snapshot(&self.assets, binding))
                    .flatten();
                if let Some(sample) = retained_authored_zero_root_sample(
                    entry,
                    &self.assets,
                    binding,
                    owner_rig.actor,
                    candidate,
                    snapshot,
                ) {
                    *output = Some(sample);
                    return None;
                }
            }
            if !drawable {
                return None;
            }
        }
        entry.snapshot(&self.assets, binding)
    }
}

/// Vanilla attachable setup distinguishes expression bindings from owner-name
/// matches. Only the latter clear the authored default TRS; animation restores the
/// former's bone defaults afterward.
pub(super) fn bind_roots(
    bones: &mut [RuntimeBone],
    names: &[Box<str>],
    owner: crate::attachment::OwnerSkeleton<'_>,
) {
    for (bone, name) in bones.iter_mut().zip(names) {
        bone.attachable_root = if bone.parent.is_some() {
            AttachableRootFrame::Actor
        } else if bone.has_binding_expression {
            AttachableRootFrame::BindingExpression
        } else if owner.has(name) {
            AttachableRootFrame::MatchingOwnerName
        } else {
            AttachableRootFrame::Actor
        };
    }
}

#[cfg(test)]
mod authored_no_draw_tests {
    use super::*;
    use crate::attachable::tests::owner_rig;
    use assets::{EntityAnimationProperty, EntityGeometryCube, EntityGeometryUv};

    fn owner_input() -> AttachableAnimationInput<'static> {
        AttachableAnimationInput {
            owner_main_hand: Some("minecraft:test_item"),
            ..Default::default()
        }
    }

    fn selected_root_primitive(compiled: &mut assets::CompiledEntityAssets) {
        compiled.geometries[0].bones[0].cubes = vec![EntityGeometryCube {
            origin: [crate::test_support::scalar(0.0); 3],
            size: [crate::test_support::scalar(1.0); 3],
            pivot: [crate::test_support::scalar(0.0); 3],
            rotation: [crate::test_support::scalar(0.0); 3],
            uv: EntityGeometryUv::Box([crate::test_support::scalar(0.0); 2]),
            inflate: crate::test_support::scalar(0.0),
            mirror: false,
        }]
        .into_boxed_slice();
        compiled.render.geometries = vec![assets::EntityRenderGeometry {
            condition: None,
            geometry: 0,
        }]
        .into_boxed_slice();
        compiled.render.layers[0].geometry_count = 1;
    }

    fn zero_scale_compiled() -> assets::CompiledEntityAssets {
        let mut compiled = crate::test_support::compiled_fixture();
        compiled.animation_channels[0].property = EntityAnimationProperty::Scale;
        compiled.animation_keyframes[0].value = [crate::test_support::scalar(0.0); 3];
        compiled.animation_keyframes[0].expressions = [None; 3];
        compiled.geometries[0].bones[0].pivot = Some([
            crate::test_support::scalar(2.0),
            crate::test_support::scalar(3.0),
            crate::test_support::scalar(4.0),
        ]);
        compiled.geometries[0].bones[0].rotation = Some([
            crate::test_support::scalar(10.0),
            crate::test_support::scalar(20.0),
            crate::test_support::scalar(30.0),
        ]);
        selected_root_primitive(&mut compiled);
        compiled
    }

    fn zero_scale_assets() -> Arc<RuntimeEntityAssets> {
        Arc::new(RuntimeEntityAssets::from_compiled(zero_scale_compiled()).unwrap())
    }

    #[test]
    fn world_main_hand_returns_same_sample_authored_zero_root_before_missing_artwork() {
        let owner = crate::tests::actor_with_metadata(HashMap::new());
        let assets = zero_scale_assets();
        let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
        let owner_snapshot = owner_rig();
        let outcome = runtime.evaluate_world_main_hand(
            "minecraft:test_item",
            &owner,
            &owner_snapshot,
            owner_input(),
        );
        let WorldAttachableEvaluation::AuthoredZeroRootSample(sample) = outcome else {
            panic!("single active zero-root authored draw should yield preliminary evidence");
        };
        let drawable = sample
            .drawable
            .as_ref()
            .expect("resolved artwork snapshot is retained");
        assert_eq!(sample.proof.effective_scale, [0.0; 3]);
        assert_eq!(sample.proof.geometry, drawable.geometry);
        assert_eq!(sample.proof.root_transform, drawable.pose[0]);
        assert_eq!(
            sample.proof.rig_scale,
            [
                drawable.scale,
                drawable.axis_scale[0],
                drawable.axis_scale[1],
                drawable.axis_scale[2]
            ]
        );
        assert_eq!(sample.proof.root_bone_name, "rightitem");
        assert_eq!(sample.proof.root_parent, AttachableBoneParent::Actor);
        assert_eq!(sample.proof.owner, owner_snapshot.actor);
        assert_eq!(sample.proof.attachable_identifier, "minecraft:test_item");
        let root = sample.proof.root_transform;
        assert!(
            root.rotation
                .into_iter()
                .chain(root.translation_scale)
                .chain(root.axis_scale)
                .all(f32::is_finite)
        );
        assert!(
            root.translation_scale[..3]
                .iter()
                .any(|value| *value != 0.0)
        );
        assert!(root.rotation[..3].iter().any(|value| *value != 0.0));
        assert_eq!(runtime.evaluations, 1, "proof reuses the single evaluation");
        assert!(
            runtime
                .evaluate("minecraft:test_item", &owner, &owner_rig(), owner_input(),)
                .is_some(),
            "legacy evaluation remains drawable and unchanged"
        );
    }

    #[test]
    fn zero_pose_with_missing_artwork_keeps_evidence_without_drawable() {
        let owner = crate::tests::actor_with_metadata(HashMap::new());
        let mut compiled = zero_scale_compiled();
        compiled.render.layers[0].unresolved_textures = true;
        compiled.render.layers[0].slot_count = 0;
        compiled.render.slots = Box::new([]);
        compiled.render.candidates = Box::new([]);
        let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
        let mut runtime = AttachablesRuntime::new(assets);
        let rig = owner_rig();
        let outcome =
            runtime.evaluate_world_main_hand("minecraft:test_item", &owner, &rig, owner_input());
        let WorldAttachableEvaluation::AuthoredZeroRootSample(sample) = outcome else {
            panic!("valid zero pose must survive artwork rejection");
        };
        assert!(sample.drawable.is_none());
        assert_eq!(sample.proof.effective_scale, [0.0; 3]);
        let root = sample.proof.root_transform;
        assert!(
            root.rotation
                .into_iter()
                .chain(root.translation_scale)
                .chain(root.axis_scale)
                .all(f32::is_finite)
        );
        assert!(
            root.translation_scale[..3]
                .iter()
                .any(|value| *value != 0.0)
        );
        assert!(root.rotation[..3].iter().any(|value| *value != 0.0));
        assert_eq!(sample.proof.owner, rig.actor);
        assert_eq!(sample.proof.attachable_identifier, "minecraft:test_item");
    }

    #[test]
    fn nonzero_pose_with_missing_artwork_is_unavailable() {
        let owner = crate::tests::actor_with_metadata(HashMap::new());
        let mut compiled = crate::test_support::compiled_fixture();
        selected_root_primitive(&mut compiled);
        compiled.render.layers[0].unresolved_textures = true;
        compiled.render.layers[0].slot_count = 0;
        compiled.render.slots = Box::new([]);
        compiled.render.candidates = Box::new([]);
        let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
        let mut runtime = AttachablesRuntime::new(assets);
        assert!(matches!(
            runtime.evaluate_world_main_hand(
                "minecraft:test_item",
                &owner,
                &owner_rig(),
                owner_input(),
            ),
            WorldAttachableEvaluation::Unavailable
        ));
        assert!(
            runtime
                .evaluate("minecraft:test_item", &owner, &owner_rig(), owner_input(),)
                .is_none(),
            "legacy evaluate preserves unresolved-artwork fallback behavior"
        );
    }

    #[test]
    fn second_active_layer_makes_single_draw_proof_ineligible() {
        let owner = crate::tests::actor_with_metadata(HashMap::new());
        let mut compiled = crate::test_support::compiled_fixture();
        compiled.animation_channels[0].property = EntityAnimationProperty::Scale;
        compiled.animation_keyframes[0].value = [crate::test_support::scalar(0.0); 3];
        compiled.animation_keyframes[0].expressions = [None; 3];
        selected_root_primitive(&mut compiled);
        let mut duplicate = compiled.render.layers[0];
        let mut slots = compiled.render.slots.to_vec();
        let mut candidate = compiled.render.candidates[0];
        candidate.source = 4;
        let mut duplicate_slot = compiled.render.slots[0];
        duplicate_slot.first_candidate = compiled.render.candidates.len() as u32;
        duplicate.first_slot = slots.len() as u32;
        let mut candidates = compiled.render.candidates.to_vec();
        candidates.push(candidate);
        slots.push(duplicate_slot);
        compiled.render.candidates = candidates.into_boxed_slice();
        compiled.render.slots = slots.into_boxed_slice();
        let mut geometries = compiled.render.geometries.to_vec();
        duplicate.first_geometry = geometries.len() as u32;
        geometries.push(geometries[0]);
        compiled.render.geometries = geometries.into_boxed_slice();
        let mut layers = compiled.render.layers.to_vec();
        layers.push(duplicate);
        compiled.render.layers = layers.into_boxed_slice();
        let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
        let mut runtime = AttachablesRuntime::new(assets);
        assert!(!matches!(
            runtime.evaluate_world_main_hand(
                "minecraft:test_item",
                &owner,
                &owner_rig(),
                owner_input(),
            ),
            WorldAttachableEvaluation::AuthoredZeroRootSample(_)
        ));
    }

    #[test]
    fn absent_active_layer_never_proves_authored_no_draw() {
        let owner = crate::tests::actor_with_metadata(HashMap::new());
        let mut compiled = crate::test_support::compiled_fixture();
        compiled.render.layers = Box::new([]);
        compiled.render.slots = Box::new([]);
        compiled.render.candidates = Box::new([]);
        compiled.render.geometries = Box::new([]);
        let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
        let mut runtime = AttachablesRuntime::new(assets);
        assert!(!matches!(
            runtime.evaluate_world_main_hand(
                "minecraft:test_item",
                &owner,
                &owner_rig(),
                owner_input(),
            ),
            WorldAttachableEvaluation::AuthoredZeroRootSample(_)
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
#[path = "attachable/preview_tests.rs"]
mod preview_tests;

#[cfg(test)]
#[path = "attachable/owner_reference_tests.rs"]
mod owner_reference_tests;

#[cfg(test)]
#[path = "attachable/owner_context_tests.rs"]
mod owner_context_tests;

#[cfg(test)]
#[path = "attachable/activation_clock_tests.rs"]
mod activation_clock_tests;

#[cfg(test)]
#[path = "attachable/parent_frame_tests.rs"]
mod parent_frame_tests;

#[cfg(test)]
mod spear_tests;

#[cfg(test)]
#[path = "attachable/selection_tests.rs"]
mod review_tests;
