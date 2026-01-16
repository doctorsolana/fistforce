//! Audio system for game sounds
//!
//! Handles weapon sounds, ambient sounds, and background music.
//! Updated for Bevy 0.17

use bevy::prelude::*;
use bevy::audio::{SpatialAudioSink, Volume};

use shared::{
    terrain::Biome, LocalPlayer, Npc, NpcArchetype, Player, PlayerPosition, WorldTerrain, Vehicle, VehicleDriver,
    VehicleState,
};
use shared::{AudioEvent, AudioEventKind};
use lightyear::prelude::*;

use crate::camera::peer_id_to_u64;
use crate::input::InputState;
use crate::states::GameState;
use crate::weapons::ShootingState;

use std::collections::HashSet;

/// Resource holding all loaded audio assets
#[derive(Resource)]
pub struct GameAudio {
    pub gun_shot: Handle<AudioSource>,
    pub desert_ambient: Handle<AudioSource>,
    // Vehicle sounds
    pub hover_idle: Handle<AudioSource>,
    pub bike_cruise: Handle<AudioSource>,
}

/// Marker for ambient sound entities
#[derive(Component)]
pub struct AmbientSound;

/// Marker for one-shot gunshot audio entities
#[derive(Component)]
pub struct GunshotSound;

// =============================================================================
// VEHICLE AUDIO
// =============================================================================

/// Marker for vehicle idle hover sound
#[derive(Component)]
pub struct VehicleIdleSound;

/// Marker for vehicle cruise/driving sound
#[derive(Component)]
pub struct VehicleCruiseSound;

/// Track vehicle audio state
#[derive(Resource, Default)]
pub struct VehicleAudioState {
    pub sounds_spawned: bool,
    /// Track if player was in vehicle last frame (for detecting enter/exit)
    pub was_in_vehicle: bool,
}

/// Track audio state
#[derive(Resource, Default)]
pub struct AudioState {
    pub last_shot_time: f32,
    pub ambient_spawned: bool,
    pub assets_ready: bool,
}

/// Marker for remote player spatial audio (gunshots, etc.)
#[derive(Component)]
pub struct RemoteSpatialSound;

// =============================================================================
// AUDIO MANAGER (limits and prioritization)
// =============================================================================

/// Priority levels for audio - higher value = higher priority (less likely to be dropped)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AudioPriority {
    /// Remote footsteps - lowest priority, drop first
    Ambient = 0,
    /// Remote vehicle engines
    VehicleRemote = 1,
    /// Remote gunshots
    CombatRemote = 2,
    /// NPC dialogue
    Dialogue = 3,
    /// Local vehicle sounds
    VehicleLocal = 4,
    /// Local weapon sounds - highest priority, never dropped
    CombatLocal = 5,
}

/// Marker component for audio entities managed by AudioManager
#[derive(Component)]
pub struct ManagedAudioTag {
    pub priority: AudioPriority,
    pub spawn_time: f32,
}

/// A queued dialogue request (NPC wants to speak)
#[derive(Debug, Clone)]
pub struct DialogueRequest {
    pub npc_entity: Entity,
    pub distance_sq: f32,
    pub archetype: NpcArchetype,
}

/// Central audio manager - tracks limits and queues
#[derive(Resource)]
pub struct AudioManager {
    /// Hard cap on all managed audio entities
    pub max_total: usize,
    /// Max concurrent dialogue sounds
    pub max_dialogue: usize,
    /// Max remote gunshot sounds
    pub max_remote_combat: usize,
    /// Max remote footstep emitters
    pub max_remote_footsteps: usize,
    /// Max remote vehicle audio pairs (each vehicle = 2 emitters)
    pub max_remote_vehicles: usize,
    /// Queued dialogue requests for this frame
    pub dialogue_queue: Vec<DialogueRequest>,
}

impl Default for AudioManager {
    fn default() -> Self {
        Self {
            max_total: 32,
            max_dialogue: 4,
            max_remote_combat: 8,
            max_remote_footsteps: 12,
            max_remote_vehicles: 6,
            dialogue_queue: Vec::with_capacity(8),
        }
    }
}

// =============================================================================
// REMOTE FOOTSTEPS (PLAYERS + NPCS)
// =============================================================================

/// Spatial audio loop attached to a remote entity to represent footsteps.
#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteFootstepEmitter {
    pub target: Entity,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteFootstepState {
    pub last_pos: Vec3,
    pub playing: bool,
}

const REMOTE_FOOTSTEP_MAX_SPAWN_DISTANCE: f32 = 90.0;
const REMOTE_FOOTSTEP_DESPAWN_DISTANCE: f32 = 130.0;
const REMOTE_FOOTSTEP_START_SPEED: f32 = 0.6;
const REMOTE_FOOTSTEP_STOP_SPEED: f32 = 0.25;
const REMOTE_FOOTSTEP_VOLUME: f32 = 0.22;

// =============================================================================
// REMOTE VEHICLE AUDIO
// =============================================================================

#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteVehicleIdleSound {
    pub vehicle: Entity,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteVehicleCruiseSound {
    pub vehicle: Entity,
}

const REMOTE_VEHICLE_MAX_SPAWN_DISTANCE: f32 = 160.0;
const REMOTE_VEHICLE_DESPAWN_DISTANCE: f32 = 220.0;

/// Load all audio assets on startup
pub fn setup_audio(mut commands: Commands, asset_server: Res<AssetServer>) {
    info!("Audio system: Loading audio assets...");
    
    let gun_shot = asset_server.load("audio/sfx/gun_shot.ogg");
    let desert_ambient = asset_server.load("audio/ambient/walking_desert.ogg");
    
    // Vehicle sounds
    let hover_idle = asset_server.load("audio/sfx/hover_idle_loop.ogg");
    let bike_cruise = asset_server.load("audio/sfx/bike_cruise_loop.ogg");
    
    info!("Audio handles created: gun_shot={:?}, desert_ambient={:?}", gun_shot, desert_ambient);
    info!("Vehicle audio handles: hover_idle={:?}, bike_cruise={:?}", hover_idle, bike_cruise);
    
    commands.insert_resource(GameAudio {
        gun_shot,
        desert_ambient,
        hover_idle,
        bike_cruise,
    });
    
    commands.init_resource::<AudioState>();
    commands.init_resource::<VehicleAudioState>();
}

/// Check if audio assets are loaded
pub fn check_audio_assets_loaded(
    audio: Option<Res<GameAudio>>,
    mut audio_state: ResMut<AudioState>,
    asset_server: Res<AssetServer>,
) {
    if audio_state.assets_ready {
        return;
    }
    
    let Some(audio) = audio else { return };
    
    use bevy::asset::RecursiveDependencyLoadState;
    
    let gun_state = asset_server.get_recursive_dependency_load_state(&audio.gun_shot);
    let ambient_state = asset_server.get_recursive_dependency_load_state(&audio.desert_ambient);
    
    match (gun_state, ambient_state) {
        (Some(RecursiveDependencyLoadState::Loaded), Some(RecursiveDependencyLoadState::Loaded)) => {
            info!("Audio assets loaded successfully!");
            audio_state.assets_ready = true;
        }
        (Some(RecursiveDependencyLoadState::Failed(_)), _) => {
            error!("Failed to load gun_shot.ogg!");
        }
        (_, Some(RecursiveDependencyLoadState::Failed(_))) => {
            error!("Failed to load walking_desert.ogg!");
        }
        _ => {
            // Still loading
        }
    }
}

/// Ensure the ambient audio entity exists (spawn once when playing).
/// We start it paused; we will play/pause via `AudioSink` based on walking/biome.
pub fn ensure_ambient_entity(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    mut audio_state: ResMut<AudioState>,
    existing: Query<Entity, With<AmbientSound>>,
) {
    // Don't spawn until assets are ready
    if !audio_state.assets_ready {
        return;
    }
    
    if audio_state.ambient_spawned || !existing.is_empty() {
        return;
    }

    let Some(audio) = audio else { return };

    info!("Spawning desert ambient audio entity...");
    commands.spawn((
        AmbientSound,
        AudioPlayer::new(audio.desert_ambient.clone()),
        PlaybackSettings::LOOP
            .paused()
            .with_volume(Volume::Linear(0.4)),
    ));

    audio_state.ambient_spawned = true;
}

/// Play gunshot sound when we *actually fire* (check ShootingState resource).
pub fn play_gunshot_sfx(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    mut shooting_state: ResMut<ShootingState>,
    mut audio_state: ResMut<AudioState>,
    time: Res<Time>,
    input_state: Res<InputState>,
) {
    // Don't play until assets are ready
    if !audio_state.assets_ready {
        shooting_state.shot_fired_this_frame = false;
        return;
    }
    
    let Some(audio) = audio else { 
        shooting_state.shot_fired_this_frame = false;
        return;
    };
    
    // Don't play sounds when in vehicle
    if input_state.in_vehicle {
        shooting_state.shot_fired_this_frame = false;
        return;
    }
    
    let current_time = time.elapsed_secs();
    
    // Check if a shot was fired this frame
    if shooting_state.shot_fired_this_frame && (current_time - audio_state.last_shot_time) > 0.05 {
        info!("Playing gunshot sound!");
        commands.spawn((
            GunshotSound,
            AudioPlayer::new(audio.gun_shot.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.7)),
        ));
        audio_state.last_shot_time = current_time;
    }
    
    // Reset the flag for next frame
    shooting_state.shot_fired_this_frame = false;
}

/// Handle audio events from remote players (spatial audio)
///
/// When other players shoot, we receive an AudioEvent from the server
/// and play a spatial sound at their position.
/// Respects max_remote_combat limit by despawning oldest sounds when at capacity.
pub fn handle_remote_audio_events(
    mut commands: Commands,
    time: Res<Time>,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    audio_manager: Res<AudioManager>,
    camera: Query<&Transform, With<Camera3d>>,
    // Get our local player ID to skip our own sounds (we already play them locally)
    local_player: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    // Receive network messages
    mut receiver: Query<&mut MessageReceiver<AudioEvent>, (With<crate::GameClient>, With<Connected>)>,
    // Query existing remote combat sounds to enforce limit
    remote_sounds: Query<(Entity, &ManagedAudioTag, &Transform), With<RemoteSpatialSound>>,
) {
    // Don't process until audio assets are ready
    if !audio_state.assets_ready {
        return;
    }
    let Some(audio) = audio else { return };
    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let now = time.elapsed_secs();

    // Get our peer ID to skip our own sounds
    let our_id = local_player.iter().next().map(|id| peer_id_to_u64(id.0));

    // Count current remote combat sounds
    let mut current_remote_count = remote_sounds.iter()
        .filter(|(_, tag, _)| tag.priority == AudioPriority::CombatRemote)
        .count();

    // Process incoming audio events
    for mut recv in receiver.iter_mut() {
        for audio_event in recv.receive() {
            // Skip if this is our own sound (we already play it locally)
            if Some(audio_event.player_id) == our_id {
                continue;
            }

            match audio_event.kind {
                AudioEventKind::Gunshot { weapon_type: _ } => {
                    // Check limit and despawn oldest if needed
                    if current_remote_count >= audio_manager.max_remote_combat {
                        // Find and despawn the oldest/farthest remote combat sound
                        if let Some((oldest_entity, _, _)) = remote_sounds.iter()
                            .filter(|(_, tag, _)| tag.priority == AudioPriority::CombatRemote)
                            .min_by(|(_, a_tag, a_tf), (_, b_tag, b_tf)| {
                                // Prefer despawning older sounds, then farther ones
                                let a_score = a_tag.spawn_time - a_tf.translation.distance_squared(listener_pos) * 0.001;
                                let b_score = b_tag.spawn_time - b_tf.translation.distance_squared(listener_pos) * 0.001;
                                a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
                            })
                        {
                            commands.entity(oldest_entity).despawn();
                            current_remote_count = current_remote_count.saturating_sub(1);
                        }
                    }

                    // Random pitch variation for variety (±5%)
                    let pitch = 0.95 + rand::random::<f32>() * 0.1;

                    // Spawn spatial audio at the shooter's position with ManagedAudioTag
                    commands.spawn((
                        RemoteSpatialSound,
                        ManagedAudioTag {
                            priority: AudioPriority::CombatRemote,
                            spawn_time: now,
                        },
                        AudioPlayer::new(audio.gun_shot.clone()),
                        PlaybackSettings::DESPAWN
                            .with_volume(Volume::Linear(0.8))
                            .with_speed(pitch)
                            .with_spatial(true),
                        Transform::from_translation(audio_event.position),
                    ));
                    current_remote_count += 1;
                }
            }
        }
    }
}

/// Ensure we have spatial footstep emitters for nearby remote players + NPCs.
///
/// Perf notes:
/// - No network traffic: we infer movement from replicated transforms.
/// - We only spawn emitters within a distance threshold.
/// - Respects max_remote_footsteps limit, prioritizing closest entities.
pub fn ensure_remote_footstep_emitters(
    mut commands: Commands,
    time: Res<Time>,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    audio_manager: Res<AudioManager>,
    camera: Query<&Transform, With<Camera3d>>,
    // Remote players only (local player has their own loop)
    players: Query<(Entity, &Player, &Transform), (With<Player>, Without<LocalPlayer>)>,
    // NPCs
    npcs: Query<(Entity, &Transform), With<Npc>>,
    // Vehicles to identify which players are driving (no footsteps)
    vehicles: Query<&VehicleDriver, With<Vehicle>>,
    existing: Query<&RemoteFootstepEmitter>,
) {
    if !audio_state.assets_ready {
        return;
    }
    let Some(audio) = audio else { return };

    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;
    let now = time.elapsed_secs();

    // Gather driver IDs so we can skip footsteps for players that are driving.
    let mut driving_ids: HashSet<u64> = HashSet::new();
    for driver in vehicles.iter() {
        if let Some(id) = driver.driver_id {
            driving_ids.insert(id);
        }
    }

    // Existing emitters -> targets.
    let has_emitter: HashSet<Entity> = existing.iter().map(|e| e.target).collect();
    let current_count = has_emitter.len();

    // Check if we're at the limit
    if current_count >= audio_manager.max_remote_footsteps {
        return;
    }
    let available_slots = audio_manager.max_remote_footsteps - current_count;

    // Collect candidates with distances (entity, position, distance_squared)
    let mut candidates: Vec<(Entity, Vec3, f32)> = Vec::new();
    let max_dist_sq = REMOTE_FOOTSTEP_MAX_SPAWN_DISTANCE * REMOTE_FOOTSTEP_MAX_SPAWN_DISTANCE;

    // Remote players.
    for (entity, player, transform) in players.iter() {
        let player_id = peer_id_to_u64(player.client_id);
        if driving_ids.contains(&player_id) || has_emitter.contains(&entity) {
            continue;
        }

        let dist_sq = transform.translation.distance_squared(listener_pos);
        if dist_sq <= max_dist_sq {
            candidates.push((entity, transform.translation, dist_sq));
        }
    }

    // NPCs.
    for (entity, transform) in npcs.iter() {
        if has_emitter.contains(&entity) {
            continue;
        }

        let dist_sq = transform.translation.distance_squared(listener_pos);
        if dist_sq <= max_dist_sq {
            candidates.push((entity, transform.translation, dist_sq));
        }
    }

    // Sort by distance (closest first), spawn up to available_slots
    candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    for (entity, pos, _dist_sq) in candidates.into_iter().take(available_slots) {
        commands.spawn((
            RemoteFootstepEmitter { target: entity },
            RemoteFootstepState {
                last_pos: pos,
                playing: false,
            },
            ManagedAudioTag {
                priority: AudioPriority::Ambient,
                spawn_time: now,
            },
            AudioPlayer::new(audio.desert_ambient.clone()),
            PlaybackSettings::LOOP
                .paused()
                .with_volume(Volume::Linear(REMOTE_FOOTSTEP_VOLUME))
                .with_spatial(true),
            Transform::from_translation(pos),
            GlobalTransform::default(),
        ));
    }
}

/// Update remote footstep emitters:
/// - Follow the target entity
/// - Pause/play the loop based on inferred movement (with hysteresis)
pub fn update_remote_footstep_emitters(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<RemoteFootstepEmitter>)>,
    target_transforms: Query<&Transform, (Without<RemoteFootstepEmitter>, Without<Camera3d>)>,
    mut emitters: Query<
        (
            Entity,
            &RemoteFootstepEmitter,
            &mut RemoteFootstepState,
            &mut Transform,
            &SpatialAudioSink,
        ),
        Without<Camera3d>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;

    for (entity, emitter, mut state, mut transform, sink) in emitters.iter_mut() {
        let Ok(target_tf) = target_transforms.get(emitter.target) else {
            // Target despawned.
            commands.entity(entity).despawn();
            continue;
        };

        let target_pos = target_tf.translation;

        // Cull far emitters to keep entity count and audio sinks small.
        if target_pos.distance(listener_pos) > REMOTE_FOOTSTEP_DESPAWN_DISTANCE {
            commands.entity(entity).despawn();
            continue;
        }

        // Follow.
        transform.translation = target_pos;

        // Infer movement.
        let delta = target_pos - state.last_pos;
        state.last_pos = target_pos;

        let horizontal = Vec3::new(delta.x, 0.0, delta.z);
        let speed = horizontal.length() / dt;

        let should_play = if state.playing {
            speed > REMOTE_FOOTSTEP_STOP_SPEED
        } else {
            speed > REMOTE_FOOTSTEP_START_SPEED
        };

        if should_play && sink.is_paused() {
            sink.play();
            state.playing = true;
        } else if !should_play && !sink.is_paused() {
            sink.pause();
            state.playing = false;
        }
    }
}

/// Ensure spatial vehicle audio emitters exist for nearby *remote* vehicles.
///
/// We don't rely on network audio events for engines: we already replicate `VehicleState`,
/// so we can generate continuous audio locally (lower bandwidth, more robust).
pub fn ensure_remote_vehicle_audio_emitters(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    camera: Query<&Transform, With<Camera3d>>,
    // Our peer ID (so we can skip the vehicle we're driving; local has non-spatial loops)
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<(Entity, &VehicleDriver, &VehicleState, &Transform), With<Vehicle>>,
    existing_idle: Query<&RemoteVehicleIdleSound>,
    existing_cruise: Query<&RemoteVehicleCruiseSound>,
) {
    if !audio_state.assets_ready {
        return;
    }
    let Some(audio) = audio else { return };

    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;

    let our_id = client_query.iter().next().map(|id| peer_id_to_u64(id.0));

    let mut has_idle: HashSet<Entity> = HashSet::new();
    for e in existing_idle.iter() {
        has_idle.insert(e.vehicle);
    }
    let mut has_cruise: HashSet<Entity> = HashSet::new();
    for e in existing_cruise.iter() {
        has_cruise.insert(e.vehicle);
    }

    for (veh_entity, driver, _state, veh_tf) in vehicles.iter() {
        // Skip the vehicle we're driving (local audio handles it).
        if let (Some(ours), Some(driver_id)) = (our_id, driver.driver_id) {
            if driver_id == ours {
                continue;
            }
        }

        if veh_tf.translation.distance(listener_pos) > REMOTE_VEHICLE_MAX_SPAWN_DISTANCE {
            continue;
        }

        if !has_idle.contains(&veh_entity) {
            commands.spawn((
                RemoteVehicleIdleSound { vehicle: veh_entity },
                AudioPlayer::new(audio.hover_idle.clone()),
                PlaybackSettings::LOOP
                    .with_volume(Volume::Linear(0.0))
                    .with_spatial(true),
                Transform::from_translation(veh_tf.translation),
                GlobalTransform::default(),
            ));
        }

        if !has_cruise.contains(&veh_entity) {
            commands.spawn((
                RemoteVehicleCruiseSound { vehicle: veh_entity },
                AudioPlayer::new(audio.bike_cruise.clone()),
                PlaybackSettings::LOOP
                    .with_volume(Volume::Linear(0.0))
                    .with_spatial(true),
                Transform::from_translation(veh_tf.translation),
                GlobalTransform::default(),
            ));
        }
    }
}

/// Compute the crossfade + pitch parameters for the motorbike audio based on speed.
fn vehicle_audio_params(speed: f32) -> (f32, f32, f32, f32) {
    // Max speed from vehicle constants (~45 m/s)
    const MAX_SPEED: f32 = 45.0;
    let speed_ratio = (speed / MAX_SPEED).clamp(0.0, 1.0);

    // === CROSSFADE LOGIC ===
    // Idle: full volume at 0 speed, fades out by ~30% max speed
    // Cruise: silent at 0, fades in from ~10% to full at ~40% max speed
    let idle_volume = if speed_ratio < 0.3 {
        1.0 - (speed_ratio / 0.3)
    } else {
        0.0
    };

    let cruise_volume = if speed_ratio < 0.1 {
        0.0
    } else if speed_ratio < 0.4 {
        (speed_ratio - 0.1) / 0.3
    } else {
        1.0
    };

    // === PITCH MODULATION ===
    // Idle: subtle pitch variation 0.9x to 1.05x
    // Cruise: 0.85x at slow speeds up to 1.25x at max speed
    let idle_pitch = 0.9 + speed_ratio * 0.15;
    let cruise_pitch = 0.85 + speed_ratio * 0.4;

    (idle_volume, cruise_volume, idle_pitch, cruise_pitch)
}

/// Update remote vehicle audio emitters:
/// - Follow the vehicle transform
/// - Modulate volume/pitch based on speed
/// - Despawn when far away or when we become the driver (avoid double audio)
pub fn update_remote_vehicle_audio_emitters(
    mut commands: Commands,
    camera: Query<&Transform, (With<Camera3d>, Without<RemoteVehicleIdleSound>, Without<RemoteVehicleCruiseSound>)>,
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<(&VehicleDriver, &VehicleState, &Transform), (With<Vehicle>, Without<RemoteVehicleIdleSound>, Without<RemoteVehicleCruiseSound>)>,
    mut idle_emitters: Query<
        (Entity, &RemoteVehicleIdleSound, &mut Transform, &mut SpatialAudioSink),
        Without<RemoteVehicleCruiseSound>,
    >,
    mut cruise_emitters: Query<
        (
            Entity,
            &RemoteVehicleCruiseSound,
            &mut Transform,
            &mut SpatialAudioSink,
        ),
        Without<RemoteVehicleIdleSound>,
    >,
) {
    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;

    let our_id = client_query.iter().next().map(|id| peer_id_to_u64(id.0));

    // Update idle loops
    for (entity, e, mut tf, mut sink) in idle_emitters.iter_mut() {
        let Ok((driver, state, veh_tf)) = vehicles.get(e.vehicle) else {
            commands.entity(entity).despawn();
            continue;
        };

        // If we started driving this vehicle, remove remote audio emitters.
        if let (Some(ours), Some(driver_id)) = (our_id, driver.driver_id) {
            if driver_id == ours {
                commands.entity(entity).despawn();
                continue;
            }
        }

        let pos = veh_tf.translation;
        if pos.distance(listener_pos) > REMOTE_VEHICLE_DESPAWN_DISTANCE {
            commands.entity(entity).despawn();
            continue;
        }

        tf.translation = pos;

        let horizontal_velocity = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
        let speed = horizontal_velocity.length();
        let (idle_v, _cruise_v, idle_pitch, _cruise_pitch) = vehicle_audio_params(speed);

        sink.set_volume(Volume::Linear(idle_v * 0.6));
        sink.set_speed(idle_pitch);
    }

    // Update cruise loops
    for (entity, e, mut tf, mut sink) in cruise_emitters.iter_mut() {
        let Ok((driver, state, veh_tf)) = vehicles.get(e.vehicle) else {
            commands.entity(entity).despawn();
            continue;
        };

        if let (Some(ours), Some(driver_id)) = (our_id, driver.driver_id) {
            if driver_id == ours {
                commands.entity(entity).despawn();
                continue;
            }
        }

        let pos = veh_tf.translation;
        if pos.distance(listener_pos) > REMOTE_VEHICLE_DESPAWN_DISTANCE {
            commands.entity(entity).despawn();
            continue;
        }

        tf.translation = pos;

        let horizontal_velocity = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
        let speed = horizontal_velocity.length();
        let (_idle_v, cruise_v, _idle_pitch, cruise_pitch) = vehicle_audio_params(speed);

        sink.set_volume(Volume::Linear(cruise_v * 0.7));
        sink.set_speed(cruise_pitch);
    }
}

/// Control desert walking ambient:
/// - Only plays while walking (WASD pressed)
/// - Only plays in Desert biome
/// - Pauses otherwise
pub fn update_desert_walking_ambient(
    input_state: Res<InputState>,
    terrain: Res<WorldTerrain>,
    player_pos: Query<&PlayerPosition, With<LocalPlayer>>,
    ambient: Query<&AudioSink, With<AmbientSound>>,
) {
    let Ok(pos) = player_pos.single() else { return };

    let biome = terrain.get_biome(pos.0.x, pos.0.z);
    let walking = (input_state.forward || input_state.backward || input_state.left || input_state.right)
        && !input_state.in_vehicle;
    let should_play = walking && biome == Biome::Desert;

    for sink in ambient.iter() {
        if should_play && sink.is_paused() {
            sink.play();
        } else if !should_play && !sink.is_paused() {
            sink.pause();
        }
    }
}

/// Stop ambient sounds when leaving gameplay
pub fn stop_ambient_sounds(
    mut commands: Commands,
    mut audio_state: ResMut<AudioState>,
    ambient_sounds: Query<Entity, With<AmbientSound>>,
) {
    for entity in ambient_sounds.iter() {
        commands.entity(entity).despawn();
    }
    audio_state.ambient_spawned = false;
}

/// Stop/despawn remote looped spatial sounds (footsteps + vehicle engines).
///
/// Remote gunshots are one-shots with `DESPAWN` and don't need explicit cleanup.
pub fn stop_remote_loop_sounds(
    mut commands: Commands,
    footsteps: Query<Entity, With<RemoteFootstepEmitter>>,
    remote_idle: Query<Entity, With<RemoteVehicleIdleSound>>,
    remote_cruise: Query<Entity, With<RemoteVehicleCruiseSound>>,
) {
    for e in footsteps.iter() {
        commands.entity(e).despawn();
    }
    for e in remote_idle.iter() {
        commands.entity(e).despawn();
    }
    for e in remote_cruise.iter() {
        commands.entity(e).despawn();
    }
}

// =============================================================================
// VEHICLE AUDIO SYSTEMS
// =============================================================================

/// Manage vehicle audio: spawn sounds when entering, despawn when exiting
pub fn manage_vehicle_audio(
    mut commands: Commands,
    audio: Option<Res<GameAudio>>,
    audio_state: Res<AudioState>,
    mut vehicle_audio_state: ResMut<VehicleAudioState>,
    input_state: Res<InputState>,
    idle_sounds: Query<Entity, With<VehicleIdleSound>>,
    cruise_sounds: Query<Entity, With<VehicleCruiseSound>>,
) {
    // Don't do anything until assets are ready
    if !audio_state.assets_ready {
        return;
    }
    
    let Some(audio) = audio else { return };
    
    let in_vehicle = input_state.in_vehicle;
    let was_in_vehicle = vehicle_audio_state.was_in_vehicle;
    
    // Detect entering vehicle
    if in_vehicle && !was_in_vehicle {
        info!("Entering vehicle - spawning bike audio loops");
        
        // Spawn idle sound (starts playing)
        commands.spawn((
            VehicleIdleSound,
            AudioPlayer::new(audio.hover_idle.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.6)),
        ));
        
        // Spawn cruise sound (starts at zero volume, we'll crossfade in)
        commands.spawn((
            VehicleCruiseSound,
            AudioPlayer::new(audio.bike_cruise.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
        ));
        
        vehicle_audio_state.sounds_spawned = true;
    }
    
    // Detect exiting vehicle
    if !in_vehicle && was_in_vehicle {
        info!("Exiting vehicle - despawning bike audio");
        
        for entity in idle_sounds.iter() {
            commands.entity(entity).despawn();
        }
        for entity in cruise_sounds.iter() {
            commands.entity(entity).despawn();
        }
        
        vehicle_audio_state.sounds_spawned = false;
    }
    
    vehicle_audio_state.was_in_vehicle = in_vehicle;
}

/// Update vehicle audio: crossfade between idle/cruise and modulate pitch based on speed
pub fn update_vehicle_audio(
    input_state: Res<InputState>,
    vehicle_audio_state: Res<VehicleAudioState>,
    // Query for our local client to find which vehicle we're driving
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<(&VehicleDriver, &VehicleState), With<Vehicle>>,
    // Use Without<T> to make these queries disjoint (avoids Bevy B0001 conflict)
    mut idle_sink: Query<&mut AudioSink, (With<VehicleIdleSound>, Without<VehicleCruiseSound>)>,
    mut cruise_sink: Query<&mut AudioSink, (With<VehicleCruiseSound>, Without<VehicleIdleSound>)>,
) {
    // Only process if we're in a vehicle with sounds spawned
    if !input_state.in_vehicle || !vehicle_audio_state.sounds_spawned {
        return;
    }
    
    // Get our peer ID
    let Some(our_peer_id) = client_query.iter().next().map(|id| crate::camera::peer_id_to_u64(id.0)) else {
        return;
    };
    
    // Find the vehicle we're driving
    let Some((_, vehicle_state)) = vehicles.iter().find(|(driver, _)| driver.driver_id == Some(our_peer_id)) else {
        return;
    };
    
    // Calculate speed (horizontal only for audio purposes)
    let horizontal_velocity = Vec3::new(vehicle_state.velocity.x, 0.0, vehicle_state.velocity.z);
    let speed = horizontal_velocity.length();
    let (idle_volume, cruise_volume, idle_pitch, cruise_pitch) = vehicle_audio_params(speed);
    
    // Apply to idle sound
    if let Ok(mut sink) = idle_sink.single_mut() {
        sink.set_volume(Volume::Linear(idle_volume * 0.6)); // Base volume * crossfade
        sink.set_speed(idle_pitch);
    }
    
    // Apply to cruise sound
    if let Ok(mut sink) = cruise_sink.single_mut() {
        sink.set_volume(Volume::Linear(cruise_volume * 0.7)); // Base volume * crossfade
        sink.set_speed(cruise_pitch);
    }
}

/// Stop vehicle sounds when leaving gameplay
pub fn stop_vehicle_sounds(
    mut commands: Commands,
    mut vehicle_audio_state: ResMut<VehicleAudioState>,
    idle_sounds: Query<Entity, With<VehicleIdleSound>>,
    cruise_sounds: Query<Entity, With<VehicleCruiseSound>>,
) {
    for entity in idle_sounds.iter() {
        commands.entity(entity).despawn();
    }
    for entity in cruise_sounds.iter() {
        commands.entity(entity).despawn();
    }
    vehicle_audio_state.sounds_spawned = false;
    vehicle_audio_state.was_in_vehicle = false;
}

// =============================================================================
// AUDIO LIMIT ENFORCEMENT
// =============================================================================

/// Enforce global audio limits by despawning lowest priority sounds when over limit
pub fn enforce_audio_limits(
    mut commands: Commands,
    audio_manager: Res<AudioManager>,
    camera: Query<&Transform, With<Camera3d>>,
    managed_audio: Query<(Entity, &ManagedAudioTag, &Transform)>,
) {
    let Ok(cam) = camera.single() else { return };
    let listener_pos = cam.translation;

    // Collect all managed audio with their priority and distance
    let mut audio_list: Vec<(Entity, AudioPriority, f32, f32)> = managed_audio
        .iter()
        .map(|(entity, tag, tf)| {
            let dist_sq = tf.translation.distance_squared(listener_pos);
            (entity, tag.priority, dist_sq, tag.spawn_time)
        })
        .collect();

    let current_count = audio_list.len();
    if current_count <= audio_manager.max_total {
        return;
    }

    let excess = current_count - audio_manager.max_total;

    // Sort by priority (ascending), then distance (descending), then age (oldest first)
    // This puts lowest-priority, farthest, oldest sounds at the front for removal
    audio_list.sort_by(|a, b| {
        match a.1.cmp(&b.1) {
            std::cmp::Ordering::Equal => {
                // Same priority: farther sounds should be removed first
                match b.2.partial_cmp(&a.2) {
                    Some(std::cmp::Ordering::Equal) | None => {
                        // Same distance: older sounds removed first
                        a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    Some(ord) => ord,
                }
            }
            ord => ord,
        }
    });

    // Despawn the excess lowest-priority sounds
    for (entity, priority, _dist, _time) in audio_list.into_iter().take(excess) {
        commands.entity(entity).despawn();
        trace!("Audio limit: despawned {:?} (priority {:?})", entity, priority);
    }
}

/// Audio plugin for easy integration
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        // Audio limit manager
        app.init_resource::<AudioManager>();

        app.add_systems(Startup, setup_audio);
        app.add_systems(
            OnExit(GameState::Playing),
            (stop_ambient_sounds, stop_vehicle_sounds, stop_remote_loop_sounds),
        );
        // Split systems to avoid tuple size limits
        app.add_systems(
            Update,
            check_audio_assets_loaded.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            ensure_ambient_entity.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_desert_walking_ambient.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            play_gunshot_sfx.run_if(in_state(GameState::Playing)),
        );
        // Remote player spatial audio
        app.add_systems(
            Update,
            handle_remote_audio_events.run_if(in_state(GameState::Playing)),
        );
        // Remote spatial footsteps (players + NPCs)
        app.add_systems(
            Update,
            (
                ensure_remote_footstep_emitters,
                update_remote_footstep_emitters,
            )
                .run_if(in_state(GameState::Playing)),
        );
        // Remote vehicle spatial audio (engines)
        app.add_systems(
            Update,
            (
                ensure_remote_vehicle_audio_emitters,
                update_remote_vehicle_audio_emitters,
            )
                .run_if(in_state(GameState::Playing)),
        );
        // Vehicle audio systems
        app.add_systems(
            Update,
            manage_vehicle_audio.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_vehicle_audio.run_if(in_state(GameState::Playing)),
        );
        // Audio limit enforcement (runs last to cull excess audio)
        app.add_systems(
            Update,
            enforce_audio_limits
                .run_if(in_state(GameState::Playing))
                .after(ensure_remote_footstep_emitters)
                .after(ensure_remote_vehicle_audio_emitters)
                .after(handle_remote_audio_events),
        );
    }
}
