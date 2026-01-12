//! Audio system for game sounds
//!
//! Handles weapon sounds, ambient sounds, and background music.
//! Updated for Bevy 0.17

use bevy::prelude::*;
use bevy::audio::Volume;

use shared::{terrain::Biome, LocalPlayer, PlayerPosition, WorldTerrain, VehicleState, VehicleDriver, Vehicle};
use lightyear::prelude::*;

use crate::input::InputState;
use crate::states::GameState;
use crate::weapons::ShootingState;

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

    let biome = terrain.generator.get_biome(pos.0.x, pos.0.z);
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
    
    // Max speed from vehicle constants (~45 m/s)
    const MAX_SPEED: f32 = 45.0;
    let speed_ratio = (speed / MAX_SPEED).clamp(0.0, 1.0);
    
    // === CROSSFADE LOGIC ===
    // Idle: full volume at 0 speed, fades out by ~30% max speed
    // Cruise: silent at 0, fades in from ~10% to full at ~40% max speed
    
    let idle_volume = if speed_ratio < 0.3 {
        // Full to fading: 1.0 at 0%, ~0.0 at 30%
        1.0 - (speed_ratio / 0.3)
    } else {
        0.0
    };
    
    let cruise_volume = if speed_ratio < 0.1 {
        0.0
    } else if speed_ratio < 0.4 {
        // Fade in from 10% to 40% speed
        (speed_ratio - 0.1) / 0.3
    } else {
        1.0
    };
    
    // === PITCH MODULATION ===
    // Idle: subtle pitch variation 0.9x to 1.05x
    // Cruise: 0.85x at slow speeds up to 1.25x at max speed
    
    let idle_pitch = 0.9 + speed_ratio * 0.15; // 0.9 to 1.05
    let cruise_pitch = 0.85 + speed_ratio * 0.4; // 0.85 to 1.25
    
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

/// Audio plugin for easy integration
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_audio);
        app.add_systems(OnExit(GameState::Playing), (stop_ambient_sounds, stop_vehicle_sounds));
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
        // Vehicle audio systems
        app.add_systems(
            Update,
            manage_vehicle_audio.run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            update_vehicle_audio.run_if(in_state(GameState::Playing)),
        );
    }
}
