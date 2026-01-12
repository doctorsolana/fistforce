//! Game Server - Headless Bevy app that manages the game world
//! 
//! Updated for Lightyear 0.25 / Bevy 0.17

mod systems;
mod npc;
mod weapons;
mod world;
mod colliders;

use bevy::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use lightyear::prelude::*;
use lightyear::prelude::server::*;
// UDP/Netcode types re-exported through prelude::server (when features enabled)
use shared::{
    protocol::*, ProtocolPlugin, WorldTerrain, 
    Vehicle, VehicleType, VehicleState, VehicleDriver,
    PRIVATE_KEY, PROTOCOL_ID, SERVER_PORT, get_server_bind_addr,
};
use std::net::SocketAddr;

use systems::ClientInputs;

/// Marker for our server entity
#[derive(Component)]
struct GameServer;

/// Tracks if vehicles have been spawned
#[derive(Resource)]
struct VehiclesSpawned;

/// Spawn the server entity with all required networking components
fn spawn_server(mut commands: Commands) {
    let bind_addr = get_server_bind_addr();
    let server_addr: SocketAddr = format!("{}:{}", bind_addr, SERVER_PORT)
        .parse()
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
    
    // Spawn a test motorbike near the spawn point - drop it from 5 meters!
    let bike_x = 5.0;
    let bike_z = 5.0;
    let ground_y = terrain.generator.get_height(bike_x, bike_z);
    
    // Spawn HIGH above ground so it visibly falls
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

    info!("Spawned test motorbike at ({}, {}) - dropping from height {}!", bike_x, bike_z, spawn_height);
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
    app.init_resource::<WorldTerrain>();

    // Server-side input cache
    app.init_resource::<ClientInputs>();

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
    
    // Spawn WorldTime after server is started
    app.add_systems(Update, world::spawn_world_time_once.run_if(server_is_started));

    // Spawn vehicles after server is started
    app.add_systems(Update, spawn_vehicles_once);
    // Spawn NPCs after server is started
    app.add_systems(Update, npc::spawn_npcs_once);

    // Fixed tick: receive inputs, handle interactions, then simulate everyone.
    app.add_systems(
        FixedUpdate,
        (
            // World time (day/night cycle)
            world::tick_world_time,
            // Static collider streaming (keep colliders near active players)
            colliders::stream_static_colliders,
            // Structure collider streaming (desert settlements)
            colliders::stream_structure_colliders,
            systems::handle_connections,
            systems::handle_disconnections,
            systems::receive_client_input,
            systems::handle_vehicle_interactions,
            systems::simulate_vehicles,
            systems::simulate_players,
            // NPC AI
            npc::tick_npc_ai,
            // World prop collisions (server-authoritative)
            colliders::resolve_vehicle_static_collisions,
            colliders::resolve_player_static_collisions,
            colliders::resolve_npc_static_collisions,
            // Weapon systems
            weapons::handle_shoot_requests,
            weapons::handle_weapon_switch,
            weapons::handle_reload_request,
            weapons::simulate_bullets,
            weapons::detect_bullet_hits,
            weapons::detect_bullet_world_hits,
            weapons::cleanup_bullets,
        )
            .chain()
            .run_if(server_is_started),
    );

    info!("Starting server on port {}", SERVER_PORT);
    app.run();
}
