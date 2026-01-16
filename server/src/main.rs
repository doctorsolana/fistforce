//! Game Server - Headless Bevy app that manages the game world
//! 
//! Updated for Lightyear 0.25 / Bevy 0.17

mod building;
mod systems;
mod npc;
mod weapons;
mod world;
mod colliders;
mod inventory;
mod persistence;

use bevy::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use lightyear::prelude::*;
use lightyear::prelude::server::*;
// UDP/Netcode types re-exported through prelude::server (when features enabled)
use shared::{
    protocol::*, ProtocolPlugin, WorldTerrain,
    Vehicle, VehicleType, VehicleState, VehicleDriver,
    PRIVATE_KEY, PROTOCOL_ID, SERVER_PORT, get_server_bind_addr,
    SpatialObstacleGrid,
};
use std::net::{SocketAddr, ToSocketAddrs};

use systems::ClientInputs;
use persistence::PlayerProfiles;

/// Marker for our server entity
#[derive(Component)]
struct GameServer;

/// Tracks if vehicles have been spawned
#[derive(Resource)]
struct VehiclesSpawned;

/// Spawn the server entity with all required networking components
fn spawn_server(mut commands: Commands) {
    let bind_addr = get_server_bind_addr();
    // `fly-global-services` is a hostname on Fly.io, so we must resolve it instead of `parse()`.
    let server_addr: SocketAddr = (bind_addr, SERVER_PORT)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .expect("Invalid server bind address");
    
    info!("Spawning server entity, binding to {:?} (fly.io: {})", 
          server_addr, 
          std::env::var("FLY_APP_NAME").is_ok());
    
    // Spawn server entity with UDP + Netcode
    commands.spawn((
        GameServer,
        Server::default(),
        ServerUdpIo::default(),
        LocalAddr(server_addr),
        NetcodeServer::new(NetcodeConfig {
            protocol_id: PROTOCOL_ID,
            private_key: PRIVATE_KEY,
            ..default()
        }),
    ));
}

/// Start the server after it's spawned
fn start_server(
    mut commands: Commands,
    server_query: Query<Entity, (With<GameServer>, Without<Started>, Without<Starting>)>,
) {
    for server_entity in server_query.iter() {
        info!("Starting server...");
        // In Bevy 0.17 + Lightyear 0.25, trigger an EntityEvent
        commands.trigger(Start { entity: server_entity });
    }
}

/// Spawn vehicles once after server starts
fn spawn_vehicles_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<VehiclesSpawned>>,
    server_query: Query<Entity, (With<GameServer>, With<Started>)>,
) {
    // Only spawn if server is started and we haven't spawned yet
    if spawned.is_some() || server_query.is_empty() {
        return;
    }
    
    commands.insert_resource(VehiclesSpawned);
    
    // Spawn two test motorbikes near the spawn point - drop them from 5 meters!
    let bike_positions = [(5.0, 5.0), (8.0, 5.0)]; // Two bikes side by side
    
    for (bike_x, bike_z) in bike_positions {
        let ground_y = terrain.get_height(bike_x, bike_z);
        let spawn_height = ground_y + 5.0;
        
        commands.spawn((
            Vehicle { vehicle_type: VehicleType::Motorbike },
            VehicleState {
                position: Vec3::new(bike_x, spawn_height, bike_z),
                velocity: Vec3::ZERO,
                heading: 0.0,
                pitch: 0.0,
                roll: 0.0,
                angular_velocity_yaw: 0.0,
                angular_velocity_pitch: 0.0,
                angular_velocity_roll: 0.0,
                grounded: false,
            },
            VehicleDriver { driver_id: None },
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));

        info!("Spawned motorbike at ({}, {}) - dropping from height {}!", bike_x, bike_z, spawn_height);
    }

    // Car removed - physics needs rework
}

/// Check if server is started (run condition)
fn server_is_started(server_query: Query<(), (With<GameServer>, With<Started>)>) -> bool {
    !server_query.is_empty()
}

fn main() {
    let mut app = App::new();

    // Headless plugins (no rendering)
    // IMPORTANT: run the main loop at the same rate as our fixed tick.
    //
    // If the headless app runs "as fast as possible", Bevy will clear `MessageReceiver` buffers every
    // frame (in `Last`), but our gameplay systems read messages in `FixedUpdate`.
    // When frames >> fixed ticks, most input/shoot messages get cleared before `FixedUpdate` runs,
    // resulting in stuck movement and missing shots.
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick_duration())));
    app.add_plugins(bevy::log::LogPlugin::default());
    app.add_plugins(bevy::state::app::StatesPlugin);

    // Deterministic world terrain (used for authoritative ground collision)
    // Includes terrain modifications (building flattening, etc.)
    app.init_resource::<WorldTerrain>();

    // Server-side input cache
    app.init_resource::<ClientInputs>();

    // Chest open tracking
    app.init_resource::<inventory::OpenChests>();

    // Delta chunk entity tracking for terrain modifications
    app.init_resource::<building::DeltaChunkEntities>();

    // Spatial grid for O(1) obstacle lookups (used by NPC AI pathfinding)
    app.init_resource::<SpatialObstacleGrid>();
    app.init_resource::<npc::ObstacleGridState>();

    // Player profile persistence
    app.insert_resource(PlayerProfiles::new(
        std::path::PathBuf::from("server_data/players")
    ));

    // Lightyear server plugins (tick_duration = 60Hz)
    app.add_plugins(ServerPlugins {
        tick_duration: tick_duration(),
    });
    
    // Protocol plugin (component/message registration)
    app.add_plugins(ProtocolPlugin);

    // Game systems
    app.add_systems(Startup, (world::setup_world, colliders::load_baked_colliders, spawn_server));

    // Start server after spawning
    app.add_systems(Update, start_server);

    // Disconnect handler - uses Bevy observer to trigger on LinkOf removal
    app.add_observer(systems::handle_disconnections);
    
    // Spawn WorldTime after server is started
    app.add_systems(Update, world::spawn_world_time_once.run_if(server_is_started));

    // Spawn vehicles after server is started
    app.add_systems(Update, spawn_vehicles_once);
    // Spawn NPCs after server is started
    app.add_systems(Update, npc::spawn_npcs_once);
    // Spawn test items after server is started
    app.add_systems(Update, inventory::spawn_test_items.run_if(server_is_started));
    // Spawn test building after server is started
    app.add_systems(Update, building::spawn_test_building.run_if(server_is_started));
    // Spawn medieval town after server is started
    app.add_systems(Update, building::spawn_medieval_town.run_if(server_is_started));

    // Fixed tick: receive inputs, handle interactions, then simulate everyone.
    // Split into multiple system groups to avoid tuple limit
    app.add_systems(
        FixedUpdate,
        (
            // World time (day/night cycle)
            world::tick_world_time,
            // Static collider streaming (keep colliders near active players)
            colliders::stream_static_colliders,
            colliders::invalidate_colliders_for_new_buildings,
            // Structure collider streaming (desert settlements)
            colliders::stream_structure_colliders,
            systems::ensure_car_suspension_state,
            systems::handle_connections,
            systems::handle_player_name_submission,
            systems::receive_client_input,
            systems::handle_vehicle_interactions,
            systems::simulate_vehicles,
            systems::simulate_players,
            // Death & respawn
            systems::check_player_deaths,
            systems::tick_respawn_timers,
            // Auto-save
            systems::periodic_player_save,
        )
            .chain()
            .run_if(server_is_started),
    );
    
    app.add_systems(
        FixedUpdate,
        (
            // Spatial grid sync (O(1) obstacle lookups for pathfinding)
            npc::sync_obstacle_grid,
            // NPC AI - damage reaction before AI tick
            npc::react_to_damage,
            npc::tick_npc_ai,
            // Dead NPC cleanup (add despawn timer, tick timer and despawn)
            npc::add_despawn_timer_to_dead_npcs,
            npc::tick_dead_npc_despawn_timers,
            // World prop collisions (server-authoritative)
            colliders::resolve_vehicle_static_collisions,
            colliders::resolve_player_static_collisions,
            colliders::resolve_npc_static_collisions,
            // Inventory / hotbar (server-authoritative)
            inventory::handle_hotbar_selection_requests,
            inventory::handle_inventory_move_requests,
            inventory::handle_pickup_requests,
            inventory::handle_drop_requests,
            inventory::sync_equipped_weapon_from_hotbar,
            // Chest / storage (server-authoritative)
            inventory::handle_open_chest_requests,
            inventory::handle_close_chest_requests,
            inventory::handle_chest_transfer_requests,
            inventory::auto_close_distant_chests,
            // Building placement (server-authoritative)
            building::handle_place_building_requests,
        )
            .chain()
            .run_if(server_is_started),
    );
    
    app.add_systems(
        FixedUpdate,
        (
            // Weapon systems
            weapons::handle_shoot_requests,
            weapons::handle_reload_request,
            weapons::simulate_bullets,
            weapons::detect_bullet_hits,
            weapons::detect_bullet_world_hits,
            weapons::cleanup_bullets,
            // Inventory death
            inventory::drop_inventory_on_death,
        )
            .chain()
            .run_if(server_is_started),
    );

    info!("Starting server on port {}", SERVER_PORT);
    app.run();
}
