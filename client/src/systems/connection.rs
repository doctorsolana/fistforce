//! Connection systems
//!
//! Networking, connection handling, cursor management, and menu transitions.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use lightyear::prelude::*;
use lightyear::prelude::client::*;
use shared::{Player, Vehicle, PRIVATE_KEY, PROTOCOL_ID};
use std::net::SocketAddr;

use crate::states::GameState;
use crate::terrain::LoadedChunks;
use crate::ui::ServerAddress;
use super::particles::SandParticle;
use super::world::ClientWorldRoot;
use shared::Npc;

// =============================================================================
// CONNECTION
// =============================================================================

/// Start connection to server
/// In Lightyear 0.25, we spawn a Client entity with the appropriate networking components
/// and then trigger the Connect event to initiate the connection
pub fn start_connection(
    mut commands: Commands,
    existing_clients: Query<Entity, With<crate::GameClient>>,
    server_address: Res<ServerAddress>,
) {
    info!("Initiating connection to server at {}:{}...", server_address.ip, server_address.port);

    // Ensure we only ever have ONE GameClient entity.
    // If we keep spawning new ones on each connect attempt, `Query::single()` calls
    // will start failing and gameplay (inputs/weapons/etc) silently stops working.
    for e in existing_clients.iter() {
        commands.entity(e).despawn();
    }
    
    let server_addr: SocketAddr = format!("{}:{}", server_address.ip, server_address.port)
        .parse()
        .expect("Invalid server address");
    let local_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
    
    // Generate a unique client ID
    let client_id = rand::random::<u64>();
    
    // Build authentication (netcode connect token)
    let auth = Authentication::Manual {
        server_addr,
        protocol_id: PROTOCOL_ID,
        private_key: PRIVATE_KEY,
        client_id,
    };
    
    // Spawn client entity with UDP + Netcode
    let client_entity = commands.spawn((
        crate::GameClient,
        Client::default(),
        UdpIo::default(),
        LocalAddr(local_addr),
        PeerAddr(server_addr),
        NetcodeClient::new(auth, NetcodeConfig::default()).expect("Failed to create netcode client"),
        // IMPORTANT: enable replication receive on this client.
        // Without this, the client will never receive `WorldTime` / `Player` / `Vehicle` / etc.
        ReplicationReceiver::default(),
        // Gameplay messages (explicitly added to avoid "silent no-op" if required-components don't apply
        // the way we expect across plugin ordering / connect timing).
        //
        // Client -> Server
        MessageSender::<shared::PlayerInput>::default(),
        MessageSender::<shared::ShootRequest>::default(),
        MessageSender::<shared::SwitchWeapon>::default(),
        MessageSender::<shared::ReloadRequest>::default(),
        // Server -> Client
        MessageReceiver::<shared::HitConfirm>::default(),
        MessageReceiver::<shared::BulletImpact>::default(),
        MessageReceiver::<shared::DamageReceived>::default(),
        MessageReceiver::<shared::PlayerKilled>::default(),
    )).id();
    
    // Trigger the Connect event to actually initiate the connection
    commands.trigger(Connect { entity: client_entity });
    
    info!("Client entity spawned, client_id: {}", client_id);
}

/// Check connection status
/// In Lightyear 0.25, we query for Connected/Disconnected components on the client entity
pub fn check_connection(
    mut next_state: ResMut<NextState<GameState>>,
    new_connections: Query<Entity, (With<crate::GameClient>, Added<Connected>)>,
    new_disconnections: Query<Entity, (With<crate::GameClient>, Added<Disconnected>)>,
) {
    for _entity in new_connections.iter() {
        info!("Connected to server!");
        next_state.set(GameState::Playing);
    }

    for _entity in new_disconnections.iter() {
        warn!("Connection failed or disconnected");
        next_state.set(GameState::MainMenu);
    }
}

// =============================================================================
// CURSOR
// =============================================================================

/// Grab cursor for FPS controls
pub fn grab_cursor(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
    mouse_button: Res<ButtonInput<MouseButton>>,
) {
    let Ok(window_entity) = windows.single() else {
        return;
    };

    if mouse_button.just_pressed(MouseButton::Left) {
        if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    }
}

// =============================================================================
// MENU TRANSITIONS
// =============================================================================

/// Entering the main menu: cleanup
pub fn enter_main_menu(
    mut commands: Commands,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
    world_roots: Query<Entity, With<ClientWorldRoot>>,
    players: Query<Entity, With<Player>>,
    npcs: Query<Entity, With<Npc>>,
    vehicles: Query<Entity, With<Vehicle>>,
    particles: Query<Entity, With<SandParticle>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
) {
    // Release cursor when entering main menu
    if let Ok(window_entity) = windows.single() {
        if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    for root in world_roots.iter() {
        commands.entity(root).despawn();
    }

    for entity in players.iter() {
        commands.entity(entity).despawn();
    }

    for entity in npcs.iter() {
        commands.entity(entity).despawn();
    }

    for entity in vehicles.iter() {
        commands.entity(entity).despawn();
    }

    // Clean up particles
    for entity in particles.iter() {
        commands.entity(entity).despawn();
    }

    loaded_chunks.chunks.clear();
    commands.insert_resource(ClearColor(Color::BLACK));
}
