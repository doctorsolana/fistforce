//! Player character systems
//!
//! Handles KayKit Ranger model loading, animation, spawning, and transform sync.

use bevy::prelude::*;
use bevy::animation::{AnimationClip, AnimationPlayer, AnimationTarget, AnimationTargetId};
use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use lightyear::prelude::*;
use lightyear::prelude::client::Connected;
use shared::{LocalPlayer, Player, PlayerPosition, PlayerRotation, Vehicle, VehicleDriver, PLAYER_HEIGHT};
use std::collections::HashMap;

use crate::input::CameraMode;

// =============================================================================
// COMPONENTS & RESOURCES
// =============================================================================

/// Loaded KayKit Ranger assets (model + animations)
#[derive(Resource, Clone)]
pub struct RangerCharacterAssets {
    pub ranger_scene: Handle<Scene>,
    pub animation_graph: Handle<AnimationGraph>,
    pub idle_node: AnimationNodeIndex,
    pub walk_node: AnimationNodeIndex,
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

/// Tracks whether the local player is currently in the "walk" state to avoid restarting every frame.
#[derive(Component, Default)]
pub struct RangerAnimState {
    pub walking: bool,
}

// =============================================================================
// ASSET LOADING
// =============================================================================

/// Load the KayKit Ranger model + basic Idle/Walk animations and build an `AnimationGraph`.
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
        .load("KayKit_Adventurers_2.0_FREE/Characters/gltf/Ranger.glb#Scene0");

    // Animations (rig clips)
    // From `Rig_Medium_General.glb`: Idle_A is Animation6
    let idle_clip: Handle<AnimationClip> = asset_server.load(
        "KayKit_Adventurers_2.0_FREE/Animations/gltf/Rig_Medium/Rig_Medium_General.glb#Animation6",
    );
    // From `Rig_Medium_MovementBasic.glb`: Walking_A is Animation8
    let walk_clip: Handle<AnimationClip> = asset_server.load(
        "KayKit_Adventurers_2.0_FREE/Animations/gltf/Rig_Medium/Rig_Medium_MovementBasic.glb#Animation8",
    );

    // Build a graph with Idle + Walk (no blending yet; we just switch states).
    let (graph, nodes) = AnimationGraph::from_clips([idle_clip.clone(), walk_clip.clone()]);
    let animation_graph = animation_graphs.add(graph);
    let idle_node = nodes[0];
    let walk_node = nodes[1];

    commands.insert_resource(RangerCharacterAssets {
        ranger_scene,
        animation_graph,
        idle_node,
        walk_node,
    });

    info!("Loaded KayKit Ranger character assets (model + idle/walk anim clips)");
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
        // IMPORTANT: `sync_player_transforms` expects `Transform` + `Visibility` to exist on the Player entity.
        commands.entity(entity).insert((
            Transform::from_translation(position.0),
            Visibility::Inherited,
        ));

        // Spawn KayKit Ranger model as a child (so it inherits transform + visibility).
        // NOTE: Our gameplay PlayerPosition is at the capsule center, so we offset the model down.
        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                RangerModelRoot,
                NeedsRangerRigSetup,
                SceneRoot(ranger_assets.ranger_scene.clone()),
                // glTF models in Bevy default to +Z forward; our game treats -Z as forward.
                // Rotate 180 degrees so the character faces the correct direction.
                Transform::from_xyz(0.0, -PLAYER_HEIGHT * 0.5, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::splat(1.0)),
            ));
        });

        if is_local {
            commands.entity(entity).insert(LocalPlayer);
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
) {
    let Some(ranger_assets) = ranger_assets else { return };

    for model_root in model_roots.iter() {
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

/// Drive the Ranger animations:
/// - Everyone plays Idle by default
/// - The *local* player switches to Walk when WASD is pressed (and not in vehicle)
pub fn update_ranger_animation(
    ranger_assets: Option<Res<RangerCharacterAssets>>,
    input_state: Res<crate::input::InputState>,
    mut anim_roots: Query<(Entity, &mut RangerAnimState, &mut AnimationPlayer), With<RangerAnimationRoot>>,
    parents: Query<&ChildOf>,
    local_players: Query<(), With<LocalPlayer>>,
) {
    let Some(ranger_assets) = ranger_assets else { return };

    let local_is_moving =
        !input_state.in_vehicle
            && (input_state.forward || input_state.backward || input_state.left || input_state.right);

    for (entity, mut state, mut player) in anim_roots.iter_mut() {
        // Check whether this rig belongs to the LocalPlayer (walk only for local).
        let mut cursor = entity;
        let mut is_local = false;
        while let Ok(parent) = parents.get(cursor) {
            let p = parent.parent();
            if local_players.contains(p) {
                is_local = true;
                break;
            }
            cursor = p;
        }

        let should_walk = is_local && local_is_moving;

        if should_walk && !state.walking {
            player.stop(ranger_assets.idle_node);
            player.start(ranger_assets.walk_node).repeat();
            state.walking = true;
        } else if !should_walk && state.walking {
            player.stop(ranger_assets.walk_node);
            player.start(ranger_assets.idle_node).repeat();
            state.walking = false;
        } else {
            // Ensure something is playing (important on first frame after rig setup).
            if state.walking {
                if !player.is_playing_animation(ranger_assets.walk_node) {
                    player.start(ranger_assets.walk_node).repeat();
                }
            } else if !player.is_playing_animation(ranger_assets.idle_node) {
                player.start(ranger_assets.idle_node).repeat();
            }
        }
    }
}

// =============================================================================
// TRANSFORM SYNC
// =============================================================================

/// Sync player transforms and handle visibility based on camera mode
pub fn sync_player_transforms(
    time: Res<Time>,
    // In Lightyear 0.25, use LocalId to identify ourselves
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<(&VehicleDriver, &Transform), (With<Vehicle>, Without<Player>)>,
    input_state: Res<crate::input::InputState>,
    mut players: Query<
        (&Player, &PlayerPosition, &PlayerRotation, &mut Transform, &mut Visibility),
        Without<Vehicle>,
    >,
) {
    let dt = time.delta_secs();
    let pos_rate: f32 = 22.0;
    let rot_rate: f32 = 26.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();
    
    // Get our peer ID from the connected client entity
    let our_peer_id = client_query.iter().next().map(|r| r.0);

    // Map: driver_id -> vehicle transform (already smoothed in `sync_vehicle_transforms`)
    let mut driver_to_vehicle: HashMap<u64, (Vec3, Quat)> = HashMap::new();
    for (driver, veh_transform) in vehicles.iter() {
        if let Some(driver_id) = driver.driver_id {
            driver_to_vehicle.insert(driver_id, (veh_transform.translation, veh_transform.rotation));
        }
    }

    for (player, position, rotation, mut transform, mut visibility) in players.iter_mut() {
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
        
        // Local player visibility depends on camera mode and vehicle state
        let is_local = our_peer_id.map(|id| player.client_id == id).unwrap_or(false);
        if is_local {
            let should_hide = match input_state.camera_mode {
                CameraMode::FirstPerson => true,  // Always hide in first person
                CameraMode::ThirdPerson => false, // Show in third person (even in vehicle)
            };
            
            *visibility = if should_hide {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
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
