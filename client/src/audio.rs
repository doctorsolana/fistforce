//! Audio system for game sounds
//!
//! Handles weapon sounds, ambient sounds, and background music.
//! Updated for Bevy 0.17

use bevy::prelude::*;
use bevy::audio::Volume;

use shared::{terrain::Biome, LocalPlayer, PlayerPosition, WorldTerrain};

use crate::input::InputState;
use crate::states::GameState;
use crate::weapons::ShootingState;

/// Resource holding all loaded audio assets
#[derive(Resource)]
pub struct GameAudio {
    pub gun_shot: Handle<AudioSource>,
    pub desert_ambient: Handle<AudioSource>,
}

/// Marker for ambient sound entities
#[derive(Component)]
pub struct AmbientSound;

/// Marker for one-shot gunshot audio entities
#[derive(Component)]
pub struct GunshotSound;

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
    
    info!("Audio handles created: gun_shot={:?}, desert_ambient={:?}", gun_shot, desert_ambient);
    
    commands.insert_resource(GameAudio {
        gun_shot,
        desert_ambient,
    });
    
    commands.init_resource::<AudioState>();
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

/// Audio plugin for easy integration
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_audio);
        app.add_systems(OnExit(GameState::Playing), stop_ambient_sounds);
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
    }
}
