//! Player-related constants and types

/// Player movement speed (units per second)
pub const PLAYER_SPEED: f32 = 8.0;

/// Player height (for capsule)
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Player radius (for capsule)
pub const PLAYER_RADIUS: f32 = 0.3;

/// Mouse sensitivity for look
pub const MOUSE_SENSITIVITY: f32 = 0.003;

/// Spawn position for new players (spawn above terrain to prevent clipping)
pub const SPAWN_POSITION: [f32; 3] = [0.0, 10.0, 0.0];
