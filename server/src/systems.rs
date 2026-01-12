//! Server-side game systems
//! 
//! Updated for Lightyear 0.25

use bevy::prelude::*;
use lightyear::prelude::*;
use lightyear::prelude::server::*;
use std::collections::HashMap;

use shared::{
    ground_clearance_center, step_character, step_vehicle_physics, can_interact_with_vehicle,
    Player, PlayerInput, PlayerPosition, PlayerRotation, PlayerVelocity,
    VehicleState, VehicleDriver, VehicleInput, InVehicle,
    WorldTerrain, FIXED_TIMESTEP_HZ, SPAWN_POSITION,
    Health, EquippedWeapon, WeaponType,
};

/// How long to wait before respawning (seconds)
const RESPAWN_TIME: f32 = 4.0;

/// Component added to dead players while waiting to respawn
#[derive(Component)]
pub struct RespawnTimer {
    pub time_remaining: f32,
}

/// Stores the latest input for each connected client.
/// We use PeerId in Lightyear 0.25
#[derive(Resource, Default)]
pub struct ClientInputs {
    pub latest: HashMap<PeerId, PlayerInput>,
}

/// Handle new client connections - spawn a player for each
/// In Lightyear 0.25, we query for newly added ClientOf + Connected entities
pub fn handle_connections(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    // Query for client links that just got Connected
    new_clients: Query<(Entity, &RemoteId), Added<Connected>>,
    // Filter to only get client links (not the server itself)
    client_filter: Query<(), With<ClientOf>>,
) {
    for (client_entity, remote_id) in new_clients.iter() {
        // Skip if this isn't a client link
        if client_filter.get(client_entity).is_err() {
            continue;
        }
        
        let peer_id = remote_id.0;
        info!("Client connected: {:?}", peer_id);

        // IMPORTANT: enable replication + message I/O on this client link.
        //
        // Lightyear 0.25 requires you to add these components to the connection entity
        // (the entity with `ClientOf` + `Connected`). Without them, no replication happens,
        // so the client never receives `WorldTime`, `Player`, terrain, etc.
        commands.entity(client_entity).insert((
            // Replication out: server -> this client
            ReplicationSender::new(shared::protocol::tick_duration(), SendUpdatesMode::SinceLastAck, false),
            // Gameplay messages (explicitly added; otherwise failures here are completely silent).
            //
            // Client -> Server
            MessageReceiver::<PlayerInput>::default(),
            MessageReceiver::<shared::ShootRequest>::default(),
            MessageReceiver::<shared::SwitchWeapon>::default(),
            MessageReceiver::<shared::ReloadRequest>::default(),
            // Server -> Client
            MessageSender::<shared::HitConfirm>::default(),
            MessageSender::<shared::DamageReceived>::default(),
            MessageSender::<shared::PlayerKilled>::default(),
            MessageSender::<shared::BulletImpact>::default(),
        ));

        let spawn_x = SPAWN_POSITION[0];
        let spawn_z = SPAWN_POSITION[2];
        let ground_y = terrain.generator.get_height(spawn_x, spawn_z);
        let spawn_pos = Vec3::new(spawn_x, ground_y + ground_clearance_center(), spawn_z);

        // Spawn player entity
        commands.spawn((
            Player { client_id: peer_id },
            PlayerPosition(spawn_pos),
            PlayerRotation(0.0),
            PlayerVelocity(Vec3::ZERO),
            // Combat components
            Health::default(),
            EquippedWeapon::new(WeaponType::AssaultRifle),
            // Replicate to all clients
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            // In Lightyear 0.25, ControlledBy uses an entity reference to the client link
            ControlledBy {
                owner: client_entity,
                lifetime: Lifetime::default(),
            },
        ));

        info!("Spawned player for client {:?}", peer_id);
    }
}

/// Handle client disconnections
pub fn handle_disconnections(
    mut commands: Commands,
    // Query for client links that just got Disconnected
    disconnected_clients: Query<(Entity, &RemoteId), Added<Disconnected>>,
    client_filter: Query<(), With<ClientOf>>,
    players: Query<(Entity, &Player)>,
    mut vehicles: Query<&mut VehicleDriver>,
    mut inputs: ResMut<ClientInputs>,
) {
    for (client_link_entity, remote_id) in disconnected_clients.iter() {
        // Skip if this isn't a client link
        if client_filter.get(client_link_entity).is_err() {
            continue;
        }
        
        let peer_id = remote_id.0;
        info!("Client disconnected: {:?}", peer_id);

        // Remove from any vehicles they're driving
        for mut driver in vehicles.iter_mut() {
            if driver.driver_id == Some(peer_id_to_u64(peer_id)) {
                driver.driver_id = None;
            }
        }

        // Despawn their player entity
        for (player_entity, player) in players.iter() {
            if player.client_id == peer_id {
                commands.entity(player_entity).despawn();
                info!("Despawned player for client {:?}", peer_id);
            }
        }

        inputs.latest.remove(&peer_id);
        
        // IMPORTANT: Despawn the client link entity itself to stop replication errors
        // This prevents "ClientOf X not found or does not have ReplicationSender" spam
        commands.entity(client_link_entity).despawn();
        info!("Cleaned up client link entity for {:?}", peer_id);
    }
}

/// Receive input messages from clients
/// In Lightyear 0.25, we read from MessageReceiver components
pub fn receive_client_input(
    mut inputs: ResMut<ClientInputs>,
    // Query client link entities that have a MessageReceiver for PlayerInput
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<PlayerInput>), With<ClientOf>>,
    time: Res<Time>,
    mut last_debug_time: Local<f32>,
) {
    let now = time.elapsed_secs();
    for (remote_id, mut receiver) in client_links.iter_mut() {
        // Read all received messages
        let mut any = false;
        for input in receiver.receive() {
            any = true;
            inputs.latest.insert(remote_id.0, input);
        }
        if any && (now - *last_debug_time) > 0.5 {
            info!("Received PlayerInput from {:?}", remote_id.0);
            *last_debug_time = now;
        }
    }
}

/// Handle player vehicle interactions (enter/exit)
pub fn handle_vehicle_interactions(
    mut commands: Commands,
    inputs: Res<ClientInputs>,
    mut players: Query<(Entity, &Player, &PlayerPosition, Option<&InVehicle>)>,
    mut vehicles: Query<(Entity, &mut VehicleDriver, &VehicleState)>,
) {
    for (player_entity, player, player_pos, in_vehicle) in players.iter_mut() {
        let Some(input) = inputs.latest.get(&player.client_id) else {
            continue;
        };

        if !input.interact {
            continue;
        }

        if let Some(in_veh) = in_vehicle {
            for (veh_entity, mut driver, _state) in vehicles.iter_mut() {
                if veh_entity == in_veh.vehicle_entity {
                    driver.driver_id = None;
                    commands.entity(player_entity).remove::<InVehicle>();
                    info!("Player {:?} exited vehicle", player.client_id);
                    break;
                }
            }
        } else {
            for (veh_entity, mut driver, state) in vehicles.iter_mut() {
                if driver.driver_id.is_some() {
                    continue;
                }
                
                if can_interact_with_vehicle(player_pos.0, state) {
                    driver.driver_id = Some(peer_id_to_u64(player.client_id));
                    commands.entity(player_entity).insert(InVehicle {
                        vehicle_entity: veh_entity,
                    });
                    info!("Player {:?} entered vehicle", player.client_id);
                    break;
                }
            }
        }
    }
}

/// Simulate all players
pub fn simulate_players(
    terrain: Res<WorldTerrain>,
    inputs: Res<ClientInputs>,
    mut players: Query<(&Player, &Health, &mut PlayerPosition, &mut PlayerRotation, &mut PlayerVelocity, Option<&InVehicle>, Option<&RespawnTimer>)>,
    vehicles: Query<&VehicleState>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (player, health, mut position, mut rotation, mut velocity, in_vehicle, respawn_timer) in players.iter_mut() {
        // Skip dead players - don't process their input
        if !is_player_alive(health, respawn_timer) {
            // Dead players just stay where they are (no gravity, no movement)
            velocity.0 = Vec3::ZERO;
            continue;
        }
        
        if let Some(in_veh) = in_vehicle {
            if let Ok(veh_state) = vehicles.get(in_veh.vehicle_entity) {
                position.0 = veh_state.position;
                rotation.0 = veh_state.heading;
                velocity.0 = Vec3::ZERO;
                continue;
            }
        }

        let input = inputs
            .latest
            .get(&player.client_id)
            .cloned()
            .unwrap_or_default();

        step_character(
            &input,
            &terrain.generator,
            &mut position,
            &mut rotation,
            &mut velocity,
            dt,
        );
    }
}

/// Simulate all vehicles
pub fn simulate_vehicles(
    terrain: Res<WorldTerrain>,
    inputs: Res<ClientInputs>,
    players: Query<&Player>,
    mut vehicles: Query<(&VehicleDriver, &mut VehicleState)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (driver, mut state) in vehicles.iter_mut() {
        let vehicle_input = if let Some(driver_id) = driver.driver_id {
            let peer_id = players.iter()
                .find(|p| peer_id_to_u64(p.client_id) == driver_id)
                .map(|p| p.client_id);

            peer_id
                .and_then(|pid| inputs.latest.get(&pid))
                .and_then(|input| input.vehicle_input.clone())
                .unwrap_or_default()
        } else {
            VehicleInput::default()
        };

        step_vehicle_physics(&vehicle_input, &mut state, &terrain.generator, dt, driver.driver_id.is_some());
    }
}

/// Helper to convert PeerId to u64 for driver tracking
pub fn peer_id_to_u64(peer_id: PeerId) -> u64 {
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        PeerId::Entity(id) => id,
        PeerId::Raw(addr) => {
            // Hash the socket address to a u64
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            addr.hash(&mut hasher);
            hasher.finish()
        },
        PeerId::Server => 0,
    }
}

// =============================================================================
// DEATH & RESPAWN
// =============================================================================

/// Check for dead players and add respawn timer
pub fn check_player_deaths(
    mut commands: Commands,
    players: Query<(Entity, &Player, &Health), (Without<RespawnTimer>,)>,
    mut vehicles: Query<&mut VehicleDriver>,
) {
    for (entity, player, health) in players.iter() {
        if health.is_dead() {
            info!("Player {:?} died! Starting respawn timer", player.client_id);
            
            // Add respawn timer
            commands.entity(entity).insert(RespawnTimer {
                time_remaining: RESPAWN_TIME,
            });
            
            // Eject from any vehicle
            for mut driver in vehicles.iter_mut() {
                if driver.driver_id == Some(peer_id_to_u64(player.client_id)) {
                    driver.driver_id = None;
                }
            }
            
            // Remove InVehicle component if present
            commands.entity(entity).remove::<InVehicle>();
        }
    }
}

/// Tick respawn timers and respawn players when ready
pub fn tick_respawn_timers(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut players: Query<(Entity, &Player, &mut Health, &mut PlayerPosition, &mut PlayerVelocity, &mut RespawnTimer)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    
    for (entity, player, mut health, mut position, mut velocity, mut timer) in players.iter_mut() {
        timer.time_remaining -= dt;
        
        if timer.time_remaining <= 0.0 {
            info!("Respawning player {:?}", player.client_id);
            
            // Reset health
            health.current = health.max;
            
            // Reset position to spawn point
            let spawn_x = SPAWN_POSITION[0];
            let spawn_z = SPAWN_POSITION[2];
            let ground_y = terrain.generator.get_height(spawn_x, spawn_z);
            position.0 = Vec3::new(spawn_x, ground_y + ground_clearance_center(), spawn_z);
            
            // Reset velocity
            velocity.0 = Vec3::ZERO;
            
            // Remove respawn timer
            commands.entity(entity).remove::<RespawnTimer>();
        }
    }
}

/// Skip input processing for dead players
pub fn is_player_alive(health: &Health, respawn_timer: Option<&RespawnTimer>) -> bool {
    !health.is_dead() && respawn_timer.is_none()
}
