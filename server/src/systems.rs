//! Server-side game systems
//! 
//! Updated for Lightyear 0.25

use bevy::prelude::*;
use lightyear::prelude::*;
use lightyear::prelude::server::*;
use std::collections::HashMap;

use shared::{
    ground_clearance_center, step_character, step_vehicle_physics, step_car_physics, can_interact_with_vehicle,
    CarSuspensionState, Player, PlayerInput, PlayerPosition, PlayerRotation, PlayerVelocity, PlayerGrounded,
    Vehicle, VehicleState, VehicleDriver, VehicleInput, InVehicle, VehicleType,
    WorldTerrain, FIXED_TIMESTEP_HZ, SPAWN_POSITION,
    Health, EquippedWeapon, WeaponType,
    Inventory, HotbarSelection,
    PlayerProfile, SubmitPlayerName, NameSubmissionResult, NameRejectionReason,
    ReliableChannel,
};

use crate::inventory::PreviousHotbarSlot;
use crate::persistence::PlayerProfiles;

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

/// Handle new client connections - setup message channels
/// In Lightyear 0.25, we query for newly added ClientOf + Connected entities
/// Player spawning now happens in handle_player_name_submission after name is validated
pub fn handle_connections(
    mut commands: Commands,
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
        info!("Client connected: {:?} - awaiting player name submission", peer_id);

        // IMPORTANT: enable replication + message I/O on this client link.
        //
        // Lightyear 0.25 requires you to add these components to the connection entity
        // (the entity with `ClientOf` + `Connected`). Without them, no replication happens.
        //
        // Split into multiple inserts to avoid tuple size limit
        commands.entity(client_entity).insert((
            // Replication out: server -> this client
            ReplicationSender::new(shared::protocol::tick_duration(), SendUpdatesMode::SinceLastAck, false),
            // Client -> Server (gameplay messages)
            MessageReceiver::<PlayerInput>::default(),
            MessageReceiver::<shared::ShootRequest>::default(),
            MessageReceiver::<shared::SwitchWeapon>::default(),
            MessageReceiver::<shared::ReloadRequest>::default(),
            // Name submission
            MessageReceiver::<SubmitPlayerName>::default(),
        ));

        commands.entity(client_entity).insert((
            // Inventory messages
            MessageReceiver::<shared::PickupRequest>::default(),
            MessageReceiver::<shared::DropRequest>::default(),
            MessageReceiver::<shared::SelectHotbarSlot>::default(),
            MessageReceiver::<shared::InventoryMoveRequest>::default(),
            // Chest messages
            MessageReceiver::<shared::OpenChestRequest>::default(),
            MessageReceiver::<shared::CloseChestRequest>::default(),
            MessageReceiver::<shared::ChestTransferRequest>::default(),
            // Building messages
            MessageReceiver::<shared::PlaceBuildingRequest>::default(),
        ));

        commands.entity(client_entity).insert((
            // Server -> Client
            MessageSender::<shared::HitConfirm>::default(),
            MessageSender::<shared::DamageReceived>::default(),
            MessageSender::<shared::PlayerKilled>::default(),
            MessageSender::<shared::BulletImpact>::default(),
            MessageSender::<NameSubmissionResult>::default(),
        ));
    }
}

/// Handle player name submissions from clients
/// Validates name, loads/creates profile, spawns player entity
pub fn handle_player_name_submission(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    mut profiles: ResMut<PlayerProfiles>,
    mut client_links: Query<(Entity, &RemoteId, &mut MessageReceiver<SubmitPlayerName>, &mut MessageSender<NameSubmissionResult>), With<ClientOf>>,
    // Check if this peer already has a player spawned
    existing_players: Query<&Player>,
) {
    for (client_entity, remote_id, mut receiver, mut sender) in client_links.iter_mut() {
        let peer_id = remote_id.0;

        // Check if player already spawned for this peer (prevent duplicate spawns)
        if existing_players.iter().any(|p| p.client_id == peer_id) {
            continue;
        }

        for submission in receiver.receive() {
            let name = submission.name.trim().to_string();
            info!("Received name submission from {:?}: '{}'", peer_id, name);

            // Validate name
            if let Err(reason) = PlayerProfiles::validate_name(&name) {
                warn!("Name '{}' rejected: {:?}", name, reason);
                sender.send::<ReliableChannel>(NameSubmissionResult::Rejected { reason });
                continue;
            }

            // Check if name already online
            if profiles.is_name_online(&name) {
                warn!("Name '{}' rejected: already online", name);
                sender.send::<ReliableChannel>(NameSubmissionResult::Rejected {
                    reason: NameRejectionReason::AlreadyOnline
                });
                continue;
            }

            // Try to load existing profile or create new
            let name_lower = name.to_lowercase();
            let (profile, profile_loaded) = match profiles.load_profile(&name) {
                Ok(profile) => {
                    info!("Loaded existing profile for '{}'", name);
                    (profile, true)
                }
                Err(e) => {
                    info!("Creating new profile for '{}': {}", name, e);
                    (PlayerProfile::new_player(name.clone()), false)
                }
            };

            // Determine spawn state based on profile
            let (spawn_pos, spawn_rot, spawn_vel, health, equipped_weapon, weapon_ammo, inventory, hotbar_sel, vehicle_spawn): (Vec3, f32, Vec3, Health, EquippedWeapon, u32, Inventory, u8, Option<(VehicleType, [f32; 3], [f32; 3], [f32; 3], [f32; 3])>) =
                if profile.is_dead {
                    // Player died before disconnecting - force respawn at spawn point
                    // Items were already dropped on death, so spawn with empty inventory
                    info!("Player '{}' was dead - spawning at spawn point with empty inventory", name);
                    let spawn_x = SPAWN_POSITION[0];
                    let spawn_z = SPAWN_POSITION[2];
                    let ground_y = terrain.get_height(spawn_x, spawn_z);
                    let pos = Vec3::new(spawn_x, ground_y + ground_clearance_center(), spawn_z);

                    (
                        pos,
                        0.0,
                        Vec3::ZERO,
                        Health::default(),
                        EquippedWeapon::new(WeaponType::AssaultRifle),
                        30, // Default ammo
                        Inventory::new(), // Empty inventory - items were dropped on death
                        0,
                        None, // Not in vehicle
                    )
                } else if profile.in_vehicle {
                    // Player was in a vehicle - spawn inside vehicle
                    info!("Player '{}' was in vehicle - restoring vehicle state", name);

                    let veh_pos = profile.vehicle_position.unwrap_or(profile.position);
                    let veh_rot = profile.vehicle_rotation.unwrap_or([profile.rotation, 0.0, 0.0]);
                    let veh_vel = profile.vehicle_velocity.unwrap_or([0.0, 0.0, 0.0]);
                    let veh_ang_vel = profile.vehicle_angular_velocity.unwrap_or([0.0, 0.0, 0.0]);
                    let veh_type = profile.vehicle_type.unwrap_or(VehicleType::Motorbike);

                    // Reconstruct inventory from saved slots
                    let mut inventory = Inventory::new();
                    for (i, slot) in profile.inventory_slots.iter().enumerate() {
                        if let Some(stack) = slot {
                            let _ = inventory.set_slot(i, Some(*stack));
                        }
                    }

                    (
                        Vec3::from_slice(&veh_pos),
                        veh_rot[0], // heading
                        Vec3::ZERO, // Player velocity is zero (vehicle handles movement)
                        Health { current: profile.health_current, max: profile.health_max },
                        EquippedWeapon::new(profile.equipped_weapon),
                        profile.weapon_ammo_in_mag,
                        inventory,
                        profile.hotbar_selection,
                        Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)),
                    )
                } else {
                    // Normal spawn - restore saved position
                    info!("Player '{}' spawning at saved position {:?}", name, profile.position);

                    // Reconstruct inventory from saved slots
                    let mut inventory = Inventory::new();
                    for (i, slot) in profile.inventory_slots.iter().enumerate() {
                        if let Some(stack) = slot {
                            let _ = inventory.set_slot(i, Some(*stack));
                        }
                    }

                    (
                        Vec3::from_slice(&profile.position),
                        profile.rotation,
                        Vec3::from_slice(&profile.velocity),
                        Health { current: profile.health_current, max: profile.health_max },
                        EquippedWeapon::new(profile.equipped_weapon),
                        profile.weapon_ammo_in_mag,
                        inventory,
                        profile.hotbar_selection,
                        None,
                    )
                };

            // Set equipped weapon ammo
            let mut equipped_weapon_component = equipped_weapon;
            equipped_weapon_component.ammo_in_mag = weapon_ammo;

            // Spawn player entity
            let player_entity = commands.spawn((
                Player { client_id: peer_id },
                PlayerPosition(spawn_pos),
                PlayerRotation(spawn_rot),
                PlayerVelocity(spawn_vel),
                PlayerGrounded::default(),
                health,
                equipped_weapon_component,
                inventory,
                HotbarSelection { index: hotbar_sel },
                PreviousHotbarSlot { index: Some(hotbar_sel as usize) },
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ControlledBy {
                    owner: client_entity,
                    lifetime: Lifetime::default(),
                },
            )).id();

            // If spawning in vehicle, spawn/restore vehicle
            if let Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)) = vehicle_spawn {
                let vehicle_entity = commands.spawn((
                    Vehicle { vehicle_type: veh_type },
                    VehicleState {
                        position: Vec3::from_slice(&veh_pos),
                        heading: veh_rot[0],
                        pitch: veh_rot[1],
                        roll: veh_rot[2],
                        velocity: Vec3::from_slice(&veh_vel),
                        angular_velocity_yaw: veh_ang_vel[0],
                        angular_velocity_pitch: veh_ang_vel[1],
                        angular_velocity_roll: veh_ang_vel[2],
                        grounded: true, // Assume grounded when spawning (will be corrected on first physics tick)
                    },
                    VehicleDriver { driver_id: Some(peer_id_to_u64(peer_id)) },
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                )).id();

                // Link player to vehicle
                commands.entity(player_entity).insert(InVehicle {
                    vehicle_entity,
                });

                info!("Spawned vehicle {:?} for player '{}'", veh_type, name);
            }

            // Track in PlayerProfiles resource
            profiles.peer_to_name.insert(peer_id, name_lower.clone());
            profiles.name_to_peer.insert(name_lower.clone(), peer_id);
            profiles.profiles.insert(name_lower, profile);

            // Send acceptance message
            sender.send::<ReliableChannel>(NameSubmissionResult::Accepted { profile_loaded });
            info!("Player '{}' spawned successfully for {:?}", name, peer_id);
        }
    }
}

/// Save player state on disconnect
/// This is an observer that triggers when a client gets Disconnected component added
pub fn handle_disconnections(
    trigger: On<Add, Disconnected>,
    mut profiles: ResMut<PlayerProfiles>,
    client_entities: Query<&RemoteId>,
    players: Query<(
        Entity,
        &Player,
        &PlayerPosition,
        &PlayerRotation,
        &PlayerVelocity,
        &Health,
        &EquippedWeapon,
        &Inventory,
        &HotbarSelection,
        Option<&InVehicle>,
        Option<&RespawnTimer>,
    )>,
    mut vehicles: Query<(&mut VehicleDriver, &VehicleState, &Vehicle)>,
    mut inputs: ResMut<ClientInputs>,
) {
    let client_entity = trigger.entity;

    // Get peer ID from client entity
    let peer_id = if let Ok(remote_id) = client_entities.get(client_entity) {
        remote_id.0
    } else {
        warn!("Disconnect trigger for entity {:?} but no RemoteId found", client_entity);
        return;
    };

    info!("PLAYER LEFT GAME - Client {:?} disconnected: {:?}", client_entity, peer_id);

    // Get player name from tracking
    let name_lower = if let Some(name) = profiles.peer_to_name.get(&peer_id) {
        name.clone()
    } else {
        warn!("Player {:?} disconnected but no name found in tracking - cannot save", peer_id);
        return;
    };

    info!("Saving state for player '{}'", name_lower);

    // Find player entity for this peer
    let mut found_player = None;
    for (player_entity, player, pos, rot, vel, health, weapon, inventory, hotbar, in_vehicle, respawn_timer) in players.iter() {
        if player.client_id == peer_id {
            found_player = Some((player_entity, pos, rot, vel, health, weapon, inventory, hotbar, in_vehicle, respawn_timer));
            break;
        }
    }

    let Some((_player_entity, pos, rot, vel, health, weapon, inventory, hotbar, in_vehicle, respawn_timer)) = found_player else {
        warn!("Player entity not found for disconnected peer {:?} - state not saved!", peer_id);
        // Still free up the name even if we can't save
        profiles.peer_to_name.remove(&peer_id);
        profiles.name_to_peer.remove(&name_lower);
        inputs.latest.remove(&peer_id);
        return;
    };

    // Check if player is in a vehicle
    let (vehicle_data, in_veh) = if let Some(in_veh) = in_vehicle {
        if let Ok((mut driver, veh_state, vehicle)) = vehicles.get_mut(in_veh.vehicle_entity) {
            // Clear driver but don't despawn vehicle
            driver.driver_id = None;

            let veh_type = vehicle.vehicle_type;
            let veh_pos = [veh_state.position.x, veh_state.position.y, veh_state.position.z];
            let veh_rot = [veh_state.heading, veh_state.pitch, veh_state.roll];
            let veh_vel = [veh_state.velocity.x, veh_state.velocity.y, veh_state.velocity.z];
            let veh_ang_vel = [
                veh_state.angular_velocity_yaw,
                veh_state.angular_velocity_pitch,
                veh_state.angular_velocity_roll
            ];

            (Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)), true)
        } else {
            (None, false)
        }
    } else {
        (None, false)
    };

    // Capture player state
    let profile = PlayerProfile {
        version: shared::PROFILE_VERSION,
        player_name: name_lower.clone(), // Store lowercase

        // Position
        position: [pos.0.x, pos.0.y, pos.0.z],
        rotation: rot.0,
        velocity: [vel.0.x, vel.0.y, vel.0.z],

        // Combat
        health_current: health.current,
        health_max: health.max,
        equipped_weapon: weapon.weapon_type,
        weapon_ammo_in_mag: weapon.ammo_in_mag,

        // Inventory - copy all slots
        inventory_slots: *inventory.slots(),
        hotbar_selection: hotbar.index,

        // Vehicle state
        in_vehicle: in_veh,
        vehicle_type: vehicle_data.as_ref().map(|(vt, _, _, _, _)| *vt),
        vehicle_position: vehicle_data.as_ref().map(|(_, pos, _, _, _)| *pos),
        vehicle_rotation: vehicle_data.as_ref().map(|(_, _, rot, _, _)| *rot),
        vehicle_velocity: vehicle_data.as_ref().map(|(_, _, _, vel, _)| *vel),
        vehicle_angular_velocity: vehicle_data.as_ref().map(|(_, _, _, _, ang)| *ang),

        // Death state
        is_dead: respawn_timer.is_some() || health.is_dead(),
        death_timestamp: if respawn_timer.is_some() || health.is_dead() {
            Some(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64())
        } else {
            None
        },

        // Metadata
        last_login: std::time::SystemTime::now(),
        total_playtime_secs: profiles.profiles
            .get(&name_lower)
            .map(|p| p.total_playtime_secs)
            .unwrap_or(0), // TODO: Increment with actual playtime
    };

    // Save to disk
    if let Err(e) = profiles.save_profile(&profile) {
        error!("Failed to save profile for '{}': {}", name_lower, e);
    }

    // Update in-memory profile
    profiles.profiles.insert(name_lower.clone(), profile);

    info!("Successfully saved state for player '{}'", name_lower);

    // Clear any vehicles they were driving
    for (mut driver, _, _) in vehicles.iter_mut() {
        if driver.driver_id == Some(peer_id_to_u64(peer_id)) {
            driver.driver_id = None;
        }
    }

    // Remove from tracking
    profiles.peer_to_name.remove(&peer_id);
    profiles.name_to_peer.remove(&name_lower);
    info!("Freed up name '{}' for peer {:?}", name_lower, peer_id);

    inputs.latest.remove(&peer_id);
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
    mut players: Query<(&Player, &Health, &mut PlayerPosition, &mut PlayerRotation, &mut PlayerVelocity, &mut PlayerGrounded, Option<&InVehicle>, Option<&RespawnTimer>)>,
    vehicles: Query<&VehicleState>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (player, health, mut position, mut rotation, mut velocity, mut grounded, in_vehicle, respawn_timer) in players.iter_mut() {
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
                grounded.on_terrain = true; // Consider grounded while in vehicle
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
            &terrain,
            &mut position,
            &mut rotation,
            &mut velocity,
            &mut grounded,
            dt,
        );
    }
}

/// Simulate all vehicles
pub fn simulate_vehicles(
    terrain: Res<WorldTerrain>,
    inputs: Res<ClientInputs>,
    players: Query<&Player>,
    mut vehicles: Query<(&Vehicle, &VehicleDriver, &mut VehicleState, Option<&mut CarSuspensionState>)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (vehicle, driver, mut state, suspension) in vehicles.iter_mut() {
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

        match vehicle.vehicle_type {
            VehicleType::Car => {
                if let Some(mut suspension) = suspension {
                    step_car_physics(
                        &vehicle_input,
                        &mut state,
                        &mut suspension,
                        &terrain,
                        dt,
                        driver.driver_id.is_some(),
                        vehicle.vehicle_type,
                    );
                } else {
                    // Fallback to basic physics if suspension isn't present
                    step_vehicle_physics(
                        &vehicle_input,
                        &mut state,
                        &terrain,
                        dt,
                        driver.driver_id.is_some(),
                        vehicle.vehicle_type,
                    );
                }
            }
            _ => {
                step_vehicle_physics(
                    &vehicle_input,
                    &mut state,
                    &terrain,
                    dt,
                    driver.driver_id.is_some(),
                    vehicle.vehicle_type,
                );
            }
        }
    }
}

/// Ensure car vehicles have suspension state attached
pub fn ensure_car_suspension_state(
    mut commands: Commands,
    cars: Query<(Entity, &Vehicle), Without<CarSuspensionState>>,
) {
    for (entity, vehicle) in cars.iter() {
        if vehicle.vehicle_type == VehicleType::Car {
            commands.entity(entity).insert(CarSuspensionState::default());
        }
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
// PERIODIC AUTO-SAVE
// =============================================================================

/// How often to auto-save all players (seconds)
/// This is a safety backup - primary save happens on disconnect
const AUTO_SAVE_INTERVAL: f32 = 30.0;

/// Periodically save all connected players
pub fn periodic_player_save(
    profiles: Res<PlayerProfiles>,
    players: Query<(
        &Player,
        &PlayerPosition,
        &PlayerRotation,
        &PlayerVelocity,
        &Health,
        &EquippedWeapon,
        &Inventory,
        &HotbarSelection,
        Option<&InVehicle>,
        Option<&RespawnTimer>,
    )>,
    vehicles: Query<(&VehicleState, &Vehicle)>,
    time: Res<Time>,
    mut last_save_time: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last_save_time < AUTO_SAVE_INTERVAL {
        return;
    }

    *last_save_time = now;

    let mut saved_count = 0;
    for (player, pos, rot, vel, health, weapon, inventory, hotbar, in_vehicle, respawn_timer) in players.iter() {
        // Get player name from tracking
        let Some(name_lower) = profiles.peer_to_name.get(&player.client_id) else {
            continue;
        };

        // Check if player is in a vehicle
        let (vehicle_data, in_veh) = if let Some(in_veh) = in_vehicle {
            if let Ok((veh_state, vehicle)) = vehicles.get(in_veh.vehicle_entity) {
                let veh_type = vehicle.vehicle_type;
                let veh_pos = [veh_state.position.x, veh_state.position.y, veh_state.position.z];
                let veh_rot = [veh_state.heading, veh_state.pitch, veh_state.roll];
                let veh_vel = [veh_state.velocity.x, veh_state.velocity.y, veh_state.velocity.z];
                let veh_ang_vel = [
                    veh_state.angular_velocity_yaw,
                    veh_state.angular_velocity_pitch,
                    veh_state.angular_velocity_roll
                ];

                (Some((veh_type, veh_pos, veh_rot, veh_vel, veh_ang_vel)), true)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        // Capture player state
        let profile = PlayerProfile {
            version: shared::PROFILE_VERSION,
            player_name: name_lower.clone(),

            // Position
            position: [pos.0.x, pos.0.y, pos.0.z],
            rotation: rot.0,
            velocity: [vel.0.x, vel.0.y, vel.0.z],

            // Combat
            health_current: health.current,
            health_max: health.max,
            equipped_weapon: weapon.weapon_type,
            weapon_ammo_in_mag: weapon.ammo_in_mag,

            // Inventory
            inventory_slots: *inventory.slots(),
            hotbar_selection: hotbar.index,

            // Vehicle state
            in_vehicle: in_veh,
            vehicle_type: vehicle_data.as_ref().map(|(vt, _, _, _, _)| *vt),
            vehicle_position: vehicle_data.as_ref().map(|(_, pos, _, _, _)| *pos),
            vehicle_rotation: vehicle_data.as_ref().map(|(_, _, rot, _, _)| *rot),
            vehicle_velocity: vehicle_data.as_ref().map(|(_, _, _, vel, _)| *vel),
            vehicle_angular_velocity: vehicle_data.as_ref().map(|(_, _, _, _, ang)| *ang),

            // Death state
            is_dead: respawn_timer.is_some() || health.is_dead(),
            death_timestamp: if respawn_timer.is_some() || health.is_dead() {
                Some(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64())
            } else {
                None
            },

            // Metadata
            last_login: std::time::SystemTime::now(),
            total_playtime_secs: profiles.profiles
                .get(name_lower)
                .map(|p| p.total_playtime_secs)
                .unwrap_or(0),
        };

        // Save to disk
        if let Err(e) = profiles.save_profile(&profile) {
            error!("Auto-save failed for '{}': {}", name_lower, e);
        } else {
            saved_count += 1;
        }
    }

    if saved_count > 0 {
        info!("Auto-saved {} player profile(s)", saved_count);
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
            let ground_y = terrain.get_height(spawn_x, spawn_z);
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
