//! Player character systems
//!
//! Handles KayKit Ranger model loading, animation, spawning, and transform sync.

use bevy::prelude::*;
use bevy::animation::{AnimationClip, AnimationPlayer, AnimationTarget, AnimationTargetId};
use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use lightyear::prelude::*;
use lightyear::prelude::client::Connected;
use shared::{Health, LocalPlayer, Player, PlayerPosition, PlayerRotation, Vehicle, VehicleDriver, PLAYER_HEIGHT};
use std::collections::HashMap;

use crate::input::{CameraMode, InputState};

// =============================================================================
// COMPONENTS & RESOURCES
// =============================================================================

/// Loaded KayKit Ranger assets (model + animations)
#[derive(Resource, Clone)]
pub struct RangerCharacterAssets {
    pub ranger_scene: Handle<Scene>,
    pub animation_graph: Handle<AnimationGraph>,
    // Movement animations
    pub idle_node: AnimationNodeIndex,
    pub walk_node: AnimationNodeIndex,
    pub run_node: AnimationNodeIndex,
    pub walk_back_node: AnimationNodeIndex,
    pub strafe_left_node: AnimationNodeIndex,
    pub strafe_right_node: AnimationNodeIndex,
    // Jump animations
    pub jump_start_node: AnimationNodeIndex,
    pub jump_air_node: AnimationNodeIndex,
    pub jump_land_node: AnimationNodeIndex,
    // Death
    pub death_node: AnimationNodeIndex,
}

/// The entity we spawn `SceneRoot` onto for the player character model.
#[derive(Component)]
pub struct RangerModelRoot;

/// We spawned the model, but the internal GLTF hierarchy might not be ready yet.
#[derive(Component)]
pub struct NeedsRangerRigSetup;

/// The `Rig_Medium` node inside the Ranger scene. We attach the `AnimationPlayer` here.
#[derive(Component)]
pub struct RangerAnimationRoot;

/// The Player entity that owns this rig (cached to avoid per-frame hierarchy walks).
#[derive(Component, Clone, Copy)]
pub struct RangerRigOwner(pub Entity);

/// Movement animation types
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MovementAnim {
    #[default]
    Idle,
    Walk,
    Run,
    WalkBack,
    StrafeLeft,
    StrafeRight,
    // Jump animations
    JumpStart,
    JumpAir,
    JumpLand,
}

/// Duration for crossfade blending between animations
const ANIM_BLEND_DURATION: f32 = 0.2;

/// Duration of jump start animation before transitioning to air
const JUMP_START_DURATION: f32 = 0.25;
/// Duration of land animation before returning to ground movement
const JUMP_LAND_DURATION: f32 = 0.2;
/// Vertical velocity threshold to detect landing (units/sec)
const LANDING_VELOCITY_THRESHOLD: f32 = 0.5;
/// Minimum airborne time before allowing landing detection
const MIN_AIRBORNE_TIME: f32 = 0.15;

/// Tracks player animation state with blending support
#[derive(Component, Default)]
pub struct RangerAnimState {
    /// Currently playing animation
    pub current_anim: MovementAnim,
    /// Target animation (for blending)
    pub target_anim: MovementAnim,
    /// Blend progress (0.0 = current, 1.0 = target)
    pub blend_progress: f32,
    /// Is dead (death animation playing)
    pub dead: bool,
    /// Has been initialized (for motion detection)
    pub initialized: bool,
    /// Last position (for motion-based animation detection)
    pub last_pos: Vec3,
    /// Last Y position (for vertical velocity detection)
    pub last_y: f32,
    /// Whether player is currently airborne
    pub airborne: bool,
    /// Timer for jump sequence phases (start -> air, land -> ground)
    pub jump_timer: f32,
    /// Was jump input pressed last frame (for edge detection)
    pub jump_pressed_last: bool,
    /// Smoothed horizontal speed (for stable remote player animations)
    pub smoothed_speed: f32,
}

/// Marker for the local player's model (used for visibility toggling)
#[derive(Component)]
pub struct LocalPlayerModel;

/// Tracks the last camera mode to detect changes
#[derive(Resource, Default)]
pub struct LastCameraMode(pub Option<CameraMode>);

// =============================================================================
// ASSET LOADING
// =============================================================================

// =============================================================================
// LOCAL PLAYER TAGGING (robust against timing/race conditions)
// =============================================================================

/// Ensure exactly one `Player` entity is tagged as `LocalPlayer`, based on our `LocalId`.
///
/// Why this exists:
/// - The first replicated `Player` can arrive while we're still in `GameState::Connecting`,
///   and any `Added<Player>` systems gated to `Playing` will miss it.
/// - On higher-latency links (e.g. Fly.io), component insertion order can vary; we want the
///   camera/terrain/UI to *always* converge on the correct local entity.
pub fn ensure_local_player_tag(
    mut commands: Commands,
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    players: Query<(Entity, &Player)>,
    existing_local: Query<Entity, With<LocalPlayer>>,
    children_q: Query<&Children>,
    model_roots: Query<Entity, With<RangerModelRoot>>,
    existing_local_models: Query<Entity, With<LocalPlayerModel>>,
    mut did_log: Local<bool>,
) {
    let Some(our_peer_id) = client_query.iter().next().map(|r| r.0) else {
        return;
    };

    // Find the player entity that belongs to us.
    let mut matches: Vec<Entity> = Vec::new();
    for (e, p) in players.iter() {
        if p.client_id == our_peer_id {
            matches.push(e);
        }
    }

    let Some(local_entity) = matches.first().copied() else {
        return;
    };

    if matches.len() > 1 && !*did_log {
        warn!(
            "Multiple Player entities matched our peer id {:?} (count={}); selecting the first. This usually indicates a reconnect/duplication issue.",
            our_peer_id,
            matches.len()
        );
        *did_log = true;
    }

    // Enforce exactly one LocalPlayer.
    for e in existing_local.iter() {
        if e != local_entity {
            commands.entity(e).remove::<LocalPlayer>();
        }
    }
    commands.entity(local_entity).insert(LocalPlayer);

    // Try to enforce exactly one LocalPlayerModel as well (used by visibility toggling).
    // Model might not exist yet if assets are still loading, so this is best-effort and
    // will converge once the child exists.
    let mut local_model_root: Option<Entity> = None;
    if let Ok(children) = children_q.get(local_entity) {
        for child in children.iter() {
            if model_roots.get(child).is_ok() {
                local_model_root = Some(child);
                break;
            }
        }
    }

    if let Some(model_root) = local_model_root {
        for e in existing_local_models.iter() {
            if e != model_root {
                commands.entity(e).remove::<LocalPlayerModel>();
            }
        }
        commands.entity(model_root).insert(LocalPlayerModel);
    }
}

/// Load the KayKit Ranger model + movement animations and build an `AnimationGraph`.
///
/// Notes:
/// - The character model (`Ranger.glb`) has a rig/skin but **no animations**.
/// - The animations live in the `Rig_Medium_*.glb` files.
/// - We attach the rig animations to the Ranger by generating `AnimationTarget` ids that match
///   the rig's bone names (they share the same `Rig_Medium` hierarchy).
pub fn setup_player_character_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut animation_graphs: ResMut<Assets<AnimationGraph>>,
) {
    // Ranger model (mesh + skin)
    let ranger_scene: Handle<Scene> = asset_server
        .load("characters/adventurers/Ranger.glb#Scene0");

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

    // Jump animations from MovementBasic.glb
    // Jump_Start = #4, Jump_Idle (air) = #2, Jump_Land = #3
    let jump_start_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementBasic.glb#Animation4",
    );
    let jump_air_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementBasic.glb#Animation2",
    );
    let jump_land_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_MovementBasic.glb#Animation3",
    );

    // Death animation
    let death_clip: Handle<AnimationClip> = asset_server.load(
        "characters/animations/Rig_Medium_General.glb#Animation0",
    );

    // Build graph with all movement animations + jump + death
    // Order: idle, walk, run, walk_back, strafe_left, strafe_right, jump_start, jump_air, jump_land, death
    let (graph, nodes) = AnimationGraph::from_clips([
        idle_clip,
        walk_clip,
        run_clip,
        walk_back_clip,
        strafe_left_clip,
        strafe_right_clip,
        jump_start_clip,
        jump_air_clip,
        jump_land_clip,
        death_clip,
    ]);
    let animation_graph = animation_graphs.add(graph);

    commands.insert_resource(RangerCharacterAssets {
        ranger_scene,
        animation_graph,
        idle_node: nodes[0],
        walk_node: nodes[1],
        run_node: nodes[2],
        walk_back_node: nodes[3],
        strafe_left_node: nodes[4],
        strafe_right_node: nodes[5],
        jump_start_node: nodes[6],
        jump_air_node: nodes[7],
        jump_land_node: nodes[8],
        death_node: nodes[9],
    });

    info!("Loaded KayKit Ranger character assets (model + 10 animation clips)");
}

// =============================================================================
// PLAYER SPAWNING
// =============================================================================

/// Handle player spawn visuals
pub fn handle_player_spawned(
    mut commands: Commands,
    ranger_assets: Option<Res<RangerCharacterAssets>>,
    // In Lightyear 0.25:
    // - `RemoteId` on the client entity refers to the SERVER
    // - `LocalId` refers to US (the local client peer id)
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    new_players: Query<(Entity, &Player, &PlayerPosition), Added<Player>>,
) {
    let Some(ranger_assets) = ranger_assets else {
        // Should exist (loaded at Startup), but don't crash if not.
        return;
    };

    // Get our peer ID from the connected client entity
    let our_peer_id = client_query.iter().next().map(|r| r.0);

    for (entity, player, position) in new_players.iter() {
        info!("Player spawned: {:?}", player.client_id);

        let is_local = our_peer_id.map(|id| player.client_id == id).unwrap_or(false);

        // The replicated player entity is server-authoritative; we only add visuals here.
        // IMPORTANT: Full spatial bundle (including GlobalTransform) to avoid B0004 warnings
        // when the model hierarchy is spawned as children.
        commands.entity(entity).insert((
            Transform::from_translation(position.0),
            GlobalTransform::from_translation(position.0),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));

        // Spawn KayKit Ranger model as a child (so it inherits transform + visibility).
        // NOTE: Our gameplay PlayerPosition is at the capsule center, so we offset the model down.
        let model_entity = commands.spawn((
                RangerModelRoot,
                NeedsRangerRigSetup,
                SceneRoot(ranger_assets.ranger_scene.clone()),
                // glTF models in Bevy default to +Z forward; our game treats -Z as forward.
                // Rotate 180 degrees so the character faces the correct direction.
                Transform::from_xyz(0.0, -PLAYER_HEIGHT * 0.5, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::splat(1.0)),
                GlobalTransform::default(),
                Visibility::Inherited,
                InheritedVisibility::default(),
        )).id();
        
        commands.entity(entity).add_child(model_entity);

        if is_local {
            commands.entity(entity).insert(LocalPlayer);
            // Mark the model as belonging to local player for shadow-only rendering
            commands.entity(model_entity).insert(LocalPlayerModel);
            info!("Local player spawned!");
        }
    }
}

// =============================================================================
// RIG SETUP & ANIMATION
// =============================================================================

/// Once the Ranger scene hierarchy is spawned, attach `AnimationPlayer` + `AnimationTarget`s
/// so we can drive KayKit rig animations (Idle/Walk) on the character.
pub fn setup_ranger_rig(
    mut commands: Commands,
    ranger_assets: Option<Res<RangerCharacterAssets>>,
    model_roots: Query<Entity, (With<RangerModelRoot>, With<NeedsRangerRigSetup>)>,
    children_q: Query<&Children>,
    names_q: Query<&Name>,
    parents_q: Query<&ChildOf>,
) {
    let Some(ranger_assets) = ranger_assets else { return };

    for model_root in model_roots.iter() {
        // Cache the owning player entity (parent of the RangerModelRoot).
        let Ok(owner) = parents_q.get(model_root).map(|p| p.parent()) else {
            continue;
        };

        // Find the rig root node inside the spawned scene (named "Rig_Medium" in KayKit).
        let mut stack: Vec<Entity> = vec![model_root];
        let mut rig_root: Option<Entity> = None;
        while let Some(e) = stack.pop() {
            if let Ok(name) = names_q.get(e) {
                if name.as_str() == "Rig_Medium" {
                    rig_root = Some(e);
                    break;
                }
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter());
            }
        }

        let Some(rig_root) = rig_root else {
            // Scene not spawned yet (assets still loading).
            continue;
        };

        // Attach animation player + graph to the rig root.
        commands.entity(rig_root).insert((
            RangerAnimationRoot,
            RangerRigOwner(owner),
            RangerAnimState::default(),
            AnimationPlayer::default(),
            AnimationGraphHandle(ranger_assets.animation_graph.clone()),
        ));

        // Generate AnimationTargets for the entire rig hierarchy.
        // This mirrors what the glTF loader would do if the model contained animations.
        let Ok(root_name) = names_q.get(rig_root) else {
            // Extremely unlikely for KayKit, but don't crash if missing.
            commands.entity(model_root).remove::<NeedsRangerRigSetup>();
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

        // Mark done so we don't redo the hierarchy walk every frame.
        commands.entity(model_root).remove::<NeedsRangerRigSetup>();
    }
}

/// Helper to get the animation node for a movement animation
fn movement_anim_to_node(anim: MovementAnim, assets: &RangerCharacterAssets) -> AnimationNodeIndex {
    match anim {
        MovementAnim::Idle => assets.idle_node,
        MovementAnim::Walk => assets.walk_node,
        MovementAnim::Run => assets.run_node,
        MovementAnim::WalkBack => assets.walk_back_node,
        MovementAnim::StrafeLeft => assets.strafe_left_node,
        MovementAnim::StrafeRight => assets.strafe_right_node,
        MovementAnim::JumpStart => assets.jump_start_node,
        MovementAnim::JumpAir => assets.jump_air_node,
        MovementAnim::JumpLand => assets.jump_land_node,
    }
}

/// Check if an animation is a jump animation (needs special sequencing)
fn is_jump_anim(anim: MovementAnim) -> bool {
    matches!(anim, MovementAnim::JumpStart | MovementAnim::JumpAir | MovementAnim::JumpLand)
}

/// Determine target animation based on input state (for local player)
fn determine_local_target_anim(input: &crate::input::InputState) -> MovementAnim {
    if input.in_vehicle {
        return MovementAnim::Idle;
    }

    let sprinting = input.shift; // Shift key for sprinting
    let forward = input.forward;
    let backward = input.backward;
    let left = input.left;
    let right = input.right;

    // Priority: forward/backward movement over pure strafing
    match (forward, backward, left, right) {
        // Forward movement (with or without diagonal)
        (true, false, _, _) => {
            if sprinting { MovementAnim::Run } else { MovementAnim::Walk }
        }
        // Pure backward movement
        (false, true, false, false) => MovementAnim::WalkBack,
        // Backward with strafe - still use walk back
        (false, true, _, _) => MovementAnim::WalkBack,
        // Pure strafe left
        (false, false, true, false) => MovementAnim::StrafeLeft,
        // Pure strafe right
        (false, false, false, true) => MovementAnim::StrafeRight,
        // No movement
        _ => MovementAnim::Idle,
    }
}

/// Determine target animation based on movement speed (for remote players)
fn determine_remote_target_anim(speed_xz: f32) -> MovementAnim {
    if speed_xz > 4.5 {
        MovementAnim::Run
    } else if speed_xz > 0.15 {
        MovementAnim::Walk
    } else {
        MovementAnim::Idle
    }
}

/// Drive the Ranger animations with directional movement, jumping, and smooth blending:
/// - Local player: animation based on WASD input direction, sprint, and jump states
/// - Remote players: animation based on movement speed and vertical velocity
/// - Jump sequence: JumpStart -> JumpAir (loop) -> JumpLand -> ground movement
/// - Crossfade blending between animation states over 0.2 seconds
/// - Dead players play death animation
pub fn update_ranger_animation(
    ranger_assets: Option<Res<RangerCharacterAssets>>,
    input_state: Res<crate::input::InputState>,
    time: Res<Time>,
    mut anim_roots: Query<(&RangerRigOwner, &mut RangerAnimState, &mut AnimationPlayer), With<RangerAnimationRoot>>,
    local_players: Query<(), With<LocalPlayer>>,
    players_with_health: Query<&Health, With<Player>>,
    player_transforms: Query<&Transform, With<Player>>,
) {
    let Some(ranger_assets) = ranger_assets else { return };
    let dt = time.delta_secs().max(1e-6);

    for (owner, mut state, mut player) in anim_roots.iter_mut() {
        // Check if this player is dead
        let is_dead = players_with_health
            .get(owner.0)
            .map(|h| h.is_dead())
            .unwrap_or(false);

        // Handle death animation
        if is_dead && !state.dead {
            player.stop_all();
            player.start(ranger_assets.death_node);
            state.dead = true;
            state.airborne = false;
            state.current_anim = MovementAnim::Idle;
            state.target_anim = MovementAnim::Idle;
            state.blend_progress = 1.0;
            continue;
        }

        // If dead, keep death animation (don't switch back)
        if state.dead {
            // Check if player respawned (health restored)
            if !is_dead {
                state.dead = false;
                state.airborne = false;
                player.stop_all();
                player.start(ranger_assets.idle_node).repeat();
                state.current_anim = MovementAnim::Idle;
                state.target_anim = MovementAnim::Idle;
                state.blend_progress = 1.0;
            }
            continue;
        }

        // Get current position and compute velocities
        let (vert_velocity, speed_xz) = if let Ok(transform) = player_transforms.get(owner.0) {
            let pos = transform.translation;
            if state.initialized {
                let delta = pos - state.last_pos;
                let vy = delta.y / dt;
                let raw_speed_xz = Vec2::new(delta.x, delta.z).length() / dt;

                // Smooth the horizontal speed for stable animation selection
                let smooth_rate = 8.0;
                state.smoothed_speed += (raw_speed_xz - state.smoothed_speed) * (1.0 - (-smooth_rate * dt).exp());

                state.last_pos = pos;
                state.last_y = pos.y;
                (vy, state.smoothed_speed)
            } else {
                state.initialized = true;
                state.last_pos = pos;
                state.last_y = pos.y;
                state.smoothed_speed = 0.0;
                (0.0, 0.0)
            }
        } else {
            (0.0, state.smoothed_speed)
        };

        let is_local = local_players.contains(owner.0);

        // Detect jump initiation (edge detection)
        let jump_just_pressed = is_local && input_state.jump && !state.jump_pressed_last;
        state.jump_pressed_last = input_state.jump;

        // Update jump timer
        state.jump_timer += dt;

        // Determine target animation with jump handling
        let target_anim = determine_target_anim_with_jump(
            &mut state,
            is_local,
            &input_state,
            vert_velocity,
            speed_xz,
            jump_just_pressed,
        );

        // Check if we need to start a new transition
        if target_anim != state.target_anim {
            // If we were in the middle of a transition, snap to current target first
            if state.blend_progress < 1.0 {
                let old_current_node = movement_anim_to_node(state.current_anim, &ranger_assets);
                player.stop(old_current_node);
                state.current_anim = state.target_anim;
            }

            // Start new transition
            state.target_anim = target_anim;
            state.blend_progress = 0.0;

            // Reset jump timer when entering a new jump phase
            if is_jump_anim(target_anim) {
                state.jump_timer = 0.0;
            }

            // Start target animation at weight 0
            // Jump animations don't loop (except JumpAir)
            let target_node = movement_anim_to_node(target_anim, &ranger_assets);
            match target_anim {
                MovementAnim::JumpStart | MovementAnim::JumpLand => {
                    player.start(target_node).set_weight(0.0);
                }
                MovementAnim::JumpAir => {
                    player.start(target_node).repeat().set_weight(0.0);
                }
                _ => {
                    player.start(target_node).repeat().set_weight(0.0);
                }
            }
        }

        // Update blend progress
        if state.blend_progress < 1.0 {
            state.blend_progress = (state.blend_progress + dt / ANIM_BLEND_DURATION).min(1.0);

            let current_node = movement_anim_to_node(state.current_anim, &ranger_assets);
            let target_node = movement_anim_to_node(state.target_anim, &ranger_assets);

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
            let current_node = movement_anim_to_node(state.current_anim, &ranger_assets);
            if !player.is_playing_animation(current_node) {
                match state.current_anim {
                    MovementAnim::JumpStart | MovementAnim::JumpLand => {
                        player.start(current_node);
                    }
                    _ => {
                        player.start(current_node).repeat();
                    }
                }
            }
        }
    }
}

/// Determine target animation with full jump sequence handling
fn determine_target_anim_with_jump(
    state: &mut RangerAnimState,
    is_local: bool,
    input_state: &crate::input::InputState,
    vert_velocity: f32,
    speed_xz: f32,
    jump_just_pressed: bool,
) -> MovementAnim {
    // If in vehicle, always idle (no jumping in vehicles)
    if is_local && input_state.in_vehicle {
        state.airborne = false;
        return MovementAnim::Idle;
    }

    // Handle jump state machine
    // IMPORTANT: Check target_anim, not current_anim!
    // During a blend transition, current_anim is the OLD animation we're coming FROM.
    // target_anim is what we're transitioning TO, which represents our actual state.
    match state.target_anim {
        // Currently in JumpStart - wait for duration then go to JumpAir
        MovementAnim::JumpStart => {
            if state.jump_timer >= JUMP_START_DURATION {
                state.airborne = true;
                return MovementAnim::JumpAir;
            }
            return MovementAnim::JumpStart;
        }

        // Currently in JumpAir - wait for landing detection
        MovementAnim::JumpAir => {
            // Detect landing: vertical velocity near zero or positive after being negative,
            // and we've been airborne for minimum time
            if state.jump_timer >= MIN_AIRBORNE_TIME && vert_velocity.abs() < LANDING_VELOCITY_THRESHOLD {
                return MovementAnim::JumpLand;
            }
            return MovementAnim::JumpAir;
        }

        // Currently in JumpLand - wait for duration then return to ground movement
        MovementAnim::JumpLand => {
            if state.jump_timer >= JUMP_LAND_DURATION {
                state.airborne = false;
                // Fall through to determine ground movement
            } else {
                return MovementAnim::JumpLand;
            }
        }

        // Not in a jump animation - check if we should start one
        _ => {
            // Local player: start jump on input
            if is_local && jump_just_pressed {
                return MovementAnim::JumpStart;
            }

            // Remote player: detect airborne state via vertical velocity
            if !is_local {
                // If significantly rising, they're jumping
                if vert_velocity > 2.0 {
                    state.airborne = true;
                    return MovementAnim::JumpAir;
                }
                // If airborne and now landing
                if state.airborne && vert_velocity.abs() < LANDING_VELOCITY_THRESHOLD {
                    state.airborne = false;
                    // Skip land animation for remote players (too brief to look good)
                }
            }
        }
    }

    // Ground movement animation
    if is_local {
        determine_local_target_anim(input_state)
    } else {
        // Motion-based animation for remote players using smoothed speed
        determine_remote_target_anim(speed_xz)
    }
}

// =============================================================================
// TRANSFORM SYNC
// =============================================================================

/// Sync player transforms (visibility is handled by update_local_player_visibility)
pub fn sync_player_transforms(
    time: Res<Time>,
    vehicles: Query<(&VehicleDriver, &Transform), (With<Vehicle>, Without<Player>)>,
    mut players: Query<
        (&Player, &PlayerPosition, &PlayerRotation, &mut Transform),
        Without<Vehicle>,
    >,
) {
    let dt = time.delta_secs();
    let pos_rate: f32 = 22.0;
    let rot_rate: f32 = 26.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();

    // Map: driver_id -> vehicle transform (already smoothed in `sync_vehicle_transforms`)
    let mut driver_to_vehicle: HashMap<u64, (Vec3, Quat)> = HashMap::new();
    for (driver, veh_transform) in vehicles.iter() {
        if let Some(driver_id) = driver.driver_id {
            driver_to_vehicle.insert(driver_id, (veh_transform.translation, veh_transform.rotation));
        }
    }

    for (player, position, rotation, mut transform) in players.iter_mut() {
        // If this player is driving a vehicle, attach their visual to the vehicle to eliminate
        // relative jitter between player and bike at high speed.
        if let Some((veh_pos, veh_rot)) = driver_to_vehicle.get(&peer_id_to_u64(player.client_id)) {
            // Seat offset: slightly above and forward on the speeder
            let seat_offset = *veh_rot * Vec3::new(0.0, 0.55, 0.15);
            let target_pos = *veh_pos + seat_offset;
            // SNAP directly to vehicle - no lerp needed, vehicle is already smoothed
            transform.translation = target_pos;
            transform.rotation = *veh_rot;
        } else {
            transform.translation = transform.translation.lerp(position.0, t_pos);
            let target_rot = Quat::from_rotation_y(rotation.0);
            transform.rotation = transform.rotation.slerp(target_rot, t_rot);
        }
        
    }
}

/// Update local player model visibility.
/// In first-person: hide the model so it doesn't block the view.
/// In third-person: show the model.
pub fn update_local_player_visibility(
    mut commands: Commands,
    input_state: Res<InputState>,
    mut last_mode: ResMut<LastCameraMode>,
    local_model: Query<Entity, With<LocalPlayerModel>>,
) {
    let Ok(model_root) = local_model.single() else {
        return;
    };
    
    // Only update when camera mode changes
    let current_mode = input_state.camera_mode;
    if last_mode.0 == Some(current_mode) {
        return;
    }
    last_mode.0 = Some(current_mode);
    
    // Set visibility based on camera mode
    let visibility = match current_mode {
        CameraMode::FirstPerson => Visibility::Hidden,
        CameraMode::ThirdPerson => Visibility::Inherited,
            };
    
    commands.entity(model_root).insert(visibility);
}

/// Helper to convert PeerId to u64
pub fn peer_id_to_u64(peer_id: PeerId) -> u64 {
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        PeerId::Entity(id) => id,
        PeerId::Raw(addr) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            addr.hash(&mut hasher);
            hasher.finish()
        },
        PeerId::Server => 0,
    }
}
