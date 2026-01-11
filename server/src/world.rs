//! World generation and management
//!
//! Updated for Lightyear 0.25

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::WorldTime;

/// Set up the game world (server-side, no rendering)
pub fn setup_world(_commands: Commands) {
    info!("Server world initialized");
    
    // The server doesn't need to spawn visual elements,
    // but we could spawn physics colliders here if using physics
    
    // For now, we just log that the world is ready
    // In the future, this would include:
    // - Terrain collision data
    // - NPC spawning
    // - World object placement
}

/// One-shot resource to ensure we only spawn `WorldTime` once.
#[derive(Resource)]
pub struct WorldTimeSpawned;

/// Spawn the server-authoritative day/night clock replicated to all clients.
///
/// This should run **after** the server has started networking, so clients actually receive it.
pub fn spawn_world_time_once(mut commands: Commands, spawned: Option<Res<WorldTimeSpawned>>) {
    if spawned.is_some() {
        return;
    }
    commands.insert_resource(WorldTimeSpawned);

    commands.spawn((
        WorldTime::new_default(),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    ));

    info!("Spawned WorldTime (day/night cycle) replicated to all clients");
}

/// Advance the world clock every fixed tick (server-authoritative).
pub fn tick_world_time(mut world_time: Query<&mut WorldTime>) {
    let dt = 1.0 / shared::FIXED_TIMESTEP_HZ as f32;
    for mut wt in world_time.iter_mut() {
        wt.advance(dt);
    }
}
