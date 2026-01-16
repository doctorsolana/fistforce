//! Client-side NPC visuals, animation, and debug rendering.

use bevy::animation::{AnimationClip, AnimationPlayer, AnimationTarget, AnimationTargetId};
use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::prelude::*;

use shared::{
    npc_capsule_endpoints, npc_head_center, Npc, NpcArchetype, NpcPosition, NpcRotation, Health,
    NPC_HEAD_RADIUS, NPC_HEIGHT, NPC_RADIUS,
};

use shared::WeaponDebugMode;

// =============================================================================
// ASSETS
// =============================================================================

#[derive(Resource, Clone)]
pub struct KayKitNpcAssets {
    pub scenes: std::collections::HashMap<NpcArchetype, Handle<Scene>>,
    pub animation_graph: Handle<AnimationGraph>,
    // Movement animations
    pub idle_node: AnimationNodeIndex,
    pub walk_node: AnimationNodeIndex,
    pub run_node: AnimationNodeIndex,
    pub walk_back_node: AnimationNodeIndex,
    pub strafe_left_node: AnimationNodeIndex,
    pub strafe_right_node: AnimationNodeIndex,
    // Death
    pub death_node: AnimationNodeIndex,
}

pub fn setup_npc_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut animation_graphs: ResMut<Assets<AnimationGraph>>,
) {
    let mut scenes = std::collections::HashMap::new();

    let mut load_scene = |arch: NpcArchetype, name: &str| {
        let path = format!("characters/adventurers/{name}.glb#Scene0");
        scenes.insert(arch, asset_server.load(path));
    };

    load_scene(NpcArchetype::Barbarian, "Barbarian");
    load_scene(NpcArchetype::Ranger, "Ranger");
    load_scene(NpcArchetype::Mage, "Mage");
    load_scene(NpcArchetype::Knight, "Knight");
    load_scene(NpcArchetype::Rogue, "Rogue");
    load_scene(NpcArchetype::RogueHooded, "Rogue_Hooded");

    // Movement animations from MovementBasic.glb
    // Idle_A = #6 (General), Walking_A = #8, Running_A = #5
    let idle_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_General.glb#Animation6",
    );
    let walk_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementBasic.glb#Animation8",
    );
    let run_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementBasic.glb#Animation5",
    );

    // Advanced movement from MovementAdvanced.glb
    // Walking_Backwards = #12, Running_Strafe_Left = #8, Running_Strafe_Right = #9
    let walk_back_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementAdvanced.glb#Animation12",
    );
    let strafe_left_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementAdvanced.glb#Animation8",
    );
    let strafe_right_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementAdvanced.glb#Animation9",
    );

    // Death animation
    let death_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_General.glb#Animation0",
    );

    // Build graph with all movement animations + death
    // Order: idle, walk, run, walk_back, strafe_left, strafe_right, death
    let (graph, nodes) = AnimationGraph::from_clips([
        idle_clip,
        walk_clip,
        run_clip,
        walk_back_clip,
        strafe_left_clip,
        strafe_right_clip,
        death_clip,
    ]);
    let animation_graph = animation_graphs.add(graph);

    commands.insert_resource(KayKitNpcAssets {
        scenes,
        animation_graph,
        idle_node: nodes[0],
        walk_node: nodes[1],
        run_node: nodes[2],
        walk_back_node: nodes[3],
        strafe_left_node: nodes[4],
        strafe_right_node: nodes[5],
        death_node: nodes[6],
    });

    info!("Loaded KayKit NPC assets (scenes + 7 animation clips)");
}

// =============================================================================
// SPAWNING
// =============================================================================

#[derive(Component)]
pub struct NpcModelRoot;

#[derive(Component)]
pub struct NeedsNpcRigSetup;

#[derive(Component)]
pub struct NpcAnimationRoot;

/// The NPC entity that owns this rig (cached to avoid per-frame hierarchy walks).
#[derive(Component, Clone, Copy)]
pub struct NpcRigOwner(pub Entity);

/// Movement animation types for NPCs
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NpcMovementAnim {
    #[default]
    Idle,
    Walk,
    Run,
    WalkBack,
    StrafeLeft,
    StrafeRight,
}

/// Duration for crossfade blending between animations
const NPC_ANIM_BLEND_DURATION: f32 = 0.2;

/// Speed smoothing factor (lower = smoother but slower response)
const NPC_SPEED_SMOOTHING: f32 = 8.0;

/// Hysteresis margins to prevent animation flip-flopping at thresholds
const HYSTERESIS_MARGIN: f32 = 0.3;

/// Tracks NPC animation state with blending support
#[derive(Component, Default)]
pub struct NpcAnimState {
    /// Currently playing animation
    pub current_anim: NpcMovementAnim,
    /// Target animation (for blending)
    pub target_anim: NpcMovementAnim,
    /// Blend progress (0.0 = current, 1.0 = target)
    pub blend_progress: f32,
    /// Is dead (death animation playing)
    pub dead: bool,
    /// Has been initialized (for motion detection)
    pub initialized: bool,
    /// Last position (for motion-based animation detection)
    pub last_pos: Vec3,
    /// Smoothed speed (exponential moving average to prevent jitter)
    pub smoothed_speed: f32,
}

/// Add render components and spawn the visual model when an NPC replicates in.
pub fn handle_npc_spawned(
    mut commands: Commands,
    assets: Option<Res<KayKitNpcAssets>>,
    new_npcs: Query<(Entity, &Npc, &NpcPosition), Added<Npc>>,
) {
    let Some(assets) = assets else { return };

    for (entity, npc, pos) in new_npcs.iter() {
        // Ensure NPC entity has full spatial components for hierarchy propagation.
        // Without GlobalTransform, children with GlobalTransform trigger B0004 warnings.
        commands.entity(entity).insert((
            Transform::from_translation(pos.0),
            GlobalTransform::from_translation(pos.0),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));

        let Some(scene) = assets.scenes.get(&npc.archetype).cloned() else {
            warn!("No KayKit scene for NPC archetype {:?}; skipping model spawn", npc.archetype);
            continue;
        };

        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                NpcModelRoot,
                NeedsNpcRigSetup,
                SceneRoot(scene),
                // NPC transform is the capsule center; drop model so feet touch ground.
                Transform::from_xyz(0.0, -NPC_HEIGHT * 0.5, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::splat(1.0)),
                GlobalTransform::default(),
                Visibility::Inherited,
                InheritedVisibility::default(),
            ));
        });
    }
}

/// Marker for NPC models with non-KayKit rigs that don't have animations set up.
#[derive(Component)]
pub struct CustomNpcModel;

/// Add `AnimationPlayer` + `AnimationTarget`s to the spawned NPC hierarchy.
/// Models with "Rig_Medium" armature (all KayKit + Doctor/GarbageMan) get full animation support.
/// Models with "Armature" are treated as static (no animations - fallback for incompatible rigs).
pub fn setup_npc_rig(
    mut commands: Commands,
    assets: Option<Res<KayKitNpcAssets>>,
    model_roots: Query<Entity, (With<NpcModelRoot>, With<NeedsNpcRigSetup>)>,
    children_q: Query<&Children>,
    names_q: Query<&Name>,
    parents_q: Query<&ChildOf>,
) {
    let Some(assets) = assets else { return };

    for model_root in model_roots.iter() {
        // Cache the owning NPC entity (parent of NpcModelRoot).
        let Ok(owner) = parents_q.get(model_root).map(|p| p.parent()) else {
            continue;
        };

        // Find the rig root node inside the spawned scene.
        // KayKit models use "Rig_Medium", custom models use "Armature".
        let mut stack: Vec<Entity> = vec![model_root];
        let mut rig_root: Option<Entity> = None;
        let mut is_custom_model = false;

        while let Some(e) = stack.pop() {
            if let Ok(name) = names_q.get(e) {
                if name.as_str() == "Rig_Medium" {
                    rig_root = Some(e);
                    break;
                }
                if name.as_str() == "Armature" {
                    // Fallback for non-Rig_Medium models - mark as static (no animations)
                    rig_root = Some(e);
                    is_custom_model = true;
                    break;
                }
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter());
            }
        }

        let Some(rig_root) = rig_root else {
            // Scene not spawned yet.
            continue;
        };

        // For custom models, just mark them as set up (no animations)
        if is_custom_model {
            commands.entity(rig_root).insert((
                CustomNpcModel,
                NpcRigOwner(owner),
            ));
            commands.entity(model_root).remove::<NeedsNpcRigSetup>();
            continue;
        }

        commands.entity(rig_root).insert((
            NpcAnimationRoot,
            NpcRigOwner(owner),
            NpcAnimState::default(),
            AnimationPlayer::default(),
            AnimationGraphHandle(assets.animation_graph.clone()),
        ));

        // Generate AnimationTargets for the entire rig hierarchy.
        let Ok(root_name) = names_q.get(rig_root) else {
            commands.entity(model_root).remove::<NeedsNpcRigSetup>();
            continue;
        };

        let mut stack: Vec<(Entity, Vec<Name>)> = vec![(rig_root, vec![root_name.clone()])];
        while let Some((e, path)) = stack.pop() {
            commands.entity(e).insert(AnimationTarget {
                id: AnimationTargetId::from_names(path.iter()),
                player: rig_root,
            });

            if let Ok(children) = children_q.get(e) {
                for child in children.iter() {
                    let mut child_path = path.clone();
                    if let Ok(child_name) = names_q.get(child) {
                        child_path.push(child_name.clone());
                    }
                    stack.push((child, child_path));
                }
            }
        }

        commands.entity(model_root).remove::<NeedsNpcRigSetup>();
    }
}

// =============================================================================
// TRANSFORM SYNC
// =============================================================================

/// Sync NPC transforms from replicated components.
pub fn sync_npc_transforms(
    time: Res<Time>,
    mut npcs: Query<(&NpcPosition, &NpcRotation, &mut Transform), With<Npc>>,
) {
    let dt = time.delta_secs();
    let pos_rate: f32 = 18.0;
    let rot_rate: f32 = 22.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();

    for (pos, rot, mut transform) in npcs.iter_mut() {
        transform.translation = transform.translation.lerp(pos.0, t_pos);
        let target_rot = Quat::from_rotation_y(rot.0);
        transform.rotation = transform.rotation.slerp(target_rot, t_rot);
    }
}

// =============================================================================
// ANIMATION
// =============================================================================

/// Helper to get the animation node for an NPC movement animation
fn npc_movement_anim_to_node(anim: NpcMovementAnim, assets: &KayKitNpcAssets) -> AnimationNodeIndex {
    match anim {
        NpcMovementAnim::Idle => assets.idle_node,
        NpcMovementAnim::Walk => assets.walk_node,
        NpcMovementAnim::Run => assets.run_node,
        NpcMovementAnim::WalkBack => assets.walk_back_node,
        NpcMovementAnim::StrafeLeft => assets.strafe_left_node,
        NpcMovementAnim::StrafeRight => assets.strafe_right_node,
    }
}

/// Determine target animation based on movement speed with hysteresis
/// The current_anim parameter enables hysteresis to prevent flip-flopping at thresholds
fn determine_npc_target_anim(speed_xz: f32, current_anim: NpcMovementAnim) -> NpcMovementAnim {
    // Thresholds with hysteresis:
    // - To START running, need speed > 4.5
    // - To STOP running, need speed < 4.5 - HYSTERESIS_MARGIN (4.2)
    // - To START walking, need speed > 0.15
    // - To STOP walking (go idle), need speed < 0.15 - margin (but clamped to ~0.05)

    match current_anim {
        NpcMovementAnim::Run => {
            // Currently running - need to slow down significantly to change
            if speed_xz < 4.5 - HYSTERESIS_MARGIN {
                if speed_xz > 0.15 {
                    NpcMovementAnim::Walk
                } else {
                    NpcMovementAnim::Idle
                }
            } else {
                NpcMovementAnim::Run
            }
        }
        NpcMovementAnim::Walk => {
            // Currently walking
            if speed_xz > 4.5 + HYSTERESIS_MARGIN {
                NpcMovementAnim::Run
            } else if speed_xz < 0.1 {
                // Lower threshold to go idle (hysteresis)
                NpcMovementAnim::Idle
            } else {
                NpcMovementAnim::Walk
            }
        }
        _ => {
            // Currently idle (or other) - need to exceed threshold to start moving
            if speed_xz > 4.5 + HYSTERESIS_MARGIN {
                NpcMovementAnim::Run
            } else if speed_xz > 0.2 {
                // Slightly higher threshold to start walking (hysteresis)
                NpcMovementAnim::Walk
            } else {
                NpcMovementAnim::Idle
            }
        }
    }
}

/// Drive NPC animations with motion-based directional movement and smooth blending:
/// - Animation selected based on movement speed
/// - Crossfade blending between animation states over 0.2 seconds
/// - Dead NPCs play death animation
pub fn update_npc_animation(
    assets: Option<Res<KayKitNpcAssets>>,
    time: Res<Time>,
    npc_roots: Query<(&Npc, &Health, &Transform)>,
    mut anim_roots: Query<(&NpcRigOwner, &mut NpcAnimState, &mut AnimationPlayer), With<NpcAnimationRoot>>,
) {
    let Some(assets) = assets else { return };

    let dt = time.delta_secs().max(1e-6);

    for (owner, mut state, mut player) in anim_roots.iter_mut() {
        let Ok((_npc, health, transform)) = npc_roots.get(owner.0) else {
            continue;
        };

        let npc_pos = transform.translation;
        let is_dead = health.is_dead();

        // Handle death animation
        if is_dead && !state.dead {
            player.stop_all();
            player.start(assets.death_node);
            state.dead = true;
            state.current_anim = NpcMovementAnim::Idle;
            state.target_anim = NpcMovementAnim::Idle;
            state.blend_progress = 1.0;
            continue;
        }

        // If dead, keep death animation (don't switch back)
        if state.dead {
            // Check if NPC respawned (health restored)
            if !is_dead {
                state.dead = false;
                player.stop_all();
                player.start(assets.idle_node).repeat();
                state.current_anim = NpcMovementAnim::Idle;
                state.target_anim = NpcMovementAnim::Idle;
                state.blend_progress = 1.0;
            }
            continue;
        }

        // Motion-based animation detection with speed smoothing
        let instant_speed = if state.initialized {
            let d = npc_pos - state.last_pos;
            state.last_pos = npc_pos;
            Vec2::new(d.x, d.z).length() / dt
        } else {
            state.initialized = true;
            state.last_pos = npc_pos;
            0.0
        };

        // Exponential moving average for smooth speed (prevents jittery animation switching)
        let smooth_factor = 1.0 - (-NPC_SPEED_SMOOTHING * dt).exp();
        state.smoothed_speed = state.smoothed_speed + (instant_speed - state.smoothed_speed) * smooth_factor;

        // Use smoothed speed with hysteresis for stable animation selection
        let target_anim = determine_npc_target_anim(state.smoothed_speed, state.target_anim);

        // Check if we need to start a new transition
        if target_anim != state.target_anim {
            // If we were in the middle of a transition, snap to current target first
            if state.blend_progress < 1.0 {
                let old_current_node = npc_movement_anim_to_node(state.current_anim, &assets);
                player.stop(old_current_node);
                state.current_anim = state.target_anim;
            }

            // Start new transition
            state.target_anim = target_anim;
            state.blend_progress = 0.0;

            // Start target animation at weight 0
            let target_node = npc_movement_anim_to_node(target_anim, &assets);
            player.start(target_node).repeat().set_weight(0.0);
        }

        // Update blend progress
        if state.blend_progress < 1.0 {
            state.blend_progress = (state.blend_progress + dt / NPC_ANIM_BLEND_DURATION).min(1.0);

            let current_node = npc_movement_anim_to_node(state.current_anim, &assets);
            let target_node = npc_movement_anim_to_node(state.target_anim, &assets);

            // Apply weights for crossfade
            let current_weight = 1.0 - state.blend_progress;
            let target_weight = state.blend_progress;

            if let Some(anim) = player.animation_mut(current_node) {
                anim.set_weight(current_weight);
            }
            if let Some(anim) = player.animation_mut(target_node) {
                anim.set_weight(target_weight);
            }

            // Transition complete - stop old animation
            if state.blend_progress >= 1.0 {
                player.stop(current_node);
                state.current_anim = state.target_anim;
            }
        } else {
            // Ensure current animation is playing (important on first frame after rig setup)
            let current_node = npc_movement_anim_to_node(state.current_anim, &assets);
            if !player.is_playing_animation(current_node) {
                player.start(current_node).repeat();
            }
        }
    }
}

// =============================================================================
// DEBUG (F4)
// =============================================================================

pub fn debug_draw_npc_hitboxes(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    npcs: Query<(&Transform, &Health), With<Npc>>,
) {
    if !debug_mode.0 {
        return;
    }

    for (transform, health) in npcs.iter() {
        let center = transform.translation;

        let head_center = npc_head_center(center);
        let (a, b) = npc_capsule_endpoints(center);

        let alive = !health.is_dead();

        let body_color = if alive {
            Color::srgba(1.0, 0.85, 0.2, 0.9)
        } else {
            Color::srgba(0.6, 0.6, 0.6, 0.7)
        };
        let head_color = if alive {
            Color::srgba(1.0, 0.2, 0.2, 0.95)
        } else {
            Color::srgba(0.5, 0.2, 0.2, 0.7)
        };

        // Body capsule (approx): spheres at endpoints + line between.
        gizmos.sphere(Isometry3d::from_translation(a), NPC_RADIUS, body_color);
        gizmos.sphere(Isometry3d::from_translation(b), NPC_RADIUS, body_color);
        gizmos.line(a, b, body_color);

        // Head sphere.
        gizmos.sphere(Isometry3d::from_translation(head_center), NPC_HEAD_RADIUS, head_color);
    }
}
