//! Shared movement logic used by both client (prediction) and server (authority)

use bevy::prelude::*;
use crate::{PlayerInput, PlayerPosition, PlayerRotation, PLAYER_SPEED};

/// Apply movement input to update player position
/// This function is used by both server and client for consistent physics
pub fn apply_movement(
    input: &PlayerInput,
    position: &mut PlayerPosition,
    rotation: &mut PlayerRotation,
    delta_seconds: f32,
) {
    // Update rotation from input
    rotation.0 = input.yaw;

    // Calculate movement direction based on input and rotation
    // In Bevy: +X is right, +Y is up, -Z is forward
    let mut direction = Vec3::ZERO;

    let forward = Vec3::new(-rotation.0.sin(), 0.0, -rotation.0.cos());
    let right = Vec3::new(rotation.0.cos(), 0.0, -rotation.0.sin());

    if input.forward {
        direction += forward;
    }
    if input.backward {
        direction -= forward;
    }
    if input.right {
        direction += right;
    }
    if input.left {
        direction -= right;
    }

    // Normalize and apply movement
    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        position.0 += direction * PLAYER_SPEED * delta_seconds;
    }
}
