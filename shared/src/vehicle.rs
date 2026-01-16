//! Vehicle physics system - PURE PHYSICS approach
//!
//! No artificial limits. Physics handles everything:
//! - Can't climb walls because: steep slope = less normal force = less traction + gravity pulls back
//! - Gets air off crests because: when ground drops away, you're airborne
//! - Slides on steep slopes because: gravity component along slope > available traction

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::terrain::{Biome, WorldTerrain};

// =============================================================================
// VEHICLE TUNING CONSTANTS
// =============================================================================

pub mod motorbike {
    pub const MASS: f32 = 180.0;
    pub const ENGINE_FORCE: f32 = 7500.0;      // Newtons (boosted for higher top speed)
    pub const MAX_SPEED: f32 = 45.0;           // m/s (~162 km/h) - FAST speeder!
    pub const MAX_REVERSE_SPEED: f32 = 8.0;    // m/s (~29 km/h) - slow reverse
    pub const REVERSE_FORCE: f32 = 2500.0;     // Newtons (weaker than forward)
    pub const BRAKE_FORCE: f32 = 8000.0;       // Newtons (stronger brakes for faster bike)
    pub const DRAG_COEFFICIENT: f32 = 0.18;    // Lower drag for speed
    pub const ROLLING_RESISTANCE: f32 = 35.0;  // Ground friction when rolling

    /// Engine braking when not on throttle (in-gear)
    pub const ENGINE_BRAKE_DRIVER: f32 = 40.0;
    /// Weaker when unmanned (neutral/parked)
    pub const ENGINE_BRAKE_NO_DRIVER: f32 = 15.0;
    
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

/// Tuning values for a vehicle type.
#[derive(Clone, Copy, Debug)]
pub struct VehicleDef {
    pub mass: f32,
    pub engine_force: f32,
    pub max_speed: f32,
    pub max_reverse_speed: f32,
    pub reverse_force: f32,
    pub brake_force: f32,
    pub drag_coefficient: f32,
    pub rolling_resistance: f32,
    pub engine_brake_driver: f32,
    pub engine_brake_no_driver: f32,
    pub steering_speed_min: f32,
    pub steering_speed_max: f32,
    pub steering_response: f32,
    pub turn_lean_angle: f32,
    pub lean_speed: f32,
    pub mu_desert: f32,
    pub mu_grasslands: f32,
    pub lateral_friction: f32,
    pub landing_drift_preserve: f32,
    pub size: Vec3,
    pub gravity: f32,
    pub air_pitch_torque: f32,
    pub air_roll_torque: f32,
    pub air_yaw_torque: f32,
    pub air_angular_damping: f32,
    pub air_linear_damping: f32,
    pub terrain_align_speed: f32,
    pub max_terrain_pitch: f32,
    pub max_terrain_roll: f32,
    pub ground_threshold: f32,
    pub bounce: f32,
    pub wheel_base: f32,
    pub track_width: f32,
    pub wheel_radius: f32,
    pub suspension_rest: f32,
    pub suspension_stiffness: f32,
    pub suspension_damping: f32,
    pub max_steer_angle: f32,
    pub yaw_inertia: f32,
    pub yaw_damping: f32,
    pub body_pitch_response: f32,
    pub body_roll_response: f32,
    pub suspension_pitch_factor: f32,
    pub suspension_roll_factor: f32,
}

pub fn vehicle_def(vehicle_type: VehicleType) -> VehicleDef {
    match vehicle_type {
        VehicleType::Motorbike => VehicleDef {
            mass: motorbike::MASS,
            engine_force: motorbike::ENGINE_FORCE,
            max_speed: motorbike::MAX_SPEED,
            max_reverse_speed: motorbike::MAX_REVERSE_SPEED,
            reverse_force: motorbike::REVERSE_FORCE,
            brake_force: motorbike::BRAKE_FORCE,
            drag_coefficient: motorbike::DRAG_COEFFICIENT,
            rolling_resistance: motorbike::ROLLING_RESISTANCE,
            engine_brake_driver: motorbike::ENGINE_BRAKE_DRIVER,
            engine_brake_no_driver: motorbike::ENGINE_BRAKE_NO_DRIVER,
            steering_speed_min: motorbike::STEERING_SPEED_MIN,
            steering_speed_max: motorbike::STEERING_SPEED_MAX,
            steering_response: motorbike::STEERING_RESPONSE,
            turn_lean_angle: motorbike::TURN_LEAN_ANGLE,
            lean_speed: motorbike::LEAN_SPEED,
            mu_desert: motorbike::MU_DESERT,
            mu_grasslands: motorbike::MU_GRASSLANDS,
            lateral_friction: motorbike::LATERAL_FRICTION,
            landing_drift_preserve: motorbike::LANDING_DRIFT_PRESERVE,
            size: Vec3::new(motorbike::SIZE.0, motorbike::SIZE.1, motorbike::SIZE.2),
            gravity: motorbike::GRAVITY,
            air_pitch_torque: motorbike::AIR_PITCH_TORQUE,
            air_roll_torque: motorbike::AIR_ROLL_TORQUE,
            air_yaw_torque: motorbike::AIR_YAW_TORQUE,
            air_angular_damping: motorbike::AIR_ANGULAR_DAMPING,
            air_linear_damping: motorbike::AIR_LINEAR_DAMPING,
            terrain_align_speed: motorbike::TERRAIN_ALIGN_SPEED,
            max_terrain_pitch: motorbike::MAX_TERRAIN_PITCH,
            max_terrain_roll: motorbike::MAX_TERRAIN_ROLL,
            ground_threshold: motorbike::GROUND_THRESHOLD,
            bounce: motorbike::BOUNCE,
            wheel_base: 1.6,
            track_width: 0.45,
            wheel_radius: 0.32,
            suspension_rest: 0.2,
            suspension_stiffness: 12000.0,
            suspension_damping: 2000.0,
            max_steer_angle: 0.55,
            yaw_inertia: 200.0,
            yaw_damping: 3.0,
            body_pitch_response: 6.0,
            body_roll_response: 6.0,
            suspension_pitch_factor: 0.08,
            suspension_roll_factor: 0.08,
        },
        VehicleType::Car => VehicleDef {
            mass: 1200.0,
            engine_force: 11000.0,
            max_speed: 42.0,
            max_reverse_speed: 10.0,
            reverse_force: 4000.0,
            brake_force: 14000.0,
            drag_coefficient: 0.35,
            rolling_resistance: 60.0,
            engine_brake_driver: 300.0,
            engine_brake_no_driver: 80.0,
            steering_speed_min: 1.6,
            steering_speed_max: 0.7,
            steering_response: 8.0,
            turn_lean_angle: 0.0,
            lean_speed: 4.0,
            mu_desert: 0.9,
            mu_grasslands: 1.0,
            lateral_friction: 12.0,
            landing_drift_preserve: 0.35,
            size: Vec3::new(4.2, 1.6, 1.8),
            gravity: -20.0,
            air_pitch_torque: 1.0,
            air_roll_torque: 1.0,
            air_yaw_torque: 0.5,
            air_angular_damping: 1.2,
            air_linear_damping: 0.05,
            terrain_align_speed: 8.0,
            max_terrain_pitch: 0.7,
            max_terrain_roll: 0.5,
            ground_threshold: 0.15,
            bounce: 0.05,
            wheel_base: 2.7,
            track_width: 1.55,
            wheel_radius: 0.34,
            suspension_rest: 0.35,
            suspension_stiffness: 22000.0,
            suspension_damping: 4500.0,
            max_steer_angle: 0.5,
            yaw_inertia: 1300.0,
            yaw_damping: 2.5,
            body_pitch_response: 4.0,
            body_roll_response: 5.0,
            suspension_pitch_factor: 0.12,
            suspension_roll_factor: 0.15,
        },
    }
}

// =============================================================================
// COMPONENTS
// =============================================================================

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub vehicle_type: VehicleType,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum VehicleType {
    #[default]
    Motorbike,
    Car,
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

/// Per-vehicle suspension state for cars (server-authoritative only)
#[derive(Component, Clone, Debug)]
pub struct CarSuspensionState {
    pub compression: [f32; 4],
    pub last_compression: [f32; 4],
}

impl Default for CarSuspensionState {
    fn default() -> Self {
        Self {
            compression: [0.0; 4],
            last_compression: [0.0; 4],
        }
    }
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

pub fn surface_mu(def: &VehicleDef, biome: Biome) -> f32 {
    match biome {
        Biome::Desert => def.mu_desert,
        Biome::Grasslands => def.mu_grasslands,
        Biome::Natureland => def.mu_grasslands, // Similar to grasslands (forest floor)
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

fn terrain_angles_from_normal(normal: Vec3, heading: f32, def: &VehicleDef) -> (f32, f32) {
    let (forward, right) = get_basis_vectors(heading);
    let forward_slope = normal.dot(forward);
    let pitch = -forward_slope.asin().clamp(-def.max_terrain_pitch, def.max_terrain_pitch);
    let right_slope = normal.dot(right);
    let roll = right_slope.asin().clamp(-def.max_terrain_roll, def.max_terrain_roll);
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
    terrain: &WorldTerrain,
    dt: f32,
    has_driver: bool,
    vehicle_type: VehicleType,
) {
    let def = vehicle_def(vehicle_type);
    let half_height = def.size.y * 0.5;
    let prev_pos = state.position;

    // Sample terrain at current position
    let ground_y = terrain.get_height(state.position.x, state.position.z);
    let ground_normal = terrain.get_normal(state.position.x, state.position.z);
    
    let bottom_y = state.position.y - half_height;
    let height_above_ground = bottom_y - ground_y;
    let was_grounded = state.grounded;

    // Grounded if close to terrain
    state.grounded = height_above_ground <= def.ground_threshold && height_above_ground >= -0.5;

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
    let mu = surface_mu(&def, biome);

    // Horizontal speed for steering calculations
    let horizontal_speed = Vec3::new(state.velocity.x, 0.0, state.velocity.z).length();

    // === STEERING ===
    // Can only steer if grounded AND wheels are actually down
    // Note: steer is negated so positive input = turn right
    if state.grounded && wheels_down {
        let speed_t = (horizontal_speed / def.max_speed).clamp(0.0, 1.0);
        let steer_rate = def.steering_speed_min * (1.0 - speed_t) + def.steering_speed_max * speed_t;
        let turn_effectiveness = (horizontal_speed / 2.0).clamp(0.0, 1.0);
        // Steering effectiveness reduced by wheel contact
        let target_yaw_vel = -input.steer * steer_rate * turn_effectiveness * wheel_contact_factor;
        state.angular_velocity_yaw += (target_yaw_vel - state.angular_velocity_yaw) * def.steering_response * dt;
    } else {
        // Air or wheels not down: air yaw control
        state.angular_velocity_yaw += -input.steer * def.air_yaw_torque * dt;
        state.angular_velocity_yaw *= (-def.air_angular_damping * dt).exp();
    }

    state.heading += state.angular_velocity_yaw * dt;
    // Wrap heading
    state.heading = (state.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;

    // === ORIENTATION (pitch/roll) ===
    // Only align to terrain if wheels are down; otherwise treat as airborne
    if state.grounded && wheels_down {
        let (terrain_pitch, terrain_roll) = terrain_angles_from_normal(ground_normal, state.heading, &def);
        
        // Turn lean (positive steer = turn right = lean right = negative roll)
        let turn_lean = if input.steer.abs() > 0.05 && horizontal_speed > 1.0 {
            let speed_factor = (horizontal_speed / def.max_speed).clamp(0.0, 1.0);
            input.steer * def.turn_lean_angle * (0.3 + 0.7 * speed_factor)
        } else {
            0.0
        };

        let target_pitch = terrain_pitch;
        let target_roll = terrain_roll + turn_lean;

        // Terrain alignment strength based on wheel contact
        let align_strength = def.terrain_align_speed * wheel_contact_factor;
        
        let pitch_error = shortest_angle_diff(target_pitch, state.pitch);
        state.angular_velocity_pitch += pitch_error * align_strength * dt;
        state.angular_velocity_pitch *= 0.8;
        state.pitch += state.angular_velocity_pitch * dt;

        let roll_error = shortest_angle_diff(target_roll, state.roll);
        state.angular_velocity_roll += roll_error * def.lean_speed * wheel_contact_factor * dt;
        state.angular_velocity_roll *= 0.85;
        state.roll += state.angular_velocity_roll * dt;
    } else {
        // Air or upside down: preserve momentum, optional trick control with Shift
        if input.air_control {
            state.angular_velocity_pitch += (input.throttle - input.brake) * def.air_pitch_torque * dt;
            state.angular_velocity_roll += input.steer * def.air_roll_torque * dt;
        }

        let ang_damp = (-def.air_angular_damping * dt).exp();
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
        let g_mag = -def.gravity;
        let cos_slope = ground_normal.y.max(0.0);
        let normal_force = def.mass * g_mag * cos_slope;
        
        // Maximum tire force from friction: F_max = mu * N * wheel_contact
        // If bike is upside down or on side, wheel_contact_factor is low/zero = no traction!
        let max_tire_force = mu * normal_force * wheel_contact_factor;

        // PHYSICS: Gravity component along slope
        // This is what makes you slide back on steep hills
        let gravity_vec = Vec3::new(0.0, def.gravity, 0.0);
        let gravity_parallel = gravity_vec - ground_normal * gravity_vec.dot(ground_normal);
        let g_forward = gravity_parallel.dot(forward_tangent);
        let g_lateral = gravity_parallel.dot(right_tangent);

        // Engine force (limited by traction)
        let speed_ratio = (forward_speed.abs() / def.max_speed).clamp(0.0, 1.0);
        let power_falloff = 1.0 - speed_ratio * 0.6;
        let engine_request = input.throttle * def.engine_force * power_falloff;
        let engine_force = engine_request.min(max_tire_force);

        // Reverse: when moving slowly or stopped and pressing brake (no throttle)
        // This allows the bike to back up slowly
        let is_reversing = forward_speed < 2.0 && input.brake > 0.1 && input.throttle < 0.1;
        let reverse_speed_ratio = ((-forward_speed).max(0.0) / def.max_reverse_speed).clamp(0.0, 1.0);
        let reverse_power_falloff = 1.0 - reverse_speed_ratio * 0.7;
        let reverse_request = if is_reversing { input.brake * def.reverse_force * reverse_power_falloff } else { 0.0 };
        let reverse_force = reverse_request.min(max_tire_force);

        // Brake force (limited by traction) - only apply when actually braking (moving forward)
        let brake_request = if !is_reversing { input.brake * def.brake_force } else { 0.0 };
        let brake_force = brake_request.min(max_tire_force);

        // Drag (air resistance - always applies)
        let drag = def.drag_coefficient * forward_speed * forward_speed.abs();
        
        // Rolling resistance (only if wheels down)
        let rolling = def.rolling_resistance * forward_speed.signum() 
            * (forward_speed.abs() > 0.1) as i32 as f32 
            * wheel_contact_factor;

        // Engine braking (only if wheels down - engine connected to wheels)
        let engine_brake_coeff = if has_driver { def.engine_brake_driver } else { def.engine_brake_no_driver };
        let engine_brake = if input.throttle < 0.1 && wheels_down { 
            engine_brake_coeff * forward_speed * wheel_contact_factor 
        } else { 
            0.0 
        };

        // Net force and acceleration
        // Reverse force is negative (pushes backward), engine force is positive (pushes forward)
        let net_force = engine_force - reverse_force - brake_force * forward_speed.signum() - drag - rolling - engine_brake;
        let accel = net_force / def.mass;

        // Update speeds
        let mut new_forward_speed = forward_speed + accel * dt;
        new_forward_speed += g_forward * dt;  // Gravity along slope (always applies)
        
        let mut new_lateral_speed = lateral_speed;
        new_lateral_speed += g_lateral * dt;  // Gravity sideways on camber
        // Lateral friction (only if wheels down - tires grip sideways)
        let lateral_grip = def.lateral_friction * mu * wheel_contact_factor;
        new_lateral_speed *= (-lateral_grip * dt).exp();

        // Speed limits (reverse is slower than forward)
        new_forward_speed = new_forward_speed.clamp(-def.max_reverse_speed, def.max_speed);
        
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

        if new_height_above <= def.ground_threshold {
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
        state.velocity.y += def.gravity * dt;
        state.velocity *= (-def.air_linear_damping * dt).exp();
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
            state.velocity.y *= -def.bounce;
            if state.velocity.y.abs() < 0.5 {
                state.velocity.y = 0.0;
            }
            
            // Hard landing: slight speed reduction but preserve momentum
            // Only kicks in on really hard landings (10+ m/s vertical impact)
            if impact_speed > 10.0 {
                // Get forward direction to only dampen forward speed, preserve drift
                let (forward_flat, right_flat) = get_basis_vectors(state.heading);
                let forward_speed = state.velocity.dot(forward_flat);
                let lateral_speed = state.velocity.dot(right_flat);

                // Light damping - preserve most of the speed
                let forward_damp = 0.85;
                let lateral_damp = def.landing_drift_preserve;

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

/// Raycast-wheel car physics with proper ground collision.
///
/// Key concepts:
/// - Each wheel raycasts down from its position on the chassis
/// - Suspension compression creates upward force (spring + damper)
/// - HARD FLOOR CONSTRAINT: Car cannot sink below terrain
/// - Tire forces provide drive, braking, and lateral grip
pub fn step_car_physics(
    input: &VehicleInput,
    state: &mut VehicleState,
    suspension: &mut CarSuspensionState,
    terrain: &WorldTerrain,
    dt: f32,
    has_driver: bool,
    vehicle_type: VehicleType,
) {
    let def = vehicle_def(vehicle_type);
    let (forward, right) = get_basis_vectors(state.heading);

    let throttle = if has_driver { input.throttle } else { 0.0 };
    let brake = if has_driver { input.brake } else { 0.0 };
    let steer = if has_driver { input.steer } else { 0.0 };

    let half_wheel_base = def.wheel_base * 0.5;
    let half_track = def.track_width * 0.5;

    // Wheel positions in local XZ (relative to chassis center)
    // The suspension mounts are at the bottom of the chassis
    let wheel_local_xz = [
        Vec2::new(-half_track, -half_wheel_base), // Front-left
        Vec2::new( half_track, -half_wheel_base), // Front-right
        Vec2::new(-half_track,  half_wheel_base), // Rear-left
        Vec2::new( half_track,  half_wheel_base), // Rear-right
    ];

    let mut total_force = Vec3::ZERO;
    let mut total_torque_y = 0.0;
    let mut grounded_wheels = 0;

    // Track ground heights at each wheel for terrain alignment
    let mut ground_heights = [0.0f32; 4]; // FL, FR, RL, RR

    for (i, local_xz) in wheel_local_xz.iter().copied().enumerate() {
        let is_front = i < 2;

        // World XZ position of this wheel
        let wheel_world_pos = state.position
            + forward * local_xz.y
            + right * local_xz.x;

        // Get terrain height at wheel position
        let ground_y = terrain.get_height(wheel_world_pos.x, wheel_world_pos.z);
        ground_heights[i] = ground_y;

        // Where the wheel center would be if sitting on the ground
        let wheel_contact_y = ground_y + def.wheel_radius;

        // The "rest position" of the chassis is ride_height above the contact point
        let chassis_rest_y = wheel_contact_y + def.suspension_rest;

        // Suspension compression: how much the spring is compressed
        // Positive when chassis is lower than rest position (spring pushing up)
        let compression = (chassis_rest_y - state.position.y).clamp(0.0, def.suspension_rest);

        // Compression velocity for damping
        let compression_vel = (compression - suspension.last_compression[i]) / dt;
        suspension.last_compression[i] = compression;
        suspension.compression[i] = compression;

        // Skip if wheel is not in contact (chassis too high, suspension fully extended)
        if compression <= 0.001 {
            continue;
        }

        grounded_wheels += 1;

        // Spring-damper force (always pushes UP)
        let spring_force = compression * def.suspension_stiffness;
        let damper_force = compression_vel * def.suspension_damping;
        let normal_force = (spring_force + damper_force).max(0.0);

        // Contact point on ground (for torque calculations)
        let contact_point = Vec3::new(wheel_world_pos.x, ground_y, wheel_world_pos.z);

        // Steering angle (front wheels only)
        let steer_angle = if is_front { steer * def.max_steer_angle } else { 0.0 };
        let steer_rot = Quat::from_rotation_y(steer_angle);
        let wheel_forward = (steer_rot * forward).normalize_or_zero();
        let wheel_right = (steer_rot * right).normalize_or_zero();

        // Velocity of the wheel contact point
        let r = contact_point - state.position;
        let omega = Vec3::Y * state.angular_velocity_yaw;
        let wheel_velocity = state.velocity + omega.cross(r);

        // Decompose into longitudinal (forward/back) and lateral (sideways)
        let long_speed = wheel_velocity.dot(wheel_forward);
        let lat_speed = wheel_velocity.dot(wheel_right);

        // Tire friction based on normal force
        let mu = surface_mu(&def, terrain.get_biome(contact_point.x, contact_point.z));
        let max_friction = mu * normal_force;

        // Drive force (from engine, applied at rear or all wheels)
        // For simplicity, apply to all wheels but rear-biased would be more realistic
        let drive_force = throttle * def.engine_force * 0.25; // Per wheel

        // Brake force (opposes current motion)
        let brake_force = brake * def.brake_force * 0.25;
        let brake_dir = if long_speed.abs() > 0.1 { long_speed.signum() } else { 0.0 };

        // Net longitudinal force (clamped by tire grip)
        let longitudinal_force = (drive_force - brake_force * brake_dir)
            .clamp(-max_friction, max_friction);

        // Lateral force: opposes sideways sliding (this is what keeps the car from drifting)
        // Higher lateral_friction = more grip, less drift
        let lateral_force = (-lat_speed * def.lateral_friction)
            .clamp(-max_friction, max_friction);

        // Total force from this wheel
        let wheel_force = wheel_forward * longitudinal_force
            + wheel_right * lateral_force
            + Vec3::Y * normal_force;
        total_force += wheel_force;

        // Torque around Y axis (yaw) from tire forces
        let horizontal_tire_force = wheel_forward * longitudinal_force + wheel_right * lateral_force;
        total_torque_y += r.cross(horizontal_tire_force).y;
    }

    state.grounded = grounded_wheels > 0;

    // Gravity (always applies)
    total_force += Vec3::Y * def.gravity * def.mass;

    // Aerodynamic drag and rolling resistance
    let horizontal_vel = Vec3::new(state.velocity.x, 0.0, state.velocity.z);
    let speed = horizontal_vel.length();
    if speed > 0.01 {
        let drag_dir = horizontal_vel.normalize();
        let drag = drag_dir * (def.drag_coefficient * speed * speed);
        total_force -= drag;

        if state.grounded {
            total_force -= drag_dir * def.rolling_resistance;
        }
    }

    // Engine braking when no throttle (helps car slow down naturally)
    if state.grounded && throttle < 0.1 && speed > 0.5 {
        let engine_brake = if has_driver { def.engine_brake_driver } else { def.engine_brake_no_driver };
        total_force -= horizontal_vel.normalize() * engine_brake;
    }

    // Integrate velocity
    let accel = total_force / def.mass;
    state.velocity += accel * dt;

    // Integrate position
    state.position += state.velocity * dt;

    // === HARD FLOOR CONSTRAINT ===
    // The car CANNOT go below terrain. This is the key fix.
    // Calculate the minimum Y the chassis can be at based on all wheel positions.
    let mut min_chassis_y = f32::NEG_INFINITY;
    for local_xz in wheel_local_xz.iter().copied() {
        let wheel_world_pos = state.position + forward * local_xz.y + right * local_xz.x;
        let ground_y = terrain.get_height(wheel_world_pos.x, wheel_world_pos.z);
        // Minimum chassis Y = ground + wheel radius (wheel sitting on ground, suspension bottomed out)
        let min_y = ground_y + def.wheel_radius;
        min_chassis_y = min_chassis_y.max(min_y);
    }

    // Enforce floor constraint
    if state.position.y < min_chassis_y {
        state.position.y = min_chassis_y;
        // Kill downward velocity when hitting the floor
        if state.velocity.y < 0.0 {
            // Small bounce for game feel
            state.velocity.y *= -def.bounce;
            if state.velocity.y.abs() < 0.5 {
                state.velocity.y = 0.0;
            }
        }
        state.grounded = true;
    }

    // Stop horizontal movement if very slow (prevents drift at standstill)
    if state.grounded && speed < 0.3 && throttle < 0.1 && brake < 0.1 {
        state.velocity.x *= 0.9;
        state.velocity.z *= 0.9;
        if speed < 0.1 {
            state.velocity.x = 0.0;
            state.velocity.z = 0.0;
        }
    }

    // Yaw rotation from tire torque
    let yaw_accel = total_torque_y / def.yaw_inertia.max(1.0);
    state.angular_velocity_yaw += yaw_accel * dt;
    state.angular_velocity_yaw *= (-def.yaw_damping * dt).exp();
    state.heading += state.angular_velocity_yaw * dt;
    state.heading = (state.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;

    // Body pitch/roll from ACTUAL TERRAIN HEIGHTS at wheel positions
    // This makes the car physically align to the ground slope
    if state.grounded && grounded_wheels >= 2 {
        // Ground heights: [FL, FR, RL, RR]
        let front_avg_height = (ground_heights[0] + ground_heights[1]) * 0.5;
        let rear_avg_height = (ground_heights[2] + ground_heights[3]) * 0.5;
        let left_avg_height = (ground_heights[0] + ground_heights[2]) * 0.5;
        let right_avg_height = (ground_heights[1] + ground_heights[3]) * 0.5;

        // Calculate pitch from front-rear height difference
        // pitch = atan((rear_height - front_height) / wheelbase)
        let height_diff_pitch = rear_avg_height - front_avg_height;
        let pitch_target = (height_diff_pitch / def.wheel_base).atan();

        // Calculate roll from left-right height difference
        // roll = atan((right_height - left_height) / track_width)
        let height_diff_roll = right_avg_height - left_avg_height;
        let roll_target = (height_diff_roll / def.track_width).atan();

        // Smoothly interpolate toward target angles
        let pitch_response = def.terrain_align_speed * dt;
        let roll_response = def.terrain_align_speed * dt;

        state.pitch += (pitch_target - state.pitch) * pitch_response.min(1.0);
        state.roll += (roll_target - state.roll) * roll_response.min(1.0);

        state.pitch = state.pitch.clamp(-def.max_terrain_pitch, def.max_terrain_pitch);
        state.roll = state.roll.clamp(-def.max_terrain_roll, def.max_terrain_roll);
    } else {
        // In air: slowly return to neutral
        state.pitch *= (-def.body_pitch_response * 0.5 * dt).exp();
        state.roll *= (-def.body_roll_response * 0.5 * dt).exp();
    }
}

pub fn can_interact_with_vehicle(player_pos: Vec3, vehicle_state: &VehicleState) -> bool {
    let dist = (player_pos - vehicle_state.position).length();
    dist < 3.0
}
