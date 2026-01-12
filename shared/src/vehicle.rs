//! Vehicle physics system - PURE PHYSICS approach
//!
//! No artificial limits. Physics handles everything:
//! - Can't climb walls because: steep slope = less normal force = less traction + gravity pulls back
//! - Gets air off crests because: when ground drops away, you're airborne
//! - Slides on steep slopes because: gravity component along slope > available traction

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::terrain::{Biome, TerrainGenerator};

// =============================================================================
// VEHICLE TUNING CONSTANTS
// =============================================================================

pub mod motorbike {
    pub const MASS: f32 = 180.0;
    pub const ENGINE_FORCE: f32 = 7500.0;      // Newtons (boosted for higher top speed)
    pub const MAX_SPEED: f32 = 45.0;           // m/s (~162 km/h) - FAST speeder!
    pub const BRAKE_FORCE: f32 = 8000.0;       // Newtons (stronger brakes for faster bike)
    pub const DRAG_COEFFICIENT: f32 = 0.18;    // Lower drag for speed
    pub const ROLLING_RESISTANCE: f32 = 35.0;  // Ground friction when rolling
    
    /// Engine braking when not on throttle (in-gear)
    pub const ENGINE_BRAKE_DRIVER: f32 = 200.0;
    /// Weaker when unmanned (neutral/parked)
    pub const ENGINE_BRAKE_NO_DRIVER: f32 = 50.0;
    
    pub const STEERING_SPEED_MIN: f32 = 2.5;   // rad/s at low speed
    pub const STEERING_SPEED_MAX: f32 = 1.0;   // rad/s at max speed
    pub const STEERING_RESPONSE: f32 = 12.0;
    
    pub const TURN_LEAN_ANGLE: f32 = 0.35;     // radians
    pub const LEAN_SPEED: f32 = 8.0;
    
    /// Friction coefficient (mu) for different surfaces
    pub const MU_DESERT: f32 = 0.75;
    pub const MU_GRASSLANDS: f32 = 0.9;
    
    /// Lateral (sideways) friction - lower = more drift
    pub const LATERAL_FRICTION: f32 = 8.0;
    /// How much lateral speed to preserve on landing (0 = none, 1 = all)
    pub const LANDING_DRIFT_PRESERVE: f32 = 0.85;
    
    pub const SIZE: (f32, f32, f32) = (2.2, 1.0, 0.8);
    pub const GRAVITY: f32 = -20.0;  // m/s^2 (slightly stronger for game feel)

    /// Air control (only with Shift held)
    pub const AIR_PITCH_TORQUE: f32 = 8.0;
    pub const AIR_ROLL_TORQUE: f32 = 6.0;
    pub const AIR_YAW_TORQUE: f32 = 2.0;
    pub const AIR_ANGULAR_DAMPING: f32 = 0.8;
    pub const AIR_LINEAR_DAMPING: f32 = 0.02;  // Very light air drag
    
    pub const TERRAIN_ALIGN_SPEED: f32 = 15.0;
    pub const MAX_TERRAIN_PITCH: f32 = 1.2;    // ~70 degrees
    pub const MAX_TERRAIN_ROLL: f32 = 0.8;     // ~45 degrees
    
    /// Height above ground to be considered "grounded"
    /// SMALLER = easier to get air off bumps/crests
    pub const GROUND_THRESHOLD: f32 = 0.08;
    
    /// Bounce coefficient on landing (0 = no bounce, 1 = full bounce)
    pub const BOUNCE: f32 = 0.15;
}

// =============================================================================
// COMPONENTS
// =============================================================================

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub vehicle_type: VehicleType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum VehicleType {
    #[default]
    Motorbike,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VehicleState {
    pub position: Vec3,
    pub velocity: Vec3,
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub angular_velocity_yaw: f32,
    pub angular_velocity_pitch: f32,
    pub angular_velocity_roll: f32,
    pub grounded: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VehicleInput {
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
    /// Hold Shift to enable air tricks (pitch/roll control while airborne)
    pub air_control: bool,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VehicleDriver {
    pub driver_id: Option<u64>,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InVehicle {
    pub vehicle_entity: Entity,
}

// =============================================================================
// PHYSICS - Pure physics, no artificial limits
// =============================================================================

pub fn surface_mu(biome: Biome) -> f32 {
    match biome {
        Biome::Desert => motorbike::MU_DESERT,
        Biome::Grasslands => motorbike::MU_GRASSLANDS,
        Biome::Natureland => motorbike::MU_GRASSLANDS, // Similar to grasslands (forest floor)
    }
}

/// Get forward and right vectors from heading
fn get_basis_vectors(heading: f32) -> (Vec3, Vec3) {
    let forward = Vec3::new(-heading.sin(), 0.0, -heading.cos());
    let right = Vec3::new(heading.cos(), 0.0, -heading.sin());
    (forward, right)
}

fn shortest_angle_diff(target: f32, current: f32) -> f32 {
    let diff = target - current;
    (diff + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn terrain_angles_from_normal(normal: Vec3, heading: f32) -> (f32, f32) {
    let (forward, right) = get_basis_vectors(heading);
    let forward_slope = normal.dot(forward);
    let pitch = -forward_slope.asin().clamp(-motorbike::MAX_TERRAIN_PITCH, motorbike::MAX_TERRAIN_PITCH);
    let right_slope = normal.dot(right);
    let roll = right_slope.asin().clamp(-motorbike::MAX_TERRAIN_ROLL, motorbike::MAX_TERRAIN_ROLL);
    (pitch, roll)
}

/// Calculate the bike's local "up" vector in world space based on its orientation
fn bike_up_vector(pitch: f32, roll: f32) -> Vec3 {
    // Start with world up, then rotate by pitch (around X) and roll (around Z)
    // Simplified: the bike's up vector after pitch and roll
    // When upright (pitch=0, roll=0): up = (0, 1, 0)
    // When pitched forward: up tilts backward
    // When rolled right: up tilts left
    let up = Vec3::new(
        -roll.sin(),                    // Roll tilts up vector sideways
        pitch.cos() * roll.cos(),       // Reduced Y when pitched or rolled
        pitch.sin(),                    // Pitch tilts up vector forward/back
    );
    up.normalize()
}

pub fn step_vehicle_physics(
    input: &VehicleInput,
    state: &mut VehicleState,
    terrain: &TerrainGenerator,
    dt: f32,
    has_driver: bool,
) {
    let half_height = motorbike::SIZE.1 * 0.5;
    let prev_pos = state.position;

    // Sample terrain at current position
    let ground_y = terrain.get_height(state.position.x, state.position.z);
    let ground_normal = terrain.get_normal(state.position.x, state.position.z);
    
    let bottom_y = state.position.y - half_height;
    let height_above_ground = bottom_y - ground_y;
    let was_grounded = state.grounded;

    // Grounded if close to terrain
    state.grounded = height_above_ground <= motorbike::GROUND_THRESHOLD && height_above_ground >= -0.5;

    // === WHEEL CONTACT CHECK ===
    // Only the underside of the bike provides traction!
    // Check how well the bike's "up" aligns with the ground normal (or world up)
    // - Upright bike: bike_up · ground_normal ≈ 1 → full traction
    // - On side: bike_up · ground_normal ≈ 0 → no traction  
    // - Upside down: bike_up · ground_normal ≈ -1 → definitely no traction
    let bike_up = bike_up_vector(state.pitch, state.roll);
    let wheel_contact = bike_up.dot(ground_normal).max(0.0); // 0 to 1, clamped (no negative)
    
    // Wheels need to be reasonably down to have any traction
    // Below ~45° from upright (contact < 0.7), traction drops off sharply
    let wheel_contact_factor = if wheel_contact > 0.5 {
        // Good contact: smooth falloff
        wheel_contact
    } else if wheel_contact > 0.1 {
        // Poor contact: very reduced traction
        wheel_contact * 0.3
    } else {
        // No meaningful contact (upside down, on side)
        0.0
    };
    
    // "Wheels down" means we have meaningful wheel contact
    let wheels_down = wheel_contact_factor > 0.1;

    let biome = terrain.get_biome(state.position.x, state.position.z);
    let mu = surface_mu(biome);

    // Horizontal speed for steering calculations
    let horizontal_speed = Vec3::new(state.velocity.x, 0.0, state.velocity.z).length();

    // === STEERING ===
    // Can only steer if grounded AND wheels are actually down
    if state.grounded && wheels_down {
        let speed_t = (horizontal_speed / motorbike::MAX_SPEED).clamp(0.0, 1.0);
        let steer_rate = motorbike::STEERING_SPEED_MIN * (1.0 - speed_t) + motorbike::STEERING_SPEED_MAX * speed_t;
        let turn_effectiveness = (horizontal_speed / 2.0).clamp(0.0, 1.0);
        // Steering effectiveness reduced by wheel contact
        let target_yaw_vel = input.steer * steer_rate * turn_effectiveness * wheel_contact_factor;
        state.angular_velocity_yaw += (target_yaw_vel - state.angular_velocity_yaw) * motorbike::STEERING_RESPONSE * dt;
    } else {
        // Air or wheels not down: air yaw control
        state.angular_velocity_yaw += input.steer * motorbike::AIR_YAW_TORQUE * dt;
        state.angular_velocity_yaw *= (-motorbike::AIR_ANGULAR_DAMPING * dt).exp();
    }

    state.heading += state.angular_velocity_yaw * dt;
    // Wrap heading
    state.heading = (state.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;

    // === ORIENTATION (pitch/roll) ===
    // Only align to terrain if wheels are down; otherwise treat as airborne
    if state.grounded && wheels_down {
        let (terrain_pitch, terrain_roll) = terrain_angles_from_normal(ground_normal, state.heading);
        
        // Turn lean
        let turn_lean = if input.steer.abs() > 0.05 && horizontal_speed > 1.0 {
            let speed_factor = (horizontal_speed / motorbike::MAX_SPEED).clamp(0.0, 1.0);
            -input.steer * motorbike::TURN_LEAN_ANGLE * (0.3 + 0.7 * speed_factor)
        } else {
            0.0
        };

        let target_pitch = terrain_pitch;
        let target_roll = terrain_roll + turn_lean;

        // Terrain alignment strength based on wheel contact
        let align_strength = motorbike::TERRAIN_ALIGN_SPEED * wheel_contact_factor;
        
        let pitch_error = shortest_angle_diff(target_pitch, state.pitch);
        state.angular_velocity_pitch += pitch_error * align_strength * dt;
        state.angular_velocity_pitch *= 0.8;
        state.pitch += state.angular_velocity_pitch * dt;

        let roll_error = shortest_angle_diff(target_roll, state.roll);
        state.angular_velocity_roll += roll_error * motorbike::LEAN_SPEED * wheel_contact_factor * dt;
        state.angular_velocity_roll *= 0.85;
        state.roll += state.angular_velocity_roll * dt;
    } else {
        // Air or upside down: preserve momentum, optional trick control with Shift
        if input.air_control {
            state.angular_velocity_pitch += (input.throttle - input.brake) * motorbike::AIR_PITCH_TORQUE * dt;
            state.angular_velocity_roll += (-input.steer) * motorbike::AIR_ROLL_TORQUE * dt;
        }

        let ang_damp = (-motorbike::AIR_ANGULAR_DAMPING * dt).exp();
        state.angular_velocity_pitch *= ang_damp;
        state.angular_velocity_roll *= ang_damp;

        state.pitch += state.angular_velocity_pitch * dt;
        state.roll += state.angular_velocity_roll * dt;
    }

    // === MOVEMENT ===
    if state.grounded {
        // Project movement onto terrain tangent plane
        let (forward_flat, _) = get_basis_vectors(state.heading);
        let forward_tangent = (forward_flat - ground_normal * forward_flat.dot(ground_normal)).normalize_or_zero();
        let right_tangent = forward_tangent.cross(ground_normal).normalize_or_zero();

        // Decompose velocity
        let forward_speed = state.velocity.dot(forward_tangent);
        let lateral_speed = state.velocity.dot(right_tangent);

        // PHYSICS: Normal force = m * g * cos(slope)
        // cos(slope) = ground_normal.y (since normal points up on flat ground)
        let g_mag = -motorbike::GRAVITY;
        let cos_slope = ground_normal.y.max(0.0);
        let normal_force = motorbike::MASS * g_mag * cos_slope;
        
        // Maximum tire force from friction: F_max = mu * N * wheel_contact
        // If bike is upside down or on side, wheel_contact_factor is low/zero = no traction!
        let max_tire_force = mu * normal_force * wheel_contact_factor;

        // PHYSICS: Gravity component along slope
        // This is what makes you slide back on steep hills
        let gravity_vec = Vec3::new(0.0, motorbike::GRAVITY, 0.0);
        let gravity_parallel = gravity_vec - ground_normal * gravity_vec.dot(ground_normal);
        let g_forward = gravity_parallel.dot(forward_tangent);
        let g_lateral = gravity_parallel.dot(right_tangent);

        // Engine force (limited by traction)
        let speed_ratio = (forward_speed.abs() / motorbike::MAX_SPEED).clamp(0.0, 1.0);
        let power_falloff = 1.0 - speed_ratio * 0.6;
        let engine_request = input.throttle * motorbike::ENGINE_FORCE * power_falloff;
        let engine_force = engine_request.min(max_tire_force);

        // Brake force (limited by traction)
        let brake_request = input.brake * motorbike::BRAKE_FORCE;
        let brake_force = brake_request.min(max_tire_force);

        // Drag (air resistance - always applies)
        let drag = motorbike::DRAG_COEFFICIENT * forward_speed * forward_speed.abs();
        
        // Rolling resistance (only if wheels down)
        let rolling = motorbike::ROLLING_RESISTANCE * forward_speed.signum() 
            * (forward_speed.abs() > 0.1) as i32 as f32 
            * wheel_contact_factor;

        // Engine braking (only if wheels down - engine connected to wheels)
        let engine_brake_coeff = if has_driver { motorbike::ENGINE_BRAKE_DRIVER } else { motorbike::ENGINE_BRAKE_NO_DRIVER };
        let engine_brake = if input.throttle < 0.1 && wheels_down { 
            engine_brake_coeff * forward_speed * wheel_contact_factor 
        } else { 
            0.0 
        };

        // Net force and acceleration
        let net_force = engine_force - brake_force * forward_speed.signum() - drag - rolling - engine_brake;
        let accel = net_force / motorbike::MASS;

        // Update speeds
        let mut new_forward_speed = forward_speed + accel * dt;
        new_forward_speed += g_forward * dt;  // Gravity along slope (always applies)
        
        let mut new_lateral_speed = lateral_speed;
        new_lateral_speed += g_lateral * dt;  // Gravity sideways on camber
        // Lateral friction (only if wheels down - tires grip sideways)
        let lateral_grip = motorbike::LATERAL_FRICTION * mu * wheel_contact_factor;
        new_lateral_speed *= (-lateral_grip * dt).exp();

        // Speed limits
        new_forward_speed = new_forward_speed.clamp(-motorbike::MAX_SPEED * 0.5, motorbike::MAX_SPEED);
        
        // Come to rest only if wheels are down and on gentle slope
        if wheels_down && new_forward_speed.abs() < 0.15 && input.throttle < 0.1 && input.brake < 0.1 && g_forward.abs() < 2.0 {
            new_forward_speed = 0.0;
        }
        if wheels_down && new_lateral_speed.abs() < 0.1 && g_lateral.abs() < 1.0 {
            new_lateral_speed = 0.0;
        }
        
        // If upside down / on side: apply sliding friction instead (body scraping ground)
        // But allow lateral drift - the bike slides nicely when tilted
        if !wheels_down && state.grounded {
            // Scraping friction - mainly slows forward, allows drift
            let scrape_friction = 0.25;
            let forward_damp = (-scrape_friction * 6.0 * dt).exp();
            let lateral_damp = (-scrape_friction * 2.0 * dt).exp(); // Much less lateral friction = drift!
            new_forward_speed *= forward_damp;
            new_lateral_speed *= lateral_damp;
        }

        // Reconstruct velocity (this includes the Y component from slope!)
        state.velocity = forward_tangent * new_forward_speed + right_tangent * new_lateral_speed;

        // Move
        state.position += state.velocity * dt;

        // Check if still grounded at new position
        let new_ground_y = terrain.get_height(state.position.x, state.position.z);
        let new_bottom_y = state.position.y - half_height;
        let new_height_above = new_bottom_y - new_ground_y;

        if new_height_above <= motorbike::GROUND_THRESHOLD {
            // Stay grounded, snap to terrain
            state.position.y = new_ground_y + half_height;
            // Recalculate velocity from actual movement (preserves slope velocity correctly)
            state.velocity = (state.position - prev_pos) / dt;
            state.grounded = true;
        } else {
            // Left the ground! Go airborne with current velocity
            state.grounded = false;
        }
    }

    // === AIRBORNE ===
    if !state.grounded {
        // Pure physics: gravity + minimal air drag
        state.velocity.y += motorbike::GRAVITY * dt;
        state.velocity *= (-motorbike::AIR_LINEAR_DAMPING * dt).exp();
        state.position += state.velocity * dt;
    }

    // === TERRAIN COLLISION (prevents tunneling) ===
    let final_ground = terrain.get_height(state.position.x, state.position.z);
    let min_y = final_ground + half_height;
    
    if state.position.y < min_y {
        state.position.y = min_y;
        
        // Bounce/impact
        if state.velocity.y < 0.0 {
            let impact_speed = -state.velocity.y;
            state.velocity.y *= -motorbike::BOUNCE;
            if state.velocity.y.abs() < 0.5 {
                state.velocity.y = 0.0;
            }
            
            // Hard landing: reduce forward speed but PRESERVE lateral for drift!
            // The harder you land, the more you "stick" forward but can still slide sideways
            if impact_speed > 8.0 {
                // Get forward direction to only dampen forward speed, preserve drift
                let (forward_flat, right_flat) = get_basis_vectors(state.heading);
                let forward_speed = state.velocity.dot(forward_flat);
                let lateral_speed = state.velocity.dot(right_flat);
                
                // Dampen forward more than lateral
                let forward_damp = 0.6;
                let lateral_damp = motorbike::LANDING_DRIFT_PRESERVE; // Preserve most of the drift!
                
                state.velocity = forward_flat * (forward_speed * forward_damp) 
                               + right_flat * (lateral_speed * lateral_damp)
                               + Vec3::new(0.0, state.velocity.y, 0.0);
            }
        }
        state.grounded = true;
    }

    // Dampen angular velocities on landing
    if !was_grounded && state.grounded {
        state.angular_velocity_roll *= 0.7;
        state.angular_velocity_pitch *= 0.7;
    }
}

pub fn can_interact_with_vehicle(player_pos: Vec3, vehicle_state: &VehicleState) -> bool {
    let dist = (player_pos - vehicle_state.position).length();
    dist < 3.0
}
