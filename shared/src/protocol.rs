//! Lightyear network protocol definition
//! 
//! Updated for Lightyear 0.25 - merged entity model

use bevy::prelude::*;
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::components::{
    Npc, NpcPosition, NpcRotation, Player, PlayerPosition, PlayerRotation, Health, EquippedWeapon,
    Bullet, BulletVelocity, WorldTime,
};
use crate::vehicle::{Vehicle, VehicleState, VehicleDriver, VehicleInput};
use crate::weapons::damage::HitZone;

// --- Input (for server-authoritative movement) ---

/// Player input sent from client to server each tick
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct PlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    /// Jump request (spacebar)
    pub jump: bool,
    /// Player's facing direction (yaw) for movement calculation
    pub yaw: f32,
    /// If in a vehicle, this contains the vehicle input
    pub vehicle_input: Option<VehicleInput>,
    /// Request to enter/exit vehicle
    pub interact: bool,
}

// --- Messages ---

/// Message sent from client when they want to spawn
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnPlayer;

/// Message sent from client to request firing a weapon
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ShootRequest {
    /// Normalized aim direction in world space
    pub direction: Vec3,
    /// Player's pitch for aiming
    pub pitch: f32,
    /// Whether aiming down sights
    pub aiming: bool,
}

/// Message sent from server to confirm a hit
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct HitConfirm {
    /// ID of the player that was hit
    pub target_id: u64,
    /// Damage dealt
    pub damage: f32,
    /// Was it a headshot
    pub headshot: bool,
    /// Did it kill the target
    pub kill: bool,
    /// Hit zone
    pub hit_zone: HitZone,
}

/// Message sent from server when player takes damage
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DamageReceived {
    /// Direction damage came from (for hit indicator)
    pub direction: Vec3,
    /// Damage amount
    pub damage: f32,
    /// Current health after damage
    pub health_remaining: f32,
}

/// Message sent from server when player dies
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PlayerKilled {
    /// ID of player who killed us
    pub killer_id: u64,
    /// Weapon used
    pub weapon: crate::weapons::WeaponType,
    /// Was it a headshot
    pub headshot: bool,
}

/// Message sent from client to switch weapons
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SwitchWeapon {
    /// Weapon type to switch to
    pub weapon_type: crate::weapons::WeaponType,
}

/// Message sent from client to reload current weapon
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReloadRequest;

/// What the bullet impacted (used for visuals/debug)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum BulletImpactSurface {
    Terrain,
    PracticeWall,
    Player,
    Npc,
}

/// Server -> Client: bullet impact (reliable visual feedback independent of bullet replication)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BulletImpact {
    pub owner_id: u64,
    pub weapon_type: crate::weapons::WeaponType,
    pub spawn_position: Vec3,
    pub initial_velocity: Vec3,
    pub impact_position: Vec3,
    pub impact_normal: Vec3,
    pub surface: BulletImpactSurface,
}

// --- Channels ---
// In Lightyear 0.25, Channel trait is auto-implemented for all Send + Sync + 'static types

/// Reliable channel for important messages
pub struct ReliableChannel;

/// Unreliable channel for frequent input (lowest latency)
pub struct InputChannel;

// --- Protocol Plugin ---

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // === PLAYER COMPONENTS ===
        // In Lightyear 0.25, register_component no longer takes ChannelDirection
        // and add_prediction no longer takes ComponentSyncMode
        
        app.register_component::<Player>()
            .add_prediction();

        app.register_component::<PlayerPosition>()
            .add_prediction();

        app.register_component::<PlayerRotation>()
            .add_prediction();

        // === NPC COMPONENTS ===
        app.register_component::<Npc>()
            .add_prediction();

        app.register_component::<NpcPosition>()
            .add_prediction();

        app.register_component::<NpcRotation>()
            .add_prediction();

        // === VEHICLE COMPONENTS ===
        
        app.register_component::<Vehicle>()
            .add_prediction();

        app.register_component::<VehicleState>()
            .add_prediction();

        app.register_component::<VehicleDriver>()
            .add_prediction();

        // === COMBAT COMPONENTS ===
        
        app.register_component::<Health>()
            .add_prediction();

        app.register_component::<EquippedWeapon>()
            .add_prediction();

        // === BULLET COMPONENTS ===
        
        app.register_component::<Bullet>()
            .add_prediction();

        app.register_component::<BulletVelocity>()
            .add_prediction();

        // === WORLD COMPONENTS ===
        app.register_component::<WorldTime>()
            .add_prediction();

        // === MESSAGES ===
        // In Lightyear 0.25, messages are registered for (de)serialization AND we can declare
        // their network direction so the correct MessageSender/MessageReceiver components are
        // auto-added to Client / ClientOf entities via required components.
        
        // Client -> Server
        app.register_message::<SpawnPlayer>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<PlayerInput>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ShootRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<SwitchWeapon>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<ReloadRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        
        // Server -> Client
        app.register_message::<HitConfirm>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<BulletImpact>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<DamageReceived>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<PlayerKilled>()
            .add_direction(NetworkDirection::ServerToClient);

        // === CHANNELS ===
        
        app.add_channel::<ReliableChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        // Used for most gameplay messages (shooting, hit confirms, etc.)
        .add_direction(NetworkDirection::Bidirectional);

        app.add_channel::<InputChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedUnreliable,
            ..default()
        })
        // High-frequency input: client -> server only
        .add_direction(NetworkDirection::ClientToServer);
    }
}

// --- Network Configuration ---

pub const SERVER_PORT: u16 = 5000;
pub const SERVER_ADDR: &str = "127.0.0.1";
pub const PROTOCOL_ID: u64 = 0x1234567890ABCDEF;

/// Get the address the server should bind to.
/// Server bind address - 0.0.0.0 works for both local and Fly.io deployments.
/// Fly.io's proxy handles UDP routing automatically.
pub fn get_server_bind_addr() -> &'static str {
    "0.0.0.0"
}

/// Shared private key for local development (use proper key management in production!)
pub const PRIVATE_KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
    0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// Fixed timestep for physics/game logic (60 Hz)
pub const FIXED_TIMESTEP_HZ: f64 = 60.0;

/// Tick duration for lightyear plugins
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ)
}
