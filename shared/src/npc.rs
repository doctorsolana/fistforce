//! NPC-related constants and helper utilities.

use bevy::prelude::*;

use crate::player::{PLAYER_HEIGHT, PLAYER_RADIUS};

/// NPC capsule height (shared humanoid rig).
pub const NPC_HEIGHT: f32 = PLAYER_HEIGHT;

/// NPC capsule radius.
pub const NPC_RADIUS: f32 = PLAYER_RADIUS;

/// Head hitbox radius.
pub const NPC_HEAD_RADIUS: f32 = 0.18;

/// Returns the approximate head hitbox center for an upright NPC.
///
/// `npc_center` is the NPC's capsule *center* position.
#[inline]
pub fn npc_head_center(npc_center: Vec3) -> Vec3 {
    // Slightly below the capsule top.
    npc_center + Vec3::new(0.0, NPC_HEIGHT * 0.5 - NPC_HEAD_RADIUS, 0.0)
}

/// Returns endpoints (sphere centers) for an upright capsule representing the NPC body.
///
/// `npc_center` is the NPC's capsule *center* position.
#[inline]
pub fn npc_capsule_endpoints(npc_center: Vec3) -> (Vec3, Vec3) {
    let half = NPC_HEIGHT * 0.5;
    let a = npc_center + Vec3::new(0.0, -(half - NPC_RADIUS), 0.0);
    let b = npc_center + Vec3::new(0.0, half - NPC_RADIUS, 0.0);
    (a, b)
}

