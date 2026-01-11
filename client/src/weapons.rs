//! Client-side weapon systems
//!
//! Handles shooting input, local tracers for prediction, and weapon effects.
//! Updated for Lightyear 0.25 / Bevy 0.17

use bevy::prelude::*;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use lightyear::prelude::*;
use shared::{
    weapons::{ballistics, WeaponDebugMode},
    Bullet, BulletImpact, BulletImpactSurface, BulletVelocity, EquippedWeapon, HitConfirm, LocalTracer,
    LocalPlayer, PlayerPosition, ShootRequest, ReloadRequest, ReliableChannel,
};

/// Marker for the debug overlay UI
#[derive(Component)]
pub struct DebugOverlay;

/// Marker for the FPS text specifically
#[derive(Component)]
pub struct FpsText;

use crate::crosshair;
use crate::input::InputState;
use crate::states::GameState;

/// Resource to track shooting state
#[derive(Resource, Default)]
pub struct ShootingState {
    pub fire_held: bool,
    pub last_fire_time: f32,
    /// Set to true when a shot was fired this frame (for audio)
    pub shot_fired_this_frame: bool,
}

/// Component for bullet impact markers
#[derive(Component)]
pub struct ImpactMarker {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub color: Color,
}

/// Component for bullet trail history (for debug visualization)
#[derive(Component, Default)]
pub struct BulletTrail {
    pub positions: Vec<Vec3>,
}

/// Resource to store persistent bullet trails for debug mode
#[derive(Resource, Default)]
pub struct DebugBulletTrails {
    pub trails: Vec<(Vec<Vec3>, f32, Color)>, // (positions, spawn_time, color)
}

/// Handle shooting input and send requests to server
pub fn handle_shoot_input(
    mouse: Res<ButtonInput<MouseButton>>,
    mut shooting_state: ResMut<ShootingState>,
    // In Lightyear 0.25, we send messages via MessageSender component - typed on message type
    mut client_query: Query<&mut MessageSender<ShootRequest>, (With<crate::GameClient>, With<Connected>)>,
    input_state: Res<InputState>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    camera: Query<&Transform, With<Camera3d>>,
    time: Res<Time>,
    game_state: Res<State<GameState>>,
    mut last_warn_time: Local<f32>,
) {
    // Reset shot flag each frame
    shooting_state.shot_fired_this_frame = false;
    
    // Don't shoot if paused
    if game_state.get() != &GameState::Playing {
        return;
    }
    
    // Don't shoot while in vehicle
    if input_state.in_vehicle {
        return;
    }
    
    let Ok(weapon) = local_player.single() else {
        return;
    };
    
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    
    // Left click to fire
    let fire_pressed = mouse.pressed(MouseButton::Left);
    let current_time = time.elapsed_secs();
    
    // Check fire rate
    let cooldown = weapon.weapon_type.fire_cooldown();
    let can_fire = weapon.ammo_in_mag > 0 
        && (current_time - shooting_state.last_fire_time) >= cooldown;
    
    if fire_pressed && can_fire {
        shooting_state.last_fire_time = current_time;
        
        // Get aim direction from camera
        let direction = camera_transform.forward().as_vec3();
        
        // Right click = aiming
        let aiming = mouse.pressed(MouseButton::Right);
        
        // Send shoot request to server via MessageSender
        if let Ok(mut sender) = client_query.single_mut() {
            let _ = sender.send::<ReliableChannel>(ShootRequest {
                direction,
                pitch: input_state.pitch,
                aiming,
            });
        } else if current_time - *last_warn_time > 1.0 {
            // If this fires, you'll hear local SFX but the server will never spawn bullets / consume ammo.
            warn!("handle_shoot_input: missing GameClient+Connected+MessageSender<ShootRequest>; shoot requests not sent");
            *last_warn_time = current_time;
        }

        // Mark that we fired for audio system
        shooting_state.shot_fired_this_frame = true;
    }
    
    shooting_state.fire_held = fire_pressed;
}

/// Handle reload input - sends request to server
pub fn handle_reload_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut client_query: Query<&mut MessageSender<ReloadRequest>, (With<crate::GameClient>, With<Connected>)>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    input_state: Res<InputState>,
) {
    // Don't reload in vehicle
    if input_state.in_vehicle {
        return;
    }
    
    if keyboard.just_pressed(KeyCode::KeyR) {
        if let Ok(weapon) = local_player.single() {
            // Only send reload request if we actually need ammo and have reserve
            let stats = weapon.weapon_type.stats();
            if weapon.ammo_in_mag < stats.magazine_size && weapon.reserve_ammo > 0 {
                if let Ok(mut sender) = client_query.single_mut() {
                    let _ = sender.send::<ReliableChannel>(ReloadRequest);
                }
                info!("Reload requested...");
            }
        }
    }
}

/// Update local tracers (simulating bullet flight for prediction)
pub fn update_local_tracers(
    mut commands: Commands,
    mut tracers: Query<(Entity, &mut LocalTracer, &mut BulletVelocity, &mut Transform)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let current_time = time.elapsed_secs();
    
    for (entity, tracer, mut velocity, mut transform) in tracers.iter_mut() {
        // Check lifetime
        if current_time - tracer.spawn_time > tracer.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        
        // Step physics
        let (new_pos, new_vel) = ballistics::step_bullet_physics(
            transform.translation,
            velocity.0,
            dt,
        );
        
        transform.translation = new_pos;
        velocity.0 = new_vel;
        
        // Orient tracer along velocity
        if new_vel.length() > 0.1 {
            transform.look_to(new_vel.normalize(), Vec3::Y);
        }
    }
}

/// Handle replicated bullets - spawn visual representations
pub fn handle_bullet_spawned(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bullets: Query<(Entity, &Bullet, &BulletVelocity, Option<&PlayerPosition>), Added<Bullet>>,
) {
    for (entity, bullet, velocity, position) in bullets.iter() {
        let spawn_pos = position.map(|p| p.0).unwrap_or(bullet.spawn_position);

        // Spawn MUCH bigger tracer visual - a glowing elongated capsule
        let tracer_mesh = meshes.add(Capsule3d::new(0.08, 2.0)); // Bigger!
        let tracer_material = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.8, 0.2),
            emissive: LinearRgba::new(20.0, 15.0, 5.0, 1.0), // Much brighter!
            unlit: true,
            ..default()
        });
        
        // Calculate initial rotation from velocity
        let direction = velocity.0.normalize_or_zero();
        let rotation = if direction.length() > 0.1 {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };
        
        commands.entity(entity).insert((
            Mesh3d(tracer_mesh),
            MeshMaterial3d(tracer_material),
            Transform::from_translation(spawn_pos)
                .with_rotation(rotation),
            BulletTrail {
                positions: vec![spawn_pos],
            },
        ));
    }
}

/// Update bullet visuals to follow replicated positions
pub fn update_bullet_visuals(
    mut bullets: Query<(&PlayerPosition, &BulletVelocity, &mut Transform, &mut BulletTrail), With<Bullet>>,
) {
    for (pos, velocity, mut transform, mut trail) in bullets.iter_mut() {
        // IMPORTANT: move bullet based on replicated PlayerPosition (Transform itself is not replicated)
        transform.translation = pos.0;

        // Orient bullet along velocity direction
        if velocity.0.length() > 0.1 {
            let direction = velocity.0.normalize();
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, direction);
        }
        
        // Record position for trail (every few frames to avoid too many points)
        if trail.positions.is_empty() || 
           trail.positions.last().map_or(true, |last| last.distance(transform.translation) > 5.0) {
            trail.positions.push(transform.translation);
        }
    }
}

/// Handle server-authoritative bullet impacts (reliable even for very fast bullets).
/// Spawns impact markers and (when debug is ON) stores a red trajectory line.
pub fn handle_bullet_impacts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // In Lightyear 0.25, we receive messages via MessageReceiver component
    mut client_query: Query<&mut MessageReceiver<BulletImpact>, (With<crate::GameClient>, With<Connected>)>,
    time: Res<Time>,
    debug_mode: Res<WeaponDebugMode>,
    mut debug_trails: ResMut<DebugBulletTrails>,
) {
    let now = time.elapsed_secs();

    // Keep trails bounded even if debug is toggled on/off
    debug_trails
        .trails
        .retain(|(_trail, spawn_time, _color)| now - *spawn_time <= 10.0);

    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };

    for impact in receiver.receive() {
        // Choose marker visuals by surface
        let (base_color, radius) = match impact.surface {
            BulletImpactSurface::Terrain => (Color::srgba(1.0, 0.5, 0.0, 0.85), 0.35),
            BulletImpactSurface::PracticeWall => (Color::srgba(1.0, 0.0, 0.0, 0.9), 0.25),
            BulletImpactSurface::Player => (Color::srgba(1.0, 0.2, 1.0, 0.9), 0.25),
        };

        let normal = impact.impact_normal.normalize_or_zero();
        let offset = if normal.length_squared() > 0.001 { normal * 0.03 } else { Vec3::Y * 0.03 };

        // Thin "bullet hole / scorch" disk (cylinder)
        let marker_mesh = meshes.add(Cylinder::new(radius, 0.03));
        let marker_material = materials.add(StandardMaterial {
            base_color,
            emissive: LinearRgba::new(
                base_color.to_srgba().red * 2.0,
                base_color.to_srgba().green * 2.0,
                base_color.to_srgba().blue * 2.0,
                1.0,
            ),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        // Rotate so the disk is flush with the surface
        let rot = if normal.length_squared() > 0.001 {
            Quat::from_rotation_arc(Vec3::Y, normal)
        } else {
            Quat::IDENTITY
        };

        commands.spawn((
            ImpactMarker {
                spawn_time: now,
                lifetime: 6.0,
                color: base_color,
            },
            Mesh3d(marker_mesh),
            MeshMaterial3d(marker_material),
            Transform::from_translation(impact.impact_position + offset).with_rotation(rot),
        ));

        // Debug trail: simulate the ballistic path from spawn -> impact using initial velocity
        if debug_mode.0 {
            let spawn = impact.spawn_position;
            let target = impact.impact_position;
            let v0 = impact.initial_velocity;

            let dist = (target - spawn).length().max(1.0);
            let speed = v0.length().max(1.0);
            let est_time = (dist / speed).clamp(0.02, 5.0);

            let dt = 1.0 / 600.0; // higher sim rate for smooth debug lines
            let mut steps = (est_time / dt).ceil() as usize;
            steps = steps.clamp(8, 2000);

            let mut points = Vec::with_capacity(steps + 2);
            let mut pos = spawn;
            let mut vel = v0;
            points.push(pos);

            for _ in 0..steps {
                let (new_pos, new_vel) = ballistics::step_bullet_physics(pos, vel, dt);
                pos = new_pos;
                vel = new_vel;
                points.push(pos);

                // Stop early if we're very close to the impact point
                if (pos - target).length_squared() < 4.0 {
                    break;
                }
            }

            points.push(target);
            debug_trails.trails.push((points, now, Color::srgb(1.0, 0.0, 0.0)));
        }
    }
}

/// Update and cleanup impact markers
pub fn update_impact_markers(
    mut commands: Commands,
    mut markers: Query<(Entity, &ImpactMarker, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();
    
    for (entity, marker, mut transform, material_handle) in markers.iter_mut() {
        let age = current_time - marker.spawn_time;
        
        if age > marker.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        
        // Fade out and expand over time
        let t = age / marker.lifetime;
        let scale = 1.0 + t * 2.0; // Expand to 3x size
        transform.scale = Vec3::splat(scale);
        
        // Fade out the material
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let base = marker.color.to_srgba();
            material.base_color = Color::srgba(base.red, base.green, base.blue, base.alpha * (1.0 - t));
        }
    }
}

/// Handle hit confirmations from server
pub fn handle_hit_confirms(
    mut commands: Commands,
    mut client_query: Query<&mut MessageReceiver<HitConfirm>, (With<crate::GameClient>, With<Connected>)>,
    time: Res<Time>,
) {
    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };

    for confirm in receiver.receive() {
        info!(
            "Hit confirmed! Damage: {:.1}, Headshot: {}, Kill: {}",
            confirm.damage, confirm.headshot, confirm.kill
        );
        
        // Spawn hit marker
        crosshair::spawn_hit_marker(&mut commands, &time, confirm.kill);
    }
}

/// Debug: Draw bullet trajectories
/// Uses thick lines to be visible with HDR/bloom
pub fn debug_draw_trajectories(
    bullets: Query<(&Bullet, &Transform, &BulletTrail)>,
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    debug_trails: Res<DebugBulletTrails>,
    time: Res<Time>,
) {
    if !debug_mode.0 {
        return;
    }

    let now = time.elapsed_secs();
    
    // Draw live bullet trails (replicated) - bright green
    for (bullet, transform, trail) in bullets.iter() {
        // Line from spawn to current position - thick bright line
        gizmos.line(
            bullet.spawn_position,
            transform.translation,
            Color::srgb(0.2, 1.0, 0.2),  // Bright green
        );
        
        // Draw trail points
        for window in trail.positions.windows(2) {
            gizmos.line(window[0], window[1], Color::srgba(0.2, 1.0, 0.2, 0.7));
        }
        
        // Draw sphere at spawn position for visibility
        gizmos.sphere(
            Isometry3d::from_translation(bullet.spawn_position),
            0.15,
            Color::srgb(0.0, 1.0, 0.0),
        );
    }

    // Draw persistent debug trails with fading - bright red/orange
    for (trail, spawn_time, _base_color) in debug_trails.trails.iter() {
        let age = now - spawn_time;
        let alpha = (1.0 - age / 10.0).clamp(0.0, 1.0);

        // Use bright red/orange for better visibility with HDR
        let color = Color::srgba(1.0, 0.3, 0.0, alpha);

        for window in trail.windows(2) {
            gizmos.line(window[0], window[1], color);
        }
        
        // Draw sphere at start of trail
        if let Some(first) = trail.first() {
            gizmos.sphere(
                Isometry3d::from_translation(*first),
                0.1,
                Color::srgba(1.0, 0.0, 0.0, alpha),
            );
        }
    }
}

/// Toggle debug mode with F3
pub fn toggle_debug_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut debug_mode: ResMut<WeaponDebugMode>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        debug_mode.0 = !debug_mode.0;
        info!("Weapon debug mode: {}", if debug_mode.0 { "ON" } else { "OFF" });
    }
}

// =============================================================================
// DEBUG OVERLAY (FPS counter, etc.)
// =============================================================================

/// Spawn the debug overlay UI (hidden by default)
pub fn spawn_debug_overlay(mut commands: Commands) {
    // Root container - top-left corner
    commands
        .spawn((
            DebugOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            BorderRadius::all(Val::Px(4.0)),
            Visibility::Hidden, // Hidden until debug mode is enabled
        ))
        .with_children(|parent| {
            // FPS text
            parent.spawn((
                FpsText,
                Text::new("FPS: --"),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgb(0.0, 1.0, 0.0)), // Green text
            ));
            
            // Debug mode indicator
            parent.spawn((
                Text::new("[F3] Debug Mode ON"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ));
        });
}

/// Update the debug overlay - show/hide based on debug mode, update FPS
pub fn update_debug_overlay(
    debug_mode: Res<WeaponDebugMode>,
    diagnostics: Res<DiagnosticsStore>,
    mut overlay_query: Query<&mut Visibility, With<DebugOverlay>>,
    mut fps_text_query: Query<(&mut Text, &mut TextColor), With<FpsText>>,
) {
    // Show/hide overlay based on debug mode
    for mut visibility in overlay_query.iter_mut() {
        *visibility = if debug_mode.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    
    // Update FPS text (only if visible)
    if debug_mode.0 {
        if let Some(fps_diagnostic) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps) = fps_diagnostic.smoothed() {
                for (mut text, mut color) in fps_text_query.iter_mut() {
                    text.0 = format!("FPS: {:.0}", fps);
                    
                    // Color code based on FPS performance
                    *color = if fps >= 55.0 {
                        TextColor(Color::srgb(0.2, 1.0, 0.2))  // Green - good
                    } else if fps >= 30.0 {
                        TextColor(Color::srgb(1.0, 0.8, 0.0))  // Yellow - okay
                    } else {
                        TextColor(Color::srgb(1.0, 0.2, 0.2))  // Red - bad
                    };
                }
            }
        }
    }
}

/// Despawn the debug overlay
pub fn despawn_debug_overlay(
    mut commands: Commands,
    overlay_query: Query<Entity, With<DebugOverlay>>,
) {
    for entity in overlay_query.iter() {
        commands.entity(entity).despawn();
    }
}
