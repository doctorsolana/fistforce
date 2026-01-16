//! Vehicle systems
//!
//! Handles speeder bike visuals, spawning, and transform synchronization.

use bevy::prelude::*;
use shared::{Vehicle, VehicleState, VehicleType};

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for vehicle visual entities
#[derive(Component)]
pub struct VehicleVisual;

/// Client-side render smoothing state for vehicles.
/// We integrate using replicated linear/angular velocities each frame.
/// When a *new* authoritative snapshot arrives (replicated `VehicleState` changes), we gently correct
/// toward it. This avoids the high-FPS "rubber pullback" that can cause visible popping/blinking at
/// high speed when render FPS > replication FPS.
#[derive(Component, Clone, Copy)]
pub struct VehicleRenderSmoothing {
    pub initialized: bool,
    pub position: Vec3,
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,

    /// Last authoritative (replicated) pose we observed. Used to detect new snapshots.
    pub last_server_position: Vec3,
    pub last_server_heading: f32,
    pub last_server_pitch: f32,
    pub last_server_roll: f32,

    /// Smoothed velocity for extrapolation (reduces jitter from velocity discontinuities)
    pub smoothed_velocity: Vec3,
    pub smoothed_angular_yaw: f32,
}

impl Default for VehicleRenderSmoothing {
    fn default() -> Self {
        Self {
            initialized: false,
            position: Vec3::ZERO,
            heading: 0.0,
            pitch: 0.0,
            roll: 0.0,
            last_server_position: Vec3::ZERO,
            last_server_heading: 0.0,
            last_server_pitch: 0.0,
            last_server_roll: 0.0,
            smoothed_velocity: Vec3::ZERO,
            smoothed_angular_yaw: 0.0,
        }
    }
}

// =============================================================================
// SPAWNING
// =============================================================================

/// Handle vehicle spawn visuals - Star Wars style speeder bike!
pub fn handle_vehicle_spawned(
    mut commands: Commands,
    _asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_vehicles: Query<(Entity, &Vehicle, &VehicleState), Added<Vehicle>>,
) {
    for (entity, vehicle, state) in new_vehicles.iter() {
        info!("Vehicle spawned ({:?}) at {:?}", vehicle.vehicle_type, state.position);

        // Set up the parent entity with transform and visibility
        let initial_rotation = Quat::from_euler(
            EulerRot::YXZ,
            state.heading,
            state.pitch,
            -state.roll,
        );
        commands.entity(entity).insert((
            Transform::from_translation(state.position).with_rotation(initial_rotation),
            Visibility::Inherited,
            VehicleVisual,
            VehicleRenderSmoothing {
                initialized: true,
                position: state.position,
                heading: state.heading,
                pitch: state.pitch,
                roll: state.roll,
                last_server_position: state.position,
                last_server_heading: state.heading,
                last_server_pitch: state.pitch,
                last_server_roll: state.roll,
                smoothed_velocity: state.velocity,
                smoothed_angular_yaw: state.angular_velocity_yaw,
            },
        ));

        if vehicle.vehicle_type == VehicleType::Car {
            // Simple procedural car for physics testing
            // Using basic shapes so we can clearly see orientation and ground alignment

            let def = shared::vehicle_def(VehicleType::Car);

            // Car body material - blue so it's distinct from bike
            let car_body = materials.add(StandardMaterial {
                base_color: Color::srgb(0.15, 0.25, 0.45),
                metallic: 0.7,
                perceptual_roughness: 0.3,
                ..default()
            });

            // Front indicator - bright yellow/green to show which way is forward
            let front_material = materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.9, 0.2),
                emissive: bevy::color::LinearRgba::new(1.0, 1.2, 0.3, 1.0),
                ..default()
            });

            // Rear indicator - red for back
            let rear_material = materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.1, 0.1),
                emissive: bevy::color::LinearRgba::new(1.5, 0.2, 0.1, 1.0),
                ..default()
            });

            // Wheel material - dark rubber
            let wheel_material = materials.add(StandardMaterial {
                base_color: Color::srgb(0.1, 0.1, 0.1),
                metallic: 0.0,
                perceptual_roughness: 0.9,
                ..default()
            });

            // Dimensions from physics def
            let body_length = def.wheel_base * 1.1;  // Slightly longer than wheelbase
            let body_width = def.track_width * 0.9;
            let body_height = 0.5;
            let wheel_radius = def.wheel_radius;
            let half_wb = def.wheel_base * 0.5;
            let half_track = def.track_width * 0.5;

            commands.entity(entity).with_children(|parent| {
                // === MAIN BODY - a box ===
                let body_mesh = meshes.add(Cuboid::new(body_width, body_height, body_length));
                parent.spawn((
                    Mesh3d(body_mesh),
                    MeshMaterial3d(car_body.clone()),
                    // Position body so bottom is near wheel centers
                    Transform::from_xyz(0.0, wheel_radius + 0.1, 0.0),
                ));

                // === FRONT INDICATOR - wedge/box at front ===
                let front_mesh = meshes.add(Cuboid::new(body_width * 0.8, 0.15, 0.3));
                parent.spawn((
                    Mesh3d(front_mesh),
                    MeshMaterial3d(front_material),
                    // Front is negative Z in our coordinate system
                    Transform::from_xyz(0.0, wheel_radius + 0.35, -half_wb - 0.2),
                ));

                // === REAR INDICATOR - red box at back ===
                let rear_mesh = meshes.add(Cuboid::new(body_width * 0.9, 0.2, 0.15));
                parent.spawn((
                    Mesh3d(rear_mesh),
                    MeshMaterial3d(rear_material),
                    Transform::from_xyz(0.0, wheel_radius + 0.3, half_wb + 0.15),
                ));

                // === WHEELS - 4 cylinders ===
                let wheel_mesh = meshes.add(Cylinder::new(wheel_radius, 0.15));

                // Front-left wheel
                parent.spawn((
                    Mesh3d(wheel_mesh.clone()),
                    MeshMaterial3d(wheel_material.clone()),
                    Transform::from_xyz(-half_track - 0.1, wheel_radius, -half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));

                // Front-right wheel
                parent.spawn((
                    Mesh3d(wheel_mesh.clone()),
                    MeshMaterial3d(wheel_material.clone()),
                    Transform::from_xyz(half_track + 0.1, wheel_radius, -half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));

                // Rear-left wheel
                parent.spawn((
                    Mesh3d(wheel_mesh.clone()),
                    MeshMaterial3d(wheel_material.clone()),
                    Transform::from_xyz(-half_track - 0.1, wheel_radius, half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));

                // Rear-right wheel
                parent.spawn((
                    Mesh3d(wheel_mesh.clone()),
                    MeshMaterial3d(wheel_material.clone()),
                    Transform::from_xyz(half_track + 0.1, wheel_radius, half_wb)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));
            });
            continue;
        }

        // Main body material - dark metallic gunmetal
        let body_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.13, 0.16),
            metallic: 0.85,
            perceptual_roughness: 0.25,
            ..default()
        });

        // Accent material - weathered copper/bronze
        let accent_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.32, 0.12),
            metallic: 0.9,
            perceptual_roughness: 0.15,
            ..default()
        });

        // Engine glow - emissive cyan thrusters
        let engine_glow = materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.9, 1.0),
            emissive: bevy::color::LinearRgba::new(0.8, 3.0, 4.0, 1.0),
            ..default()
        });

        // Secondary glow - red accent lights
        let red_glow = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.2, 0.1),
            emissive: bevy::color::LinearRgba::new(2.0, 0.3, 0.1, 1.0),
            ..default()
        });

        // Seat material - dark leather look
        let seat_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.06, 0.05),
            perceptual_roughness: 0.85,
            metallic: 0.0,
            ..default()
        });

        // Build the speeder bike from child meshes
        commands.entity(entity).with_children(|parent| {
            // === MAIN CHASSIS ===
            // Central body - elongated capsule shape
            let body_mesh = meshes.add(Capsule3d::new(0.22, 1.8));
            parent.spawn((
                Mesh3d(body_mesh),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, 0.35, 0.0)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));

            // Upper fairing/cowl - sleeker top piece
            let fairing = meshes.add(Capsule3d::new(0.15, 1.2));
            parent.spawn((
                Mesh3d(fairing),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, 0.55, -0.3)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 * 0.95)),
            ));

            // === NOSE SECTION ===
            // Main nose cone
            let nose = meshes.add(Cone { radius: 0.18, height: 0.7 });
            parent.spawn((
                Mesh3d(nose),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(0.0, 0.35, -1.4)
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            ));

            // Nose tip accent
            let nose_tip = meshes.add(Sphere::new(0.06));
            parent.spawn((
                Mesh3d(nose_tip),
                MeshMaterial3d(red_glow.clone()),
                Transform::from_xyz(0.0, 0.35, -1.75),
            ));

            // === ENGINE PODS (Left & Right) ===
            let engine_pod = meshes.add(Cylinder::new(0.14, 0.9));
            let engine_housing = meshes.add(Cylinder::new(0.18, 0.3));
            
            // Left engine pod
            parent.spawn((
                Mesh3d(engine_pod.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(-0.45, 0.25, 0.5)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
            // Left engine intake housing
            parent.spawn((
                Mesh3d(engine_housing.clone()),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(-0.45, 0.25, 0.0)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));

            // Right engine pod
            parent.spawn((
                Mesh3d(engine_pod),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.45, 0.25, 0.5)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
            // Right engine intake housing
            parent.spawn((
                Mesh3d(engine_housing),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(0.45, 0.25, 0.0)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));

            // === THRUSTERS (Glowing exhaust) ===
            let thruster_outer = meshes.add(Cylinder::new(0.12, 0.08));
            let thruster_inner = meshes.add(Cylinder::new(0.08, 0.12));

            // Left thruster
            parent.spawn((
                Mesh3d(thruster_outer.clone()),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(-0.45, 0.25, 1.0)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
            parent.spawn((
                Mesh3d(thruster_inner.clone()),
                MeshMaterial3d(engine_glow.clone()),
                Transform::from_xyz(-0.45, 0.25, 1.06)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));

            // Right thruster
            parent.spawn((
                Mesh3d(thruster_outer),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(0.45, 0.25, 1.0)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
            parent.spawn((
                Mesh3d(thruster_inner),
                MeshMaterial3d(engine_glow.clone()),
                Transform::from_xyz(0.45, 0.25, 1.06)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));

            // === CONTROL VANES / STEERING FINS ===
            let vane = meshes.add(Cuboid::new(0.015, 0.18, 0.35));
            
            // Left control vane
            parent.spawn((
                Mesh3d(vane.clone()),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(-0.28, 0.6, -0.7)
                    .with_rotation(Quat::from_rotation_z(0.25)),
            ));
            
            // Right control vane
            parent.spawn((
                Mesh3d(vane),
                MeshMaterial3d(accent_material.clone()),
                Transform::from_xyz(0.28, 0.6, -0.7)
                    .with_rotation(Quat::from_rotation_z(-0.25)),
            ));

            // Handlebar cross-piece
            let handlebar = meshes.add(Cylinder::new(0.02, 0.5));
            parent.spawn((
                Mesh3d(handlebar),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, 0.65, -0.6)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ));

            // === SEAT ===
            let seat = meshes.add(Cuboid::new(0.28, 0.08, 0.55));
            parent.spawn((
                Mesh3d(seat),
                MeshMaterial3d(seat_material),
                Transform::from_xyz(0.0, 0.62, 0.25),
            ));

            // Seat back rest
            let backrest = meshes.add(Cuboid::new(0.22, 0.15, 0.06));
            parent.spawn((
                Mesh3d(backrest),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, 0.7, 0.55)
                    .with_rotation(Quat::from_rotation_x(-0.2)),
            ));

            // === FOOT PEGS ===
            let foot_peg = meshes.add(Cylinder::new(0.025, 0.12));
            parent.spawn((
                Mesh3d(foot_peg.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(-0.25, 0.15, 0.1)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ));
            parent.spawn((
                Mesh3d(foot_peg),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.25, 0.15, 0.1)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ));

            // === ANTI-GRAV REPULSORS (underneath) ===
            let repulsor = meshes.add(Cylinder::new(0.1, 0.04));
            let repulsor_glow = meshes.add(Cylinder::new(0.07, 0.02));
            
            // Front repulsor
            parent.spawn((
                Mesh3d(repulsor.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, 0.08, -0.8),
            ));
            parent.spawn((
                Mesh3d(repulsor_glow.clone()),
                MeshMaterial3d(engine_glow.clone()),
                Transform::from_xyz(0.0, 0.04, -0.8),
            ));

            // Rear left repulsor
            parent.spawn((
                Mesh3d(repulsor.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(-0.3, 0.08, 0.6),
            ));
            parent.spawn((
                Mesh3d(repulsor_glow.clone()),
                MeshMaterial3d(engine_glow.clone()),
                Transform::from_xyz(-0.3, 0.04, 0.6),
            ));

            // Rear right repulsor
            parent.spawn((
                Mesh3d(repulsor),
                MeshMaterial3d(body_material),
                Transform::from_xyz(0.3, 0.08, 0.6),
            ));
            parent.spawn((
                Mesh3d(repulsor_glow),
                MeshMaterial3d(engine_glow),
                Transform::from_xyz(0.3, 0.04, 0.6),
            ));
        });
    }
}

// =============================================================================
// TRANSFORM SYNC
// =============================================================================

/// Sync vehicle transforms from replicated state with improved smoothing.
///
/// Improvements over basic dead-reckoning:
/// 1. Velocity smoothing - reduces jitter from velocity discontinuities
/// 2. Heading-aware extrapolation - predicts curved paths when turning
/// 3. Continuous background correction - spreads corrections over time
/// 4. Error clamping - prevents large desync
pub fn sync_vehicle_transforms(
    time: Res<Time>,
    mut vehicles: Query<(&VehicleState, &mut VehicleRenderSmoothing, &mut Transform), With<Vehicle>>,
) {
    let dt = time.delta_secs();

    // === CORRECTION RATES ===
    // On-snapshot correction (stronger, only when server sends new data)
    let pos_correction_rate: f32 = 25.0;
    let rot_correction_rate: f32 = 30.0;
    let t_pos = 1.0_f32 - (-pos_correction_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_correction_rate * dt).exp();

    // Background correction (always-on, very gentle drift toward server)
    let background_pos_rate: f32 = 3.0;
    let background_rot_rate: f32 = 4.0;
    let t_bg_pos = 1.0_f32 - (-background_pos_rate * dt).exp();
    let t_bg_rot = 1.0_f32 - (-background_rot_rate * dt).exp();

    // Velocity smoothing rate
    let vel_smooth_rate: f32 = 15.0;
    let t_vel = 1.0_f32 - (-vel_smooth_rate * dt).exp();

    // Maximum allowed prediction error before emergency correction
    let max_error: f32 = 1.5; // meters

    for (state, mut smooth, mut transform) in vehicles.iter_mut() {
        if !smooth.initialized {
            smooth.initialized = true;
            smooth.position = state.position;
            smooth.heading = state.heading;
            smooth.pitch = state.pitch;
            smooth.roll = state.roll;
            smooth.last_server_position = state.position;
            smooth.last_server_heading = state.heading;
            smooth.last_server_pitch = state.pitch;
            smooth.last_server_roll = state.roll;
            smooth.smoothed_velocity = state.velocity;
            smooth.smoothed_angular_yaw = state.angular_velocity_yaw;
        } else {
            // === 1. VELOCITY SMOOTHING ===
            // Smooth velocity/angular velocity to reduce jitter from discrete server updates
            smooth.smoothed_velocity = smooth.smoothed_velocity.lerp(state.velocity, t_vel);
            smooth.smoothed_angular_yaw += (state.angular_velocity_yaw - smooth.smoothed_angular_yaw) * t_vel;

            // === 2. HEADING-AWARE EXTRAPOLATION ===
            // When turning, rotate velocity by half the yaw change (midpoint approximation for curves)
            let yaw_delta = smooth.smoothed_angular_yaw * dt;
            let half_yaw_rot = Quat::from_rotation_y(-yaw_delta * 0.5);
            let curved_velocity = half_yaw_rot * smooth.smoothed_velocity;
            smooth.position += curved_velocity * dt;

            // Extrapolate angles
            smooth.heading = normalize_angle(smooth.heading + smooth.smoothed_angular_yaw * dt);
            smooth.pitch = normalize_angle(smooth.pitch + state.angular_velocity_pitch * dt);
            smooth.roll = normalize_angle(smooth.roll + state.angular_velocity_roll * dt);

            // === 3. CONTINUOUS BACKGROUND CORRECTION ===
            // Always apply a very gentle drift toward server (catches accumulated error)
            smooth.position = smooth.position.lerp(state.position, t_bg_pos);
            smooth.heading = lerp_angle(smooth.heading, state.heading, t_bg_rot);
            smooth.pitch = lerp_angle(smooth.pitch, state.pitch, t_bg_rot);
            smooth.roll = lerp_angle(smooth.roll, state.roll, t_bg_rot);

            // === 4. ON-SNAPSHOT STRONGER CORRECTION ===
            // When new server data arrives, apply stronger correction
            let server_updated = state.position != smooth.last_server_position
                || state.heading != smooth.last_server_heading
                || state.pitch != smooth.last_server_pitch
                || state.roll != smooth.last_server_roll;

            if server_updated {
                smooth.position = smooth.position.lerp(state.position, t_pos);
                smooth.heading = lerp_angle(smooth.heading, state.heading, t_rot);
                smooth.pitch = lerp_angle(smooth.pitch, state.pitch, t_rot);
                smooth.roll = lerp_angle(smooth.roll, state.roll, t_rot);

                smooth.last_server_position = state.position;
                smooth.last_server_heading = state.heading;
                smooth.last_server_pitch = state.pitch;
                smooth.last_server_roll = state.roll;
            }

            // === 5. ERROR CLAMPING (SAFETY NET) ===
            // If prediction drifted too far, snap harder to prevent obvious desync
            let error = (smooth.position - state.position).length();
            if error > max_error {
                let emergency_t = ((error - max_error) / max_error).clamp(0.0, 1.0) * 0.5;
                smooth.position = smooth.position.lerp(state.position, emergency_t);
            }
        }

        transform.translation = smooth.position;
        transform.rotation = Quat::from_euler(
            EulerRot::YXZ,
            smooth.heading,
            smooth.pitch,
            -smooth.roll,
        );
    }
}

// =============================================================================
// ANGLE HELPERS
// =============================================================================

#[allow(dead_code)]
fn angle_diff(from: f32, to: f32) -> f32 {
    let diff = to - from;
    ((diff + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)) - std::f32::consts::PI
}

fn normalize_angle(angle: f32) -> f32 {
    ((angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)) - std::f32::consts::PI
}

fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    normalize_angle(from + angle_diff(from, to) * t)
}
