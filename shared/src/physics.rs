//! Shared character-controller style physics.
//!
//! Goals:
//! - Server-authoritative simulation (single source of truth)
//! - Deterministic ground collision against the terrain (includes modifications)
//! - Runs at a fixed timestep (see `FIXED_TIMESTEP_HZ`)
//!
//! This is intentionally lightweight (height-sampled terrain) so it scales well for open worlds.
//! We can add full rigidbody physics (Rapier/Avian) later for dynamic objects.

use bevy::prelude::*;

use crate::{
    terrain::WorldTerrain, PlayerInput, PlayerPosition, PlayerRotation, PlayerVelocity,
    PlayerGrounded, PLAYER_HEIGHT, PLAYER_SPEED,
};

/// Gravity in m/s^2 (negative Y).
/// Slightly stronger than real-world for snappier game feel.
pub const GRAVITY: f32 = -18.0;

/// Horizontal acceleration in m/s^2.
pub const MOVE_ACCEL: f32 = 45.0;

/// Horizontal deceleration when no input ("friction") in m/s^2.
pub const MOVE_BRAKE: f32 = 55.0;

/// How close to the ground we "snap" when falling (prevents tiny hovering).
pub const GROUND_SNAP_DISTANCE: f32 = 0.35;

/// Jump velocity in m/s (upward).
/// ~5.5 m/s gives a jump height of roughly 0.84m with GRAVITY=-18.
pub const JUMP_VELOCITY: f32 = 7.5;

/// Minimum Y for the capsule center above ground.
#[inline]
pub fn ground_clearance_center() -> f32 {
    PLAYER_HEIGHT * 0.5
}

/// Step the player character one fixed tick.
///
/// - Updates rotation from input yaw
/// - Applies acceleration/braking on the XZ plane
/// - Applies gravity
/// - Collides against the terrain ground (includes any modifications like building flattening)
/// - Updates grounded state based on terrain contact
pub fn step_character(
    input: &PlayerInput,
    terrain: &WorldTerrain,
    position: &mut PlayerPosition,
    rotation: &mut PlayerRotation,
    velocity: &mut PlayerVelocity,
    grounded: &mut PlayerGrounded,
    dt: f32,
) {
    // --- Facing ---
    rotation.0 = input.yaw;

    // --- Desired horizontal movement ---
    // In Bevy: +X right, +Y up, -Z forward.
    let forward = Vec3::new(-rotation.0.sin(), 0.0, -rotation.0.cos());
    let right = Vec3::new(rotation.0.cos(), 0.0, -rotation.0.sin());

    let mut move_dir = Vec3::ZERO;
    if input.forward {
        move_dir += forward;
    }
    if input.backward {
        move_dir -= forward;
    }
    if input.right {
        move_dir += right;
    }
    if input.left {
        move_dir -= right;
    }

    if move_dir.length_squared() > 0.0 {
        move_dir = move_dir.normalize();
    }

    let desired_horiz = move_dir * PLAYER_SPEED;
    let mut horiz = Vec3::new(velocity.0.x, 0.0, velocity.0.z);

    // Accelerate toward desired velocity.
    let delta = desired_horiz - horiz;
    let accel = if move_dir.length_squared() > 0.0 {
        MOVE_ACCEL
    } else {
        MOVE_BRAKE
    };
    let max_change = accel * dt;

    if delta.length_squared() > 0.0 {
        let delta_len = delta.length();
        if delta_len <= max_change {
            horiz = desired_horiz;
        } else {
            horiz += delta * (max_change / delta_len);
        }
    }

    velocity.0.x = horiz.x;
    velocity.0.z = horiz.z;

    // --- Jump ---
    // Use grounded component (set by collision system last tick) with coyote time
    if input.jump && grounded.can_jump() && velocity.0.y < 1.0 {
        velocity.0.y = JUMP_VELOCITY;
        // Reset coyote timer after jumping
        grounded.time_since_grounded = PlayerGrounded::COYOTE_TIME;
    }

    // --- Gravity ---
    velocity.0.y += GRAVITY * dt;

    // --- Integrate ---
    position.0 += velocity.0 * dt;

    // --- Ground collision (heightfield) ---
    // Re-sample ground in case we moved horizontally
    let ground_y = terrain.get_height(position.0.x, position.0.z);
    let target_y = ground_y + ground_clearance_center();

    // Track if we're on terrain this tick
    let mut on_terrain_now = false;

    // Snap if we are below ground, or very close and falling.
    if position.0.y < target_y {
        position.0.y = target_y;
        if velocity.0.y < 0.0 {
            velocity.0.y = 0.0;
        }
        on_terrain_now = true;
    } else if velocity.0.y <= 0.0 && (position.0.y - target_y) < GROUND_SNAP_DISTANCE {
        position.0.y = target_y;
        velocity.0.y = 0.0;
        on_terrain_now = true;
    }
    
    // Update grounded state
    grounded.on_terrain = on_terrain_now;
    
    // Update coyote timer
    if grounded.is_grounded() {
        grounded.time_since_grounded = 0.0;
    } else {
        grounded.time_since_grounded += dt;
    }
}
